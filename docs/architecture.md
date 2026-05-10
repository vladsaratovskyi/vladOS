# Kernel Architecture

This guide explains the current kernel at the level of design and data flow. It
is meant to be read before the line-by-line walkthroughs in
[code_walkthrough/](code_walkthrough/README.md).

The project is an educational `x86_64` Rust kernel. It is deliberately small:
there is no heap, no frame allocator, no PIC/APIC setup, no hardware IRQs, no
userspace, and no scheduler yet. The current goal is to make CPU exception
handling understandable and testable.

## What Exists Today

The kernel currently has:

- a `no_std` shared kernel library in `src/lib.rs`
- a `no_main` boot binary in `src/main.rs`
- VGA text output for normal boot
- COM1 serial output for QEMU tests
- QEMU debug-exit support for pass/fail integration tests
- GDT and TSS initialization
- a dedicated double-fault IST stack
- IDT initialization
- handlers for breakpoint, double fault, and page fault exceptions
- one isolated double-fault test kernel
- one isolated page-fault test kernel

For line-by-line details, start with
[kernel_entry.md](code_walkthrough/kernel_entry.md).

## Boot Model

The kernel uses the `bootloader = "0.9"` crate and `cargo bootimage`. The
bootloader creates enough early CPU state to enter the kernel in 64-bit long
mode, then jumps to `_start` in `src/main.rs`.

The kernel is not a normal Rust program:

- `#![no_std]` means Rust's standard library is unavailable because there is no
  operating system underneath us.
- `#![no_main]` means Rust's usual `main` entry point is disabled because the
  bootloader jumps to our `_start` symbol directly.
- `panic = "abort"` means panics do not unwind the stack. In a kernel with no
  runtime, unwinding would need infrastructure we do not have.

The target file `x86_64-blog_os.json` disables the red zone and disables
SIMD/floating-point code generation. Early interrupt handlers must not emit SSE
instructions before the kernel explicitly enables that CPU state.

See [build_config.md](code_walkthrough/build_config.md) for the target and
Cargo configuration line by line.

## Normal Boot Flow

Normal boot follows this sequence:

1. `_start` prints `Hello from Rust OS!` through VGA text mode.
2. `_start` calls `gdt::init()`.
3. `gdt::init()` configures the TSS, builds and loads the GDT, sets `CS`, and
   loads the TSS selector.
4. `_start` calls `interrupts::init_idt()`.
5. `interrupts::init_idt()` installs exception handlers into the IDT and loads
   it with `lidt`.
6. `_start` executes `int3`, intentionally raising a breakpoint exception.
7. The breakpoint handler prints the interrupt stack frame and returns.
8. `_start` prints `Still alive after breakpoint`.
9. `_start` enters `hlt_loop()` forever.

Normal boot does not intentionally double fault or page fault. Those failures
are tested only in separate integration test kernels.

See [kernel_entry.md](code_walkthrough/kernel_entry.md) and
[exceptions.md](code_walkthrough/exceptions.md) for the exact code.

## Shared Library vs Boot Binary

The project has both `src/lib.rs` and `src/main.rs`.

`src/lib.rs` exposes reusable kernel modules:

- `gdt`
- `interrupts`
- `qemu`
- `serial`
- `vga_buffer`
- `hlt_loop`

`src/main.rs` is the normal boot binary. Keeping common setup in the library
lets integration test kernels reuse GDT/TSS setup, serial output, QEMU exit, and
halt behavior without copying production code.

See [kernel_entry.md](code_walkthrough/kernel_entry.md).

## GDT, TSS, And IST

The Global Descriptor Table still matters in long mode even though most classic
segmentation is disabled. The CPU still needs a valid code segment, and it uses
a TSS descriptor to find the Task State Segment.

The Task State Segment is not used for old-style hardware task switching here.
Instead, it provides an Interrupt Stack Table entry for double faults.

The kernel allocates a five-page emergency stack and stores its top address in:

```rust
tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX]
```

Stacks on x86_64 grow downward, so the IST entry points to the end of the stack
buffer, not the beginning.

When a double fault occurs, the CPU switches to that emergency stack before it
calls the double-fault handler. This prevents a stack overflow from immediately
becoming a triple fault reset.

See [cpu_tables.md](code_walkthrough/cpu_tables.md).

## IDT And Exception Handlers

The Interrupt Descriptor Table maps exception vectors to handler functions.
The current production IDT installs:

- vector 3: breakpoint
- vector 8: double fault
- vector 14: page fault

All handlers use `extern "x86-interrupt"`. This ABI tells Rust to generate the
right function prologue and epilogue for CPU exceptions. Ordinary Rust calls use
`ret`; interrupt handlers must restore CPU state with the interrupt-return path.

The breakpoint handler returns normally because `int3` is a recoverable
exception. The double-fault handler and page-fault handler halt because this
kernel cannot recover from them yet.

See [exceptions.md](code_walkthrough/exceptions.md).

## Page Fault Reporting

When the CPU raises a page fault, it stores the faulting virtual address in
`CR2`. The handler reads it through:

```rust
Cr2::read()
```

The CPU also pushes a page-fault error code. The handler decodes the main bits:

- page not present vs protection violation
- read vs write
- supervisor vs user mode
- reserved page-table bit violation
- instruction fetch

At this milestone the kernel reports the fault and halts. It does not allocate
frames, map memory, recover, or retry the instruction.

See [exceptions.md](code_walkthrough/exceptions.md).

## Output Paths

The kernel uses two output mechanisms:

- VGA text mode at memory address `0xb8000` for normal boot.
- COM1 serial port at I/O base `0x3f8` for QEMU tests.

VGA output is useful for visible early boot messages. Serial output is better
for automated tests because QEMU can forward it to the terminal with
`-serial stdio`.

See [output_and_qemu.md](code_walkthrough/output_and_qemu.md).

## QEMU Integration Tests

The tests are harness-free bootable kernels:

- `tests/stack_overflow.rs`
- `tests/page_fault.rs`

They do not use Rust's normal test harness because this is a `no_std` kernel.
Instead, each test file defines its own `_start`.

QEMU pass/fail is reported through the `isa-debug-exit` device. The kernel
writes to I/O port `0xf4`; QEMU exits with a configured process status. Cargo
treats status `33` as success because `0x10` becomes `(0x10 << 1) | 1`.

Each test owns a test-local IDT so success behavior stays out of the production
handlers.

See [tests.md](code_walkthrough/tests.md) and
[build_config.md](code_walkthrough/build_config.md).

## Verification Commands

Use these commands after changing kernel code:

```powershell
cargo +nightly check
cargo +nightly bootimage
cargo +nightly test --test stack_overflow
cargo +nightly test --test page_fault
```

Use this to boot the normal kernel:

```powershell
cargo +nightly run
```

The normal kernel does not exit by itself. It reaches `hlt_loop()` and stays
there.

## Current Boundaries

This documentation describes only the current CPU-exception milestone. It does
not cover future memory management implementation because the kernel does not
yet have:

- bootloader memory-map handling
- page-table editing
- physical frame allocation
- heap allocation
- hardware IRQ setup
- scheduler or userspace

Those belong to later roadmap steps in [GENERAL_PLAN.md](../GENERAL_PLAN.md).
