use crate::arch::x86_64::context::TrapFrame;

pub const SYSCALL_VECTOR: u8 = 0x80;

#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyscallNumber {
    Yield = 0,
    Exit = 1,
}

impl SyscallNumber {
    fn from_raw(raw: u64) -> Option<Self> {
        match raw {
            value if value == Self::Yield as u64 => Some(Self::Yield),
            value if value == Self::Exit as u64 => Some(Self::Exit),
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
        None => {
            frame.rax = u64::MAX;
            frame_rsp
        }
    }
}
