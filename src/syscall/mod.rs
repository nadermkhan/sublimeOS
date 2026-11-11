pub mod fs_calls;
pub mod table;

use crate::arch::x86_64::idt::InterruptContext;
use crate::syscall::table::*;

#[no_mangle]
pub extern "C" fn syscall_dispatcher(
    num: u64,
    a1: u64,
    a2: u64,
    a3: u64,
    _a4: u64,
    _a5: u64,
    _a6: u64,
) -> i64 {
    match num {
        SYS_READ => fs_calls::sys_read(a1 as usize, a2 as *mut u8, a3 as usize) as i64,
        SYS_WRITE => fs_calls::sys_write(a1 as usize, a2 as *const u8, a3 as usize) as i64,
        SYS_OPEN => crate::task::process::sys_open(a1 as *const u8, a2 as i32, a3 as i32),
        SYS_CLOSE => crate::task::process::sys_close(a1 as usize),
        SYS_IOCTL => crate::task::process::sys_ioctl(a1 as usize, a2 as u32, a3 as usize),
        SYS_MMAP => crate::task::process::sys_mmap(a1, a2 as usize, a3 as i32, _a4 as i32, _a5 as i32, _a6),
        SYS_MUNMAP => crate::task::process::sys_munmap(a1, a2 as usize) as i64,
        SYS_FTRUNCATE => crate::task::process::sys_ftruncate(a1 as usize, a2 as usize),
        SYS_BRK => crate::task::user::sys_brk(a1) as i64,
        SYS_UNAME => fs_calls::sys_uname(a1 as *mut fs_calls::UtsName) as i64,
        SYS_SCHED_YIELD => {
            crate::task::scheduler::yield_now();
            0
        }
        SYS_NANOSLEEP => {
            let was_enabled = crate::arch::x86_64::registers::are_interrupts_enabled();
            if !was_enabled {
                crate::arch::x86_64::registers::enable_interrupts();
            }
            crate::arch::x86_64::pit::sleep_ms(a1);
            if !was_enabled {
                crate::arch::x86_64::registers::disable_interrupts();
            }
            0
        }
        SYS_GETPID => crate::task::process::get_current_pid() as i64,
        SYS_SOCKET => crate::task::process::sys_socket(a1 as i32, a2 as i32, a3 as i32),
        SYS_CONNECT => crate::task::process::sys_connect(a1 as usize, a2 as *const crate::task::process::SockAddrIn, a3 as usize),
        SYS_ACCEPT => crate::task::process::sys_accept(a1 as usize, a2 as *mut crate::task::process::SockAddrIn, a3 as *mut u32),
        SYS_SENDTO => crate::task::process::sys_sendto(a1 as usize, a2 as *const u8, a3 as usize, _a4 as i32, _a5 as *const crate::task::process::SockAddrIn, _a6 as usize),
        SYS_RECVFROM => crate::task::process::sys_recvfrom(a1 as usize, a2 as *mut u8, a3 as usize, _a4 as i32, _a5 as *mut crate::task::process::SockAddrIn, _a6 as *mut u32),
        SYS_BIND => crate::task::process::sys_bind(a1 as usize, a2 as *const crate::task::process::SockAddrIn, a3 as usize),
        SYS_LISTEN => crate::task::process::sys_listen(a1 as usize, a2 as i32),
        SYS_POSIX_OPENPT => crate::task::process::sys_posix_openpt(a1 as i32),
        SYS_PTSNAME => crate::task::process::sys_ptsname(a1 as usize, a2 as *mut u8, a3 as usize),
        SYS_UNLOCKPT => crate::task::process::sys_unlockpt(a1 as usize),
        SYS_FORK => crate::task::process::sys_fork() as i64,
        SYS_EXECVE => crate::task::process::sys_execve(a1 as *const u8, a2 as *const *const u8, a3 as *const *const u8) as i64,
        SYS_WAIT4 => crate::task::process::sys_waitpid(a1 as i64, a2 as *mut i32, a3 as i32) as i64,
        SYS_KILL => crate::task::process::sys_kill(a1 as i32, a2 as i32) as i64,
        SYS_SETPGID => crate::task::process::sys_setpgid(a1 as i32, a2 as i32) as i64,
        SYS_GETPGRP => crate::task::process::sys_getpgrp() as i64,
        SYS_GETPGID => crate::task::process::sys_getpgid(a1 as i32) as i64,
        SYS_SETSID => crate::task::process::sys_setsid() as i64,
        SYS_GETSID => crate::task::process::sys_getsid(a1 as i32) as i64,
        SYS_RT_SIGACTION => crate::task::process::sys_rt_sigaction(a1 as i32, a2 as *const crate::ipc::signal::SigAction, a3 as *mut crate::ipc::signal::SigAction, _a4 as usize) as i64,
        SYS_RT_SIGPROCMASK => crate::task::process::sys_rt_sigprocmask(a1 as i32, a2 as *const u64, a3 as *mut u64, _a4 as usize) as i64,
        SYS_SCHED_SETAFFINITY => crate::task::process::sys_sched_setaffinity(a1 as i32, a2 as usize, a3 as *const u64) as i64,
        SYS_SCHED_GETAFFINITY => crate::task::process::sys_sched_getaffinity(a1 as i32, a2 as usize, a3 as *mut u64) as i64,
        SYS_EXIT => crate::task::user::handle_user_exit(a1 as i32),
        _ => -38, // -ENOSYS
    }
}

pub fn handle_syscall_int80(ctx: &mut InterruptContext) {
    let ret = syscall_dispatcher(
        ctx.rax,
        ctx.rdi,
        ctx.rsi,
        ctx.rdx,
        ctx.r10,
        ctx.r8,
        ctx.r9,
    );
    ctx.rax = ret as u64;
}
