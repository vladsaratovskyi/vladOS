# ELF Loader Walkthrough

Back to the [architecture guide](../architecture.md) or the
[walkthrough index](README.md).

This page covers:

- `src/elf.rs`
- the generated ELF fixtures from `build.rs`
- `tests/elf_loader.rs`

## Purpose

The ELF loader is the first process-style user spawn path. Earlier user tests
copied small in-kernel assembly snippets into user pages. This milestone keeps
the same syscall, trap-frame, and CR3-switching machinery, but the user entry
point now comes from an embedded ELF64 executable.

This is not a filesystem or `execve` implementation. The test programs are
generated at build time, embedded in the kernel test binary with
`include_bytes!`, and loaded from memory.

## Supported ELF Subset

`src/elf.rs` accepts only a small, explicit subset:

| Field | Required Value |
| --- | --- |
| class | ELF64 |
| endian | little-endian |
| type | `ET_EXEC` |
| machine | `EM_X86_64` |
| version | current |
| program headers | `PT_LOAD` only |

Unsupported or suspicious input is rejected. The loader checks malformed header
bounds, unsupported architecture, segment overlap, `p_memsz < p_filesz`,
non-page-aligned load segments, load ranges outside the user ELF region, and an
entry point that is not inside an executable load segment.

## Loading Flow

The high-level flow is:

1. `scheduler::spawn_user_elf(...)` receives a name and ELF byte slice.
2. `elf::load_user_elf(...)` parses and validates the ELF before allocation.
3. The loader creates a fresh `AddressSpace`.
4. Each `PT_LOAD` segment is eagerly mapped with user-accessible pages.
5. File-backed bytes are copied through the kernel's direct physical-memory map.
6. The BSS tail, if any, is zeroed.
7. The existing private user stack is mapped.
8. The loader computes the initial heap start from the highest loaded segment
   end, rounded up to a page.
9. The loader returns `UserTaskInit` with `entry_point = e_entry` and heap
   metadata.
10. The scheduler builds the normal user trap frame and later resumes it through
   `iretq`.

The first user instruction therefore runs at the ELF entry point, but no new
context-switch mechanism is introduced.

## Segment Permissions

ELF segment flags are translated conservatively:

| ELF flag | Kernel effect |
| --- | --- |
| `PF_W` | Adds `WRITABLE` to the leaf page-table entries. |
| `PF_X` | Marks the segment as executable for loader validation. |
| `PF_R` | Marks the segment as readable in loader metadata. |

All loaded pages are `PRESENT | USER_ACCESSIBLE`. The current kernel has not
enabled NX, so non-executable mappings are tracked logically but not enforced in
hardware yet. Read-only data is enforced because pages without `PF_W` are mapped
without `WRITABLE`; a user write raises a contained user page fault.

## User Layout

ELF segments must fit in the existing reserved user range:

```text
USER_ELF_LOAD_START = USER_CODE_BASE
USER_DATA_BASE      = writable or read-only user data in test ELFs
USER_ELF_LOAD_END   = USER_TEST_PAGE_BASE
USER_STACK_TOP      = top of the private 8 KiB user stack
```

The loader rejects segments that collide with the stack or reserved test page.
There is no ASLR, demand paging, relocation processing, dynamic linking, argv,
envp, or filesystem-backed executable lookup.

## Generated Fixtures

`build.rs` writes small ELF64 files into Cargo's `OUT_DIR`. It does this with
plain Rust byte construction rather than an external assembler or linker, so the
test fixture build has no extra tool dependency.

The generated programs use the current syscall ABI:

| Register | Meaning |
| --- | --- |
| `rax = 0` | syscall `yield` |
| `rax = 1` | syscall `exit` |
| `rax = 2` | syscall `write` |
| `rax = 3` | syscall `getpid` |
| `rax = 4` | syscall `waitpid` |
| `rax = 5` | syscall `open` |
| `rax = 6` | syscall `read` |
| `rax = 7` | syscall `close` |
| `rax = 8` | syscall `brk` |
| `rdi` | exit code for `exit`, or initial test argument before the program changes it |
| `int 0x80` | enter the kernel syscall path |

Fixtures include:

- `exit_42.elf`: exits with code 42.
- `write_private_data.elf`: writes its initial `rdi` argument to
  `USER_DATA_BASE`, reads it back, and exits with that value.
- `write_readonly_segment.elf`: attempts to write to a read-only segment and
  should fault.
- `busy_counter.elf`: increments `USER_DATA_BASE` forever so timer preemption
  can be observed.
- `bad_machine.elf`: has a wrong `e_machine` value for rejection coverage.
- process-lifecycle fixtures: exercise `getpid`, exact-child `waitpid`,
  zombie status, and contained user faults.
- file-descriptor fixtures: exercise embedded-file `open`, `read`, fd-routed
  `write`, `close`, bad arguments, fd reuse, independent offsets, and
  descriptor cleanup on process exit.
- user-heap fixtures: exercise `brk` query, growth, shrink, zeroing,
  heap-backed `write`, private heaps, and preemption after heap growth.

## `tests/elf_loader.rs`

The test kernel follows the same boot setup as the userspace and address-space
tests: initialize GDT, IDT, PIC, PIT, heap, and global memory state, then spawn
an orchestrator kernel task.

The orchestrator verifies:

- bad magic is rejected as `BadMagic`
- wrong `e_machine` is rejected as `UnsupportedMachine`
- `exit_42` exits normally with code 42
- two instances of the same ELF keep private contents at the same user virtual
  address
- writing to a read-only segment raises a contained user page fault
- PIT preemption still switches across ELF-backed CR3 roots

The test exits QEMU only after every check passes.
