use crate::syscall::uaccess;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct UtsName {
    pub sysname: [u8; 65],
    pub nodename: [u8; 65],
    pub release: [u8; 65],
    pub version: [u8; 65],
    pub machine: [u8; 65],
    pub domainname: [u8; 65],
}

pub fn sys_write(fd: usize, buf: *const u8, count: usize) -> isize {
    if count == 0 {
        return 0;
    }
    let slice = match uaccess::copy_slice_from_user(buf, count) {
        Ok(v) => v,
        Err(e) => return e as isize,
    };

    // 1. Write to VFS file descriptor if mapped in process fd_table
    let current_pid = crate::task::process::get_current_pid();
    if let Some(proc) = crate::task::process::get_process(current_pid) {
        let p = proc.lock();
        if fd < p.fd_table.len() {
            if let Some(ref file) = p.fd_table[fd] {
                let f = file.clone();
                drop(p);
                return match f.write(&slice) {
                    Ok(n) => n as isize,
                    Err(_) => uaccess::EIO as isize,
                };
            }
        }
    }

    // 2. Default console fallback for standard output / standard error
    if fd == 1 || fd == 2 {
        if let Ok(s) = core::str::from_utf8(&slice) {
            crate::print!("{}", s);
        } else {
            for &b in &slice {
                crate::print!("{}", b as char);
            }
        }
        return slice.len() as isize;
    }

    uaccess::EBADF as isize
}

pub fn sys_read(fd: usize, buf: *mut u8, count: usize) -> isize {
    if count == 0 {
        return 0;
    }
    let capped = count.min(uaccess::MAX_USER_COPY);
    if let Err(e) = uaccess::validate_user_range(buf as u64, capped) {
        return e as isize;
    }

    // 1. Read from VFS file descriptor if mapped in process fd_table
    let current_pid = crate::task::process::get_current_pid();
    if let Some(proc) = crate::task::process::get_process(current_pid) {
        let p = proc.lock();
        if fd < p.fd_table.len() {
            if let Some(ref file) = p.fd_table[fd] {
                let f = file.clone();
                drop(p);
                let mut kbuf = alloc::vec![0u8; capped];
                return match f.read(&mut kbuf) {
                    Ok(n) => match uaccess::copy_slice_to_user(buf, &kbuf[..n]) {
                        Ok(()) => n as isize,
                        Err(e) => e as isize,
                    },
                    Err(_) => uaccess::EIO as isize,
                };
            }
        }
    }

    // 2. Default console fallback for standard input (stdin)
    if fd == 0 {
        let mut kbuf = alloc::vec![0u8; capped.min(4096)];
        let mut read_bytes = 0;
        loop {
            while read_bytes < kbuf.len() {
                if let Some(ch) = crate::shell::get_next_input_char() {
                    kbuf[read_bytes] = ch;
                    read_bytes += 1;
                } else {
                    break;
                }
            }
            if read_bytes > 0 {
                return match uaccess::copy_slice_to_user(buf, &kbuf[..read_bytes]) {
                    Ok(()) => read_bytes as isize,
                    Err(e) => e as isize,
                };
            }
            crate::task::scheduler::sleep_ms(5);
        }
    }

    uaccess::EBADF as isize
}

pub fn sys_uname(uts: *mut UtsName) -> isize {
    let mut k_uts = UtsName {
        sysname: [0; 65],
        nodename: [0; 65],
        release: [0; 65],
        version: [0; 65],
        machine: [0; 65],
        domainname: [0; 65],
    };
    copy_cstr(&mut k_uts.sysname, b"KiteKernel\0");
    copy_cstr(&mut k_uts.nodename, b"sublimeos\0");
    copy_cstr(&mut k_uts.release, b"1.0.0\0");
    copy_cstr(&mut k_uts.version, b"SublimeOS 1.0.0 (x86_64) by Nader Mahbub Khan (c) 2026\0");
    copy_cstr(&mut k_uts.machine, b"x86_64\0");
    copy_cstr(&mut k_uts.domainname, b"sublimeos.local\0");

    match uaccess::copy_to_user(uts, k_uts) {
        Ok(()) => 0,
        Err(e) => e as isize,
    }
}

fn copy_cstr(dst: &mut [u8; 65], src: &[u8]) {
    dst.fill(0);
    let len = src.len().min(64);
    dst[..len].copy_from_slice(&src[..len]);
}

/// Linux linux_dirent64 header for SYS_GETDENTS64 (217).
#[repr(C, packed)]
struct LinuxDirent64Hdr {
    d_ino: u64,
    d_off: i64,
    d_reclen: u16,
    d_type: u8,
}

pub fn sys_getdents64(fd: usize, dirp: *mut u8, count: usize) -> isize {
    if count < 32 {
        return uaccess::EINVAL as isize;
    }
    let capped = count.min(uaccess::MAX_USER_COPY);
    if let Err(e) = uaccess::validate_user_range(dirp as u64, capped) {
        return e as isize;
    }

    let current_pid = crate::task::process::get_current_pid();
    let file = if let Some(proc) = crate::task::process::get_process(current_pid) {
        let p = proc.lock();
        if fd < p.fd_table.len() {
            p.fd_table[fd].clone()
        } else {
            None
        }
    } else {
        None
    };

    let file = match file {
        Some(f) => f,
        None => return uaccess::EBADF as isize,
    };

    let entries = match file.inode.lock().readdir() {
        Ok(e) => e,
        Err(_) => return uaccess::ENOTDIR as isize,
    };

    let mut off = file.offset.lock();
    let cur_index = *off;
    if cur_index >= entries.len() {
        return 0; // EOF
    }

    let mut out_buf = alloc::vec::Vec::with_capacity(capped.min(8192));
    let mut i = cur_index;
    while i < entries.len() {
        let entry = &entries[i];
        let name_bytes = entry.name.as_bytes();
        let hdr_size = core::mem::size_of::<LinuxDirent64Hdr>(); // 19
        let rec_len = ((hdr_size + name_bytes.len() + 1 + 7) & !7).max(24);
        if out_buf.len() + rec_len > capped {
            if out_buf.is_empty() {
                return uaccess::EINVAL as isize;
            }
            break;
        }

        let d_type = match entry.file_type {
            crate::fs::vfs::FileType::Directory => 4,   // DT_DIR
            crate::fs::vfs::FileType::Regular => 8,     // DT_REG
            crate::fs::vfs::FileType::CharDevice => 2,  // DT_CHR
            crate::fs::vfs::FileType::BlockDevice => 6, // DT_BLK
            crate::fs::vfs::FileType::Pipe => 1,        // DT_FIFO
            crate::fs::vfs::FileType::Socket => 12,     // DT_SOCK
        };

        let hdr = LinuxDirent64Hdr {
            d_ino: entry.inode,
            d_off: (i + 1) as i64,
            d_reclen: rec_len as u16,
            d_type,
        };

        let hdr_bytes = unsafe {
            core::slice::from_raw_parts(&hdr as *const _ as *const u8, hdr_size)
        };
        out_buf.extend_from_slice(hdr_bytes);
        out_buf.extend_from_slice(name_bytes);
        let pad_len = rec_len - (hdr_size + name_bytes.len());
        out_buf.resize(out_buf.len() + pad_len, 0);

        i += 1;
    }

    if let Err(e) = uaccess::copy_slice_to_user(dirp, &out_buf) {
        return e as isize;
    }

    *off = i;
    out_buf.len() as isize
}

#[repr(C)]
#[derive(Clone, Copy)]
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
#[derive(Clone, Copy)]
pub struct TimeVal {
    pub tv_sec: i64,
    pub tv_usec: i64,
}

fn build_linux_stat(st: &crate::fs::vfs::Stat) -> LinuxStat {
    let type_bits: u32 = match st.file_type {
        crate::fs::vfs::FileType::Regular => 0o100000,
        crate::fs::vfs::FileType::Directory => 0o040000,
        crate::fs::vfs::FileType::CharDevice => 0o020000,
        crate::fs::vfs::FileType::BlockDevice => 0o060000,
        crate::fs::vfs::FileType::Pipe => 0o010000,
        crate::fs::vfs::FileType::Socket => 0o140000,
    };
    LinuxStat {
        st_dev: 1,
        st_ino: st.inode,
        st_nlink: 1,
        st_mode: type_bits | (st.mode & 0o7777),
        st_uid: 0,
        st_gid: 0,
        __pad0: 0,
        st_rdev: 0,
        st_size: st.size as i64,
        st_blksize: 4096,
        st_blocks: ((st.size + 511) / 512) as i64,
        st_atime: 0,
        st_atime_nsec: 0,
        st_mtime: 0,
        st_mtime_nsec: 0,
        st_ctime: 0,
        st_ctime_nsec: 0,
        __unused: [0; 3],
    }
}

fn normalize_abs_path(cwd: &str, raw: &str) -> alloc::string::String {
    let mut parts: alloc::vec::Vec<&str> = alloc::vec::Vec::new();
    let seed = if raw.starts_with('/') { "" } else { cwd };
    for seg in seed.split('/').chain(raw.split('/')) {
        match seg {
            "" | "." => {}
            ".." => {
                let _ = parts.pop();
            }
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        alloc::string::String::from("/")
    } else {
        let mut out = alloc::string::String::new();
        for p in parts {
            out.push('/');
            out.push_str(p);
        }
        out
    }
}

/// POSIX `stat(2)` syscall (SYS_STAT = 4).
pub fn sys_stat(path_ptr: *const u8, stat_ptr: *mut LinuxStat) -> isize {
    let raw_path = match uaccess::copy_user_cstr(path_ptr, 255) {
        Ok(s) => s,
        Err(e) => return e as isize,
    };
    let cwd = {
        let pid = crate::task::process::get_current_pid();
        crate::task::process::get_process(pid)
            .map(|p| p.lock().cwd.clone())
            .unwrap_or_else(|| alloc::string::String::from("/"))
    };
    let abs = normalize_abs_path(&cwd, &raw_path);
    let inode = {
        let vfs = crate::fs::vfs::VFS.lock();
        match vfs.resolve_path(&abs) {
            Ok(n) => n,
            Err(_) => return uaccess::ENOENT as isize,
        }
    };
    let st = inode.lock().stat();
    match uaccess::copy_to_user(stat_ptr, build_linux_stat(&st)) {
        Ok(()) => 0,
        Err(e) => e as isize,
    }
}

/// POSIX `fstat(2)` syscall (SYS_FSTAT = 5).
pub fn sys_fstat(fd: usize, stat_ptr: *mut LinuxStat) -> isize {
    let pid = crate::task::process::get_current_pid();
    let file = match crate::task::process::get_process(pid).and_then(|p| p.lock().get_fd(fd)) {
        Some(f) => f,
        None => return uaccess::EBADF as isize,
    };
    let st = file.stat();
    match uaccess::copy_to_user(stat_ptr, build_linux_stat(&st)) {
        Ok(()) => 0,
        Err(e) => e as isize,
    }
}

/// POSIX `lseek(2)` syscall (SYS_LSEEK = 8).
pub fn sys_lseek(fd: usize, offset: isize, whence: i32) -> isize {
    let w = match whence {
        0 => {
            if offset < 0 {
                return uaccess::EINVAL as isize;
            }
            crate::fs::vfs::SeekWhence::Start(offset as usize)
        }
        1 => crate::fs::vfs::SeekWhence::Current(offset),
        2 => crate::fs::vfs::SeekWhence::End(offset),
        _ => return uaccess::EINVAL as isize,
    };
    let pid = crate::task::process::get_current_pid();
    let file = match crate::task::process::get_process(pid).and_then(|p| p.lock().get_fd(fd)) {
        Some(f) => f,
        None => return uaccess::EBADF as isize,
    };
    match file.seek(w) {
        Ok(pos) => pos as isize,
        Err(_) => uaccess::EINVAL as isize,
    }
}

/// POSIX `gettimeofday(2)` syscall (SYS_GETTIMEOFDAY = 96).
pub fn sys_gettimeofday(tv_ptr: *mut TimeVal, tz_ptr: *mut u8) -> isize {
    if !tv_ptr.is_null() {
        let ticks = crate::arch::x86_64::pit::get_ticks();
        let tv = TimeVal {
            tv_sec: crate::arch::x86_64::rtc::get_system_time_unix() as i64,
            tv_usec: ((ticks % 1000) * 1000) as i64,
        };
        if let Err(e) = uaccess::copy_to_user(tv_ptr, tv) {
            return e as isize;
        }
    }
    if !tz_ptr.is_null() {
        if let Err(e) = uaccess::copy_to_user(tz_ptr as *mut [u8; 8], [0u8; 8]) {
            return e as isize;
        }
    }
    0
}
