# File Descriptors And Basic I/O Walkthrough

Back to the [architecture guide](../architecture.md) or the
[walkthrough index](README.md).

This page covers:

- `src/fd.rs`
- `src/file.rs`
- the fd-backed `open`, `read`, `write`, and `close` paths in
  `src/scheduler.rs`
- the generated fd syscall fixtures in `build.rs`
- `tests/file_descriptors.rs`

## Purpose

This milestone adds the first real descriptor layer without turning the kernel
into a filesystem project. Each process gets a small descriptor table. The
kernel owns a small open-file table. The only regular files are read-only byte
arrays embedded in the kernel image.

The split matters even before `dup` or `fork` exist:

- a descriptor is a per-process number
- an open-file object owns the current offset
- two separate `open` calls create two open-file objects with independent
  offsets

There is still no VFS, directory traversal, writable regular file, `lseek`,
`dup`, pipe, socket, real stdin device, blocking I/O, or fd inheritance.

## `src/file.rs`

`src/file.rs` defines kernel-wide file objects:

| Code | Explanation |
| --- | --- |
| `MAX_OPEN_FILES: usize = 32` | Caps the system-wide open-file table. This keeps resource use deterministic for the fixed early heap. |
| `MAX_PATH_LEN: usize = 128` | Caps copied user paths before lookup. Longer paths return `-ENAMETOOLONG`. |
| `OpenFileId(pub usize)` | Small index into the kernel open-file table. Descriptor entries store this, not a file object directly. |
| `EmbeddedFileId(pub usize)` | Small index into the static embedded file registry. |
| `AccessMode::ReadOnly` and `AccessMode::WriteOnly` | Tracks whether the open-file object may be read or written. This milestone does not support read-write opens. |
| `OpenFileKind::NullInput` | fd 0's current placeholder object. Reads return EOF and writes fail. |
| `OpenFileKind::ConsoleStdout` and `ConsoleStderr` | Serial-backed output objects used by fd 1 and fd 2. |
| `OpenFileKind::EmbeddedFile(id)` | A read-only regular file backed by a static byte slice. |
| `FileError` | Internal file-layer errors. Syscall code maps these to errno-like negative return values. |
| `OpenFile { kind, access, offset, ref_count }` | Stores object type, allowed direction, current byte offset, and reference count. |
| `OpenFileTable { files: Vec<Option<OpenFile>> }` | Kernel-wide table. Empty slots can be reused after all descriptors referencing the open file are closed. |

`OpenFileTable::alloc(...)` first reuses an empty slot. If none exists, it
pushes a new slot until `MAX_OPEN_FILES` is reached. New open-file objects start
with offset 0 and refcount 1.

`dec_ref(...)` saturates the count down and frees the slot when it reaches 0.
This is enough for close and process-exit cleanup. `inc_ref(...)` exists for a
future `dup` or fork milestone, but this milestone does not call it.

## Embedded Files

The registry is deliberately tiny:

```rust
pub const EMBEDDED_FILES: &[EmbeddedFile] = &[
    EmbeddedFile {
        path: b"/hello.txt",
        bytes: b"hello from embedded file\n",
    },
    EmbeddedFile {
        path: b"/motd",
        bytes: b"tiny kernel says hello\n",
    },
];
```

`find_embedded_file(path)` performs exact byte matching. The syscall ABI passes
`path_ptr` and `path_len`, so paths are not NUL-terminated and do not need to
be UTF-8. There are no directories, relative paths, normalization rules, or
metadata beyond the byte contents.

## `src/fd.rs`

`src/fd.rs` defines per-process descriptor tables:

| Code | Explanation |
| --- | --- |
| `STDIN_FILENO = 0` | Standard input descriptor number. It maps to `NullInput` for now. |
| `STDOUT_FILENO = 1` | Standard output descriptor number. It maps to `ConsoleStdout`. |
| `STDERR_FILENO = 2` | Standard error descriptor number. It maps to `ConsoleStderr`. |
| `MAX_FDS_PER_PROCESS = 16` | Small fixed descriptor limit per process. |
| `FileDescriptor(pub usize)` | Returned descriptor number. |
| `FdEntry { open_file: OpenFileId }` | One descriptor table entry pointing to a kernel open-file object. |
| `FdTable { entries: [Option<FdEntry>; MAX_FDS_PER_PROCESS] }` | Fixed-size per-process table. |

`FdTable::new_with_stdio(open_files)` installs three normal descriptor entries:
fd 0, fd 1, and fd 2. They are not special-cased later; `read`, `write`, and
`close` all resolve them through the same descriptor lookup as embedded files.

`allocate_lowest(...)` chooses the lowest free descriptor. If fd 0 was closed,
the next `open` may reuse fd 0. That matches the simple table rule and avoids
inventing special stdio behavior.

`close(...)` removes the descriptor entry and decrements the referenced
open-file object. `close_all(...)` is used when a process exits so leaked file
descriptors do not keep open-file table slots alive.

## Process Integration

`Process` now owns:

```rust
fd_table: FdTable
```

Every new user process receives a fresh stdio set. Parent/child spawn helpers
do not inherit descriptors yet, because this kernel does not have `fork`. That
means two processes can both receive fd 3 for their first file open without
sharing descriptor state.

When a process becomes a zombie, `ProcessTable::mark_exited(...)` calls
`close_all(...)`. The process object may remain in the table for `waitpid`, but
its descriptor entries are gone and the open-file table references have been
released.

## Syscall ABI

The new syscall numbers are:

| Number | Syscall | Registers |
| --- | --- | --- |
| `5` | `open` | `rdi = path_ptr`, `rsi = path_len`, `rdx = flags` |
| `6` | `read` | `rdi = fd`, `rsi = user_buf`, `rdx = len` |
| `7` | `close` | `rdi = fd` |

Existing `write` stays syscall number `2`:

| Number | Syscall | Registers |
| --- | --- | --- |
| `2` | `write` | `rdi = fd`, `rsi = user_buf`, `rdx = len` |

All return values come back in `rax`. Non-negative values are success results.
Errors are negative errno-like values. New file-related values include:

| Error | Meaning |
| --- | --- |
| `-ENOENT` | Embedded path was not found or empty. |
| `-ENFILE` | Kernel open-file table is full. |
| `-EMFILE` | Process descriptor table is full. |
| `-ENAMETOOLONG` | Path length exceeds `MAX_PATH_LEN`. |

Only `O_RDONLY = 0` is accepted by `open`.

## `open`

`sys_open_current(path_ptr, path_len, flags)`:

1. Rejects unsupported flags.
2. Rejects zero-length and overlong paths.
3. Resolves the current process and its address space.
4. Copies the path from user memory with `copy_from_user`.
5. Performs exact lookup in the embedded registry.
6. Allocates a read-only embedded open-file object with offset 0.
7. Allocates the lowest free process descriptor.
8. Rolls back the open-file object if descriptor allocation fails.

The kernel never creates a slice from `path_ptr` directly. A bad path pointer
returns `-EFAULT`.

## `read`

`sys_read_current(fd, user_buf, len)`:

1. Returns 0 immediately for `len == 0`.
2. Resolves the fd through the current process's table.
3. Resolves the referenced open-file object.
4. Requires `AccessMode::ReadOnly`.
5. Dispatches by open-file kind.

`NullInput` returns EOF immediately. Console stdout and stderr are not readable
and return `-EBADF`. Embedded files read from the current open-file offset,
copy bytes to checked user memory with `copy_to_user`, then advance the offset
only after the copy succeeds. This means a bad user destination returns
`-EFAULT` without consuming file bytes.

Short reads near EOF are allowed. A read at EOF returns 0.

## `write`

`sys_write_current(fd, user_buf, len)` now follows the descriptor layer:

1. Returns 0 for `len == 0`.
2. Resolves the current process, fd entry, and open-file object.
3. Requires `AccessMode::WriteOnly`.
4. Allows only `ConsoleStdout` and `ConsoleStderr`.
5. Validates the entire user read range.
6. Copies fixed-size chunks from user memory and writes them to serial output.

Writing to fd 1 and fd 2 works because those descriptors point at console
objects. Writing to stdin or an embedded read-only file returns `-EBADF`.

## `close`

`sys_close_current(fd)` removes one process descriptor entry and decrements the
referenced open-file object. Closing an invalid or already closed descriptor
returns `-EBADF`. A successful close returns 0.

Descriptor numbers are reusable. The tests close fd 3 and then open another
embedded file, expecting fd 3 again.

## Generated Fixtures

`build.rs` generates fd-specific ELF programs:

| Fixture | Purpose |
| --- | --- |
| `fd_syscall_suite.elf` | Exercises stdin EOF, open/read/write/close, EOF reads, bad paths, bad flags, bad read destination, fd reuse, independent offsets, read-only write rejection, stdout read rejection, and cross-page reads. |
| `fd_first_open_exit.elf` | Opens `/hello.txt` and exits 0 only if the returned descriptor is 3. |
| `fd_open_leak_exit.elf` | Opens a file and exits without closing it, proving process-exit cleanup closes descriptors. |

The fixtures use the same hand-encoded ELF style as earlier tests. They call
`int 0x80` directly with the syscall number in `rax` and arguments in
`rdi/rsi/rdx`.

## `tests/file_descriptors.rs`

The file-descriptor test uses the normal userspace test setup: GDT, IDT, PIC,
PIT, heap, global memory state, then a kernel orchestrator task.

It verifies:

- new processes start with fd 0, 1, and 2 installed
- the first regular open returns fd 3
- `/hello.txt` and `/motd` can be opened, read, written to stdout, and closed
- reads return 0 at EOF
- unknown paths, bad path pointers, bad flags, closed fds, double close,
  read-only write targets, and stdout reads return the expected errors
- a failed read copy does not advance the file offset
- read destination buffers may cross page boundaries
- two separate opens of the same file have independent offsets
- two processes have private fd tables and can both use fd 3 independently
- process exit closes descriptors and releases open-file references
- PIT preemption and CR3 switching still work while file syscalls run

The serial output mirror is used only for deterministic assertions that the
expected embedded file bytes were written.

## Deferred Work

Deferred intentionally:

- VFS
- directory traversal and path normalization
- writable regular files
- persistent storage
- `lseek`
- `dup` and `dup2`
- pipes and sockets
- permissions
- filesystem-backed `exec`
- real stdin/input devices
- blocking I/O
- fork-based fd inheritance
- close-on-exec
- broad POSIX compatibility
