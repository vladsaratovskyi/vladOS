# CPU Setup And Exception Tests

> Current docs live in [architecture.md](architecture.md), with detailed
> line-by-line walkthroughs under
> [code_walkthrough/](code_walkthrough/README.md). This file is kept as a
> milestone note for the CPU exception work.

This note documents the earlier CPU setup milestone and the isolated QEMU tests
that prove the double-fault and page-fault handlers work. The current kernel has
since added paging changes and a fixed early heap, but it is still small and
educational on purpose: no hardware IRQs, no PIC or APIC setup, and no
scheduler.

## Normal Boot Path

The normal kernel entry point is `src/main.rs::_start`.

Boot flow:

1. Print `Hello from vladOS!` through VGA text mode.
2. Call `gdt::init()`.
3. Call `interrupts::init_idt()`.
4. Build the active page-table mapper from bootloader `BootInfo`.
5. Print memory diagnostics.
6. Map the fixed heap and initialize the global allocator.
7. Trigger one breakpoint exception with `x86_64::instructions::interrupts::int3()`.
8. Return from the breakpoint handler.
9. Print `Still alive after breakpoint`.
10. Halt forever in `hlt_loop()`.

The normal boot path does not intentionally trigger a double fault. It only uses
`int3` because breakpoint exceptions are recoverable and prove that the IDT is
loaded correctly.

## Shared Kernel Library

The project now has a small `src/lib.rs` so the normal kernel binary and QEMU
integration tests can reuse the same setup code.

It exposes:

- `gdt`: Global Descriptor Table, Task State Segment, and double-fault IST stack.
- `interrupts`: production IDT and CPU exception handlers.
- `memory`: active page-table access and boot-info frame allocation.
- `allocator`: fixed heap mapping and global allocator setup.
- `vga_buffer`: VGA `print!` and `println!` macros.
- `serial`: COM1 serial output used by QEMU tests.
- `qemu`: QEMU debug-exit support and test panic handling.
- `hlt_loop()`: the common halt loop used after fatal paths.

`src/main.rs` remains the production boot entry point. It imports the library
and keeps the boot sequence easy to read.

## GDT And TSS

The GDT setup lives in `src/gdt.rs`.

Even in x86_64 long mode, where most segmentation is disabled, the CPU still
needs a valid code segment and a TSS descriptor. The kernel creates:

- a kernel code segment descriptor
- a TSS descriptor
- selectors for both descriptors

`gdt::init()` performs three main operations:

1. Fill the TSS interrupt stack table.
2. Build the GDT entries.
3. Load the GDT, set `CS`, and load the TSS selector.

The TSS is not used for old-style hardware task switching. In this kernel, its
important job is providing an Interrupt Stack Table entry for double faults.

## Double-Fault IST Stack

`gdt.rs` defines:

```rust
pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;
```

It also allocates a five-page emergency stack:

```rust
static mut DOUBLE_FAULT_STACK: [u8; 4096 * 5] = [0; 4096 * 5];
```

The TSS entry points to the end of that array because x86_64 stacks grow
downward. When a double fault occurs, the CPU switches to this stack before
calling the double-fault handler.

This is important because double faults often happen when the current stack is
already unusable. Without a separate IST stack, a stack overflow can turn into a
triple fault and reset the machine before Rust code gets control.

The CPU tables and stack are `static mut` because they need stable addresses for
the entire kernel lifetime and GDT setup runs before heap initialization.
Mutation is kept during single-threaded initialization.

## IDT And Exception Handlers

The production IDT setup lives in `src/interrupts.rs`.

It installs:

- a breakpoint handler
- a double-fault handler
- a page-fault handler

The breakpoint handler has this shape:

```rust
extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame)
```

It prints `EXCEPTION: BREAKPOINT`, prints the interrupt stack frame, and returns
normally. Returning proves that the CPU can resume after the `int3` instruction.

The double-fault handler has this shape:

```rust
extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) -> !
```

It prints diagnostic information and halts forever. It returns `!` because a
double fault is not safely recoverable in this kernel.

The production double-fault IDT entry uses:

```rust
.set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX)
```

That connects the handler to the dedicated IST stack from the TSS.

The page-fault handler has this shape:

```rust
extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
)
```

When a page fault occurs, the CPU stores the faulting virtual address in `CR2`.
The handler reads `Cr2::read()`, prints the accessed address, prints the raw
`PageFaultErrorCode`, decodes the common bits into readable fields, prints the
interrupt stack frame, and halts forever.

The decoded fields are:

- reason: page not present or protection violation
- access: read or write
- mode: supervisor or user
- reserved bit violation: yes or no
- instruction fetch: yes or no

The handler does not return because this kernel has no frame allocator, demand
paging, or recovery policy yet. Returning would just retry the same faulting
instruction.

## Serial Output

`src/serial.rs` adds minimal COM1 output for tests. Normal boot still uses VGA
output; serial exists so QEMU integration tests can print useful status text
when running with `-serial stdio`.

The serial module provides:

- `serial::init()`
- `serial_print!`
- `serial_println!`

It writes directly to port `0x3f8`.

## QEMU Exit Support

`src/qemu.rs` adds the standard `isa-debug-exit` pattern.

The QEMU exit codes are:

```rust
Success = 0x10
Failed = 0x11
```

The function `exit_qemu()` writes the code to I/O port `0xf4`. With the QEMU
device configured, this terminates QEMU and lets Cargo see whether the test
passed or failed.

The configured success process exit code is `33`, because QEMU reports:

```text
(value << 1) | 1
```

For `0x10`, that is `0x21`, or decimal `33`.

## Bootimage Test Configuration

`Cargo.toml` configures the exception integration tests as harness-free test
kernels:

```toml
[[test]]
name = "stack_overflow"
harness = false

[[test]]
name = "page_fault"
harness = false
```

The normal kernel binary is excluded from Cargo's test harness:

```toml
[[bin]]
name = "vlad_os"
path = "src/main.rs"
test = false
```

The bootimage QEMU test arguments are:

```toml
test-args = [
    "-device", "isa-debug-exit,iobase=0xf4,iosize=0x04",
    "-serial", "stdio",
    "-display", "none",
    "-no-reboot",
]
test-success-exit-code = 33
```

`.cargo/config.toml` enables:

```toml
panic-abort-tests = true
```

This keeps no-std test builds consistent with the kernel's `panic = "abort"`
profiles and avoids building dependencies twice with incompatible panic modes.

## Stack Overflow Test

The isolated test lives in `tests/stack_overflow.rs`.

Its `_start()` does this:

1. Initialize serial output.
2. Print `stack_overflow::stack_overflow...\t`.
3. Call `gdt::init()`.
4. Load a test-local IDT.
5. Recursively call `stack_overflow()`.
6. Panic if execution ever continues.

The test does not call the production `interrupts::init_idt()`. Instead, it owns
a local IDT with a custom double-fault handler that exits QEMU with success.
This keeps test-only behavior out of the production handler.

The recursive function is marked `#[inline(never)]` and includes a volatile read
after the recursive call. That prevents the compiler from optimizing the
recursion into a tail call.

When the stack overflows, the CPU cannot handle the resulting fault on the
broken stack. The double-fault IDT entry switches to the dedicated IST stack,
then calls the test handler.

The test handler:

1. Prints `[ok]`.
2. Calls `exit_qemu(QemuExitCode::Success)`.
3. Enters `hlt_loop()`.

It never returns because double faults are fatal in this kernel.

## Page Fault Test

The isolated page-fault test lives in `tests/page_fault.rs`.

Its `_start()` does this:

1. Initialize serial output.
2. Print `page_fault::invalid_memory_access...\t`.
3. Call `gdt::init()`.
4. Load a test-local IDT with a page-fault handler.
5. Read from `0x4444_4444_0000` with `core::ptr::read_volatile`.
6. Panic if execution ever continues.

The address is canonical on x86_64 but should be unmapped in this early kernel.
The volatile read keeps the compiler from removing the intentionally faulting
memory access.

The test handler reads `CR2`, prints the accessed address, error code, and stack
frame over serial, prints `[ok]`, exits QEMU with `QemuExitCode::Success`, and
then enters `hlt_loop()`.

This keeps the QEMU success path out of the production page-fault handler.

## Commands

Check the kernel:

```powershell
cargo +nightly check
```

Build the normal boot image:

```powershell
cargo +nightly bootimage
```

Run the normal kernel:

```powershell
cargo +nightly run
```

Run the double-fault integration test:

```powershell
cargo +nightly test --test stack_overflow
```

Run the page-fault integration test:

```powershell
cargo +nightly test --test page_fault
```

Expected test output:

```text
stack_overflow::stack_overflow...    [ok]
page_fault::invalid_memory_access... [ok]
```

## Current Limitations

- There is still no full custom test framework.
- There are only focused exception integration test kernels.
- Normal boot has no QEMU exit path, so `cargo run` keeps running until stopped.
- Serial output is intentionally minimal and not interrupt-safe.
- The page-fault handler reports faults but does not recover from them yet.
- Hardware IRQs are not configured or tested yet.
