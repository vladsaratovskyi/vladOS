#![no_std]
#![no_main]

use core::panic::PanicInfo;

use bootloader::{entry_point, BootInfo};
use vlad_os::memory::BootInfoFrameAllocator;
use vlad_os::process::{ProcessExit, ProcessState};
use vlad_os::qemu::{exit_qemu, QemuExitCode};
use vlad_os::task::UserFaultKind;
use vlad_os::{
    allocator, gdt, hlt_loop, interrupts, memory, scheduler, serial, serial_print, serial_println,
    user,
};
use x86_64::VirtAddr;

const PAGE_SIZE: u64 = 4096;

const BRK_QUERY_INVALID_SUITE_ELF: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/brk_query_invalid_suite.elf"));
const BRK_GROWTH_SUITE_ELF: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/brk_growth_suite.elf"));
const BRK_SHRINK_FAULT_ELF: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/brk_shrink_fault.elf"));
const BRK_SHRINK_CONTINUE_ELF: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/brk_shrink_continue.elf"));
const BRK_PRIVATE_WRITER_ELF: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/brk_private_writer.elf"));
const BRK_BUSY_COUNTER_ELF: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/brk_busy_counter.elf"));

entry_point!(test_kernel_main);

fn test_kernel_main(boot_info: &'static BootInfo) -> ! {
    vlad_os::serial::init();
    serial_print!("user_heap::brk_growth_and_isolation...\t");

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

    panic!("user heap scheduler returned without test success");
}

fn orchestrator() {
    heap_metadata_and_invalid_requests();
    growth_zeroing_and_write();
    shrink_unmaps_whole_pages();
    shrink_preserves_lower_heap();
    same_virtual_heap_address_is_private_per_process();
    preemption_still_works_across_brk();

    serial_println!("[ok]");
    exit_qemu(QemuExitCode::Success);
    hlt_loop();
}

fn heap_metadata_and_invalid_requests() {
    let process = scheduler::spawn_user_elf_process("brk_query", BRK_QUERY_INVALID_SUITE_ELF)
        .expect("failed to spawn brk query suite");
    let start = scheduler::process_heap_start(process.pid).expect("missing heap start");
    let brk = scheduler::process_program_break(process.pid).expect("missing program break");
    let mapped_end =
        scheduler::process_heap_mapped_end(process.pid).expect("missing mapped heap end");
    let limit = scheduler::process_heap_limit(process.pid).expect("missing heap limit");

    assert_eq!(brk, start);
    assert_eq!(mapped_end, start);
    assert_eq!(limit, VirtAddr::new(user::USER_HEAP_LIMIT));
    assert_eq!(start.as_u64() & (PAGE_SIZE - 1), 0);
    assert!(start.as_u64() < limit.as_u64());
    assert!(!scheduler::user_page_is_mapped(process.pid, start));

    scheduler::yield_now();

    assert_eq!(scheduler::task_exit_code(process.task_id), Some(0));
    assert_eq!(scheduler::process_program_break(process.pid), Some(start));
    assert_eq!(scheduler::process_heap_mapped_end(process.pid), Some(start));
}

fn growth_zeroing_and_write() {
    serial::clear_output_buffer();

    let process = scheduler::spawn_user_elf_process("brk_growth", BRK_GROWTH_SUITE_ELF)
        .expect("failed to spawn brk growth suite");
    let start = scheduler::process_heap_start(process.pid).expect("missing heap start");

    scheduler::yield_now();

    assert_eq!(scheduler::task_exit_code(process.task_id), Some(0));
    assert!(serial::output_contains(b"hello from brk heap\n"));
    assert_eq!(
        scheduler::process_program_break(process.pid),
        Some(VirtAddr::new(start.as_u64() + 3 * PAGE_SIZE + 1))
    );
    assert_eq!(
        scheduler::process_heap_mapped_end(process.pid),
        Some(VirtAddr::new(start.as_u64() + 4 * PAGE_SIZE))
    );
    assert!(scheduler::user_page_is_mapped(process.pid, start));
    assert!(scheduler::user_page_is_mapped(
        process.pid,
        VirtAddr::new(start.as_u64() + 3 * PAGE_SIZE)
    ));
}

fn shrink_unmaps_whole_pages() {
    let process = scheduler::spawn_user_elf_process("brk_shrink_fault", BRK_SHRINK_FAULT_ELF)
        .expect("failed to spawn brk shrink fault");
    let start = scheduler::process_heap_start(process.pid).expect("missing heap start");

    scheduler::yield_now();

    assert_eq!(
        scheduler::process_state(process.pid),
        Some(ProcessState::Zombie(ProcessExit::Faulted))
    );
    let fault = scheduler::task_fault_info(process.task_id).expect("task did not fault");
    assert_eq!(fault.kind, UserFaultKind::PageFault);
    assert_eq!(
        fault.address,
        Some(VirtAddr::new(start.as_u64() + PAGE_SIZE))
    );
    assert_eq!(
        scheduler::process_program_break(process.pid),
        Some(VirtAddr::new(start.as_u64() + PAGE_SIZE))
    );
    assert!(scheduler::user_page_is_mapped(process.pid, start));
    assert!(!scheduler::user_page_is_mapped(
        process.pid,
        VirtAddr::new(start.as_u64() + PAGE_SIZE)
    ));
}

fn shrink_preserves_lower_heap() {
    let process = scheduler::spawn_user_elf_process("brk_shrink_continue", BRK_SHRINK_CONTINUE_ELF)
        .expect("failed to spawn brk shrink continue");
    let start = scheduler::process_heap_start(process.pid).expect("missing heap start");

    scheduler::yield_now();

    assert_eq!(scheduler::task_exit_code(process.task_id), Some(0));
    assert_eq!(
        scheduler::process_program_break(process.pid),
        Some(VirtAddr::new(start.as_u64() + 1))
    );
    assert_eq!(
        scheduler::process_heap_mapped_end(process.pid),
        Some(VirtAddr::new(start.as_u64() + PAGE_SIZE))
    );
    assert_eq!(
        scheduler::read_user_u64(process.task_id, start).unwrap_or(0) & 0xff,
        0x77
    );
    assert!(!scheduler::user_page_is_mapped(
        process.pid,
        VirtAddr::new(start.as_u64() + PAGE_SIZE)
    ));
}

fn same_virtual_heap_address_is_private_per_process() {
    let a = scheduler::spawn_user_elf_process_with_arg("heap_a", BRK_PRIVATE_WRITER_ELF, 0xaa)
        .expect("failed to spawn heap writer A");
    let b = scheduler::spawn_user_elf_process_with_arg("heap_b", BRK_PRIVATE_WRITER_ELF, 0xbb)
        .expect("failed to spawn heap writer B");
    let start_a = scheduler::process_heap_start(a.pid).expect("missing heap A start");
    let start_b = scheduler::process_heap_start(b.pid).expect("missing heap B start");

    assert_eq!(start_a, start_b);

    scheduler::yield_now();

    assert_eq!(scheduler::task_exit_code(a.task_id), Some(0));
    assert_eq!(scheduler::task_exit_code(b.task_id), Some(0));
    assert_eq!(scheduler::read_user_u64(a.task_id, start_a), Some(0xaa));
    assert_eq!(scheduler::read_user_u64(b.task_id, start_b), Some(0xbb));
}

fn preemption_still_works_across_brk() {
    let process = scheduler::spawn_user_elf_process("brk_busy", BRK_BUSY_COUNTER_ELF)
        .expect("failed to spawn brk busy counter");
    let start = scheduler::process_heap_start(process.pid).expect("missing heap start");

    let start_ticks = interrupts::timer_ticks();
    scheduler::enable_preemption();
    scheduler::yield_now();
    scheduler::disable_preemption();

    assert!(interrupts::timer_ticks() > start_ticks);
    assert!(scheduler::read_user_u64(process.task_id, start).unwrap_or(0) > 0);
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    vlad_os::qemu::test_panic_handler(info);
}
