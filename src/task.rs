use alloc::{boxed::Box, vec};

use crate::arch::x86_64::context::{Context, TrapFrame};
use crate::gdt;
use crate::process::ProcessId;
use x86_64::VirtAddr;

pub const TASK_STACK_SIZE: usize = 8 * 1024;
pub const MAX_TASKS: usize = 8;

pub type TaskEntry = fn();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Ready,
    Running,
    Blocked(WaitReason),
    Finished,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitReason {
    ChildExit {
        child: ProcessId,
        status_ptr: Option<VirtAddr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserFaultKind {
    GeneralProtection,
    PageFault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserFaultInfo {
    pub kind: UserFaultKind,
    pub address: Option<VirtAddr>,
    pub error_code: u64,
}

pub enum TaskKind {
    Kernel {
        entry: TaskEntry,
    },
    User {
        entry_point: VirtAddr,
        user_stack_top: VirtAddr,
    },
}

pub enum TaskAddressSpace {
    Kernel,
    User(ProcessId),
}

pub struct Task {
    id: TaskId,
    state: TaskState,
    context: Context,
    kind: TaskKind,
    address_space: TaskAddressSpace,
    kernel_stack_top: VirtAddr,
    exit_code: Option<u64>,
    fault_info: Option<UserFaultInfo>,
    _kernel_stack: Box<[u8]>,
}

impl Task {
    pub fn new(id: TaskId, entry: TaskEntry, rflags: u64) -> Self {
        Self::new_kernel(id, entry, rflags)
    }

    pub fn new_kernel(id: TaskId, entry: TaskEntry, rflags: u64) -> Self {
        let mut stack = vec![0u8; TASK_STACK_SIZE].into_boxed_slice();
        let context =
            unsafe { Context::new_task(&mut stack, task_trampoline as *const () as usize, rflags) };
        let kernel_stack_top = stack_top(&stack);

        Self {
            id,
            state: TaskState::Ready,
            context,
            kind: TaskKind::Kernel { entry },
            address_space: TaskAddressSpace::Kernel,
            kernel_stack_top,
            exit_code: None,
            fault_info: None,
            _kernel_stack: stack,
        }
    }

    pub fn new_user(
        id: TaskId,
        process_id: ProcessId,
        entry_point: VirtAddr,
        user_stack_top: VirtAddr,
        arg0: u64,
        rflags: u64,
    ) -> Self {
        let mut stack = vec![0u8; TASK_STACK_SIZE].into_boxed_slice();
        let context = unsafe {
            Context::new_user_task(
                &mut stack,
                entry_point.as_u64(),
                user_stack_top.as_u64(),
                gdt::user_code_selector().0,
                gdt::user_data_selector().0,
                rflags,
                arg0,
            )
        };
        let kernel_stack_top = stack_top(&stack);

        Self {
            id,
            state: TaskState::Ready,
            context,
            kind: TaskKind::User {
                entry_point,
                user_stack_top,
            },
            address_space: TaskAddressSpace::User(process_id),
            kernel_stack_top,
            exit_code: None,
            fault_info: None,
            _kernel_stack: stack,
        }
    }

    pub fn id(&self) -> TaskId {
        self.id
    }

    pub fn state(&self) -> TaskState {
        self.state
    }

    pub(crate) fn set_state(&mut self, state: TaskState) {
        self.state = state;
    }

    pub(crate) fn kernel_entry(&self) -> Option<TaskEntry> {
        match self.kind {
            TaskKind::Kernel { entry } => Some(entry),
            TaskKind::User { .. } => None,
        }
    }

    pub(crate) fn kernel_stack_top(&self) -> VirtAddr {
        self.kernel_stack_top
    }

    pub(crate) fn process_id(&self) -> Option<ProcessId> {
        match self.address_space {
            TaskAddressSpace::Kernel => None,
            TaskAddressSpace::User(process_id) => Some(process_id),
        }
    }

    pub(crate) fn set_exit_code(&mut self, exit_code: u64) {
        self.exit_code = Some(exit_code);
    }

    pub(crate) fn exit_code(&self) -> Option<u64> {
        self.exit_code
    }

    pub(crate) fn set_fault_info(&mut self, fault_info: UserFaultInfo) {
        self.fault_info = Some(fault_info);
    }

    pub(crate) fn fault_info(&self) -> Option<UserFaultInfo> {
        self.fault_info
    }

    pub(crate) fn context(&self) -> *const Context {
        core::ptr::addr_of!(self.context)
    }

    pub(crate) fn saved_rsp(&self) -> u64 {
        self.context.rsp()
    }

    pub(crate) fn set_saved_rsp(&mut self, rsp: u64) {
        self.context.set_rsp(rsp);
    }

    pub(crate) fn set_saved_rax(&mut self, value: u64) {
        let frame = self.saved_trap_frame_mut();
        frame.rax = value;
    }

    pub(crate) fn set_saved_rdi(&mut self, value: u64) {
        let frame = self.saved_trap_frame_mut();
        frame.rdi = value;
    }

    fn saved_trap_frame_mut(&mut self) -> &mut TrapFrame {
        // Runnable and blocked tasks always resume through the full trap-frame
        // restore path. Faulted tasks can have an error-code frame, but this
        // helper is only used before first run or while a syscall task is
        // blocked.
        unsafe { &mut *(self.context.rsp() as *mut TrapFrame) }
    }
}

extern "C" fn task_trampoline() -> ! {
    crate::scheduler::run_current_task()
}

fn stack_top(stack: &[u8]) -> VirtAddr {
    const STACK_ALIGN: u64 = 16;

    let top = stack.as_ptr() as u64 + stack.len() as u64;
    VirtAddr::new(top & !(STACK_ALIGN - 1))
}
