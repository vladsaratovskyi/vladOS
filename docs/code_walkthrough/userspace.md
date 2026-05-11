# Userspace Walkthrough

Back to the [architecture guide](../architecture.md) or the
[walkthrough index](README.md).

This page covers:

- `src/user.rs`
- `src/syscall.rs`
- the userspace pieces of `src/gdt.rs`, `src/interrupts.rs`, `src/task.rs`,
  `src/scheduler.rs`, and `src/arch/x86_64/context.rs`

## Purpose

This milestone proves that the kernel can enter CPL3, receive a controlled
return to CPL0 through a software interrupt, terminate user tasks, and contain a
user-mode privileged-instruction fault. User tasks now run in isolated address
spaces; this file focuses on the user entry and syscall layer. It deliberately
keeps the syscall ABI small. ELF loading is covered separately in
[elf_loader.md](elf_loader.md); there is still no `syscall/sysret`, process
model, or broad POSIX API.

## Core Model

A user task is still scheduled by the same stackful scheduler as kernel tasks.
The difference is its initial trap frame:

| Field | Value |
| --- | --- |
| `rip` | User entry virtual address. |
| `cs` | User code selector with RPL 3. |
| `rflags` | Reserved bit set, and IF set when spawned with interrupts enabled. |
| `rsp` | Top of the mapped user stack, adjusted for function-entry alignment. |
| `ss` | User data selector with RPL 3. |
| `rdi` | One initial argument; tests usually pass `USER_DATA_BASE` or another user virtual address. |

The scheduler restores this frame through the existing
`restore_interrupt_context` path. The final `iretq` performs the first ring-3
transition, so there is no special second entry path for user mode.

## GDT And TSS

`src/gdt.rs` now contains four flat long-mode segment descriptors:

- kernel code
- kernel data
- user code
- user data

The user selectors are returned with RPL 3. That requested privilege level is
part of what makes the `iretq` frame enter CPL3 instead of returning to ring 0.

The TSS still keeps the double-fault IST stack. It also uses
`privilege_stack_table[0]`, commonly called `rsp0`. When an interrupt or
exception arrives while CPL3 code is running, the CPU loads `rsp0` before
pushing the interrupt return frame. The scheduler updates `rsp0` to the selected
task's kernel-stack top every time it switches tasks.

## User Task Setup

`src/user.rs` provides tiny built-in user programs and helpers that create a
complete `UserTaskInit`:

| Code | Explanation |
| --- | --- |
| `USER_CODE_BASE`, `USER_DATA_BASE`, `USER_TEST_PAGE_BASE`, `USER_STACK_TOP` | Fixed user virtual layout inside the reserved user P4 slot. These addresses are reused in every user address space. |
| `USER_ELF_LOAD_START` and `USER_ELF_LOAD_END` | Bounds used by the ELF loader for eager `PT_LOAD` segment mappings. |
| `create_user_task(...)` | Allocates a fresh `AddressSpace`, copies one tiny user program into a user code page, maps a private data page, and maps a private 8 KiB user stack. |
| `create_user_task_with_test_page(...)` | Same setup, plus an optional private test page used by address-space isolation tests. |
| `map_user_stack(...)` | Shared helper used by both built-in user snippets and ELF-backed user tasks. |
| `userspace_yield_exit_entry` | Writes a marker, executes `int 0x80` yield, writes a second marker, then executes `int 0x80` exit. |
| `userspace_privileged_hlt_entry` | Writes a marker and executes `hlt`, which should raise a user-mode #GP. |
| `userspace_busy_counter_entry` | Increments a marker forever; the userspace test only regains control if timer preemption from CPL3 works. |
| `userspace_write_read_aa_entry` and `userspace_write_read_bb_entry` | Write distinct values to the same user virtual address, yield, then exit with the value read back. |
| `userspace_read_arg_exit_entry` | Reads from the address in `rdi` and exits with that value; tests use it for mapped, unmapped, and kernel-only addresses. |

User code is copied into user pages; kernel text is not mapped user-executable.
See [address_spaces.md](address_spaces.md) for page-table construction.

## Syscall Flow

`src/syscall.rs` defines vector `0x80` and two syscall numbers:

- `0`: `Yield`
- `1`: `Exit`

The calling convention is intentionally small: syscall number in `rax`, return
value in `rax`, and `exit` uses `rdi` as a minimal exit code for deterministic
tests. The current user code uses inline/global assembly `int 0x80` wrappers
rather than calling kernel scheduler functions directly.

The flow is:

1. User code executes `int 0x80`.
2. The IDT entry allows DPL 3 callers.
3. If the interrupt came from CPL3, the CPU switches to `TSS.rsp0`.
4. The CPU pushes `ss`, `rsp`, `rflags`, `cs`, and `rip`.
5. `syscall_interrupt_entry` pushes all general-purpose registers.
6. `syscall_interrupt_rust` calls `syscall::dispatch(frame_rsp)`.
7. `Yield` returns through the normal scheduler switch path.
8. `Exit` stores `rdi` as the task exit code, marks the current task
   `Finished`, and resumes the next ready task.
9. Assembly restores the selected trap frame and finishes with `iretq`.

This uses software interrupts instead of `syscall/sysret` because the kernel
already has a full trap-frame interrupt path. It keeps this step about safe
privilege transitions rather than fast syscall entry.

## User Fault Flow

General-protection faults push an error code before the normal interrupt return
state. `TrapFrameWithErrorCode` documents that layout:

1. Manually saved `r15` through `rax`.
2. CPU-pushed error code.
3. CPU-pushed `rip`, `cs`, `rflags`, `rsp`, and `ss`.

`general_protection_rust` checks the saved `cs` privilege bits. If the fault
came from CPL3, the scheduler marks the current task `Failed` and resumes
another ready task. If the fault came from CPL0, the kernel prints diagnostics
and halts. Page faults use the same error-code trap-frame path now: CPL3 #PF is
task-local failure, while CPL0 #PF remains fatal.

## Test Coverage

`tests/userspace.rs` proves the minimum flow:

- a user task enters CPL3 and writes a marker
- `int 0x80` yield switches back to a kernel task
- the user task resumes, writes another marker, and exits by syscall
- a second user task executes privileged `hlt`, is marked failed, and does not
  crash the kernel
- a busy user loop never calls yield, and the kernel task resumes only after a
  PIT-driven preemption from user mode

The test exits QEMU successfully only after those markers and scheduler states
match the expected path.
