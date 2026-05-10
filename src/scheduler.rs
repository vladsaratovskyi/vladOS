use alloc::vec::Vec;

use crate::arch::x86_64::context::{self, Context};
use crate::task::{Task, TaskEntry, TaskId, TaskState, MAX_TASKS};
use crate::{hlt_loop, println};

static mut SCHEDULER: Scheduler = Scheduler::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnError {
    Full,
}

struct Scheduler {
    tasks: Vec<Task>,
    current: Option<usize>,
    main_context: Context,
}

impl Scheduler {
    const fn new() -> Self {
        Self {
            tasks: Vec::new(),
            current: None,
            main_context: Context::empty(),
        }
    }

    fn spawn(&mut self, entry: TaskEntry) -> Result<TaskId, SpawnError> {
        if self.tasks.len() >= MAX_TASKS {
            return Err(SpawnError::Full);
        }

        let id = TaskId(self.tasks.len());
        self.tasks.push(Task::new(id, entry));

        Ok(id)
    }

    fn next_ready_after(&self, start: usize) -> Option<usize> {
        if self.tasks.is_empty() {
            return None;
        }

        for offset in 1..=self.tasks.len() {
            let index = (start + offset) % self.tasks.len();

            if self.tasks[index].state() == TaskState::Ready {
                return Some(index);
            }
        }

        None
    }

    fn first_ready(&self) -> Option<usize> {
        self.tasks
            .iter()
            .position(|task| task.state() == TaskState::Ready)
    }

    fn finished_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|task| task.state() == TaskState::Finished)
            .count()
    }

    fn all_tasks_finished(&self) -> bool {
        !self.tasks.is_empty() && self.finished_count() == self.tasks.len()
    }
}

pub fn spawn(entry: TaskEntry) -> Result<TaskId, SpawnError> {
    unsafe { scheduler_mut().spawn(entry) }
}

pub fn run() {
    unsafe {
        let scheduler = scheduler_mut();
        let Some(next) = scheduler.first_ready() else {
            return;
        };

        scheduler.current = Some(next);
        scheduler.tasks[next].set_state(TaskState::Running);

        let old_context = core::ptr::addr_of_mut!(scheduler.main_context);
        let new_context = scheduler.tasks[next].context();

        context::switch(old_context, new_context);
    }
}

pub fn yield_now() {
    unsafe {
        let scheduler = scheduler_mut();
        let Some(current) = scheduler.current else {
            return;
        };

        let Some(next) = scheduler.next_ready_after(current) else {
            return;
        };

        scheduler.tasks[current].set_state(TaskState::Ready);
        scheduler.tasks[next].set_state(TaskState::Running);
        scheduler.current = Some(next);

        switch_between_tasks(scheduler, current, next);
    }
}

pub fn finished_task_count() -> usize {
    unsafe { scheduler_mut().finished_count() }
}

pub fn all_tasks_finished() -> bool {
    unsafe { scheduler_mut().all_tasks_finished() }
}

pub(crate) fn run_current_task() -> ! {
    let entry = unsafe {
        let scheduler = scheduler_mut();
        let current = scheduler.current.expect("no current task");

        scheduler.tasks[current].entry()
    };

    entry();
    finish_current_task();
}

fn finish_current_task() -> ! {
    unsafe {
        let scheduler = scheduler_mut();
        let current = scheduler.current.expect("no current task");

        scheduler.tasks[current].set_state(TaskState::Finished);

        if let Some(next) = scheduler.next_ready_after(current) {
            scheduler.tasks[next].set_state(TaskState::Running);
            scheduler.current = Some(next);

            switch_between_tasks(scheduler, current, next);
        } else {
            scheduler.current = None;

            let old_context = scheduler.tasks[current].context_mut();
            let new_context = core::ptr::addr_of!(scheduler.main_context);

            context::switch(old_context, new_context);
        }
    }

    println!("task returned after finish");
    hlt_loop();
}

unsafe fn switch_between_tasks(scheduler: &mut Scheduler, current: usize, next: usize) {
    let old_context = scheduler.tasks[current].context_mut();
    let new_context = scheduler.tasks[next].context();

    unsafe {
        context::switch(old_context, new_context);
    }
}

unsafe fn scheduler_mut() -> &'static mut Scheduler {
    unsafe { &mut *core::ptr::addr_of_mut!(SCHEDULER) }
}
