# Kernel Entry Walkthrough

Back to the [architecture guide](../architecture.md) or the
[walkthrough index](README.md).

This page covers:

- `src/main.rs`: the production boot entry point
- `src/lib.rs`: the shared no-std kernel library

## `src/main.rs`

### Purpose

`src/main.rs` is the normal kernel binary. The bootloader jumps to `_start`,
then `_start` initializes CPU tables, triggers a breakpoint proof, and halts.

### Dependencies

- `core::panic::PanicInfo` for the panic handler argument
- `blog_os::gdt` for GDT/TSS setup
- `blog_os::interrupts` for IDT setup
- `blog_os::println` for VGA output
- `blog_os::hlt_loop` for the final halt state
- `x86_64::instructions::interrupts::int3` for the breakpoint test

### Invariants

- `_start` must never return.
- The panic handler must never return.
- Normal boot must not intentionally double fault or page fault.
- `gdt::init()` must run before any handler that depends on the TSS IST stack.
- `interrupts::init_idt()` must run before `int3()`.

### Line-By-Line

| Code | Explanation |
| --- | --- |
| `#![no_std]` | Disables Rust's standard library. There is no host operating system, so `std` cannot exist. The kernel uses `core` instead. |
| `#![no_main]` | Disables Rust's normal `main` startup path. The bootloader jumps directly to `_start`. |
| `use core::panic::PanicInfo;` | Imports the type passed to the custom panic handler. |
| `use blog_os::{gdt, hlt_loop, interrupts, println};` | Imports shared kernel modules and macros from `src/lib.rs`. This keeps production setup code reusable by tests. |
| `#[no_mangle]` | Prevents Rust from changing the symbol name. The bootloader expects an externally visible `_start` symbol. |
| `pub extern "C" fn _start() -> ! {` | Defines the boot entry point. `extern "C"` gives it a predictable ABI, and `-> !` says it never returns. |
| `println!("Hello from Rust OS!");` | Writes the first visible boot message to VGA text memory. This proves basic output works before CPU table setup. |
| `gdt::init();` | Initializes the Task State Segment, Global Descriptor Table, code segment selector, and TSS selector. |
| `interrupts::init_idt();` | Builds and loads the Interrupt Descriptor Table with exception handlers. |
| `x86_64::instructions::interrupts::int3();` | Executes the `int3` instruction, deliberately raising the breakpoint exception on vector 3. |
| `println!("Still alive after breakpoint");` | Proves the breakpoint handler returned normally and execution continued after `int3`. |
| `hlt_loop();` | Stops active spinning and repeatedly halts the CPU. Normal boot ends here. |
| `}` | Closes `_start`. The function never reaches an ordinary return. |
| `#[panic_handler]` | Marks the following function as the panic handler required by `no_std` binaries. |
| `fn panic(info: &PanicInfo) -> ! {` | Receives panic details and never returns. |
| `println!("{}", info);` | Prints panic information through VGA output if the kernel panics during normal boot. |
| `hlt_loop();` | Halts forever after a panic. There is no unwinding or recovery in this kernel. |
| `}` | Closes the panic handler. |

## `src/lib.rs`

### Purpose

`src/lib.rs` is the shared kernel crate. The production binary and integration
test kernels import common modules from here.

### Dependencies

- `x86_64::instructions::hlt` for the halt loop
- the local modules listed with `pub mod`

### Invariants

- This crate must remain `no_std`.
- It must expose only reusable kernel pieces, not production-only `_start`.
- `hlt_loop()` must never return.

### Line-By-Line

| Code | Explanation |
| --- | --- |
| `#![no_std]` | Keeps the library compatible with bare-metal kernel code. |
| `#![feature(abi_x86_interrupt)]` | Enables the nightly interrupt ABI used by exception handlers in `interrupts.rs` and tests. |
| `pub mod gdt;` | Exposes GDT/TSS setup to the production kernel and test kernels. |
| `pub mod interrupts;` | Exposes the production IDT setup and exception handlers. |
| `pub mod qemu;` | Exposes QEMU debug-exit helpers used by integration tests. |
| `pub mod serial;` | Exposes COM1 serial output used by tests. |
| `pub mod vga_buffer;` | Exposes VGA text output and the `print!`/`println!` macros. |
| `pub fn hlt_loop() -> ! {` | Defines the shared halt loop. It never returns. |
| `loop {` | Repeats forever because the kernel has no shutdown path. |
| `x86_64::instructions::hlt();` | Executes the CPU `hlt` instruction, sleeping until the next interrupt instead of busy-spinning. |
| `}` | Closes the infinite loop. |
| `}` | Closes `hlt_loop()`. |
