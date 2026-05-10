use core::arch::{asm, global_asm};

use x86_64::structures::paging::{
    mapper::MapToError, FrameAllocator, Mapper, Page, PageSize, PageTableFlags, PhysFrame,
    Size4KiB, Translate,
};
use x86_64::VirtAddr;

use crate::task::TASK_STACK_SIZE;

const PAGE_SIZE: u64 = Size4KiB::SIZE;
const USER_SLOT_SIZE: u64 = 0x20_000;
const USER_CODE_ALIAS_PAGES: u64 = 2;

pub const USER_CODE_START: u64 = 0x0000_4000_0000;
pub const USER_MARKER_START: u64 = 0x0000_5000_0000;
pub const USER_STACK_START: u64 = 0x0000_6000_0000;
pub const USER_STACK_SIZE: usize = TASK_STACK_SIZE;

pub const USER_MARKER_RAN: usize = 0;
pub const USER_MARKER_AFTER_YIELD: usize = 1;
pub const USER_MARKER_BEFORE_FAULT: usize = 2;
pub const USER_MARKER_AFTER_FAULT: usize = 3;
pub const USER_MARKER_BUSY_COUNT: usize = 4;

global_asm!(
    r#"
    .section .text.user, "ax"

    .global userspace_yield_exit_entry
    .type userspace_yield_exit_entry, @function
userspace_yield_exit_entry:
    mov qword ptr [rdi], 1
    mov rax, 0
    int 0x80
    mov qword ptr [rdi + 8], 1
    mov rax, 1
    int 0x80
1:
    jmp 1b
    .size userspace_yield_exit_entry, . - userspace_yield_exit_entry

    .global userspace_privileged_hlt_entry
    .type userspace_privileged_hlt_entry, @function
userspace_privileged_hlt_entry:
    mov qword ptr [rdi + 16], 1
    hlt
    mov qword ptr [rdi + 24], 1
    mov rax, 1
    int 0x80
1:
    jmp 1b
    .size userspace_privileged_hlt_entry, . - userspace_privileged_hlt_entry

    .global userspace_busy_counter_entry
    .type userspace_busy_counter_entry, @function
userspace_busy_counter_entry:
    mov rax, qword ptr [rdi + 32]
1:
    inc rax
    mov qword ptr [rdi + 32], rax
    jmp 1b
    .size userspace_busy_counter_entry, . - userspace_busy_counter_entry

    .section .text
"#
);

extern "C" {
    static userspace_yield_exit_entry: u8;
    static userspace_privileged_hlt_entry: u8;
    static userspace_busy_counter_entry: u8;
}

#[derive(Debug, Clone, Copy)]
pub enum UserProgram {
    YieldThenExit,
    PrivilegedHlt,
    BusyCounter,
}

impl UserProgram {
    fn kernel_entry(self) -> VirtAddr {
        let entry = match self {
            Self::YieldThenExit => core::ptr::addr_of!(userspace_yield_exit_entry),
            Self::PrivilegedHlt => core::ptr::addr_of!(userspace_privileged_hlt_entry),
            Self::BusyCounter => core::ptr::addr_of!(userspace_busy_counter_entry),
        };

        VirtAddr::from_ptr(entry)
    }

    fn slot(self) -> usize {
        match self {
            Self::YieldThenExit => 0,
            Self::PrivilegedHlt => 1,
            Self::BusyCounter => 2,
        }
    }
}

pub fn map_user_program<M, F>(
    mapper: &mut M,
    frame_allocator: &mut F,
    program: UserProgram,
) -> Result<VirtAddr, MapToError<Size4KiB>>
where
    M: Mapper<Size4KiB> + Translate,
    F: FrameAllocator<Size4KiB>,
{
    let kernel_entry = program.kernel_entry();
    let source_page = Page::<Size4KiB>::containing_address(kernel_entry);
    let offset = kernel_entry.as_u64() - source_page.start_address().as_u64();
    let alias_start = VirtAddr::new(USER_CODE_START + program.slot() as u64 * USER_SLOT_SIZE);
    let flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;

    for page_index in 0..USER_CODE_ALIAS_PAGES {
        let source_address = source_page.start_address() + page_index * PAGE_SIZE;
        let physical_address = mapper
            .translate_addr(source_address)
            .ok_or(MapToError::FrameAllocationFailed)?;
        let source_frame = PhysFrame::containing_address(physical_address);
        let alias_page = Page::containing_address(alias_start + page_index * PAGE_SIZE);

        unsafe {
            mapper
                .map_to(alias_page, source_frame, flags, frame_allocator)?
                .flush();
        }
    }

    Ok(alias_start + offset)
}

pub fn map_user_stack<M, F>(
    mapper: &mut M,
    frame_allocator: &mut F,
    stack_slot: usize,
) -> Result<VirtAddr, MapToError<Size4KiB>>
where
    M: Mapper<Size4KiB>,
    F: FrameAllocator<Size4KiB>,
{
    let stack_bottom = VirtAddr::new(USER_STACK_START + stack_slot as u64 * USER_SLOT_SIZE);
    let flags =
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;

    for page_offset in (0..USER_STACK_SIZE).step_by(PAGE_SIZE as usize) {
        let page = Page::containing_address(stack_bottom + page_offset as u64);
        let frame = frame_allocator
            .allocate_frame()
            .ok_or(MapToError::FrameAllocationFailed)?;

        unsafe {
            mapper.map_to(page, frame, flags, frame_allocator)?.flush();
        }
    }

    Ok(stack_bottom + USER_STACK_SIZE as u64)
}

pub fn map_user_marker_page<M, F>(
    mapper: &mut M,
    frame_allocator: &mut F,
) -> Result<VirtAddr, MapToError<Size4KiB>>
where
    M: Mapper<Size4KiB>,
    F: FrameAllocator<Size4KiB>,
{
    let page = Page::containing_address(VirtAddr::new(USER_MARKER_START));
    let frame = frame_allocator
        .allocate_frame()
        .ok_or(MapToError::FrameAllocationFailed)?;
    let flags =
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;

    unsafe {
        mapper.map_to(page, frame, flags, frame_allocator)?.flush();
    }

    for index in 0..8 {
        unsafe {
            marker_ptr(index).write_volatile(0);
        }
    }

    Ok(VirtAddr::new(USER_MARKER_START))
}

pub fn marker_value(index: usize) -> u64 {
    unsafe { marker_ptr(index).read_volatile() }
}

pub unsafe fn syscall_yield() {
    unsafe {
        asm!(
            "int {vector}",
            vector = const crate::syscall::SYSCALL_VECTOR,
            inlateout("rax") crate::syscall::SyscallNumber::Yield as u64 => _,
        );
    }
}

pub unsafe fn syscall_exit() -> ! {
    unsafe {
        asm!(
            "int {vector}",
            vector = const crate::syscall::SYSCALL_VECTOR,
            in("rax") crate::syscall::SyscallNumber::Exit as u64,
        );
    }

    loop {
        core::hint::spin_loop();
    }
}

fn marker_ptr(index: usize) -> *mut u64 {
    (USER_MARKER_START as *mut u64).wrapping_add(index)
}
