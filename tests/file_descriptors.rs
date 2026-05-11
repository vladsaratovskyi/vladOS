#![no_std]
#![no_main]

use core::panic::PanicInfo;

use bootloader::{entry_point, BootInfo};
use vlad_os::memory::BootInfoFrameAllocator;
use vlad_os::qemu::{exit_qemu, QemuExitCode};
use vlad_os::{
    allocator, gdt, hlt_loop, interrupts, memory, scheduler, serial, serial_print, serial_println,
    user,
};
use x86_64::VirtAddr;

const FD_SYSCALL_SUITE_ELF: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/fd_syscall_suite.elf"));
const FD_FIRST_OPEN_EXIT_ELF: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/fd_first_open_exit.elf"));
const FD_OPEN_LEAK_EXIT_ELF: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/fd_open_leak_exit.elf"));
const BUSY_COUNTER_ELF: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/busy_counter.elf"));

entry_point!(test_kernel_main);

fn test_kernel_main(boot_info: &'static BootInfo) -> ! {
    vlad_os::serial::init();
    serial_print!("file_descriptors::embedded_file_io...\t");

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

    panic!("file descriptor scheduler returned without test success");
}

fn orchestrator() {
    file_syscall_suite();
    per_process_fd_tables_are_private();
    process_exit_closes_descriptors();
    preemption_still_works_across_file_syscalls();

    serial_println!("[ok]");
    exit_qemu(QemuExitCode::Success);
    hlt_loop();
}

fn file_syscall_suite() {
    serial::clear_output_buffer();
    assert_eq!(scheduler::open_file_count(), 0);

    let process = scheduler::spawn_user_elf_process("fd_suite", FD_SYSCALL_SUITE_ELF)
        .expect("failed to spawn fd suite");

    assert!(scheduler::process_fd_is_open(process.pid, 0));
    assert!(scheduler::process_fd_is_open(process.pid, 1));
    assert!(scheduler::process_fd_is_open(process.pid, 2));
    assert_eq!(scheduler::process_open_fd_count(process.pid), 3);

    scheduler::yield_now();

    assert_eq!(scheduler::task_exit_code(process.task_id), Some(0));
    assert_eq!(scheduler::process_open_fd_count(process.pid), 0);
    assert_eq!(scheduler::open_file_count(), 0);
    assert!(serial::output_contains(b"hello from embedded file\n"));
    assert!(serial::output_contains(b"tiny kernel says hello\n"));
}

fn per_process_fd_tables_are_private() {
    let a = scheduler::spawn_user_elf_process("fd_first_a", FD_FIRST_OPEN_EXIT_ELF)
        .expect("failed to spawn first fd process");
    let b = scheduler::spawn_user_elf_process("fd_first_b", FD_FIRST_OPEN_EXIT_ELF)
        .expect("failed to spawn second fd process");

    assert_eq!(scheduler::process_open_fd_count(a.pid), 3);
    assert_eq!(scheduler::process_open_fd_count(b.pid), 3);

    scheduler::yield_now();

    assert_eq!(scheduler::task_exit_code(a.task_id), Some(0));
    assert_eq!(scheduler::task_exit_code(b.task_id), Some(0));
    assert_eq!(scheduler::process_open_fd_count(a.pid), 0);
    assert_eq!(scheduler::process_open_fd_count(b.pid), 0);
    assert_eq!(scheduler::open_file_count(), 0);
}

fn process_exit_closes_descriptors() {
    let process = scheduler::spawn_user_elf_process("fd_leak", FD_OPEN_LEAK_EXIT_ELF)
        .expect("failed to spawn fd leak process");

    scheduler::yield_now();

    assert_eq!(scheduler::task_exit_code(process.task_id), Some(0));
    assert_eq!(scheduler::process_open_fd_count(process.pid), 0);
    assert_eq!(scheduler::open_file_count(), 0);
}

fn preemption_still_works_across_file_syscalls() {
    serial::clear_output_buffer();

    let busy = scheduler::spawn_user_elf("busy_counter", BUSY_COUNTER_ELF)
        .expect("failed to spawn busy counter");
    let file_task = scheduler::spawn_user_elf("fd_preempt", FD_SYSCALL_SUITE_ELF)
        .expect("failed to spawn preempted fd suite");

    let start_ticks = interrupts::timer_ticks();
    scheduler::enable_preemption();
    scheduler::yield_now();
    scheduler::disable_preemption();

    assert!(interrupts::timer_ticks() > start_ticks);
    assert!(scheduler::read_user_u64(busy, VirtAddr::new(user::USER_DATA_BASE)).unwrap_or(0) > 0);
    assert_eq!(scheduler::task_exit_code(file_task), Some(0));
    assert!(serial::output_contains(b"hello from embedded file\n"));
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    vlad_os::qemu::test_panic_handler(info);
}
