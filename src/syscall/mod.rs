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
        SYS_UNAME => fs_calls::sys_uname(a1 as *mut fs_calls::UtsName) as i64,
        SYS_SCHED_YIELD => {
            crate::task::scheduler::yield_now();
            0
        }
        SYS_NANOSLEEP => {
            crate::task::scheduler::sleep_ms(a1);
            0
        }
        SYS_GETPID => 1,
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
