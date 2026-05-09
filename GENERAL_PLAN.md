# General Plan

This project is a small x86_64 Rust OS built in stages. The goal is to keep the
early work narrow, understandable, and bootable before moving into larger OS
features.

## Target

- Architecture: x86_64
- Emulator: QEMU
- Language: Rust with `#![no_std]`
- Boot method: use an existing bootloader first, not a custom bootloader

Bare-metal Rust cannot use the normal Rust standard library because there is no
operating system underneath it. Use `#![no_std]`, `core`, and later maybe
`alloc`. In bare metal, no OS has loaded the program, so `std` is unavailable.

## Main Guide

Use Philipp Oppermann's *Writing an OS in Rust* as the first main guide. It
builds a small Rust OS step by step, with code for each post.

Keep the OSDev Wiki nearby too. The Rust Bare Bones page explains the basic Rust
OS-dev toolchain idea: `rustup`, `rustc`, `cargo`, and cross-compilation.

## First Milestones

Build in this order.

### Freestanding Rust Binary

- `#![no_std]`
- `#![no_main]`
- custom panic handler
- no heap yet
- no threads
- no filesystem

### Boot In QEMU

- generate a bootable image
- run it with QEMU
- print something through VGA text mode or serial output

### Basic CPU Setup

- Global Descriptor Table
- Interrupt Descriptor Table
- exceptions
- breakpoint exception
- double fault handler

### Memory Management

- read bootloader memory map
- identity mapping / higher-half mapping
- physical frame allocator
- paging
- heap allocator

### Interrupts And Timers

- PIC/APIC basics
- timer interrupt
- keyboard interrupt
- simple interrupt-safe logging

### Tasks

- cooperative tasks first
- then preemptive scheduling
- save/restore registers
- kernel stack per task

### Userspace

- privilege levels
- syscall mechanism
- ELF loading
- user/kernel address separation

### Files And Drivers

- RAM filesystem first
- then block device
- then simple filesystem
- later PCI, AHCI/NVMe, USB, network

Do not jump to filesystems or multitasking too early. The real wall is memory,
interrupts, and privilege separation.

## Rust Concepts To Know Well

- `#![no_std]`
- raw pointers
- `unsafe`
- lifetimes around memory ownership
- `repr(C)`, `repr(transparent)`, `repr(packed)`
- volatile memory access
- atomics
- spinlocks
- custom allocators
- FFI / ABI boundaries
- inline/global assembly basics

*Rust for Rustaceans* is useful here, especially for advanced Rust foundations,
unsafe Rust, concurrency, FFI, and `no_std`.

## OS Theory To Study In Parallel

### Computer Systems Basics

Study:

- memory layout
- stack/heap
- linking/loading
- virtual memory
- machine-level execution

Use *Computer Systems: A Programmer's Perspective* for machine-level programs,
memory hierarchy, linking, exceptional control flow, virtual memory, and
system-level I/O.

### OS Concepts

Study:

- processes
- threads
- scheduling
- synchronization
- deadlocks
- paging
- filesystems
- protection/security

Use *Operating System Concepts* for the high-level OS design map.

### Unix/Linux System Behavior

Study:

- processes
- syscalls
- files
- signals
- threads
- IPC
- sockets

Use *The Linux Programming Interface* or *Advanced Programming in the UNIX
Environment* to understand what a mature userspace API looks like.

## Practical Roadmap

### Phase 1: It Boots

Goal: QEMU displays `Hello from Rust kernel`.

Learn:

- freestanding Rust
- target JSON
- bootloader
- panic handler
- serial output

### Phase 2: It Handles CPU Exceptions

Goal: trigger and handle breakpoint and page fault exceptions.

Learn:

- GDT
- IDT
- privilege rings
- interrupt stack table
- CPU exception model

### Phase 3: It Owns Memory

Goal: implement heap allocation and use `Box`/`Vec`.

Learn:

- physical memory map
- page tables
- frame allocator
- virtual memory
- heap allocator

### Phase 4: It Can Run Tasks

Goal: two kernel tasks print alternately.

Learn:

- context switching
- task structs
- kernel stacks
- scheduler
- timer interrupts

### Phase 5: It Has Userspace

Goal: run a tiny user program that calls a `write()` syscall.

Learn:

- ELF loader
- syscall ABI
- user/kernel mode switch
- address space isolation

### Phase 6: It Becomes An OS

Goal: shell-like user program, filesystem, keyboard, simple process model.

Learn:

- process table
- file descriptors
- VFS idea
- drivers
- IPC

## Suggested Project Structure

```text
my_os/
  kernel/
    src/
      main.rs
      interrupts.rs
      memory.rs
      allocator.rs
      task.rs
      syscall.rs
      drivers/
        serial.rs
        vga.rs
        keyboard.rs
  boot/
  userspace/
  crates/
    kernel_api/
    common/
```

Keep hardware-specific code isolated. Most of the kernel should not know whether
output goes to VGA, serial, or framebuffer.

## Unsafe Rule

Use `unsafe`, but contain it.

Good pattern:

```rust
pub struct Port {
    port: u16,
}

impl Port {
    pub const fn new(port: u16) -> Self {
        Self { port }
    }

    pub unsafe fn write_u8(&self, value: u8) {
        core::arch::asm!(
            "out dx, al",
            in("dx") self.port,
            in("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}
```

Then expose a safer wrapper above it. The kernel will require `unsafe`, but
unsafe code should be small, reviewed, and hidden behind safe abstractions where
possible.

## First Week

Install:

```powershell
rustup update
rustup component add rust-src llvm-tools-preview
cargo install bootimage
```

Then:

- create a minimal `no_std` Rust kernel
- boot it in QEMU
- print to serial
- add a panic handler
- add the IDT and handle breakpoint exception
- commit each milestone separately

The first real success is not multitasking. It is: I can boot my own Rust code
on bare metal and understand every line between reset and `kernel_main()`.
