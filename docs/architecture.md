# Kernel Architecture

This guide explains the current kernel at the level of design and data flow. It
is meant to be read before the line-by-line walkthroughs in
[code_walkthrough/](code_walkthrough/README.md).

The project is an educational `x86_64` Rust kernel. It is deliberately small:
there is now a fixed early heap, a legacy PIC/PIT interrupt foundation, a
stackful task scheduler, and a minimal ring-3 userspace foundation with
per-process address spaces. The current userspace step can load tiny embedded
ELF64 executables into isolated address spaces, safely copy user buffers for
the first byte-oriented syscall, and track single-threaded user processes
through exit and `waitpid`. Dynamic heap growth, demand paging, dynamic
linking, and filesystems are still deferred.

## What Exists Today

The kernel currently has:

- a `no_std` shared kernel library in `src/lib.rs`
- a `no_main` boot binary in `src/main.rs`
- VGA text output for normal boot
- COM1 serial output for QEMU tests
- QEMU debug-exit support for pass/fail integration tests
- GDT and TSS initialization
- ring-0 and ring-3 GDT selectors
- TSS `rsp0` updates for user-to-kernel transitions
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
- PIT-driven preemptive kernel scheduling
- dedicated 8 KiB heap-backed kernel stack per task
- a small round-robin scheduler with explicit `yield_now()` and opt-in
  preemption
- an x86_64 full trap-frame context switch routine for interrupt-time switches
- minimal user tasks entered through `iretq`
- software-interrupt syscalls on vector `0x80`
- user `yield`, `exit`, and `write` syscalls
- checked user-memory range validation and page-by-page copying
- a small process table above the task scheduler
- process IDs, parent/child metadata, zombie state, `getpid`, and exact-child
  `waitpid`
- contained user-mode general-protection faults
- isolated user address spaces with one page-table root per user process
- scheduler CR3 switching between kernel and user address spaces
- contained user-mode page faults
- a strict embedded ELF64 loader for static user executables
- process-style user spawning from ELF entry points
- a narrow `write(fd, user_ptr, len)` path for fd 1 and 2
- one isolated double-fault test kernel
- one isolated page-fault test kernel
- one isolated memory-mapping test kernel
- one isolated heap-allocation test kernel
- one isolated interrupt-foundation test kernel
- one isolated cooperative-task test kernel
- one isolated preemptive-task test kernel
- one isolated userspace test kernel
- one isolated address-space test kernel
- one isolated ELF-loader test kernel
- one isolated user-syscall and checked user-memory test kernel
- one isolated process-lifecycle test kernel

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
- `process`
- `address_space`
- `elf`
- `syscall`
- `user`
- `user_memory`
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
segmentation is disabled. The CPU still needs valid code and stack/data
selectors, and it uses a TSS descriptor to find the Task State Segment.

The Task State Segment is not used for old-style hardware task switching here.
Instead, it provides an Interrupt Stack Table entry for double faults and the
`rsp0` stack pointer used when an interrupt or exception arrives from CPL3.

The kernel allocates a five-page emergency stack and stores its top address in:

```rust
tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX]
```

Stacks on x86_64 grow downward, so the IST entry points to the end of the stack
buffer, not the beginning.

When a double fault occurs, the CPU switches to that emergency stack before it
calls the double-fault handler. This prevents a stack overflow from immediately
becoming a triple fault reset.

The GDT also contains ring-3 user code and data descriptors. User task frames
use those selectors with RPL 3. Whenever the scheduler selects a task, it writes
that task's kernel-stack top into `TSS.rsp0`. If a user task takes a timer IRQ,
syscall interrupt, or user-mode fault, the CPU switches to that kernel stack
before the low-level stub saves registers.

See [cpu_tables.md](code_walkthrough/cpu_tables.md).

## IDT And Exception Handlers

The Interrupt Descriptor Table maps exception and interrupt vectors to handler
functions. The current production IDT installs these exception and software
interrupt entries:

- vector 3: breakpoint
- vector 8: double fault
- vector 13: general protection
- vector 14: page fault
- vector 128: software-interrupt syscall

Simple handlers use `extern "x86-interrupt"`. This ABI tells Rust to generate
the right function prologue and epilogue for CPU exceptions. Paths that can
switch tasks or need the full trap-frame layout use explicit assembly stubs and
return through the shared interrupt-return path.

The breakpoint handler returns normally because `int3` is a recoverable
exception. The double-fault handler and kernel page-fault path halt because this
kernel cannot recover from them yet. General-protection faults are fatal in
kernel mode, but a #GP from CPL3 marks only the current user task failed and
lets the scheduler continue.

See [exceptions.md](code_walkthrough/exceptions.md).

## Legacy Hardware Interrupt Foundation

The current hardware interrupt path uses the legacy 8259 Programmable Interrupt
Controllers and PIT first. This remains intentionally simple while later
milestones build scheduling and userspace on top of it before any APIC
migration.

The legacy PICs deliver hardware IRQs to CPU interrupt vectors. Their default
mapping overlaps CPU exception vectors, so the kernel remaps them before
enabling interrupts:

- primary PIC: IRQ0 through IRQ7 become vectors `32..39`
- secondary PIC: IRQ8 through IRQ15 become vectors `40..47`

The production IDT installs handlers for the two device IRQs this stage
supports:

- IRQ0 timer: vector 32
- IRQ1 keyboard: vector 33

After remapping, the kernel masks every PIC line except IRQ0 and IRQ1. This
keeps later device IRQs disabled until the kernel has handlers for them.

The interrupt flow is:

1. A device asserts an IRQ line.
2. The PIC maps that IRQ to the configured CPU vector.
3. The CPU looks up that vector in the IDT and enters either a simple Rust
   `extern "x86-interrupt"` handler or the explicit timer assembly stub.
4. The handler does the small amount of work for that device.
5. The handler sends End Of Interrupt to the PIC so another IRQ can be
   delivered.

The PIT is programmed through ports `0x43` and `0x40` for a 100 Hz channel 0
timer. The timer path increments an `AtomicU64` tick counter, optionally asks
the scheduler to switch tasks when preemption is enabled, and sends EOI. There
is still no sleeping API or timekeeping abstraction.

The keyboard handler reads one raw scancode byte from I/O port `0x60`, prints it
in hexadecimal, and sends EOI. It deliberately does not decode keymaps or build
input queues yet.

See [exceptions.md](code_walkthrough/exceptions.md) for the handler code.

## Tasks

The task foundation is stackful. A task is the schedulable execution unit. It
is either a kernel function or the main task of a single-threaded user process:

- a `TaskId`
- a `TaskState`: `Ready`, `Running`, `Blocked`, `Finished`, or `Failed`
- a saved CPU `Context`
- a dedicated heap-backed kernel stack
- either a kernel task entry function or user entry/stack metadata
- for user tasks, the `ProcessId` of the owning process

Each task gets an 8 KiB kernel stack. That size is deliberately small because
the current heap is only 100 KiB and this milestone caps the initial task table
at eight tasks. For user tasks, this stack is also the ring-0 entry stack named
by `TSS.rsp0`; the user stack is mapped separately in user-accessible pages.

New task stacks are prepared with an interrupt-return frame that enters a task
trampoline. The trampoline calls the current task entry function. When that
function returns, the trampoline marks the task `Finished` and switches away;
finished tasks are skipped and not resumed. Task stacks are retained for the
lifetime of the scheduler, so the kernel does not free a stack while still
executing on it.

The original cooperative context was enough only at a known function-call
boundary. Timer preemption can interrupt arbitrary code, so the current task
context stores a full trap frame: `rax`, `rbx`, `rcx`, `rdx`, `rbp`, `rdi`,
`rsi`, `r8` through `r15`, plus the CPU-pushed return state `rip`, `cs`,
`rflags`, `rsp`, and `ss`. The assembly restore path pops the general-purpose
registers and returns through `iretq`.

The `yield_now()` path is:

1. The running task calls `scheduler::yield_now()`.
2. `yield_now()` raises a private software interrupt vector.
3. The low-level interrupt stub saves the same full trap frame used by timer
   preemption.
4. The scheduler finds the next `Ready` task in round-robin order.
5. The current task becomes `Ready`; the selected task becomes `Running`.
6. The restore path resumes the selected task through `iretq`.

The preemptive timer path is gated. Timer ticks always increment the global
counter and send PIC EOI, but the timer IRQ asks the scheduler for a new task
only after task state exists and `scheduler::enable_preemption()` has been
called. The first quantum is intentionally simple: one scheduler decision per
PIT tick. Scheduler mutations run with local interrupts disabled, which is
enough for this single-core kernel and avoids adding SMP or blocking locks.

The timer scheduling flow is:

1. PIT channel 0 asserts IRQ0.
2. The remapped PIC delivers vector 32.
3. The CPU enters the IDT entry and pushes the interrupt return state. If the
   interrupted task was in CPL3, it first switches to `TSS.rsp0` and includes
   the old user `rsp` and `ss`.
4. The low-level timer stub saves all general-purpose registers.
5. Rust increments the tick counter, optionally records the interrupted task
   frame, and chooses the next ready task.
6. Rust sends the timer EOI exactly once.
7. Assembly restores the selected trap frame and returns through `iretq`.

See [tasks.md](code_walkthrough/tasks.md).

## Processes, Userspace, And Address Spaces

The scheduler still schedules tasks, not processes. The process layer owns
longer-lived user resources and lifecycle metadata:

- `ProcessId`
- optional parent PID
- child PID list
- `Running` or `Zombie` state
- process exit reason
- the process address space
- the process main task

This milestone supports only one user task per process. That keeps the process
model small while giving later `fork`, `exec`, and file-descriptor work a
natural owner for address spaces and lifecycle state.

User processes no longer share the kernel page table. Each process owns an
`AddressSpace`, which is a fresh level-4 page-table frame. Kernel tasks
continue using the boot/kernel address space.

New user address spaces are built by reserving P4 index 1 for user mappings and
copying the other kernel P4 entries from the boot address space. Copied kernel
top-level entries have `USER_ACCESSIBLE` cleared, so kernel text/data, heap,
kernel stacks, device mappings, and the bootloader physical-memory direct map
remain available to CPL0 but inaccessible to CPL3. User code/data/stack pages
are mapped under P4 index 1 with `USER_ACCESSIBLE`.

The resulting layout is:

```text
kernel P4 entries: shared supervisor-only kernel mappings
user P4 index 1:   per-process user code/data/stack

task A USER_DATA_BASE -> frame A
task B USER_DATA_BASE -> frame B
```

User task creation builds an initial `TrapFrame` on the task's kernel stack:

- `rip`: user entry point
- `cs`: ring-3 user code selector with RPL 3
- `rflags`: reserved bit set and interrupts enabled when the task was spawned
- `rsp`: top of the mapped user stack
- `ss`: ring-3 user data selector with RPL 3

The scheduler restores that frame through the same `iretq` path used by
preemptive task switching. There is no separate entry mechanism for the first
CPL3 transition.

Syscalls use `int 0x80`. The IDT entry is present at DPL 3, so user code may
invoke it directly. The low-level syscall stub saves the same full trap frame as
timer/yield, then Rust dispatch reads the syscall number from `rax`:

- `0`: yield through the scheduler's existing switch path
- `1`: mark the current process zombie with the `rdi` exit code, finish its
  main task, wake any blocked parent, and schedule another task
- `2`: copy bytes from a checked user buffer to the serial-backed output path
- `3`: return the current process ID
- `4`: wait for one exact child PID

This is intentionally not `syscall/sysret`; software interrupts reuse the
kernel's current trap-frame machinery and keep this step focused on safe
privilege transitions.

A user-mode privileged instruction such as `hlt` raises #GP. A user access to
an unmapped page or a supervisor-only kernel mapping raises #PF. The kernel
checks the saved `cs` privilege bits. If the fault came from CPL3, the current
task is marked `Failed`, the owning process becomes
`Zombie(ProcessExit::Faulted)`, fault details are recorded, and another ready
task is resumed. If the same exception came from CPL0, the kernel preserves the
fatal behavior and halts.

`waitpid` is deliberately narrow. It supports only exact positive child PIDs,
options `0` and `WNOHANG`, and a temporary project-local status structure:

```rust
#[repr(C)]
pub struct UserWaitStatus {
    pub kind: u32, // 0 = exited, 1 = faulted
    pub code: i32, // exit code for normal exits
}
```

If the child is still running and `WNOHANG` is set, `waitpid` returns `0`. If
the child is still running and options are `0`, the parent task is marked
`Blocked`; it is skipped by round-robin selection until the child exits. Child
exit completes the wait by copying status to checked user memory, reaping the
zombie process, patching the parent's saved trap-frame `rax` to the child PID,
and marking the parent task `Ready`. A bad status pointer returns `-EFAULT`
without reaping the child. Non-child or already reaped PIDs return `-ECHILD`.

Timer IRQs can also arrive while user code is running. The CPU uses the current
task's `TSS.rsp0` to enter ring 0, the timer stub saves the interrupted frame,
and the scheduler may load a different CR3 before resuming a kernel task,
another user task, or the same task later.

See [userspace.md](code_walkthrough/userspace.md),
[address_spaces.md](code_walkthrough/address_spaces.md), and
[process_lifecycle.md](code_walkthrough/process_lifecycle.md).

## Embedded ELF Loader

The current loader is an in-kernel loader for embedded test binaries, not a
filesystem-backed `execve`. Test fixtures are generated at build time as small
ELF64 byte arrays and embedded with `include_bytes!`. The kernel parses those
bytes directly.

The supported ELF subset is intentionally narrow:

- ELF64
- little-endian
- `ET_EXEC`
- `EM_X86_64`
- current ELF version
- `PT_LOAD` program headers only

The loader rejects malformed or unsupported files instead of guessing. It
checks header bounds, segment sizes, page alignment, overlapping mappings,
load-range limits, and whether the entry point lies inside an executable load
segment.

For each `PT_LOAD` segment, the loader creates mappings in a fresh
`AddressSpace`, copies the file-backed bytes through the kernel direct map, and
zeros the remaining `p_memsz - p_filesz` range. Segment `PF_W` controls whether
the leaf PTE is writable. Execute/read permissions are tracked by loader
validation; this kernel has not enabled NX yet, so non-executable mappings are
documented but not enforced in hardware at this milestone.

After loading segments, the loader maps the existing 8 KiB user stack and
returns a `UserTaskInit` with:

- `rip = e_entry`
- `rsp = USER_STACK_TOP`
- the process's fresh address-space root
- the initial `rdi` argument used by tests

`scheduler::spawn_user_elf` creates a root process, registers its main task
with the existing scheduler, and keeps syscalls, user page-fault containment,
and timer preemption on the same trap-frame and CR3-switching paths as earlier
user tasks.

See [elf_loader.md](code_walkthrough/elf_loader.md).

## User Memory And `write`

The first useful user-facing syscall is intentionally small:

```text
rax = 2              syscall number
rdi = fd             1 for stdout, 2 for stderr
rsi = user pointer   byte buffer in the current user address space
rdx = len            byte count
int 0x80
```

Return values come back in `rax`. Successful syscalls return a non-negative byte
count. Errors return negative errno-like values such as `-EBADF`, `-EFAULT`,
`-EINVAL`, or `-ENOSYS`.

Syscall pointers are never trusted as kernel pointers. The checked user-memory
helper validates the requested range, walks every touched page in the current
task's `AddressSpace`, checks user accessibility and write permission when
needed, translates each chunk to a physical address, and copies through the
bootloader direct physical-memory mapping.

This preserves a useful distinction:

- direct illegal user access, such as user code writing to read-only memory,
  still raises a user #PF and fails only that task
- an illegal pointer passed to `write` returns `-EFAULT`, and the user task may
  continue

The current `write` accepts only fd 1 and fd 2, treats the buffer as arbitrary
bytes, and routes both to COM1 serial output. There is no file descriptor table,
VFS, blocking I/O, `read`, `open`, or `close` yet.

See [user_memory_and_write.md](code_walkthrough/user_memory_and_write.md).

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

Kernel-mode page faults are reported and halt. User-mode page faults are
contained to the current task and do not allocate frames, demand-map memory, or
retry the instruction.

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
- `tests/preemptive_tasks.rs`
- `tests/userspace.rs`
- `tests/address_spaces.rs`
- `tests/elf_loader.rs`
- `tests/user_syscalls.rs`
- `tests/process_lifecycle.rs`

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
exits QEMU after both tasks finish. The preemptive task test enables PIT-driven
preemption, runs two tasks that do not call `yield_now()` during the proof, and
exits only after both task-local counters make progress and one task finishes.
The userspace test creates isolated user tasks, enters CPL3, checks `yield` and
`exit` syscalls, contains a user-mode privileged-instruction fault, and proves a
busy user loop can be preempted by the PIT. The address-space test proves that
two user tasks can reuse the same virtual addresses with independent physical
memory, that unmapped user pages and kernel-only mappings fault only the
current task, and that preemption still works across CR3 switches. The
ELF-loader test spawns embedded ELF user programs, checks exit codes and private
data mappings, rejects bad ELF headers, contains a read-only segment write
fault, and proves PIT preemption still crosses ELF-backed CR3 roots. The
user-syscall test checks `write`, recoverable bad syscall pointers, read-only
write sources, cross-page buffers, checked kernel-to-user copying, unchanged
direct user fault semantics, and preemption across syscall-heavy user tasks.
The process-lifecycle test checks process IDs, parent/child relationships,
blocking and nonblocking `waitpid`, zombie persistence before reap, bad status
pointers, child fault status, and continued scheduling after child exit.

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
cargo +nightly test --test preemptive_tasks
cargo +nightly test --test userspace
cargo +nightly test --test address_spaces
cargo +nightly test --test elf_loader
cargo +nightly test --test user_syscalls
cargo +nightly test --test process_lifecycle
```

Use this to boot the normal kernel:

```powershell
cargo +nightly run
```

The normal kernel does not exit by itself. It reaches `hlt_loop()` and stays
there.

## Current Boundaries

This documentation describes only the current CPU-exception, memory-foundation,
legacy interrupt-foundation, task, and isolated userspace milestones. The
kernel still does not have:

- heap growth or physical frame reclamation
- APIC setup
- kernel stack guard pages
- demand paging
- dynamic linking
- filesystem-backed `execve`
- copy-on-write
- a broad syscall API
- file descriptor tables, `read`, `open`, and `close`
- `fork`, `exec` replacement, signals, process groups, and multiple user
  threads per process
- keyboard decoding or input queues

Those belong to later roadmap steps in [GENERAL_PLAN.md](../GENERAL_PLAN.md).
