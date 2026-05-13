# Process Lifecycle Walkthrough

Back to the [architecture guide](../architecture.md) or the
[walkthrough index](README.md).

This page covers:

- `src/process.rs`
- the process-aware pieces of `src/task.rs` and `src/scheduler.rs`
- the `getpid` and `waitpid` branches in `src/syscall.rs`
- the generated process fixtures in `build.rs`
- `tests/process_lifecycle.rs`

## Purpose

Earlier milestones had isolated user tasks, but no process object. This step
adds a narrow process layer without changing the scheduler's core job: the
scheduler still chooses tasks. A process now owns the user address space and
the lifecycle metadata that belongs above a schedulable task.

There is still only one user task per process. There is no `fork`, exec
replacement, signal delivery, process group, session, or multi-threaded process
model yet.

## Task Versus Process

`Task` remains the execution object:

- `TaskId`
- saved trap-frame context
- dedicated kernel stack
- `Ready`, `Running`, `Blocked`, `Finished`, or `Failed` state
- optional `ProcessId` for user tasks

`Process` is the lifecycle object:

- `ProcessId`
- optional parent PID
- child PID list
- `Running` or `Zombie` state
- `ProcessExit::Exited(code)` or `ProcessExit::Faulted`
- process-owned `AddressSpace`
- process-owned `UserHeap`
- process-owned file descriptor table
- main `TaskId`

Kernel tasks have no process ID and continue to run in the kernel address
space. User tasks resolve CR3 through their owning process.

## Process Table

`ProcessTable` is owned by the scheduler. PIDs are monotonic, start at 1, and
are not reused in this milestone. Reaping removes the process-table entry, but
the kernel does not yet reclaim physical frames or page-table pages because the
early frame allocator is still monotonic.

Root processes have `parent = None`. Child processes are created only by
kernel-facing spawn helpers for tests and future kernel code. If a parent exits
before a child, the child is marked parentless so it cannot later wake a dead
parent. Full init/reaper semantics are deferred.

## Spawn Path

Existing APIs such as `spawn_user_elf(...)` still return a `TaskId` so older
tests keep working. Internally they now create a root `Process` and then create
the process main task.

New APIs return both IDs:

```rust
pub struct UserProcessHandle {
    pub pid: ProcessId,
    pub task_id: TaskId,
}
```

Child-spawn helpers take a parent `ProcessId`, create a process with that
parent relationship, and attach the new PID to the parent's child list.

## Syscall ABI

The syscall entry remains `int 0x80`:

| Register | Meaning |
| --- | --- |
| `rax = 0` | `yield` |
| `rax = 1` | `exit`, with exit code in `rdi` |
| `rax = 2` | `write(fd, user_ptr, len)` |
| `rax = 3` | `getpid()` |
| `rax = 4` | `waitpid(child_pid, status_ptr, options)` |

`getpid` returns the current process ID in `rax`.

`waitpid` supports only:

- exact positive child PIDs
- `options == 0`
- `options == WNOHANG`

Unsupported options return `-EINVAL`. Non-child, missing, or already reaped
PIDs return `-ECHILD`.

## Wait Status ABI

The wait status is deliberately project-local and not POSIX-compatible:

```rust
#[repr(C)]
pub struct UserWaitStatus {
    pub kind: u32, // WAIT_EXITED = 0, WAIT_FAULTED = 1
    pub code: i32, // exit code for WAIT_EXITED
}
```

If `status_ptr == 0`, the status write is skipped and the child can still be
reaped. If `status_ptr != 0`, the scheduler validates the destination with the
checked user-memory helpers before blocking or reaping. A bad status pointer
returns `-EFAULT` and leaves the child waitable for a later valid call.

## Blocking Wait

Blocking wait does not spin in the kernel.

When a user task calls `waitpid(child, status, 0)` and the child is still
running:

1. The syscall handler validates the child relationship and status pointer.
2. The current trap-frame stack pointer is saved in the task context.
3. The parent task becomes `Blocked(WaitReason::ChildExit { ... })`.
4. The scheduler chooses the next `Ready` task.

When the child exits or faults:

1. The child process becomes a zombie.
2. The scheduler looks for a blocked parent waiting for that child.
3. The wait status is copied to the parent's user address space if requested.
4. The child zombie is reaped from the process table.
5. The parent's saved trap-frame `rax` is patched to the child PID.
6. The parent task becomes `Ready` and later returns from the original syscall.

`WNOHANG` avoids blocking. It returns `0` if the child exists and is still
running.

## User Faults

Contained user faults now terminate the owning process, not only the task.
A CPL3 #GP or #PF marks the current task `Failed` and the process
`Zombie(ProcessExit::Faulted)`. That zombie can be collected by the parent with
`waitpid`, which reports `WAIT_FAULTED`.

Kernel-mode faults keep their existing fatal behavior.

## Generated Fixtures

`build.rs` generates small ELF programs for the lifecycle test:

- `getpid_ok.elf`: verifies `getpid` returns a nonzero PID.
- `delayed_exit_child.elf`: yields several times, then exits 42.
- `immediate_exit_child.elf`: exits 7.
- `faulting_child.elf`: touches an unmapped user page and faults.
- `process_wait_parent_suite.elf`: exercises non-child wait, bad status
  pointers, `WNOHANG`, blocking wait, zombie reaping, and faulted-child status.

The parent suite reads child PIDs from its writable user data page. The kernel
test fills that page before scheduling the process.

## Test Coverage

`tests/process_lifecycle.rs` boots a standalone QEMU test kernel. Its
orchestrator creates one parent process, three child processes, and one
non-child root process. It verifies:

- task-to-process metadata
- parent/child relationships
- `getpid` from user mode
- child zombie state before reap
- bad status pointers do not reap
- `WNOHANG` while a child is running
- blocking wait wakes after child exit
- second wait on a reaped child returns `-ECHILD`
- a faulted child reports `WAIT_FAULTED`
- parent and sibling execution continue after child faults

The test exits QEMU with success only after the parent process exits with code
0 and the expected children have been reaped.

## Deferred Work

Deferred intentionally:

- `fork`
- copy-on-write
- exec replacement
- signals and `SIGCHLD`
- `waitpid(-1)`
- process groups, sessions, and job control
- multiple user threads per process
- filesystem-backed programs
- full POSIX wait-status encoding
- page-table and frame reclamation
