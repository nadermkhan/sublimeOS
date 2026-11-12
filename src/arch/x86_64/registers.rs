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

/// Enable SSE, FXSAVE/FXRSTOR and SIMD floating-point exceptions
/// (CR0.MP=1, CR0.EM=0, CR4.OSFXSR=1, CR4.OSXMMEXCPT=1).
#[inline(always)]
pub unsafe fn enable_sse() {
    let mut cr0 = read_cr0();
    cr0 &= !(1 << 2); // Clear EM (Emulation)
    cr0 |= 1 << 1;    // Set MP (Monitor Coprocessor)
    write_cr0(cr0);

    let mut cr4 = read_cr4();
    cr4 |= (1 << 9) | (1 << 10); // Set OSFXSR (bit 9) and OSXMMEXCPT (bit 10)
    write_cr4(cr4);
}

/// Initialize IA32_PAT MSR (0x277) to support Write-Combining (WC) in Entry 1.
///
/// Entry 0: WB  (0x06) - Standard Write-Back caching for DRAM
/// Entry 1: WC  (0x01) - Write-Combining for Framebuffers & MMIO burst writes
/// Entry 2: UC- (0x07) - Uncached Weak
/// Entry 3: UC  (0x00) - Uncacheable for hardware device registers
/// Entry 4: WB  (0x06)
/// Entry 5: WT  (0x04) - Write-Through
/// Entry 6: UC- (0x07)
/// Entry 7: UC  (0x00)
#[inline(always)]
pub unsafe fn enable_write_combining() {
    const PAT_VALUE: u64 = 0x0007040600070106;
    crate::arch::x86_64::msr::wrmsr(crate::arch::x86_64::msr::IA32_PAT, PAT_VALUE);
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

#[derive(Debug, Clone, Copy)]
pub struct CpuidResult {
    pub eax: u32,
    pub ebx: u32,
    pub ecx: u32,
    pub edx: u32,
}

/// Execute CPUID instruction with `leaf` (eax) and `sub_leaf` (ecx), safely preserving `rbx`.
#[inline(always)]
pub fn cpuid(leaf: u32, sub_leaf: u32) -> CpuidResult {
    let eax: u32;
    let ebx: u32;
    let ecx: u32;
    let edx: u32;
    unsafe {
        asm!(
            "push rbx",
            "cpuid",
            "mov {ebx_tmp:e}, ebx",
            "pop rbx",
            inout("eax") leaf => eax,
            inout("ecx") sub_leaf => ecx,
            ebx_tmp = out(reg) ebx,
            out("edx") edx,
            options(nomem)
        );
    }
    CpuidResult { eax, ebx, ecx, edx }
}

/// Retrieve authentic CPU vendor ID string (e.g. "GenuineIntel", "AuthenticAMD").
pub fn get_cpu_vendor() -> alloc::string::String {
    let res = cpuid(0, 0);
    let mut bytes = [0u8; 12];
    bytes[0..4].copy_from_slice(&res.ebx.to_le_bytes());
    bytes[4..8].copy_from_slice(&res.edx.to_le_bytes());
    bytes[8..12].copy_from_slice(&res.ecx.to_le_bytes());
    if let Ok(s) = core::str::from_utf8(&bytes) {
        alloc::string::String::from(s.trim())
    } else {
        alloc::string::String::from("x86_64 Compatible")
    }
}

/// Retrieve authentic CPU brand string directly from processor silicon registers
/// via CPUID extended leaves 0x80000002..0x80000004 (e.g. "Intel(R) Core(TM) i7-2760QM CPU @ 2.40GHz").
pub fn get_cpu_brand_string() -> alloc::string::String {
    let max_ext = cpuid(0x8000_0000, 0).eax;
    if max_ext >= 0x8000_0004 {
        let mut brand_bytes = [0u8; 48];
        for i in 0..3 {
            let res = cpuid(0x8000_0002 + i, 0);
            let offset = (i as usize) * 16;
            brand_bytes[offset..offset + 4].copy_from_slice(&res.eax.to_le_bytes());
            brand_bytes[offset + 4..offset + 8].copy_from_slice(&res.ebx.to_le_bytes());
            brand_bytes[offset + 8..offset + 12].copy_from_slice(&res.ecx.to_le_bytes());
            brand_bytes[offset + 12..offset + 16].copy_from_slice(&res.edx.to_le_bytes());
        }

        // Find null terminator if present
        let len = brand_bytes.iter().position(|&b| b == 0).unwrap_or(48);
        if let Ok(s) = core::str::from_utf8(&brand_bytes[..len]) {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                return alloc::string::String::from(trimmed);
            }
        }
    }

    // Fallback if extended brand leaves not available
    let vendor = get_cpu_vendor();
    alloc::format!("{} x86_64 Processor", vendor)
}

/// Retrieve authentic core and logical thread counts from CPUID and ACPI MADT.
pub fn get_cpu_cores_and_threads() -> (u32, u32) {
    let madt_threads = crate::arch::x86_64::madt::MADT.lock().cpus.len() as u32;

    // CPUID leaf 1: logical processor count in ebx[23:16]
    let leaf1 = cpuid(1, 0);
    let cpuid_logical = ((leaf1.ebx >> 16) & 0xFF) as u32;

    // CPUID leaf 4, sub-leaf 0: max cores in eax[31:26] + 1
    let leaf4 = cpuid(4, 0);
    let cpuid_cores = (((leaf4.eax >> 26) & 0x3F) + 1) as u32;

    let threads = if madt_threads > 0 {
        madt_threads
    } else if cpuid_logical > 0 {
        cpuid_logical
    } else {
        1
    };

    let cores = if cpuid_cores > 0 && cpuid_cores <= threads {
        cpuid_cores
    } else {
        threads
    };

    (cores, threads)
}

