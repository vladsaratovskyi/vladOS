#![no_std]
#![no_main]

use core::panic::PanicInfo;

use bootloader::{entry_point, BootInfo};
use vlad_os::memory::BootInfoFrameAllocator;
use vlad_os::process::{ProcessExit, ProcessState};
use vlad_os::qemu::{exit_qemu, QemuExitCode};
use vlad_os::{
    allocator, gdt, hlt_loop, interrupts, memory, scheduler, serial_print, serial_println, user,
};
use x86_64::VirtAddr;

const PARENT_SUITE_ELF: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/process_wait_parent_suite.elf"));
const DELAYED_EXIT_CHILD_ELF: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/delayed_exit_child.elf"));
const IMMEDIATE_EXIT_CHILD_ELF: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/immediate_exit_child.elf"));
const FAULTING_CHILD_ELF: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/faulting_child.elf"));

entry_point!(test_kernel_main);

fn test_kernel_main(boot_info: &'static BootInfo) -> ! {
    vlad_os::serial::init();
    serial_print!("process_lifecycle::getpid_waitpid_zombies...\t");

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

    panic!("process lifecycle scheduler returned without test success");
}

fn orchestrator() {
    let parent = scheduler::spawn_user_elf_process("wait_parent", PARENT_SUITE_ELF)
        .expect("failed to spawn parent process");
    let delayed = scheduler::spawn_child_user_elf_process(
        parent.pid,
        "delayed_child",
        DELAYED_EXIT_CHILD_ELF,
    )
    .expect("failed to spawn delayed child");
    let immediate = scheduler::spawn_child_user_elf_process(
        parent.pid,
        "immediate_child",
        IMMEDIATE_EXIT_CHILD_ELF,
    )
    .expect("failed to spawn immediate child");
    let faulting =
        scheduler::spawn_child_user_elf_process(parent.pid, "faulting_child", FAULTING_CHILD_ELF)
            .expect("failed to spawn faulting child");
    let non_child = scheduler::spawn_user_elf_process("non_child", IMMEDIATE_EXIT_CHILD_ELF)
        .expect("failed to spawn non-child process");

    assert_eq!(
        parent.pid,
        scheduler::task_process_id(parent.task_id).unwrap()
    );
    assert_eq!(scheduler::process_parent(parent.pid), Some(None));
    assert_eq!(
        scheduler::process_parent(delayed.pid),
        Some(Some(parent.pid))
    );
    assert_eq!(
        scheduler::process_parent(immediate.pid),
        Some(Some(parent.pid))
    );
    assert_eq!(
        scheduler::process_parent(faulting.pid),
        Some(Some(parent.pid))
    );
    assert_eq!(scheduler::process_parent(non_child.pid), Some(None));

    write_pid_table(
        parent.task_id,
        delayed.pid.0,
        immediate.pid.0,
        faulting.pid.0,
        non_child.pid.0,
    );

    scheduler::yield_now();

    assert_eq!(
        scheduler::process_state(immediate.pid),
        Some(ProcessState::Zombie(ProcessExit::Exited(7)))
    );
    assert_eq!(
        scheduler::process_state(faulting.pid),
        Some(ProcessState::Zombie(ProcessExit::Faulted))
    );
    assert!(scheduler::process_exists(immediate.pid));
    assert!(scheduler::task_exit_code(parent.task_id).is_none());

    scheduler::yield_now();
    assert!(scheduler::task_exit_code(parent.task_id).is_none());
    assert_eq!(
        scheduler::process_state(delayed.pid),
        Some(ProcessState::Running)
    );

    for _ in 0..8 {
        if scheduler::task_exit_code(parent.task_id).is_some() {
            break;
        }

        scheduler::yield_now();
    }

    assert_eq!(scheduler::task_exit_code(parent.task_id), Some(0));
    assert_eq!(
        scheduler::process_state(parent.pid),
        Some(ProcessState::Zombie(ProcessExit::Exited(0)))
    );
    assert!(!scheduler::process_exists(delayed.pid));
    assert!(!scheduler::process_exists(immediate.pid));
    assert!(!scheduler::process_exists(faulting.pid));
    assert_eq!(
        scheduler::process_state(non_child.pid),
        Some(ProcessState::Zombie(ProcessExit::Exited(7)))
    );

    serial_println!("[ok]");
    exit_qemu(QemuExitCode::Success);
    hlt_loop();
}

fn write_pid_table(
    parent_task: vlad_os::task::TaskId,
    delayed: usize,
    immediate: usize,
    faulting: usize,
    non_child: usize,
) {
    let mut table = [0_u8; 32];
    table[0..8].copy_from_slice(&(delayed as u64).to_le_bytes());
    table[8..16].copy_from_slice(&(immediate as u64).to_le_bytes());
    table[16..24].copy_from_slice(&(faulting as u64).to_le_bytes());
    table[24..32].copy_from_slice(&(non_child as u64).to_le_bytes());

    scheduler::copy_to_user(parent_task, VirtAddr::new(user::USER_DATA_BASE), &table)
        .expect("failed to copy process pid table to parent");
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    vlad_os::qemu::test_panic_handler(info);
}
