# Allocator Walkthrough

Back to the [architecture guide](../architecture.md) or the
[walkthrough index](README.md).

This page covers `src/allocator.rs`.

## Purpose

`allocator.rs` creates the first kernel heap. The heap is deliberately fixed in
size and virtual location so the kernel can use `alloc` types without designing
heap growth or a production allocator yet.

The setup has two layers:

- paging maps every heap virtual page to a fresh usable physical frame
- `linked_list_allocator` manages allocations inside those already mapped
  virtual bytes

## Invariants

- The heap starts at `0x5555_5555_0000` and is 100 KiB.
- Every heap page must be mapped before the global allocator is initialized.
- Heap mappings must be present and writable.
- Physical frames come from `BootInfoFrameAllocator` and are not reclaimed.
- This heap does not grow and does not demand-map pages.

## Line-By-Line

| Code | Explanation |
| --- | --- |
| `use linked_list_allocator::LockedHeap;` | Imports the small no-std allocator used for the first heap milestone. |
| `use x86_64::{ ... };` | Imports paging traits, page types, page-table flags, and virtual-address helpers. |
| `pub const HEAP_START: usize = 0x_5555_5555_0000;` | Chooses the fixed virtual start address for the kernel heap. This avoids the `0x4444_4444_0000` scratch address used by paging tests. |
| `pub const HEAP_SIZE: usize = 100 * 1024;` | Reserves 100 KiB of virtual heap space, enough for `Box`, `Vec`, and small early-kernel experiments. |
| `#[global_allocator]` | Registers the following allocator as the allocator used by the `alloc` crate. |
| `static ALLOCATOR: LockedHeap = LockedHeap::empty();` | Creates an empty locked heap. It cannot allocate until `init_heap` gives it mapped memory. |
| `pub fn init_heap(...) -> Result<(), MapToError<Size4KiB>>` | Maps the fixed heap range and initializes the global allocator. It returns paging errors so callers can panic or fail tests with context. |
| `let heap_start = VirtAddr::new(HEAP_START as u64);` | Converts the heap start constant into the address type used by `x86_64`. |
| `let heap_end = heap_start + HEAP_SIZE as u64 - 1u64;` | Computes the inclusive last byte in the heap. Subtracting one ensures the page range contains exactly the heap bytes. |
| `Page::containing_address(...)` | Finds the first and last 4 KiB pages touched by the heap range. |
| `Page::range_inclusive(...)` | Builds the iterator over every heap page that needs a mapping. |
| `frame_allocator.allocate_frame()` | Requests one fresh usable physical frame for the current heap page. |
| `MapToError::FrameAllocationFailed` | Converts physical-memory exhaustion into the paging error type returned by this function. |
| `PageTableFlags::PRESENT | PageTableFlags::WRITABLE` | Makes heap pages accessible and writable by kernel code. |
| `mapper.map_to(page, frame, flags, frame_allocator)?.flush();` | Installs the virtual-to-physical mapping, lets the mapper allocate intermediate page tables if needed, and flushes the TLB entry before use. |
| `ALLOCATOR.lock().init(HEAP_START as *mut u8, HEAP_SIZE);` | Gives the linked-list allocator the mapped virtual heap range. After this line, `Box` and `Vec` can allocate. |
| `Ok(())` | Reports successful heap setup to the boot path or QEMU test. |

## Current Limits

This is an early educational heap, not the final memory manager. Deallocating a
`Box` or shrinking a `Vec` returns virtual heap blocks to `linked_list_allocator`,
but it does not return physical frames to `BootInfoFrameAllocator`. The heap
range is fixed, cannot grow, and is mapped eagerly during boot or test setup.
