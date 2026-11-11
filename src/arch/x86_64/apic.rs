use core::sync::atomic::{AtomicU64, Ordering};
use crate::arch::x86_64::msr::{rdmsr, wrmsr, IA32_APIC_BASE};

pub const APIC_DEFAULT_BASE: u64 = 0xFEE00000;
pub const APIC_VIRTUAL_BASE: u64 = 0xFFFFFFFF80000000u64.wrapping_add(APIC_DEFAULT_BASE);

pub const REG_ID: u32 = 0x020;
pub const REG_VERSION: u32 = 0x030;
pub const REG_TPR: u32 = 0x080;
pub const REG_EOI: u32 = 0x0B0;
pub const REG_SVR: u32 = 0x0F0;
pub const REG_ESR: u32 = 0x280;
pub const REG_ICR_LOW: u32 = 0x300;
pub const REG_ICR_HIGH: u32 = 0x310;
pub const REG_TIMER: u32 = 0x320;
pub const REG_TIMER_INIT_CNT: u32 = 0x380;
pub const REG_TIMER_CURR_CNT: u32 = 0x390;
pub const REG_TIMER_DIV_CFG: u32 = 0x3E0;

static LAPIC_BASE_VIRT: AtomicU64 = AtomicU64::new(0);

#[inline(always)]
pub fn get_lapic_base_virt() -> u64 {
    let addr = LAPIC_BASE_VIRT.load(Ordering::Relaxed);
    if addr != 0 {
        addr
    } else {
        APIC_VIRTUAL_BASE
    }
}

/// Check if APIC is supported via CPUID.
pub fn is_supported() -> bool {
    let edx: u32;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "mov eax, 1",
            "cpuid",
            "pop rbx",
            out("edx") edx,
            out("eax") _,
            out("ecx") _,
            options(nomem)
        );
    }
    (edx & (1 << 9)) != 0
}

/// Read an APIC 32-bit register.
pub unsafe fn read_reg(reg: u32) -> u32 {
    let base = get_lapic_base_virt();
    let ptr = (base + reg as u64) as *const u32;
    core::ptr::read_volatile(ptr)
}

/// Write an APIC 32-bit register.
pub unsafe fn write_reg(reg: u32, val: u32) {
    let base = get_lapic_base_virt();
    let ptr = (base + reg as u64) as *mut u32;
    core::ptr::write_volatile(ptr, val);
}

/// Get current processor's APIC ID.
pub fn get_apic_id() -> u8 {
    if !is_supported() {
        return 0;
    }
    unsafe { (read_reg(REG_ID) >> 24) as u8 }
}

/// Enable Local APIC and configure spurious interrupt vector.
pub fn init() {
    if !is_supported() {
        return;
    }

    let madt_base = crate::arch::x86_64::madt::MADT.lock().lapic_address as u64;
    let phys_base = if madt_base != 0 { madt_base } else { APIC_DEFAULT_BASE };

    if let Ok(virt) = crate::mm::vmm::map_mmio_range(phys_base, 4096) {
        LAPIC_BASE_VIRT.store(virt, Ordering::SeqCst);
    }

    unsafe {
        // Enable APIC via MSR (bit 11)
        let apic_base = rdmsr(IA32_APIC_BASE);
        wrmsr(IA32_APIC_BASE, apic_base | (1 << 11));

        // Enable APIC in Spurious Vector Register with vector 255 (bit 8 = software enable)
        write_reg(REG_SVR, 0x1FF);

        // Clear Task Priority Register to accept all interrupts
        write_reg(REG_TPR, 0);
    }
}

/// Signal End-of-Interrupt to Local APIC.
pub fn eoi() {
    unsafe {
        write_reg(REG_EOI, 0);
    }
}

/// Send Inter-Processor Interrupt (IPI) via ICR.
pub fn send_ipi(dest_apic_id: u8, vector: u8, delivery_mode: u32) {
    unsafe {
        write_reg(REG_ICR_HIGH, (dest_apic_id as u32) << 24);
        // Delivery mode at bits 8..10, level assert at bit 14
        write_reg(REG_ICR_LOW, (delivery_mode << 8) | (vector as u32) | (1 << 14));
    }
}

/// Send INIT IPI to reset target AP core.
pub fn send_init_ipi(dest_apic_id: u8) {
    send_ipi(dest_apic_id, 0, 5); // Mode 5: INIT
}

/// Send Startup IPI (SIPI) to start AP core executing at (page_num * 4096).
pub fn send_sipi(dest_apic_id: u8, page_num: u8) {
    send_ipi(dest_apic_id, page_num, 6); // Mode 6: Startup
}
