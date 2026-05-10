#![no_std]
#![no_main]

use core::hint::spin_loop;
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use bootloader::{entry_point, BootInfo};
use vlad_os::memory::BootInfoFrameAllocator;
use vlad_os::qemu::{exit_qemu, QemuExitCode};
use vlad_os::{
    allocator, gdt, hlt_loop, interrupts, memory, scheduler, serial_print, serial_println,
};
use x86_64::VirtAddr;

static A_PROGRESS: AtomicUsize = AtomicUsize::new(0);
static B_PROGRESS: AtomicUsize = AtomicUsize::new(0);
static A_DONE: AtomicBool = AtomicBool::new(false);

const PROGRESS_TARGET: usize = 20_000;
const LOCAL_STALL_LIMIT: usize = 100_000_000;
const TICK_TIMEOUT: u64 = 300;

entry_point!(test_kernel_main);

fn test_kernel_main(boot_info: &'static BootInfo) -> ! {
    vlad_os::serial::init();
    serial_print!("preemptive_tasks::timer_preemption...\t");

    gdt::init();
    interrupts::init_idt();
    interrupts::init_pics();
    interrupts::init_pit();

    let physical_memory_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(physical_memory_offset) };
    let mut frame_allocator = unsafe { BootInfoFrameAllocator::init(&boot_info.memory_map) };

    allocator::init_heap(&mut mapper, &mut frame_allocator).expect("failed to initialize heap");

    interrupts::enable_interrupts();

    scheduler::spawn(task_a).expect("failed to spawn task A");
    scheduler::spawn(task_b).expect("failed to spawn task B");
    scheduler::enable_preemption();
    scheduler::run();

    panic!("preemptive task scheduler returned without test success");
}

fn task_a() {
    let mut local_state = 0usize;

    loop {
        local_state = local_state.wrapping_add(1);
        A_PROGRESS.store(local_state, Ordering::SeqCst);

        if local_state >= PROGRESS_TARGET && B_PROGRESS.load(Ordering::SeqCst) >= PROGRESS_TARGET {
            A_DONE.store(true, Ordering::SeqCst);
            return;
        }

        assert!(
            !(interrupts::timer_ticks() > TICK_TIMEOUT && B_PROGRESS.load(Ordering::SeqCst) == 0),
            "task B did not run after timer ticks"
        );
        assert!(
            !(local_state > LOCAL_STALL_LIMIT && B_PROGRESS.load(Ordering::SeqCst) == 0),
            "task B did not run before local stall limit"
        );

        spin_loop();
    }
}

fn task_b() {
    let mut local_state = 0usize;

    loop {
        local_state = local_state.wrapping_add(1);
        B_PROGRESS.store(local_state, Ordering::SeqCst);

        if A_DONE.load(Ordering::SeqCst)
            && A_PROGRESS.load(Ordering::SeqCst) >= PROGRESS_TARGET
            && local_state >= PROGRESS_TARGET
            && interrupts::timer_ticks() > 0
            && scheduler::finished_task_count() >= 1
        {
            serial_println!("[ok]");
            exit_qemu(QemuExitCode::Success);
            hlt_loop();
        }

        assert!(
            !(interrupts::timer_ticks() > TICK_TIMEOUT
                && A_PROGRESS.load(Ordering::SeqCst) < PROGRESS_TARGET),
            "task A did not keep running under preemption"
        );
        assert!(
            !(local_state > LOCAL_STALL_LIMIT
                && A_PROGRESS.load(Ordering::SeqCst) < PROGRESS_TARGET),
            "task A did not reach the progress target"
        );

        spin_loop();
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    vlad_os::qemu::test_panic_handler(info);
}
