# Integration Tests Walkthrough

Back to the [architecture guide](../architecture.md) or the
[walkthrough index](README.md).

This page covers:

- `tests/stack_overflow.rs`
- `tests/page_fault.rs`
- `tests/memory_mapping.rs`
- `tests/heap_allocation.rs`
- `tests/interrupts.rs`
- `tests/cooperative_tasks.rs`
- `tests/preemptive_tasks.rs`
- `tests/userspace.rs`
- `tests/address_spaces.rs`
- `tests/elf_loader.rs`

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

## `tests/interrupts.rs`

### Purpose

This test proves that the interrupt foundation can be initialized in a bootable
kernel: production IDT entries are installed, the legacy PICs are remapped, the
PIT is programmed, and the public IRQ indexes match the expected timer and
keyboard vectors.

It deliberately does not enable external interrupts or wait for a PIT tick.
That keeps the test deterministic and avoids hanging QEMU if an interrupt is
not delivered in the test environment.

### Invariants

- The test uses the production `interrupts` module.
- The GDT must be initialized before the production IDT because the IDT includes
  the double-fault IST stack.
- PIC and PIT setup must complete before the test exits.
- Success is reported only after the timer and keyboard vector constants and
  initial tick counter are checked.

### Line-By-Line

| Code | Explanation |
| --- | --- |
| `#![no_std]` | The test kernel cannot use the Rust standard library. |
| `#![no_main]` | The test provides its own `_start` entry point. |
| `use vlad_os::{gdt, hlt_loop, interrupts, serial_print, serial_println};` | Imports the production CPU-table and interrupt setup plus serial output. |
| `pub extern "C" fn _start() -> !` | Defines the test kernel entry point. It never returns. |
| `vlad_os::serial::init();` | Configures COM1 before printing test output. |
| `serial_print!("interrupts::pic_pit_foundation...\t");` | Prints the test name without a newline so `[ok]` appears beside it. |
| `gdt::init();` | Initializes the GDT and TSS before loading the production IDT. |
| `interrupts::init_idt();` | Loads the production IDT with exception, timer, and keyboard handlers. |
| `interrupts::init_pics();` | Remaps the legacy PICs and unmasks only IRQ0 and IRQ1. |
| `interrupts::init_pit();` | Programs PIT channel 0 for the early timer source. |
| `InterruptIndex::Timer.as_u8()` | Verifies that timer IRQ0 maps to `PIC_1_OFFSET`, vector 32. |
| `InterruptIndex::Keyboard.as_u8()` | Verifies that keyboard IRQ1 maps to `PIC_1_OFFSET + 1`, vector 33. |
| `interrupts::timer_ticks()` | Confirms the tick counter starts at zero before external interrupts are enabled. |
| `serial_println!("[ok]");` | Reports success after all checks pass. |
| `exit_qemu(QemuExitCode::Success);` | Exits QEMU with the configured success status. |
| `#[panic_handler]` | Defines panic behavior for this test kernel. |
| `vlad_os::qemu::test_panic_handler(info);` | Reports assertion failures through serial and QEMU debug-exit. |

## `tests/cooperative_tasks.rs`

### Purpose

This test proves that two stackful kernel tasks can run on separate stacks,
voluntarily yield to each other, preserve task-local state across switches, and
finish without being resumed.

### Invariants

- The fixed heap must be initialized before spawning tasks because each task
  allocates an 8 KiB stack.
- The test uses deterministic atomic step checks, not PIT timing.
- Both task entry functions must return so the scheduler exercises finished
  task handling.
- Success is reported only after the scheduler returns to the test kernel and
  both tasks are marked finished.

### Line-By-Line

| Code | Explanation |
| --- | --- |
| `#![no_std]` | The test kernel cannot use the Rust standard library. |
| `#![no_main]` | The bootloader enters through this test's generated `_start`. |
| `static STEP: AtomicUsize = AtomicUsize::new(0);` | Shared deterministic schedule counter. Each task advances it only when it runs at the expected point. |
| `static COMPLETED_TASKS: AtomicUsize = AtomicUsize::new(0);` | Counts task entries that reached their normal return path. |
| `entry_point!(test_kernel_main);` | Generates the boot entry point and passes `BootInfo` to the test. |
| `vlad_os::serial::init();` | Configures COM1 before printing test output. |
| `serial_print!("cooperative_tasks::round_robin_yield...\t");` | Prints the test name without a newline so `[ok]` appears beside it. |
| `gdt::init();` | Initializes the GDT/TSS before loading the production IDT. |
| `interrupts::init_idt();` | Installs the production IDT, including the software yield vector used by `scheduler::yield_now()`. |
| `memory::init(...)` and `BootInfoFrameAllocator::init(...)` | Reuses the normal page-table and frame allocator setup. |
| `allocator::init_heap(...)` | Maps the fixed heap before task stack allocation. |
| `scheduler::spawn(task_a)` and `scheduler::spawn(task_b)` | Creates two tasks with dedicated kernel stacks. |
| `scheduler::run();` | Switches away from the test stack and returns only after no runnable tasks remain. |
| `assert_eq!(STEP.load(...), 6);` | Verifies the exact observed schedule: A, B, A, B, A, B. |
| `assert_eq!(COMPLETED_TASKS.load(...), 2);` | Verifies both task entry functions reached completion. |
| `scheduler::finished_task_count()` | Verifies the scheduler marked both tasks finished. |
| `scheduler::all_tasks_finished()` | Verifies no task remains ready or running. |
| `task_a()` | Uses a local variable, yields twice, and checks that the local value survived each context switch. |
| `task_b()` | Mirrors task A with a different local value and complementary schedule steps. |
| `expect_step(expected)` | Uses atomic compare-exchange so a wrong task order fails immediately. |
| `#[panic_handler]` | Defines panic behavior for this test kernel. |
| `vlad_os::qemu::test_panic_handler(info);` | Reports failed assertions through serial and QEMU debug-exit. |

## `tests/preemptive_tasks.rs`

### Purpose

This test proves that PIT timer interrupts can preempt a running kernel task.
The two task functions do not call `yield_now()` during the proof; success
requires both task-local counters to make progress and one task to finish.

### Invariants

- The production IDT, PIC, and PIT setup must be used.
- CPU interrupts are enabled before spawning tasks so their initial frames have
  IF set.
- Preemption is explicitly enabled only after both tasks are spawned.
- The success path is based on counters and task completion, not a fixed delay.
- Bounded tick and local-loop guards fail the test instead of hanging forever.

### Line-By-Line

| Code | Explanation |
| --- | --- |
| `static A_PROGRESS` and `static B_PROGRESS` | Atomically publish each task's local progress so the other task can observe it. |
| `static A_DONE` | Records that task A returned through the scheduler finish path. |
| `PROGRESS_TARGET` | Minimum per-task local counter value required for success. |
| `TICK_TIMEOUT` and `LOCAL_STALL_LIMIT` | Deterministic failure guards for a broken preemptive path. |
| `gdt::init(); interrupts::init_idt(); interrupts::init_pics(); interrupts::init_pit();` | Builds the real CPU and legacy IRQ path used by timer preemption. |
| `allocator::init_heap(...)` | Maps the fixed heap before task stack allocation. |
| `interrupts::enable_interrupts();` | Enables CPU interrupt delivery after IDT, PIC, PIT, and heap setup are complete. |
| `scheduler::spawn(task_a)` and `scheduler::spawn(task_b)` | Creates two stackful kernel tasks. |
| `scheduler::enable_preemption();` | Opens the timer-driven scheduling gate after valid task state exists. |
| `scheduler::run();` | Starts the scheduler. The test expects QEMU success before this returns. |
| `task_a()` | Busy-loops with a local counter, publishes progress, and returns only after task B has also progressed. |
| `task_b()` | Busy-loops with its own local counter and exits QEMU successfully only after task A finished and both counters reached the target. |
| no `scheduler::yield_now()` calls | Ensures success depends on timer preemption, not cooperative switching. |
| `#[panic_handler]` | Reports assertion failures through serial and QEMU debug-exit. |

## `tests/userspace.rs`

### Purpose

This test proves the first userspace foundation: a task enters CPL3 through an
`iretq` frame, returns to the kernel with `int 0x80`, exits by syscall, contains
a user-mode privileged-instruction fault, and can be preempted by the PIT while
running in user mode.

### Invariants

- The production GDT, TSS, IDT, PIC, PIT, heap, scheduler, syscall,
  address-space, and user setup helpers are used together.
- User code never calls kernel scheduler functions directly.
- User code/data/stacks are mapped with `USER_ACCESSIBLE` in per-task address
  spaces.
- Success depends on deterministic marker writes and scheduler state.
- The privileged `hlt` fault must terminate only the user task, not the kernel.

### Line-By-Line

| Code | Explanation |
| --- | --- |
| `gdt::init(); interrupts::init_idt(); interrupts::init_pics(); interrupts::init_pit();` | Builds the real descriptor, syscall, and timer paths needed for ring transitions. |
| `allocator::init_heap(...)` | Maps the fixed heap before task stack allocation. |
| `memory::init_global(...)` | Stores the direct-map offset and remaining frame allocator so user address spaces can be built later. |
| `interrupts::enable_interrupts();` | Enables timer delivery before spawning tasks, so user frames can run with IF set. |
| `scheduler::spawn(orchestrator)` | Creates the kernel task that checks every userspace scenario. |
| `user::create_user_task(... YieldThenExit, USER_DATA_BASE)` | Creates an isolated address space with copied user code, private data, and private user stack. |
| `scheduler::spawn_user(...)` | Adds the prepared user task to the scheduler. |
| `scheduler::yield_now()` in `orchestrator` | Gives the user task a chance to run and then expects the user syscall yield to switch back. |
| marker checks after first yield | Prove user code ran before the syscall and had not yet executed the post-yield marker. |
| marker checks after second yield | Prove the syscall returned to user mode and syscall exit marked the task finished. |
| spawning the faulting user task | Starts a second user task after the first one exited. |
| `scheduler::task_fault_info(...)` | Verifies the user #GP path marked only that task failed and recorded the fault. |
| spawning the busy user task | Starts a user loop that cannot return control by cooperative yield. |
| `scheduler::enable_preemption(); scheduler::yield_now();` | Gives the busy user task the CPU; the orchestrator can resume only through a timer preemption from CPL3. |
| `timer_ticks() > start_ticks` and busy marker check | Prove the PIT fired while user code was running and that the busy task made progress. |
| `serial_println!("[ok]"); exit_qemu(...)` | Reports success only after all userspace checks pass. |

## `tests/address_spaces.rs`

### Purpose

This test proves that user tasks have isolated page-table roots and that CR3
switching, user page-fault containment, and timer preemption work across those
roots.

### Invariants

- Every user task is created with a fresh `AddressSpace`.
- The same user virtual address can map to different physical frames in
  different tasks.
- User faults must mark only the faulting task failed.
- Kernel mappings must remain present for ring 0 but supervisor-only for ring 3.

### Line-By-Line

| Code | Explanation |
| --- | --- |
| `memory::init_global(...)` | Makes the direct-map offset and remaining frame allocator available to address-space creation. |
| `same_virtual_address_is_private()` | Spawns two user tasks that both use `USER_DATA_BASE`; one exits with `0xaa`, the other with `0xbb`. |
| `unmapped_user_page_is_task_local()` | Gives only task B a mapping at `USER_TEST_PAGE_BASE`; task A faults there while task B exits with the mapped value. |
| `kernel_mapping_is_supervisor_only()` | Passes a kernel function address to user code and expects a user page fault, proving kernel mappings are not user-accessible. |
| `preemption_crosses_cr3_roots()` | Runs a busy user loop in one address space and a second user task in another; success requires timer preemption across CR3 roots. |

## `tests/elf_loader.rs`

### Purpose

This test proves the embedded ELF loader path: user programs are represented as
ELF64 byte streams, loaded into fresh address spaces, started at `e_entry`, and
scheduled through the existing syscall, fault, and preemption paths.

### Invariants

- Test ELFs are embedded with `include_bytes!` from files generated by
  `build.rs`.
- Bad ELF inputs must be rejected before they become tasks.
- ELF-backed user tasks use the same per-task address-space isolation as the
  earlier user snippets.
- Read-only segment writes must fail as contained user page faults.

### Line-By-Line

| Code | Explanation |
| --- | --- |
| `include_bytes!(concat!(env!("OUT_DIR"), "..."))` | Embeds the generated ELF fixtures in the test kernel image. |
| `elf_rejects_bad_magic()` | Passes non-ELF bytes to the loader and expects `BadMagic`. |
| `elf_rejects_bad_machine()` | Passes a generated ELF with the wrong `e_machine` and expects `UnsupportedMachine`. |
| `scheduler::spawn_user_elf("exit_42", EXIT_42_ELF)` | Loads an ELF into a fresh address space and registers it as a user task. |
| `scheduler::task_exit_code(...) == Some(42)` | Proves the task started at the ELF entry point and reached syscall exit. |
| `spawn_user_elf_with_arg(... WRITE_PRIVATE_DATA_ELF, value)` | Starts two instances of the same ELF with different initial `rdi` values. |
| user-memory checks at `USER_DATA_BASE` | Prove the same user virtual address maps to private physical memory per ELF task. |
| `WRITE_READONLY_SEGMENT_ELF` | Attempts to write to a non-writable load segment; success requires a contained user page fault. |
| `BUSY_COUNTER_ELF` with preemption enabled | Proves PIT preemption still crosses CR3 roots for ELF-backed tasks. |
