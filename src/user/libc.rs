extern crate alloc;
use core::arch::asm;
use crate::syscall::table::*;
use crate::syscall::fs_calls::UtsName;

// POSIX Open Flags
pub const O_RDONLY: u32 = 0x0000;
pub const O_WRONLY: u32 = 0x0001;
pub const O_RDWR: u32 = 0x0002;
pub const O_CREAT: u32 = 0x0040;
pub const O_TRUNC: u32 = 0x0200;
pub const O_APPEND: u32 = 0x0400;

/// Fast AMD64 Userspace System Call Gateway (num only)
#[inline(always)]
pub unsafe fn syscall0(num: u64) -> i64 {
    let ret: i64;
    asm!(
        "syscall",
        inlateout("rax") num => ret,
        out("rcx") _,
        out("r11") _,
        options(nostack, preserves_flags)
    );
    ret
}

/// Fast AMD64 Userspace System Call Gateway (1 argument)
#[inline(always)]
pub unsafe fn syscall1(num: u64, a1: u64) -> i64 {
    let ret: i64;
    asm!(
        "syscall",
        inlateout("rax") num => ret,
        in("rdi") a1,
        out("rcx") _,
        out("r11") _,
        options(nostack, preserves_flags)
    );
    ret
}

/// Fast AMD64 Userspace System Call Gateway (2 arguments)
#[inline(always)]
pub unsafe fn syscall2(num: u64, a1: u64, a2: u64) -> i64 {
    let ret: i64;
    asm!(
        "syscall",
        inlateout("rax") num => ret,
        in("rdi") a1,
        in("rsi") a2,
        out("rcx") _,
        out("r11") _,
        options(nostack, preserves_flags)
    );
    ret
}

/// Fast AMD64 Userspace System Call Gateway (3 arguments)
#[inline(always)]
pub unsafe fn syscall3(num: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    asm!(
        "syscall",
        inlateout("rax") num => ret,
        in("rdi") a1,
        in("rsi") a2,
        in("rdx") a3,
        out("rcx") _,
        out("r11") _,
        options(nostack, preserves_flags)
    );
    ret
}

/// Fast AMD64 Userspace System Call Gateway (4 arguments)
#[inline(always)]
pub unsafe fn syscall4(num: u64, a1: u64, a2: u64, a3: u64, a4: u64) -> i64 {
    let ret: i64;
    asm!(
        "syscall",
        inlateout("rax") num => ret,
        in("rdi") a1,
        in("rsi") a2,
        in("rdx") a3,
        in("r10") a4,
        out("rcx") _,
        out("r11") _,
        options(nostack, preserves_flags)
    );
    ret
}

/// Fast AMD64 Userspace System Call Gateway (5 arguments)
#[inline(always)]
pub unsafe fn syscall5(num: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) -> i64 {
    let ret: i64;
    asm!(
        "syscall",
        inlateout("rax") num => ret,
        in("rdi") a1,
        in("rsi") a2,
        in("rdx") a3,
        in("r10") a4,
        in("r8") a5,
        out("rcx") _,
        out("r11") _,
        options(nostack, preserves_flags)
    );
    ret
}

/// Fast AMD64 Userspace System Call Gateway (6 arguments)
#[inline(always)]
pub unsafe fn syscall6(num: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64) -> i64 {
    let ret: i64;
    asm!(
        "syscall",
        inlateout("rax") num => ret,
        in("rdi") a1,
        in("rsi") a2,
        in("rdx") a3,
        in("r10") a4,
        in("r8") a5,
        in("r9") a6,
        out("rcx") _,
        out("r11") _,
        options(nostack, preserves_flags)
    );
    ret
}

/// POSIX write() system call wrapper
pub fn write(fd: usize, buf: &[u8]) -> isize {
    unsafe {
        syscall3(SYS_WRITE, fd as u64, buf.as_ptr() as u64, buf.len() as u64) as isize
    }
}

/// POSIX read() system call wrapper
pub fn read(fd: usize, buf: &mut [u8]) -> isize {
    unsafe {
        syscall3(SYS_READ, fd as u64, buf.as_mut_ptr() as u64, buf.len() as u64) as isize
    }
}

/// POSIX puts() - write a string with trailing newline to stdout (fd 1)
pub fn puts(s: &str) -> isize {
    let w1 = write(1, s.as_bytes());
    let w2 = write(1, b"\n");
    if w1 >= 0 && w2 >= 0 {
        w1 + w2
    } else {
        -1
    }
}

/// POSIX putchar() - write a single character to stdout
pub fn putchar(c: u8) -> isize {
    write(1, &[c])
}

/// POSIX brk() - set end of the data segment
pub fn brk(addr: u64) -> u64 {
    unsafe {
        syscall1(SYS_BRK, addr) as u64
    }
}

/// POSIX sbrk() - change data segment size by increment
pub fn sbrk(increment: i64) -> *mut u8 {
    let current_brk = brk(0);
    if increment == 0 {
        return current_brk as *mut u8;
    }

    let new_brk = if increment > 0 {
        current_brk.saturating_add(increment as u64)
    } else {
        current_brk.saturating_sub((-increment) as u64)
    };

    let res = brk(new_brk);
    if res == current_brk && increment > 0 {
        core::ptr::null_mut()
    } else {
        current_brk as *mut u8
    }
}

/// Minimal Userspace Heap Allocator (malloc/free)
pub struct UserHeap {
    pub current_break: u64,
}

impl UserHeap {
    pub const fn new() -> Self {
        Self {
            current_break: 0x4000_0000,
        }
    }

    /// Allocate `size` bytes from the user heap via sbrk.
    pub fn malloc(&mut self, size: usize) -> *mut u8 {
        let aligned_size = (size + 15) & !15; // 16-byte align
        let ptr = sbrk(aligned_size as i64);
        ptr
    }

    /// Free allocated memory (no-op in basic bump allocator).
    pub fn free(&mut self, _ptr: *mut u8) {
        // Blocks can be coalesced in full allocator
    }
}

/// POSIX getpid() system call wrapper
pub fn getpid() -> u64 {
    unsafe {
        syscall0(SYS_GETPID) as u64
    }
}

/// POSIX fork() system call wrapper (Syscall 57)
pub fn fork() -> i32 {
    unsafe {
        syscall0(SYS_FORK) as i32
    }
}

/// POSIX waitpid() system call wrapper (Syscall 61)
pub fn waitpid(pid: i64, status: *mut i32, options: i32) -> i32 {
    unsafe {
        syscall3(SYS_WAIT4, pid as u64, status as u64, options as u64) as i32
    }
}

/// POSIX execve() system call wrapper (Syscall 59)
pub fn execve(path: *const u8, argv: *const *const u8, envp: *const *const u8) -> i32 {
    unsafe {
        syscall3(SYS_EXECVE, path as u64, argv as u64, envp as u64) as i32
    }
}

/// POSIX uname() system call wrapper
pub fn uname(uts: &mut UtsName) -> isize {
    unsafe {
        syscall1(SYS_UNAME, uts as *mut UtsName as u64) as isize
    }
}

/// POSIX nanosleep() system call wrapper
pub fn nanosleep(ms: u64) {
    unsafe {
        syscall1(SYS_NANOSLEEP, ms);
    }
}

/// POSIX kill() system call wrapper
pub fn kill(pid: i32, sig: i32) -> i32 {
    unsafe {
        syscall2(SYS_KILL, pid as u64, sig as u64) as i32
    }
}

/// POSIX sigaction() system call wrapper
pub fn sigaction(
    sig: i32,
    act: Option<&crate::ipc::signal::SigAction>,
    oact: Option<&mut crate::ipc::signal::SigAction>,
) -> i32 {
    let act_ptr = act.map(|a| a as *const crate::ipc::signal::SigAction).unwrap_or(core::ptr::null());
    let oact_ptr = oact.map(|o| o as *mut crate::ipc::signal::SigAction).unwrap_or(core::ptr::null_mut());
    unsafe {
        syscall3(SYS_RT_SIGACTION, sig as u64, act_ptr as u64, oact_ptr as u64) as i32
    }
}

/// POSIX sigprocmask() system call wrapper
pub fn sigprocmask(how: i32, set: Option<&u64>, oldset: Option<&mut u64>) -> i32 {
    let set_ptr = set.map(|s| s as *const u64).unwrap_or(core::ptr::null());
    let oldset_ptr = oldset.map(|o| o as *mut u64).unwrap_or(core::ptr::null_mut());
    unsafe {
        syscall3(SYS_RT_SIGPROCMASK, how as u64, set_ptr as u64, oldset_ptr as u64) as i32
    }
}

/// POSIX signal() convenience function
pub fn signal(sig: i32, handler: usize) -> usize {
    let new_act = crate::ipc::signal::SigAction::new(handler);
    let mut old_act = crate::ipc::signal::SigAction::default();
    if sigaction(sig, Some(&new_act), Some(&mut old_act)) == 0 {
        old_act.sa_handler
    } else {
        usize::MAX // SIG_ERR
    }
}

/// POSIX open() system call wrapper
pub fn open(path: &[u8], flags: u32, mode: u32) -> i32 {
    unsafe {
        syscall3(SYS_OPEN, path.as_ptr() as u64, flags as u64, mode as u64) as i32
    }
}

/// POSIX close() system call wrapper
pub fn close(fd: usize) -> i32 {
    unsafe {
        syscall1(SYS_CLOSE, fd as u64) as i32
    }
}

/// POSIX ioctl() system call wrapper
pub fn ioctl(fd: usize, cmd: u32, arg: usize) -> i32 {
    unsafe {
        syscall3(SYS_IOCTL, fd as u64, cmd as u64, arg as u64) as i32
    }
}

/// POSIX mmap() system call wrapper
pub fn mmap(addr: u64, length: u64, prot: u64, flags: u64, fd: i64, offset: u64) -> *mut u8 {
    unsafe {
        syscall6(SYS_MMAP, addr, length, prot, flags, fd as u64, offset) as *mut u8
    }
}

/// POSIX munmap() system call wrapper
pub fn munmap(addr: u64, length: u64) -> i32 {
    unsafe {
        syscall2(SYS_MUNMAP, addr, length) as i32
    }
}

/// POSIX sched_setaffinity() system call wrapper
pub fn sched_setaffinity(pid: i32, mask: u64) -> i32 {
    unsafe {
        syscall3(SYS_SCHED_SETAFFINITY, pid as u64, 8, &mask as *const u64 as u64) as i32
    }
}

/// POSIX sched_getaffinity() system call wrapper
pub fn sched_getaffinity(pid: i32, mask_ptr: *mut u64) -> i32 {
    unsafe {
        syscall3(SYS_SCHED_GETAFFINITY, pid as u64, 8, mask_ptr as u64) as i32
    }
}

pub use crate::task::process::SockAddrIn;

pub const AF_INET: i32 = 2;
pub const SOCK_STREAM: i32 = 1;

/// POSIX socket() system call wrapper
pub fn socket(domain: i32, sock_type: i32, protocol: i32) -> i32 {
    unsafe {
        syscall3(SYS_SOCKET, domain as u64, sock_type as u64, protocol as u64) as i32
    }
}

/// POSIX bind() system call wrapper
pub fn bind(fd: usize, addr: &SockAddrIn) -> i32 {
    unsafe {
        syscall3(SYS_BIND, fd as u64, addr as *const SockAddrIn as u64, core::mem::size_of::<SockAddrIn>() as u64) as i32
    }
}

/// POSIX listen() system call wrapper
pub fn listen(fd: usize, backlog: i32) -> i32 {
    unsafe {
        syscall2(SYS_LISTEN, fd as u64, backlog as u64) as i32
    }
}

/// POSIX accept() system call wrapper
pub fn accept(fd: usize, addr_out: Option<&mut SockAddrIn>) -> i32 {
    let (ptr, len_ptr) = if let Some(a) = addr_out {
        let mut len = core::mem::size_of::<SockAddrIn>() as u32;
        (a as *mut SockAddrIn as u64, &mut len as *mut u32 as u64)
    } else {
        (0, 0)
    };
    unsafe {
        syscall3(SYS_ACCEPT, fd as u64, ptr, len_ptr) as i32
    }
}

/// POSIX connect() system call wrapper
pub fn connect(fd: usize, addr: &SockAddrIn) -> i32 {
    unsafe {
        syscall3(SYS_CONNECT, fd as u64, addr as *const SockAddrIn as u64, core::mem::size_of::<SockAddrIn>() as u64) as i32
    }
}

/// POSIX send() system call wrapper
pub fn send(fd: usize, buf: &[u8], flags: i32) -> isize {
    unsafe {
        syscall6(SYS_SENDTO, fd as u64, buf.as_ptr() as u64, buf.len() as u64, flags as u64, 0, 0) as isize
    }
}

/// POSIX recv() system call wrapper
pub fn recv(fd: usize, buf: &mut [u8], flags: i32) -> isize {
    unsafe {
        syscall6(SYS_RECVFROM, fd as u64, buf.as_mut_ptr() as u64, buf.len() as u64, flags as u64, 0, 0) as isize
    }
}

/// POSIX posix_openpt() system call wrapper
pub fn posix_openpt(flags: i32) -> i32 {
    unsafe {
        syscall1(SYS_POSIX_OPENPT, flags as u64) as i32
    }
}

/// POSIX ptsname() system call wrapper
pub fn ptsname(fd: usize, buf: &mut [u8]) -> i32 {
    unsafe {
        syscall3(SYS_PTSNAME, fd as u64, buf.as_mut_ptr() as u64, buf.len() as u64) as i32
    }
}

/// POSIX unlockpt() system call wrapper
pub fn unlockpt(fd: usize) -> i32 {
    unsafe {
        syscall1(SYS_UNLOCKPT, fd as u64) as i32
    }
}

/// POSIX grantpt() wrapper (no-op on CosmicOS, permissions granted automatically)
pub fn grantpt(_fd: usize) -> i32 {
    0
}

/// POSIX setpgid() system call wrapper
pub fn setpgid(pid: i32, pgid: i32) -> i32 {
    unsafe {
        syscall2(SYS_SETPGID, pid as u64, pgid as u64) as i32
    }
}

/// POSIX getpgrp() system call wrapper
pub fn getpgrp() -> i32 {
    unsafe {
        syscall0(SYS_GETPGRP) as i32
    }
}

/// POSIX getpgid() system call wrapper
pub fn getpgid(pid: i32) -> i32 {
    unsafe {
        syscall1(SYS_GETPGID, pid as u64) as i32
    }
}

/// POSIX setsid() system call wrapper
pub fn setsid() -> i32 {
    unsafe {
        syscall0(SYS_SETSID) as i32
    }
}

/// POSIX getsid() system call wrapper
pub fn getsid(pid: i32) -> i32 {
    unsafe {
        syscall1(SYS_GETSID, pid as u64) as i32
    }
}

/// POSIX ftruncate() system call wrapper
pub fn ftruncate(fd: usize, length: usize) -> i32 {
    unsafe {
        syscall2(SYS_FTRUNCATE, fd as u64, length as u64) as i32
    }
}

/// POSIX shm_open() wrapper (opens /dev/shm/<name>)
pub fn shm_open(name: &str, oflag: i32, mode: u32) -> i32 {
    let mut path = alloc::string::String::from("/dev/shm/");
    path.push_str(name.trim_start_matches('/'));
    path.push('\0');
    open(path.as_bytes(), oflag as u32, mode)
}

/// POSIX shm_unlink() wrapper
pub fn shm_unlink(name: &str) -> i32 {
    let clean_name = name.trim_start_matches('/');
    if crate::mm::shm::SHM_REGISTRY.lock().unlink(clean_name) {
        0
    } else {
        -2 // -ENOENT
    }
}

/// POSIX sem_open() wrapper
pub fn sem_open(name: &str, oflag: i32, mode: u32, value: u32) -> i32 {
    let mut null_terminated = alloc::string::String::from(name);
    null_terminated.push('\0');
    unsafe {
        syscall4(SYS_SEM_OPEN, null_terminated.as_ptr() as u64, oflag as u64, mode as u64, value as u64) as i32
    }
}

/// POSIX sem_close() wrapper
pub fn sem_close(fd: usize) -> i32 {
    unsafe { syscall1(SYS_SEM_CLOSE, fd as u64) as i32 }
}

/// POSIX sem_post() wrapper
pub fn sem_post(fd: usize) -> i32 {
    unsafe { syscall1(SYS_SEM_POST, fd as u64) as i32 }
}

/// POSIX sem_wait() wrapper
pub fn sem_wait(fd: usize) -> i32 {
    unsafe { syscall1(SYS_SEM_WAIT, fd as u64) as i32 }
}

/// POSIX sem_trywait() wrapper
pub fn sem_trywait(fd: usize) -> i32 {
    unsafe { syscall1(SYS_SEM_TRYWAIT, fd as u64) as i32 }
}

/// POSIX sem_getvalue() wrapper
pub fn sem_getvalue(fd: usize, sval: &mut i32) -> i32 {
    unsafe { syscall2(SYS_SEM_GETVALUE, fd as u64, sval as *mut i32 as u64) as i32 }
}

/// POSIX sem_unlink() wrapper
pub fn sem_unlink(name: &str) -> i32 {
    let mut null_terminated = alloc::string::String::from(name);
    null_terminated.push('\0');
    unsafe { syscall1(SYS_SEM_UNLINK, null_terminated.as_ptr() as u64) as i32 }
}

/// POSIX mq_open() wrapper
pub fn mq_open(name: &str, oflag: i32, attr: Option<&crate::ipc::mqueue::MqAttr>) -> i32 {
    let mut null_terminated = alloc::string::String::from(name);
    null_terminated.push('\0');
    let attr_ptr = match attr {
        Some(a) => a as *const crate::ipc::mqueue::MqAttr as u64,
        None => 0,
    };
    unsafe {
        syscall3(SYS_MQ_OPEN, null_terminated.as_ptr() as u64, oflag as u64, attr_ptr) as i32
    }
}

/// POSIX mq_send() wrapper
pub fn mq_send(fd: usize, msg: &[u8], prio: u32) -> i32 {
    unsafe {
        syscall4(SYS_MQ_SEND, fd as u64, msg.as_ptr() as u64, msg.len() as u64, prio as u64) as i32
    }
}

/// POSIX mq_receive() wrapper
pub fn mq_receive(fd: usize, buf: &mut [u8], prio: Option<&mut u32>) -> isize {
    let prio_ptr = match prio {
        Some(p) => p as *mut u32 as u64,
        None => 0,
    };
    unsafe {
        syscall4(SYS_MQ_RECEIVE, fd as u64, buf.as_mut_ptr() as u64, buf.len() as u64, prio_ptr) as isize
    }
}

/// POSIX mq_getattr() wrapper
pub fn mq_getattr(fd: usize, attr: &mut crate::ipc::mqueue::MqAttr) -> i32 {
    unsafe {
        syscall2(SYS_MQ_GETATTR, fd as u64, attr as *mut crate::ipc::mqueue::MqAttr as u64) as i32
    }
}

/// POSIX mq_close() wrapper
pub fn mq_close(fd: usize) -> i32 {
    unsafe { syscall1(SYS_CLOSE, fd as u64) as i32 }
}

/// POSIX mq_unlink() wrapper
pub fn mq_unlink(name: &str) -> i32 {
    let mut null_terminated = alloc::string::String::from(name);
    null_terminated.push('\0');
    unsafe { syscall1(SYS_MQ_UNLINK, null_terminated.as_ptr() as u64) as i32 }
}

/// POSIX clone() system call wrapper
pub fn clone(flags: u64, stack: *mut u8, ptid: *mut i32, ctid: *mut i32, tls: u64) -> i64 {
    unsafe {
        syscall5(SYS_CLONE, flags, stack as u64, ptid as u64, ctid as u64, tls)
    }
}

/// Linux/POSIX futex() system call wrapper
pub fn futex(uaddr: *mut u32, futex_op: i32, val: u32, timeout: *const u64, uaddr2: *mut u32, val3: u32) -> i64 {
    unsafe {
        syscall6(SYS_FUTEX, uaddr as u64, futex_op as u64, val as u64, timeout as u64, uaddr2 as u64, val3 as u64)
    }
}

/// POSIX exit() system call wrapper
pub fn exit(status: i32) -> ! {
    unsafe {
        syscall1(SYS_EXIT, status as u64);
    }
    loop {
        core::hint::spin_loop();
    }
}
