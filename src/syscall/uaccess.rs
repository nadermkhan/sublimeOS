extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};
use crate::arch::x86_64::registers::SMAP_ENABLED;

/// Exclusive upper bound of canonical lower-half user virtual addresses on x86_64 (LA48).
pub const USER_MAX_ADDR: u64 = 0x0000_8000_0000_0000;
/// Maximum single bounce-buffer size for user<->kernel copies (prevents OOM panic DoS).
pub const MAX_USER_COPY: usize = 8 * 1024 * 1024;

pub const EPERM: i64 = -1;
pub const ENOENT: i64 = -2;
pub const ESRCH: i64 = -3;
pub const EIO: i64 = -5;
pub const ENOEXEC: i64 = -8;
pub const EBADF: i64 = -9;
pub const ECHILD: i64 = -10;
pub const EAGAIN: i64 = -11;
pub const ENOMEM: i64 = -12;
pub const EACCES: i64 = -13;
pub const EFAULT: i64 = -14;
pub const EEXIST: i64 = -17;
pub const ENOTDIR: i64 = -20;
pub const EINVAL: i64 = -22;
pub const EMFILE: i64 = -24;
pub const ERANGE: i64 = -34;
pub const ENAMETOOLONG: i64 = -36;
pub const ENOSYS: i64 = -38;

static ACTIVE_USER_SYSCALLS: AtomicUsize = AtomicUsize::new(0);

/// RAII guard marking that the current call entered through the Ring-3 syscall boundary.
pub struct UserSyscallScope;

impl UserSyscallScope {
    #[inline]
    pub fn enter() -> Self {
        ACTIVE_USER_SYSCALLS.fetch_add(1, Ordering::AcqRel);
        Self
    }
}

impl Drop for UserSyscallScope {
    #[inline]
    fn drop(&mut self) {
        let _ = ACTIVE_USER_SYSCALLS.try_update(Ordering::AcqRel, Ordering::Acquire, |v| {
            Some(v.saturating_sub(1))
        });
    }
}

/// Decrement `ACTIVE_USER_SYSCALLS` before a diverging exit path (`-> !`).
#[inline]
pub fn exit_diverging_user_syscall() {
    let _ = ACTIVE_USER_SYSCALLS.try_update(Ordering::AcqRel, Ordering::Acquire, |v| {
        Some(v.saturating_sub(1))
    });
}

/// Returns `true` when pointer arguments must be strictly validated as user-space addresses.
#[inline]
pub fn enforce_user_pointers() -> bool {
    let kcr3 = crate::task::scheduler::kernel_cr3();
    let cur_cr3 = unsafe { crate::arch::x86_64::registers::read_cr3() };
    !crate::arch::x86_64::pit::IN_KERNEL_TESTS.load(Ordering::Relaxed)
        || (kcr3 != 0 && cur_cr3 != kcr3)
        || ACTIVE_USER_SYSCALLS.load(Ordering::Acquire) > 0
        || crate::task::scheduler::is_current_user_thread()
}

/// RAII guard that temporarily enables supervisor access to user pages (`STAC` / `CLAC`)
/// when CPU SMAP is active.
pub struct SmapGuard {
    active: bool,
}

impl SmapGuard {
    #[inline(always)]
    pub fn new() -> Self {
        let active = SMAP_ENABLED.load(Ordering::Relaxed);
        if active {
            unsafe {
                core::arch::asm!("stac", options(nomem, nostack));
            }
        }
        Self { active }
    }
}

impl Drop for SmapGuard {
    #[inline(always)]
    fn drop(&mut self) {
        if self.active {
            unsafe {
                core::arch::asm!("clac", options(nomem, nostack));
            }
        }
    }
}

/// Validate that `[addr, addr + len)` is non-null, does not overflow, and resides strictly
/// in canonical lower-half user space when invoked from Ring 3 (or outside kernel self-tests).
#[inline]
pub fn validate_user_range(addr: u64, len: usize) -> Result<u64, i64> {
    if addr == 0 {
        return Err(EFAULT);
    }
    let end = addr.checked_add(len as u64).ok_or(EFAULT)?;
    if enforce_user_pointers() && (addr >= USER_MAX_ADDR || end > USER_MAX_ADDR) {
        return Err(EFAULT);
    }
    Ok(addr)
}

/// Copy `T` from validated user memory into kernel space.
pub fn copy_from_user<T: Copy>(ptr: *const T) -> Result<T, i64> {
    let addr = validate_user_range(ptr as u64, core::mem::size_of::<T>())?;
    let _smap = SmapGuard::new();
    Ok(unsafe { core::ptr::read_unaligned(addr as *const T) })
}

/// Copy `T` from kernel space into validated user memory.
pub fn copy_to_user<T: Copy>(ptr: *mut T, val: T) -> Result<(), i64> {
    let addr = validate_user_range(ptr as u64, core::mem::size_of::<T>())?;
    let _smap = SmapGuard::new();
    unsafe {
        core::ptr::write_unaligned(addr as *mut T, val);
    }
    Ok(())
}

/// Copy `len` bytes from validated user memory into an owned kernel `Vec<u8>`.
pub fn copy_slice_from_user(ptr: *const u8, len: usize) -> Result<Vec<u8>, i64> {
    if len == 0 {
        return Ok(Vec::new());
    }
    if len > MAX_USER_COPY {
        return Err(ENOMEM);
    }
    let addr = validate_user_range(ptr as u64, len)?;
    let _smap = SmapGuard::new();
    let mut out = alloc::vec![0u8; len];
    unsafe {
        core::ptr::copy_nonoverlapping(addr as *const u8, out.as_mut_ptr(), len);
    }
    Ok(out)
}

/// Copy kernel bytes into validated user memory.
pub fn copy_slice_to_user(ptr: *mut u8, src: &[u8]) -> Result<(), i64> {
    if src.is_empty() {
        return Ok(());
    }
    let addr = validate_user_range(ptr as u64, src.len())?;
    let _smap = SmapGuard::new();
    unsafe {
        core::ptr::copy_nonoverlapping(src.as_ptr(), addr as *mut u8, src.len());
    }
    Ok(())
}

/// Safely read a NUL-terminated UTF-8 string up to `max_len` bytes from user memory.
/// Returns `Err(EFAULT)` for invalid pointers and `Err(ENAMETOOLONG)` if no NUL terminator
/// appears within `max_len`.
pub fn copy_user_cstr(ptr: *const u8, max_len: usize) -> Result<String, i64> {
    if ptr.is_null() {
        return Err(EFAULT);
    }
    let base = ptr as u64;
    let strict = enforce_user_pointers();
    if strict && base >= USER_MAX_ADDR {
        return Err(EFAULT);
    }

    let _smap = SmapGuard::new();
    let mut buf = Vec::new();
    for i in 0..max_len {
        let cur = base.checked_add(i as u64).ok_or(EFAULT)?;
        if strict && cur >= USER_MAX_ADDR {
            return Err(EFAULT);
        }
        let b = unsafe { *(cur as *const u8) };
        if b == 0 {
            return String::from_utf8(buf).map_err(|_| EINVAL);
        }
        buf.push(b);
    }
    Err(ENAMETOOLONG)
}

/// Copy an array of NUL-terminated C strings from user memory (up to `limit` items).
pub fn copy_user_cstr_array(arr: *const *const u8, limit: usize) -> Result<Vec<String>, i64> {
    if arr.is_null() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for i in 0..limit {
        let elem_ptr = (arr as u64).checked_add((i * core::mem::size_of::<u64>()) as u64).ok_or(EFAULT)? as *const *const u8;
        let item_ptr = copy_from_user(elem_ptr)?;
        if item_ptr.is_null() {
            break;
        }
        out.push(copy_user_cstr(item_ptr, 512)?);
    }
    Ok(out)
}
