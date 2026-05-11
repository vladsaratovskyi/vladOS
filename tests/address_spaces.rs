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
    serial_print!("address_spaces::isolation_and_faults...\t");

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

    panic!("address-space scheduler returned without test success");
}

fn orchestrator() {
    same_virtual_address_is_private();
    unmapped_user_page_is_task_local();
    kernel_mapping_is_supervisor_only();
    preemption_crosses_cr3_roots();

    serial_println!("[ok]");
    exit_qemu(QemuExitCode::Success);
    hlt_loop();
}

fn same_virtual_address_is_private() {
    let task_a = user::create_user_task(user::UserProgram::WriteReadAa, user::USER_DATA_BASE)
        .expect("failed to create task A");
    let task_b = user::create_user_task(user::UserProgram::WriteReadBb, user::USER_DATA_BASE)
        .expect("failed to create task B");
    let task_a = scheduler::spawn_user(task_a).expect("failed to spawn task A");
    let task_b = scheduler::spawn_user(task_b).expect("failed to spawn task B");

    scheduler::yield_now();
    assert_eq!(
        scheduler::read_user_u64(task_a, VirtAddr::new(user::USER_DATA_BASE)),
        Some(0xaa)
    );
    assert_eq!(
        scheduler::read_user_u64(task_b, VirtAddr::new(user::USER_DATA_BASE)),
        Some(0xbb)
    );

    scheduler::yield_now();
    assert_eq!(scheduler::task_exit_code(task_a), Some(0xaa));
    assert_eq!(scheduler::task_exit_code(task_b), Some(0xbb));
}

fn unmapped_user_page_is_task_local() {
    let task_a = user::create_user_task(user::UserProgram::ReadArgExit, user::USER_TEST_PAGE_BASE)
        .expect("failed to create unmapped reader");
    let task_b = user::create_user_task_with_test_page(
        user::UserProgram::ReadArgExit,
        user::USER_TEST_PAGE_BASE,
        Some(0xcc),
    )
    .expect("failed to create mapped reader");
    let task_a = scheduler::spawn_user(task_a).expect("failed to spawn unmapped reader");
    let task_b = scheduler::spawn_user(task_b).expect("failed to spawn mapped reader");

    scheduler::yield_now();

    let fault = scheduler::task_fault_info(task_a).expect("task A did not fault");
    assert_eq!(fault.kind, UserFaultKind::PageFault);
    assert_eq!(
        fault.address,
        Some(VirtAddr::new(user::USER_TEST_PAGE_BASE))
    );
    assert_eq!(scheduler::task_exit_code(task_b), Some(0xcc));
}

fn kernel_mapping_is_supervisor_only() {
    let kernel_address = orchestrator as *const () as u64;
    let task = user::create_user_task(user::UserProgram::ReadArgExit, kernel_address)
        .expect("failed to create kernel reader");
    let task = scheduler::spawn_user(task).expect("failed to spawn kernel reader");

    scheduler::yield_now();

    let fault = scheduler::task_fault_info(task).expect("kernel reader did not fault");
    assert_eq!(fault.kind, UserFaultKind::PageFault);
    assert_eq!(fault.address, Some(VirtAddr::new(kernel_address)));
}

fn preemption_crosses_cr3_roots() {
    let busy = user::create_user_task(user::UserProgram::BusyCounter, user::USER_DATA_BASE)
        .expect("failed to create busy task");
    let second = user::create_user_task(user::UserProgram::WriteReadAa, user::USER_DATA_BASE)
        .expect("failed to create second task");
    let busy = scheduler::spawn_user(busy).expect("failed to spawn busy task");
    let second = scheduler::spawn_user(second).expect("failed to spawn second task");

    let start_ticks = interrupts::timer_ticks();
    scheduler::enable_preemption();
    scheduler::yield_now();
    scheduler::disable_preemption();

    assert!(interrupts::timer_ticks() > start_ticks);
    assert!(
        scheduler::read_user_u64(
            busy,
            VirtAddr::new(user::USER_DATA_BASE + user::USER_MARKER_BUSY_COUNT as u64 * 8)
        )
        .unwrap_or(0)
            > 0
    );
    assert_eq!(
        scheduler::read_user_u64(second, VirtAddr::new(user::USER_DATA_BASE)),
        Some(0xaa)
    );
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    vlad_os::qemu::test_panic_handler(info);
}
