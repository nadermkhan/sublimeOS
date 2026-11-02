use core::cell::UnsafeCell;
use core::hint::spin_loop;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};
use crate::arch::x86_64::registers::{are_interrupts_enabled, disable_interrupts, enable_interrupts};

/// A basic Spinlock using atomic test-and-set.
pub struct Spinlock<T: ?Sized> {
    lock: AtomicBool,
    data: UnsafeCell<T>,
}

unsafe impl<T: ?Sized + Send> Sync for Spinlock<T> {}
unsafe impl<T: ?Sized + Send> Send for Spinlock<T> {}

impl<T> Spinlock<T> {
    pub const fn new(data: T) -> Self {
        Self {
            lock: AtomicBool::new(false),
            data: UnsafeCell::new(data),
        }
    }
}

impl<T: ?Sized> Spinlock<T> {
    pub fn lock(&self) -> SpinlockGuard<'_, T> {
        while self.lock.swap(true, Ordering::Acquire) {
            while self.lock.load(Ordering::Relaxed) {
                spin_loop();
            }
        }
        SpinlockGuard {
            lock: &self.lock,
            data: unsafe { &mut *self.data.get() },
        }
    }

    pub fn try_lock(&self) -> Option<SpinlockGuard<'_, T>> {
        if !self.lock.swap(true, Ordering::Acquire) {
            Some(SpinlockGuard {
                lock: &self.lock,
                data: unsafe { &mut *self.data.get() },
            })
        } else {
            None
        }
    }
}

pub struct SpinlockGuard<'a, T: ?Sized> {
    lock: &'a AtomicBool,
    data: &'a mut T,
}

impl<T: ?Sized> Deref for SpinlockGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self.data
    }
}

impl<T: ?Sized> DerefMut for SpinlockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.data
    }
}

impl<T: ?Sized> Drop for SpinlockGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.store(false, Ordering::Release);
    }
}

/// An IRQ-safe Spinlock that disables interrupts while holding the lock
/// and restores the previous interrupt flag state when released.
pub struct IrqSafeSpinlock<T: ?Sized> {
    lock: AtomicBool,
    data: UnsafeCell<T>,
}

unsafe impl<T: ?Sized + Send> Sync for IrqSafeSpinlock<T> {}
unsafe impl<T: ?Sized + Send> Send for IrqSafeSpinlock<T> {}

impl<T> IrqSafeSpinlock<T> {
    pub const fn new(data: T) -> Self {
        Self {
            lock: AtomicBool::new(false),
            data: UnsafeCell::new(data),
        }
    }
}

impl<T: ?Sized> IrqSafeSpinlock<T> {
    pub fn lock(&self) -> IrqSafeSpinlockGuard<'_, T> {
        let prev_irq = are_interrupts_enabled();
        disable_interrupts();

        while self.lock.swap(true, Ordering::Acquire) {
            while self.lock.load(Ordering::Relaxed) {
                spin_loop();
            }
        }

        IrqSafeSpinlockGuard {
            lock: &self.lock,
            data: unsafe { &mut *self.data.get() },
            prev_irq,
        }
    }
}

pub struct IrqSafeSpinlockGuard<'a, T: ?Sized> {
    lock: &'a AtomicBool,
    data: &'a mut T,
    prev_irq: bool,
}

impl<T: ?Sized> Deref for IrqSafeSpinlockGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self.data
    }
}

impl<T: ?Sized> DerefMut for IrqSafeSpinlockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.data
    }
}

impl<T: ?Sized> Drop for IrqSafeSpinlockGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.store(false, Ordering::Release);
        if self.prev_irq {
            enable_interrupts();
        }
    }
}
