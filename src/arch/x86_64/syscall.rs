use crate::arch::x86_64::gdt::{KERNEL_CS, USER_CS};
use crate::arch::x86_64::msr::{
    rdmsr, wrmsr, IA32_EFER, IA32_FMASK, IA32_GS_BASE, IA32_KERNEL_GSBASE, IA32_LSTAR, IA32_STAR,
};

extern "C" {
    fn syscall_entry();
}

/// Per-CPU scratch context pointed to by `IA32_KERNEL_GSBASE` for `swapgs` stack switching.
/// Field offsets are relied upon directly by `src/syscall/syscall_entry.s`:
/// - offset 0: `kernel_rsp`
/// - offset 8: `user_rsp`
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CpuSyscallContext {
    pub kernel_rsp: u64,
    pub user_rsp: u64,
}

impl CpuSyscallContext {
    pub const fn new() -> Self {
        Self {
            kernel_rsp: 0,
            user_rsp: 0,
        }
    }
}

const EMPTY_CTX: CpuSyscallContext = CpuSyscallContext::new();
static mut CPU_SYSCALL_CTX: [CpuSyscallContext; 256] = [EMPTY_CTX; 256];

/// Configure fast `syscall`/`sysretq` MSRs and `IA32_KERNEL_GSBASE` on the executing CPU core.
fn configure_cpu_syscall_msrs() {
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

        // Set FMASK to clear IF (disable interrupts), TF, and DF during syscall entry prologue
        // 0x200 = IF, 0x100 = TF, 0x400 = DF
        wrmsr(IA32_FMASK, 0x700);

        // Point IA32_KERNEL_GSBASE at this core's CpuSyscallContext so `swapgs` reaches kernel_rsp
        let apic_id = crate::arch::x86_64::apic::get_apic_id() as usize;
        let ctx_addr = core::ptr::addr_of!(CPU_SYSCALL_CTX[apic_id]) as u64;
        wrmsr(IA32_GS_BASE, 0);
        wrmsr(IA32_KERNEL_GSBASE, ctx_addr);
    }
}

/// Initialize x86_64 fast syscall (MSR STAR, LSTAR, FMASK, EFER.SCE, KERNEL_GSBASE) on BSP.
pub fn init() {
    configure_cpu_syscall_msrs();
}

/// Initialize fast syscall MSRs and `IA32_KERNEL_GSBASE` on a secondary Application Processor.
pub fn init_ap() {
    configure_cpu_syscall_msrs();
}

/// Update the kernel stack top loaded on `syscall` entry for the executing CPU core.
#[inline]
pub fn set_syscall_kernel_stack(stack_top: u64) {
    let apic_id = crate::arch::x86_64::apic::get_apic_id() as usize;
    unsafe {
        core::ptr::write_volatile(
            core::ptr::addr_of_mut!(CPU_SYSCALL_CTX[apic_id].kernel_rsp),
            stack_top,
        );
    }
}
