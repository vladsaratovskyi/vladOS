use core::fmt;

use spin::Mutex;
use x86_64::instructions::interrupts;

const BUFFER_HEIGHT: usize = 25;
const BUFFER_WIDTH: usize = 80;
const VGA_BUFFER: *mut u8 = 0xb8000 as *mut u8;
const COLOR_BYTE: u8 = 0x0f;

static WRITER: Mutex<Writer> = Mutex::new(Writer {
    column_position: 0,
    row_position: 0,
});

struct Writer {
    column_position: usize,
    row_position: usize,
}

impl Writer {
    fn write_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.new_line(),
            byte => {
                if self.column_position >= BUFFER_WIDTH {
                    self.new_line();
                }

                self.write_cell(self.row_position, self.column_position, byte);
                self.column_position += 1;
            }
        }
    }

    fn new_line(&mut self) {
        self.column_position = 0;

        if self.row_position + 1 < BUFFER_HEIGHT {
            self.row_position += 1;
        } else {
            self.scroll_up();
        }
    }

    fn scroll_up(&mut self) {
        for row in 1..BUFFER_HEIGHT {
            for col in 0..BUFFER_WIDTH {
                let byte = self.read_cell(row, col);
                self.write_cell(row - 1, col, byte);
            }
        }

        self.clear_row(BUFFER_HEIGHT - 1);
    }

    fn clear_row(&mut self, row: usize) {
        for col in 0..BUFFER_WIDTH {
            self.write_cell(row, col, b' ');
        }
    }

    fn read_cell(&self, row: usize, col: usize) -> u8 {
        let offset = (row * BUFFER_WIDTH + col) * 2;

        unsafe { VGA_BUFFER.add(offset).read_volatile() }
    }

    fn write_cell(&mut self, row: usize, col: usize, byte: u8) {
        let offset = (row * BUFFER_WIDTH + col) * 2;

        unsafe {
            VGA_BUFFER.add(offset).write_volatile(byte);
            VGA_BUFFER.add(offset + 1).write_volatile(COLOR_BYTE);
        }
    }

    fn write_string(&mut self, s: &str) {
        for byte in s.bytes() {
            match byte {
                0x20..=0x7e | b'\n' => self.write_byte(byte),
                _ => self.write_byte(0xfe),
            }
        }
    }
}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_string(s);
        Ok(())
    }
}

pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;

    // Hardware IRQ handlers can print too. Disabling local interrupts while
    // holding the writer lock prevents an IRQ from re-entering this path and
    // spinning forever on the same single-core CPU.
    interrupts::without_interrupts(|| {
        WRITER.lock().write_fmt(args).unwrap();
    });
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::vga_buffer::_print(format_args!($($arg)*));
    };
}

#[macro_export]
macro_rules! println {
    () => {
        $crate::print!("\n");
    };
    ($fmt:expr) => {
        $crate::print!(concat!($fmt, "\n"));
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::print!(concat!($fmt, "\n"), $($arg)*);
    };
}
