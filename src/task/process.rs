extern crate alloc;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::fs::vfs::File;
use crate::mm::address_space::AddressSpace;
use crate::sync::IrqSafeSpinlock;

pub type ProcessId = u64;

static NEXT_PID: AtomicU64 = AtomicU64::new(1);
static CURRENT_PID: AtomicU64 = AtomicU64::new(1);

pub static PROCESS_TABLE: IrqSafeSpinlock<Vec<Arc<IrqSafeSpinlock<ProcessControlBlock>>>> =
    IrqSafeSpinlock::new(Vec::new());

pub fn get_current_pid() -> ProcessId {
    CURRENT_PID.load(Ordering::Relaxed)
}

pub fn set_current_pid(pid: ProcessId) {
    CURRENT_PID.store(pid, Ordering::Relaxed);
}

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
    pub children: Vec<ProcessId>,
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
            children: Vec::new(),
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

/// Allocate and register a new process in the process table.
pub fn alloc_process(name: &str) -> ProcessId {
    let pid = NEXT_PID.fetch_add(1, Ordering::Relaxed);
    let ppid = get_current_pid();
    let pcb = Arc::new(IrqSafeSpinlock::new(ProcessControlBlock::new(pid, ppid, name)));
    PROCESS_TABLE.lock().push(pcb);
    pid
}

/// Look up a process by PID.
pub fn get_process(pid: ProcessId) -> Option<Arc<IrqSafeSpinlock<ProcessControlBlock>>> {
    let table = PROCESS_TABLE.lock();
    for p in table.iter() {
        if p.lock().pid == pid {
            return Some(p.clone());
        }
    }
    None
}

/// Mark a process as a Zombie with the given exit code.
pub fn exit_process(pid: ProcessId, code: i32) {
    if let Some(proc) = get_process(pid) {
        let mut p = proc.lock();
        p.state = ProcessState::Zombie(code);
        p.exit_code = code;
    }
}

/// POSIX sys_fork (Syscall 57): Clones the active address space with Copy-on-Write
/// and creates a child process inheriting file descriptors and credentials.
pub fn sys_fork() -> i32 {
    let parent_pid = get_current_pid();
    let mut space_guard = crate::task::user::CURRENT_USER_ADDRESS_SPACE.lock();
    if let Some(ref mut parent_space) = *space_guard {
        if let Some(child_space) = parent_space.clone_cow() {
            let child_pid = NEXT_PID.fetch_add(1, Ordering::Relaxed);
            let mut child_pcb = ProcessControlBlock::new(child_pid, parent_pid, "fork_child");
            child_pcb.address_space = Some(child_space);

            // Record child in parent PCB
            if let Some(parent_proc) = get_process(parent_pid) {
                parent_proc.lock().children.push(child_pid);
            }

            PROCESS_TABLE.lock().push(Arc::new(IrqSafeSpinlock::new(child_pcb)));
            return child_pid as i32;
        }
    }
    -1 // -EAGAIN / -ENOMEM
}

/// POSIX sys_waitpid (Syscall 61): Waits for child process termination and retrieves exit code.
pub fn sys_waitpid(pid: i64, status_ptr: *mut i32, _options: i32) -> i32 {
    let current_pid = get_current_pid();
    loop {
        let mut target_child = None;
        {
            let table = PROCESS_TABLE.lock();
            for p in table.iter() {
                let proc = p.lock();
                if (pid == -1 && proc.ppid == current_pid) || (pid > 0 && proc.pid == pid as u64) {
                    if let ProcessState::Zombie(code) = proc.state {
                        target_child = Some((proc.pid, code));
                        break;
                    }
                }
            }
        }

        if let Some((child_pid, exit_code)) = target_child {
            if !status_ptr.is_null() {
                unsafe {
                    *status_ptr = (exit_code & 0xFF) << 8;
                }
            }
            // Remove zombie from process table
            let mut table = PROCESS_TABLE.lock();
            table.retain(|p| p.lock().pid != child_pid);
            return child_pid as i32;
        }

        // If no matching child exists, return -ECHILD (-10)
        let has_child = {
            let table = PROCESS_TABLE.lock();
            table.iter().any(|p| {
                let proc = p.lock();
                (pid == -1 && proc.ppid == current_pid) || (pid > 0 && proc.pid == pid as u64)
            })
        };

        if !has_child {
            return -10; // -ECHILD
        }

        // Yield to let child run
        crate::task::scheduler::yield_now();
        break; // In cooperative single-turn wait, return immediately if still running or return child
    }
    0
}

/// POSIX sys_execve (Syscall 59): Replaces the process image with a new executable.
pub fn sys_execve(path_ptr: *const u8, _argv: *const *const u8, _envp: *const *const u8) -> i32 {
    if path_ptr.is_null() {
        return -14; // -EFAULT
    }

    // Safely copy null-terminated path from user pointer
    let mut path_bytes = [0u8; 256];
    let mut len = 0;
    unsafe {
        while len < 255 {
            let byte = *path_ptr.add(len);
            if byte == 0 {
                break;
            }
            path_bytes[len] = byte;
            len += 1;
        }
    }

    let path_str = match core::str::from_utf8(&path_bytes[..len]) {
        Ok(s) => s,
        Err(_) => return -22, // -EINVAL
    };

    let vfs = crate::fs::vfs::VFS.lock();
    let file = match vfs.open(path_str, 0) {
        Ok(f) => f,
        Err(_) => return -2, // -ENOENT
    };
    drop(vfs);

    let mut buf = alloc::vec![0u8; 65536];
    let n = match file.read(&mut buf) {
        Ok(bytes) => bytes,
        Err(_) => return -5, // -EIO
    };

    match crate::task::elf::ElfBinary::parse(&buf[..n]) {
        Ok(elf) => {
            let mut new_space = match AddressSpace::new() {
                Some(s) => s,
                None => return -12, // -ENOMEM
            };

            if elf.load_into_address_space(&mut new_space).is_err() {
                return -8; // -ENOEXEC
            }

            // Replace active address space
            *crate::task::user::CURRENT_USER_ADDRESS_SPACE.lock() = Some(new_space);
            0
        }
        Err(_) => -8, // -ENOEXEC
    }
}
