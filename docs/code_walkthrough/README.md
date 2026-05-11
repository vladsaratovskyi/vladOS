# Code Walkthrough Index

This directory explains the current kernel code at a lower level than the
[architecture guide](../architecture.md). The goal is educational: each page
documents what a file is for, what it depends on, what invariants it relies on,
and what each meaningful line or small code block does.

Read order:

1. [kernel_entry.md](kernel_entry.md): `src/main.rs` and `src/lib.rs`
2. [cpu_tables.md](cpu_tables.md): `src/gdt.rs`
3. [exceptions.md](exceptions.md): `src/interrupts.rs`, including CPU
   exceptions and legacy hardware IRQs
4. [memory.md](memory.md): `src/memory.rs`
5. [allocator.md](allocator.md): `src/allocator.rs`
6. [tasks.md](tasks.md): `src/task.rs`, `src/scheduler.rs`, and
   `src/arch/x86_64/context.rs`
7. [address_spaces.md](address_spaces.md): `src/address_space.rs` and the
   address-space state in `src/memory.rs`
8. [userspace.md](userspace.md): `src/user.rs`, `src/syscall.rs`, and the
   ring-3 pieces of `src/gdt.rs`, `src/interrupts.rs`, and the scheduler
9. [elf_loader.md](elf_loader.md): `src/elf.rs`, generated embedded ELF
   fixtures, and `tests/elf_loader.rs`
10. [user_memory_and_write.md](user_memory_and_write.md): checked user-buffer
   copying, `src/user_memory.rs`, and the `write` syscall path
11. [process_lifecycle.md](process_lifecycle.md): `src/process.rs`, the
   process-aware scheduler pieces, `getpid`, `waitpid`, and
   `tests/process_lifecycle.rs`
12. [file_descriptors_and_basic_io.md](file_descriptors_and_basic_io.md):
   `src/fd.rs`, `src/file.rs`, descriptor syscalls, and
   `tests/file_descriptors.rs`
13. [output_and_qemu.md](output_and_qemu.md): `src/vga_buffer.rs`,
   `src/serial.rs`, and `src/qemu.rs`
14. [tests.md](tests.md): `tests/stack_overflow.rs`, `tests/page_fault.rs`,
   `tests/memory_mapping.rs`, `tests/heap_allocation.rs`, and
   `tests/interrupts.rs`, `tests/cooperative_tasks.rs`, and
   `tests/preemptive_tasks.rs`, `tests/userspace.rs`, and
   `tests/address_spaces.rs`, `tests/elf_loader.rs`, and
   `tests/user_syscalls.rs`, `tests/process_lifecycle.rs`, and
   `tests/file_descriptors.rs`
15. [build_config.md](build_config.md): `Cargo.toml`, `build.rs`,
   `.cargo/config.toml`,
   `x86_64-vlad_os.json`, and `rust-toolchain.toml`

## How To Read The Tables

The `Code` column quotes either a single line or the smallest useful block of
neighboring lines. Blank lines and closing braces are usually explained as part
of the block they close. The `Explanation` column tells you why the line exists
and what would break or change if it were different.

The walkthroughs are intentionally more verbose than the source. The source
should stay readable and focused; these docs carry the extra teaching detail.
