# Build Configuration Walkthrough

Back to the [architecture guide](../architecture.md) or the
[walkthrough index](README.md).

This page covers:

- `Cargo.toml`
- `.cargo/config.toml`
- `x86_64-vlad_os.json`
- `rust-toolchain.toml`

## `Cargo.toml`

### Purpose

`Cargo.toml` declares the crate, dependencies, bootimage test configuration,
binary target, integration test kernels, and panic strategy.

### Line-By-Line

| Code | Explanation |
| --- | --- |
| `[package]` | Starts package metadata. |
| `name = "vlad_os"` | The crate is named `vlad_os`; integration tests import it as `vlad_os`. |
| `version = "0.1.0"` | Current package version. |
| `edition = "2021"` | Uses Rust 2021 edition syntax and rules. |
| `[package.metadata.bootimage]` | Configuration read by `cargo bootimage`. |
| `test-args = [` | Starts the QEMU arguments used when running bootimage tests. |
| `"-device", "isa-debug-exit,iobase=0xf4,iosize=0x04"` | Adds QEMU's debug-exit device on port `0xf4`; tests write here to exit QEMU. |
| `"-serial", "stdio"` | Forwards COM1 serial output to the terminal. |
| `"-display", "none"` | Runs tests headlessly without a graphical QEMU window. |
| `"-no-reboot"` | Prevents silent reboot loops after fatal CPU resets. |
| `]` | Ends the QEMU test-args list. |
| `test-success-exit-code = 33` | Tells bootimage that QEMU process exit code 33 means success. This corresponds to writing `0x10` to `isa-debug-exit`. |
| `[dependencies]` | Starts runtime dependencies. |
| `bootloader = { version = "0.9", features = ["map_physical_memory"] }` | Provides the bootloader used by `cargo bootimage` and enables the direct physical-memory mapping plus `BootInfo::physical_memory_offset`. |
| `linked_list_allocator = "0.10.6"` | Provides a small no-std heap allocator suitable for the first fixed-size heap milestone. |
| `pic8259 = "0.10.2"` | Provides a small `no_std` helper for remapping the legacy chained 8259 PICs and sending EOIs correctly. Cargo currently resolves this to the compatible `0.10.4` release in `Cargo.lock`. |
| `spin = "0.9.8"` | Provides `no_std` mutexes for PIC access and interrupt-safe VGA/serial output locks. |
| `x86_64 = "=0.14.7"` | Provides CPU instructions, registers, GDT/TSS/IDT types, and port I/O. The exact pin keeps the known API stable. |
| `[[bin]]` | Declares the production kernel binary target. |
| `name = "vlad_os"` | Binary name. |
| `path = "src/main.rs"` | Binary entry source. |
| `test = false` | Prevents Cargo from trying to build the normal boot binary as a Rust test harness. |
| `[[test]] name = "stack_overflow"` | Declares the double-fault integration test target. |
| `harness = false` | Disables Rust's test harness; the file is a bootable test kernel with `_start`. |
| `[[test]] name = "page_fault"` | Declares the page-fault integration test target. |
| `harness = false` | Makes the page-fault test a bootable kernel too. |
| `[[test]] name = "memory_mapping"` | Declares the memory-mapping integration test target. |
| `harness = false` | Makes the memory-mapping proof a bootable kernel with its own entry point. |
| `[[test]] name = "heap_allocation"` | Declares the heap-allocation integration test target. |
| `harness = false` | Makes the heap test a bootable kernel that exits QEMU after its allocation checks. |
| `[[test]] name = "interrupts"` | Declares the interrupt-foundation integration test target. |
| `harness = false` | Makes the interrupt test a bootable kernel that exits QEMU after checking PIC/PIT setup state. |
| `[[test]] name = "cooperative_tasks"` | Declares the cooperative task integration test target. |
| `harness = false` | Makes the task test a bootable kernel that exits QEMU after deterministic task switching checks. |
| `[profile.dev] panic = "abort"` | Development builds abort on panic. There is no stack unwinding runtime. |
| `[profile.release] panic = "abort"` | Release builds also abort on panic. |

## `.cargo/config.toml`

### Purpose

This file tells Cargo to build for the custom bare-metal target and use
bootimage as the runner.

### Line-By-Line

| Code | Explanation |
| --- | --- |
| `[build]` | Starts build-wide Cargo configuration. |
| `target = "x86_64-vlad_os.json"` | Uses the custom target specification instead of the host Windows target. |
| `[unstable]` | Starts nightly-only Cargo settings. |
| `build-std = ["core", "compiler_builtins", "alloc"]` | Rebuilds `core`, compiler builtins, and the heap-backed `alloc` crate for the custom target. |
| `build-std-features = ["compiler-builtins-mem"]` | Enables memory intrinsics like `memcmp` for bare-metal linking. |
| `json-target-spec = true` | Allows the custom JSON target file on current nightly. |
| `panic-abort-tests = true` | Lets test kernels build with panic abort, avoiding duplicate `core` builds with incompatible panic modes. |
| `[target.'cfg(target_os = "none")']` | Applies the following runner only to bare-metal targets. |
| `runner = "bootimage runner"` | Converts the kernel binary into a bootable image and runs it in QEMU. |

## `x86_64-vlad_os.json`

### Purpose

This is the custom Rust target specification for the kernel. It describes the
machine ABI and code-generation rules for a freestanding x86_64 kernel.

### Line-By-Line

| Code | Explanation |
| --- | --- |
| `"llvm-target": "x86_64-unknown-none"` | Tells LLVM this is x86_64 code with no operating system. |
| `"data-layout": "..."` | Describes primitive type sizes and alignments for LLVM. |
| `"arch": "x86_64"` | Sets the architecture. |
| `"target-endian": "little"` | x86_64 is little-endian. |
| `"target-pointer-width": 64` | Pointers are 64 bits wide. |
| `"target-c-int-width": 32` | C `int` is 32 bits. |
| `"features": "-mmx,-sse,...,+soft-float"` | Disables SIMD/floating-point instruction generation and requests soft-float behavior. Early exception handlers must not emit SSE instructions before SSE is enabled. |
| `"rustc-abi": "softfloat"` | Matches the soft-float ABI requirement on current nightly Rust. |
| `"os": "none"` | Marks the target as bare metal. |
| `"executables": true` | Allows executable binaries to be produced. |
| `"linker-flavor": "ld.lld"` | Uses LLD-style linker arguments. |
| `"linker": "rust-lld"` | Uses Rust's bundled LLD linker. |
| `"panic-strategy": "abort"` | Makes abort the target panic strategy. |
| `"disable-redzone": true` | Disables the x86_64 red zone. Interrupts can clobber stack memory below `rsp`, so kernels must disable it. |
| `"dynamic-linking": false` | The kernel is statically linked. |
| `"relocation-model": "static"` | Produces statically addressed code. |
| `"code-model": "kernel"` | Uses the x86_64 kernel code model. |
| `"has-rpath": false` | Runtime library search paths do not apply to this kernel. |
| `"no-default-libraries": true` | Prevents linking host OS libraries. |

## `rust-toolchain.toml`

### Purpose

This file pins the Rust channel and required components for anyone opening the
project.

### Line-By-Line

| Code | Explanation |
| --- | --- |
| `[toolchain]` | Starts rustup toolchain configuration. |
| `channel = "nightly"` | Uses nightly Rust because this kernel needs unstable features such as `abi_x86_interrupt` and build-std support. |
| `components = ["rust-src", "llvm-tools-preview"]` | Installs Rust source for `build-std` and LLVM tools used by bootimage. |
