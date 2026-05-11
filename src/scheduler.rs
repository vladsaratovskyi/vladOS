use alloc::vec::Vec;
use core::arch::asm;

use x86_64::instructions::interrupts as cpu_interrupts;
use x86_64::registers::control::{Cr3, Cr3Flags};
use x86_64::registers::rflags;
use x86_64::VirtAddr;

use crate::address_space::AddressSpace;
use crate::arch::x86_64::context::{self, Context};
use crate::elf::ElfLoadError;
use crate::gdt;
use crate::process::{ProcessError, ProcessExit, ProcessId, ProcessState, ProcessTable};
use crate::task::{Task, TaskEntry, TaskId, TaskState, UserFaultInfo, WaitReason, MAX_TASKS};
use crate::user::UserTaskInit;
use crate::user_memory::UserMemoryError;
use crate::{hlt_loop, println};

static mut SCHEDULER: Scheduler = Scheduler::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnError {
    Full,
    Process(ProcessError),
    ElfLoad(ElfLoadError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserProcessHandle {
    pub pid: ProcessId,
    pub task_id: TaskId,
}

struct Scheduler {
    tasks: Vec<Task>,
    processes: ProcessTable,
    current: Option<usize>,
    main_context: Context,
    preemption_enabled: bool,
    current_level_4_frame: Option<u64>,
}

impl Scheduler {
    const fn new() -> Self {
        Self {
            tasks: Vec::new(),
            processes: ProcessTable::new(),
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
    ) -> Result<UserProcessHandle, SpawnError> {
        self.spawn_user_process(None, init, initial_rflags)
    }

    fn spawn_user_process(
        &mut self,
        parent: Option<ProcessId>,
        init: UserTaskInit,
        initial_rflags: u64,
    ) -> Result<UserProcessHandle, SpawnError> {
        if self.tasks.len() >= MAX_TASKS {
            return Err(SpawnError::Full);
        }

        let id = TaskId(self.tasks.len());
        let pid = self
            .processes
            .create(parent, init.address_space, id)
            .map_err(SpawnError::Process)?;
        self.tasks.push(Task::new_user(
            id,
            pid,
            init.entry_point,
            init.user_stack_top,
            init.arg0,
            initial_rflags,
        ));

        Ok(UserProcessHandle { pid, task_id: id })
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

        if let Some(pid) = self.tasks[current].process_id() {
            let process_exit = match final_state {
                TaskState::Finished => ProcessExit::Exited(exit_code.unwrap_or(0) as i32),
                TaskState::Failed => ProcessExit::Faulted,
                _ => unreachable!("process exit must use a terminal task state"),
            };

            self.processes
                .mark_exited(pid, process_exit)
                .expect("current task process disappeared during exit");
            self.wake_parent_waiting_for(pid);
            self.processes.reap_orphan_if_zombie(pid);
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

    fn waitpid_from_interrupt(
        &mut self,
        frame_rsp: u64,
        child: ProcessId,
        status_ptr: Option<VirtAddr>,
        options: usize,
    ) -> u64 {
        let Some(current) = self.current else {
            return self.return_sys_error(frame_rsp, crate::syscall::SysError::Child);
        };
        let Some(parent) = self.tasks[current].process_id() else {
            return self.return_sys_error(frame_rsp, crate::syscall::SysError::Child);
        };

        if options != 0 && options != crate::syscall::WNOHANG {
            return self.return_sys_error(frame_rsp, crate::syscall::SysError::Invalid);
        }

        if child.0 == 0 || !self.processes.is_child(parent, child) {
            return self.return_sys_error(frame_rsp, crate::syscall::SysError::Child);
        }

        if let Some(status_ptr) = status_ptr {
            if self
                .validate_wait_status_target(parent, status_ptr)
                .is_err()
            {
                return self.return_sys_error(frame_rsp, crate::syscall::SysError::Fault);
            }
        }

        let Some(child_state) = self.processes.state(child) else {
            return self.return_sys_error(frame_rsp, crate::syscall::SysError::Child);
        };

        match child_state {
            ProcessState::Zombie(_) => {
                self.tasks[current].set_saved_rsp(frame_rsp);
                self.complete_wait(current, child, status_ptr);
                frame_rsp
            }
            ProcessState::Running => {
                if options == crate::syscall::WNOHANG {
                    self.set_frame_rax(frame_rsp, 0);
                    return frame_rsp;
                }

                self.tasks[current].set_saved_rsp(frame_rsp);
                self.tasks[current].set_state(TaskState::Blocked(WaitReason::ChildExit {
                    child,
                    status_ptr,
                }));

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
        }
    }

    fn complete_wait(
        &mut self,
        parent_task_index: usize,
        child: ProcessId,
        status_ptr: Option<VirtAddr>,
    ) {
        let Some(parent) = self.tasks[parent_task_index].process_id() else {
            self.tasks[parent_task_index]
                .set_saved_rax(crate::syscall::SysError::Child.raw_return());
            return;
        };
        let Some(ProcessState::Zombie(exit)) = self.processes.state(child) else {
            self.tasks[parent_task_index]
                .set_saved_rax(crate::syscall::SysError::Child.raw_return());
            return;
        };

        if let Some(status_ptr) = status_ptr {
            if self.write_wait_status(parent, status_ptr, exit).is_err() {
                self.tasks[parent_task_index]
                    .set_saved_rax(crate::syscall::SysError::Fault.raw_return());
                return;
            }
        }

        if self.processes.reap_child(parent, child).is_err() {
            self.tasks[parent_task_index]
                .set_saved_rax(crate::syscall::SysError::Child.raw_return());
            return;
        }

        self.tasks[parent_task_index].set_saved_rax(child.0 as u64);
    }

    fn wake_parent_waiting_for(&mut self, child: ProcessId) {
        let waiter = self
            .tasks
            .iter()
            .enumerate()
            .find_map(|(index, task)| match task.state() {
                TaskState::Blocked(WaitReason::ChildExit {
                    child: waited_child,
                    status_ptr,
                }) if waited_child == child => Some((index, status_ptr)),
                _ => None,
            });

        let Some((waiter, status_ptr)) = waiter else {
            return;
        };

        self.complete_wait(waiter, child, status_ptr);
        if matches!(self.tasks[waiter].state(), TaskState::Blocked(_)) {
            self.tasks[waiter].set_state(TaskState::Ready);
        }
    }

    fn validate_wait_status_target(
        &self,
        parent: ProcessId,
        status_ptr: VirtAddr,
    ) -> Result<(), UserMemoryError> {
        let address_space = self
            .processes
            .address_space(parent)
            .ok_or(UserMemoryError::Unmapped)?;

        crate::user_memory::validate_user_write_range(
            address_space,
            status_ptr,
            crate::process::wait_status_size(),
        )
    }

    fn write_wait_status(
        &self,
        parent: ProcessId,
        status_ptr: VirtAddr,
        exit: ProcessExit,
    ) -> Result<(), UserMemoryError> {
        let address_space = self
            .processes
            .address_space(parent)
            .ok_or(UserMemoryError::Unmapped)?;

        crate::user_memory::copy_to_user(address_space, status_ptr, &exit.wait_status_bytes())
    }

    fn return_sys_error(&self, frame_rsp: u64, error: crate::syscall::SysError) -> u64 {
        self.set_frame_rax(frame_rsp, error.raw_return());
        frame_rsp
    }

    fn set_frame_rax(&self, frame_rsp: u64, value: u64) {
        let frame = unsafe { &mut *(frame_rsp as *mut crate::arch::x86_64::context::TrapFrame) };
        frame.rax = value;
    }

    fn prepare_to_run(&mut self, task_index: usize) {
        self.load_kernel_stack_for(task_index);
        self.switch_to_task_address_space(task_index);
    }

    fn load_kernel_stack_for(&self, task_index: usize) {
        gdt::set_kernel_stack(self.tasks[task_index].kernel_stack_top());
    }

    fn switch_to_task_address_space(&mut self, task_index: usize) {
        let next_frame = match self.tasks[task_index].process_id() {
            Some(pid) => self
                .processes
                .address_space(pid)
                .expect("runnable user task has no process address space")
                .level_4_frame(),
            None => crate::memory::kernel_level_4_frame(),
        };
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
    spawn_user_process(init).map(|handle| handle.task_id)
}

pub fn spawn_user_process(init: UserTaskInit) -> Result<UserProcessHandle, SpawnError> {
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
    spawn_user_elf_process_with_arg(_name, elf_bytes, arg0).map(|handle| handle.task_id)
}

pub fn spawn_user_elf_process(
    name: &'static str,
    elf_bytes: &'static [u8],
) -> Result<UserProcessHandle, SpawnError> {
    spawn_user_elf_process_with_arg(name, elf_bytes, 0)
}

pub fn spawn_user_elf_process_with_arg(
    _name: &'static str,
    elf_bytes: &'static [u8],
    arg0: u64,
) -> Result<UserProcessHandle, SpawnError> {
    let init = crate::elf::load_user_elf(elf_bytes, arg0).map_err(SpawnError::ElfLoad)?;
    spawn_user_process(init)
}

pub fn spawn_child_user_elf_process(
    parent: ProcessId,
    name: &'static str,
    elf_bytes: &'static [u8],
) -> Result<UserProcessHandle, SpawnError> {
    spawn_child_user_elf_process_with_arg(parent, name, elf_bytes, 0)
}

pub fn spawn_child_user_elf_process_with_arg(
    parent: ProcessId,
    _name: &'static str,
    elf_bytes: &'static [u8],
    arg0: u64,
) -> Result<UserProcessHandle, SpawnError> {
    let init = crate::elf::load_user_elf(elf_bytes, arg0).map_err(SpawnError::ElfLoad)?;
    let initial_rflags = if rflags::read().contains(rflags::RFlags::INTERRUPT_FLAG) {
        0x202
    } else {
        0x2
    };

    cpu_interrupts::without_interrupts(|| unsafe {
        scheduler_mut().spawn_user_process(Some(parent), init, initial_rflags)
    })
}

pub fn set_task_initial_arg(task_id: TaskId, arg0: u64) -> bool {
    cpu_interrupts::without_interrupts(|| unsafe {
        let scheduler = scheduler_mut();
        let Some(task) = scheduler.tasks.get_mut(task_id.0) else {
            return false;
        };

        if task.state() != TaskState::Ready {
            return false;
        }

        task.set_saved_rdi(arg0);
        true
    })
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

pub fn task_process_id(task_id: TaskId) -> Option<ProcessId> {
    cpu_interrupts::without_interrupts(|| unsafe {
        scheduler_mut()
            .tasks
            .get(task_id.0)
            .and_then(Task::process_id)
    })
}

pub fn process_state(pid: ProcessId) -> Option<ProcessState> {
    cpu_interrupts::without_interrupts(|| unsafe { scheduler_mut().processes.state(pid) })
}

pub fn process_parent(pid: ProcessId) -> Option<Option<ProcessId>> {
    cpu_interrupts::without_interrupts(|| unsafe { scheduler_mut().processes.parent(pid) })
}

pub fn process_exists(pid: ProcessId) -> bool {
    cpu_interrupts::without_interrupts(|| unsafe { scheduler_mut().processes.exists(pid) })
}

pub fn read_user_u64(task_id: TaskId, address: VirtAddr) -> Option<u64> {
    cpu_interrupts::without_interrupts(|| unsafe {
        let scheduler = scheduler_mut();
        let process_id = scheduler.tasks.get(task_id.0)?.process_id()?;
        scheduler
            .processes
            .address_space(process_id)?
            .read_user_u64(address)
    })
}

pub fn copy_to_user(
    task_id: TaskId,
    address: VirtAddr,
    bytes: &[u8],
) -> Result<(), UserMemoryError> {
    cpu_interrupts::without_interrupts(|| unsafe {
        let scheduler = scheduler_mut();
        let process_id = scheduler
            .tasks
            .get(task_id.0)
            .and_then(Task::process_id)
            .ok_or(UserMemoryError::Unmapped)?;
        let address_space = scheduler
            .processes
            .address_space(process_id)
            .ok_or(UserMemoryError::Unmapped)?;

        crate::user_memory::copy_to_user(address_space, address, bytes)
    })
}

pub(crate) fn with_current_user_address_space<R>(
    f: impl FnOnce(&AddressSpace) -> R,
) -> Result<R, UserMemoryError> {
    unsafe {
        let scheduler = scheduler_mut();
        let current = scheduler.current.ok_or(UserMemoryError::Unmapped)?;
        let process_id = scheduler.tasks[current]
            .process_id()
            .ok_or(UserMemoryError::Unmapped)?;
        let address_space = scheduler
            .processes
            .address_space(process_id)
            .ok_or(UserMemoryError::Unmapped)?;

        Ok(f(address_space))
    }
}

pub(crate) fn current_process_id() -> Option<ProcessId> {
    unsafe {
        let scheduler = scheduler_mut();
        let current = scheduler.current?;
        scheduler.tasks[current].process_id()
    }
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

pub(crate) fn on_syscall_waitpid(
    frame_rsp: u64,
    child: ProcessId,
    status_ptr: Option<VirtAddr>,
    options: usize,
) -> u64 {
    unsafe { scheduler_mut().waitpid_from_interrupt(frame_rsp, child, status_ptr, options) }
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
