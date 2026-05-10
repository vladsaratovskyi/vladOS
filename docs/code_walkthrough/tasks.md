# Kernel Tasks Walkthrough

Back to the [architecture guide](../architecture.md) or the
[walkthrough index](README.md).

This page covers:

- `src/task.rs`
- `src/scheduler.rs`
- `src/arch/x86_64/context.rs`

## Purpose

The task modules provide stackful tasks. Kernel tasks run ring-0 Rust functions;
user tasks start from a prepared trap frame and enter CPL3 through `iretq`. The
first version was cooperative only; the current version keeps `yield_now()` but
also lets the PIT timer preempt a running task when preemption is explicitly
enabled.

There is still no separate user address space, sleep queue, priority
scheduling, or SMP support. Every task has its own kernel stack; user tasks also
have a small mapped user stack.

## Invariants

- Each task owns a dedicated 8 KiB heap-backed kernel stack.
- User tasks use that kernel stack as their `TSS.rsp0` entry stack.
- At most four tasks can be spawned while the heap is fixed at 100 KiB.
- Finished and failed tasks are skipped and never resumed.
- Task stacks are retained for the lifetime of the scheduler.
- Preemption is disabled until `scheduler::enable_preemption()` is called.
- Scheduler mutations run with local interrupts disabled on this single CPU.
- The timer IRQ sends exactly one EOI before the selected task is resumed.

## `src/task.rs`

`TaskId`, `TaskState`, `TaskKind`, and `TaskEntry` keep the public task model
small:

- `TaskId(pub usize)` is assigned from the task table index.
- `TaskState` is `Ready`, `Running`, `Finished`, or `Failed`.
- `TaskKind` distinguishes kernel tasks from user tasks.
- `TaskEntry = fn()` is a plain kernel function.

`Task::new(id, entry, rflags)` allocates an 8 KiB stack and calls
`Context::new_task(...)`. The initial context enters `task_trampoline`, which
calls `scheduler::run_current_task()`. When the task entry returns, the
scheduler marks it `Finished` and switches away without freeing the active
stack.

`Task::new_user(...)` allocates the same 8 KiB kernel stack, then builds an
initial user `TrapFrame` with ring-3 code/data selectors, a user instruction
pointer, a user stack pointer, and an initial argument in `rdi`. The scheduler
restores this frame through `iretq`, so first entry into user mode uses the same
path as every later interrupt return.

## `src/scheduler.rs`

The scheduler is a single global `Scheduler`:

```rust
struct Scheduler {
    tasks: Vec<Task>,
    current: Option<usize>,
    main_context: Context,
    preemption_enabled: bool,
}
```

`tasks` is the fixed-size early task table, `current` tracks the running task,
`main_context` stores the boot stack so `scheduler::run()` can return after all
tasks finish, and `preemption_enabled` gates timer-driven switching.

Public scheduler entry points use `interrupts::without_interrupts(...)` around
critical sections. That is the smallest correct protection for this single-core
milestone: a timer IRQ cannot observe a half-updated run queue or task state.

`spawn(entry)` captures whether interrupts are currently enabled and gives the
new task an initial `rflags` value with IF either set or clear. This lets tests
spawn purely cooperative tasks before enabling interrupts, while the preemptive
test spawns tasks after enabling interrupts so the timer can keep firing after
the first restore.

`run()` chooses the first ready task, marks it running, saves the boot stack, and
updates `TSS.rsp0` to that task's kernel-stack top, saves the boot stack, and
restores the task trap frame. `yield_now()` raises a private software interrupt
instead of calling a callee-saved-only switch routine; this keeps cooperative
and preemptive switching on the same full-context path.

`on_timer_interrupt(frame_rsp)` is called by the timer IRQ stub. It records the
interrupted task frame, selects the next ready task in round-robin order if
preemption is enabled, and returns the stack pointer for the frame that assembly
should resume. `on_yield_interrupt(frame_rsp)` does the same selection without
checking the preemption gate because an explicit yield is always allowed.
`on_syscall_yield(frame_rsp)` reuses the same switching path for user
`int 0x80` yield. `exit_current_from_interrupt(frame_rsp)` and
`fail_current_from_interrupt(frame_rsp)` mark the current task terminal and
select another ready task, returning to the boot stack only when no runnable
task remains.

## `src/arch/x86_64/context.rs`

`Context` stores one saved `rsp`. The data at that `rsp` is a `TrapFrame`:

```rust
pub struct TrapFrame {
    r15, r14, r13, r12, r11, r10, r9, r8,
    rsi, rdi, rbp, rdx, rcx, rbx, rax,
    rip, cs, rflags, rsp, ss,
}
```

The manually saved general-purpose registers come first because the interrupt
stubs push them after the CPU enters the handler. The final five fields are the
CPU return frame. `iretq` always consumes `rip`, `cs`, and `rflags`; it consumes
`rsp` and `ss` on privilege-changing returns, and synthetic task-start frames
include those slots so the same restore routine can enter user tasks.

The CPU always pushes this state on interrupt entry:

- `rip`
- `cs`
- `rflags`

When an interrupt changes privilege level, such as CPL3 user code entering the
kernel, the CPU first switches to `TSS.rsp0` and also pushes:

- `rsp`
- `ss`

The timer and yield stubs then push:

- `rax`
- `rbx`
- `rcx`
- `rdx`
- `rbp`
- `rdi`
- `rsi`
- `r8` through `r15`

`Context::new_user_task(...)` writes the same trap-frame shape, but with a
ring-3 `cs`, ring-3 `ss`, user `rip`, and user `rsp`. The first restore of that
frame is the privilege transition.

`timer_interrupt_entry` passes the trap-frame pointer to `timer_interrupt_rust`.
Rust may return the same frame pointer or another task's saved frame pointer.
Assembly loads that pointer into `rsp`, pops the general-purpose registers, and
uses `iretq` to restore the CPU-pushed return state.

`yield_interrupt_entry` uses the same save and restore code. The only
difference is that it calls `yield_interrupt_rust`, so no PIC EOI is involved.
`syscall_interrupt_entry` also uses this save and restore code for `int 0x80`.

`TrapFrameWithErrorCode` documents the #GP layout. It has the same manually
saved registers as `TrapFrame`, followed by the CPU-pushed error code and return
state. The user #GP path never restores that frame; it marks the task failed and
returns another normal `TrapFrame` instead.

`restore_main_context` is separate because the boot stack is not an interrupt
frame. It restores the small callee-saved frame created by
`switch_from_main_to_task` and returns to the code that called `scheduler::run()`.

## Preemption Flow

The timer scheduling path is:

1. PIT channel 0 raises IRQ0.
2. The remapped PIC delivers vector 32.
3. The CPU enters the IDT entry and pushes the interrupt return frame.
4. `timer_interrupt_entry` saves all general-purpose registers.
5. `timer_interrupt_rust` increments the tick counter.
6. If preemption is enabled and another task is ready, the scheduler records
   the interrupted task frame and selects the next task.
7. The timer path sends EOI to the PIC.
8. Assembly restores the chosen frame and returns through `iretq`.

This prepares the kernel for later userspace work because the scheduler already
has a full interrupted-context representation. Later ring-3 work can extend the
same idea with privilege transitions and address-space state instead of
replacing the stackful task model.
