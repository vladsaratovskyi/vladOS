# User Memory And Write Walkthrough

Back to the [architecture guide](../architecture.md) or the
[walkthrough index](README.md).

This page covers:

- `src/user_memory.rs`
- the `write` branch in `src/syscall.rs`
- the byte-oriented pieces of `src/serial.rs`

## Purpose

Syscalls receive raw numbers from user mode. A user pointer in `rsi` is not a
kernel pointer, even when the current CR3 happens to map it. The kernel must
validate and translate user memory before copying. This milestone adds that
boundary and the first useful user-facing syscall:

```text
write(fd, user_ptr, len)
```

It is intentionally narrow. There is still no file descriptor table,
filesystem, blocking I/O, `read`, `open`, or `close`.

## Syscall ABI

The syscall entry path is still `int 0x80`:

| Register | Meaning |
| --- | --- |
| `rax = 0` | `yield` |
| `rax = 1` | `exit`, with exit code in `rdi` |
| `rax = 2` | `write` |
| `rdi` | `write` fd; only 1 and 2 are accepted |
| `rsi` | `write` user buffer pointer |
| `rdx` | `write` byte length |
| `rax` on return | non-negative success value or negative errno-like error |

The only error numbers added for this step are `EBADF`, `EFAULT`, `EINVAL`, and
`ENOSYS`. Unknown syscalls return `-ENOSYS`.

## Checked User Memory

`src/user_memory.rs` defines:

```rust
pub enum UserMemoryError {
    AddressOverflow,
    OutsideUserRange,
    Unmapped,
    NotWritable,
}
```

The read and write validators handle `len == 0` as success, use checked
arithmetic for `start + len`, reject ranges outside the fixed user virtual
range, and walk every touched page. Read validation requires a present,
user-accessible page. Write validation also requires the leaf PTE to be
`WRITABLE`.

Copies are page-by-page:

1. Validate the current user virtual address.
2. Translate it through the task's `AddressSpace`.
3. Compute how many bytes fit before the next page boundary.
4. Copy through the bootloader direct physical-memory mapping.

The unsafe blocks are limited to copying through already validated physical
addresses. The kernel never creates a slice directly from an arbitrary user
virtual pointer.

## Fault Distinction

This milestone separates two cases:

- direct illegal user access, such as a user instruction writing to read-only
  memory, still raises a user page fault and marks the task `Failed`
- a bad pointer passed as a syscall argument returns `-EFAULT`, so the task can
  inspect the return value and continue

That distinction is important for later syscalls such as `read`, `exec`, IPC,
and argv/envp setup.

## `SYS_WRITE`

`sys_write(fd, user_buf, len)` supports:

- `fd == 1`: stdout
- `fd == 2`: stderr

Both currently route to COM1 serial output. The buffer is arbitrary bytes, not a
NUL-terminated string and not assumed to be UTF-8. The implementation validates
the entire read range first, then copies in small fixed chunks and writes those
chunks to serial.

For deterministic tests, `serial::write_bytes(...)` also mirrors syscall output
into a fixed in-kernel byte buffer. This is a test helper, not a file
descriptor table or general logging design.

## Test Coverage

`tests/user_syscalls.rs` verifies:

- valid writes from read-only user data
- fd 1 and fd 2
- bad unmapped and kernel pointers return `-EFAULT`
- bad fd returns `-EBADF`
- a single write can span two user pages
- kernel `copy_to_user` rejects read-only user pages
- kernel `copy_to_user` accepts writable user pages
- direct user writes to read-only pages still fault the task
- PIT preemption still works with syscall-heavy ELF user tasks

## Deferred Work

Deferred intentionally:

- file descriptor tables
- filesystem-backed files
- `read`, `open`, and `close`
- blocking I/O
- process hierarchy and `wait`
- `fork` and copy-on-write
- demand paging, `brk`, and `mmap`
- dynamic linking
- argv/envp
- NX, SMAP, and SMEP
- broad POSIX compatibility
