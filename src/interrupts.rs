use x86_64::registers::control::Cr2;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

use crate::{gdt, hlt_loop, println};

static mut IDT: InterruptDescriptorTable = InterruptDescriptorTable::new();

pub fn init_idt() {
    let idt = unsafe { &mut *core::ptr::addr_of_mut!(IDT) };

    idt.breakpoint.set_handler_fn(breakpoint_handler);
    idt.page_fault.set_handler_fn(page_fault_handler);

    unsafe {
        idt.double_fault
            .set_handler_fn(double_fault_handler)
            .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
    }

    idt.load();

    println!("IDT initialized");
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    println!("EXCEPTION: BREAKPOINT");
    println!("{:#?}", stack_frame);
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) -> ! {
    println!("EXCEPTION: DOUBLE FAULT");
    println!("Error code: {}", error_code);
    println!("{:#?}", stack_frame);

    // A double fault means the CPU could not handle another exception cleanly,
    // so there is no safe instruction stream to return to.
    hlt_loop();
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    // CR2 contains the virtual address that the CPU tried to access when it
    // raised this page fault.
    let accessed_address = Cr2::read();

    println!("EXCEPTION: PAGE FAULT");
    println!("Accessed Address: {:?}", accessed_address);
    println!("Error Code: {:?}", error_code);
    print_page_fault_error(error_code);
    println!("{:#?}", stack_frame);

    // This kernel does not have a frame allocator, demand paging, or recovery
    // policy yet, so returning would just retry the same faulting instruction.
    hlt_loop();
}

fn print_page_fault_error(error_code: PageFaultErrorCode) {
    // The page-fault error code is a bitfield supplied by the CPU. It explains
    // what kind of access faulted and from which privilege level.
    println!("Page fault details:");
    println!("  reason: {}", page_fault_reason(error_code));
    println!("  access: {}", page_fault_access(error_code));
    println!("  mode: {}", page_fault_mode(error_code));
    println!(
        "  reserved bit violation: {}",
        yes_no(error_code.contains(PageFaultErrorCode::MALFORMED_TABLE))
    );
    println!(
        "  instruction fetch: {}",
        yes_no(error_code.contains(PageFaultErrorCode::INSTRUCTION_FETCH))
    );
}

fn page_fault_reason(error_code: PageFaultErrorCode) -> &'static str {
    if error_code.contains(PageFaultErrorCode::PROTECTION_VIOLATION) {
        "protection violation"
    } else {
        "page not present"
    }
}

fn page_fault_access(error_code: PageFaultErrorCode) -> &'static str {
    if error_code.contains(PageFaultErrorCode::CAUSED_BY_WRITE) {
        "write"
    } else {
        "read"
    }
}

fn page_fault_mode(error_code: PageFaultErrorCode) -> &'static str {
    if error_code.contains(PageFaultErrorCode::USER_MODE) {
        "user"
    } else {
        "supervisor"
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}
