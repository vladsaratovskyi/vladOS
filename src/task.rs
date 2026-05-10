use alloc::{boxed::Box, vec};

use crate::arch::x86_64::context::Context;

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
}

pub struct Task {
    id: TaskId,
    state: TaskState,
    context: Context,
    entry: TaskEntry,
    _stack: Box<[u8]>,
}

impl Task {
    pub fn new(id: TaskId, entry: TaskEntry, rflags: u64) -> Self {
        let mut stack = vec![0u8; TASK_STACK_SIZE].into_boxed_slice();
        let context =
            unsafe { Context::new_task(&mut stack, task_trampoline as *const () as usize, rflags) };

        Self {
            id,
            state: TaskState::Ready,
            context,
            entry,
            _stack: stack,
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

    pub(crate) fn entry(&self) -> TaskEntry {
        self.entry
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
