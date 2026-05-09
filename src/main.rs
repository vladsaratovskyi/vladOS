#![no_std]
#![no_main]

use core::panic::PanicInfo;

const VGA_BUFFER: *mut u8 = 0xb8000 as *mut u8;
const COLOR_BYTE: u8 = 0x0f;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    write_vga_message(b"Hello from Rust OS!");

    loop {
        core::hint::spin_loop();
    }
}

fn write_vga_message(message: &[u8]) {
    for (i, byte) in message.iter().enumerate() {
        let offset = i * 2;

        unsafe {
            VGA_BUFFER.add(offset).write_volatile(*byte);
            VGA_BUFFER.add(offset + 1).write_volatile(COLOR_BYTE);
        }
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
