pub mod fs_calls;
pub mod table;

use crate::arch::x86_64::idt::InterruptContext;
use crate::syscall::table::*;
use crate::task::thread::SyscallFrame;

fn spawn_from_user_cmd(path_ptr: *const u8, wait: bool) -> i64 {
    if path_ptr.is_null() {
        return -14; // -EFAULT
    }
    let mut path_bytes = [0u8; 256];
    let mut len = 0;
    unsafe {
        while len < 255 {
            let b = *path_ptr.add(len);
            if b == 0 {
                break;
            }
            path_bytes[len] = b;
            len += 1;
        }
    }
    if let Ok(cmd_line) = core::str::from_utf8(&path_bytes[..len]) {
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
        let mut buf = alloc::vec![0u8; 131072];
        let n = match file.read(&mut buf) {
            Ok(n) => n,
            Err(_) => return -5, // -EIO
        };
        match crate::task::user::spawn_user_process(&buf[..n], &parts[..], wait) {
            Ok(res) => res as i64,
            Err(_) => -8, // -ENOEXEC
        }
    } else {
        -22 // -EINVAL
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
    match num {
        SYS_READ => fs_calls::sys_read(a1 as usize, a2 as *mut u8, a3 as usize) as i64,
        SYS_WRITE => fs_calls::sys_write(a1 as usize, a2 as *const u8, a3 as usize) as i64,
        SYS_OPEN => crate::task::process::sys_open(a1 as *const u8, a2 as i32, a3 as i32),
        SYS_CLOSE => crate::task::process::sys_close(a1 as usize),
        SYS_DUP => crate::task::process::sys_dup(a1 as usize),
        SYS_DUP2 => crate::task::process::sys_dup2(a1 as usize, a2 as usize),
        SYS_PIPE => crate::task::process::sys_pipe(a1 as *mut i32),
        SYS_MKDIR => crate::task::process::sys_mkdir(a1 as *const u8, a2 as u32),
        SYS_UNLINK => crate::task::process::sys_unlink(a1 as *const u8),
        SYS_IOCTL => crate::task::process::sys_ioctl(a1 as usize, a2 as u32, a3 as usize),
        SYS_MMAP => crate::task::process::sys_mmap(a1, a2 as usize, a3 as i32, _a4 as i32, _a5 as i32, _a6),
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
        SYS_EXECVE => crate::task::process::sys_execve(a1 as *const u8, a2 as *const *const u8, a3 as *const *const u8) as i64,
        SYS_REBOOT => {
            crate::arch::x86_64::acpi::poweroff();
        }
        SYS_SYSCOMMAND => {
            let cmd_ptr = a1 as *const u8;
            if cmd_ptr.is_null() {
                return -14;
            }
            let mut cmd_bytes = [0u8; 256];
            let mut len = 0;
            unsafe {
                while len < 255 {
                    let b = *cmd_ptr.add(len);
                    if b == 0 {
                        break;
                    }
                    cmd_bytes[len] = b;
                    len += 1;
                }
            }
            if let Ok(cmd_str) = core::str::from_utf8(&cmd_bytes[..len]) {
                crate::shell::commands::execute_command(cmd_str);
                0
            } else {
                -22
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
        SYS_EXIT => crate::task::user::handle_user_exit(a1 as i32),
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
