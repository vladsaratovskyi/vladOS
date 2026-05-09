#![no_std]
#![no_main]

use core::panic::PanicInfo;

use blog_os::{gdt, hlt_loop, interrupts, println};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    println!("Hello from Rust OS!");

    gdt::init();
    interrupts::init_idt();

    x86_64::instructions::interrupts::int3();

    println!("Still alive after breakpoint");

    hlt_loop();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    hlt_loop();
}
