# Interrupt And Exception Walkthrough

Back to the [architecture guide](../architecture.md) or the
[walkthrough index](README.md).

This page covers `src/interrupts.rs`.

## Purpose

`interrupts.rs` builds and loads the production Interrupt Descriptor Table,
initializes the legacy PIC/PIT interrupt path, and installs handlers for:

- breakpoint exceptions
- double faults
- general-protection faults
- page faults
- timer IRQs
- keyboard IRQs
- the private software yield interrupt used by `scheduler::yield_now()`
- the user-callable syscall interrupt on vector `0x80`

## Dependencies

- `AtomicU64` for the early timer tick counter
- `pic8259::ChainedPics` for remapping and acknowledging the legacy PICs
- `spin::Mutex` for protecting PIC access
- `x86_64::instructions::interrupts` for enabling CPU interrupts
- `x86_64::instructions::port::Port` for PIT and keyboard I/O ports
- `Cr2` to read the virtual address that caused a page fault
- `InterruptDescriptorTable` for IDT construction
- `InterruptStackFrame` for saved CPU state passed to simple Rust handlers
- `crate::arch::x86_64::context` for the low-level timer and yield stubs
- `TrapFrameWithErrorCode` for decoding the explicit #GP entry frame
- `PageFaultErrorCode` for CPU-supplied page-fault details
- `crate::gdt` for the double-fault IST index
- `crate::hlt_loop` for fatal handlers
- `crate::println` for VGA diagnostics

## Invariants

- The IDT must live at a stable address after `lidt`.
- Simple exception and keyboard handlers use `extern "x86-interrupt"`.
- The timer IRQ uses an explicit assembly stub because it may switch tasks.
- The syscall, #GP, and #PF paths use explicit assembly stubs so they can share
  the scheduler's full trap-frame context model.
- The syscall IDT entry must be DPL 3 so user code can execute `int 0x80`.
- The PICs must be remapped away from CPU exception vectors before CPU
  interrupts are enabled.
- Only IRQ0 and IRQ1 are unmasked at this milestone.
- Every hardware IRQ handler must send EOI before returning.
- The breakpoint handler may return.
- The double-fault handler must not return.
- Kernel page faults currently must not return because the kernel cannot
  recover or map missing pages yet.
- #GP and #PF from CPL3 are task-local; the same faults from CPL0 remain fatal.

## Line-By-Line

| Code | Explanation |
| --- | --- |
| `use core::sync::atomic::{AtomicU64, Ordering};` | Imports atomics for the early timer tick counter. The timer handler can update this without heap allocation or locking. |
| `use pic8259::ChainedPics;` | Imports the helper type for a primary and secondary 8259 PIC pair. The dependency is small and keeps the remap and EOI sequence explicit without open-coding every PIC command. |
| `use spin::Mutex;` | Provides a `no_std` lock for the global PIC state. |
| `use x86_64::instructions::{interrupts as cpu_interrupts, port::Port};` | Imports CPU interrupt enable support and I/O port access for PIT and keyboard device ports. |
| `use x86_64::registers::control::Cr2;` | Imports access to the `CR2` register, where the CPU stores the faulting virtual address for page faults. |
| `use x86_64::structures::idt::{...};` | Imports the IDT type and the argument types used by exception handlers. |
| `InterruptDescriptorTable` | The table that maps exception and interrupt vectors to handler functions. |
| `InterruptStackFrame` | A snapshot of CPU state pushed for an exception: instruction pointer, code segment, flags, stack pointer, and stack segment. |
| `PageFaultErrorCode` | A bitflag value pushed by the CPU for page faults. It explains the access type and cause. |
| `use crate::arch::x86_64::context::{self, TrapFrameWithErrorCode};` | Imports addresses for explicit interrupt entry stubs and the #GP/#PF frame layout with an error code. |
| `use crate::{gdt, hlt_loop, println};` | Imports local kernel helpers: GDT constants, halt loop, and VGA output. |
| `use x86_64::PrivilegeLevel;` | Imports DPL values so the syscall IDT entry can allow ring-3 callers. |
| `pub const PIC_1_OFFSET: u8 = 32;` | Chooses vector 32 as the first hardware IRQ vector. CPU vectors `0..31` are reserved for exceptions, so remapping starts after them. |
| `pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;` | Maps the secondary PIC directly after the primary PIC, giving vectors `40..47`. |
| `pub const PIT_FREQUENCY_HZ: u32 = 100;` | Sets a simple 100 Hz timer rate for early ticks and the first preemptive scheduling quantum. There is still no sleep API or timekeeping abstraction. |
| `pub const YIELD_VECTOR: u8 = PIC_2_OFFSET + 8;` | Reserves vector 48 for cooperative `yield_now()`. This is not a PIC IRQ; it is a private software interrupt. |
| `crate::syscall::SYSCALL_VECTOR` | Reserves vector 128 for user `int 0x80` syscalls. |
| `PIT_COMMAND_PORT`, `PIT_CHANNEL_0_PORT`, and `KEYBOARD_DATA_PORT` | Name the I/O ports used for PIT setup and raw keyboard scancode reads. |
| `static PICS: Mutex<ChainedPics> = ...;` | Stores the remapped PIC pair behind a lock so initialization and EOI notifications serialize access to PIC command ports. |
| `static TIMER_TICKS: AtomicU64 = AtomicU64::new(0);` | Global monotonic counter incremented by the timer IRQ handler. |
| `static mut IDT: InterruptDescriptorTable = InterruptDescriptorTable::new();` | Stores the production IDT at a stable global address. It is mutated during boot and then loaded. |
| `pub enum InterruptIndex { ... }` | Gives names to the interrupt vectors the kernel currently handles: timer IRQ0, keyboard IRQ1, private software yield, and syscall. |
| `as_u8()` and `as_usize()` | Convert the enum into the forms needed by PIC EOI calls and IDT indexing. |
| `pub fn init_idt() {` | Public function called by normal boot to initialize exception handling. |
| `let idt = unsafe { &mut *core::ptr::addr_of_mut!(IDT) };` | Gets a mutable reference to the static IDT through a raw pointer. |
| `idt.breakpoint.set_handler_fn(breakpoint_handler);` | Installs the vector 3 breakpoint handler. |
| `idt.page_fault.set_handler_addr(...)` | Installs the explicit vector 14 page-fault entry so user #PF can schedule away from the faulting task. |
| `idt.general_protection_fault.set_handler_addr(...)` | Installs an explicit vector 13 entry so the kernel can inspect the saved CS and contain user-mode #GP faults. |
| `unsafe { idt.double_fault ... }` | Configures the vector 8 double-fault handler and its IST stack. `set_stack_index` is unsafe because the index must be valid in the loaded TSS. |
| `.set_handler_fn(double_fault_handler)` | Sets the double-fault handler function. |
| `.set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);` | Tells the CPU to switch to the dedicated IST stack before calling the double-fault handler. |
| `idt[InterruptIndex::Timer.as_usize()].set_handler_addr(...)` | Installs the explicit low-level IRQ0 entry stub. The timer path can switch tasks, so it cannot use a normal Rust `extern "x86-interrupt"` handler. |
| `idt[InterruptIndex::Yield.as_usize()].set_handler_addr(...)` | Installs the low-level software-yield entry stub used by `scheduler::yield_now()`. |
| `idt[InterruptIndex::Syscall.as_usize()].set_handler_addr(...).set_privilege_level(PrivilegeLevel::Ring3)` | Installs the user-callable `int 0x80` syscall gate. DPL 3 is what allows CPL3 code to invoke it without a #GP. |
| `idt[InterruptIndex::Keyboard.as_usize()].set_handler_fn(keyboard_interrupt_handler);` | Installs the handler for IRQ1 after PIC remapping. |
| `idt.load();` | Executes `lidt` through the `x86_64` crate. After this, the CPU uses this IDT. |
| `println!("IDT initialized");` | Prints confirmation after the IDT is active. |
| `pub fn init_pics()` | Initializes and remaps the legacy PICs. Normal boot calls this after loading the IDT and before enabling CPU interrupts. |
| `pics.initialize();` | Sends the 8259 initialization sequence and applies the configured offsets. |
| `pics.write_masks(0b1111_1100, 0b1111_1111);` | Unmasks only IRQ0 and IRQ1 on the primary PIC. All secondary PIC lines remain masked because this milestone has no handlers for them. |
| `pub fn init_pit()` | Programs PIT channel 0 as the initial timer source. |
| `let divisor = (PIT_BASE_FREQUENCY_HZ / PIT_FREQUENCY_HZ) as u16;` | Converts the PIT base frequency into the divisor for the requested tick rate. |
| `command_port.write(PIT_CHANNEL_0_SQUARE_WAVE);` | Selects channel 0, low-byte/high-byte access, square-wave mode, and binary counting. |
| `channel_0.write(...)` twice | Sends the low byte and high byte of the divisor to PIT channel 0. |
| `pub fn enable_interrupts()` | Enables external interrupts at the CPU after the IDT, PICs, PIT, and early kernel setup are ready. |
| `pub fn timer_ticks() -> u64` | Exposes the current early tick count with relaxed atomic ordering. The preemptive scheduler uses timer delivery as a quantum source but does not expose sleeps or time accounting yet. |
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
| `fn fatal_page_fault(...) -> !` | Prints the production kernel page-fault diagnostics and halts forever. Returning would retry the same faulting instruction. |
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
| `USER_MODE` present | Means the fault came from CPL 3 user mode. Production #PF also checks saved `cs` so user page faults can be contained to the current task. |
| `USER_MODE` absent | Means the fault came from supervisor/kernel mode. |
| `fn yes_no(value: bool) -> &'static str` | Small formatting helper for boolean page-fault details. |
| `if value { "yes" } else { "no" }` | Converts a boolean into stable user-facing text. |
| `pub extern "C" fn timer_interrupt_rust(frame_rsp: u64) -> u64` | Rust half of the timer interrupt path. The assembly stub passes the saved trap-frame stack pointer and expects back the frame pointer to resume. |
| `TIMER_TICKS.fetch_add(1, Ordering::Relaxed);` | Increments the early global tick counter without allocating or locking. |
| `crate::scheduler::on_timer_interrupt(frame_rsp)` | Lets the scheduler save the interrupted task and optionally choose another ready task when preemption is enabled. |
| `notify_end_of_interrupt(InterruptIndex::Timer.as_u8())` | Sends exactly one EOI to the PIC before the selected context is restored. |
| `pub extern "C" fn yield_interrupt_rust(frame_rsp: u64) -> u64` | Rust half of the software-yield path. It shares the scheduler switch logic but does not send PIC EOI because no device IRQ occurred. |
| `pub extern "C" fn syscall_interrupt_rust(frame_rsp: u64) -> u64` | Rust half of the syscall interrupt path. It passes the saved trap frame to `syscall::dispatch`. |
| `pub extern "C" fn general_protection_rust(frame_rsp: u64) -> u64` | Rust half of the explicit #GP path. It receives a frame that includes the CPU-pushed error code. |
| `frame.cs & 0x3 == 0x3` | Checks whether the fault came from CPL3. User-mode #GP marks the current task failed and resumes another task. |
| kernel-mode #GP branch | Prints diagnostics and halts. A kernel privileged-instruction or descriptor fault is still fatal. |
| `pub extern "C" fn page_fault_rust(frame_rsp: u64) -> u64` | Rust half of the explicit #PF path. It reads CR2, decodes the error code, contains CPL3 faults, and keeps CPL0 faults fatal. |
| `extern "x86-interrupt" fn keyboard_interrupt_handler(...)` | Handles keyboard IRQ1. |
| `Port::new(KEYBOARD_DATA_PORT).read()` | Reads one raw scancode byte from port `0x60`. Reading the data port acknowledges the keyboard controller side of the interrupt. |
| `println!("keyboard scancode: {:#04x}", scancode);` | Logs the raw scancode for this milestone. Full key decoding is intentionally deferred. |
| `notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8())` | Sends EOI to the PIC after the scancode is read and logged. |
