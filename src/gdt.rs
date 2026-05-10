use crate::println;

use x86_64::instructions::interrupts as cpu_interrupts;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::{PrivilegeLevel, VirtAddr};

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

const DOUBLE_FAULT_STACK_SIZE: usize = 4096 * 5;

struct Selectors {
    code_selector: SegmentSelector,
    data_selector: SegmentSelector,
    user_code_selector: SegmentSelector,
    user_data_selector: SegmentSelector,
    tss_selector: SegmentSelector,
}

// These CPU tables and the emergency stack must live at stable addresses for
// as long as the processor can reference them. GDT setup also runs before heap
// initialization, so `static mut` keeps the storage explicit; all mutation
// happens during single-threaded boot before the tables are used.
static mut DOUBLE_FAULT_STACK: [u8; DOUBLE_FAULT_STACK_SIZE] = [0; DOUBLE_FAULT_STACK_SIZE];
static mut TSS: TaskStateSegment = TaskStateSegment::new();
static mut GDT: GlobalDescriptorTable = GlobalDescriptorTable::new();
static mut SELECTORS: Selectors = Selectors {
    code_selector: SegmentSelector::new(0, PrivilegeLevel::Ring0),
    data_selector: SegmentSelector::new(0, PrivilegeLevel::Ring0),
    user_code_selector: SegmentSelector::new(0, PrivilegeLevel::Ring3),
    user_data_selector: SegmentSelector::new(0, PrivilegeLevel::Ring3),
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
    let data_selector = gdt.add_entry(Descriptor::kernel_data_segment());
    let user_data_selector = gdt.add_entry(Descriptor::user_data_segment());
    let user_code_selector = gdt.add_entry(Descriptor::user_code_segment());
    let tss_selector = gdt.add_entry(Descriptor::tss_segment(unsafe {
        &*core::ptr::addr_of!(TSS)
    }));

    unsafe {
        *core::ptr::addr_of_mut!(SELECTORS) = Selectors {
            code_selector,
            data_selector,
            user_code_selector,
            user_data_selector,
            tss_selector,
        };
    }
}

fn load_gdt() {
    use x86_64::instructions::segmentation::{Segment, CS, SS};
    use x86_64::instructions::tables::load_tss;

    let gdt = unsafe { &*core::ptr::addr_of!(GDT) };
    let selectors = unsafe { &*core::ptr::addr_of!(SELECTORS) };

    gdt.load();

    unsafe {
        CS::set_reg(selectors.code_selector);
        SS::set_reg(selectors.data_selector);
        load_tss(selectors.tss_selector);
    }
}

pub fn kernel_code_selector() -> SegmentSelector {
    unsafe { (*core::ptr::addr_of!(SELECTORS)).code_selector }
}

pub fn kernel_data_selector() -> SegmentSelector {
    unsafe { (*core::ptr::addr_of!(SELECTORS)).data_selector }
}

pub fn user_code_selector() -> SegmentSelector {
    with_rpl(
        unsafe { (*core::ptr::addr_of!(SELECTORS)).user_code_selector },
        PrivilegeLevel::Ring3,
    )
}

pub fn user_data_selector() -> SegmentSelector {
    with_rpl(
        unsafe { (*core::ptr::addr_of!(SELECTORS)).user_data_selector },
        PrivilegeLevel::Ring3,
    )
}

pub fn set_kernel_stack(stack_top: VirtAddr) {
    // A CPL3 -> CPL0 interrupt transition loads RSP from TSS.rsp0 before
    // pushing the interrupt frame. This single-core kernel updates it while
    // interrupts are disabled whenever the scheduler selects a new task.
    cpu_interrupts::without_interrupts(|| unsafe {
        (*core::ptr::addr_of_mut!(TSS)).privilege_stack_table[0] = stack_top;
    });
}

fn with_rpl(mut selector: SegmentSelector, rpl: PrivilegeLevel) -> SegmentSelector {
    selector.set_rpl(rpl);
    selector
}
