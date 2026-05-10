# Cooperative Tasks Walkthrough

Back to the [architecture guide](../architecture.md) or the
[walkthrough index](README.md).

This page covers:

- `src/task.rs`
- `src/scheduler.rs`
- `src/arch/x86_64/context.rs`

## Purpose

The task modules provide the first stackful kernel task foundation. Tasks are
cooperative: they run until they explicitly call `scheduler::yield_now()` or
return from their entry function. Timer interrupts do not preempt tasks yet.

## Invariants

- Each task owns a dedicated 8 KiB heap-backed kernel stack.
- At most four tasks can be spawned in this milestone.
- Finished tasks are skipped and never resumed.
- Task stacks are retained for the lifetime of the scheduler.
- The low-level switch saves only x86_64 callee-saved registers plus `rsp`.
- No scheduler lock is held across a context switch.

## `src/task.rs`

### Line-By-Line

| Code | Explanation |
| --- | --- |
| `use alloc::{boxed::Box, vec};` | Imports heap-backed storage for task stacks. The heap must already be initialized before tasks are spawned. |
| `use crate::arch::x86_64::context::Context;` | Imports the saved CPU context type used by the scheduler. |
| `pub const TASK_STACK_SIZE: usize = 8 * 1024;` | Gives each early task an 8 KiB stack. This is intentionally small because the current heap is 100 KiB. |
| `pub const MAX_TASKS: usize = 4;` | Caps the first scheduler at four tasks so task stacks cannot consume the whole fixed heap. |
| `pub type TaskEntry = fn();` | A task entry is a plain Rust kernel function that takes no arguments and returns normally when the task is done. |
| `pub struct TaskId(pub usize);` | Stable identifier assigned from the task table index. IDs are not recycled yet. |
| `pub enum TaskState` | Tracks whether a task is `Ready`, `Running`, or `Finished`. |
| `pub struct Task { ... }` | Stores the ID, state, saved context, entry function, and owned stack. |
| `_stack: Box<[u8]>` | Keeps the stack allocation alive even though the scheduler normally touches only the saved `rsp`. The leading underscore documents that ownership is the important part. |
| `Task::new(id, entry)` | Allocates the stack and prepares the initial context. |
| `vec![0u8; TASK_STACK_SIZE].into_boxed_slice()` | Allocates and zero-fills the dedicated stack. |
| `Context::new_task(&mut stack, task_trampoline as *const () as usize)` | Builds the first saved stack frame so the context switch returns into the trampoline. |
| `set_state`, `entry`, `context`, and `context_mut` | Small scheduler-facing helpers that keep task fields private outside the module. |
| `extern "C" fn task_trampoline() -> !` | First code reached by a new task. It delegates to the scheduler so the current task entry can be called and completion can be handled in one place. |

## `src/scheduler.rs`

### Line-By-Line

| Code | Explanation |
| --- | --- |
| `static mut SCHEDULER: Scheduler = Scheduler::new();` | Stores the single-core scheduler. This milestone has no SMP or preemption, so access is isolated behind small unsafe helpers instead of a lock held across switches. |
| `pub enum SpawnError { Full }` | The only recoverable spawn failure is exceeding `MAX_TASKS`. Heap exhaustion still uses the kernel allocation error path. |
| `struct Scheduler { tasks, current, main_context }` | Holds the task table, current task index, and saved boot/main context. |
| `tasks: Vec<Task>` | Dynamic task table backed by the fixed heap. |
| `current: Option<usize>` | Index of the running task, or `None` while running on the boot/main stack. |
| `main_context: Context` | Saved stack pointer for the boot/main code so the scheduler can return after all tasks finish. |
| `spawn(entry)` | Creates a task if the fixed task cap has not been reached. |
| `next_ready_after(start)` | Finds the next `Ready` task in round-robin order. |
| `first_ready()` | Chooses the first task when `run()` starts from the boot/main stack. |
| `run()` | Switches from the boot/main context into the first ready task. It returns only after no runnable tasks remain. |
| `yield_now()` | Voluntary switch point. The current task becomes `Ready`, the next ready task becomes `Running`, and the context switch transfers control. |
| `finished_task_count()` and `all_tasks_finished()` | Test/debug helpers used by the QEMU task test. |
| `run_current_task() -> !` | Called by the task trampoline. It reads the current task's entry function, calls it, and then marks the task finished. |
| `finish_current_task() -> !` | Marks the current task `Finished` and switches either to another ready task or back to `main_context`. It never intentionally returns to the finished task. |
| `switch_between_tasks(...)` | Extracts raw context pointers and calls the architecture switch routine. |
| `scheduler_mut()` | Centralizes the unsafe mutable access to the global scheduler. |

## `src/arch/x86_64/context.rs`

### Line-By-Line

| Code | Explanation |
| --- | --- |
| `use core::arch::global_asm;` | Allows one isolated assembly routine without adding a dependency. |
| `#[repr(C)] pub struct Context { rsp: u64 }` | The saved context is currently just the saved stack pointer. The rest of the saved registers live on that stack. |
| `Context::empty()` | Creates a blank context for the boot/main stack before the first switch saves its real `rsp`. |
| `Context::new_task(stack, entry_point)` | Prepares a new task stack as if it had already yielded once and can be resumed by the switch routine. |
| `STACK_ALIGN = 16` | x86_64 SysV code expects stack alignment rules around function calls. |
| `INITIAL_FRAME_SIZE = 8 * size_of::<u64>()` | Reserves six saved registers, one return address, and one padding slot. |
| `stack_top & !(STACK_ALIGN - 1)` | Aligns the high end of the stack down to a 16-byte boundary. |
| `frame_bottom = stack_top - INITIAL_FRAME_SIZE` | Leaves one padding word so the trampoline begins with `rsp % 16 == 8`, matching normal function entry. |
| `frame.add(0)..frame.add(5)` | Initial saved `r15`, `r14`, `r13`, `r12`, `rbx`, and `rbp`, all zeroed for a new task. |
| `frame.add(6).write(entry_point as u64)` | Return address used by `ret` after the switch restores the saved registers. |
| `frame.add(7).write(0)` | Padding for correct stack alignment at trampoline entry. |
| `global_asm!(...)` | Defines the architecture-specific switch in one place. |
| `push rbp`, `push rbx`, `push r12`..`push r15` | Saves the callee-saved registers required across a cooperative function-call boundary. |
| `mov [rdi], rsp` | Stores the old task's stack pointer into `old_context.rsp`. |
| `mov rsp, [rsi]` | Loads the next task's saved stack pointer. |
| `pop r15`..`pop rbp` | Restores the next task's callee-saved registers from its stack. |
| `ret` | Resumes the next task at its saved return address. For a new task, this enters the trampoline. |
| `pub unsafe fn switch(...)` | Thin Rust wrapper around the assembly symbol. The caller must pass valid context pointers. |

## How This Prepares Preemption

The current scheduler switches only when a task calls `yield_now()`. The saved
context format, per-task stacks, and task states are still useful for a future
timer-driven scheduler. A later preemptive path can save an interrupt frame and
reuse the task table and stack ownership model, but that is intentionally not
part of this milestone.
