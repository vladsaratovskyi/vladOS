use alloc::vec::Vec;

use x86_64::VirtAddr;

use crate::address_space::{AddressSpace, AddressSpaceError, UserCopyError, UserMapFlags};
use crate::user::{
    map_user_stack, UserTaskInit, USER_ELF_LOAD_END, USER_ELF_LOAD_START, USER_STACK_TOP,
};

const PAGE_SIZE: u64 = 4096;

const EI_CLASS: usize = 4;
const EI_DATA: usize = 5;
const EI_VERSION: usize = 6;
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;

const ET_EXEC: u16 = 2;
const EM_X86_64: u16 = 0x3e;
const EV_CURRENT: u32 = 1;

const ELF_HEADER_SIZE: usize = 64;
const PROGRAM_HEADER_SIZE: usize = 56;
const PT_LOAD: u32 = 1;
const PF_X: u32 = 1;
const PF_W: u32 = 2;
const PF_R: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElfLoadError {
    BadMagic,
    UnsupportedClass,
    UnsupportedEndian,
    UnsupportedType,
    UnsupportedMachine,
    MalformedHeader,
    MalformedProgramHeader,
    SegmentOutsideUserRange,
    SegmentMemSizeLessThanFileSize,
    SegmentOverlap,
    UnalignedSegment,
    EntryNotExecutable,
    OutOfFrames,
    MapFailed,
    UserCopyFailed,
}

struct ParsedElf<'a> {
    entry: u64,
    segments: Vec<LoadSegment<'a>>,
}

struct LoadSegment<'a> {
    vaddr: u64,
    filesz: u64,
    memsz: u64,
    flags: u32,
    data: &'a [u8],
    map_start: u64,
    map_end: u64,
}

pub fn load_user_elf(elf_bytes: &'static [u8], arg0: u64) -> Result<UserTaskInit, ElfLoadError> {
    let elf = parse_elf(elf_bytes)?;
    let mut address_space = AddressSpace::new_user().map_err(map_address_space_error)?;

    for segment in &elf.segments {
        address_space
            .map_user_region(
                VirtAddr::new(segment.map_start),
                (segment.map_end - segment.map_start) as usize,
                segment.user_flags(),
            )
            .map_err(map_address_space_error)?;

        address_space
            .copy_to_user(VirtAddr::new(segment.vaddr), segment.data)
            .map_err(map_user_copy_error)?;

        if segment.memsz > segment.filesz {
            address_space
                .zero_user_range(
                    VirtAddr::new(segment.vaddr + segment.filesz),
                    (segment.memsz - segment.filesz) as usize,
                )
                .map_err(map_user_copy_error)?;
        }
    }

    map_user_stack(&mut address_space).map_err(map_address_space_error)?;

    Ok(UserTaskInit {
        address_space,
        entry_point: VirtAddr::new(elf.entry),
        user_stack_top: VirtAddr::new(USER_STACK_TOP),
        arg0,
    })
}

pub fn parse(elf_bytes: &'static [u8]) -> Result<(), ElfLoadError> {
    parse_elf(elf_bytes).map(|_| ())
}

fn parse_elf(elf_bytes: &'static [u8]) -> Result<ParsedElf<'static>, ElfLoadError> {
    if elf_bytes.len() < 4 {
        return Err(ElfLoadError::MalformedHeader);
    }

    if &elf_bytes[0..4] != b"\x7fELF" {
        return Err(ElfLoadError::BadMagic);
    }

    if elf_bytes.len() < ELF_HEADER_SIZE {
        return Err(ElfLoadError::MalformedHeader);
    }

    if elf_bytes[EI_CLASS] != ELFCLASS64 {
        return Err(ElfLoadError::UnsupportedClass);
    }

    if elf_bytes[EI_DATA] != ELFDATA2LSB {
        return Err(ElfLoadError::UnsupportedEndian);
    }

    if elf_bytes[EI_VERSION] != EV_CURRENT as u8 || read_u32(elf_bytes, 20)? != EV_CURRENT {
        return Err(ElfLoadError::MalformedHeader);
    }

    if read_u16(elf_bytes, 16)? != ET_EXEC {
        return Err(ElfLoadError::UnsupportedType);
    }

    if read_u16(elf_bytes, 18)? != EM_X86_64 {
        return Err(ElfLoadError::UnsupportedMachine);
    }

    if read_u16(elf_bytes, 52)? as usize != ELF_HEADER_SIZE {
        return Err(ElfLoadError::MalformedHeader);
    }

    if read_u16(elf_bytes, 54)? as usize != PROGRAM_HEADER_SIZE {
        return Err(ElfLoadError::MalformedProgramHeader);
    }

    let entry = read_u64(elf_bytes, 24)?;
    let program_header_offset = read_u64(elf_bytes, 32)?;
    let program_header_count = read_u16(elf_bytes, 56)? as usize;
    if program_header_count == 0 {
        return Err(ElfLoadError::MalformedHeader);
    }

    let program_headers_size = PROGRAM_HEADER_SIZE
        .checked_mul(program_header_count)
        .ok_or(ElfLoadError::MalformedProgramHeader)?;
    let program_headers_end = checked_usize_range_end(program_header_offset, program_headers_size)
        .ok_or(ElfLoadError::MalformedProgramHeader)?;
    if program_headers_end > elf_bytes.len() {
        return Err(ElfLoadError::MalformedProgramHeader);
    }

    let mut segments = Vec::new();
    for index in 0..program_header_count {
        let offset = program_header_offset as usize + index * PROGRAM_HEADER_SIZE;
        let segment = parse_program_header(elf_bytes, offset)?;
        check_segment_overlap(&segments, &segment)?;
        segments.push(segment);
    }

    if segments.is_empty() {
        return Err(ElfLoadError::MalformedProgramHeader);
    }

    if !segments.iter().any(|segment| segment.contains_entry(entry)) {
        return Err(ElfLoadError::EntryNotExecutable);
    }

    Ok(ParsedElf { entry, segments })
}

fn parse_program_header(
    elf_bytes: &'static [u8],
    offset: usize,
) -> Result<LoadSegment<'static>, ElfLoadError> {
    let p_type = read_u32(elf_bytes, offset)?;
    if p_type != PT_LOAD {
        return Err(ElfLoadError::MalformedProgramHeader);
    }

    let flags = read_u32(elf_bytes, offset + 4)?;
    if flags & !(PF_R | PF_W | PF_X) != 0 {
        return Err(ElfLoadError::MalformedProgramHeader);
    }

    let file_offset = read_u64(elf_bytes, offset + 8)?;
    let vaddr = read_u64(elf_bytes, offset + 16)?;
    let filesz = read_u64(elf_bytes, offset + 32)?;
    let memsz = read_u64(elf_bytes, offset + 40)?;
    let align = read_u64(elf_bytes, offset + 48)?;

    if memsz < filesz {
        return Err(ElfLoadError::SegmentMemSizeLessThanFileSize);
    }

    if memsz == 0 {
        return Err(ElfLoadError::MalformedProgramHeader);
    }

    if align != PAGE_SIZE || vaddr % PAGE_SIZE != 0 || file_offset % PAGE_SIZE != 0 {
        return Err(ElfLoadError::UnalignedSegment);
    }

    if vaddr % align != file_offset % align {
        return Err(ElfLoadError::UnalignedSegment);
    }

    let data_end = checked_usize_range_end(file_offset, filesz as usize)
        .ok_or(ElfLoadError::MalformedProgramHeader)?;
    if data_end > elf_bytes.len() {
        return Err(ElfLoadError::MalformedProgramHeader);
    }

    let segment_end = vaddr
        .checked_add(memsz)
        .ok_or(ElfLoadError::MalformedProgramHeader)?;
    let map_start = align_down(vaddr, PAGE_SIZE);
    let map_end = align_up(segment_end, PAGE_SIZE).ok_or(ElfLoadError::MalformedProgramHeader)?;

    if map_start < USER_ELF_LOAD_START || map_end > USER_ELF_LOAD_END || map_start >= map_end {
        return Err(ElfLoadError::SegmentOutsideUserRange);
    }

    let data_start = file_offset as usize;
    let data = &elf_bytes[data_start..data_end];

    Ok(LoadSegment {
        vaddr,
        filesz,
        memsz,
        flags,
        data,
        map_start,
        map_end,
    })
}

fn check_segment_overlap(
    existing_segments: &[LoadSegment<'static>],
    segment: &LoadSegment<'static>,
) -> Result<(), ElfLoadError> {
    for existing in existing_segments {
        if segment.map_start < existing.map_end && existing.map_start < segment.map_end {
            return Err(ElfLoadError::SegmentOverlap);
        }
    }

    Ok(())
}

impl LoadSegment<'_> {
    fn user_flags(&self) -> UserMapFlags {
        UserMapFlags::new(
            self.flags & PF_R != 0,
            self.flags & PF_W != 0,
            self.flags & PF_X != 0,
        )
    }

    fn contains_entry(&self, entry: u64) -> bool {
        self.flags & PF_X != 0 && entry >= self.vaddr && entry < self.vaddr + self.memsz
    }
}

fn map_address_space_error(error: AddressSpaceError) -> ElfLoadError {
    match error {
        AddressSpaceError::FrameAllocationFailed => ElfLoadError::OutOfFrames,
        AddressSpaceError::KernelUserSlotInUse
        | AddressSpaceError::RangeOverflow
        | AddressSpaceError::MapTo(_) => ElfLoadError::MapFailed,
    }
}

fn map_user_copy_error(error: UserCopyError) -> ElfLoadError {
    match error {
        UserCopyError::RangeOverflow | UserCopyError::NotMapped => ElfLoadError::UserCopyFailed,
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, ElfLoadError> {
    let data = bytes
        .get(offset..offset + 2)
        .ok_or(ElfLoadError::MalformedHeader)?;
    Ok(u16::from_le_bytes([data[0], data[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, ElfLoadError> {
    let data = bytes
        .get(offset..offset + 4)
        .ok_or(ElfLoadError::MalformedHeader)?;
    Ok(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, ElfLoadError> {
    let data = bytes
        .get(offset..offset + 8)
        .ok_or(ElfLoadError::MalformedHeader)?;
    Ok(u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]))
}

fn checked_usize_range_end(start: u64, len: usize) -> Option<usize> {
    let start = usize::try_from(start).ok()?;
    start.checked_add(len)
}

fn align_down(value: u64, align: u64) -> u64 {
    value & !(align - 1)
}

fn align_up(value: u64, align: u64) -> Option<u64> {
    Some(value.checked_add(align - 1)? & !(align - 1))
}
