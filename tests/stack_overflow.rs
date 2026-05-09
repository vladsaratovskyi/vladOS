#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

use core::panic::PanicInfo;

use blog_os::qemu::{exit_qemu, QemuExitCode};
use blog_os::{gdt, hlt_loop, serial_print, serial_println};
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};

static mut TEST_IDT: InterruptDescriptorTable = InterruptDescriptorTable::new();

#[no_mangle]
pub extern "C" fn _start() -> ! {
    blog_os::serial::init();
    serial_print!("stack_overflow::stack_overflow...\t");

    gdt::init();
    init_test_idt();

    stack_overflow();

    panic!("Execution continued after stack overflow");
}

fn init_test_idt() {
    let idt = unsafe { &mut *core::ptr::addr_of_mut!(TEST_IDT) };

    unsafe {
        idt.double_fault
            .set_handler_fn(test_double_fault_handler)
            .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
    }

    idt.load();
}

#[allow(unconditional_recursion)]
#[inline(never)]
fn stack_overflow() {
    stack_overflow();

    // Keep a side effect after the recursive call so the compiler cannot turn
    // this into a tail call. The overflowing kernel stack should force a page
    // fault while the CPU is already trying to deliver an exception, producing
    // a double fault.
    unsafe {
        core::ptr::read_volatile(&0);
    }
}

extern "x86-interrupt" fn test_double_fault_handler(
    _stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    serial_println!("[ok]");

    // Reaching this handler proves the double-fault IDT entry and dedicated
    // IST stack worked. QEMU exits here so the integration test can report
    // success instead of halting forever.
    exit_qemu(QemuExitCode::Success);
    hlt_loop();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    blog_os::qemu::test_panic_handler(info);
}
