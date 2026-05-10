# Memory Walkthrough

Back to the [architecture guide](../architecture.md) or the
[walkthrough index](README.md).

This page covers `src/memory.rs`.

## Purpose

`memory.rs` contains the first memory-management foundation. It does not create
a heap or a final virtual address-space layout. It gives the kernel enough tools
to inspect and edit the active page-table hierarchy:

- access the active level-4 page table through `CR3`
- use the bootloader-provided physical-memory offset mapping
- create an `OffsetPageTable`
- allocate unused 4 KiB physical frames from the bootloader memory map

## Invariants

- `physical_memory_offset` must be the value supplied by `BootInfo`.
- The bootloader must have mapped all physical memory at that offset.
- The active level-4 table frame read from `CR3` must belong to the active page
  table hierarchy.
- `BootInfoFrameAllocator` may hand out only frames from
  `MemoryRegionType::Usable` regions.
- The frame allocator is monotonic: it never frees frames and is intentionally
  only an early bootstrap allocator.

## Line-By-Line

| Code | Explanation |
| --- | --- |
| `use bootloader::bootinfo::{MemoryMap, MemoryRegionType};` | Imports the bootloader memory-map type and the region classification enum. |
| `use x86_64::{ ... };` | Imports the control-register, paging, physical-address, and virtual-address types needed to work with page tables. |
| `pub unsafe fn init(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static>` | Builds a mapper for the current page-table hierarchy. It is unsafe because the caller must pass the correct bootloader offset. |
| `let level_4_table = unsafe { active_level_4_table(physical_memory_offset) };` | Finds the active L4 page table by reading `CR3` and translating the table's physical address through the direct physical-memory mapping. |
| `OffsetPageTable::new(level_4_table, physical_memory_offset)` | Creates an `x86_64` mapper that can walk and edit page tables through the offset mapping. |
| `unsafe fn active_level_4_table(...) -> &'static mut PageTable` | Returns a mutable reference to the active level-4 table. This is unsafe because aliasing or using the wrong table would break memory safety. |
| `let (level_4_table_frame, _) = Cr3::read();` | Reads the physical frame address of the active level-4 page table from `CR3`. |
| `let phys = level_4_table_frame.start_address();` | Gets the physical start address of that frame. |
| `let virt = physical_memory_offset + phys.as_u64();` | Converts the physical page-table address to a virtual address using the bootloader-provided mapping. |
| `let page_table_ptr: *mut PageTable = virt.as_mut_ptr();` | Treats that virtual address as a raw pointer to an x86_64 page table. |
| `&mut *page_table_ptr` | Converts the raw pointer into the mutable reference required by `OffsetPageTable`. |
| `pub struct BootInfoFrameAllocator` | Stores the bootloader memory map and the index of the next frame to hand out. |
| `memory_map: &'static MemoryMap` | Borrows the memory map supplied by the bootloader for the whole kernel lifetime. |
| `next: usize` | Counts how many usable frames have already been skipped/allocated. |
| `pub unsafe fn init(memory_map: &'static MemoryMap) -> Self` | Creates the allocator. It is unsafe because the caller must guarantee the memory map is trustworthy and that usable frames are not allocated elsewhere. |
| `fn usable_frames(&self) -> impl Iterator<Item = PhysFrame>` | Builds an iterator over every 4 KiB frame in every usable memory region. |
| `regions.filter(|region| region.region_type == MemoryRegionType::Usable)` | Ignores bootloader, kernel, page-table, reserved, ACPI, bad-memory, and other non-usable regions. |
| `region.range.start_addr()..region.range.end_addr()` | Converts each usable frame range into a physical address range. |
| `range.step_by(4096)` | Steps through each physical range one 4 KiB frame at a time. |
| `PhysFrame::containing_address(PhysAddr::new(addr))` | Converts each frame start address into an `x86_64` physical frame value. |
| `unsafe impl FrameAllocator<Size4KiB> for BootInfoFrameAllocator` | Implements the allocator trait used by the `x86_64` paging mapper. The impl is unsafe because it promises to return only unique unused frames. |
| `let frame = self.usable_frames().nth(self.next);` | Recreates the usable-frame iterator and selects the next not-yet-returned frame. This is simple, not efficient. |
| `self.next += 1;` | Advances the monotonic allocation counter. |
| `frame` | Returns `Some(frame)` while usable frames remain, or `None` when memory is exhausted. |
