# Integration Tests Walkthrough

Back to the [architecture guide](../architecture.md) or the
[walkthrough index](README.md).

This page covers:

- `tests/stack_overflow.rs`
- `tests/page_fault.rs`

Both tests are full bootable kernels. They do not use Rust's normal test
harness. Each file defines its own `_start`, installs a test-local IDT, triggers
one controlled exception path, and exits QEMU only from the expected handler.

## `tests/stack_overflow.rs`

### Purpose

This test proves that the double-fault handler can run on the dedicated IST
stack. It intentionally overflows the normal kernel stack.

### Invariants

- The normal production IDT is not used.
- The test-local double-fault handler must use `DOUBLE_FAULT_IST_INDEX`.
- The handler never returns.
- Success is reported only from the double-fault handler.

### Line-By-Line

| Code | Explanation |
| --- | --- |
| `#![no_std]` | The test kernel cannot use the Rust standard library. |
| `#![no_main]` | The test provides its own `_start` entry point. |
| `#![feature(abi_x86_interrupt)]` | Enables the interrupt ABI for the test handler. |
| `use core::panic::PanicInfo;` | Imports panic information for the test panic handler. |
| `use blog_os::qemu::{exit_qemu, QemuExitCode};` | Imports QEMU pass/fail exit support. |
| `use blog_os::{gdt, hlt_loop, serial_print, serial_println};` | Imports shared GDT setup, halt behavior, and serial output. |
| `use x86_64::structures::idt::{...};` | Imports IDT and handler argument types. |
| `static mut TEST_IDT: InterruptDescriptorTable = ...;` | Stores the test-local IDT at a stable address. |
| `#[no_mangle]` | Keeps the `_start` symbol visible to the bootloader. |
| `pub extern "C" fn _start() -> !` | Defines the test kernel entry point. It never returns. |
| `blog_os::serial::init();` | Configures COM1 before printing test output. |
| `serial_print!("stack_overflow::stack_overflow...\t");` | Prints the test name without a newline so `[ok]` appears beside it. |
| `gdt::init();` | Initializes the GDT and TSS, including the double-fault IST stack. |
| `init_test_idt();` | Loads the test-local IDT with a double-fault handler. |
| `stack_overflow();` | Starts intentional recursion until the normal stack overflows. |
| `panic!("Execution continued after stack overflow");` | Fails the test if no double fault occurred. |
| `fn init_test_idt()` | Builds the IDT used only by this test. |
| `let idt = unsafe { &mut *core::ptr::addr_of_mut!(TEST_IDT) };` | Gets mutable access to the static test IDT. |
| `idt.double_fault.set_handler_fn(test_double_fault_handler)` | Installs the test double-fault handler. |
| `.set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);` | Requires the CPU to switch to the dedicated IST stack before entering the handler. |
| `idt.load();` | Makes the CPU use the test IDT. |
| `#[allow(unconditional_recursion)]` | Suppresses the expected recursion warning. |
| `#[inline(never)]` | Makes it harder for the compiler to optimize the recursion shape away. |
| `fn stack_overflow()` | Recursive function that intentionally consumes stack frames. |
| `stack_overflow();` | Calls itself forever until the stack is exhausted. |
| `core::ptr::read_volatile(&0);` | Side effect after the recursive call prevents tail-call optimization. |
| `extern "x86-interrupt" fn test_double_fault_handler(...) -> !` | Test handler for vector 8. It never returns. |
| `_stack_frame` and `_error_code` | Handler arguments are accepted but unused; leading underscores silence warnings. |
| `serial_println!("[ok]");` | Reports that the expected handler ran. |
| `exit_qemu(QemuExitCode::Success);` | Exits QEMU with the configured success status. |
| `hlt_loop();` | Halts forever if QEMU does not exit. |
| `#[panic_handler]` | Defines the test kernel panic handler. |
| `blog_os::qemu::test_panic_handler(info);` | Prints failure, exits QEMU with failure, and halts. |

## `tests/page_fault.rs`

### Purpose

This test proves that page-fault vector 14 reaches a test-local page-fault
handler. It intentionally reads from a canonical but likely unmapped address.

### Invariants

- The normal production page-fault handler is not used.
- The invalid access must be volatile so the compiler cannot remove it.
- Success is reported only from the page-fault handler.

### Line-By-Line

| Code | Explanation |
| --- | --- |
| `#![no_std]` | Keeps the test kernel bare-metal. |
| `#![no_main]` | Disables the normal Rust `main` path. |
| `#![feature(abi_x86_interrupt)]` | Enables the interrupt ABI for the page-fault handler. |
| `use core::panic::PanicInfo;` | Imports panic information. |
| `use blog_os::qemu::{exit_qemu, QemuExitCode};` | Imports QEMU success/failure reporting. |
| `use blog_os::{gdt, hlt_loop, serial_print, serial_println};` | Imports shared GDT setup, halt loop, and serial macros. |
| `use x86_64::registers::control::Cr2;` | Imports access to the faulting-address register. |
| `use x86_64::structures::idt::{...};` | Imports IDT and page-fault handler argument types. |
| `static mut TEST_IDT: InterruptDescriptorTable = ...;` | Static test-local IDT storage. |
| `pub extern "C" fn _start() -> !` | Test kernel entry point. |
| `blog_os::serial::init();` | Initializes COM1 serial output. |
| `serial_print!("page_fault::invalid_memory_access...\t");` | Prints the test name. |
| `gdt::init();` | Initializes GDT/TSS. The page-fault test does not require IST, but this keeps test setup consistent. |
| `init_test_idt();` | Loads an IDT with the test page-fault handler. |
| `let ptr = 0x4444_4444_0000 as *const u64;` | Chooses a canonical virtual address expected to be unmapped. |
| `core::ptr::read_volatile(ptr);` | Performs the intentional faulting read and prevents optimization. |
| `panic!("Execution continued after invalid memory access");` | Fails the test if the read did not fault. |
| `fn init_test_idt()` | Builds the page-fault test IDT. |
| `idt.page_fault.set_handler_fn(test_page_fault_handler);` | Installs the vector 14 test handler. |
| `idt.load();` | Makes the CPU use the test IDT. |
| `extern "x86-interrupt" fn test_page_fault_handler(...)` | Test handler for vector 14. |
| `stack_frame: InterruptStackFrame` | Receives saved CPU state from the fault. |
| `error_code: PageFaultErrorCode` | Receives page-fault flags from the CPU. |
| `let accessed_address = Cr2::read();` | Reads the address that caused the page fault. |
| `serial_println!();` | Moves output to a new line after the test label. |
| `serial_println!("EXCEPTION: PAGE FAULT");` | Labels the expected exception. |
| `serial_println!("Accessed Address: {:?}", accessed_address);` | Prints the CR2 address. |
| `serial_println!("Error Code: {:?}", error_code);` | Prints the raw page-fault flags. |
| `serial_println!("Stack Frame: {:#?}", stack_frame);` | Prints the saved CPU state. |
| `serial_println!("[ok]");` | Reports test success. |
| `exit_qemu(QemuExitCode::Success);` | Exits QEMU successfully. |
| `hlt_loop();` | Halts if QEMU does not exit. |
| `#[panic_handler]` | Defines panic behavior for this test kernel. |
| `blog_os::qemu::test_panic_handler(info);` | Reports failure through serial and QEMU debug-exit. |
