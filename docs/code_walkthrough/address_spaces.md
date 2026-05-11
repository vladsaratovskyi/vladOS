# Address Spaces Walkthrough

Back to the [architecture guide](../architecture.md) or the
[walkthrough index](README.md).

This page covers `src/address_space.rs` and the global address-space support in
`src/memory.rs`.

## Purpose

The first userspace milestone entered CPL3 but still used the kernel's active
page table. This milestone gives every user task a separate P4 root, switches
CR3 in the scheduler, and treats user page faults as task-local failures.

The next layer can now load embedded ELF binaries into these address spaces,
but there is still no demand paging, copy-on-write, `fork`, filesystem-backed
`execve`, or process object.

## Layout

The kernel reserves P4 index 1 for user mappings:

```text
P4 index 0, high indexes, etc.  shared kernel mappings, supervisor-only
P4 index 1                     per-task user code/data/stack mappings
```

Every user address space uses the same user virtual addresses:

```text
USER_CODE_BASE       executable user code page
USER_DATA_BASE       private writable user data page
USER_ELF_LOAD_START  first virtual address accepted for ELF PT_LOAD segments
USER_ELF_LOAD_END    exclusive end of the ELF load range
USER_TEST_PAGE_BASE  optional private test page
USER_STACK_TOP       top of the private 8 KiB user stack
```

Because each task has a distinct P4 root, two tasks can both use
`USER_DATA_BASE` while reaching different physical frames.

## `src/memory.rs`

`memory::init(...)` still returns an `OffsetPageTable` over the currently active
page tables. Address-space creation also needs the bootloader physical-memory
offset and a frame allocator after normal heap setup, so tests that create user
tasks call:

```rust
memory::init_global(physical_memory_offset, frame_allocator);
```

The stored memory state contains:

- the bootloader direct physical-memory offset
- the kernel P4 frame read from CR3
- the monotonic boot-info frame allocator

This is single-core early-kernel state behind a `spin::Mutex`. It is used only
while local interrupts are disabled or during controlled setup paths.

## `src/address_space.rs`

`AddressSpace` owns one top-level page-table frame:

```rust
pub struct AddressSpace {
    level_4_frame: PhysFrame<Size4KiB>,
}
```

`AddressSpace::new_user()` allocates and zeroes a fresh P4. It then shallow
copies all kernel P4 entries except the reserved user slot. Copied entries have
`USER_ACCESSIBLE` cleared at the P4 level, so the kernel text, heap, stacks,
device mappings, and physical-memory direct map remain usable by ring 0 but not
by ring 3.

User pages are mapped with the `x86_64` crate's mapper:

- code: `PRESENT | USER_ACCESSIBLE`
- data/test/stack: `PRESENT | WRITABLE | USER_ACCESSIBLE`

The mapper allocates lower-level page tables as needed. Since the leaf flags
include `USER_ACCESSIBLE`, the parent entries for the user slot are also
created user-accessible. Kernel P4 entries stay supervisor-only.

`AddressSpace::read_user_u64(...)` is a test helper. It walks the target P4 via
the physical-memory direct map and reads the physical frame without switching
CR3.

`AddressSpace::map_user_region(...)`, `copy_to_user(...)`, and
`zero_user_range(...)` are loader helpers. They let the ELF loader allocate
eager user pages, copy file-backed segment bytes, and zero BSS ranges without
temporarily switching CR3. Copies use the kernel direct map, so the loader can
initialize read-only user pages before user mode ever runs.

## CR3 Switching

Tasks carry either the kernel address space or a user `AddressSpace`. Whenever
the scheduler selects a task, it:

1. saves the old trap frame as before
2. chooses the next ready task as before
3. updates `TSS.rsp0` to the selected task's kernel stack
4. compares the selected P4 frame with the currently loaded one
5. writes CR3 only if the P4 root changed
6. restores the selected trap frame through `iretq`

When the scheduler returns to the boot stack because no runnable task remains,
it switches CR3 back to the kernel P4 first.

## User Page Faults

Production #PF now uses an explicit error-code trap-frame stub, matching the
#GP path. The Rust half reads CR2 and inspects saved `cs`:

- CPL3 #PF records `UserFaultInfo`, marks the current task `Failed`, and
  schedules the next ready task.
- CPL0 #PF prints diagnostics and halts, preserving the old fatal kernel
  behavior.

This is containment only. The kernel does not allocate a missing page, grow a
user stack, or retry the instruction.
