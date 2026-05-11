#![no_std]
#![no_main]

use core::panic::PanicInfo;

use bootloader::{entry_point, BootInfo};
use vlad_os::memory::BootInfoFrameAllocator;
use vlad_os::qemu::{exit_qemu, QemuExitCode};
use vlad_os::task::UserFaultKind;
use vlad_os::{
    allocator, gdt, hlt_loop, interrupts, memory, scheduler, serial_print, serial_println, user,
};
use x86_64::VirtAddr;

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
    memory::init_global(physical_memory_offset, frame_allocator);

    interrupts::enable_interrupts();

    scheduler::spawn(orchestrator).expect("failed to spawn orchestrator");
    scheduler::run();

    panic!("userspace scheduler returned without test success");
}

fn orchestrator() {
    let yield_task = user::create_user_task(user::UserProgram::YieldThenExit, user::USER_DATA_BASE)
        .expect("failed to create yield/exit user task");
    let yield_task_id = scheduler::spawn_user(yield_task).expect("failed to spawn user task");

    scheduler::yield_now();
    assert_eq!(
        scheduler::read_user_u64(yield_task_id, VirtAddr::new(user::USER_DATA_BASE)),
        Some(1)
    );
    assert_eq!(
        scheduler::read_user_u64(yield_task_id, VirtAddr::new(user::USER_DATA_BASE + 8)),
        Some(0)
    );

    scheduler::yield_now();
    assert_eq!(
        scheduler::read_user_u64(yield_task_id, VirtAddr::new(user::USER_DATA_BASE + 8)),
        Some(1)
    );
    assert_eq!(scheduler::task_exit_code(yield_task_id), Some(0));

    let fault_task = user::create_user_task(user::UserProgram::PrivilegedHlt, user::USER_DATA_BASE)
        .expect("failed to create faulting user task");
    let fault_task_id = scheduler::spawn_user(fault_task).expect("failed to spawn fault task");

    scheduler::yield_now();
    assert_eq!(
        scheduler::read_user_u64(fault_task_id, VirtAddr::new(user::USER_DATA_BASE + 16)),
        Some(1)
    );
    assert_eq!(
        scheduler::read_user_u64(fault_task_id, VirtAddr::new(user::USER_DATA_BASE + 24)),
        Some(0)
    );
    assert_eq!(
        scheduler::task_fault_info(fault_task_id).map(|info| info.kind),
        Some(UserFaultKind::GeneralProtection)
    );

    let busy_task = user::create_user_task(user::UserProgram::BusyCounter, user::USER_DATA_BASE)
        .expect("failed to create busy user task");
    let busy_task_id = scheduler::spawn_user(busy_task).expect("failed to spawn busy task");

    let start_ticks = interrupts::timer_ticks();
    scheduler::enable_preemption();
    scheduler::yield_now();
    scheduler::disable_preemption();

    assert!(interrupts::timer_ticks() > start_ticks);
    assert!(
        scheduler::read_user_u64(
            busy_task_id,
            VirtAddr::new(user::USER_DATA_BASE + user::USER_MARKER_BUSY_COUNT as u64 * 8)
        )
        .unwrap_or(0)
            > 0
    );

    serial_println!("[ok]");
    exit_qemu(QemuExitCode::Success);
    hlt_loop();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    vlad_os::qemu::test_panic_handler(info);
}
