use core::fmt;

use spin::Mutex;
use x86_64::instructions::{interrupts, port::Port};

const COM1: u16 = 0x3f8;
const OUTPUT_BUFFER_SIZE: usize = 4096;

static SERIAL1: Mutex<SerialPort> = Mutex::new(SerialPort);
static OUTPUT_BUFFER: Mutex<OutputBuffer> = Mutex::new(OutputBuffer {
    bytes: [0; OUTPUT_BUFFER_SIZE],
    len: 0,
});

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

    fn write_bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.write_byte(*byte);
        }
    }
}

struct OutputBuffer {
    bytes: [u8; OUTPUT_BUFFER_SIZE],
    len: usize,
}

impl OutputBuffer {
    fn clear(&mut self) {
        self.len = 0;
    }

    fn push_bytes(&mut self, bytes: &[u8]) {
        let remaining = OUTPUT_BUFFER_SIZE.saturating_sub(self.len);
        let count = core::cmp::min(remaining, bytes.len());

        self.bytes[self.len..self.len + count].copy_from_slice(&bytes[..count]);
        self.len += count;
    }

    fn contains(&self, needle: &[u8]) -> bool {
        if needle.is_empty() {
            return true;
        }

        if needle.len() > self.len {
            return false;
        }

        self.bytes[..self.len]
            .windows(needle.len())
            .any(|window| window == needle)
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

pub fn write_bytes(bytes: &[u8]) {
    interrupts::without_interrupts(|| {
        SERIAL1.lock().write_bytes(bytes);
        OUTPUT_BUFFER.lock().push_bytes(bytes);
    });
}

pub fn clear_output_buffer() {
    interrupts::without_interrupts(|| {
        OUTPUT_BUFFER.lock().clear();
    });
}

pub fn output_contains(bytes: &[u8]) -> bool {
    interrupts::without_interrupts(|| OUTPUT_BUFFER.lock().contains(bytes))
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
