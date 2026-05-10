# Output And QEMU Walkthrough

Back to the [architecture guide](../architecture.md) or the
[walkthrough index](README.md).

This page covers:

- `src/vga_buffer.rs`
- `src/serial.rs`
- `src/qemu.rs`

## `src/vga_buffer.rs`

### Purpose

The VGA module provides normal-boot text output by writing directly to VGA text
memory at `0xb8000`.

### Invariants

- VGA text mode is assumed to be available.
- Each screen cell is two bytes: ASCII byte then color byte.
- The writer is global and not synchronized. That is acceptable for this early
  single-core, no-IRQ kernel stage.

### Line-By-Line

| Code | Explanation |
| --- | --- |
| `use core::fmt;` | Imports formatting traits so the kernel can implement Rust-style `print!` and `println!`. |
| `const BUFFER_HEIGHT: usize = 25;` | VGA text mode has 25 rows. |
| `const BUFFER_WIDTH: usize = 80;` | VGA text mode has 80 columns. |
| `const VGA_BUFFER: *mut u8 = 0xb8000 as *mut u8;` | Raw pointer to VGA text memory. |
| `const COLOR_BYTE: u8 = 0x0f;` | White foreground on black background. |
| `static mut WRITER: Writer = ...;` | Global cursor state for VGA output. Mutable static is used because there is no allocator or lock yet. |
| `struct Writer { column_position, row_position }` | Tracks where the next character will be written. |
| `fn write_byte(&mut self, byte: u8)` | Writes one byte or handles a newline. |
| `b'\n' => self.new_line()` | Newline moves the cursor to the next row. |
| `if self.column_position >= BUFFER_WIDTH` | Wraps to the next line when reaching column 80. |
| `self.write_cell(self.row_position, self.column_position, byte);` | Writes the visible character to the current screen cell. |
| `self.column_position += 1;` | Advances the cursor after writing a normal character. |
| `fn new_line(&mut self)` | Resets the column and advances or scrolls the row. |
| `if self.row_position + 1 < BUFFER_HEIGHT` | If there is space below, move down one row. |
| `else { self.scroll_up(); }` | If already at the bottom, scroll old content up. |
| `fn scroll_up(&mut self)` | Moves rows 1 through 24 into rows 0 through 23. |
| `let byte = self.read_cell(row, col);` | Reads the visible byte from a cell. Color is reset when re-written. |
| `self.write_cell(row - 1, col, byte);` | Writes that byte one row above. |
| `self.clear_row(BUFFER_HEIGHT - 1);` | Clears the final row after scrolling. |
| `fn clear_row(&mut self, row: usize)` | Fills a row with spaces. |
| `fn read_cell(&self, row, col) -> u8` | Computes a VGA byte offset and reads from hardware memory. |
| `(row * BUFFER_WIDTH + col) * 2` | Converts a row/column into a byte offset. Multiplication by 2 skips color bytes. |
| `read_volatile()` | Forces an actual memory read; hardware memory must not be optimized away. |
| `fn write_cell(&mut self, row, col, byte)` | Writes a character and color attribute to a VGA cell. |
| `write_volatile(byte)` | Forces an actual memory write to the VGA buffer. |
| `write_volatile(COLOR_BYTE)` | Writes the color byte next to the character byte. |
| `fn write_string(&mut self, s: &str)` | Writes a Rust string byte by byte. |
| `0x20..=0x7e | b'\n'` | Allows printable ASCII and newlines. |
| `_ => self.write_byte(0xfe)` | Replaces unsupported bytes with a visible placeholder. |
| `impl fmt::Write for Writer` | Allows `write_fmt` to route formatted strings into VGA output. |
| `fn write_str(&mut self, s: &str) -> fmt::Result` | Required method for `fmt::Write`. |
| `pub fn _print(args: fmt::Arguments)` | Internal function used by the exported macros. |
| `(*core::ptr::addr_of_mut!(WRITER)).write_fmt(args).unwrap();` | Writes formatted text through the global VGA writer using raw-pointer access to avoid direct static-mut references. |
| `macro_rules! print` | Exports a `print!` macro for the whole crate. |
| `$crate::vga_buffer::_print(format_args!(...))` | Uses `format_args!` without heap allocation. |
| `macro_rules! println` | Exports newline-aware printing forms. |
| `println!()` | Prints only a newline. |
| `println!($fmt)` | Appends a newline to a literal format string. |
| `println!($fmt, $($arg)*)` | Formats arguments and appends a newline. |

## `src/serial.rs`

### Purpose

The serial module provides COM1 output for QEMU tests. QEMU forwards this to the
terminal with `-serial stdio`.

### Line-By-Line

| Code | Explanation |
| --- | --- |
| `use core::fmt;` | Imports formatting support. |
| `use x86_64::instructions::port::Port;` | Imports safe wrappers around x86 I/O port instructions. Port reads and writes are still unsafe operations. |
| `const COM1: u16 = 0x3f8;` | Base I/O port for the first serial port. |
| `pub fn init()` | Configures COM1 before tests write to it. |
| `Port::new(COM1 + 1).write(0x00u8);` | Disables serial interrupts. |
| `Port::new(COM1 + 3).write(0x80u8);` | Enables DLAB so the divisor can be configured. |
| `Port::new(COM1).write(0x03u8);` | Sets divisor low byte for 38400 baud. |
| `Port::new(COM1 + 1).write(0x00u8);` | Sets divisor high byte. |
| `Port::new(COM1 + 3).write(0x03u8);` | Configures 8 data bits, no parity, one stop bit. |
| `Port::new(COM1 + 2).write(0xc7u8);` | Enables FIFO, clears it, and sets a 14-byte threshold. |
| `Port::new(COM1 + 4).write(0x0bu8);` | Enables IRQs, RTS/DSR, and normal operating mode. |
| `struct SerialPort;` | Zero-sized type representing COM1 output. |
| `fn write_byte(&mut self, byte: u8)` | Writes one byte to COM1. |
| `Port::new(COM1).write(byte);` | Sends the byte to the serial data port. |
| `impl fmt::Write for SerialPort` | Lets formatted strings write to serial. |
| `for byte in s.bytes()` | Sends a string one byte at a time. |
| `pub fn _print(args: fmt::Arguments)` | Internal formatter entry point used by serial macros. |
| `SerialPort.write_fmt(args).unwrap();` | Formats directly into the zero-sized serial writer. |
| `macro_rules! serial_print` | Exports `serial_print!` for test kernels. |
| `macro_rules! serial_println` | Exports newline-aware serial printing. |

## `src/qemu.rs`

### Purpose

The QEMU module lets tests report pass/fail by writing to QEMU's
`isa-debug-exit` device.

### Line-By-Line

| Code | Explanation |
| --- | --- |
| `use core::panic::PanicInfo;` | Imports panic details for the test panic handler. |
| `use x86_64::instructions::port::Port;` | Imports I/O port access. |
| `use crate::{hlt_loop, serial_println};` | Imports shared halt behavior and serial diagnostics. |
| `#[derive(Debug, Clone, Copy, PartialEq, Eq)]` | Makes exit codes printable, copyable, and comparable. |
| `#[repr(u32)]` | Forces enum values to be represented as `u32`, matching the debug-exit port write. |
| `pub enum QemuExitCode` | Defines semantic names for QEMU test outcomes. |
| `Success = 0x10` | Test success value. QEMU maps it to process status 33 with this device. |
| `Failed = 0x11` | Test failure value. |
| `pub fn exit_qemu(exit_code: QemuExitCode)` | Writes the chosen code to the debug-exit port. |
| `let mut port = Port::new(0xf4);` | Creates a handle to the configured debug-exit I/O port. |
| `port.write(exit_code as u32);` | Writes the code. QEMU exits when the device is present. |
| `pub fn test_panic_handler(info: &PanicInfo) -> !` | Shared panic handler for integration test kernels. |
| `serial_println!("[failed]");` | Marks the test as failed in serial output. |
| `serial_println!("Error: {}", info);` | Prints panic details. |
| `exit_qemu(QemuExitCode::Failed);` | Exits QEMU with failure. |
| `hlt_loop();` | Halts if QEMU does not exit for some reason. |
