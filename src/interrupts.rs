use core::sync::atomic::{AtomicU64, Ordering};

use pic8259::ChainedPics;
use spin::Mutex;
use x86_64::instructions::{interrupts as cpu_interrupts, port::Port};
use x86_64::registers::control::Cr2;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use x86_64::PrivilegeLevel;

use crate::arch::x86_64::context::{self, TrapFrameWithErrorCode};
use crate::{gdt, hlt_loop, println};

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;
pub const PIT_FREQUENCY_HZ: u32 = 100;
pub const YIELD_VECTOR: u8 = PIC_2_OFFSET + 8;

const PIT_BASE_FREQUENCY_HZ: u32 = 1_193_182;
const PIT_COMMAND_PORT: u16 = 0x43;
const PIT_CHANNEL_0_PORT: u16 = 0x40;
const PIT_CHANNEL_0_SQUARE_WAVE: u8 = 0x36;
const KEYBOARD_DATA_PORT: u16 = 0x60;

static PICS: Mutex<ChainedPics> =
    Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

static TIMER_TICKS: AtomicU64 = AtomicU64::new(0);
static mut IDT: InterruptDescriptorTable = InterruptDescriptorTable::new();

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = PIC_1_OFFSET,
    Keyboard = PIC_1_OFFSET + 1,
    Yield = YIELD_VECTOR,
    Syscall = crate::syscall::SYSCALL_VECTOR,
}

impl InterruptIndex {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn as_usize(self) -> usize {
        self.as_u8() as usize
    }
}

pub fn init_idt() {
    let idt = unsafe { &mut *core::ptr::addr_of_mut!(IDT) };

    idt.breakpoint.set_handler_fn(breakpoint_handler);
    idt.page_fault.set_handler_fn(page_fault_handler);

    unsafe {
        idt.general_protection_fault
            .set_handler_addr(context::general_protection_entry_addr());
        idt.double_fault
            .set_handler_fn(double_fault_handler)
            .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
    }

    unsafe {
        idt[InterruptIndex::Timer.as_usize()]
            .set_handler_addr(context::timer_interrupt_entry_addr());
        idt[InterruptIndex::Yield.as_usize()]
            .set_handler_addr(context::yield_interrupt_entry_addr());
        idt[InterruptIndex::Syscall.as_usize()]
            .set_handler_addr(context::syscall_interrupt_entry_addr())
            .set_privilege_level(PrivilegeLevel::Ring3);
    }

    idt[InterruptIndex::Keyboard.as_usize()].set_handler_fn(keyboard_interrupt_handler);

    idt.load();

    println!("IDT initialized");
}

pub fn init_pics() {
    unsafe {
        let mut pics = PICS.lock();

        pics.initialize();

        // Only IRQ0 (timer) and IRQ1 (keyboard) are unmasked for this
        // milestone. Other device IRQs stay masked until handlers exist.
        pics.write_masks(0b1111_1100, 0b1111_1111);
    }

    println!(
        "PICs initialized at offsets {} and {}",
        PIC_1_OFFSET, PIC_2_OFFSET
    );
}

pub fn init_pit() {
    let divisor = (PIT_BASE_FREQUENCY_HZ / PIT_FREQUENCY_HZ) as u16;

    unsafe {
        let mut command_port = Port::new(PIT_COMMAND_PORT);
        let mut channel_0 = Port::new(PIT_CHANNEL_0_PORT);

        command_port.write(PIT_CHANNEL_0_SQUARE_WAVE);
        channel_0.write((divisor & 0x00ff) as u8);
        channel_0.write((divisor >> 8) as u8);
    }

    println!("PIT initialized at {} Hz", PIT_FREQUENCY_HZ);
}

pub fn enable_interrupts() {
    cpu_interrupts::enable();
    println!("CPU interrupts enabled");
}

pub fn timer_ticks() -> u64 {
    TIMER_TICKS.load(Ordering::Relaxed)
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

#[no_mangle]
pub extern "C" fn timer_interrupt_rust(frame_rsp: u64) -> u64 {
    TIMER_TICKS.fetch_add(1, Ordering::Relaxed);

    let next_rsp = crate::scheduler::on_timer_interrupt(frame_rsp);

    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Timer.as_u8());
    }

    next_rsp
}

#[no_mangle]
pub extern "C" fn yield_interrupt_rust(frame_rsp: u64) -> u64 {
    crate::scheduler::on_yield_interrupt(frame_rsp)
}

#[no_mangle]
pub extern "C" fn syscall_interrupt_rust(frame_rsp: u64) -> u64 {
    crate::syscall::dispatch(frame_rsp)
}

#[no_mangle]
pub extern "C" fn general_protection_rust(frame_rsp: u64) -> u64 {
    let frame = unsafe { &*(frame_rsp as *const TrapFrameWithErrorCode) };

    if frame.cs & 0x3 == 0x3 {
        println!("EXCEPTION: USER GENERAL PROTECTION");
        println!("Error code: {}", frame.error_code);
        return crate::scheduler::fail_current_from_interrupt(frame_rsp);
    }

    println!("EXCEPTION: GENERAL PROTECTION");
    println!("Error code: {}", frame.error_code);
    println!("RIP: {:#018x}", frame.rip);
    println!("CS: {:#06x}", frame.cs);

    hlt_loop();
}

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    let scancode: u8 = unsafe { Port::new(KEYBOARD_DATA_PORT).read() };

    println!("keyboard scancode: {:#04x}", scancode);

    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8());
    }
}
