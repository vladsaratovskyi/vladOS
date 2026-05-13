# User Heap And `brk` Walkthrough

Back to the [architecture guide](../architecture.md) or the
[walkthrough index](README.md).

This page covers:

- the heap layout constants in `src/user.rs`
- `UserHeap` in `src/process.rs`
- heap page helpers in `src/address_space.rs`
- the `brk` syscall path in `src/syscall.rs` and `src/scheduler.rs`
- generated heap fixtures in `build.rs`
- `tests/user_heap.rs`

## Purpose

This milestone gives each user process a private, bounded heap and a
project-local `brk` syscall. It does not add a userspace allocator. User code
can ask the kernel to move the program break, then use the resulting mapped
bytes directly.

The implementation stays eager and explicit: growth maps pages immediately,
and shrink removes whole pages when possible. There is no demand paging,
overcommit, `mmap`, `sbrk`, copy-on-write, or frame reuse yet.

## User Layout

The current fixed user layout is:

```text
USER_CODE_BASE..USER_ELF_LOAD_END   ELF PT_LOAD mappings
heap start..USER_TEST_PAGE_BASE     process-local brk heap
USER_TEST_PAGE_BASE                 reserved test page
USER_STACK_TOP - 8 KiB..top         user stack
```

`USER_HEAP_LIMIT` is `USER_TEST_PAGE_BASE`. ELF-backed processes compute
`heap.start` by taking the highest loaded segment end and rounding it up to the
next 4 KiB page. Built-in user snippets keep their existing data page at
`USER_DATA_BASE`; their heap starts at the next page.

The heap start is page-aligned so heap pages do not inherit ELF segment
permissions.

## Process Heap State

`UserHeap` stores four addresses:

| Field | Meaning |
| --- | --- |
| `start` | First valid break value. Requests below this return `-EINVAL`. |
| `brk` | Current byte-granular logical end of the heap. |
| `mapped_end` | Page-aligned end of mapped heap pages. |
| `limit` | Exclusive maximum break. Requests above this return `-ENOMEM`. |

`Process` owns the heap state next to its address space and fd table. The
scheduler still schedules tasks, but syscalls resolve the current task to its
owning process before changing heap state.

## Address-Space Helpers

`AddressSpace::map_user_heap_pages(start, end)` maps a page-aligned range as
user-accessible and writable. Each allocated physical frame is zeroed before
the page becomes visible to userspace. If mapping a later page fails, the helper
unmaps pages created earlier in the same call where practical. The early frame
allocator is still monotonic, so physical frames are not returned to a free
list.

`AddressSpace::unmap_user_heap_pages(start, end)` unmaps whole page-aligned
ranges and flushes each unmapped page. This removes the virtual mapping, so a
later user access to that page raises a contained user page fault.

## `SYS_BRK`

The syscall ABI is:

| Register | Meaning |
| --- | --- |
| `rax = 8` | `brk` syscall number |
| `rdi` | requested new break, or `0` to query |
| `rax` on success | current or updated break |
| `rax` on error | negative errno-like value |

Error behavior:

| Case | Return |
| --- | --- |
| `brk(0)` | current break |
| request below heap start | `-EINVAL` |
| request above heap limit | `-ENOMEM` |
| allocation or mapping failure while growing | `-ENOMEM` |
| invalid arithmetic or impossible internal range | `-EINVAL` |

This ABI is project-local. It intentionally does not copy Linux raw `brk` or
libc wrapper quirks.

## Growth And Shrink

Growth computes `new_mapped_end = align_up(requested, 4096)`. If that is beyond
the old mapped end, the kernel maps every new page eagerly, zeroes it, updates
`brk`, and returns the requested break.

Shrink also computes the aligned mapped end. If the new mapped end is below the
old mapped end, the kernel unmaps whole pages above the new break before
updating heap state.

The program break is byte-granular, but page tables are page-granular. The
kernel does not claim that bytes above `brk` fault when they are still inside a
mapped page. Tests that prove shrink faults shrink across a full page boundary.

## Test Coverage

`tests/user_heap.rs` verifies:

- initial heap metadata for a newly spawned ELF process
- `brk(0)` query behavior
- rejection below heap start and above heap limit
- failed requests do not change the break
- one-page and multi-page growth
- newly mapped heap pages are zeroed
- heap memory can be passed to `write`
- whole-page shrink unmaps removed heap pages
- lower heap bytes remain valid after shrink
- identical heap virtual addresses are private per process
- PIT preemption still works after a user task grows a heap page and busy-loops

## Deferred Work

Deferred intentionally:

- userspace `malloc` or `free`
- `sbrk`
- `mmap` and `munmap`
- demand paging
- overcommit and resource limits
- frame reuse after unmap
- copy-on-write and `fork`
- exec replacement
- argv/envp setup
