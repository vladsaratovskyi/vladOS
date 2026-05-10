#![no_std]
#![no_main]

use core::panic::PanicInfo;
use core::sync::atomic::{AtomicUsize, Ordering};

use bootloader::{entry_point, BootInfo};
use vlad_os::memory::BootInfoFrameAllocator;
use vlad_os::qemu::{exit_qemu, QemuExitCode};
use vlad_os::{
    allocator, gdt, hlt_loop, interrupts, memory, scheduler, serial_print, serial_println,
};
use x86_64::VirtAddr;

static STEP: AtomicUsize = AtomicUsize::new(0);
static COMPLETED_TASKS: AtomicUsize = AtomicUsize::new(0);

entry_point!(test_kernel_main);

fn test_kernel_main(boot_info: &'static BootInfo) -> ! {
    vlad_os::serial::init();
    serial_print!("cooperative_tasks::round_robin_yield...\t");

    gdt::init();
    interrupts::init_idt();

    let physical_memory_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(physical_memory_offset) };
    let mut frame_allocator = unsafe { BootInfoFrameAllocator::init(&boot_info.memory_map) };

    allocator::init_heap(&mut mapper, &mut frame_allocator).expect("failed to initialize heap");

    scheduler::spawn(task_a).expect("failed to spawn task A");
    scheduler::spawn(task_b).expect("failed to spawn task B");
    scheduler::run();

    assert_eq!(STEP.load(Ordering::SeqCst), 6);
    assert_eq!(COMPLETED_TASKS.load(Ordering::SeqCst), 2);
    assert_eq!(scheduler::finished_task_count(), 2);
    assert!(scheduler::all_tasks_finished());

    serial_println!("[ok]");
    exit_qemu(QemuExitCode::Success);
    hlt_loop();
}

fn task_a() {
    let mut local_state = 10usize;

    expect_step(0);
    local_state += 1;
    scheduler::yield_now();

    assert_eq!(local_state, 11);
    expect_step(2);
    local_state += 1;
    scheduler::yield_now();

    assert_eq!(local_state, 12);
    expect_step(4);
    COMPLETED_TASKS.fetch_add(1, Ordering::SeqCst);
}

fn task_b() {
    let mut local_state = 20usize;

    expect_step(1);
    local_state += 2;
    scheduler::yield_now();

    assert_eq!(local_state, 22);
    expect_step(3);
    local_state += 2;
    scheduler::yield_now();

    assert_eq!(local_state, 24);
    expect_step(5);
    COMPLETED_TASKS.fetch_add(1, Ordering::SeqCst);
}

fn expect_step(expected: usize) {
    assert_eq!(
        STEP.compare_exchange(expected, expected + 1, Ordering::SeqCst, Ordering::SeqCst),
        Ok(expected)
    );
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    vlad_os::qemu::test_panic_handler(info);
}
