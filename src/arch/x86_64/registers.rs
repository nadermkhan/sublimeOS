use core::arch::asm;

/// Read CR0.
#[inline(always)]
pub unsafe fn read_cr0() -> u64 {
    let cr0: u64;
    asm!("mov {}, cr0", out(reg) cr0, options(nomem, nostack, preserves_flags));
    cr0
}

/// Write CR0.
#[inline(always)]
pub unsafe fn write_cr0(val: u64) {
    asm!("mov cr0, {}", in(reg) val, options(nomem, nostack, preserves_flags));
}

/// Read CR2 (page fault linear address).
#[inline(always)]
pub unsafe fn read_cr2() -> u64 {
    let cr2: u64;
    asm!("mov {}, cr2", out(reg) cr2, options(nomem, nostack, preserves_flags));
    cr2
}

/// Read CR3 (page directory base register).
#[inline(always)]
pub unsafe fn read_cr3() -> u64 {
    let cr3: u64;
    asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags));
    cr3
}

/// Write CR3 (page directory base register).
#[inline(always)]
pub unsafe fn write_cr3(val: u64) {
    asm!("mov cr3, {}", in(reg) val, options(nomem, nostack, preserves_flags));
}

/// Read CR4.
#[inline(always)]
pub unsafe fn read_cr4() -> u64 {
    let cr4: u64;
    asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags));
    cr4
}

/// Write CR4.
#[inline(always)]
pub unsafe fn write_cr4(val: u64) {
    asm!("mov cr4, {}", in(reg) val, options(nomem, nostack, preserves_flags));
}

/// Read RFLAGS register.
#[inline(always)]
pub fn read_rflags() -> u64 {
    let rflags: u64;
    unsafe {
        asm!("pushfq", "pop {}", out(reg) rflags, options(nomem, preserves_flags));
    }
    rflags
}

/// Disable interrupts (cli).
#[inline(always)]
pub fn disable_interrupts() {
    unsafe {
        asm!("cli", options(nomem, nostack));
    }
}

/// Enable interrupts (sti).
#[inline(always)]
pub fn enable_interrupts() {
    unsafe {
        asm!("sti", options(nomem, nostack));
    }
}

/// Check if interrupts are currently enabled.
#[inline(always)]
pub fn are_interrupts_enabled() -> bool {
    (read_rflags() & (1 << 9)) != 0
}

/// Halt processor until next interrupt.
#[inline(always)]
pub fn hlt() {
    unsafe {
        asm!("hlt", options(nomem, nostack));
    }
}

/// Atomically enable interrupts and halt.
#[inline(always)]
pub fn enable_interrupts_and_hlt() {
    unsafe {
        asm!("sti", "hlt", options(nomem, nostack));
    }
}

/// Invalidate TLB for a specific virtual address.
#[inline(always)]
pub unsafe fn invlpg(addr: u64) {
    asm!("invlpg [{}]", in(reg) addr, options(nostack, preserves_flags));
}
