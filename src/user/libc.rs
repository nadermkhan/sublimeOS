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

/// POSIX exit() system call wrapper
pub fn exit(status: i32) -> ! {
    unsafe {
        syscall1(SYS_EXIT, status as u64);
    }
    loop {
        core::hint::spin_loop();
    }
}
