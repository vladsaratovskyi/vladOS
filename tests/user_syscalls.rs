#![no_std]
#![no_main]

use core::panic::PanicInfo;

use bootloader::{entry_point, BootInfo};
use vlad_os::memory::BootInfoFrameAllocator;
use vlad_os::qemu::{exit_qemu, QemuExitCode};
use vlad_os::task::UserFaultKind;
use vlad_os::user_memory::UserMemoryError;
use vlad_os::{
    allocator, gdt, hlt_loop, interrupts, memory, scheduler, serial, serial_print, serial_println,
    user,
};
use x86_64::VirtAddr;

const WRITE_SYSCALL_SUITE_ELF: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/write_syscall_suite.elf"));
const READ_DATA_EXIT_ELF: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/read_data_exit.elf"));
const WRITE_READONLY_SEGMENT_ELF: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/write_readonly_segment.elf"));
const BUSY_COUNTER_ELF: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/busy_counter.elf"));
const WRITE_HELLO_ELF: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/write_hello.elf"));

entry_point!(test_kernel_main);

fn test_kernel_main(boot_info: &'static BootInfo) -> ! {
    vlad_os::serial::init();
    serial_print!("user_syscalls::write_and_user_memory...\t");

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

    panic!("user syscall scheduler returned without test success");
}

fn orchestrator() {
    write_syscall_suite();
    copy_to_user_accepts_writable_page();
    direct_readonly_write_still_faults();
    preemption_still_works_with_write_syscalls();

    serial_println!("[ok]");
    exit_qemu(QemuExitCode::Success);
    hlt_loop();
}

fn write_syscall_suite() {
    serial::clear_output_buffer();

    let task = scheduler::spawn_user_elf("write_syscall_suite", WRITE_SYSCALL_SUITE_ELF)
        .expect("failed to spawn write syscall suite");
    assert_eq!(
        scheduler::copy_to_user(task, VirtAddr::new(user::USER_DATA_BASE), b"x"),
        Err(UserMemoryError::NotWritable)
    );

    scheduler::yield_now();

    assert_eq!(scheduler::task_exit_code(task), Some(0));
    assert_eq!(scheduler::task_fault_info(task), None);
    assert!(serial::output_contains(b"hello from user write\n"));
    assert!(serial::output_contains(b"stderr from user write\n"));
    assert!(serial::output_contains(b"cross-page user write ok\n"));
}

fn copy_to_user_accepts_writable_page() {
    let task = scheduler::spawn_user_elf("read_data_exit", READ_DATA_EXIT_ELF)
        .expect("failed to spawn read-data task");

    scheduler::copy_to_user(
        task,
        VirtAddr::new(user::USER_DATA_BASE),
        &0x55_u64.to_le_bytes(),
    )
    .expect("copy_to_user rejected writable page");

    scheduler::yield_now();

    assert_eq!(scheduler::task_exit_code(task), Some(0x55));
}

fn direct_readonly_write_still_faults() {
    let task = scheduler::spawn_user_elf("direct_readonly_fault", WRITE_READONLY_SEGMENT_ELF)
        .expect("failed to spawn direct-fault task");

    scheduler::yield_now();

    let fault = scheduler::task_fault_info(task).expect("direct readonly write did not fault");
    assert_eq!(fault.kind, UserFaultKind::PageFault);
    assert_eq!(fault.address, Some(VirtAddr::new(user::USER_DATA_BASE)));
    assert_eq!(scheduler::task_exit_code(task), None);
}

fn preemption_still_works_with_write_syscalls() {
    serial::clear_output_buffer();

    let busy = scheduler::spawn_user_elf("busy_counter", BUSY_COUNTER_ELF)
        .expect("failed to spawn busy task");
    let writer =
        scheduler::spawn_user_elf("write_hello", WRITE_HELLO_ELF).expect("failed to spawn writer");

    let start_ticks = interrupts::timer_ticks();
    scheduler::enable_preemption();
    scheduler::yield_now();
    scheduler::disable_preemption();

    assert!(interrupts::timer_ticks() > start_ticks);
    assert!(scheduler::read_user_u64(busy, VirtAddr::new(user::USER_DATA_BASE)).unwrap_or(0) > 0);
    assert_eq!(scheduler::task_exit_code(writer), Some(0));
    assert!(serial::output_contains(b"preempted write hello\n"));
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    vlad_os::qemu::test_panic_handler(info);
}
