use alloc::vec::Vec;

use x86_64::VirtAddr;

use crate::address_space::AddressSpace;
use crate::task::TaskId;

pub const MAX_PROCESSES: usize = 16;

pub const WAIT_EXITED: u32 = 0;
pub const WAIT_FAULTED: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Running,
    Zombie(ProcessExit),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessExit {
    Exited(i32),
    Faulted,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserWaitStatus {
    pub kind: u32,
    pub code: i32,
}

impl ProcessExit {
    pub fn wait_status(self) -> UserWaitStatus {
        match self {
            Self::Exited(code) => UserWaitStatus {
                kind: WAIT_EXITED,
                code,
            },
            Self::Faulted => UserWaitStatus {
                kind: WAIT_FAULTED,
                code: 0,
            },
        }
    }

    pub fn wait_status_bytes(self) -> [u8; core::mem::size_of::<UserWaitStatus>()] {
        let status = self.wait_status();
        let mut bytes = [0_u8; core::mem::size_of::<UserWaitStatus>()];

        bytes[..4].copy_from_slice(&status.kind.to_le_bytes());
        bytes[4..].copy_from_slice(&status.code.to_le_bytes());

        bytes
    }
}

pub struct Process {
    pid: ProcessId,
    parent: Option<ProcessId>,
    children: Vec<ProcessId>,
    state: ProcessState,
    address_space: AddressSpace,
    main_task: TaskId,
    orphaned: bool,
}

impl Process {
    pub fn pid(&self) -> ProcessId {
        self.pid
    }

    pub fn parent(&self) -> Option<ProcessId> {
        self.parent
    }

    pub fn state(&self) -> ProcessState {
        self.state
    }

    pub fn address_space(&self) -> &AddressSpace {
        &self.address_space
    }

    pub fn main_task(&self) -> TaskId {
        self.main_task
    }

    pub fn is_child(&self, child: ProcessId) -> bool {
        self.children.contains(&child)
    }

    fn remove_child(&mut self, child: ProcessId) {
        self.children.retain(|pid| *pid != child);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessError {
    Full,
    MissingParent,
    NotFound,
    NotChild,
    NotZombie,
}

pub struct ProcessTable {
    processes: Vec<Option<Process>>,
    next_pid: usize,
}

impl ProcessTable {
    pub const fn new() -> Self {
        Self {
            processes: Vec::new(),
            next_pid: 1,
        }
    }

    pub fn create(
        &mut self,
        parent: Option<ProcessId>,
        address_space: AddressSpace,
        main_task: TaskId,
    ) -> Result<ProcessId, ProcessError> {
        if self.active_count() >= MAX_PROCESSES {
            return Err(ProcessError::Full);
        }

        if let Some(parent) = parent {
            self.get(parent).ok_or(ProcessError::MissingParent)?;
        }

        let pid = ProcessId(self.next_pid);
        self.next_pid += 1;

        while self.processes.len() <= pid.0 {
            self.processes.push(None);
        }

        self.processes[pid.0] = Some(Process {
            pid,
            parent,
            children: Vec::new(),
            state: ProcessState::Running,
            address_space,
            main_task,
            orphaned: false,
        });

        if let Some(parent) = parent {
            self.get_mut(parent)
                .expect("parent disappeared during process creation")
                .children
                .push(pid);
        }

        Ok(pid)
    }

    pub fn get(&self, pid: ProcessId) -> Option<&Process> {
        self.processes.get(pid.0)?.as_ref()
    }

    pub fn get_mut(&mut self, pid: ProcessId) -> Option<&mut Process> {
        self.processes.get_mut(pid.0)?.as_mut()
    }

    pub fn exists(&self, pid: ProcessId) -> bool {
        self.get(pid).is_some()
    }

    pub fn state(&self, pid: ProcessId) -> Option<ProcessState> {
        Some(self.get(pid)?.state())
    }

    pub fn parent(&self, pid: ProcessId) -> Option<Option<ProcessId>> {
        Some(self.get(pid)?.parent())
    }

    pub fn address_space(&self, pid: ProcessId) -> Option<&AddressSpace> {
        Some(self.get(pid)?.address_space())
    }

    pub fn is_child(&self, parent: ProcessId, child: ProcessId) -> bool {
        self.get(parent)
            .map(|process| process.is_child(child))
            .unwrap_or(false)
    }

    pub fn mark_exited(&mut self, pid: ProcessId, exit: ProcessExit) -> Result<(), ProcessError> {
        let process = self.get_mut(pid).ok_or(ProcessError::NotFound)?;
        process.state = ProcessState::Zombie(exit);

        let children = process.children.clone();
        process.children.clear();

        for child in children {
            let should_reap = if let Some(child_process) = self.get_mut(child) {
                child_process.parent = None;
                child_process.orphaned = true;
                matches!(child_process.state, ProcessState::Zombie(_))
            } else {
                false
            };

            if should_reap {
                if let Some(slot) = self.processes.get_mut(child.0) {
                    *slot = None;
                }
            }
        }

        Ok(())
    }

    pub fn reap_child(
        &mut self,
        parent: ProcessId,
        child: ProcessId,
    ) -> Result<ProcessExit, ProcessError> {
        if !self.is_child(parent, child) {
            return Err(ProcessError::NotChild);
        }

        let exit = match self.get(child).ok_or(ProcessError::NotFound)?.state {
            ProcessState::Zombie(exit) => exit,
            ProcessState::Running => return Err(ProcessError::NotZombie),
        };

        if let Some(parent_process) = self.get_mut(parent) {
            parent_process.remove_child(child);
        }

        if let Some(slot) = self.processes.get_mut(child.0) {
            *slot = None;
        }

        Ok(exit)
    }

    pub fn reap_orphan_if_zombie(&mut self, pid: ProcessId) {
        let should_reap = self
            .get(pid)
            .map(|process| process.orphaned && matches!(process.state, ProcessState::Zombie(_)))
            .unwrap_or(false);

        if should_reap {
            if let Some(slot) = self.processes.get_mut(pid.0) {
                *slot = None;
            }
        }
    }

    fn active_count(&self) -> usize {
        self.processes
            .iter()
            .filter(|process| process.is_some())
            .count()
    }
}

pub fn wait_status_size() -> usize {
    core::mem::size_of::<UserWaitStatus>()
}

pub fn wait_status_address(address: u64) -> Option<VirtAddr> {
    if address == 0 {
        None
    } else {
        Some(VirtAddr::new(address))
    }
}
