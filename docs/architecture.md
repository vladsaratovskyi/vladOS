# Kernel Architecture

This guide explains the current kernel at the level of design and data flow. It
is meant to be read before the line-by-line walkthroughs in
[code_walkthrough/](code_walkthrough/README.md).

The project is an educational `x86_64` Rust kernel. It is deliberately small:
there is now a fixed early heap, a legacy PIC/PIT interrupt foundation, and a
stackful cooperative task foundation. The current goal is to make voluntary
kernel task switching work without jumping ahead to timer-driven preemption,
userspace, dynamic heap growth, or filesystems.

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
- bootloader `BootInfo` handling in the normal kernel entry point
- bootloader-provided physical memory map access
- bootloader-provided direct physical-memory offset mapping
- active level-4 page table access through `CR3`
- an `OffsetPageTable` mapper over the current page-table hierarchy
- a simple monotonic physical frame allocator for usable 4 KiB frames
- a fixed-size kernel heap at virtual address `0x5555_5555_0000`
- a global allocator backed by `linked_list_allocator`
- `alloc` crate support for kernel `Box` and `Vec`
- legacy 8259 PIC remapping for hardware IRQs
- PIT channel 0 timer interrupts at 100 Hz
- an atomic early timer tick counter
- keyboard IRQ handling with raw scancode reads from port `0x60`
- interrupt-safe VGA and serial output
- stackful cooperative kernel tasks
- dedicated 8 KiB heap-backed kernel stack per task
- a small round-robin scheduler with explicit `yield_now()`
- an x86_64 cooperative context switch routine
- one isolated double-fault test kernel
- one isolated page-fault test kernel
- one isolated memory-mapping test kernel
- one isolated heap-allocation test kernel
- one isolated interrupt-foundation test kernel
- one isolated cooperative-task test kernel

For line-by-line details, start with
[kernel_entry.md](code_walkthrough/kernel_entry.md).

## Boot Model

The kernel uses the `bootloader = "0.9"` crate and `cargo bootimage`. The
bootloader creates enough early CPU state to enter the kernel in 64-bit long
mode, builds a `BootInfo` structure, then jumps to the `_start` symbol generated
by `bootloader::entry_point!` in `src/main.rs`.

The kernel is not a normal Rust program:

- `#![no_std]` means Rust's standard library is unavailable because there is no
  operating system underneath us.
- `#![no_main]` means Rust's usual `main` entry point is disabled because the
  bootloader jumps to our `_start` symbol directly.
- `panic = "abort"` means panics do not unwind the stack. In a kernel with no
  runtime, unwinding would need infrastructure we do not have.
- `BootInfo` gives the kernel the memory map and the direct physical-memory
  offset selected by the bootloader.

The target file `x86_64-vlad_os.json` disables the red zone and disables
SIMD/floating-point code generation. Early interrupt handlers must not emit SSE
instructions before the kernel explicitly enables that CPU state.

See [build_config.md](code_walkthrough/build_config.md) for the target and
Cargo configuration line by line.

## Normal Boot Flow

Normal boot follows this sequence:

1. `_start` prints `Hello from vladOS!` through VGA text mode.
2. `_start` calls `gdt::init()`.
3. `gdt::init()` configures the TSS, builds and loads the GDT, sets `CS`, and
   loads the TSS selector.
4. `_start` calls `interrupts::init_idt()`.
5. `interrupts::init_idt()` installs exception and hardware IRQ handlers into
   the IDT and loads it with `lidt`.
6. `_start` calls `interrupts::init_pics()` to remap the legacy PICs and
   unmask only timer and keyboard IRQs.
7. `_start` calls `interrupts::init_pit()` to program PIT channel 0.
8. `_start` reads `boot_info.physical_memory_offset` and creates an
   `OffsetPageTable` for the active page-table hierarchy.
9. `_start` prints compact memory diagnostics: the physical-memory offset,
   selected virtual-to-physical translations, and the usable region count.
10. `_start` creates the boot-info frame allocator, maps the fixed heap pages,
   initializes the global allocator, and prints `Heap initialized`.
11. `_start` enables CPU interrupts.
12. `_start` executes `int3`, intentionally raising a breakpoint exception.
13. The breakpoint handler prints the interrupt stack frame and returns.
14. `_start` prints `Still alive after breakpoint`.
15. `_start` spawns two demo kernel tasks and starts the cooperative scheduler.
16. Each demo task prints short progress, calls `yield_now()`, resumes on its
    own stack, and eventually returns.
17. `_start` prints that the cooperative task demo completed.
18. `_start` enters `hlt_loop()` forever.

Normal boot does not intentionally double fault or page fault. Those failures
are tested only in separate integration test kernels.

See [kernel_entry.md](code_walkthrough/kernel_entry.md) and
[exceptions.md](code_walkthrough/exceptions.md) for the exact code.

## Memory Management Foundation

The current memory milestone uses the bootloader's direct physical-memory
mapping instead of designing a final custom higher-half layout. With the
`map_physical_memory` bootloader feature enabled, the bootloader maps all
physical memory at a runtime virtual offset and stores that offset in
`BootInfo::physical_memory_offset`.

The current strategy is:

- physical addresses are accessed as `physical_memory_offset + physical_address`
- the active level-4 page table is found by reading `CR3`
- the level-4 table's physical frame is accessed through the direct physical
  mapping
- `OffsetPageTable` edits the active hierarchy through that mapping
- only regions marked `MemoryRegionType::Usable` are handed out by the early
  frame allocator
- the fixed heap virtual range is mapped to fresh usable physical frames before
  the allocator receives it

The heap starts at `0x5555_5555_0000` and is 100 KiB. This avoids the
`0x4444_4444_0000` scratch page used by the page-fault and memory-mapping tests.
The allocator manages virtual heap memory only; paging has already assigned
physical frames to every heap page. The heap does not grow, reclaim physical
frames, demand-map pages, or use slabs yet.

The frame allocator is deliberately simple. It walks the bootloader memory map,
turns usable regions into 4 KiB frames, and returns the `next` frame on each
allocation. It never frees frames and is not efficient, but it is enough for
early page-table and fixed heap work.

See [memory.md](code_walkthrough/memory.md) and
[allocator.md](code_walkthrough/allocator.md).

## Shared Library vs Boot Binary

The project has both `src/lib.rs` and `src/main.rs`.

`src/lib.rs` exposes reusable kernel modules:

- `gdt`
- `interrupts`
- `allocator`
- `task`
- `scheduler`
- `arch`
- `memory`
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

The Interrupt Descriptor Table maps exception and interrupt vectors to handler
functions. The current production IDT installs these CPU exception entries:

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

## Legacy Hardware Interrupt Foundation

The current hardware interrupt path uses the legacy 8259 Programmable Interrupt
Controllers and PIT first. This is intentionally simple and matches the roadmap
step before APIC, scheduling, or userspace work.

The legacy PICs deliver hardware IRQs to CPU interrupt vectors. Their default
mapping overlaps CPU exception vectors, so the kernel remaps them before
enabling interrupts:

- primary PIC: IRQ0 through IRQ7 become vectors `32..39`
- secondary PIC: IRQ8 through IRQ15 become vectors `40..47`

The production IDT installs handlers for the two IRQs this milestone supports:

- IRQ0 timer: vector 32
- IRQ1 keyboard: vector 33

After remapping, the kernel masks every PIC line except IRQ0 and IRQ1. This
keeps later device IRQs disabled until the kernel has handlers for them.

The interrupt flow is:

1. A device asserts an IRQ line.
2. The PIC maps that IRQ to the configured CPU vector.
3. The CPU looks up that vector in the IDT and enters the `extern
   "x86-interrupt"` handler.
4. The handler does the small amount of work for that device.
5. The handler sends End Of Interrupt to the PIC so another IRQ can be
   delivered.

The PIT is programmed through ports `0x43` and `0x40` for a 100 Hz channel 0
timer. The timer handler increments an `AtomicU64` tick counter and sends EOI.
There is no timer-driven scheduler, sleeping API, or timekeeping abstraction
yet.

The keyboard handler reads one raw scancode byte from I/O port `0x60`, prints it
in hexadecimal, and sends EOI. It deliberately does not decode keymaps or build
input queues yet.

See [exceptions.md](code_walkthrough/exceptions.md) for the handler code.

## Cooperative Kernel Tasks

The task foundation is stackful and cooperative. A task is a kernel function
plus scheduler metadata:

- a `TaskId`
- a `TaskState`: `Ready`, `Running`, or `Finished`
- a saved CPU `Context`
- a dedicated heap-backed kernel stack
- a task entry function

Each task gets an 8 KiB stack. That size is deliberately small because the
current heap is only 100 KiB and this milestone caps the initial task table at
four tasks. It is enough for the current demo and QEMU test, but it is not a
final stack-sizing policy.

New task stacks are prepared so the first context switch restores zeroed
callee-saved registers and returns into a task trampoline. The trampoline calls
the current task entry function. When that function returns, the trampoline
marks the task `Finished` and switches away; finished tasks are skipped and not
resumed. Task stacks are retained for the lifetime of the scheduler, so the
kernel does not free a stack while still executing on it.

The cooperative context switch saves the x86_64 SysV callee-saved registers:

- `rbp`
- `rbx`
- `r12`
- `r13`
- `r14`
- `r15`
- `rsp`

The instruction pointer is restored through `ret`. Caller-saved registers are
not preserved by the switch because `yield_now()` is an ordinary function-call
boundary; Rust already treats caller-saved registers as clobberable across that
call.

The `yield_now()` path is:

1. The running task calls `scheduler::yield_now()`.
2. The scheduler finds the next `Ready` task in round-robin order.
3. The current task becomes `Ready`; the selected task becomes `Running`.
4. The low-level context switch saves the old stack pointer and callee-saved
   registers, loads the next task's stack pointer, restores its saved registers,
   and returns into that task.

The PIT timer interrupt does not call the scheduler yet. This keeps the current
milestone cooperative only, while the saved context and dedicated task stacks
are shaped so a later preemptive timer path can reuse them.

See [tasks.md](code_walkthrough/tasks.md).

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

Both output paths are protected with `spin::Mutex` and wrapped in
`without_interrupts`. This prevents a hardware interrupt handler from trying to
print while normal code is already holding the same output lock. The tradeoff is
that printing briefly delays local interrupt delivery, which is acceptable for
this single-core educational stage but not a final logging design.

See [output_and_qemu.md](code_walkthrough/output_and_qemu.md).

## QEMU Integration Tests

The tests are harness-free bootable kernels:

- `tests/stack_overflow.rs`
- `tests/page_fault.rs`
- `tests/memory_mapping.rs`
- `tests/heap_allocation.rs`
- `tests/interrupts.rs`
- `tests/cooperative_tasks.rs`

They do not use Rust's normal test harness because this is a `no_std` kernel.
Instead, each test file defines or generates its own `_start`.

QEMU pass/fail is reported through the `isa-debug-exit` device. The kernel
writes to I/O port `0xf4`; QEMU exits with a configured process status. Cargo
treats status `33` as success because `0x10` becomes `(0x10 << 1) | 1`.

Each exception-oriented test owns a test-local IDT so success behavior stays out
of the production handlers. The memory-mapping test also installs a test-local
page-fault handler, but success comes from explicitly mapping one scratch page,
writing through it, and reading the value back. The heap-allocation test
initializes the same memory mapper and frame allocator, maps the fixed heap, and
then verifies `Box`, `Vec` growth, and repeated allocation/deallocation. The
interrupt test initializes the production IDT, remapped PICs, and PIT, then
checks the public interrupt indexes and initial tick counter without enabling
external interrupts or waiting on timer delivery. The cooperative task test
initializes the fixed heap, spawns two tasks, verifies deterministic explicit
yield ordering, checks that task-local state survives context switches, and
exits QEMU after both tasks finish.

See [tests.md](code_walkthrough/tests.md) and
[build_config.md](code_walkthrough/build_config.md).

## Verification Commands

Use these commands after changing kernel code:

```powershell
cargo +nightly check
cargo +nightly bootimage
cargo +nightly test --test stack_overflow
cargo +nightly test --test page_fault
cargo +nightly test --test memory_mapping
cargo +nightly test --test heap_allocation
cargo +nightly test --test interrupts
cargo +nightly test --test cooperative_tasks
```

Use this to boot the normal kernel:

```powershell
cargo +nightly run
```

The normal kernel does not exit by itself. It reaches `hlt_loop()` and stays
there.

## Current Boundaries

This documentation describes only the current CPU-exception, memory-foundation,
legacy interrupt-foundation, and cooperative task-foundation milestones. The
kernel still does not have:

- heap growth or physical frame reclamation
- APIC setup
- timer-driven preemption
- kernel stack guard pages
- userspace
- keyboard decoding or input queues

Those belong to later roadmap steps in [GENERAL_PLAN.md](../GENERAL_PLAN.md).
