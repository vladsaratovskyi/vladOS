use core::fmt;

use spin::Mutex;
use x86_64::instructions::{interrupts, port::Port};

const COM1: u16 = 0x3f8;

static SERIAL1: Mutex<SerialPort> = Mutex::new(SerialPort);

pub fn init() {
    interrupts::without_interrupts(|| unsafe {
        Port::new(COM1 + 1).write(0x00u8);
        Port::new(COM1 + 3).write(0x80u8);
        Port::new(COM1).write(0x03u8);
        Port::new(COM1 + 1).write(0x00u8);
        Port::new(COM1 + 3).write(0x03u8);
        Port::new(COM1 + 2).write(0xc7u8);
        Port::new(COM1 + 4).write(0x0bu8);
    });
}

struct SerialPort;

impl SerialPort {
    fn write_byte(&mut self, byte: u8) {
        unsafe {
            Port::new(COM1).write(byte);
        }
    }
}

impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            self.write_byte(byte);
        }

        Ok(())
    }
}

pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;

    interrupts::without_interrupts(|| {
        SERIAL1.lock().write_fmt(args).unwrap();
    });
}

#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {
        $crate::serial::_print(format_args!($($arg)*));
    };
}

#[macro_export]
macro_rules! serial_println {
    () => {
        $crate::serial_print!("\n");
    };
    ($fmt:expr) => {
        $crate::serial_print!(concat!($fmt, "\n"));
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::serial_print!(concat!($fmt, "\n"), $($arg)*);
    };
}
