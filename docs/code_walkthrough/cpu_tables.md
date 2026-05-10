# CPU Tables Walkthrough

Back to the [architecture guide](../architecture.md) or the
[walkthrough index](README.md).

This page covers `src/gdt.rs`.

## Purpose

`gdt.rs` sets up the CPU structures needed before robust exception handling:

- the Global Descriptor Table
- a kernel code segment descriptor
- a kernel data/stack segment descriptor
- a Task State Segment descriptor
- the Task State Segment itself
- a dedicated Interrupt Stack Table entry for double faults

## Dependencies

- `crate::println` for VGA status output
- `x86_64::structures::gdt` for GDT descriptors and selectors
- `x86_64::structures::tss::TaskStateSegment` for the TSS structure
- `x86_64::VirtAddr` for canonical virtual addresses
- `x86_64::instructions::segmentation::{CS, SS}` for loading code and stack
  selectors
- `x86_64::instructions::tables::load_tss` for loading the TSS selector

## Invariants

- CPU table memory must stay at a stable address after loading.
- The double-fault IST stack must live for the whole kernel lifetime.
- The IST pointer must point to the top of the stack because x86_64 stacks grow
  downward.
- The GDT must be loaded before `CS::set_reg` and `load_tss`.
- Mutation of `static mut` values is limited to early single-threaded
  initialization.

## Line-By-Line

| Code | Explanation |
| --- | --- |
| `use crate::println;` | Imports VGA printing so `gdt::init()` can report successful initialization. |
| `use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};` | Imports the GDT type, descriptor constructors, and selector type from the `x86_64` crate. |
| `use x86_64::structures::tss::TaskStateSegment;` | Imports the TSS type. In long mode, the TSS is mainly useful for stack switching through IST entries. |
| `use x86_64::{PrivilegeLevel, VirtAddr};` | Imports privilege levels for placeholder selectors and virtual-address helpers for stack addresses. |
| `pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;` | Defines IST slot 0 as the double-fault stack. Tests and the IDT use the same constant. |
| `const DOUBLE_FAULT_STACK_SIZE: usize = 4096 * 5;` | Allocates five pages for the emergency double-fault stack. This mirrors the classic Writing an OS in Rust teaching setup. |
| `struct Selectors { ... }` | Stores the GDT selectors returned when entries are inserted. We need them later to load `CS`, `SS`, and the TSS. |
| `code_selector: SegmentSelector,` | Selector for the kernel code segment descriptor. |
| `data_selector: SegmentSelector,` | Selector for the kernel data/stack descriptor. Long mode ignores most data-segment base/limit fields, but `SS` still needs a valid selector for stack-return state. |
| `tss_selector: SegmentSelector,` | Selector for the TSS descriptor. |
| `static mut DOUBLE_FAULT_STACK: [u8; DOUBLE_FAULT_STACK_SIZE] = ...;` | Reserves permanent stack memory for double faults. It is `static mut` because it is global storage with a stable address. |
| `static mut TSS: TaskStateSegment = TaskStateSegment::new();` | Creates the TSS in static storage so the CPU can keep referencing it after `ltr`. |
| `static mut GDT: GlobalDescriptorTable = GlobalDescriptorTable::new();` | Creates the GDT in static storage. Loaded descriptor tables must not move. |
| `static mut SELECTORS: Selectors = ...;` | Creates placeholder selectors. They are overwritten after the real GDT entries are inserted. |
| `SegmentSelector::new(0, PrivilegeLevel::Ring0)` | Builds harmless placeholder ring-0 selectors. Index 0 is the null descriptor and is replaced before use. |
| `pub fn init() {` | Public entry point for GDT/TSS initialization. Called by normal boot and test kernels. |
| `init_tss();` | Fills the TSS IST entry before the TSS descriptor is created. |
| `init_gdt();` | Adds the code segment and TSS descriptors to the GDT. |
| `load_gdt();` | Loads the CPU registers that point at the new GDT and TSS. |
| `println!("GDT initialized");` | Emits visible confirmation after setup completes. |
| `fn init_tss() {` | Starts the helper that configures the double-fault IST stack. |
| `let stack_start = VirtAddr::from_ptr(...);` | Converts the static stack's address into a canonical virtual address. |
| `core::ptr::addr_of!(DOUBLE_FAULT_STACK) as *const u8` | Gets a raw pointer to the static stack without creating a shared reference to `static mut`. |
| `let stack_end = stack_start + DOUBLE_FAULT_STACK_SIZE;` | Computes the top of the stack. The CPU starts stacks at the high address because stacks grow downward. |
| `unsafe { ... }` | Required because writing a `static mut` TSS is unsafe. This happens during early boot before concurrency exists. |
| `(*core::ptr::addr_of_mut!(TSS)).interrupt_stack_table[...] = stack_end;` | Stores the stack top in IST slot 0 of the TSS. |
| `fn init_gdt() {` | Starts the helper that constructs GDT entries. |
| `let gdt = unsafe { &mut *core::ptr::addr_of_mut!(GDT) };` | Gets a mutable reference to the static GDT via a raw pointer. |
| `let code_selector = gdt.add_entry(Descriptor::kernel_code_segment());` | Adds a kernel code descriptor and saves its selector. Long mode still requires a valid code segment. |
| `let data_selector = gdt.add_entry(Descriptor::kernel_data_segment());` | Adds a kernel data/stack descriptor and saves its selector for `SS`. |
| `let tss_selector = gdt.add_entry(Descriptor::tss_segment(...));` | Adds a TSS descriptor and saves its selector. The CPU uses it for IST stack switching. |
| `&*core::ptr::addr_of!(TSS)` | Provides a stable shared reference to the static TSS for descriptor construction. |
| `*core::ptr::addr_of_mut!(SELECTORS) = Selectors { ... };` | Stores the real selectors after GDT insertion. |
| `fn load_gdt() {` | Starts the helper that makes the CPU use the table. |
| `use x86_64::instructions::segmentation::{Segment, CS, SS};` | Imports the trait and segment-register types required to load `CS` and `SS`. |
| `use x86_64::instructions::tables::load_tss;` | Imports the instruction wrapper for `ltr`. |
| `let gdt = unsafe { &*core::ptr::addr_of!(GDT) };` | Gets a stable shared reference to the GDT. |
| `let selectors = unsafe { &*core::ptr::addr_of!(SELECTORS) };` | Gets the selectors stored during `init_gdt()`. |
| `gdt.load();` | Executes `lgdt` through the `x86_64` crate, loading the GDT pointer into the CPU. |
| `unsafe { CS::set_reg(selectors.code_selector); ... }` | Unsafe because the caller must guarantee the selector points to a valid code descriptor. |
| `CS::set_reg(selectors.code_selector);` | Reloads the code segment register so the CPU uses the new code descriptor. |
| `SS::set_reg(selectors.data_selector);` | Loads a valid stack selector. This matters now that task trap frames carry `ss` for `iretq`. |
| `load_tss(selectors.tss_selector);` | Executes `ltr`, loading the task register with the TSS selector. |
