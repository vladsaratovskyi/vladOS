#![no_std]
#![no_main]

use core::panic::PanicInfo;

use bootloader::{entry_point, BootInfo};
use vlad_os::elf::{self, ElfLoadError};
use vlad_os::memory::BootInfoFrameAllocator;
use vlad_os::qemu::{exit_qemu, QemuExitCode};
use vlad_os::task::UserFaultKind;
use vlad_os::{
    allocator, gdt, hlt_loop, interrupts, memory, scheduler, serial_print, serial_println, user,
};
use x86_64::VirtAddr;

const EXIT_42_ELF: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/exit_42.elf"));
const WRITE_PRIVATE_DATA_ELF: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/write_private_data.elf"));
const WRITE_READONLY_SEGMENT_ELF: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/write_readonly_segment.elf"));
const BUSY_COUNTER_ELF: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/busy_counter.elf"));
const BAD_MACHINE_ELF: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/bad_machine.elf"));
const BAD_MAGIC_ELF: &[u8] = b"not an elf";

entry_point!(test_kernel_main);

fn test_kernel_main(boot_info: &'static BootInfo) -> ! {
    vlad_os::serial::init();
    serial_print!("elf_loader::embedded_user_elfs...\t");

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

    panic!("ELF loader scheduler returned without test success");
}

fn orchestrator() {
    elf_rejects_bad_magic();
    elf_rejects_bad_machine();
    elf_exit_code();
    elf_private_address_spaces();
    elf_readonly_segment_fault();
    timer_preemption_across_elf_processes();

    serial_println!("[ok]");
    exit_qemu(QemuExitCode::Success);
    hlt_loop();
}

fn elf_rejects_bad_magic() {
    assert!(matches!(
        elf::load_user_elf(BAD_MAGIC_ELF, 0),
        Err(ElfLoadError::BadMagic)
    ));
}

fn elf_rejects_bad_machine() {
    assert!(matches!(
        elf::load_user_elf(BAD_MACHINE_ELF, 0),
        Err(ElfLoadError::UnsupportedMachine)
    ));
}

fn elf_exit_code() {
    let task = scheduler::spawn_user_elf("exit_42", EXIT_42_ELF).expect("failed to spawn exit_42");

    scheduler::yield_now();

    assert_eq!(scheduler::task_exit_code(task), Some(42));
}

fn elf_private_address_spaces() {
    let task_a = scheduler::spawn_user_elf_with_arg("private_a", WRITE_PRIVATE_DATA_ELF, 0x11)
        .expect("failed to spawn private A");
    let task_b = scheduler::spawn_user_elf_with_arg("private_b", WRITE_PRIVATE_DATA_ELF, 0x22)
        .expect("failed to spawn private B");

    scheduler::yield_now();

    assert_eq!(scheduler::task_exit_code(task_a), Some(0x11));
    assert_eq!(scheduler::task_exit_code(task_b), Some(0x22));
    assert_eq!(
        scheduler::read_user_u64(task_a, VirtAddr::new(user::USER_DATA_BASE)),
        Some(0x11)
    );
    assert_eq!(
        scheduler::read_user_u64(task_b, VirtAddr::new(user::USER_DATA_BASE)),
        Some(0x22)
    );
}

fn elf_readonly_segment_fault() {
    let task = scheduler::spawn_user_elf("readonly_fault", WRITE_READONLY_SEGMENT_ELF)
        .expect("failed to spawn readonly fault task");

    scheduler::yield_now();

    let fault = scheduler::task_fault_info(task).expect("readonly task did not fault");
    assert_eq!(fault.kind, UserFaultKind::PageFault);
    assert_eq!(fault.address, Some(VirtAddr::new(user::USER_DATA_BASE)));
    assert_eq!(scheduler::task_exit_code(task), None);
}

fn timer_preemption_across_elf_processes() {
    let busy = scheduler::spawn_user_elf("busy_counter", BUSY_COUNTER_ELF)
        .expect("failed to spawn busy ELF");
    let exit = scheduler::spawn_user_elf("exit_42_again", EXIT_42_ELF)
        .expect("failed to spawn second ELF");

    let start_ticks = interrupts::timer_ticks();
    scheduler::enable_preemption();
    scheduler::yield_now();
    scheduler::disable_preemption();

    assert!(interrupts::timer_ticks() > start_ticks);
    assert!(scheduler::read_user_u64(busy, VirtAddr::new(user::USER_DATA_BASE)).unwrap_or(0) > 0);
    assert_eq!(scheduler::task_exit_code(exit), Some(42));
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    vlad_os::qemu::test_panic_handler(info);
}
