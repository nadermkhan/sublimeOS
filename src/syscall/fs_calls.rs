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

    // Standard output / Standard error
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

    // Write to VFS file descriptor if available
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

    count as isize
}

pub fn sys_read(fd: usize, buf: *mut u8, count: usize) -> isize {
    if buf.is_null() || count == 0 {
        return 0;
    }

    // Standard input (stdin)
    if fd == 0 {
        let slice = unsafe { core::slice::from_raw_parts_mut(buf, count) };
        if let Some(ch) = crate::shell::get_next_input_char() {
            slice[0] = ch;
            return 1;
        }
        return 0;
    }

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

    0
}

pub fn sys_uname(uts: *mut UtsName) -> isize {
    if uts.is_null() {
        return -1;
    }

    unsafe {
        let u = &mut *uts;
        copy_cstr(&mut u.sysname, b"CosmosKernel\0");
        copy_cstr(&mut u.nodename, b"cosmos\0");
        copy_cstr(&mut u.release, b"0.1.0\0");
        copy_cstr(&mut u.version, b"#1 Monolithic Rust 2026\0");
        copy_cstr(&mut u.machine, b"x86_64\0");
        copy_cstr(&mut u.domainname, b"(none)\0");
    }

    0
}

fn copy_cstr(dst: &mut [u8; 65], src: &[u8]) {
    dst.fill(0);
    let len = src.len().min(64);
    dst[..len].copy_from_slice(&src[..len]);
}
