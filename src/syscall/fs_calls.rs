#[repr(C)]
pub struct UtsName {
    pub sysname: [u8; 65],
    pub nodename: [u8; 65],
    pub release: [u8; 65],
    pub version: [u8; 65],
    pub machine: [u8; 65],
    pub domainname: [u8; 65],
}

pub fn sys_write(fd: usize, buf: *const u8, count: usize) -> isize {
    if buf.is_null() || count == 0 {
        return 0;
    }

    let slice = unsafe { core::slice::from_raw_parts(buf, count) };

    // 1. Write to VFS file descriptor if mapped in process fd_table
    let current_pid = crate::task::process::get_current_pid();
    if let Some(proc) = crate::task::process::get_process(current_pid) {
        let p = proc.lock();
        if fd < p.fd_table.len() {
            if let Some(ref file) = p.fd_table[fd] {
                let f = file.clone();
                drop(p);
                if let Ok(n) = f.write(slice) {
                    return n as isize;
                }
            }
        }
    }

    // 2. Default console fallback for standard output / standard error
    if fd == 1 || fd == 2 {
        if let Ok(s) = core::str::from_utf8(slice) {
            crate::print!("{}", s);
        } else {
            for &b in slice {
                crate::print!("{}", b as char);
            }
        }
        return count as isize;
    }

    count as isize
}

pub fn sys_read(fd: usize, buf: *mut u8, count: usize) -> isize {
    if buf.is_null() || count == 0 {
        return 0;
    }

    // 1. Read from VFS file descriptor if mapped in process fd_table
    let current_pid = crate::task::process::get_current_pid();
    if let Some(proc) = crate::task::process::get_process(current_pid) {
        let p = proc.lock();
        if fd < p.fd_table.len() {
            if let Some(ref file) = p.fd_table[fd] {
                let f = file.clone();
                drop(p);
                let slice = unsafe { core::slice::from_raw_parts_mut(buf, count) };
                if let Ok(n) = f.read(slice) {
                    return n as isize;
                }
            }
        }
    }

    // 2. Default console fallback for standard input (stdin)
    if fd == 0 {
        let slice = unsafe { core::slice::from_raw_parts_mut(buf, count) };
        let mut read_bytes = 0;
        loop {
            while read_bytes < count {
                if let Some(ch) = crate::shell::get_next_input_char() {
                    slice[read_bytes] = ch;
                    read_bytes += 1;
                } else {
                    break;
                }
            }
            if read_bytes > 0 {
                return read_bytes as isize;
            }
            crate::task::scheduler::sleep_ms(5);
        }
    }

    0
}

pub fn sys_uname(uts: *mut UtsName) -> isize {
    if uts.is_null() {
        return -1;
    }

    unsafe {
        let u = &mut *uts;
        copy_cstr(&mut u.sysname, b"KiteKernel\0");
        copy_cstr(&mut u.nodename, b"sublimeos\0");
        copy_cstr(&mut u.release, b"1.0.0\0");
        copy_cstr(&mut u.version, b"SublimeOS 1.0.0 (x86_64) by Nader Mahbub Khan (c) 2026\0");
        copy_cstr(&mut u.machine, b"x86_64\0");
        copy_cstr(&mut u.domainname, b"sublimeos.local\0");
    }

    0
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
    if dirp.is_null() || count < 32 {
        return -22; // -EINVAL
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
        None => return -9, // -EBADF
    };

    let entries = match file.inode.lock().readdir() {
        Ok(e) => e,
        Err(_) => return -20, // -ENOTDIR
    };

    let mut off = file.offset.lock();
    let cur_index = *off;
    if cur_index >= entries.len() {
        return 0; // EOF
    }

    let mut written = 0usize;
    let mut i = cur_index;
    while i < entries.len() {
        let entry = &entries[i];
        let name_bytes = entry.name.as_bytes();
        let hdr_size = core::mem::size_of::<LinuxDirent64Hdr>(); // 19
        let rec_len = ((hdr_size + name_bytes.len() + 1 + 7) & !7).max(24);
        if written + rec_len > count {
            if written == 0 {
                return -22;
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

        unsafe {
            let dest = dirp.add(written);
            core::ptr::copy_nonoverlapping(&hdr as *const _ as *const u8, dest, hdr_size);
            core::ptr::copy_nonoverlapping(name_bytes.as_ptr(), dest.add(hdr_size), name_bytes.len());
            let pad_start = hdr_size + name_bytes.len();
            for p in pad_start..rec_len {
                *dest.add(p) = 0;
            }
        }

        written += rec_len;
        i += 1;
    }

    *off = i;
    written as isize
}
