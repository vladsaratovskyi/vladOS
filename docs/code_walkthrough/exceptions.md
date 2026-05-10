# Exception Handling Walkthrough

Back to the [architecture guide](../architecture.md) or the
[walkthrough index](README.md).

This page covers `src/interrupts.rs`.

## Purpose

`interrupts.rs` builds and loads the production Interrupt Descriptor Table. It
installs handlers for:

- breakpoint exceptions
- double faults
- page faults

## Dependencies

- `Cr2` to read the virtual address that caused a page fault
- `InterruptDescriptorTable` for IDT construction
- `InterruptStackFrame` for saved CPU state passed to handlers
- `PageFaultErrorCode` for CPU-supplied page-fault details
- `crate::gdt` for the double-fault IST index
- `crate::hlt_loop` for fatal handlers
- `crate::println` for VGA diagnostics

## Invariants

- The IDT must live at a stable address after `lidt`.
- Exception handlers must use `extern "x86-interrupt"`.
- The breakpoint handler may return.
- The double-fault handler must not return.
- The page-fault handler currently must not return because the kernel cannot
  recover or map missing pages yet.

## Line-By-Line

| Code | Explanation |
| --- | --- |
| `use x86_64::registers::control::Cr2;` | Imports access to the `CR2` register, where the CPU stores the faulting virtual address for page faults. |
| `use x86_64::structures::idt::{...};` | Imports the IDT type and the argument types used by exception handlers. |
| `InterruptDescriptorTable` | The table that maps exception and interrupt vectors to handler functions. |
| `InterruptStackFrame` | A snapshot of CPU state pushed for an exception: instruction pointer, code segment, flags, stack pointer, and stack segment. |
| `PageFaultErrorCode` | A bitflag value pushed by the CPU for page faults. It explains the access type and cause. |
| `use crate::{gdt, hlt_loop, println};` | Imports local kernel helpers: GDT constants, halt loop, and VGA output. |
| `static mut IDT: InterruptDescriptorTable = InterruptDescriptorTable::new();` | Stores the production IDT at a stable global address. It is mutated during boot and then loaded. |
| `pub fn init_idt() {` | Public function called by normal boot to initialize exception handling. |
| `let idt = unsafe { &mut *core::ptr::addr_of_mut!(IDT) };` | Gets a mutable reference to the static IDT through a raw pointer. |
| `idt.breakpoint.set_handler_fn(breakpoint_handler);` | Installs the vector 3 breakpoint handler. |
| `idt.page_fault.set_handler_fn(page_fault_handler);` | Installs the vector 14 page-fault handler. |
| `unsafe { idt.double_fault ... }` | Configures the vector 8 double-fault handler and its IST stack. `set_stack_index` is unsafe because the index must be valid in the loaded TSS. |
| `.set_handler_fn(double_fault_handler)` | Sets the double-fault handler function. |
| `.set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);` | Tells the CPU to switch to the dedicated IST stack before calling the double-fault handler. |
| `idt.load();` | Executes `lidt` through the `x86_64` crate. After this, the CPU uses this IDT. |
| `println!("IDT initialized");` | Prints confirmation after the IDT is active. |
| `extern "x86-interrupt" fn breakpoint_handler(...)` | Defines a CPU exception handler with the interrupt ABI. |
| `stack_frame: InterruptStackFrame` | Receives the CPU state saved at the breakpoint. |
| `println!("EXCEPTION: BREAKPOINT");` | Labels the exception in VGA output. |
| `println!("{:#?}", stack_frame);` | Pretty-prints the saved CPU state. |
| returning from `breakpoint_handler` | Safe here because breakpoint exceptions are intentionally recoverable. Execution resumes after `int3`. |
| `extern "x86-interrupt" fn double_fault_handler(... ) -> !` | Defines the double-fault handler. `-> !` means it never returns. |
| `error_code: u64` | Double faults push an error code. It is normally zero. |
| `println!("EXCEPTION: DOUBLE FAULT");` | Labels the fatal exception. |
| `println!("Error code: {}", error_code);` | Prints the CPU-supplied error code. |
| `println!("{:#?}", stack_frame);` | Prints the saved CPU state. |
| `hlt_loop();` | Halts forever. The kernel cannot safely resume from a double fault. |
| `extern "x86-interrupt" fn page_fault_handler(...)` | Defines the production page-fault handler. |
| `error_code: PageFaultErrorCode` | Receives CPU-supplied page-fault flags. |
| `let accessed_address = Cr2::read();` | Reads `CR2`, the faulting virtual address. |
| `println!("EXCEPTION: PAGE FAULT");` | Labels the page fault in VGA output. |
| `println!("Accessed Address: {:?}", accessed_address);` | Prints the virtual address that caused the fault. |
| `println!("Error Code: {:?}", error_code);` | Prints the raw page-fault flags using the crate's debug formatting. |
| `print_page_fault_error(error_code);` | Calls the local decoder so the bitfield is human-readable. |
| `println!("{:#?}", stack_frame);` | Prints the saved CPU state at the faulting instruction. |
| `hlt_loop();` | Halts forever. Returning would retry the same faulting instruction. |
| `fn print_page_fault_error(error_code: PageFaultErrorCode)` | Helper that prints decoded page-fault meaning. |
| `println!("Page fault details:");` | Starts the decoded diagnostic section. |
| `println!("  reason: {}", page_fault_reason(error_code));` | Prints whether the fault was a not-present page or a protection violation. |
| `println!("  access: {}", page_fault_access(error_code));` | Prints whether the access was a read or write. |
| `println!("  mode: {}", page_fault_mode(error_code));` | Prints whether the fault came from supervisor/kernel mode or user mode. |
| `MALFORMED_TABLE` | Indicates the CPU saw a reserved bit set in a page-table entry. |
| `INSTRUCTION_FETCH` | Indicates the fault happened while fetching an instruction. |
| `fn page_fault_reason(...) -> &'static str` | Converts bit 0 into a readable reason. |
| `PROTECTION_VIOLATION` present | Means a page was present but access permissions were violated. |
| `PROTECTION_VIOLATION` absent | Means the fault was caused by a not-present page. |
| `fn page_fault_access(...) -> &'static str` | Converts bit 1 into `read` or `write`. |
| `CAUSED_BY_WRITE` present | Means the faulting access was a write. |
| `CAUSED_BY_WRITE` absent | Means the faulting access was a read. |
| `fn page_fault_mode(...) -> &'static str` | Converts bit 2 into privilege-level meaning. |
| `USER_MODE` present | Means the fault came from CPL 3 user mode. The current kernel does not have userspace yet. |
| `USER_MODE` absent | Means the fault came from supervisor/kernel mode. |
| `fn yes_no(value: bool) -> &'static str` | Small formatting helper for boolean page-fault details. |
| `if value { "yes" } else { "no" }` | Converts a boolean into stable user-facing text. |
