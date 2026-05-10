#![no_std]
#![no_main]

use core::panic::PanicInfo;
use core::sync::atomic::{AtomicU64, Ordering};

use bootloader::{entry_point, BootInfo};
use vlad_os::memory::BootInfoFrameAllocator;
use vlad_os::qemu::{exit_qemu, QemuExitCode};
use vlad_os::{
    allocator, gdt, hlt_loop, interrupts, memory, scheduler, serial_print, serial_println, user,
};
use x86_64::VirtAddr;

static YIELD_EXIT_ENTRY: AtomicU64 = AtomicU64::new(0);
static FAULT_ENTRY: AtomicU64 = AtomicU64::new(0);
static BUSY_ENTRY: AtomicU64 = AtomicU64::new(0);
static USER_STACK_0: AtomicU64 = AtomicU64::new(0);
static USER_STACK_1: AtomicU64 = AtomicU64::new(0);
static USER_STACK_2: AtomicU64 = AtomicU64::new(0);
static MARKER_PAGE: AtomicU64 = AtomicU64::new(0);

entry_point!(test_kernel_main);

fn test_kernel_main(boot_info: &'static BootInfo) -> ! {
    vlad_os::serial::init();
    serial_print!("userspace::ring3_syscalls_and_faults...\t");

    gdt::init();
    interrupts::init_idt();
    interrupts::init_pics();
    interrupts::init_pit();

    let physical_memory_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(physical_memory_offset) };
    let mut frame_allocator = unsafe { BootInfoFrameAllocator::init(&boot_info.memory_map) };

    allocator::init_heap(&mut mapper, &mut frame_allocator).expect("failed to initialize heap");

    let marker_page =
        user::map_user_marker_page(&mut mapper, &mut frame_allocator).expect("marker map failed");
    let yield_exit = user::map_user_program(
        &mut mapper,
        &mut frame_allocator,
        user::UserProgram::YieldThenExit,
    )
    .expect("yield/exit user code map failed");
    let fault = user::map_user_program(
        &mut mapper,
        &mut frame_allocator,
        user::UserProgram::PrivilegedHlt,
    )
    .expect("fault user code map failed");
    let busy = user::map_user_program(
        &mut mapper,
        &mut frame_allocator,
        user::UserProgram::BusyCounter,
    )
    .expect("busy user code map failed");

    let stack_0 =
        user::map_user_stack(&mut mapper, &mut frame_allocator, 0).expect("user stack 0 failed");
    let stack_1 =
        user::map_user_stack(&mut mapper, &mut frame_allocator, 1).expect("user stack 1 failed");
    let stack_2 =
        user::map_user_stack(&mut mapper, &mut frame_allocator, 2).expect("user stack 2 failed");

    MARKER_PAGE.store(marker_page.as_u64(), Ordering::SeqCst);
    YIELD_EXIT_ENTRY.store(yield_exit.as_u64(), Ordering::SeqCst);
    FAULT_ENTRY.store(fault.as_u64(), Ordering::SeqCst);
    BUSY_ENTRY.store(busy.as_u64(), Ordering::SeqCst);
    USER_STACK_0.store(stack_0.as_u64(), Ordering::SeqCst);
    USER_STACK_1.store(stack_1.as_u64(), Ordering::SeqCst);
    USER_STACK_2.store(stack_2.as_u64(), Ordering::SeqCst);

    interrupts::enable_interrupts();

    scheduler::spawn(orchestrator).expect("failed to spawn orchestrator");
    scheduler::spawn_user(yield_exit, stack_0, marker_page.as_u64())
        .expect("failed to spawn yield/exit user task");
    scheduler::run();

    panic!("userspace scheduler returned without test success");
}

fn orchestrator() {
    scheduler::yield_now();
    assert_eq!(user::marker_value(user::USER_MARKER_RAN), 1);
    assert_eq!(user::marker_value(user::USER_MARKER_AFTER_YIELD), 0);

    scheduler::yield_now();
    assert_eq!(user::marker_value(user::USER_MARKER_AFTER_YIELD), 1);
    assert!(scheduler::finished_task_count() >= 1);

    scheduler::spawn_user(
        VirtAddr::new(FAULT_ENTRY.load(Ordering::SeqCst)),
        VirtAddr::new(USER_STACK_1.load(Ordering::SeqCst)),
        MARKER_PAGE.load(Ordering::SeqCst),
    )
    .expect("failed to spawn faulting user task");

    scheduler::yield_now();
    assert_eq!(user::marker_value(user::USER_MARKER_BEFORE_FAULT), 1);
    assert_eq!(user::marker_value(user::USER_MARKER_AFTER_FAULT), 0);
    assert!(scheduler::failed_task_count() >= 1);

    scheduler::spawn_user(
        VirtAddr::new(BUSY_ENTRY.load(Ordering::SeqCst)),
        VirtAddr::new(USER_STACK_2.load(Ordering::SeqCst)),
        MARKER_PAGE.load(Ordering::SeqCst),
    )
    .expect("failed to spawn busy user task");

    let start_ticks = interrupts::timer_ticks();
    scheduler::enable_preemption();
    scheduler::yield_now();
    scheduler::disable_preemption();

    assert!(interrupts::timer_ticks() > start_ticks);
    assert!(user::marker_value(user::USER_MARKER_BUSY_COUNT) > 0);

    serial_println!("[ok]");
    exit_qemu(QemuExitCode::Success);
    hlt_loop();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    vlad_os::qemu::test_panic_handler(info);
}
