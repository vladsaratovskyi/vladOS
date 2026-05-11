use alloc::vec::Vec;
use core::arch::asm;

use x86_64::instructions::interrupts as cpu_interrupts;
use x86_64::registers::control::{Cr3, Cr3Flags};
use x86_64::registers::rflags;
use x86_64::VirtAddr;

use crate::arch::x86_64::context::{self, Context};
use crate::elf::ElfLoadError;
use crate::gdt;
use crate::task::{Task, TaskEntry, TaskId, TaskState, UserFaultInfo, MAX_TASKS};
use crate::user::UserTaskInit;
use crate::{hlt_loop, println};

static mut SCHEDULER: Scheduler = Scheduler::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnError {
    Full,
    ElfLoad(ElfLoadError),
}

struct Scheduler {
    tasks: Vec<Task>,
    current: Option<usize>,
    main_context: Context,
    preemption_enabled: bool,
    current_level_4_frame: Option<u64>,
}

impl Scheduler {
    const fn new() -> Self {
        Self {
            tasks: Vec::new(),
            current: None,
            main_context: Context::empty(),
            preemption_enabled: false,
            current_level_4_frame: None,
        }
    }

    fn spawn(&mut self, entry: TaskEntry, initial_rflags: u64) -> Result<TaskId, SpawnError> {
        if self.tasks.len() >= MAX_TASKS {
            return Err(SpawnError::Full);
        }

        let id = TaskId(self.tasks.len());
        self.tasks.push(Task::new(id, entry, initial_rflags));

        Ok(id)
    }

    fn spawn_user(
        &mut self,
        init: UserTaskInit,
        initial_rflags: u64,
    ) -> Result<TaskId, SpawnError> {
        if self.tasks.len() >= MAX_TASKS {
            return Err(SpawnError::Full);
        }

        let id = TaskId(self.tasks.len());
        self.tasks.push(Task::new_user(
            id,
            init.address_space,
            init.entry_point,
            init.user_stack_top,
            init.arg0,
            initial_rflags,
        ));

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

    fn failed_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|task| task.state() == TaskState::Failed)
            .count()
    }

    fn terminal_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|task| matches!(task.state(), TaskState::Finished | TaskState::Failed))
            .count()
    }

    fn all_tasks_finished(&self) -> bool {
        !self.tasks.is_empty() && self.terminal_count() == self.tasks.len()
    }

    fn save_current_frame(&mut self, frame_rsp: u64) -> Option<usize> {
        let current = self.current?;

        if self.tasks[current].state() == TaskState::Running {
            self.tasks[current].set_saved_rsp(frame_rsp);
            Some(current)
        } else {
            None
        }
    }

    fn switch_from_interrupt(&mut self, frame_rsp: u64) -> u64 {
        let Some(current) = self.save_current_frame(frame_rsp) else {
            return frame_rsp;
        };

        let Some(next) = self.next_ready_after(current) else {
            return frame_rsp;
        };

        self.tasks[current].set_state(TaskState::Ready);
        self.tasks[next].set_state(TaskState::Running);
        self.current = Some(next);
        self.prepare_to_run(next);

        self.tasks[next].saved_rsp()
    }

    fn finish_current_from_interrupt(
        &mut self,
        frame_rsp: u64,
        final_state: TaskState,
        exit_code: Option<u64>,
        fault_info: Option<UserFaultInfo>,
    ) -> u64 {
        let current = self.current.expect("no current task");

        self.tasks[current].set_saved_rsp(frame_rsp);
        self.tasks[current].set_state(final_state);
        if let Some(exit_code) = exit_code {
            self.tasks[current].set_exit_code(exit_code);
        }
        if let Some(fault_info) = fault_info {
            self.tasks[current].set_fault_info(fault_info);
        }

        if let Some(next) = self.next_ready_after(current) {
            self.tasks[next].set_state(TaskState::Running);
            self.current = Some(next);
            self.prepare_to_run(next);

            self.tasks[next].saved_rsp()
        } else {
            self.current = None;
            self.switch_to_kernel_address_space();

            let main_context = core::ptr::addr_of!(self.main_context);
            unsafe { context::restore_main(main_context) }
        }
    }

    fn prepare_to_run(&mut self, task_index: usize) {
        self.load_kernel_stack_for(task_index);
        self.switch_to_task_address_space(task_index);
    }

    fn load_kernel_stack_for(&self, task_index: usize) {
        gdt::set_kernel_stack(self.tasks[task_index].kernel_stack_top());
    }

    fn switch_to_task_address_space(&mut self, task_index: usize) {
        let next_frame = self.tasks[task_index].level_4_frame();
        let next_frame_addr = next_frame.start_address().as_u64();

        if self.current_level_4_frame == Some(next_frame_addr) {
            return;
        }

        unsafe {
            Cr3::write(next_frame, Cr3Flags::empty());
        }
        self.current_level_4_frame = Some(next_frame_addr);
    }

    fn switch_to_kernel_address_space(&mut self) {
        let kernel_frame = crate::memory::kernel_level_4_frame();
        let kernel_frame_addr = kernel_frame.start_address().as_u64();

        if self.current_level_4_frame == Some(kernel_frame_addr) {
            return;
        }

        unsafe {
            Cr3::write(kernel_frame, Cr3Flags::empty());
        }
        self.current_level_4_frame = Some(kernel_frame_addr);
    }
}

pub fn spawn(entry: TaskEntry) -> Result<TaskId, SpawnError> {
    let initial_rflags = if rflags::read().contains(rflags::RFlags::INTERRUPT_FLAG) {
        0x202
    } else {
        0x2
    };

    cpu_interrupts::without_interrupts(|| unsafe { scheduler_mut().spawn(entry, initial_rflags) })
}

pub fn spawn_user(init: UserTaskInit) -> Result<TaskId, SpawnError> {
    let initial_rflags = if rflags::read().contains(rflags::RFlags::INTERRUPT_FLAG) {
        0x202
    } else {
        0x2
    };

    cpu_interrupts::without_interrupts(|| unsafe {
        scheduler_mut().spawn_user(init, initial_rflags)
    })
}

pub fn spawn_user_elf(_name: &'static str, elf_bytes: &'static [u8]) -> Result<TaskId, SpawnError> {
    spawn_user_elf_with_arg(_name, elf_bytes, 0)
}

pub fn spawn_user_elf_with_arg(
    _name: &'static str,
    elf_bytes: &'static [u8],
    arg0: u64,
) -> Result<TaskId, SpawnError> {
    let init = crate::elf::load_user_elf(elf_bytes, arg0).map_err(SpawnError::ElfLoad)?;
    spawn_user(init)
}

pub fn run() {
    cpu_interrupts::without_interrupts(|| unsafe {
        let scheduler = scheduler_mut();
        let Some(next) = scheduler.first_ready() else {
            return;
        };

        scheduler.current = Some(next);
        scheduler.tasks[next].set_state(TaskState::Running);
        scheduler.current_level_4_frame = Some(Cr3::read().0.start_address().as_u64());
        scheduler.prepare_to_run(next);

        let old_context = core::ptr::addr_of_mut!(scheduler.main_context);
        let new_context = scheduler.tasks[next].context();

        context::switch_from_main(old_context, new_context);
    });
}

pub fn yield_now() {
    unsafe {
        asm!("int {vector}", vector = const crate::interrupts::YIELD_VECTOR);
    }
}

pub fn enable_preemption() {
    cpu_interrupts::without_interrupts(|| unsafe {
        scheduler_mut().preemption_enabled = true;
    });
}

pub fn disable_preemption() {
    cpu_interrupts::without_interrupts(|| unsafe {
        scheduler_mut().preemption_enabled = false;
    });
}

pub fn preemption_enabled() -> bool {
    cpu_interrupts::without_interrupts(|| unsafe { scheduler_mut().preemption_enabled })
}

pub fn finished_task_count() -> usize {
    cpu_interrupts::without_interrupts(|| unsafe { scheduler_mut().finished_count() })
}

pub fn failed_task_count() -> usize {
    cpu_interrupts::without_interrupts(|| unsafe { scheduler_mut().failed_count() })
}

pub fn all_tasks_finished() -> bool {
    cpu_interrupts::without_interrupts(|| unsafe { scheduler_mut().all_tasks_finished() })
}

pub fn task_exit_code(task_id: TaskId) -> Option<u64> {
    cpu_interrupts::without_interrupts(|| unsafe {
        scheduler_mut()
            .tasks
            .get(task_id.0)
            .and_then(Task::exit_code)
    })
}

pub fn task_fault_info(task_id: TaskId) -> Option<UserFaultInfo> {
    cpu_interrupts::without_interrupts(|| unsafe {
        scheduler_mut()
            .tasks
            .get(task_id.0)
            .and_then(Task::fault_info)
    })
}

pub fn read_user_u64(task_id: TaskId, address: VirtAddr) -> Option<u64> {
    cpu_interrupts::without_interrupts(|| unsafe {
        scheduler_mut()
            .tasks
            .get(task_id.0)
            .and_then(|task| task.read_user_u64(address))
    })
}

pub(crate) fn run_current_task() -> ! {
    let entry = cpu_interrupts::without_interrupts(|| unsafe {
        let scheduler = scheduler_mut();
        let current = scheduler.current.expect("no current task");

        scheduler.tasks[current]
            .kernel_entry()
            .expect("current task is not a kernel task")
    });

    entry();
    finish_current_task();
}

pub(crate) fn on_timer_interrupt(frame_rsp: u64) -> u64 {
    unsafe {
        let scheduler = scheduler_mut();

        if scheduler.preemption_enabled && scheduler.current.is_some() {
            scheduler.switch_from_interrupt(frame_rsp)
        } else {
            frame_rsp
        }
    }
}

pub(crate) fn on_yield_interrupt(frame_rsp: u64) -> u64 {
    unsafe { scheduler_mut().switch_from_interrupt(frame_rsp) }
}

pub(crate) fn on_syscall_yield(frame_rsp: u64) -> u64 {
    unsafe { scheduler_mut().switch_from_interrupt(frame_rsp) }
}

pub(crate) fn exit_current_from_interrupt(frame_rsp: u64, exit_code: u64) -> u64 {
    unsafe {
        scheduler_mut().finish_current_from_interrupt(
            frame_rsp,
            TaskState::Finished,
            Some(exit_code),
            None,
        )
    }
}

pub(crate) fn fail_current_with_fault(frame_rsp: u64, fault_info: UserFaultInfo) -> u64 {
    unsafe {
        scheduler_mut().finish_current_from_interrupt(
            frame_rsp,
            TaskState::Failed,
            None,
            Some(fault_info),
        )
    }
}

fn finish_current_task() -> ! {
    cpu_interrupts::without_interrupts(|| unsafe {
        let scheduler = scheduler_mut();
        let current = scheduler.current.expect("no current task");

        scheduler.tasks[current].set_state(TaskState::Finished);

        if let Some(next) = scheduler.next_ready_after(current) {
            scheduler.tasks[next].set_state(TaskState::Running);
            scheduler.current = Some(next);
            scheduler.prepare_to_run(next);

            let new_context = scheduler.tasks[next].context();
            context::restore_task(new_context);
        } else {
            scheduler.current = None;
            scheduler.switch_to_kernel_address_space();

            let main_context = core::ptr::addr_of!(scheduler.main_context);
            context::restore_main(main_context);
        }
    });

    println!("task returned after finish");
    hlt_loop();
}

unsafe fn scheduler_mut() -> &'static mut Scheduler {
    unsafe { &mut *core::ptr::addr_of_mut!(SCHEDULER) }
}
