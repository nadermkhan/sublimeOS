pub mod fs_calls;
pub mod table;
pub mod uaccess;

use crate::arch::x86_64::idt::InterruptContext;
use crate::syscall::table::*;
use crate::task::thread::SyscallFrame;

#[no_mangle]
pub extern "C" fn syscall_bad_return_rip() -> ! {
    uaccess::exit_diverging_user_syscall();
    crate::kerror!("Security violation: non-canonical or kernel-range RCX on sysretq; terminating process");
    crate::task::user::handle_user_fault(139);
}

fn spawn_from_user_cmd(path_ptr: *const u8, wait: bool) -> i64 {
    let cmd_line = match uaccess::copy_user_cstr(path_ptr, 255) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let parts: alloc::vec::Vec<&str> = cmd_line.split_whitespace().collect();
    if parts.is_empty() {
        return -22; // -EINVAL
    }
    let raw_path = parts[0];
    let resolved_path = if raw_path.starts_with('/') {
        alloc::string::String::from(raw_path)
    } else {
        alloc::format!("/bin/{}", raw_path)
    };
    let vfs = crate::fs::vfs::VFS.lock();
    let file = match vfs.open(&resolved_path, 0) {
        Ok(f) => f,
        Err(_) => match vfs.open(raw_path, 0) {
            Ok(f) => f,
            Err(_) => return -2, // -ENOENT
        },
    };
    drop(vfs);
    let size_hint = {
        let st = file.inode.lock().stat();
        (st.size as usize).clamp(4096, 8 * 1024 * 1024)
    };
    let mut buf = alloc::vec::Vec::with_capacity(size_hint);
    let mut chunk = alloc::vec![0u8; 65536];
    loop {
        match file.read(&mut chunk) {
            Ok(0) => break,
            Ok(m) => {
                buf.extend_from_slice(&chunk[..m]);
                if buf.len() >= 8 * 1024 * 1024 {
                    break;
                }
            }
            Err(_) => return -5, // -EIO
        }
    }
    if buf.is_empty() {
        return -8; // -ENOEXEC
    }
    match crate::task::user::spawn_user_process(&buf, &parts[..], wait) {
        Ok(res) => res as i64,
        Err(_) => -8, // -ENOEXEC
    }
}

#[no_mangle]
pub extern "C" fn syscall_dispatcher(
    num: u64,
    a1: u64,
    a2: u64,
    a3: u64,
    _a4: u64,
    _a5: u64,
    _a6: u64,
    frame: *const SyscallFrame,
) -> i64 {
    let _user_scope = uaccess::UserSyscallScope::enter();
    match num {
        SYS_READ => fs_calls::sys_read(a1 as usize, a2 as *mut u8, a3 as usize) as i64,
        SYS_WRITE => fs_calls::sys_write(a1 as usize, a2 as *const u8, a3 as usize) as i64,
        SYS_OPEN => crate::task::process::sys_open(a1 as *const u8, a2 as i32, a3 as i32),
        SYS_CLOSE => crate::task::process::sys_close(a1 as usize),
        SYS_STAT => fs_calls::sys_stat(a1 as *const u8, a2 as *mut fs_calls::LinuxStat) as i64,
        SYS_FSTAT => fs_calls::sys_fstat(a1 as usize, a2 as *mut fs_calls::LinuxStat) as i64,
        SYS_LSEEK => fs_calls::sys_lseek(a1 as usize, a2 as isize, a3 as i32) as i64,
        SYS_DUP => crate::task::process::sys_dup(a1 as usize),
        SYS_DUP2 => crate::task::process::sys_dup2(a1 as usize, a2 as usize),
        SYS_PIPE => crate::task::process::sys_pipe(a1 as *mut i32),
        SYS_MKDIR => crate::task::process::sys_mkdir(a1 as *const u8, a2 as u32),
        SYS_UNLINK => crate::task::process::sys_unlink(a1 as *const u8),
        SYS_IOCTL => crate::task::process::sys_ioctl(a1 as usize, a2 as u32, a3 as usize),
        SYS_MMAP => crate::task::process::sys_mmap(a1, a2 as usize, a3 as i32, _a4 as i32, _a5 as i32, _a6),
        SYS_MPROTECT => crate::task::process::sys_mprotect(a1, a2 as usize, a3 as i32),
        SYS_MUNMAP => crate::task::process::sys_munmap(a1, a2 as usize) as i64,
        SYS_FSYNC => {
            let _ = crate::fs::bcache::BLOCK_CACHE.lock().flush_all();
            0
        }
        SYS_SYNC => {
            let _ = crate::fs::bcache::BLOCK_CACHE.lock().flush_all();
            0
        }
        SYS_FTRUNCATE => crate::task::process::sys_ftruncate(a1 as usize, a2 as usize),
        SYS_BRK => crate::task::user::sys_brk(a1) as i64,
        SYS_UNAME => fs_calls::sys_uname(a1 as *mut fs_calls::UtsName) as i64,
        SYS_SCHED_YIELD => {
            crate::task::scheduler::yield_now();
            0
        }
        SYS_NANOSLEEP => {
            crate::task::scheduler::sleep_ms(a1);
            0
        }
        SYS_GETPID => crate::task::process::get_current_pid() as i64,
        SYS_GETTIMEOFDAY => fs_calls::sys_gettimeofday(a1 as *mut fs_calls::TimeVal, a2 as *mut u8) as i64,
        SYS_SOCKET => crate::task::process::sys_socket(a1 as i32, a2 as i32, a3 as i32),
        SYS_CONNECT => crate::task::process::sys_connect(a1 as usize, a2 as *const u8, a3 as usize),
        SYS_ACCEPT => crate::task::process::sys_accept(a1 as usize, a2 as *mut u8, a3 as *mut u32),
        SYS_SENDTO => crate::task::process::sys_sendto(a1 as usize, a2 as *const u8, a3 as usize, _a4 as i32, _a5 as *const u8, _a6 as usize),
        SYS_RECVFROM => crate::task::process::sys_recvfrom(a1 as usize, a2 as *mut u8, a3 as usize, _a4 as i32, _a5 as *mut u8, _a6 as *mut u32),
        SYS_BIND => crate::task::process::sys_bind(a1 as usize, a2 as *const u8, a3 as usize),
        SYS_LISTEN => crate::task::process::sys_listen(a1 as usize, a2 as i32),
        SYS_POSIX_OPENPT => crate::task::process::sys_posix_openpt(a1 as i32),
        SYS_PTSNAME => crate::task::process::sys_ptsname(a1 as usize, a2 as *mut u8, a3 as usize),
        SYS_UNLOCKPT => crate::task::process::sys_unlockpt(a1 as usize),
        SYS_CLONE => crate::task::process::sys_clone(a1, a2 as *mut u8, a3 as *mut i32, _a4 as *mut i32, _a5),
        SYS_FUTEX => crate::ipc::futex::sys_futex(a1 as *mut u32, a2 as i32, a3 as u32, _a4 as *const u64, _a5 as *mut u32, _a6 as u32),
        SYS_FORK => {
            let frame_ref = if frame.is_null() {
                None
            } else {
                Some(unsafe { &*frame })
            };
            crate::task::process::sys_fork_with_frame(frame_ref) as i64
        }
        SYS_SPAWN => spawn_from_user_cmd(a1 as *const u8, true),
        SYS_SPAWN_ASYNC => spawn_from_user_cmd(a1 as *const u8, false),
        SYS_EXECVE => crate::task::process::sys_execve_with_frame(
            a1 as *const u8,
            a2 as *const *const u8,
            a3 as *const *const u8,
            frame as *mut SyscallFrame,
        ) as i64,
        SYS_REBOOT => {
            if !crate::task::process::is_current_privileged() {
                return uaccess::EPERM;
            }
            crate::arch::x86_64::acpi::poweroff();
        }
        SYS_SYSCOMMAND => {
            let cmd_str = match uaccess::copy_user_cstr(a1 as *const u8, 255) {
                Ok(s) => s,
                Err(e) => return e,
            };
            // Privilege boundary: arbitrary kernel shell execution from Ring 3 is forbidden.
            // Only PID 1 (or UID 0 root) may request the transitional compositor launch.
            let pid = crate::task::process::get_current_pid();
            if pid == 1 && cmd_str.trim() == "desktop" {
                crate::shell::commands::execute_command("desktop");
                0
            } else if crate::task::process::is_current_privileged()
                && matches!(cmd_str.trim(), "shutdown" | "reboot")
            {
                crate::shell::commands::execute_command(cmd_str.trim());
                0
            } else {
                uaccess::EPERM
            }
        }
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
        SYS_GETDENTS64 => crate::syscall::fs_calls::sys_getdents64(a1 as usize, a2 as *mut u8, a3 as usize) as i64,
        SYS_GETCWD => crate::task::process::sys_getcwd(a1 as *mut u8, a2 as usize),
        SYS_CHDIR => crate::task::process::sys_chdir(a1 as *const u8),
        SYS_SEM_OPEN => crate::task::process::sys_sem_open(a1 as *const u8, a2 as i32, a3 as u32, _a4 as u32),
        SYS_SEM_CLOSE => crate::task::process::sys_sem_close(a1 as usize),
        SYS_SEM_POST => crate::task::process::sys_sem_post(a1 as usize),
        SYS_SEM_WAIT => crate::task::process::sys_sem_wait(a1 as usize),
        SYS_SEM_TRYWAIT => crate::task::process::sys_sem_trywait(a1 as usize),
        SYS_SEM_GETVALUE => crate::task::process::sys_sem_getvalue(a1 as usize, a2 as *mut i32),
        SYS_SEM_UNLINK => crate::task::process::sys_sem_unlink(a1 as *const u8),
        SYS_MQ_OPEN => crate::task::process::sys_mq_open(a1 as *const u8, a2 as i32, a3 as *const crate::ipc::mqueue::MqAttr),
        SYS_MQ_SEND => crate::task::process::sys_mq_send(a1 as usize, a2 as *const u8, a3 as usize, _a4 as u32),
        SYS_MQ_RECEIVE => crate::task::process::sys_mq_receive(a1 as usize, a2 as *mut u8, a3 as usize, _a4 as *mut u32),
        SYS_MQ_GETATTR => crate::task::process::sys_mq_getattr(a1 as usize, a2 as *mut crate::ipc::mqueue::MqAttr),
        SYS_MQ_UNLINK => crate::task::process::sys_mq_unlink(a1 as *const u8),
        SYS_EXIT => {
            drop(_user_scope);
            crate::task::user::handle_user_exit(a1 as i32);
        }
        _ => -38, // -ENOSYS
    }
}

pub fn handle_syscall_int80(ctx: &mut InterruptContext) {
    let frame = SyscallFrame {
        r9: ctx.r9,
        r8: ctx.r8,
        r10: ctx.r10,
        rdx: ctx.rdx,
        rsi: ctx.rsi,
        rdi: ctx.rdi,
        r15: ctx.r15,
        r14: ctx.r14,
        r13: ctx.r13,
        r12: ctx.r12,
        rbx: ctx.rbx,
        rbp: ctx.rbp,
        rax: ctx.rax,
        rcx: ctx.rip,
        r11: ctx.rflags,
        user_rsp: ctx.rsp,
    };
    let ret = syscall_dispatcher(
        ctx.rax,
        ctx.rdi,
        ctx.rsi,
        ctx.rdx,
        ctx.r10,
        ctx.r8,
        ctx.r9,
        &frame as *const SyscallFrame,
    );
    ctx.rax = ret as u64;
}
