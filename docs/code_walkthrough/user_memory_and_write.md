# User Memory And Write Walkthrough

Back to the [architecture guide](../architecture.md) or the
[walkthrough index](README.md).

This page covers:

- `src/user_memory.rs`
- the user-buffer parts of `src/syscall.rs`
- the `write` fd dispatch path in `src/scheduler.rs`
- the byte-oriented pieces of `src/serial.rs`

## Purpose

Syscalls receive raw numbers from user mode. A user pointer in `rsi` is not a
kernel pointer, even when the current CR3 happens to map it. The kernel must
validate and translate user memory before copying. This milestone adds that
boundary and the first useful user-facing syscall:

```text
write(fd, user_ptr, len)
```

The same checked-copy helpers now also support `open` path copying and `read`
destination buffers. The descriptor model itself is covered in
[file_descriptors_and_basic_io.md](file_descriptors_and_basic_io.md).

## Syscall ABI

The syscall entry path is still `int 0x80`:

| Register | Meaning |
| --- | --- |
| `rax = 0` | `yield` |
| `rax = 1` | `exit`, with exit code in `rdi` |
| `rax = 2` | `write` |
| `rax = 3` | `getpid` |
| `rax = 4` | `waitpid` |
| `rax = 5` | `open` |
| `rax = 6` | `read` |
| `rax = 7` | `close` |
| `rdi` | first syscall argument, such as fd or path pointer |
| `rsi` | second syscall argument, such as user buffer pointer or path length |
| `rdx` | third syscall argument, such as byte length, flags, or wait options |
| `rax` on return | non-negative success value or negative errno-like error |

The errno-like values are deliberately small: `ENOENT`, `EBADF`, `ECHILD`,
`EFAULT`, `EINVAL`, `ENFILE`, `EMFILE`, `ENAMETOOLONG`, and `ENOSYS`. Unknown
syscalls return `-ENOSYS`.

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

`sys_write(fd, user_buf, len)` now resolves `fd` through the current process's
descriptor table:

- `ConsoleStdout`: copy bytes from user memory and write them to COM1 serial
- `ConsoleStderr`: copy bytes from user memory and write them to COM1 serial
- `NullInput`: return `-EBADF`
- read-only embedded files: return `-EBADF`

The buffer is arbitrary bytes, not a NUL-terminated string and not assumed to
be UTF-8. The implementation validates the entire read range first, then copies
in small fixed chunks and writes those chunks to serial.

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

`tests/file_descriptors.rs` adds coverage for the same copy boundary through
`open`, `read`, and fd-routed `write`.

## Deferred Work

Deferred intentionally:

- a VFS and filesystem-backed files
- writable regular files
- `lseek`, `dup`, pipes, sockets, and fd inheritance
- blocking I/O
- `fork` and copy-on-write
- demand paging, `brk`, and `mmap`
- dynamic linking
- argv/envp
- NX, SMAP, and SMEP
- broad POSIX compatibility
