use crate::arch::x86_64::context::TrapFrame;
use crate::user_memory::UserMemoryError;
use x86_64::VirtAddr;

pub const SYSCALL_VECTOR: u8 = 0x80;
pub const EBADF: isize = 9;
pub const EFAULT: isize = 14;
pub const EINVAL: isize = 22;
pub const ENOSYS: isize = 38;
pub const ECHILD: isize = 10;
pub const WNOHANG: usize = 1;

const WRITE_CHUNK_SIZE: usize = 256;

pub type SysResult = Result<usize, SysError>;

#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyscallNumber {
    Yield = 0,
    Exit = 1,
    Write = 2,
    GetPid = 3,
    WaitPid = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SysError {
    BadFd,
    Fault,
    Invalid,
    Child,
    NoSys,
}

impl SysError {
    fn errno(self) -> isize {
        match self {
            Self::BadFd => EBADF,
            Self::Fault => EFAULT,
            Self::Invalid => EINVAL,
            Self::Child => ECHILD,
            Self::NoSys => ENOSYS,
        }
    }

    pub(crate) fn raw_return(self) -> u64 {
        (-(self.errno() as i64)) as u64
    }
}

impl SyscallNumber {
    fn from_raw(raw: u64) -> Option<Self> {
        match raw {
            value if value == Self::Yield as u64 => Some(Self::Yield),
            value if value == Self::Exit as u64 => Some(Self::Exit),
            value if value == Self::Write as u64 => Some(Self::Write),
            value if value == Self::GetPid as u64 => Some(Self::GetPid),
            value if value == Self::WaitPid as u64 => Some(Self::WaitPid),
            _ => None,
        }
    }
}

pub fn dispatch(frame_rsp: u64) -> u64 {
    let frame = unsafe { &mut *(frame_rsp as *mut TrapFrame) };

    match SyscallNumber::from_raw(frame.rax) {
        Some(SyscallNumber::Yield) => {
            frame.rax = 0;
            crate::scheduler::on_syscall_yield(frame_rsp)
        }
        Some(SyscallNumber::Exit) => {
            crate::scheduler::exit_current_from_interrupt(frame_rsp, frame.rdi)
        }
        Some(SyscallNumber::Write) => {
            frame.rax = sys_write(frame.rdi, VirtAddr::new(frame.rsi), frame.rdx as usize)
                .map(|count| count as u64)
                .unwrap_or_else(SysError::raw_return);

            frame_rsp
        }
        Some(SyscallNumber::GetPid) => {
            frame.rax = crate::scheduler::current_process_id()
                .map(|pid| pid.0 as u64)
                .unwrap_or_else(|| SysError::Child.raw_return());
            frame_rsp
        }
        Some(SyscallNumber::WaitPid) => crate::scheduler::on_syscall_waitpid(
            frame_rsp,
            crate::process::ProcessId(frame.rdi as usize),
            crate::process::wait_status_address(frame.rsi),
            frame.rdx as usize,
        ),
        None => {
            frame.rax = SysError::NoSys.raw_return();
            frame_rsp
        }
    }
}

fn sys_write(fd: u64, user_buf: VirtAddr, len: usize) -> SysResult {
    match fd {
        1 | 2 => {}
        _ => return Err(SysError::BadFd),
    }

    if len == 0 {
        return Ok(0);
    }

    crate::scheduler::with_current_user_address_space(|address_space| {
        crate::user_memory::validate_user_read_range(address_space, user_buf, len)?;

        let mut written = 0;
        let mut buffer = [0_u8; WRITE_CHUNK_SIZE];

        while written < len {
            let count = core::cmp::min(buffer.len(), len - written);
            let src = VirtAddr::new(
                user_buf
                    .as_u64()
                    .checked_add(written as u64)
                    .ok_or(UserMemoryError::AddressOverflow)?,
            );
            crate::user_memory::copy_from_user(address_space, &mut buffer[..count], src)?;
            crate::serial::write_bytes(&buffer[..count]);
            written += count;
        }

        Ok(written)
    })
    .map_err(map_user_memory_error)?
    .map_err(map_user_memory_error)
}

fn map_user_memory_error(error: UserMemoryError) -> SysError {
    match error {
        UserMemoryError::AddressOverflow
        | UserMemoryError::OutsideUserRange
        | UserMemoryError::Unmapped
        | UserMemoryError::NotWritable => SysError::Fault,
    }
}
