extern crate alloc;
use alloc::vec::Vec;
use crate::sync::IrqSafeSpinlock;

pub const FUTEX_WAIT: i32 = 0;
pub const FUTEX_WAKE: i32 = 1;
pub const FUTEX_FD: i32 = 2;
pub const FUTEX_REQUEUE: i32 = 3;
pub const FUTEX_CMP_REQUEUE: i32 = 4;
pub const FUTEX_WAKE_OP: i32 = 5;

pub const FUTEX_PRIVATE_FLAG: i32 = 128;
pub const FUTEX_CLOCK_REALTIME: i32 = 256;

#[derive(Debug, Clone, Copy)]
pub struct FutexWaiter {
    pub uaddr: usize,
    pub task_id: u64,
    pub woken: bool,
}

pub struct FutexRegistry {
    pub waiters: Vec<FutexWaiter>,
}

impl FutexRegistry {
    pub const fn new() -> Self {
        Self {
            waiters: Vec::new(),
        }
    }

    pub fn wait(&mut self, uaddr: usize, task_id: u64) {
        self.waiters.push(FutexWaiter {
            uaddr,
            task_id,
            woken: false,
        });
    }

    pub fn wake(&mut self, uaddr: usize, max_wake: usize) -> usize {
        let mut woken_count = 0;
        let mut i = 0;
        while i < self.waiters.len() && woken_count < max_wake {
            if self.waiters[i].uaddr == uaddr && !self.waiters[i].woken {
                self.waiters[i].woken = true;
                woken_count += 1;
                self.waiters.remove(i);
                continue;
            }
            i += 1;
        }
        woken_count
    }

    pub fn requeue(&mut self, uaddr1: usize, max_wake: usize, uaddr2: usize, max_requeue: usize) -> usize {
        let mut woken_count = 0;
        let mut requeued_count = 0;
        let mut i = 0;

        while i < self.waiters.len() {
            if self.waiters[i].uaddr == uaddr1 {
                if woken_count < max_wake {
                    self.waiters.remove(i);
                    woken_count += 1;
                    continue;
                } else if requeued_count < max_requeue {
                    self.waiters[i].uaddr = uaddr2;
                    requeued_count += 1;
                }
            }
            i += 1;
        }

        woken_count + requeued_count
    }
}

pub static FUTEX_REGISTRY: IrqSafeSpinlock<FutexRegistry> =
    IrqSafeSpinlock::new(FutexRegistry::new());

/// Query number of active waiters for a futex address.
pub fn waiter_count(uaddr: *mut u32) -> usize {
    let reg = FUTEX_REGISTRY.lock();
    let addr = uaddr as usize;
    reg.waiters.iter().filter(|w| w.uaddr == addr && !w.woken).count()
}

/// POSIX / Linux sys_futex implementation for fast userspace synchronization locks.
pub fn sys_futex(
    uaddr: *mut u32,
    op: i32,
    val: u32,
    _timeout: *const u64,
    uaddr2: *mut u32,
    _val3: u32,
) -> i64 {
    if uaddr.is_null() || (uaddr as usize & 3) != 0 {
        return -14; // -EFAULT / -EINVAL (misaligned address)
    }

    let cmd = op & !(FUTEX_PRIVATE_FLAG | FUTEX_CLOCK_REALTIME);
    let uaddr_val = uaddr as usize;

    match cmd {
        FUTEX_WAIT => {
            // Check if current value matches expected value
            let current_val = unsafe { core::ptr::read_volatile(uaddr) };
            if current_val != val {
                return -11; // -EAGAIN (value changed before waiting)
            }

            let current_pid = crate::task::process::get_current_pid();
            {
                let mut reg = FUTEX_REGISTRY.lock();
                reg.wait(uaddr_val, current_pid);
            }

            // Yield CPU slice to allow other threads to execute and unlock
            crate::task::scheduler::yield_now();
            0
        }
        FUTEX_WAKE => {
            let max_wake = if val > 0 { val as usize } else { 1 };
            let mut reg = FUTEX_REGISTRY.lock();
            let woken = reg.wake(uaddr_val, max_wake);
            woken as i64
        }
        FUTEX_REQUEUE => {
            if uaddr2.is_null() {
                return -14;
            }
            let max_wake = val as usize;
            let max_requeue = if _val3 > 0 { _val3 as usize } else { 1024 };
            let mut reg = FUTEX_REGISTRY.lock();
            let total = reg.requeue(uaddr_val, max_wake, uaddr2 as usize, max_requeue);
            total as i64
        }
        _ => -38, // -ENOSYS
    }
}
