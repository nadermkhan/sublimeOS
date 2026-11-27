#![no_std]

extern crate alloc;

use core::arch::naked_asm;

extern "C" {
    fn main() -> i32;
}

static mut ARGC: usize = 0;
static mut ARGV: *const *const u8 = core::ptr::null();
static mut ENVP: *const *const u8 = core::ptr::null();

#[no_mangle]
pub unsafe extern "C" fn cosmic_entry(argc: usize, argv: *const *const u8) -> i32 {
    ARGC = argc;
    ARGV = argv;
    ENVP = if !argv.is_null() {
        argv.add(argc + 1)
    } else {
        core::ptr::null()
    };
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

/// Retrieve all `KEY=VALUE` environment variables passed on initial process stack.
pub fn env() -> alloc::vec::Vec<&'static str> {
    let mut v = alloc::vec::Vec::new();
    unsafe {
        if ENVP.is_null() {
            return v;
        }
        for i in 0..128 {
            let ptr = *ENVP.add(i);
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

/// Look up an environment variable by key (returns slice after `KEY=`).
pub fn getenv(key: &str) -> Option<&'static str> {
    for entry in env() {
        if let Some((k, val)) = entry.split_once('=') {
            if k == key {
                return Some(val);
            }
        }
    }
    None
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

#[inline(always)]
pub unsafe fn syscall6(
    num: u64,
    a1: u64,
    a2: u64,
    a3: u64,
    a4: u64,
    a5: u64,
    a6: u64,
) -> i64 {
    let ret: i64;
    core::arch::asm!(
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

pub const ENAMETOOLONG: isize = -36;

fn copy_path_cstr(path: &str, out: &mut [u8; 256]) -> Result<(), isize> {
    let bytes = path.as_bytes();
    if bytes.len() > 255 {
        return Err(ENAMETOOLONG);
    }
    out[..bytes.len()].copy_from_slice(bytes);
    out[bytes.len()] = 0;
    Ok(())
}

pub fn read_dir(path: &str) -> Result<alloc::vec::Vec<DirEntry>, isize> {
    let mut path_buf = [0u8; 256];
    copy_path_cstr(path, &mut path_buf)?;

    let fd = unsafe { syscall3(2, path_buf.as_ptr() as u64, O_RDONLY as u64, 0) as isize };
    if fd < 0 {
        return Err(fd);
    }

    let mut entries = alloc::vec::Vec::new();
    let mut buf = [0u8; 1024];
    loop {
        let n = unsafe { syscall3(217, fd as u64, buf.as_mut_ptr() as u64, buf.len() as u64) as isize };
        if n < 0 {
            close(fd as usize);
            return Err(n);
        }
        if n == 0 {
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
    copy_path_cstr(path, &mut path_buf)?;

    let fd = unsafe { syscall3(2, path_buf.as_ptr() as u64, O_RDONLY as u64, 0) as isize };
    if fd < 0 {
        return Err(fd);
    }
    let mut res = alloc::string::String::new();
    let mut buf = [0u8; 512];
    loop {
        let n = read(fd as usize, &mut buf);
        if n < 0 {
            close(fd as usize);
            return Err(n);
        }
        if n == 0 {
            break;
        }
        if let Ok(s) = core::str::from_utf8(&buf[..n as usize]) {
            res.push_str(s);
        }
    }
    close(fd as usize);
    Ok(res)
}

/// Read up to `max_bytes` raw bytes from `path` (e.g. ELF64 headers or binary files).
pub fn read_bytes(path: &str, max_bytes: usize) -> Result<alloc::vec::Vec<u8>, isize> {
    let mut path_buf = [0u8; 256];
    copy_path_cstr(path, &mut path_buf)?;

    let fd = unsafe { syscall3(2, path_buf.as_ptr() as u64, O_RDONLY as u64, 0) as isize };
    if fd < 0 {
        return Err(fd);
    }
    let mut res = alloc::vec::Vec::new();
    let mut buf = [0u8; 512];
    while res.len() < max_bytes {
        let want = (max_bytes - res.len()).min(buf.len());
        let n = read(fd as usize, &mut buf[..want]);
        if n < 0 {
            close(fd as usize);
            return Err(n);
        }
        if n == 0 {
            break;
        }
        res.extend_from_slice(&buf[..n as usize]);
    }
    close(fd as usize);
    Ok(res)
}

pub fn write_file(path: &str, data: &[u8]) -> Result<(), isize> {
    let mut path_buf = [0u8; 256];
    copy_path_cstr(path, &mut path_buf)?;

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
    if let Err(e) = copy_path_cstr(path, &mut buf) {
        return e;
    }
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
    if let Err(e) = copy_path_cstr(path, &mut buf) {
        return e;
    }
    unsafe { syscall1(87, buf.as_ptr() as u64) as isize }
}

pub fn mkdir(path: &str, mode: u32) -> isize {
    let mut buf = [0u8; 256];
    if let Err(e) = copy_path_cstr(path, &mut buf) {
        return e;
    }
    unsafe { syscall2(83, buf.as_ptr() as u64, mode as u64) as isize }
}

pub fn try_getcwd() -> Result<alloc::string::String, isize> {
    let mut buf = [0u8; 256];
    let ret = unsafe { syscall2(79, buf.as_mut_ptr() as u64, buf.len() as u64) as isize };
    if ret < 0 {
        return Err(ret);
    }
    let mut len = 0;
    while len < buf.len() && buf[len] != 0 {
        len += 1;
    }
    core::str::from_utf8(&buf[..len])
        .map(alloc::string::String::from)
        .map_err(|_| -22)
}

pub fn getcwd() -> alloc::string::String {
    try_getcwd().unwrap_or_else(|_| alloc::string::String::from("/"))
}

pub fn chdir(path: &str) -> isize {
    let mut buf = [0u8; 256];
    if let Err(e) = copy_path_cstr(path, &mut buf) {
        return e;
    }
    unsafe { syscall1(80, buf.as_ptr() as u64) as isize }
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

pub fn kill(pid: usize, sig: i32) -> isize {
    unsafe { syscall2(62, pid as u64, sig as u64) as isize }
}

/// Replace the current process image with the ELF64 binary at `path` (POSIX `execve`, syscall 59).
pub fn execve(path: &str, argv: &[&str]) -> isize {
    let mut path_buf = [0u8; 256];
    if let Err(e) = copy_path_cstr(path, &mut path_buf) {
        return e;
    }

    let mut storage = alloc::vec::Vec::with_capacity(argv.len());
    let mut ptrs = alloc::vec::Vec::with_capacity(argv.len() + 1);
    for &arg in argv {
        let mut s = alloc::vec::Vec::with_capacity(arg.len() + 1);
        s.extend_from_slice(arg.as_bytes());
        s.push(0);
        ptrs.push(s.as_ptr());
        storage.push(s);
    }
    ptrs.push(core::ptr::null());

    unsafe { syscall3(59, path_buf.as_ptr() as u64, ptrs.as_ptr() as u64, 0) as isize }
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
    if let Err(e) = copy_path_cstr(path, &mut buf) {
        return e;
    }
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
    if let Err(e) = copy_path_cstr(cmd, &mut buf) {
        return e;
    }
    unsafe { syscall1(400, buf.as_ptr() as u64) as isize }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct LinuxStat {
    pub st_dev: u64,
    pub st_ino: u64,
    pub st_nlink: u64,
    pub st_mode: u32,
    pub st_uid: u32,
    pub st_gid: u32,
    pub __pad0: u32,
    pub st_rdev: u64,
    pub st_size: i64,
    pub st_blksize: i64,
    pub st_blocks: i64,
    pub st_atime: i64,
    pub st_atime_nsec: i64,
    pub st_mtime: i64,
    pub st_mtime_nsec: i64,
    pub st_ctime: i64,
    pub st_ctime_nsec: i64,
    pub __unused: [i64; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TimeVal {
    pub tv_sec: i64,
    pub tv_usec: i64,
}

pub const SEEK_SET: i32 = 0;
pub const SEEK_CUR: i32 = 1;
pub const SEEK_END: i32 = 2;
pub const PROT_NONE: i32 = 0x0;
pub const PROT_EXEC: i32 = 0x4;

pub fn lseek(fd: usize, offset: isize, whence: i32) -> isize {
    unsafe { syscall3(8, fd as u64, offset as u64, whence as u64) as isize }
}

pub fn stat(path: &str, out: &mut LinuxStat) -> isize {
    let mut buf = [0u8; 256];
    if let Err(e) = copy_path_cstr(path, &mut buf) {
        return e;
    }
    unsafe { syscall2(4, buf.as_ptr() as u64, out as *mut LinuxStat as u64) as isize }
}

pub fn fstat(fd: usize, out: &mut LinuxStat) -> isize {
    unsafe { syscall2(5, fd as u64, out as *mut LinuxStat as u64) as isize }
}

pub fn mprotect(addr: *mut u8, length: usize, prot: i32) -> isize {
    unsafe { syscall3(10, addr as u64, length as u64, prot as u64) as isize }
}

pub fn gettimeofday(tv: &mut TimeVal) -> isize {
    unsafe { syscall2(96, tv as *mut TimeVal as u64, 0) as isize }
}

pub fn shutdown() -> ! {
    unsafe {
        syscall1(169, 0);
        loop {
            core::arch::asm!("hlt");
        }
    }
}

pub const AF_UNIX: i32 = 1;
pub const SOCK_STREAM: i32 = 1;
pub const PROT_READ: i32 = 0x1;
pub const PROT_WRITE: i32 = 0x2;
pub const MAP_SHARED: i32 = 0x01;

#[repr(C)]
pub struct SockAddrUn {
    pub sun_family: u16,
    pub sun_path: [u8; 108],
}

pub fn socket(domain: i32, sock_type: i32, protocol: i32) -> isize {
    unsafe { syscall3(41, domain as u64, sock_type as u64, protocol as u64) as isize }
}

pub fn connect_unix(fd: usize, path: &str) -> isize {
    let bytes = path.as_bytes();
    if bytes.len() > 107 {
        return ENAMETOOLONG;
    }
    let mut addr = SockAddrUn {
        sun_family: AF_UNIX as u16,
        sun_path: [0u8; 108],
    };
    addr.sun_path[..bytes.len()].copy_from_slice(bytes);
    unsafe {
        syscall3(
            42,
            fd as u64,
            &addr as *const SockAddrUn as u64,
            core::mem::size_of::<SockAddrUn>() as u64,
        ) as isize
    }
}

pub fn sendto(fd: usize, buf: &[u8]) -> isize {
    unsafe { syscall6(44, fd as u64, buf.as_ptr() as u64, buf.len() as u64, 0, 0, 0) as isize }
}

pub fn recvfrom(fd: usize, buf: &mut [u8]) -> isize {
    unsafe {
        syscall6(
            45,
            fd as u64,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
            0,
            0,
            0,
        ) as isize
    }
}

pub fn ftruncate(fd: usize, length: usize) -> isize {
    unsafe { syscall2(77, fd as u64, length as u64) as isize }
}

pub fn mmap(addr: u64, length: usize, prot: i32, flags: i32, fd: i32, offset: u64) -> isize {
    unsafe {
        syscall6(
            9,
            addr,
            length as u64,
            prot as u64,
            flags as u64,
            fd as u64,
            offset,
        ) as isize
    }
}

pub fn munmap(addr: *mut u8, length: usize) -> isize {
    unsafe { syscall2(11, addr as u64, length as u64) as isize }
}

/// Real Ring-3 `libgui::ipc::WmOs` implementation backed by AF_UNIX + `/dev/shm/*` syscalls.
pub struct SublimeWmOs;

impl libgui::ipc::WmOs for SublimeWmOs {
    fn connect_unix(&mut self, path: &str) -> Result<usize, i32> {
        let fd = socket(AF_UNIX, SOCK_STREAM, 0);
        if fd < 0 {
            return Err(fd as i32);
        }
        let rc = connect_unix(fd as usize, path);
        if rc < 0 {
            close(fd as usize);
            return Err(rc as i32);
        }
        Ok(fd as usize)
    }

    fn send_all(&mut self, fd: usize, data: &[u8]) -> Result<usize, i32> {
        let mut sent = 0usize;
        while sent < data.len() {
            let n = sendto(fd, &data[sent..]);
            if n < 0 {
                return Err(n as i32);
            }
            if n == 0 {
                return Err(-32);
            }
            sent += n as usize;
        }
        Ok(sent)
    }

    fn recv_nonblocking(&mut self, fd: usize, buf: &mut [u8]) -> Result<usize, i32> {
        let n = recvfrom(fd, buf);
        if n < 0 {
            Err(n as i32)
        } else {
            Ok(n as usize)
        }
    }

    fn open_shm_rw_create(&mut self, name: &str) -> Result<usize, i32> {
        let mut path = alloc::string::String::from("/dev/shm/");
        path.push_str(name);
        let fd = open(&path, O_CREAT | O_RDWR, 0o666);
        if fd < 0 {
            Err(fd as i32)
        } else {
            Ok(fd as usize)
        }
    }

    fn ftruncate(&mut self, fd: usize, len: usize) -> Result<(), i32> {
        let rc = ftruncate(fd, len);
        if rc < 0 {
            Err(rc as i32)
        } else {
            Ok(())
        }
    }

    fn mmap_shared(&mut self, fd: usize, len: usize) -> Result<*mut u8, i32> {
        let res = mmap(0, len, PROT_READ | PROT_WRITE, MAP_SHARED, fd as i32, 0);
        if res < 0 {
            Err(res as i32)
        } else {
            Ok(res as usize as *mut u8)
        }
    }

    fn munmap(&mut self, ptr: *mut u8, len: usize) -> Result<(), i32> {
        let rc = munmap(ptr, len);
        if rc < 0 {
            Err(rc as i32)
        } else {
            Ok(())
        }
    }

    fn close(&mut self, fd: usize) {
        let _ = close(fd);
    }

    fn getpid(&mut self) -> u32 {
        getpid() as u32
    }

    fn sleep_ms(&mut self, ms: u64) {
        sleep_ms(ms);
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

// Userland Heap Allocator (backed by sys_brk with free-list reuse)
struct FreeNode {
    next: *mut FreeNode,
    cap: usize,
}

struct AllocState {
    lock: core::sync::atomic::AtomicBool,
    free_head: core::sync::atomic::AtomicUsize,
}

static ALLOC_STATE: AllocState = AllocState {
    lock: core::sync::atomic::AtomicBool::new(false),
    free_head: core::sync::atomic::AtomicUsize::new(0),
};

impl AllocState {
    fn lock_guard(&self) {
        while self
            .lock
            .compare_exchange(
                false,
                true,
                core::sync::atomic::Ordering::Acquire,
                core::sync::atomic::Ordering::Relaxed,
            )
            .is_err()
        {
            core::hint::spin_loop();
        }
    }

    fn unlock_guard(&self) {
        self.lock.store(false, core::sync::atomic::Ordering::Release);
    }
}

struct UserAllocator;

unsafe impl core::alloc::GlobalAlloc for UserAllocator {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        let node_align = core::mem::align_of::<FreeNode>();
        let align = layout.align().max(node_align);
        let raw_size = layout.size().max(core::mem::size_of::<FreeNode>());
        let size = match raw_size.checked_add(node_align - 1) {
            Some(s) => s & !(node_align - 1),
            None => return core::ptr::null_mut(),
        };
        ALLOC_STATE.lock_guard();

        let mut prev: *mut FreeNode = core::ptr::null_mut();
        let mut cur = ALLOC_STATE.free_head.load(core::sync::atomic::Ordering::Relaxed) as *mut FreeNode;
        while !cur.is_null() {
            if (cur as usize) & (align - 1) == 0 && (*cur).cap >= size {
                let mut next = (*cur).next;
                let rem = (*cur).cap - size;
                if rem >= core::mem::size_of::<FreeNode>() + node_align {
                    let tail = ((cur as usize) + size) as *mut FreeNode;
                    (*tail).cap = rem;
                    (*tail).next = next;
                    next = tail;
                }
                if prev.is_null() {
                    ALLOC_STATE
                        .free_head
                        .store(next as usize, core::sync::atomic::Ordering::Relaxed);
                } else {
                    (*prev).next = next;
                }
                ALLOC_STATE.unlock_guard();
                return cur as *mut u8;
            }
            prev = cur;
            cur = (*cur).next;
        }

        let cur_brk = brk(0);
        if cur_brk < 0x1000 {
            ALLOC_STATE.unlock_guard();
            return core::ptr::null_mut();
        }
        let aligned_brk = match cur_brk.checked_add(align - 1) {
            Some(v) => v & !(align - 1),
            None => {
                ALLOC_STATE.unlock_guard();
                return core::ptr::null_mut();
            }
        };
        let new_brk = match aligned_brk.checked_add(size) {
            Some(v) => v,
            None => {
                ALLOC_STATE.unlock_guard();
                return core::ptr::null_mut();
            }
        };
        let res = brk(new_brk);
        ALLOC_STATE.unlock_guard();
        if res >= new_brk {
            aligned_brk as *mut u8
        } else {
            core::ptr::null_mut()
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
        if ptr.is_null() || (ptr as usize) < 0x1000 {
            return;
        }
        let node_align = core::mem::align_of::<FreeNode>();
        if (ptr as usize) & (node_align - 1) != 0 {
            return;
        }
        let raw_cap = layout.size().max(core::mem::size_of::<FreeNode>());
        let cap = match raw_cap.checked_add(node_align - 1) {
            Some(s) => s & !(node_align - 1),
            None => return,
        };
        ALLOC_STATE.lock_guard();
        let node = ptr as *mut FreeNode;
        let head = ALLOC_STATE.free_head.load(core::sync::atomic::Ordering::Relaxed) as *mut FreeNode;
        (*node).next = head;
        (*node).cap = cap;
        ALLOC_STATE
            .free_head
            .store(node as usize, core::sync::atomic::Ordering::Relaxed);
        ALLOC_STATE.unlock_guard();
    }
}

#[global_allocator]
static ALLOCATOR: UserAllocator = UserAllocator;
