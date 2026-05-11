#![no_std]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]

extern crate alloc;

pub mod address_space;
pub mod allocator;
pub mod arch;
pub mod elf;
pub mod gdt;
pub mod interrupts;
pub mod memory;
pub mod qemu;
pub mod scheduler;
pub mod serial;
pub mod syscall;
pub mod task;
pub mod user;
pub mod vga_buffer;

pub fn hlt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}

#[alloc_error_handler]
fn alloc_error_handler(layout: alloc::alloc::Layout) -> ! {
    panic!("allocation error: {:?}", layout)
}
