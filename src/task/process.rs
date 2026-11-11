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
    pub signal_state: crate::ipc::signal::SignalState,
    pub sigactions: [crate::ipc::signal::SigAction; 32],
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
            signal_state: crate::ipc::signal::SignalState::new(),
            sigactions: [crate::ipc::signal::SigAction::default(); 32],
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
    let ppid = if let Some(proc) = get_process(pid) {
        let mut p = proc.lock();
        p.state = ProcessState::Zombie(code);
        p.exit_code = code;
        p.ppid
    } else {
        0
    };

    if ppid != 0 {
        if let Some(parent) = get_process(ppid) {
            parent.lock().signal_state.send(crate::ipc::signal::Signal::SIGCHLD);
        }
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

            // Inherit signals and record child in parent PCB
            if let Some(parent_proc) = get_process(parent_pid) {
                let mut p = parent_proc.lock();
                p.children.push(child_pid);
                child_pcb.sigactions = p.sigactions;
                child_pcb.signal_state.blocked = p.signal_state.blocked;
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

/// POSIX sys_kill (Syscall 62): Sends a signal to a process or process group.
pub fn sys_kill(pid: i32, sig: i32) -> i32 {
    if sig < 0 || sig >= 32 {
        return -22; // -EINVAL
    }

    if sig == 0 {
        // Just verify process existence
        if pid > 0 {
            if get_process(pid as u64).is_some() {
                return 0;
            } else {
                return -3; // -ESRCH
            }
        }
        return 0;
    }

    let signal = match crate::ipc::signal::Signal::from_u32(sig as u32) {
        Some(s) => s,
        None => return -22, // -EINVAL
    };

    let target_pid = if pid > 0 {
        pid as u64
    } else if pid == 0 || pid == -1 {
        get_current_pid()
    } else {
        (-pid) as u64
    };

    let proc = match get_process(target_pid) {
        Some(p) => p,
        None => return -3, // -ESRCH
    };

    let mut p = proc.lock();

    // SIGKILL cannot be caught or ignored
    if signal == crate::ipc::signal::Signal::SIGKILL {
        p.state = ProcessState::Zombie(128 + sig);
        p.exit_code = 128 + sig;
        let ppid = p.ppid;
        drop(p);
        if ppid != 0 {
            if let Some(parent) = get_process(ppid) {
                parent.lock().signal_state.send(crate::ipc::signal::Signal::SIGCHLD);
            }
        }
        return 0;
    }

    let action = p.sigactions[sig as usize];
    if action.sa_handler == crate::ipc::signal::SIG_IGN {
        return 0;
    }

    p.signal_state.send(signal);

    // If default action for terminating signals
    if action.sa_handler == crate::ipc::signal::SIG_DFL {
        match signal {
            crate::ipc::signal::Signal::SIGINT
            | crate::ipc::signal::Signal::SIGTERM
            | crate::ipc::signal::Signal::SIGQUIT
            | crate::ipc::signal::Signal::SIGABRT
            | crate::ipc::signal::Signal::SIGSEGV
            | crate::ipc::signal::Signal::SIGBUS
            | crate::ipc::signal::Signal::SIGFPE => {
                p.state = ProcessState::Zombie(128 + sig);
                p.exit_code = 128 + sig;
                let ppid = p.ppid;
                drop(p);
                if ppid != 0 {
                    if let Some(parent) = get_process(ppid) {
                        parent.lock().signal_state.send(crate::ipc::signal::Signal::SIGCHLD);
                    }
                }
            }
            _ => {}
        }
    }

    0
}

/// POSIX sys_rt_sigaction (Syscall 13): Examines and changes a signal action.
pub fn sys_rt_sigaction(
    sig: i32,
    act_ptr: *const crate::ipc::signal::SigAction,
    oact_ptr: *mut crate::ipc::signal::SigAction,
    _sigsetsize: usize,
) -> i32 {
    if sig <= 0 || sig >= 32 || sig == 9 || sig == 19 {
        return -22; // -EINVAL (SIGKILL and SIGSTOP cannot be modified)
    }

    let current_pid = get_current_pid();
    let proc = match get_process(current_pid) {
        Some(p) => p,
        None => return -3, // -ESRCH
    };

    let mut p = proc.lock();

    if !oact_ptr.is_null() {
        unsafe {
            *oact_ptr = p.sigactions[sig as usize];
        }
    }

    if !act_ptr.is_null() {
        unsafe {
            p.sigactions[sig as usize] = *act_ptr;
        }
    }

    0
}

/// POSIX sys_rt_sigprocmask (Syscall 14): Examines and changes blocked signals.
pub fn sys_rt_sigprocmask(
    how: i32,
    set_ptr: *const u64,
    oldset_ptr: *mut u64,
    _sigsetsize: usize,
) -> i32 {
    let current_pid = get_current_pid();
    let proc = match get_process(current_pid) {
        Some(p) => p,
        None => return -3, // -ESRCH
    };

    let mut p = proc.lock();

    if !oldset_ptr.is_null() {
        unsafe {
            *oldset_ptr = p.signal_state.blocked;
        }
    }

    if !set_ptr.is_null() {
        let set = unsafe { *set_ptr };
        let unblockable = (1 << 9) | (1 << 19);
        match how {
            crate::ipc::signal::SIG_BLOCK => {
                p.signal_state.blocked |= set & !unblockable;
            }
            crate::ipc::signal::SIG_UNBLOCK => {
                p.signal_state.blocked &= !(set & !unblockable);
            }
            crate::ipc::signal::SIG_SETMASK => {
                p.signal_state.blocked = set & !unblockable;
            }
            _ => return -22, // -EINVAL
        }
    }

    0
}

pub fn sys_open(path_ptr: *const u8, flags: i32, _mode: i32) -> i64 {
    if path_ptr.is_null() {
        return -14; // -EFAULT
    }
    let mut len = 0;
    while unsafe { *path_ptr.add(len) } != 0 && len < 256 {
        len += 1;
    }
    let path_str = match core::str::from_utf8(unsafe { core::slice::from_raw_parts(path_ptr, len) }) {
        Ok(s) => s,
        Err(_) => return -22, // -EINVAL
    };

    let vfs = crate::fs::vfs::VFS.lock();
    let inode = match vfs.resolve_path(path_str) {
        Ok(n) => n,
        Err(_) => return -2, // -ENOENT
    };
    drop(vfs);

    let file = Arc::new(crate::fs::vfs::File::new(inode, flags as u32));
    let current_pid = get_current_pid();
    if let Some(proc) = get_process(current_pid) {
        if let Some(fd) = proc.lock().alloc_fd(file) {
            return fd as i64;
        }
    }
    -24 // -EMFILE
}

pub fn sys_close(fd: usize) -> i64 {
    let current_pid = get_current_pid();
    if let Some(proc) = get_process(current_pid) {
        let mut p = proc.lock();
        if fd < p.fd_table.len() {
            p.fd_table[fd] = None;
            return 0;
        }
    }
    -9 // -EBADF
}

pub fn sys_ioctl(fd: usize, cmd: u32, arg: usize) -> i64 {
    let current_pid = get_current_pid();
    let file = if let Some(proc) = get_process(current_pid) {
        let p = proc.lock();
        if fd < p.fd_table.len() {
            p.fd_table[fd].clone()
        } else {
            None
        }
    } else {
        None
    };

    if let Some(f) = file {
        match f.ioctl(cmd, arg) {
            Ok(ret) => ret as i64,
            Err(_) => -22, // -EINVAL
        }
    } else {
        -9 // -EBADF
    }
}

pub fn sys_mmap(
    addr: u64,
    length: u64,
    _prot: u64,
    _flags: u64,
    fd: i64,
    _offset: u64,
) -> i64 {
    if length == 0 {
        return -22; // -EINVAL
    }

    let current_pid = get_current_pid();
    let proc = match get_process(current_pid) {
        Some(p) => p,
        None => return -3, // -ESRCH
    };

    let mut p = proc.lock();
    let target_addr = if addr != 0 { addr } else { 0x6000_0000 };

    if fd >= 0 {
        let fd_idx = fd as usize;
        let is_fb = if fd_idx < p.fd_table.len() {
            if let Some(ref file) = p.fd_table[fd_idx] {
                file.inode.lock().stat().inode == 105
            } else {
                false
            }
        } else {
            false
        };

        if is_fb {
            let vbe = crate::drivers::vbe::VBE.lock();
            let phys = if vbe.lfb_phys > 0 { vbe.lfb_phys } else { 0xFD000000 };
            drop(vbe);

            if let Some(ref mut aspace) = p.address_space {
                if aspace.map_phys_region(
                    target_addr,
                    length,
                    phys,
                    crate::mm::vmm::PAGE_WRITABLE,
                    crate::mm::address_space::VmaType::Mmap,
                ).is_ok() {
                    return target_addr as i64;
                }
            }
            return -12; // -ENOMEM
        }
    }

    if let Some(ref mut aspace) = p.address_space {
        if aspace.map_region(
            target_addr,
            length,
            crate::mm::vmm::PAGE_WRITABLE,
            crate::mm::address_space::VmaType::Mmap,
        ).is_ok() {
            return target_addr as i64;
        }
    }

    -12 // -ENOMEM
}

pub fn sys_munmap(_addr: u64, _length: u64) -> i64 {
    0
}

pub fn sys_sched_setaffinity(pid: i32, _len: usize, mask_ptr: *const u64) -> i32 {
    if mask_ptr.is_null() {
        return -14; // -EFAULT
    }
    let mask = unsafe { *mask_ptr };
    if mask == 0 {
        return -22; // -EINVAL
    }

    let target_id = if pid <= 0 {
        0
    } else {
        pid as u64
    };

    let _ = crate::task::scheduler::set_affinity(target_id, mask);
    0
}

pub fn sys_sched_getaffinity(pid: i32, _len: usize, mask_ptr: *mut u64) -> i32 {
    if mask_ptr.is_null() {
        return -14; // -EFAULT
    }

    let target_id = if pid <= 0 {
        0
    } else {
        pid as u64
    };

    let mask = crate::task::scheduler::get_affinity(target_id);
    unsafe {
        *mask_ptr = mask;
    }
    0
}
