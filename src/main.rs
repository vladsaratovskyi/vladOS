#![no_std]
#![no_main]

use core::panic::PanicInfo;

use bootloader::{entry_point, BootInfo};
use vlad_os::{allocator, gdt, hlt_loop, interrupts, memory, println, scheduler};
use x86_64::{structures::paging::Translate, PhysAddr, VirtAddr};

entry_point!(kernel_main);

fn kernel_main(boot_info: &'static BootInfo) -> ! {
    println!("Hello from vladOS!");

    gdt::init();
    interrupts::init_idt();
    interrupts::init_pics();
    interrupts::init_pit();

    let physical_memory_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(physical_memory_offset) };

    print_memory_diagnostics(boot_info, &mapper, physical_memory_offset);

    let mut frame_allocator =
        unsafe { memory::BootInfoFrameAllocator::init(&boot_info.memory_map) };
    allocator::init_heap(&mut mapper, &mut frame_allocator).expect("heap initialization failed");
    println!("Heap initialized");

    interrupts::enable_interrupts();

    x86_64::instructions::interrupts::int3();

    println!("Still alive after breakpoint");

    run_demo_tasks();

    hlt_loop();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    hlt_loop();
}

fn print_memory_diagnostics(
    boot_info: &'static BootInfo,
    mapper: &impl Translate,
    physical_memory_offset: VirtAddr,
) {
    let usable_regions = boot_info
        .memory_map
        .iter()
        .filter(|region| region.region_type == bootloader::bootinfo::MemoryRegionType::Usable)
        .count();

    println!("Memory diagnostics:");
    println!("  physical_memory_offset: {:?}", physical_memory_offset);
    print_translation(mapper, "VGA", VirtAddr::new(0xb8000));
    print_translation(
        mapper,
        "kernel_main",
        VirtAddr::new(kernel_main as *const () as u64),
    );
    print_translation(mapper, "BootInfo", VirtAddr::from_ptr(boot_info));
    println!("  usable memory regions: {}", usable_regions);
}

fn print_translation(mapper: &impl Translate, label: &str, virtual_address: VirtAddr) {
    match mapper.translate_addr(virtual_address) {
        Some(physical_address) => print_mapping(label, virtual_address, physical_address),
        None => println!("  {}: {:?} -> unmapped", label, virtual_address),
    }
}

fn print_mapping(label: &str, virtual_address: VirtAddr, physical_address: PhysAddr) {
    println!(
        "  {}: {:?} -> {:?}",
        label, virtual_address, physical_address
    );
}

fn run_demo_tasks() {
    println!("Starting cooperative task demo");

    scheduler::spawn(demo_task_a).expect("failed to spawn demo task A");
    scheduler::spawn(demo_task_b).expect("failed to spawn demo task B");
    scheduler::run();

    println!("Cooperative task demo complete");
}

fn demo_task_a() {
    for step in 0..2 {
        println!("task A: {}", step);
        scheduler::yield_now();
    }
}

fn demo_task_b() {
    for step in 0..2 {
        println!("task B: {}", step);
        scheduler::yield_now();
    }
}
