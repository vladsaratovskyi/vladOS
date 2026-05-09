use crate::println;

use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::{PrivilegeLevel, VirtAddr};

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

const DOUBLE_FAULT_STACK_SIZE: usize = 4096 * 5;

struct Selectors {
    code_selector: SegmentSelector,
    tss_selector: SegmentSelector,
}

// These CPU tables and the emergency stack must live at stable addresses for
// as long as the processor can reference them. This early kernel has no heap or
// once-initialization primitive yet, so `static mut` keeps the storage explicit;
// all mutation happens during single-threaded boot before the tables are used.
static mut DOUBLE_FAULT_STACK: [u8; DOUBLE_FAULT_STACK_SIZE] = [0; DOUBLE_FAULT_STACK_SIZE];
static mut TSS: TaskStateSegment = TaskStateSegment::new();
static mut GDT: GlobalDescriptorTable = GlobalDescriptorTable::new();
static mut SELECTORS: Selectors = Selectors {
    code_selector: SegmentSelector::new(0, PrivilegeLevel::Ring0),
    tss_selector: SegmentSelector::new(0, PrivilegeLevel::Ring0),
};

pub fn init() {
    init_tss();
    init_gdt();
    load_gdt();

    println!("GDT initialized");
}

fn init_tss() {
    // The TSS gives the CPU a known-good IST stack for double faults. That
    // matters because a double fault often means the normal kernel stack was
    // already unusable when the second exception arrived.
    let stack_start = VirtAddr::from_ptr(core::ptr::addr_of!(DOUBLE_FAULT_STACK) as *const u8);
    let stack_end = stack_start + DOUBLE_FAULT_STACK_SIZE;

    unsafe {
        (*core::ptr::addr_of_mut!(TSS)).interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] =
            stack_end;
    }
}

fn init_gdt() {
    // Long mode mostly disables segmentation, but x86_64 still requires a
    // valid code segment and uses a TSS descriptor for IST stack switching.
    let gdt = unsafe { &mut *core::ptr::addr_of_mut!(GDT) };
    let code_selector = gdt.add_entry(Descriptor::kernel_code_segment());
    let tss_selector =
        gdt.add_entry(Descriptor::tss_segment(unsafe { &*core::ptr::addr_of!(TSS) }));

    unsafe {
        *core::ptr::addr_of_mut!(SELECTORS) = Selectors {
            code_selector,
            tss_selector,
        };
    }
}

fn load_gdt() {
    use x86_64::instructions::segmentation::{Segment, CS};
    use x86_64::instructions::tables::load_tss;

    let gdt = unsafe { &*core::ptr::addr_of!(GDT) };
    let selectors = unsafe { &*core::ptr::addr_of!(SELECTORS) };

    gdt.load();

    unsafe {
        CS::set_reg(selectors.code_selector);
        load_tss(selectors.tss_selector);
    }
}
