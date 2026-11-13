#![no_std]

extern crate alloc;

use core::arch::naked_asm;

extern "C" {
    fn main() -> i32;
}

static mut ARGC: usize = 0;
static mut ARGV: *const *const u8 = core::ptr::null();

#[no_mangle]
pub unsafe extern "C" fn cosmic_entry(argc: usize, argv: *const *const u8) -> i32 {
    ARGC = argc;
    ARGV = argv;
    main()
}

// Entry Point
#[unsafe(naked)]
#[no_mangle]
#[link_section = ".text._start"]
pub unsafe extern "C" fn _start() -> ! {
    naked_asm!(
        "xor rbp, rbp",      // Clear frame pointer per AMD64 ABI
        "pop rdi",           // argc (pops 8 bytes; rsp becomes 16-byte aligned)
        "mov rsi, rsp",      // argv pointer
        "call cosmic_entry", // Call runtime entry(argc, argv) -> main() -> i32
        "mov edi, eax",      // Exit code into rdi
        "mov eax, 60",       // SYS_EXIT
        "syscall",
        "2:",
        "hlt",
        "jmp 2b",
    );
}

/// Retrieve command-line arguments passed from shell / parent process.
pub fn args() -> alloc::vec::Vec<&'static str> {
    let mut v = alloc::vec::Vec::new();
    unsafe {
        for i in 0..ARGC {
            let ptr = *ARGV.add(i);
            if ptr.is_null() {
                break;
            }
            let mut len = 0;
            while *ptr.add(len) != 0 {
                len += 1;
            }
            if let Ok(s) = core::str::from_utf8(core::slice::from_raw_parts(ptr, len)) {
                v.push(s);
            }
        }
    }
    v
}

// Low-level Syscall Primitives
#[inline(always)]
pub unsafe fn syscall0(num: u64) -> i64 {
    let ret: i64;
    core::arch::asm!(
        "syscall",
        inlateout("rax") num => ret,
        out("rcx") _,
        out("r11") _,
        options(nostack, preserves_flags)
    );
    ret
}

#[inline(always)]
pub unsafe fn syscall1(num: u64, a1: u64) -> i64 {
    let ret: i64;
    core::arch::asm!(
        "syscall",
        inlateout("rax") num => ret,
        in("rdi") a1,
        out("rcx") _,
        out("r11") _,
        options(nostack, preserves_flags)
    );
    ret
}

#[inline(always)]
pub unsafe fn syscall2(num: u64, a1: u64, a2: u64) -> i64 {
    let ret: i64;
    core::arch::asm!(
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

#[inline(always)]
pub unsafe fn syscall3(num: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    core::arch::asm!(
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

pub const O_RDONLY: i32 = 0;
pub const O_WRONLY: i32 = 1;
pub const O_RDWR: i32 = 2;
pub const O_CREAT: i32 = 0x40;
pub const O_TRUNC: i32 = 0x200;
pub const O_APPEND: i32 = 0x400;

#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: alloc::string::String,
    pub is_dir: bool,
}

pub fn read_dir(path: &str) -> Result<alloc::vec::Vec<DirEntry>, isize> {
    let mut path_buf = [0u8; 256];
    let bytes = path.as_bytes();
    let len = bytes.len().min(255);
    path_buf[..len].copy_from_slice(&bytes[..len]);
    path_buf[len] = 0;

    let fd = unsafe { syscall3(2, path_buf.as_ptr() as u64, O_RDONLY as u64, 0) as isize };
    if fd < 0 {
        return Err(fd);
    }

    let mut entries = alloc::vec::Vec::new();
    let mut buf = [0u8; 1024];
    loop {
        let n = unsafe { syscall3(217, fd as u64, buf.as_mut_ptr() as u64, buf.len() as u64) as isize };
        if n <= 0 {
            break;
        }
        let mut off = 0usize;
        while off + 19 <= n as usize {
            let reclen = u16::from_ne_bytes([buf[off + 16], buf[off + 17]]) as usize;
            if reclen == 0 || off + reclen > n as usize {
                break;
            }
            let d_type = buf[off + 18];
            let name_start = off + 19;
            let mut name_end = name_start;
            while name_end < off + reclen && buf[name_end] != 0 {
                name_end += 1;
            }
            if let Ok(s) = core::str::from_utf8(&buf[name_start..name_end]) {
                if s != "." && s != ".." {
                    entries.push(DirEntry {
                        name: alloc::string::String::from(s),
                        is_dir: d_type == 4,
                    });
                }
            }
            off += reclen;
        }
    }
    close(fd as usize);
    Ok(entries)
}

pub fn read_to_string(path: &str) -> Result<alloc::string::String, isize> {
    let mut path_buf = [0u8; 256];
    let bytes = path.as_bytes();
    let len = bytes.len().min(255);
    path_buf[..len].copy_from_slice(&bytes[..len]);
    path_buf[len] = 0;

    let fd = unsafe { syscall3(2, path_buf.as_ptr() as u64, O_RDONLY as u64, 0) as isize };
    if fd < 0 {
        return Err(fd);
    }
    let mut res = alloc::string::String::new();
    let mut buf = [0u8; 512];
    loop {
        let n = read(fd as usize, &mut buf);
        if n <= 0 {
            break;
        }
        if let Ok(s) = core::str::from_utf8(&buf[..n as usize]) {
            res.push_str(s);
        }
    }
    close(fd as usize);
    Ok(res)
}

pub fn write_file(path: &str, data: &[u8]) -> Result<(), isize> {
    let mut path_buf = [0u8; 256];
    let bytes = path.as_bytes();
    let len = bytes.len().min(255);
    path_buf[..len].copy_from_slice(&bytes[..len]);
    path_buf[len] = 0;

    let flags = O_WRONLY | O_CREAT | O_TRUNC;
    let fd = unsafe { syscall3(2, path_buf.as_ptr() as u64, flags as u64, 0o644) as isize };
    if fd < 0 {
        return Err(fd);
    }
    let mut written = 0usize;
    while written < data.len() {
        let n = write(fd as usize, &data[written..]);
        if n <= 0 {
            close(fd as usize);
            return Err(n);
        }
        written += n as usize;
    }
    close(fd as usize);
    Ok(())
}

// POSIX System Call Wrappers
pub fn exit(code: i32) -> ! {
    unsafe {
        syscall1(60, code as u64);
        loop {
            core::arch::asm!("hlt");
        }
    }
}

pub fn write(fd: usize, buf: &[u8]) -> isize {
    unsafe { syscall3(1, fd as u64, buf.as_ptr() as u64, buf.len() as u64) as isize }
}

pub fn read(fd: usize, buf: &mut [u8]) -> isize {
    unsafe { syscall3(0, fd as u64, buf.as_mut_ptr() as u64, buf.len() as u64) as isize }
}

pub fn open(path: &str, flags: i32, mode: i32) -> isize {
    let mut buf = [0u8; 256];
    let bytes = path.as_bytes();
    let len = bytes.len().min(255);
    buf[..len].copy_from_slice(&bytes[..len]);
    buf[len] = 0;
    unsafe { syscall3(2, buf.as_ptr() as u64, flags as u64, mode as u64) as isize }
}

pub fn close(fd: usize) -> isize {
    unsafe { syscall1(3, fd as u64) as isize }
}

pub fn dup(oldfd: usize) -> isize {
    unsafe { syscall1(32, oldfd as u64) as isize }
}

pub fn dup2(oldfd: usize, newfd: usize) -> isize {
    unsafe { syscall2(33, oldfd as u64, newfd as u64) as isize }
}

pub fn pipe(fds: &mut [i32; 2]) -> isize {
    unsafe { syscall1(22, fds.as_mut_ptr() as u64) as isize }
}

pub fn unlink(path: &str) -> isize {
    let mut buf = [0u8; 256];
    let bytes = path.as_bytes();
    let len = bytes.len().min(255);
    buf[..len].copy_from_slice(&bytes[..len]);
    buf[len] = 0;
    unsafe { syscall1(87, buf.as_ptr() as u64) as isize }
}

pub fn mkdir(path: &str, mode: u32) -> isize {
    let mut buf = [0u8; 256];
    let bytes = path.as_bytes();
    let len = bytes.len().min(255);
    buf[..len].copy_from_slice(&bytes[..len]);
    buf[len] = 0;
    unsafe { syscall2(83, buf.as_ptr() as u64, mode as u64) as isize }
}

pub fn getpid() -> usize {
    unsafe { syscall0(39) as usize }
}

pub fn fork() -> isize {
    unsafe { syscall0(57) as isize }
}

pub fn waitpid(pid: isize, status: &mut i32, options: i32) -> isize {
    unsafe { syscall3(61, pid as u64, status as *mut i32 as u64, options as u64) as isize }
}

pub fn sched_yield() {
    unsafe {
        syscall0(24);
    }
}

pub fn sleep_ms(ms: u64) {
    unsafe {
        syscall1(35, ms);
    }
}

pub fn brk(addr: usize) -> usize {
    unsafe { syscall1(12, addr as u64) as usize }
}

pub fn sync() {
    unsafe {
        syscall0(162);
    }
}

fn spawn_syscall(sysno: u64, path: &str) -> isize {
    let mut buf = [0u8; 256];
    let bytes = path.as_bytes();
    let len = bytes.len().min(255);
    buf[..len].copy_from_slice(&bytes[..len]);
    buf[len] = 0;
    unsafe { syscall1(sysno, buf.as_ptr() as u64) as isize }
}

pub fn spawn(path: &str) -> isize {
    spawn_syscall(58, path)
}

pub fn spawn_bg(path: &str) -> isize {
    spawn_syscall(401, path)
}

pub fn syscommand(cmd: &str) -> isize {
    let mut buf = [0u8; 256];
    let bytes = cmd.as_bytes();
    let len = bytes.len().min(255);
    buf[..len].copy_from_slice(&bytes[..len]);
    buf[len] = 0;
    unsafe { syscall1(400, buf.as_ptr() as u64) as isize }
}

pub fn shutdown() -> ! {
    unsafe {
        syscall1(169, 0);
        loop {
            core::arch::asm!("hlt");
        }
    }
}

// I/O & Formatting
pub struct Stdout;

impl core::fmt::Write for Stdout {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        write(1, s.as_bytes());
        Ok(())
    }
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        let mut stdout = $crate::Stdout;
        let _ = core::fmt::write(&mut stdout, format_args!($($arg)*));
    };
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => {{
        $crate::print!($($arg)*);
        $crate::print!("\n");
    }};
}

// Panic Handler for Userland Binaries
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    print!("[USER PANIC] ");
    if let Some(loc) = info.location() {
        print!("{}:{}:{} ", loc.file(), loc.line(), loc.column());
    }
    println!("{}", info.message());
    exit(1);
}

// Userland Heap Allocator (backed by sys_brk)
struct UserAllocator;

unsafe impl core::alloc::GlobalAlloc for UserAllocator {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        let align = layout.align();
        let size = layout.size();
        let cur_brk = brk(0);
        if cur_brk == 0 {
            return core::ptr::null_mut();
        }
        let aligned_brk = (cur_brk + align - 1) & !(align - 1);
        let new_brk = aligned_brk + size;
        let res = brk(new_brk);
        if res >= new_brk {
            aligned_brk as *mut u8
        } else {
            core::ptr::null_mut()
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: core::alloc::Layout) {
        // Bump allocator: no-op dealloc for simple userland processes
    }
}

#[global_allocator]
static ALLOCATOR: UserAllocator = UserAllocator;
