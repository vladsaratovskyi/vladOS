use crate::arch::x86_64::context::TrapFrame;
use x86_64::VirtAddr;

pub const SYSCALL_VECTOR: u8 = 0x80;
pub const ENOENT: isize = 2;
pub const EBADF: isize = 9;
pub const EFAULT: isize = 14;
pub const EINVAL: isize = 22;
pub const ENFILE: isize = 23;
pub const EMFILE: isize = 24;
pub const ENAMETOOLONG: isize = 36;
pub const ENOSYS: isize = 38;
pub const ECHILD: isize = 10;
pub const WNOHANG: usize = 1;
pub const O_RDONLY: usize = 0;

pub type SysResult = Result<usize, SysError>;

#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyscallNumber {
    Yield = 0,
    Exit = 1,
    Write = 2,
    GetPid = 3,
    WaitPid = 4,
    Open = 5,
    Read = 6,
    Close = 7,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SysError {
    NoEntry,
    BadFd,
    Fault,
    Invalid,
    SystemFileLimit,
    ProcessFileLimit,
    NameTooLong,
    Child,
    NoSys,
}

impl SysError {
    fn errno(self) -> isize {
        match self {
            Self::NoEntry => ENOENT,
            Self::BadFd => EBADF,
            Self::Fault => EFAULT,
            Self::Invalid => EINVAL,
            Self::SystemFileLimit => ENFILE,
            Self::ProcessFileLimit => EMFILE,
            Self::NameTooLong => ENAMETOOLONG,
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
            value if value == Self::Open as u64 => Some(Self::Open),
            value if value == Self::Read as u64 => Some(Self::Read),
            value if value == Self::Close as u64 => Some(Self::Close),
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
            frame.rax = crate::scheduler::sys_write(
                frame.rdi as usize,
                VirtAddr::new(frame.rsi),
                frame.rdx as usize,
            )
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
        Some(SyscallNumber::Open) => {
            frame.rax = crate::scheduler::sys_open(
                VirtAddr::new(frame.rdi),
                frame.rsi as usize,
                frame.rdx as usize,
            )
            .map(|fd| fd as u64)
            .unwrap_or_else(SysError::raw_return);
            frame_rsp
        }
        Some(SyscallNumber::Read) => {
            frame.rax = crate::scheduler::sys_read(
                frame.rdi as usize,
                VirtAddr::new(frame.rsi),
                frame.rdx as usize,
            )
            .map(|count| count as u64)
            .unwrap_or_else(SysError::raw_return);
            frame_rsp
        }
        Some(SyscallNumber::Close) => {
            frame.rax = crate::scheduler::sys_close(frame.rdi as usize)
                .map(|value| value as u64)
                .unwrap_or_else(SysError::raw_return);
            frame_rsp
        }
        None => {
            frame.rax = SysError::NoSys.raw_return();
            frame_rsp
        }
    }
}
