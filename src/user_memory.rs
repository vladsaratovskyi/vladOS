use x86_64::structures::paging::PageTableFlags;
use x86_64::VirtAddr;

use crate::address_space::AddressSpace;
use crate::{memory, user};

const PAGE_SIZE: u64 = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserMemoryError {
    AddressOverflow,
    OutsideUserRange,
    Unmapped,
    NotWritable,
}

pub fn validate_user_read_range(
    address_space: &AddressSpace,
    start: VirtAddr,
    len: usize,
) -> Result<(), UserMemoryError> {
    validate_user_range(address_space, start, len, Access::Read)
}

pub fn validate_user_write_range(
    address_space: &AddressSpace,
    start: VirtAddr,
    len: usize,
) -> Result<(), UserMemoryError> {
    validate_user_range(address_space, start, len, Access::Write)
}

pub fn copy_from_user(
    address_space: &AddressSpace,
    dst: &mut [u8],
    src_user: VirtAddr,
) -> Result<(), UserMemoryError> {
    let mut copied = 0;

    while copied < dst.len() {
        let current = checked_offset(src_user, copied)?;
        let translation = validate_user_byte(address_space, current, Access::Read)?;
        let page_offset = (translation.phys.as_u64() & (PAGE_SIZE - 1)) as usize;
        let count = core::cmp::min(dst.len() - copied, PAGE_SIZE as usize - page_offset);

        memory::with_state(|state| unsafe {
            // The user virtual address has just been translated through the
            // task's page tables. Reading through the bootloader direct map
            // avoids dereferencing untrusted user virtual pointers in ring 0.
            let src = state.phys_ptr::<u8>(translation.phys);
            core::ptr::copy_nonoverlapping(src, dst[copied..].as_mut_ptr(), count);
        });

        copied += count;
    }

    Ok(())
}

pub fn copy_to_user(
    address_space: &AddressSpace,
    dst_user: VirtAddr,
    src: &[u8],
) -> Result<(), UserMemoryError> {
    let mut copied = 0;

    while copied < src.len() {
        let current = checked_offset(dst_user, copied)?;
        let translation = validate_user_byte(address_space, current, Access::Write)?;
        let page_offset = (translation.phys.as_u64() & (PAGE_SIZE - 1)) as usize;
        let count = core::cmp::min(src.len() - copied, PAGE_SIZE as usize - page_offset);

        memory::with_state(|state| unsafe {
            // The write target is a validated user-writable physical page.
            // The raw pointer is through the trusted direct physical map, not
            // through the untrusted user virtual address.
            let dst: *mut u8 =
                (state.physical_memory_offset() + translation.phys.as_u64()).as_mut_ptr();
            core::ptr::copy_nonoverlapping(src[copied..].as_ptr(), dst, count);
        });

        copied += count;
    }

    Ok(())
}

fn validate_user_range(
    address_space: &AddressSpace,
    start: VirtAddr,
    len: usize,
    access: Access,
) -> Result<(), UserMemoryError> {
    if len == 0 {
        return Ok(());
    }

    let start_addr = start.as_u64();
    let end_addr = start_addr
        .checked_add(len as u64)
        .ok_or(UserMemoryError::AddressOverflow)?;

    if start_addr < user::USER_BASE || end_addr > user::USER_STACK_TOP {
        return Err(UserMemoryError::OutsideUserRange);
    }

    let mut page = align_down(start_addr);
    while page < end_addr {
        validate_user_byte(address_space, VirtAddr::new(page), access)?;
        page = page
            .checked_add(PAGE_SIZE)
            .ok_or(UserMemoryError::AddressOverflow)?;
    }

    Ok(())
}

fn validate_user_byte(
    address_space: &AddressSpace,
    address: VirtAddr,
    access: Access,
) -> Result<crate::address_space::UserTranslation, UserMemoryError> {
    let addr = address.as_u64();
    if !(user::USER_BASE..user::USER_STACK_TOP).contains(&addr) {
        return Err(UserMemoryError::OutsideUserRange);
    }

    let translation = address_space
        .translate_user(address)
        .ok_or(UserMemoryError::Unmapped)?;

    if access == Access::Write && !translation.flags.contains(PageTableFlags::WRITABLE) {
        return Err(UserMemoryError::NotWritable);
    }

    Ok(translation)
}

fn checked_offset(start: VirtAddr, offset: usize) -> Result<VirtAddr, UserMemoryError> {
    Ok(VirtAddr::new(
        start
            .as_u64()
            .checked_add(offset as u64)
            .ok_or(UserMemoryError::AddressOverflow)?,
    ))
}

fn align_down(value: u64) -> u64 {
    value & !(PAGE_SIZE - 1)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Access {
    Read,
    Write,
}
