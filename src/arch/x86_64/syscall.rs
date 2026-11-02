use crate::arch::x86_64::gdt::{KERNEL_CS, USER_CS};
use crate::arch::x86_64::msr::{rdmsr, wrmsr, IA32_EFER, IA32_FMASK, IA32_LSTAR, IA32_STAR};

extern "C" {
    fn syscall_entry();
}

/// Initialize x86_64 fast syscall (MSR STAR, LSTAR, FMASK, EFER.SCE).
pub fn init() {
    unsafe {
        // Enable System Call Extensions (SCE) in EFER (bit 0)
        let efer = rdmsr(IA32_EFER);
        wrmsr(IA32_EFER, efer | 1);

        // Configure STAR MSR:
        // Bits 47:32 -> Kernel CS (and SS = CS + 8)
        // Bits 63:48 -> User CS 32-bit (User CS 64-bit = selector + 16, User SS = selector + 8)
        // USER_CS is 0x20 | 3, USER_DS is 0x18 | 3
        let star: u64 = ((USER_CS as u64 - 16) << 48) | ((KERNEL_CS as u64) << 32);
        wrmsr(IA32_STAR, star);

        // Set LSTAR to syscall assembly entry point
        wrmsr(IA32_LSTAR, syscall_entry as *const () as usize as u64);

        // Set FMASK to clear IF (disable interrupts) and DF during syscall
        // 0x200 = IF, 0x100 = TF, 0x400 = DF
        wrmsr(IA32_FMASK, 0x700);
    }
}
