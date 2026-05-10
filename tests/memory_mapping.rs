#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

use core::panic::PanicInfo;

use bootloader::{entry_point, BootInfo};
use vlad_os::memory::BootInfoFrameAllocator;
use vlad_os::qemu::{exit_qemu, QemuExitCode};
use vlad_os::{gdt, hlt_loop, memory, serial_print, serial_println};
use x86_64::{
    registers::control::Cr2,
    structures::{
        idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode},
        paging::{FrameAllocator, Mapper, Page, PageTableFlags, Translate},
    },
    VirtAddr,
};

static mut TEST_IDT: InterruptDescriptorTable = InterruptDescriptorTable::new();

entry_point!(test_kernel_main);

fn test_kernel_main(boot_info: &'static BootInfo) -> ! {
    vlad_os::serial::init();
    serial_print!("memory_mapping::map_one_page...\t");

    gdt::init();
    init_test_idt();

    let physical_memory_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(physical_memory_offset) };
    let mut frame_allocator = unsafe { BootInfoFrameAllocator::init(&boot_info.memory_map) };

    let page = Page::containing_address(VirtAddr::new(0x4444_4444_0000));
    assert!(
        mapper.translate_addr(page.start_address()).is_none(),
        "scratch page was unexpectedly already mapped"
    );

    let frame = frame_allocator
        .allocate_frame()
        .expect("no usable physical frames available");
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

    unsafe {
        mapper
            .map_to(page, frame, flags, &mut frame_allocator)
            .expect("failed to map scratch page")
            .flush();
    }

    let value = 0x_f021_f077_f065_f04e;
    let ptr: *mut u64 = page.start_address().as_mut_ptr();

    unsafe {
        core::ptr::write_volatile(ptr, value);
        assert_eq!(core::ptr::read_volatile(ptr), value);
    }

    serial_println!("[ok]");
    exit_qemu(QemuExitCode::Success);
    hlt_loop();
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
