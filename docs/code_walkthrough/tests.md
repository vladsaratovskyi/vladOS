# Integration Tests Walkthrough

Back to the [architecture guide](../architecture.md) or the
[walkthrough index](README.md).

This page covers:

- `tests/stack_overflow.rs`
- `tests/page_fault.rs`
- `tests/memory_mapping.rs`
- `tests/heap_allocation.rs`

These tests are full bootable kernels. They do not use Rust's normal test
harness. Each file defines or generates its own `_start`, installs only the
test-local setup it needs, triggers one controlled path, and exits QEMU only
after the expected behavior.

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
| `use vlad_os::qemu::{exit_qemu, QemuExitCode};` | Imports QEMU pass/fail exit support. |
| `use vlad_os::{gdt, hlt_loop, serial_print, serial_println};` | Imports shared GDT setup, halt behavior, and serial output. |
| `use x86_64::structures::idt::{...};` | Imports IDT and handler argument types. |
| `static mut TEST_IDT: InterruptDescriptorTable = ...;` | Stores the test-local IDT at a stable address. |
| `#[no_mangle]` | Keeps the `_start` symbol visible to the bootloader. |
| `pub extern "C" fn _start() -> !` | Defines the test kernel entry point. It never returns. |
| `vlad_os::serial::init();` | Configures COM1 before printing test output. |
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
| `vlad_os::qemu::test_panic_handler(info);` | Prints failure, exits QEMU with failure, and halts. |

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
| `use vlad_os::qemu::{exit_qemu, QemuExitCode};` | Imports QEMU success/failure reporting. |
| `use vlad_os::{gdt, hlt_loop, serial_print, serial_println};` | Imports shared GDT setup, halt loop, and serial macros. |
| `use x86_64::registers::control::Cr2;` | Imports access to the faulting-address register. |
| `use x86_64::structures::idt::{...};` | Imports IDT and page-fault handler argument types. |
| `static mut TEST_IDT: InterruptDescriptorTable = ...;` | Static test-local IDT storage. |
| `pub extern "C" fn _start() -> !` | Test kernel entry point. |
| `vlad_os::serial::init();` | Initializes COM1 serial output. |
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
| `vlad_os::qemu::test_panic_handler(info);` | Reports failure through serial and QEMU debug-exit. |

## `tests/memory_mapping.rs`

### Purpose

This test proves that the kernel can create one new virtual mapping with the
active page tables. It maps the same scratch virtual page used by the
page-fault test, writes through it, reads the value back, and exits QEMU only
after the write is verified.

### Invariants

- The test uses `bootloader::entry_point!` so it can receive `BootInfo`.
- The scratch page must be unmapped before the test maps it.
- The mapped frame must come from `BootInfoFrameAllocator`.
- The mapping must be flushed before the virtual address is accessed.
- Success is reported only after the volatile write/read round trip succeeds.

### Line-By-Line

| Code | Explanation |
| --- | --- |
| `#![no_std]` | The test kernel cannot use the Rust standard library. |
| `#![no_main]` | The test provides a boot entry point instead of a Rust `main`. |
| `#![feature(abi_x86_interrupt)]` | Enables the interrupt ABI for the test page-fault handler. |
| `use core::panic::PanicInfo;` | Imports panic information for the test panic handler. |
| `use vlad_os::memory::BootInfoFrameAllocator;` | Imports the early physical frame allocator. |
| `use vlad_os::qemu::{exit_qemu, QemuExitCode};` | Imports QEMU pass/fail exit support. |
| `use vlad_os::{gdt, hlt_loop, memory, serial_print, serial_println};` | Imports shared setup, memory, halt behavior, and serial output. |
| `use bootloader::{entry_point, BootInfo};` | Imports the typed boot entry macro and boot information structure. |
| `use x86_64::{ ... };` | Imports CR2, IDT types, paging traits, page types, flags, and virtual addresses. |
| `static mut TEST_IDT: InterruptDescriptorTable = ...;` | Stores the test-local IDT at a stable address. |
| `entry_point!(test_kernel_main);` | Generates `_start` and verifies that `test_kernel_main` accepts `&'static BootInfo`. |
| `fn test_kernel_main(boot_info: &'static BootInfo) -> !` | Defines the memory-mapping test entry point. |
| `vlad_os::serial::init();` | Configures COM1 before printing test output. |
| `serial_print!("memory_mapping::map_one_page...\t");` | Prints the test name without a newline so `[ok]` appears beside it. |
| `gdt::init();` | Initializes GDT/TSS setup before loading the test IDT. |
| `init_test_idt();` | Loads a page-fault handler that reports failure if the mapping proof faults. |
| `let physical_memory_offset = VirtAddr::new(boot_info.physical_memory_offset);` | Reads the runtime direct-map offset supplied by the bootloader. |
| `let mut mapper = unsafe { memory::init(physical_memory_offset) };` | Creates a mapper for the active page tables. |
| `let mut frame_allocator = unsafe { BootInfoFrameAllocator::init(&boot_info.memory_map) };` | Creates the monotonic frame allocator from the bootloader memory map. |
| `let page = Page::containing_address(VirtAddr::new(0x4444_4444_0000));` | Chooses the scratch virtual page, matching the address used by the page-fault test. |
| `mapper.translate_addr(page.start_address()).is_none()` | Asserts that the scratch page was unmapped before this test creates the mapping. |
| `frame_allocator.allocate_frame().expect(...)` | Allocates one fresh usable physical frame for the scratch page. |
| `PageTableFlags::PRESENT | PageTableFlags::WRITABLE` | Makes the new page present and writable. |
| `mapper.map_to(page, frame, flags, &mut frame_allocator)` | Adds the virtual-to-physical mapping, allocating intermediate page tables if needed. |
| `.flush();` | Flushes the page from the TLB before using the new mapping. |
| `let value = 0x_f021_f077_f065_f04e;` | Defines the known value used for the write/read proof. |
| `let ptr: *mut u64 = page.start_address().as_mut_ptr();` | Converts the scratch virtual page start into a writable pointer. |
| `core::ptr::write_volatile(ptr, value);` | Writes through the newly mapped virtual address without letting the compiler remove the access. |
| `assert_eq!(core::ptr::read_volatile(ptr), value);` | Reads back through the same virtual address and verifies the mapping works. |
| `serial_println!("[ok]");` | Reports success. |
| `exit_qemu(QemuExitCode::Success);` | Exits QEMU with the configured success status. |
| `hlt_loop();` | Halts forever if QEMU does not exit. |
| `fn init_test_idt()` | Builds and loads the test-local IDT. |
| `idt.page_fault.set_handler_fn(test_page_fault_handler);` | Installs a page-fault handler that marks the test failed. |
| `extern "x86-interrupt" fn test_page_fault_handler(...)` | Handles unexpected page faults during the mapping proof. |
| `let accessed_address = Cr2::read();` | Reads the faulting virtual address for diagnostics. |
| `serial_println!("[failed]");` | Marks the unexpected page fault as a test failure. |
| `exit_qemu(QemuExitCode::Failed);` | Exits QEMU with failure status. |
| `#[panic_handler]` | Defines panic behavior for this test kernel. |
| `vlad_os::qemu::test_panic_handler(info);` | Reports assertion failures and other panics through serial and QEMU debug-exit. |

## `tests/heap_allocation.rs`

### Purpose

This test proves that the fixed heap is mapped and that the global allocator can
serve real `alloc` crate types. It initializes the mapper and frame allocator,
calls `allocator::init_heap`, then checks `Box`, `Vec`, and repeated
allocation/deallocation.

### Invariants

- The test uses `bootloader::entry_point!` so it can receive `BootInfo`.
- The heap must be initialized before any `Box` or `Vec` is created.
- A test-local page-fault handler reports failure if a heap page was not mapped.
- Success is reported only after all allocation checks pass.

### Line-By-Line

| Code | Explanation |
| --- | --- |
| `extern crate alloc;` | Makes `alloc` crate types available in this no-std test kernel. |
| `use alloc::{boxed::Box, vec::Vec};` | Imports the heap-backed types used by the checks. |
| `use vlad_os::{allocator, ...};` | Imports fixed heap setup along with shared test setup helpers. |
| `entry_point!(test_kernel_main);` | Generates the boot entry point and passes `BootInfo` to the test. |
| `serial_print!("heap_allocation::heap_allocations...\t");` | Prints the test name without a newline so `[ok]` appears beside it. |
| `gdt::init();` | Initializes GDT/TSS before loading the test-local IDT. |
| `init_test_idt();` | Loads a page-fault handler that marks the test failed on unexpected faults. |
| `let mut mapper = unsafe { memory::init(physical_memory_offset) };` | Creates the active page-table mapper from the bootloader direct-map offset. |
| `let mut frame_allocator = unsafe { BootInfoFrameAllocator::init(&boot_info.memory_map) };` | Creates the monotonic physical frame allocator from usable bootloader regions. |
| `allocator::init_heap(&mut mapper, &mut frame_allocator).expect(...)` | Maps all heap pages and initializes the global allocator before any heap allocation occurs. |
| `simple_box_allocation();` | Allocates one `Box`, reads the value back, and drops it at function exit. |
| `vec_allocation_and_growth();` | Pushes 500 `u64` values into a `Vec`, forcing allocation and growth, then verifies the sum. |
| `many_boxes_with_deallocation();` | Repeatedly allocates and drops boxes, then allocates once more to show freed heap blocks are reusable. |
| `serial_println!("[ok]");` | Reports success after all heap checks pass. |
| `exit_qemu(QemuExitCode::Success);` | Exits QEMU with the configured success status. |
| `test_page_fault_handler(...)` | Prints CR2, the page-fault error code, and the stack frame before exiting QEMU failure. |
