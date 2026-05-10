#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

extern crate alloc;

use alloc::{boxed::Box, vec::Vec};
use core::panic::PanicInfo;

use bootloader::{entry_point, BootInfo};
use vlad_os::memory::BootInfoFrameAllocator;
use vlad_os::qemu::{exit_qemu, QemuExitCode};
use vlad_os::{allocator, gdt, hlt_loop, memory, serial_print, serial_println};
use x86_64::{
    registers::control::Cr2,
    structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode},
    VirtAddr,
};

static mut TEST_IDT: InterruptDescriptorTable = InterruptDescriptorTable::new();

entry_point!(test_kernel_main);

fn test_kernel_main(boot_info: &'static BootInfo) -> ! {
    vlad_os::serial::init();
    serial_print!("heap_allocation::heap_allocations...\t");

    gdt::init();
    init_test_idt();

    let physical_memory_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(physical_memory_offset) };
    let mut frame_allocator = unsafe { BootInfoFrameAllocator::init(&boot_info.memory_map) };

    allocator::init_heap(&mut mapper, &mut frame_allocator).expect("failed to initialize heap");

    simple_box_allocation();
    vec_allocation_and_growth();
    many_boxes_with_deallocation();

    serial_println!("[ok]");
    exit_qemu(QemuExitCode::Success);
    hlt_loop();
}

fn simple_box_allocation() {
    let heap_value = Box::new(41);

    assert_eq!(*heap_value, 41);
}

fn vec_allocation_and_growth() {
    let mut vec: Vec<u64> = Vec::new();

    for i in 0u64..500 {
        vec.push(i);
    }

    assert_eq!(vec.iter().copied().sum::<u64>(), 499 * 500 / 2);
}

fn many_boxes_with_deallocation() {
    for i in 0u64..1000 {
        let x = Box::new(i);

        assert_eq!(*x, i);
    }

    let reused = Box::new(1234u64);
    assert_eq!(*reused, 1234);
}

fn init_test_idt() {
    let idt = unsafe { &mut *core::ptr::addr_of_mut!(TEST_IDT) };

    idt.page_fault.set_handler_fn(test_page_fault_handler);
    idt.load();
}

extern "x86-interrupt" fn test_page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    let accessed_address = Cr2::read();

    serial_println!();
    serial_println!("EXCEPTION: PAGE FAULT");
    serial_println!("Accessed Address: {:?}", accessed_address);
    serial_println!("Error Code: {:?}", error_code);
    serial_println!("Stack Frame: {:#?}", stack_frame);
    serial_println!("[failed]");

    exit_qemu(QemuExitCode::Failed);
    hlt_loop();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    vlad_os::qemu::test_panic_handler(info);
}
