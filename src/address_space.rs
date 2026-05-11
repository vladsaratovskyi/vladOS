use x86_64::registers::control::{Cr3, Cr3Flags};
use x86_64::structures::paging::{
    mapper::MapToError, Mapper, OffsetPageTable, Page, PageTableFlags, PhysFrame, Size4KiB,
};
use x86_64::{PhysAddr, VirtAddr};

use crate::memory;

pub const USER_P4_INDEX: usize = 1;
const PAGE_SIZE: u64 = 4096;

#[derive(Debug)]
pub enum AddressSpaceError {
    FrameAllocationFailed,
    KernelUserSlotInUse,
    RangeOverflow,
    MapTo(MapToError<Size4KiB>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserCopyError {
    RangeOverflow,
    NotMapped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserMapFlags {
    pub readable: bool,
    pub writable: bool,
    pub executable: bool,
}

impl UserMapFlags {
    pub const fn new(readable: bool, writable: bool, executable: bool) -> Self {
        Self {
            readable,
            writable,
            executable,
        }
    }

    pub const fn read_execute() -> Self {
        Self::new(true, false, true)
    }

    pub const fn read_write() -> Self {
        Self::new(true, true, false)
    }

    fn page_table_flags(self) -> PageTableFlags {
        let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;

        if self.writable {
            flags |= PageTableFlags::WRITABLE;
        }

        flags
    }
}

#[derive(Debug)]
pub struct AddressSpace {
    level_4_frame: PhysFrame<Size4KiB>,
}

impl AddressSpace {
    pub fn new_user() -> Result<Self, AddressSpaceError> {
        memory::with_state(|state| {
            let level_4_frame = state
                .allocate_frame()
                .ok_or(AddressSpaceError::FrameAllocationFailed)?;

            unsafe {
                let new_level_4 = state.page_table_mut(level_4_frame);
                new_level_4.zero();

                let kernel_level_4 = state.page_table(state.kernel_level_4_frame());

                for (index, source_entry) in kernel_level_4.iter().enumerate() {
                    if index == USER_P4_INDEX {
                        if !source_entry.is_unused() {
                            return Err(AddressSpaceError::KernelUserSlotInUse);
                        }

                        continue;
                    }

                    let mut entry = source_entry.clone();

                    if !entry.is_unused() {
                        let mut flags = entry.flags();
                        flags.remove(PageTableFlags::USER_ACCESSIBLE);
                        entry.set_flags(flags);
                    }

                    new_level_4[index] = entry;
                }
            }

            Ok(Self { level_4_frame })
        })
    }

    pub fn kernel() -> Self {
        Self {
            level_4_frame: memory::kernel_level_4_frame(),
        }
    }

    pub fn level_4_frame(&self) -> PhysFrame<Size4KiB> {
        self.level_4_frame
    }

    pub fn load(&self) {
        unsafe {
            Cr3::write(self.level_4_frame, Cr3Flags::empty());
        }
    }

    pub fn map_zeroed_user_page(
        &mut self,
        address: VirtAddr,
        flags: PageTableFlags,
    ) -> Result<PhysFrame<Size4KiB>, AddressSpaceError> {
        let frame = memory::with_state(|state| {
            let frame = state
                .allocate_frame()
                .ok_or(AddressSpaceError::FrameAllocationFailed)?;

            unsafe {
                state.frame_slice_mut(frame).fill(0);
            }

            Ok(frame)
        })?;

        self.map_frame(address, frame, flags)?;

        Ok(frame)
    }

    pub fn map_user_region(
        &mut self,
        virt_start: VirtAddr,
        size: usize,
        flags: UserMapFlags,
    ) -> Result<(), AddressSpaceError> {
        if size == 0 {
            return Ok(());
        }

        let start = align_down(virt_start.as_u64(), PAGE_SIZE);
        let end = align_up(
            virt_start
                .as_u64()
                .checked_add(size as u64)
                .ok_or(AddressSpaceError::RangeOverflow)?,
            PAGE_SIZE,
        )
        .ok_or(AddressSpaceError::RangeOverflow)?;

        let mut address = start;
        while address < end {
            self.map_zeroed_user_page(VirtAddr::new(address), flags.page_table_flags())?;
            address = address
                .checked_add(PAGE_SIZE)
                .ok_or(AddressSpaceError::RangeOverflow)?;
        }

        Ok(())
    }

    pub fn map_frame(
        &mut self,
        address: VirtAddr,
        frame: PhysFrame<Size4KiB>,
        flags: PageTableFlags,
    ) -> Result<(), AddressSpaceError> {
        let page = Page::containing_address(address);

        memory::with_state(|state| {
            let physical_memory_offset = state.physical_memory_offset();
            let level_4_table = unsafe { state.page_table_mut(self.level_4_frame) };
            let mut mapper = unsafe { OffsetPageTable::new(level_4_table, physical_memory_offset) };

            unsafe {
                mapper
                    .map_to(page, frame, flags, state.frame_allocator_mut())
                    .map_err(AddressSpaceError::MapTo)?
                    .flush();
            }

            Ok(())
        })
    }

    pub fn copy_to_user(&mut self, dst: VirtAddr, src: &[u8]) -> Result<(), UserCopyError> {
        let mut copied = 0;

        while copied < src.len() {
            let dst_addr = dst
                .as_u64()
                .checked_add(copied as u64)
                .ok_or(UserCopyError::RangeOverflow)?;
            let phys = self
                .translate_user(VirtAddr::new(dst_addr))
                .ok_or(UserCopyError::NotMapped)?;
            let page_offset = (phys.as_u64() & (PAGE_SIZE - 1)) as usize;
            let count = core::cmp::min(src.len() - copied, PAGE_SIZE as usize - page_offset);

            memory::with_state(|state| unsafe {
                let dst_ptr: *mut u8 =
                    (state.physical_memory_offset() + phys.as_u64()).as_mut_ptr();
                core::ptr::copy_nonoverlapping(src[copied..].as_ptr(), dst_ptr, count);
            });

            copied += count;
        }

        Ok(())
    }

    pub fn zero_user_range(&mut self, dst: VirtAddr, len: usize) -> Result<(), UserCopyError> {
        let mut zeroed = 0;

        while zeroed < len {
            let dst_addr = dst
                .as_u64()
                .checked_add(zeroed as u64)
                .ok_or(UserCopyError::RangeOverflow)?;
            let phys = self
                .translate_user(VirtAddr::new(dst_addr))
                .ok_or(UserCopyError::NotMapped)?;
            let page_offset = (phys.as_u64() & (PAGE_SIZE - 1)) as usize;
            let count = core::cmp::min(len - zeroed, PAGE_SIZE as usize - page_offset);

            memory::with_state(|state| unsafe {
                let dst_ptr: *mut u8 =
                    (state.physical_memory_offset() + phys.as_u64()).as_mut_ptr();
                core::ptr::write_bytes(dst_ptr, 0, count);
            });

            zeroed += count;
        }

        Ok(())
    }

    pub fn write_frame_bytes(
        &self,
        frame: PhysFrame<Size4KiB>,
        bytes: &[u8],
    ) -> Result<(), AddressSpaceError> {
        if bytes.len() > 4096 {
            return Err(AddressSpaceError::FrameAllocationFailed);
        }

        memory::with_state(|state| {
            let slice = unsafe { state.frame_slice_mut(frame) };
            slice[..bytes.len()].copy_from_slice(bytes);

            Ok(())
        })
    }

    pub fn write_frame_u64(&self, frame: PhysFrame<Size4KiB>, value: u64) {
        memory::with_state(|state| {
            let slice = unsafe { state.frame_slice_mut(frame) };
            slice[..core::mem::size_of::<u64>()].copy_from_slice(&value.to_le_bytes());
        });
    }

    pub fn read_user_u64(&self, address: VirtAddr) -> Option<u64> {
        let phys = self.translate_user(address)?;
        let offset = phys.as_u64() & 0xfff;

        if offset > 4096 - core::mem::size_of::<u64>() as u64 {
            return None;
        }

        memory::with_state(|state| unsafe {
            let ptr = state.phys_ptr::<u64>(phys);
            Some(ptr.read_volatile())
        })
    }

    pub fn user_page_is_accessible(&self, address: VirtAddr) -> bool {
        self.translate_user(address).is_some()
    }

    fn translate_user(&self, address: VirtAddr) -> Option<PhysAddr> {
        memory::with_state(|state| {
            let page = Page::<Size4KiB>::containing_address(address);
            let level_4 = unsafe { state.page_table(self.level_4_frame) };

            let p4_entry = &level_4[page.p4_index()];
            if !entry_allows_user(p4_entry.flags()) {
                return None;
            }

            let level_3 = unsafe { state.page_table(p4_entry.frame().ok()?) };
            let p3_entry = &level_3[page.p3_index()];
            if !entry_allows_user(p3_entry.flags()) {
                return None;
            }

            let level_2 = unsafe { state.page_table(p3_entry.frame().ok()?) };
            let p2_entry = &level_2[page.p2_index()];
            if !entry_allows_user(p2_entry.flags()) {
                return None;
            }

            let level_1 = unsafe { state.page_table(p2_entry.frame().ok()?) };
            let p1_entry = &level_1[page.p1_index()];
            if !entry_allows_user(p1_entry.flags()) {
                return None;
            }

            let page_offset = address.as_u64() & 0xfff;
            Some(p1_entry.frame().ok()?.start_address() + page_offset)
        })
    }
}

fn entry_allows_user(flags: PageTableFlags) -> bool {
    flags.contains(PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE)
}

fn align_down(value: u64, align: u64) -> u64 {
    value & !(align - 1)
}

fn align_up(value: u64, align: u64) -> Option<u64> {
    Some(value.checked_add(align - 1)? & !(align - 1))
}
