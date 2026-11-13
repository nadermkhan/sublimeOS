extern crate alloc;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::fs::vfs::File;
use crate::mm::address_space::AddressSpace;
use crate::sync::IrqSafeSpinlock;

pub type ProcessId = u64;

static NEXT_PID: AtomicU64 = AtomicU64::new(2);
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
    Stopped(i32),
    Zombie(i32),
}

pub struct ProcessControlBlock {
    pub pid: ProcessId,
    pub ppid: ProcessId,
    pub pgid: ProcessId,
    pub sid: ProcessId,
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
        Self::new_with_space(pid, ppid, name, AddressSpace::new())
    }

    pub fn new_with_space(
        pid: ProcessId,
        ppid: ProcessId,
        name: &str,
        address_space: Option<AddressSpace>,
    ) -> Self {
        let mut fd_table = Vec::with_capacity(16);
        for _ in 0..16 {
            fd_table.push(None);
        }

        Self {
            pid,
            ppid,
            pgid: pid,
            sid: if ppid > 0 { ppid } else { pid },
            name: String::from(name),
            state: ProcessState::Running,
            address_space,
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

/// Initialize the bootstrap kernel PID 1 slot before `/bin/init` takes over PID 1.
pub fn init() {
    let mut table = PROCESS_TABLE.lock();
    if !table.iter().any(|p| p.lock().pid == 1) {
        let pcb = Arc::new(IrqSafeSpinlock::new(ProcessControlBlock::new(1, 0, "kernel_boot")));
        table.push(pcb);
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

/// Allocate and register a new process with a pre-built AddressSpace (avoids extra PML4 allocation).
pub fn alloc_process_with_space(name: &str, ppid: ProcessId, space: AddressSpace) -> ProcessId {
    let pid = NEXT_PID.fetch_add(1, Ordering::Relaxed);
    let mut pcb = ProcessControlBlock::new_with_space(pid, ppid, name, Some(space));
    if ppid != 0 {
        if let Some(parent_proc) = get_process(ppid) {
            let mut p = parent_proc.lock();
            p.children.push(pid);
            pcb.pgid = p.pgid;
            pcb.sid = p.sid;
            pcb.fd_table = p.fd_table.clone();
        }
    }
    PROCESS_TABLE.lock().push(Arc::new(IrqSafeSpinlock::new(pcb)));
    pid
}

/// Allocate and initialize the master init process strictly as PID 1.
pub fn alloc_init_process() -> ProcessId {
    let mut table = PROCESS_TABLE.lock();
    table.retain(|p| p.lock().pid != 1);
    let pcb = Arc::new(IrqSafeSpinlock::new(ProcessControlBlock::new(1, 0, "init")));
    table.push(pcb);
    1
}

/// Allocate and initialize the master init process strictly as PID 1 with a pre-built AddressSpace.
pub fn alloc_init_process_with_space(space: AddressSpace) -> ProcessId {
    let mut table = PROCESS_TABLE.lock();
    table.retain(|p| p.lock().pid != 1);
    let pcb = Arc::new(IrqSafeSpinlock::new(ProcessControlBlock::new_with_space(
        1,
        0,
        "init",
        Some(space),
    )));
    table.push(pcb);
    1
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

/// Mark a process as a Zombie with the given exit code and wake any waiting parent.
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
        crate::task::scheduler::wake_pid(ppid);
    }
}

/// POSIX sys_fork (Syscall 57): Clones the active address space with Copy-on-Write,
/// creates a child process inheriting descriptors/credentials, and schedules a forked
/// Ring 3 thread when invoked from `syscall` with a captured `SyscallFrame`.
pub fn sys_fork_with_frame(frame: Option<&crate::task::thread::SyscallFrame>) -> i32 {
    let parent_pid = get_current_pid();
    let parent_proc = get_process(parent_pid);

    let mut child_space_opt = None;
    let mut inherited_name = String::from("fork_child");
    let mut inherited_pgid = parent_pid;
    let mut inherited_sid = parent_pid;
    let mut inherited_sigactions = [crate::ipc::signal::SigAction::default(); 32];
    let mut inherited_blocked = 0u64;
    let mut inherited_fds = Vec::new();

    if let Some(ref pproc) = parent_proc {
        let mut p = pproc.lock();
        inherited_name = alloc::format!("{}_child", p.name);
        inherited_pgid = p.pgid;
        inherited_sid = p.sid;
        inherited_sigactions = p.sigactions;
        inherited_blocked = p.signal_state.blocked;
        inherited_fds = p.fd_table.clone();
        if let Some(ref mut space) = p.address_space {
            child_space_opt = space.clone_cow();
        }
    }

    if child_space_opt.is_none() {
        let mut space_guard = crate::task::user::CURRENT_USER_ADDRESS_SPACE.lock();
        if let Some(ref mut parent_space) = *space_guard {
            child_space_opt = parent_space.clone_cow();
        }
    }

    let child_space = match child_space_opt {
        Some(s) => s,
        None => return -1, // -EAGAIN / -ENOMEM
    };

    let child_cr3 = child_space.pml4_phys;
    let child_pid = NEXT_PID.fetch_add(1, Ordering::Relaxed);
    let mut child_pcb =
        ProcessControlBlock::new_with_space(child_pid, parent_pid, &inherited_name, Some(child_space));
    child_pcb.pgid = inherited_pgid;
    child_pcb.sid = inherited_sid;
    child_pcb.sigactions = inherited_sigactions;
    child_pcb.signal_state.blocked = inherited_blocked;
    if !inherited_fds.is_empty() {
        child_pcb.fd_table = inherited_fds;
    }

    if let Some(ref pproc) = parent_proc {
        pproc.lock().children.push(child_pid);
    }

    PROCESS_TABLE.lock().push(Arc::new(IrqSafeSpinlock::new(child_pcb)));

    if let Some(f) = frame {
        crate::task::scheduler::spawn_forked_user_thread(child_pid, &inherited_name, child_cr3, f);
    }

    child_pid as i32
}

/// POSIX sys_fork compatibility entry point for kernel-side tests.
pub fn sys_fork() -> i32 {
    sys_fork_with_frame(None)
}

pub const WNOHANG: i32 = 1;
pub const WUNTRACED: i32 = 2;
pub const WCONTINUED: i32 = 8;

/// POSIX sys_waitpid (Syscall 61): Waits for child process termination or state change.
pub fn sys_waitpid(pid: i64, status_ptr: *mut i32, options: i32) -> i32 {
    let current_pid = get_current_pid();
    let current_pgid = get_process(current_pid)
        .map(|cp| cp.lock().pgid)
        .unwrap_or(current_pid);

    loop {
        let mut target_child = None;
        let mut target_stopped = None;
        {
            let table = PROCESS_TABLE.lock();
            for p in table.iter() {
                let proc = p.lock();
                let matches_target = if pid == -1 {
                    proc.ppid == current_pid
                } else if pid > 0 {
                    proc.pid == pid as u64
                } else if pid < -1 {
                    proc.pgid == (-pid) as u64
                } else {
                    proc.pgid == current_pgid
                };

                if matches_target {
                    match proc.state {
                        ProcessState::Zombie(code) => {
                            target_child = Some((proc.pid, code));
                            break;
                        }
                        ProcessState::Stopped(sig) => {
                            if (options & WUNTRACED) != 0 {
                                target_stopped = Some((proc.pid, sig));
                                break;
                            }
                        }
                        _ => {}
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

        if let Some((child_pid, stop_sig)) = target_stopped {
            if !status_ptr.is_null() {
                unsafe {
                    *status_ptr = 0x007F | ((stop_sig & 0xFF) << 8);
                }
            }
            return child_pid as i32;
        }

        // If no matching child exists, return -ECHILD (-10)
        let has_child = {
            let table = PROCESS_TABLE.lock();
            table.iter().any(|p| {
                let proc = p.lock();
                if pid == -1 {
                    proc.ppid == current_pid
                } else if pid > 0 {
                    proc.pid == pid as u64
                } else if pid < -1 {
                    proc.pgid == (-pid) as u64
                } else {
                    proc.pgid == current_pgid
                }
            })
        };

        if !has_child {
            return -10; // -ECHILD
        }

        if (options & WNOHANG) != 0 {
            return 0; // Child is still running, non-blocking return
        }

        // Sleep briefly (woken early by exit_process via scheduler::wake_pid)
        crate::task::scheduler::sleep_ms(5);
    }
}

/// Block until `child_pid` terminates, reap its zombie PCB, and return its raw exit code.
pub fn wait_for_child_exit(child_pid: ProcessId) -> i32 {
    loop {
        let zombie_code = if let Some(proc) = get_process(child_pid) {
            let p = proc.lock();
            match p.state {
                ProcessState::Zombie(code) => Some(code),
                _ => None,
            }
        } else {
            return -10; // -ECHILD
        };

        if let Some(code) = zombie_code {
            let mut table = PROCESS_TABLE.lock();
            table.retain(|p| p.lock().pid != child_pid);
            return code;
        }

        crate::task::scheduler::sleep_ms(5);
    }
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

    let current_pid = get_current_pid();
    let current_pgid = get_process(current_pid).map(|cp| cp.lock().pgid).unwrap_or(current_pid);

    let mut target_pids = Vec::new();
    {
        let table = PROCESS_TABLE.lock();
        if pid > 0 {
            if table.iter().any(|p| p.lock().pid == pid as u64) {
                target_pids.push(pid as u64);
            }
        } else if pid == 0 {
            // All processes in caller's process group
            for p in table.iter() {
                let proc = p.lock();
                if proc.pgid == current_pgid {
                    target_pids.push(proc.pid);
                }
            }
        } else if pid < -1 {
            // All processes in process group -pid
            let target_pgid = (-pid) as u64;
            for p in table.iter() {
                let proc = p.lock();
                if proc.pgid == target_pgid {
                    target_pids.push(proc.pid);
                }
            }
        } else {
            // pid == -1: caller process
            target_pids.push(current_pid);
        }
    }

    if target_pids.is_empty() {
        return -3; // -ESRCH
    }

    for target_pid in target_pids {
        if let Some(proc) = get_process(target_pid) {
            let mut p = proc.lock();

            // SIGKILL cannot be caught or ignored
            if signal == crate::ipc::signal::Signal::SIGKILL {
                p.state = ProcessState::Zombie(128 + sig);
                p.exit_code = 128 + sig;
                let ppid = p.ppid;
                drop(p);
                crate::task::scheduler::kill_process_threads(target_pid);
                if ppid != 0 {
                    if let Some(parent) = get_process(ppid) {
                        parent.lock().signal_state.send(crate::ipc::signal::Signal::SIGCHLD);
                    }
                    crate::task::scheduler::wake_pid(ppid);
                }
                continue;
            }

            let action = p.sigactions[sig as usize];
            if action.sa_handler == crate::ipc::signal::SIG_IGN {
                continue;
            }

            p.signal_state.send(signal);

            // Default actions for signals
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
                        crate::task::scheduler::kill_process_threads(target_pid);
                        if ppid != 0 {
                            if let Some(parent) = get_process(ppid) {
                                parent.lock().signal_state.send(crate::ipc::signal::Signal::SIGCHLD);
                            }
                            crate::task::scheduler::wake_pid(ppid);
                        }
                    }
                    crate::ipc::signal::Signal::SIGTSTP | crate::ipc::signal::Signal::SIGSTOP => {
                        p.state = ProcessState::Stopped(sig);
                        let ppid = p.ppid;
                        drop(p);
                        if ppid != 0 {
                            if let Some(parent) = get_process(ppid) {
                                parent.lock().signal_state.send(crate::ipc::signal::Signal::SIGCHLD);
                            }
                        }
                    }
                    crate::ipc::signal::Signal::SIGCONT => {
                        if matches!(p.state, ProcessState::Stopped(_)) {
                            p.state = ProcessState::Running;
                        }
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
        }
    }

    0
}

/// POSIX sys_setpgid (Syscall 109): Sets process group ID of process.
pub fn sys_setpgid(pid: i32, pgid: i32) -> i32 {
    let target_pid = if pid <= 0 { get_current_pid() } else { pid as u64 };
    let new_pgid = if pgid <= 0 { target_pid } else { pgid as u64 };

    if let Some(proc) = get_process(target_pid) {
        let mut p = proc.lock();
        p.pgid = new_pgid;
        0
    } else {
        -3 // -ESRCH
    }
}

/// POSIX sys_getpgrp (Syscall 111): Returns calling process's process group ID.
pub fn sys_getpgrp() -> i32 {
    let current_pid = get_current_pid();
    if let Some(proc) = get_process(current_pid) {
        proc.lock().pgid as i32
    } else {
        current_pid as i32
    }
}

/// POSIX sys_getpgid (Syscall 121): Returns process group ID of specified process.
pub fn sys_getpgid(pid: i32) -> i32 {
    let target_pid = if pid <= 0 { get_current_pid() } else { pid as u64 };
    if let Some(proc) = get_process(target_pid) {
        proc.lock().pgid as i32
    } else {
        -3 // -ESRCH
    }
}

/// POSIX sys_setsid (Syscall 112): Creates a new session if caller is not group leader.
pub fn sys_setsid() -> i32 {
    let current_pid = get_current_pid();
    if let Some(proc) = get_process(current_pid) {
        let mut p = proc.lock();
        p.sid = current_pid;
        p.pgid = current_pid;
        current_pid as i32
    } else {
        -1 // -EPERM
    }
}

/// POSIX sys_getsid (Syscall 124): Obtains session ID of a process.
pub fn sys_getsid(pid: i32) -> i32 {
    let target_pid = if pid <= 0 { get_current_pid() } else { pid as u64 };
    if let Some(proc) = get_process(target_pid) {
        proc.lock().sid as i32
    } else {
        -3 // -ESRCH
    }
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

    let inode = if path_str == "/dev/ptmx" {
        crate::tty::pty::allocate_pty_master()
    } else {
        let vfs = crate::fs::vfs::VFS.lock();
        match vfs.resolve_path(path_str) {
            Ok(n) => {
                if (flags & 0x200) != 0 {
                    // O_TRUNC: truncate existing file
                    let _ = n.lock().truncate(0);
                }
                n
            }
            Err(_) => {
                if (flags & 0x40) != 0 {
                    // O_CREAT: create file in parent directory
                    let mut parts: alloc::vec::Vec<&str> = path_str.split('/').filter(|s| !s.is_empty()).collect();
                    if parts.is_empty() {
                        return -22;
                    }
                    let filename = parts.pop().unwrap();
                    let parent_path = if parts.is_empty() { "/" } else { &path_str[..path_str.rfind('/').unwrap_or(0)] };
                    let parent = match vfs.resolve_path(parent_path) {
                        Ok(p) => p,
                        Err(_) => return -2,
                    };
                    drop(vfs);
                    let created = match parent.lock().create(filename, crate::fs::vfs::FileType::Regular) {
                        Ok(n) => n,
                        Err(_) => return -13, // -EACCES
                    };
                    created
                } else {
                    return -2; // -ENOENT
                }
            }
        }
    };

    let file = Arc::new(crate::fs::vfs::File::new(inode, flags as u32));
    let current_pid = get_current_pid();
    if let Some(proc) = get_process(current_pid) {
        if let Some(fd) = proc.lock().alloc_fd(file) {
            return fd as i64;
        }
    }
    -24 // -EMFILE
}

pub fn sys_posix_openpt(flags: i32) -> i64 {
    let inode = crate::tty::pty::allocate_pty_master();
    let file = Arc::new(crate::fs::vfs::File::new(inode, flags as u32));
    let current_pid = get_current_pid();
    if let Some(proc) = get_process(current_pid) {
        if let Some(fd) = proc.lock().alloc_fd(file) {
            return fd as i64;
        }
    }
    -24 // -EMFILE
}

pub fn sys_ptsname(fd: usize, buf: *mut u8, buflen: usize) -> i64 {
    if buf.is_null() || buflen == 0 {
        return -14; // -EFAULT
    }
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
        let mut pty_num: u32 = 0;
        if f.ioctl(crate::tty::pty::TIOCGPTN, &mut pty_num as *mut u32 as usize).is_ok() {
            let pts_path = alloc::format!("/dev/pts/{}\0", pty_num);
            let bytes = pts_path.as_bytes();
            if bytes.len() > buflen {
                return -34; // -ERANGE
            }
            let slice = unsafe { core::slice::from_raw_parts_mut(buf, bytes.len()) };
            slice.copy_from_slice(bytes);
            return 0;
        }
    }
    -9 // -EBADF
}

pub fn sys_unlockpt(fd: usize) -> i64 {
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
        let unlock: i32 = 0;
        if f.ioctl(crate::tty::pty::TIOCSPTLCK, &unlock as *const i32 as usize).is_ok() {
            return 0;
        }
    }
    -9 // -EBADF
}

pub fn sys_shm_open(name_ptr: *const u8, oflag: i32, _mode: u32) -> i64 {
    if name_ptr.is_null() {
        return -14; // -EFAULT
    }
    let mut len = 0;
    while unsafe { *name_ptr.add(len) } != 0 && len < 256 {
        len += 1;
    }
    let raw_name = match core::str::from_utf8(unsafe { core::slice::from_raw_parts(name_ptr, len) }) {
        Ok(s) => s,
        Err(_) => return -22, // -EINVAL
    };
    let clean_name = raw_name.trim_start_matches('/');

    let shm_obj = crate::mm::shm::SHM_REGISTRY.lock().open_or_create(clean_name);
    let file = Arc::new(crate::fs::vfs::File::new(shm_obj, oflag as u32));
    let current_pid = get_current_pid();
    if let Some(proc) = get_process(current_pid) {
        if let Some(fd) = proc.lock().alloc_fd(file) {
            return fd as i64;
        }
    }
    -24 // -EMFILE
}

pub fn sys_shm_unlink(name_ptr: *const u8) -> i64 {
    if name_ptr.is_null() {
        return -14; // -EFAULT
    }
    let mut len = 0;
    while unsafe { *name_ptr.add(len) } != 0 && len < 256 {
        len += 1;
    }
    let raw_name = match core::str::from_utf8(unsafe { core::slice::from_raw_parts(name_ptr, len) }) {
        Ok(s) => s,
        Err(_) => return -22, // -EINVAL
    };
    let clean_name = raw_name.trim_start_matches('/');
    if crate::mm::shm::SHM_REGISTRY.lock().unlink(clean_name) {
        0
    } else {
        -2 // -ENOENT
    }
}

pub fn sys_ftruncate(fd: usize, length: usize) -> i64 {
    let current_pid = get_current_pid();
    if let Some(proc) = get_process(current_pid) {
        let p = proc.lock();
        if let Some(file) = p.get_fd(fd) {
            let inode = file.inode.clone();
            drop(p);
            let mut node = inode.lock();
            let _ = node.truncate(length);
            return 0;
        }
    }
    -9 // -EBADF
}

pub fn sys_sem_open(name_ptr: *const u8, oflag: i32, mode: u32, value: u32) -> i64 {
    if name_ptr.is_null() {
        return -14; // -EFAULT
    }
    let mut len = 0;
    while unsafe { *name_ptr.add(len) } != 0 && len < 256 {
        len += 1;
    }
    let raw_name = match core::str::from_utf8(unsafe { core::slice::from_raw_parts(name_ptr, len) }) {
        Ok(s) => s,
        Err(_) => return -22, // -EINVAL
    };
    let clean_name = raw_name.trim_start_matches('/');

    let sem_obj = match crate::ipc::semaphore::SEMAPHORE_REGISTRY.lock().open_or_create(clean_name, oflag, mode, value) {
        Ok(s) => s,
        Err(_) => return -2, // -ENOENT or error
    };

    let file = Arc::new(crate::fs::vfs::File::new(sem_obj, oflag as u32));
    let current_pid = get_current_pid();
    let proc = match get_process(current_pid) {
        Some(p) => p,
        None => {
            let pcb = Arc::new(IrqSafeSpinlock::new(ProcessControlBlock::new(current_pid, 0, "proc")));
            PROCESS_TABLE.lock().push(pcb.clone());
            pcb
        }
    };

    let res = if let Some(fd) = proc.lock().alloc_fd(file) {
        fd as i64
    } else {
        -24 // -EMFILE
    };
    res
}

pub fn sys_sem_close(fd: usize) -> i64 {
    sys_close(fd)
}

pub fn sys_sem_post(fd: usize) -> i64 {
    let current_pid = get_current_pid();
    if let Some(proc) = get_process(current_pid) {
        let p = proc.lock();
        if let Some(file) = p.get_fd(fd) {
            let inode = file.inode.clone();
            drop(p);
            return match inode.lock().sem_post() {
                Ok(()) => 0,
                Err(e) => e as i64,
            };
        }
    }
    -9 // -EBADF
}

pub fn sys_sem_wait(fd: usize) -> i64 {
    let current_pid = get_current_pid();
    if let Some(proc) = get_process(current_pid) {
        let p = proc.lock();
        if let Some(file) = p.get_fd(fd) {
            let inode = file.inode.clone();
            drop(p);
            return match inode.lock().sem_wait() {
                Ok(()) => 0,
                Err(e) => e as i64,
            };
        }
    }
    -9 // -EBADF
}

pub fn sys_sem_trywait(fd: usize) -> i64 {
    let current_pid = get_current_pid();
    if let Some(proc) = get_process(current_pid) {
        let p = proc.lock();
        if let Some(file) = p.get_fd(fd) {
            let inode = file.inode.clone();
            drop(p);
            return match inode.lock().sem_trywait() {
                Ok(()) => 0,
                Err(e) => e as i64,
            };
        }
    }
    -9 // -EBADF
}

pub fn sys_sem_getvalue(fd: usize, sval_ptr: *mut i32) -> i64 {
    if sval_ptr.is_null() {
        return -14; // -EFAULT
    }
    let current_pid = get_current_pid();
    if let Some(proc) = get_process(current_pid) {
        let p = proc.lock();
        if let Some(file) = p.get_fd(fd) {
            let inode = file.inode.clone();
            drop(p);
            return match inode.lock().sem_getvalue() {
                Ok(val) => {
                    unsafe { *sval_ptr = val; }
                    0
                }
                Err(e) => e as i64,
            };
        }
    }
    -9 // -EBADF
}

pub fn sys_sem_unlink(name_ptr: *const u8) -> i64 {
    if name_ptr.is_null() {
        return -14; // -EFAULT
    }
    let mut len = 0;
    while unsafe { *name_ptr.add(len) } != 0 && len < 256 {
        len += 1;
    }
    let raw_name = match core::str::from_utf8(unsafe { core::slice::from_raw_parts(name_ptr, len) }) {
        Ok(s) => s,
        Err(_) => return -22, // -EINVAL
    };
    let clean_name = raw_name.trim_start_matches('/');
    if crate::ipc::semaphore::SEMAPHORE_REGISTRY.lock().unlink(clean_name) {
        0
    } else {
        -2 // -ENOENT
    }
}

pub fn sys_mq_open(name_ptr: *const u8, oflag: i32, attr_ptr: *const crate::ipc::mqueue::MqAttr) -> i64 {
    if name_ptr.is_null() {
        return -14; // -EFAULT
    }
    let mut len = 0;
    while unsafe { *name_ptr.add(len) } != 0 && len < 256 {
        len += 1;
    }
    let raw_name = match core::str::from_utf8(unsafe { core::slice::from_raw_parts(name_ptr, len) }) {
        Ok(s) => s,
        Err(_) => return -22, // -EINVAL
    };
    let clean_name = raw_name.trim_start_matches('/');
    let attr = if !attr_ptr.is_null() {
        Some(unsafe { *attr_ptr })
    } else {
        None
    };

    let mq_obj = match crate::ipc::mqueue::MQUEUE_REGISTRY.lock().open_or_create(clean_name, oflag, attr) {
        Ok(mq) => mq,
        Err(_) => return -2,
    };

    let file = Arc::new(crate::fs::vfs::File::new(mq_obj, oflag as u32));
    let current_pid = get_current_pid();
    let proc = match get_process(current_pid) {
        Some(p) => p,
        None => {
            let pcb = Arc::new(IrqSafeSpinlock::new(ProcessControlBlock::new(current_pid, 0, "proc")));
            PROCESS_TABLE.lock().push(pcb.clone());
            pcb
        }
    };

    let res = if let Some(fd) = proc.lock().alloc_fd(file) {
        fd as i64
    } else {
        -24 // -EMFILE
    };
    res
}

pub fn sys_mq_send(fd: usize, msg_ptr: *const u8, msg_len: usize, msg_prio: u32) -> i64 {
    if msg_ptr.is_null() && msg_len > 0 {
        return -14; // -EFAULT
    }
    let slice = if msg_len > 0 {
        unsafe { core::slice::from_raw_parts(msg_ptr, msg_len) }
    } else {
        &[]
    };

    let current_pid = get_current_pid();
    if let Some(proc) = get_process(current_pid) {
        let p = proc.lock();
        if let Some(file) = p.get_fd(fd) {
            let inode = file.inode.clone();
            drop(p);
            return match inode.lock().mq_send(slice, msg_prio, false) {
                Ok(()) => 0,
                Err(e) => e as i64,
            };
        }
    }
    -9 // -EBADF
}

pub fn sys_mq_receive(fd: usize, msg_ptr: *mut u8, msg_len: usize, msg_prio_ptr: *mut u32) -> i64 {
    if msg_ptr.is_null() || msg_len == 0 {
        return -14; // -EFAULT
    }
    let slice = unsafe { core::slice::from_raw_parts_mut(msg_ptr, msg_len) };

    let current_pid = get_current_pid();
    if let Some(proc) = get_process(current_pid) {
        let p = proc.lock();
        if let Some(file) = p.get_fd(fd) {
            let inode = file.inode.clone();
            drop(p);
            return match inode.lock().mq_receive(slice, msg_len, false) {
                Ok((bytes, prio)) => {
                    if !msg_prio_ptr.is_null() {
                        unsafe { *msg_prio_ptr = prio; }
                    }
                    bytes as i64
                }
                Err(e) => e as i64,
            };
        }
    }
    -9 // -EBADF
}

pub fn sys_mq_getattr(fd: usize, attr_ptr: *mut crate::ipc::mqueue::MqAttr) -> i64 {
    if attr_ptr.is_null() {
        return -14; // -EFAULT
    }
    let current_pid = get_current_pid();
    if let Some(proc) = get_process(current_pid) {
        let p = proc.lock();
        if let Some(file) = p.get_fd(fd) {
            let inode = file.inode.clone();
            drop(p);
            return match inode.lock().mq_getattr() {
                Ok(attr) => {
                    unsafe { *attr_ptr = attr; }
                    0
                }
                Err(e) => e as i64,
            };
        }
    }
    -9 // -EBADF
}

pub fn sys_mq_unlink(name_ptr: *const u8) -> i64 {
    if name_ptr.is_null() {
        return -14; // -EFAULT
    }
    let mut len = 0;
    while unsafe { *name_ptr.add(len) } != 0 && len < 256 {
        len += 1;
    }
    let raw_name = match core::str::from_utf8(unsafe { core::slice::from_raw_parts(name_ptr, len) }) {
        Ok(s) => s,
        Err(_) => return -22, // -EINVAL
    };
    let clean_name = raw_name.trim_start_matches('/');
    if crate::ipc::mqueue::MQUEUE_REGISTRY.lock().unlink(clean_name) {
        0
    } else {
        -2 // -ENOENT
    }
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

pub fn sys_dup(oldfd: usize) -> i64 {
    let current_pid = get_current_pid();
    if let Some(proc) = get_process(current_pid) {
        let mut p = proc.lock();
        if oldfd < p.fd_table.len() && p.fd_table[oldfd].is_none() && oldfd < 3 {
            if let Ok(file) = crate::fs::vfs::VFS.lock().open("/dev/console", if oldfd == 0 { 0 } else { 1 }) {
                p.fd_table[oldfd] = Some(file);
            }
        }
        if oldfd < p.fd_table.len() {
            if let Some(ref file) = p.fd_table[oldfd] {
                let f = file.clone();
                if let Some(newfd) = p.alloc_fd(f) {
                    return newfd as i64;
                } else {
                    return -24; // -EMFILE
                }
            }
        }
    }
    -9 // -EBADF
}

pub fn sys_dup2(oldfd: usize, newfd: usize) -> i64 {
    if newfd >= 256 {
        return -9; // -EBADF
    }
    let current_pid = get_current_pid();
    if oldfd == newfd {
        if let Some(proc) = get_process(current_pid) {
            let p = proc.lock();
            if oldfd < p.fd_table.len() && p.fd_table[oldfd].is_some() {
                return newfd as i64;
            }
        }
        return -9; // -EBADF
    }

    if let Some(proc) = get_process(current_pid) {
        let mut p = proc.lock();
        if oldfd < p.fd_table.len() && p.fd_table[oldfd].is_none() && oldfd < 3 {
            if let Ok(file) = crate::fs::vfs::VFS.lock().open("/dev/console", if oldfd == 0 { 0 } else { 1 }) {
                p.fd_table[oldfd] = Some(file);
            }
        }
        if oldfd < p.fd_table.len() {
            if let Some(ref file) = p.fd_table[oldfd] {
                let f = file.clone();
                while p.fd_table.len() <= newfd {
                    p.fd_table.push(None);
                }
                p.fd_table[newfd] = Some(f);
                return newfd as i64;
            }
        }
    }
    -9 // -EBADF
}

pub fn sys_pipe(pipefd_ptr: *mut i32) -> i64 {
    if pipefd_ptr.is_null() {
        return -14; // -EFAULT
    }
    let (r_inode, w_inode) = crate::ipc::pipe::create_pipe();
    let r_file = Arc::new(crate::fs::vfs::File::new(r_inode, 0));
    let w_file = Arc::new(crate::fs::vfs::File::new(w_inode, 1));

    let current_pid = get_current_pid();
    if let Some(proc) = get_process(current_pid) {
        let mut p = proc.lock();
        if let Some(fd0) = p.alloc_fd(r_file) {
            if let Some(fd1) = p.alloc_fd(w_file) {
                unsafe {
                    *pipefd_ptr = fd0 as i32;
                    *pipefd_ptr.add(1) = fd1 as i32;
                }
                return 0;
            } else {
                p.fd_table[fd0] = None;
                return -24; // -EMFILE
            }
        }
    }
    -24 // -EMFILE
}

pub fn sys_unlink(path_ptr: *const u8) -> i64 {
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
    let (parent_path, filename) = match path_str.rfind('/') {
        Some(pos) => {
            let p = if pos == 0 { "/" } else { &path_str[..pos] };
            let f = &path_str[pos + 1..];
            (p, f)
        }
        None => ("/", path_str),
    };
    let vfs = crate::fs::vfs::VFS.lock();
    let parent = match vfs.resolve_path(parent_path) {
        Ok(node) => node,
        Err(_) => return -2, // -ENOENT
    };
    drop(vfs);
    let res = parent.lock().unlink(filename);
    match res {
        Ok(()) => 0,
        Err(_) => -2, // -ENOENT
    }
}

pub fn sys_mkdir(path_ptr: *const u8, _mode: u32) -> i64 {
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
    let (parent_path, dir_name) = match path_str.rfind('/') {
        Some(pos) => {
            let p = if pos == 0 { "/" } else { &path_str[..pos] };
            let d = &path_str[pos + 1..];
            (p, d)
        }
        None => ("/", path_str),
    };
    if dir_name.is_empty() {
        return -17; // -EEXIST
    }
    let vfs = crate::fs::vfs::VFS.lock();
    let parent = match vfs.resolve_path(parent_path) {
        Ok(node) => node,
        Err(_) => return -2, // -ENOENT
    };
    drop(vfs);
    let res = parent.lock().create(dir_name, crate::fs::vfs::FileType::Directory);
    match res {
        Ok(_) => 0,
        Err(_) => -13, // -EACCES
    }
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
    length: usize,
    prot: i32,
    _flags: i32,
    fd: i32,
    _offset: u64,
) -> i64 {
    if length == 0 {
        return -22; // -EINVAL
    }

    let pages = (length + crate::mm::PAGE_SIZE - 1) / crate::mm::PAGE_SIZE;
    let current_pid = get_current_pid();

    let proc = match get_process(current_pid) {
        Some(p) => p,
        None => {
            let pcb = Arc::new(IrqSafeSpinlock::new(ProcessControlBlock::new(current_pid, 0, "proc")));
            PROCESS_TABLE.lock().push(pcb.clone());
            pcb
        }
    };

    let mut p = proc.lock();
    if p.address_space.is_none() {
        p.address_space = AddressSpace::new();
    }

    let target_addr = if addr != 0 { addr } else { 0x6000_0000 };

    let mut map_flags = crate::mm::vmm::PAGE_PRESENT | crate::mm::vmm::PAGE_USER;
    if (prot & crate::mm::shm::PROT_WRITE) != 0 || prot == 0 {
        map_flags |= crate::mm::vmm::PAGE_WRITABLE;
    }

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
            let phys = vbe.lfb_phys;
            drop(vbe);

            let fb_flags = map_flags | crate::mm::vmm::PAGE_WRITE_COMBINING;

            if phys != 0 {
                if let Some(ref mut aspace) = p.address_space {
                    if aspace.map_phys_region(
                        target_addr,
                        length as u64,
                        phys,
                        fb_flags,
                        crate::mm::address_space::VmaType::Mmap,
                    ).is_ok() {
                        return target_addr as i64;
                    }
                }
                return -12; // -ENOMEM
            } else {
                // Virtual RAM-backed fallback: map allocated RAM pages
                if let Some(ref mut aspace) = p.address_space {
                    if aspace.map_region(
                        target_addr,
                        length as u64,
                        map_flags,
                        crate::mm::address_space::VmaType::Mmap,
                    ).is_ok() {
                        return target_addr as i64;
                    }
                }
                return -12; // -ENOMEM
            }
        }

        // Check if file inode provides physical frames (e.g. POSIX shared memory ShmObject)
        let frames_opt = if fd_idx < p.fd_table.len() {
            if let Some(ref file) = p.fd_table[fd_idx] {
                file.inode.lock().get_physical_frames()
            } else {
                None
            }
        } else {
            None
        };

        if let Some(frames) = frames_opt {
            if let Some(ref mut aspace) = p.address_space {
                let vmm = crate::mm::vmm::VMM.lock();
                let pml4 = unsafe { &mut *(crate::mm::phys_to_virt(aspace.pml4_phys) as *mut crate::mm::vmm::PageTable) };
                for (i, &frame) in frames.iter().take(pages).enumerate() {
                    let virt = target_addr + (i * crate::mm::PAGE_SIZE) as u64;
                    let _ = vmm.map_page(pml4, virt, frame, map_flags);
                }
                aspace.vmas.push(crate::mm::address_space::VirtualMemoryArea {
                    start: target_addr,
                    end: target_addr + (pages * crate::mm::PAGE_SIZE) as u64,
                    flags: map_flags,
                    vma_type: crate::mm::address_space::VmaType::Mmap,
                });
                return target_addr as i64;
            }
            return -12; // -ENOMEM
        }
    }

    if let Some(ref mut aspace) = p.address_space {
        if aspace.map_region(
            target_addr,
            length as u64,
            map_flags,
            crate::mm::address_space::VmaType::Mmap,
        ).is_ok() {
            return target_addr as i64;
        }
    }

    -12 // -ENOMEM
}

pub fn sys_munmap(addr: u64, length: usize) -> i64 {
    if length == 0 || addr == 0 {
        return -22; // -EINVAL
    }
    let current_pid = get_current_pid();
    if let Some(proc) = get_process(current_pid) {
        let mut p = proc.lock();
        if let Some(ref mut aspace) = p.address_space {
            aspace.vmas.retain(|vma| vma.start != addr);
        }
    }
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

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SockAddrIn {
    pub sin_family: u16,   // AF_INET = 2
    pub sin_port: u16,     // Network byte order (big endian)
    pub sin_addr: [u8; 4], // IPv4 address
    pub sin_zero: [u8; 8],
}

pub enum GenericSocket {
    Inet(usize),
    Unix(usize),
}

fn get_sock_handle(fd: usize) -> Option<GenericSocket> {
    let current_pid = get_current_pid();
    let proc = get_process(current_pid)?;
    let p = proc.lock();
    if fd < p.fd_table.len() {
        if let Some(ref file) = p.fd_table[fd] {
            let stat = file.inode.lock().stat();
            if stat.file_type == crate::fs::vfs::FileType::Socket || stat.inode >= 5000 {
                return Some(GenericSocket::Unix((stat.inode - 5000) as usize));
            } else if stat.inode >= 200 {
                return Some(GenericSocket::Inet((stat.inode - 200) as usize));
            }
        }
    }
    None
}

pub fn sys_socket(domain: i32, sock_type: i32, _protocol: i32) -> i64 {
    if domain == 1 {
        // AF_UNIX / AF_LOCAL
        let st = if sock_type == 2 { crate::net::unix::SOCK_DGRAM } else { crate::net::unix::SOCK_STREAM };
        let sock_id = crate::net::unix::unix_socket_open(st);
        let inode = Arc::new(IrqSafeSpinlock::new(crate::net::unix::UnixSocketInode { sock_id }));
        let file = Arc::new(crate::fs::vfs::File::new(inode, crate::user::libc::O_RDWR));
        let current_pid = get_current_pid();
        if let Some(proc) = get_process(current_pid) {
            if let Some(fd) = proc.lock().alloc_fd(file) {
                return fd as i64;
            }
        }
        let _ = crate::net::unix::unix_socket_close(sock_id);
        -24
    } else if domain == 2 {
        // AF_INET
        let sock_id = match crate::net::socket::socket_open() {
            Ok(id) => id,
            Err(_) => return -24, // -EMFILE
        };
        let inode = Arc::new(IrqSafeSpinlock::new(crate::net::socket::SocketInode { sock_id }));
        let file = Arc::new(crate::fs::vfs::File::new(inode, crate::user::libc::O_RDWR));
        let current_pid = get_current_pid();
        if let Some(proc) = get_process(current_pid) {
            if let Some(fd) = proc.lock().alloc_fd(file) {
                return fd as i64;
            }
        }
        let _ = crate::net::socket::socket_close(sock_id);
        -24
    } else {
        -97 // -EAFNOSUPPORT
    }
}

pub fn sys_bind(fd: usize, sockaddr_ptr: *const u8, _addrlen: usize) -> i64 {
    if sockaddr_ptr.is_null() {
        return -14; // -EFAULT
    }
    let sock = match get_sock_handle(fd) {
        Some(s) => s,
        None => return -88, // -ENOTSOCK
    };
    match sock {
        GenericSocket::Unix(unix_id) => {
            let sun = unsafe { &*(sockaddr_ptr as *const crate::net::unix::SockAddrUn) };
            match crate::net::unix::unix_socket_bind(unix_id, sun.path_str()) {
                Ok(()) => 0,
                Err(_) => -98, // -EADDRINUSE
            }
        }
        GenericSocket::Inet(inet_id) => {
            let addr = unsafe { *(sockaddr_ptr as *const SockAddrIn) };
            let port = u16::from_be(addr.sin_port);
            match crate::net::socket::socket_bind(inet_id, addr.sin_addr, port) {
                Ok(()) => 0,
                Err(_) => -98,
            }
        }
    }
}

pub fn sys_listen(fd: usize, backlog: i32) -> i64 {
    let sock = match get_sock_handle(fd) {
        Some(s) => s,
        None => return -88, // -ENOTSOCK
    };
    let bl = if backlog > 0 { backlog as usize } else { 5 };
    match sock {
        GenericSocket::Unix(unix_id) => match crate::net::unix::unix_socket_listen(unix_id, bl) {
            Ok(()) => 0,
            Err(_) => -22,
        },
        GenericSocket::Inet(inet_id) => match crate::net::socket::socket_listen(inet_id, bl) {
            Ok(()) => 0,
            Err(_) => -22,
        },
    }
}

pub fn sys_accept(fd: usize, sockaddr_out: *mut u8, addrlen_out: *mut u32) -> i64 {
    let sock = match get_sock_handle(fd) {
        Some(s) => s,
        None => return -88, // -ENOTSOCK
    };
    match sock {
        GenericSocket::Unix(unix_id) => {
            match crate::net::unix::unix_socket_accept(unix_id) {
                Ok(child_id) => {
                    let inode = Arc::new(IrqSafeSpinlock::new(crate::net::unix::UnixSocketInode { sock_id: child_id }));
                    let file = Arc::new(crate::fs::vfs::File::new(inode, crate::user::libc::O_RDWR));
                    let current_pid = get_current_pid();
                    if let Some(proc) = get_process(current_pid) {
                        if let Some(client_fd) = proc.lock().alloc_fd(file) {
                            return client_fd as i64;
                        }
                    }
                    let _ = crate::net::unix::unix_socket_close(child_id);
                    -24
                }
                Err(_) => -11, // -EAGAIN
            }
        }
        GenericSocket::Inet(inet_id) => {
            match crate::net::socket::socket_accept(inet_id, 3000) {
                Ok((client_id, r_ip, r_port)) => {
                    if !sockaddr_out.is_null() {
                        let out = SockAddrIn {
                            sin_family: 2,
                            sin_port: r_port.to_be(),
                            sin_addr: r_ip,
                            sin_zero: [0; 8],
                        };
                        unsafe {
                            *(sockaddr_out as *mut SockAddrIn) = out;
                            if !addrlen_out.is_null() {
                                *addrlen_out = core::mem::size_of::<SockAddrIn>() as u32;
                            }
                        }
                    }

                    let inode = Arc::new(IrqSafeSpinlock::new(crate::net::socket::SocketInode { sock_id: client_id }));
                    let file = Arc::new(crate::fs::vfs::File::new(inode, crate::user::libc::O_RDWR));
                    let current_pid = get_current_pid();
                    if let Some(proc) = get_process(current_pid) {
                        if let Some(client_fd) = proc.lock().alloc_fd(file) {
                            return client_fd as i64;
                        }
                    }
                    let _ = crate::net::socket::socket_close(client_id);
                    -24 // -EMFILE
                }
                Err(_) => -11, // -EAGAIN / timed out
            }
        }
    }
}

pub fn sys_connect(fd: usize, sockaddr_ptr: *const u8, _addrlen: usize) -> i64 {
    if sockaddr_ptr.is_null() {
        return -14; // -EFAULT
    }
    let sock = match get_sock_handle(fd) {
        Some(s) => s,
        None => return -88, // -ENOTSOCK
    };
    match sock {
        GenericSocket::Unix(unix_id) => {
            let sun = unsafe { &*(sockaddr_ptr as *const crate::net::unix::SockAddrUn) };
            match crate::net::unix::unix_socket_connect(unix_id, sun.path_str()) {
                Ok(()) => 0,
                Err(_) => -111, // -ECONNREFUSED
            }
        }
        GenericSocket::Inet(inet_id) => {
            let addr = unsafe { *(sockaddr_ptr as *const SockAddrIn) };
            let port = u16::from_be(addr.sin_port);
            match crate::net::socket::socket_connect(inet_id, addr.sin_addr, port) {
                Ok(()) => 0,
                Err(_) => -111, // -ECONNREFUSED
            }
        }
    }
}

pub fn sys_sendto(fd: usize, buf: *const u8, len: usize, _flags: i32, _dest: *const u8, _addrlen: usize) -> i64 {
    if buf.is_null() {
        return -14; // -EFAULT
    }
    let sock = match get_sock_handle(fd) {
        Some(s) => s,
        None => return -88, // -ENOTSOCK
    };
    let slice = unsafe { core::slice::from_raw_parts(buf, len) };
    match sock {
        GenericSocket::Unix(unix_id) => match crate::net::unix::unix_socket_send(unix_id, slice) {
            Ok(n) => n as i64,
            Err(_) => -32, // -EPIPE
        },
        GenericSocket::Inet(inet_id) => match crate::net::socket::socket_send(inet_id, slice) {
            Ok(n) => n as i64,
            Err(_) => -32, // -EPIPE
        },
    }
}

pub fn sys_recvfrom(fd: usize, buf: *mut u8, len: usize, _flags: i32, _src: *mut u8, _addrlen: *mut u32) -> i64 {
    if buf.is_null() {
        return -14; // -EFAULT
    }
    let sock = match get_sock_handle(fd) {
        Some(s) => s,
        None => return -88, // -ENOTSOCK
    };
    let slice = unsafe { core::slice::from_raw_parts_mut(buf, len) };
    match sock {
        GenericSocket::Unix(unix_id) => match crate::net::unix::unix_socket_recv(unix_id, slice) {
            Ok(n) => n as i64,
            Err(_) => -11, // -EAGAIN
        },
        GenericSocket::Inet(inet_id) => match crate::net::socket::socket_recv(inet_id, slice, 2000) {
            Ok(n) => n as i64,
            Err(_) => -11, // -EAGAIN
        },
    }
}

pub const CLONE_VM: u64 = 0x00000100;
pub const CLONE_FS: u64 = 0x00000200;
pub const CLONE_FILES: u64 = 0x00000400;
pub const CLONE_SIGHAND: u64 = 0x00000800;
pub const CLONE_THREAD: u64 = 0x00010000;
pub const CLONE_SETTLS: u64 = 0x00080000;
pub const CLONE_PARENT_SETTID: u64 = 0x00100000;
pub const CLONE_CHILD_CLEARTID: u64 = 0x00200000;
pub const CLONE_CHILD_SETTID: u64 = 0x01000000;

/// POSIX / Linux sys_clone implementation for process forking and kernel-level POSIX threads.
pub fn sys_clone(flags: u64, _stack: *mut u8, ptid: *mut i32, ctid: *mut i32, _tls: u64) -> i64 {
    let parent_pid = get_current_pid();
    let parent_proc = match get_process(parent_pid) {
        Some(p) => p,
        None => return -3, // -ESRCH
    };

    let new_tid = NEXT_PID.fetch_add(1, Ordering::Relaxed);
    let mut child_pcb = ProcessControlBlock::new_with_space(new_tid, parent_pid, "thread", None);

    let mut p = parent_proc.lock();
    child_pcb.pgid = p.pgid;
    child_pcb.sid = p.sid;

    // CLONE_VM: share address space
    if (flags & CLONE_VM) != 0 {
        child_pcb.address_space = p.address_space.clone();
    } else if let Some(ref mut parent_space) = p.address_space {
        child_pcb.address_space = parent_space.clone_cow();
    }

    // CLONE_FILES: share file descriptor table
    if (flags & CLONE_FILES) != 0 {
        child_pcb.fd_table = p.fd_table.clone();
    }

    // CLONE_SIGHAND: share signal actions
    if (flags & CLONE_SIGHAND) != 0 {
        child_pcb.sigactions = p.sigactions;
        child_pcb.signal_state = p.signal_state.clone();
    }
    drop(p);

    if (flags & CLONE_VM) == 0 && child_pcb.address_space.is_none() {
        let mut space_guard = crate::task::user::CURRENT_USER_ADDRESS_SPACE.lock();
        if let Some(ref mut parent_space) = *space_guard {
            child_pcb.address_space = parent_space.clone_cow();
        }
    }

    if (flags & CLONE_PARENT_SETTID) != 0 && !ptid.is_null() {
        unsafe { *ptid = new_tid as i32; }
    }
    if (flags & CLONE_CHILD_SETTID) != 0 && !ctid.is_null() {
        unsafe { *ctid = new_tid as i32; }
    }

    PROCESS_TABLE.lock().push(Arc::new(IrqSafeSpinlock::new(child_pcb)));
    new_tid as i64
}
