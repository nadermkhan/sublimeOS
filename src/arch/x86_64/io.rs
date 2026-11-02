use core::arch::asm;
use core::marker::PhantomData;

/// Read a byte from an I/O port.
#[inline(always)]
pub unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    asm!("in al, dx", in("dx") port, out("al") value, options(nomem, nostack, preserves_flags));
    value
}

/// Write a byte to an I/O port.
#[inline(always)]
pub unsafe fn outb(port: u16, value: u8) {
    asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack, preserves_flags));
}

/// Read a 16-bit word from an I/O port.
#[inline(always)]
pub unsafe fn inw(port: u16) -> u16 {
    let value: u16;
    asm!("in ax, dx", in("dx") port, out("ax") value, options(nomem, nostack, preserves_flags));
    value
}

/// Write a 16-bit word to an I/O port.
#[inline(always)]
pub unsafe fn outw(port: u16, value: u16) {
    asm!("out dx, ax", in("dx") port, in("ax") value, options(nomem, nostack, preserves_flags));
}

/// Read a 32-bit double-word from an I/O port.
#[inline(always)]
pub unsafe fn inl(port: u16) -> u32 {
    let value: u32;
    asm!("in eax, dx", in("dx") port, out("eax") value, options(nomem, nostack, preserves_flags));
    value
}

/// Write a 32-bit double-word to an I/O port.
#[inline(always)]
pub unsafe fn outl(port: u16, value: u32) {
    asm!("out dx, eax", in("dx") port, in("eax") value, options(nomem, nostack, preserves_flags));
}

/// Wait a very small amount of time (1 I/O cycle) via port 0x80.
#[inline(always)]
pub fn io_wait() {
    unsafe {
        outb(0x80, 0);
    }
}

/// Type-safe hardware I/O Port wrapper.
#[derive(Debug, Clone, Copy)]
pub struct Port<T> {
    port: u16,
    _phantom: PhantomData<T>,
}

impl<T> Port<T> {
    pub const fn new(port: u16) -> Self {
        Self {
            port,
            _phantom: PhantomData,
        }
    }
}

impl Port<u8> {
    #[inline(always)]
    pub unsafe fn read(&self) -> u8 {
        inb(self.port)
    }

    #[inline(always)]
    pub unsafe fn write(&self, value: u8) {
        outb(self.port, value);
    }
}

impl Port<u16> {
    #[inline(always)]
    pub unsafe fn read(&self) -> u16 {
        inw(self.port)
    }

    #[inline(always)]
    pub unsafe fn write(&self, value: u16) {
        outw(self.port, value);
    }
}

impl Port<u32> {
    #[inline(always)]
    pub unsafe fn read(&self) -> u32 {
        inl(self.port)
    }

    #[inline(always)]
    pub unsafe fn write(&self, value: u32) {
        outl(self.port, value);
    }
}
