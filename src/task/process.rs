extern crate alloc;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::fs::vfs::File;
use crate::mm::address_space::AddressSpace;

pub type ProcessId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Running,
    Waiting,
    Zombie(i32),
}

pub struct ProcessControlBlock {
    pub pid: ProcessId,
    pub ppid: ProcessId,
    pub name: String,
    pub state: ProcessState,
    pub address_space: Option<AddressSpace>,
    pub fd_table: Vec<Option<Arc<File>>>,
    pub exit_code: i32,
}

impl ProcessControlBlock {
    pub fn new(pid: ProcessId, ppid: ProcessId, name: &str) -> Self {
        let mut fd_table = Vec::with_capacity(16);
        for _ in 0..16 {
            fd_table.push(None);
        }

        Self {
            pid,
            ppid,
            name: String::from(name),
            state: ProcessState::Running,
            address_space: AddressSpace::new(),
            fd_table,
            exit_code: 0,
        }
    }

    /// Allocate the lowest available file descriptor.
    pub fn alloc_fd(&mut self, file: Arc<File>) -> Option<usize> {
        for (i, entry) in self.fd_table.iter_mut().enumerate() {
            if entry.is_none() {
                *entry = Some(file);
                return Some(i);
            }
        }
        if self.fd_table.len() < 256 {
            let fd = self.fd_table.len();
            self.fd_table.push(Some(file));
            Some(fd)
        } else {
            None
        }
    }

    /// Get a file from a file descriptor.
    pub fn get_fd(&self, fd: usize) -> Option<Arc<File>> {
        self.fd_table.get(fd).and_then(|f| f.clone())
    }

    /// Close a file descriptor.
    pub fn close_fd(&mut self, fd: usize) -> bool {
        if let Some(entry) = self.fd_table.get_mut(fd) {
            if entry.is_some() {
                *entry = None;
                return true;
            }
        }
        false
    }
}
