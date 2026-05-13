use core::arch::{asm, global_asm};

use x86_64::structures::paging::{PageSize, PageTableFlags, Size4KiB};
use x86_64::VirtAddr;

use crate::address_space::{AddressSpace, AddressSpaceError, UserMapFlags, USER_P4_INDEX};
use crate::process::{UserHeap, UserHeapError};
use crate::task::TASK_STACK_SIZE;

const PAGE_SIZE: u64 = Size4KiB::SIZE;

pub const USER_BASE: u64 = (USER_P4_INDEX as u64) << 39;
pub const USER_CODE_BASE: u64 = USER_BASE + 0x0040_0000;
pub const USER_DATA_BASE: u64 = USER_BASE + 0x0060_0000;
pub const USER_TEST_PAGE_BASE: u64 = USER_BASE + 0x0070_0000;
pub const USER_STACK_TOP: u64 = USER_BASE + 0x0080_0000;
pub const USER_STACK_PAGES: usize = TASK_STACK_SIZE / 4096;
pub const USER_ELF_LOAD_START: u64 = USER_CODE_BASE;
pub const USER_ELF_LOAD_END: u64 = USER_TEST_PAGE_BASE;
pub const USER_HEAP_LIMIT: u64 = USER_TEST_PAGE_BASE;

pub const USER_MARKER_RAN: usize = 0;
pub const USER_MARKER_AFTER_YIELD: usize = 1;
pub const USER_MARKER_BEFORE_FAULT: usize = 2;
pub const USER_MARKER_AFTER_FAULT: usize = 3;
pub const USER_MARKER_BUSY_COUNT: usize = 4;

pub struct UserTaskInit {
    pub address_space: AddressSpace,
    pub entry_point: VirtAddr,
    pub user_stack_top: VirtAddr,
    pub heap: UserHeap,
    pub arg0: u64,
}

#[derive(Debug, Clone, Copy)]
pub enum UserProgram {
    YieldThenExit,
    PrivilegedHlt,
    BusyCounter,
    WriteReadAa,
    WriteReadBb,
    ReadArgExit,
}

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
    xor rdi, rdi
    mov rax, 1
    int 0x80
1:
    jmp 1b
userspace_yield_exit_entry_end:
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
userspace_privileged_hlt_entry_end:
    .size userspace_privileged_hlt_entry, . - userspace_privileged_hlt_entry

    .global userspace_busy_counter_entry
    .type userspace_busy_counter_entry, @function
userspace_busy_counter_entry:
    mov rax, qword ptr [rdi + 32]
1:
    inc rax
    mov qword ptr [rdi + 32], rax
    jmp 1b
userspace_busy_counter_entry_end:
    .size userspace_busy_counter_entry, . - userspace_busy_counter_entry

    .global userspace_write_read_aa_entry
    .type userspace_write_read_aa_entry, @function
userspace_write_read_aa_entry:
    mov qword ptr [rdi], 0xaa
    mov rax, 0
    int 0x80
    mov rdi, qword ptr [rdi]
    mov rax, 1
    int 0x80
1:
    jmp 1b
userspace_write_read_aa_entry_end:
    .size userspace_write_read_aa_entry, . - userspace_write_read_aa_entry

    .global userspace_write_read_bb_entry
    .type userspace_write_read_bb_entry, @function
userspace_write_read_bb_entry:
    mov qword ptr [rdi], 0xbb
    mov rax, 0
    int 0x80
    mov rdi, qword ptr [rdi]
    mov rax, 1
    int 0x80
1:
    jmp 1b
userspace_write_read_bb_entry_end:
    .size userspace_write_read_bb_entry, . - userspace_write_read_bb_entry

    .global userspace_read_arg_exit_entry
    .type userspace_read_arg_exit_entry, @function
userspace_read_arg_exit_entry:
    mov rdi, qword ptr [rdi]
    mov rax, 1
    int 0x80
1:
    jmp 1b
userspace_read_arg_exit_entry_end:
    .size userspace_read_arg_exit_entry, . - userspace_read_arg_exit_entry

    .section .text
"#
);

extern "C" {
    static userspace_yield_exit_entry: u8;
    static userspace_yield_exit_entry_end: u8;
    static userspace_privileged_hlt_entry: u8;
    static userspace_privileged_hlt_entry_end: u8;
    static userspace_busy_counter_entry: u8;
    static userspace_busy_counter_entry_end: u8;
    static userspace_write_read_aa_entry: u8;
    static userspace_write_read_aa_entry_end: u8;
    static userspace_write_read_bb_entry: u8;
    static userspace_write_read_bb_entry_end: u8;
    static userspace_read_arg_exit_entry: u8;
    static userspace_read_arg_exit_entry_end: u8;
}

pub fn create_user_task(
    program: UserProgram,
    arg0: u64,
) -> Result<UserTaskInit, AddressSpaceError> {
    create_user_task_with_test_page(program, arg0, None)
}

pub fn create_user_task_with_test_page(
    program: UserProgram,
    arg0: u64,
    test_page_value: Option<u64>,
) -> Result<UserTaskInit, AddressSpaceError> {
    let mut address_space = AddressSpace::new_user()?;
    let entry_point = map_program(&mut address_space, program)?;

    let data_frame =
        address_space.map_zeroed_user_page(VirtAddr::new(USER_DATA_BASE), user_data_flags())?;
    address_space.write_frame_u64(data_frame, 0);

    if let Some(value) = test_page_value {
        let test_frame = address_space
            .map_zeroed_user_page(VirtAddr::new(USER_TEST_PAGE_BASE), user_data_flags())?;
        address_space.write_frame_u64(test_frame, value);
    }

    map_user_stack(&mut address_space)?;
    let heap = default_builtin_heap().map_err(|_| AddressSpaceError::RangeOverflow)?;

    Ok(UserTaskInit {
        address_space,
        entry_point,
        user_stack_top: VirtAddr::new(USER_STACK_TOP),
        heap,
        arg0,
    })
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
            in("rdi") 0_u64,
        );
    }

    loop {
        core::hint::spin_loop();
    }
}

pub fn map_user_stack(address_space: &mut AddressSpace) -> Result<(), AddressSpaceError> {
    let stack_bottom = USER_STACK_TOP - USER_STACK_PAGES as u64 * PAGE_SIZE;
    address_space.map_user_region(
        VirtAddr::new(stack_bottom),
        USER_STACK_PAGES * PAGE_SIZE as usize,
        UserMapFlags::read_write(),
    )
}

fn map_program(
    address_space: &mut AddressSpace,
    program: UserProgram,
) -> Result<VirtAddr, AddressSpaceError> {
    let code_frame = address_space.map_zeroed_user_page(
        VirtAddr::new(USER_CODE_BASE),
        PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE,
    )?;
    address_space.write_frame_bytes(code_frame, program.bytes())?;

    Ok(VirtAddr::new(USER_CODE_BASE))
}

fn user_data_flags() -> PageTableFlags {
    PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE
}

pub fn heap_for_loaded_image(max_segment_end: u64) -> Result<UserHeap, UserHeapError> {
    let heap_start = align_up(max_segment_end, PAGE_SIZE).ok_or(UserHeapError::InvalidRange)?;
    if heap_start < USER_ELF_LOAD_START || heap_start > USER_HEAP_LIMIT {
        return Err(UserHeapError::InvalidRange);
    }

    UserHeap::new(VirtAddr::new(heap_start), VirtAddr::new(USER_HEAP_LIMIT))
}

fn default_builtin_heap() -> Result<UserHeap, UserHeapError> {
    UserHeap::new(
        VirtAddr::new(USER_DATA_BASE + PAGE_SIZE),
        VirtAddr::new(USER_HEAP_LIMIT),
    )
}

fn align_up(value: u64, align: u64) -> Option<u64> {
    Some(value.checked_add(align - 1)? & !(align - 1))
}

impl UserProgram {
    fn bytes(self) -> &'static [u8] {
        let (start, end) = match self {
            Self::YieldThenExit => (
                core::ptr::addr_of!(userspace_yield_exit_entry),
                core::ptr::addr_of!(userspace_yield_exit_entry_end),
            ),
            Self::PrivilegedHlt => (
                core::ptr::addr_of!(userspace_privileged_hlt_entry),
                core::ptr::addr_of!(userspace_privileged_hlt_entry_end),
            ),
            Self::BusyCounter => (
                core::ptr::addr_of!(userspace_busy_counter_entry),
                core::ptr::addr_of!(userspace_busy_counter_entry_end),
            ),
            Self::WriteReadAa => (
                core::ptr::addr_of!(userspace_write_read_aa_entry),
                core::ptr::addr_of!(userspace_write_read_aa_entry_end),
            ),
            Self::WriteReadBb => (
                core::ptr::addr_of!(userspace_write_read_bb_entry),
                core::ptr::addr_of!(userspace_write_read_bb_entry_end),
            ),
            Self::ReadArgExit => (
                core::ptr::addr_of!(userspace_read_arg_exit_entry),
                core::ptr::addr_of!(userspace_read_arg_exit_entry_end),
            ),
        };

        let len = end as usize - start as usize;
        assert!(len <= 4096);

        unsafe { core::slice::from_raw_parts(start, len) }
    }
}
