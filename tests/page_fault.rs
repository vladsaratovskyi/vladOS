#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

use core::panic::PanicInfo;

use blog_os::qemu::{exit_qemu, QemuExitCode};
use blog_os::{gdt, hlt_loop, serial_print, serial_println};
use x86_64::registers::control::Cr2;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

static mut TEST_IDT: InterruptDescriptorTable = InterruptDescriptorTable::new();

#[no_mangle]
pub extern "C" fn _start() -> ! {
    blog_os::serial::init();
    serial_print!("page_fault::invalid_memory_access...\t");

    gdt::init();
    init_test_idt();

    unsafe {
        // This address is canonical on x86_64, but it should not be mapped by
        // this early kernel. A volatile read keeps the compiler from removing
        // the intentionally faulting access.
        let ptr = 0x4444_4444_0000 as *const u64;
        core::ptr::read_volatile(ptr);
    }

    panic!("Execution continued after invalid memory access");
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
    // CR2 is set by the CPU to the virtual address that caused the page fault.
    let accessed_address = Cr2::read();

    serial_println!();
    serial_println!("EXCEPTION: PAGE FAULT");
    serial_println!("Accessed Address: {:?}", accessed_address);
    serial_println!("Error Code: {:?}", error_code);
    serial_println!("Stack Frame: {:#?}", stack_frame);
    serial_println!("[ok]");

    // Reaching this handler proves vector 14 was installed and invoked. The
    // test exits QEMU here so success is tied to the page-fault path.
    exit_qemu(QemuExitCode::Success);
    hlt_loop();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    blog_os::qemu::test_panic_handler(info);
}
