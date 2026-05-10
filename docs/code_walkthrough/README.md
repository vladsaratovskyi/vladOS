# Code Walkthrough Index

This directory explains the current kernel code at a lower level than the
[architecture guide](../architecture.md). The goal is educational: each page
documents what a file is for, what it depends on, what invariants it relies on,
and what each meaningful line or small code block does.

Read order:

1. [kernel_entry.md](kernel_entry.md): `src/main.rs` and `src/lib.rs`
2. [cpu_tables.md](cpu_tables.md): `src/gdt.rs`
3. [exceptions.md](exceptions.md): `src/interrupts.rs`
4. [output_and_qemu.md](output_and_qemu.md): `src/vga_buffer.rs`,
   `src/serial.rs`, and `src/qemu.rs`
5. [tests.md](tests.md): `tests/stack_overflow.rs` and `tests/page_fault.rs`
6. [build_config.md](build_config.md): `Cargo.toml`, `.cargo/config.toml`,
   `x86_64-blog_os.json`, and `rust-toolchain.toml`

## How To Read The Tables

The `Code` column quotes either a single line or the smallest useful block of
neighboring lines. Blank lines and closing braces are usually explained as part
of the block they close. The `Explanation` column tells you why the line exists
and what would break or change if it were different.

The walkthroughs are intentionally more verbose than the source. The source
should stay readable and focused; these docs carry the extra teaching detail.
