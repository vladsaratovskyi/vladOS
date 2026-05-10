use alloc::{boxed::Box, vec};

use crate::arch::x86_64::context::Context;
use crate::gdt;
use x86_64::VirtAddr;

pub const TASK_STACK_SIZE: usize = 8 * 1024;
pub const MAX_TASKS: usize = 4;

pub type TaskEntry = fn();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Ready,
    Running,
    Finished,
    Failed,
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

pub struct Task {
    id: TaskId,
    state: TaskState,
    context: Context,
    kind: TaskKind,
    kernel_stack_top: VirtAddr,
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
            kernel_stack_top,
            _kernel_stack: stack,
        }
    }

    pub fn new_user(
        id: TaskId,
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
            kernel_stack_top,
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

    pub(crate) fn context(&self) -> *const Context {
        core::ptr::addr_of!(self.context)
    }

    pub(crate) fn saved_rsp(&self) -> u64 {
        self.context.rsp()
    }

    pub(crate) fn set_saved_rsp(&mut self, rsp: u64) {
        self.context.set_rsp(rsp);
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
