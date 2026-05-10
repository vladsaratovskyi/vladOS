#![no_std]
#![no_main]

use core::panic::PanicInfo;

use vlad_os::qemu::{exit_qemu, QemuExitCode};
use vlad_os::{gdt, hlt_loop, interrupts, serial_print, serial_println};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    vlad_os::serial::init();
    serial_print!("interrupts::pic_pit_foundation...\t");

    gdt::init();
    interrupts::init_idt();
    interrupts::init_pics();
    interrupts::init_pit();

    assert_eq!(
        interrupts::InterruptIndex::Timer.as_u8(),
        interrupts::PIC_1_OFFSET
    );
    assert_eq!(
        interrupts::InterruptIndex::Keyboard.as_u8(),
        interrupts::PIC_1_OFFSET + 1
    );
    assert_eq!(
        interrupts::InterruptIndex::Timer.as_usize(),
        usize::from(interrupts::PIC_1_OFFSET)
    );
    assert_eq!(interrupts::timer_ticks(), 0);

    serial_println!("[ok]");
    exit_qemu(QemuExitCode::Success);
    hlt_loop();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    vlad_os::qemu::test_panic_handler(info);
}
