use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use crate::arch::x86_64::idt::LAPIC_TIMER_VECTOR;
use crate::arch::x86_64::msr::{rdmsr, wrmsr, IA32_APIC_BASE};
use crate::kinfo;

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

const LVT_MASKED: u32 = 1 << 16;
const LVT_TIMER_PERIODIC: u32 = 1 << 17;

static LAPIC_BASE_VIRT: AtomicU64 = AtomicU64::new(0);
static LAPIC_TIMER_ACTIVE: AtomicBool = AtomicBool::new(false);
static LAPIC_TICKS_PER_MS: AtomicU32 = AtomicU32::new(0);

#[inline(always)]
pub fn get_lapic_base_virt() -> u64 {
    let addr = LAPIC_BASE_VIRT.load(Ordering::Relaxed);
    if addr != 0 {
        addr
    } else {
        APIC_VIRTUAL_BASE
    }
}

#[inline(always)]
pub fn is_lapic_timer_active() -> bool {
    LAPIC_TIMER_ACTIVE.load(Ordering::Acquire)
}

#[inline(always)]
pub fn get_lapic_ticks_per_ms() -> u32 {
    LAPIC_TICKS_PER_MS.load(Ordering::Acquire)
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

/// Get current processor's APIC ID (safe even before MMIO LAPIC is mapped via CPUID leaf 1).
pub fn get_apic_id() -> u8 {
    if !is_supported() {
        return 0;
    }
    if LAPIC_BASE_VIRT.load(Ordering::Acquire) != 0 {
        unsafe { (read_reg(REG_ID) >> 24) as u8 }
    } else {
        let ebx: u32;
        unsafe {
            core::arch::asm!(
                "push rbx",
                "mov eax, 1",
                "cpuid",
                "mov {out:e}, ebx",
                "pop rbx",
                out = out(reg) ebx,
                out("eax") _,
                out("ecx") _,
                out("edx") _,
                options(nomem)
            );
        }
        (ebx >> 24) as u8
    }
}

unsafe fn enable_lapic_hardware() {
    // Enable Local APIC via IA32_APIC_BASE MSR (bit 11)
    let apic_base = rdmsr(IA32_APIC_BASE);
    wrmsr(IA32_APIC_BASE, apic_base | (1 << 11));

    // Enable APIC in Spurious Vector Register with vector 255 (bit 8 = software enable)
    write_reg(REG_SVR, 0x1FF);

    // Clear Task Priority Register to accept all interrupts
    write_reg(REG_TPR, 0);
}

/// Enable Local APIC, configure spurious interrupt vector, and calibrate + start
/// the periodic Local APIC MMIO timer (1000 Hz on Vector 48), replacing legacy PIT IRQ 0.
pub fn init() {
    if !is_supported() {
        return;
    }

    let madt_base = crate::arch::x86_64::madt::MADT.lock().lapic_address;
    let phys_base = if madt_base != 0 { madt_base } else { APIC_DEFAULT_BASE };

    if let Ok(virt) = crate::mm::vmm::map_mmio_range(phys_base, 4096) {
        LAPIC_BASE_VIRT.store(virt, Ordering::SeqCst);
    } else {
        return;
    }

    unsafe {
        enable_lapic_hardware();
        calibrate_and_start_timer(1000);
    }
}

/// Initialize the Local APIC on an Application Processor (AP) core without re-mapping MMIO
/// or perturbing BSP's global system tick calibration.
pub fn init_ap() {
    if !is_supported() || LAPIC_BASE_VIRT.load(Ordering::Acquire) == 0 {
        return;
    }
    unsafe {
        enable_lapic_hardware();
        // Keep AP local timer masked while parked in idle loop
        write_reg(REG_TIMER, LVT_MASKED);
        write_reg(REG_ESR, 0);
        eoi();
    }
}

/// Calibrate LAPIC bus frequency and program Periodic MMIO Timer on Vector 48.
unsafe fn calibrate_and_start_timer(hz: u32) {
    // Divide by 16 configuration
    write_reg(REG_TIMER_DIV_CFG, 0x3);
    // Mask timer during one-shot calibration measurement
    write_reg(REG_TIMER, LVT_MASKED);
    write_reg(REG_TIMER_INIT_CNT, 0xFFFF_FFFF);

    // Wait a calibrated 10ms window (uses PIT Ch2 one-shot or RDTSC fallback)
    crate::arch::x86_64::pit::measure_calibration_window_10ms();

    let curr_cnt = read_reg(REG_TIMER_CURR_CNT);
    // Stop timer before programming periodic mode
    write_reg(REG_TIMER, LVT_MASKED);

    let elapsed_10ms = 0xFFFF_FFFFu32.saturating_sub(curr_cnt);
    let ticks_per_ms = (elapsed_10ms / 10).max(1_000);
    let initial_count = ((ticks_per_ms as u64 * 1000) / (hz.max(1) as u64)).max(100) as u32;

    LAPIC_TICKS_PER_MS.store(ticks_per_ms, Ordering::Release);

    // Program Periodic MMIO Timer on IDT Vector 48
    write_reg(REG_TIMER_DIV_CFG, 0x3);
    write_reg(REG_TIMER, LVT_TIMER_PERIODIC | (LAPIC_TIMER_VECTOR as u32));
    write_reg(REG_TIMER_INIT_CNT, initial_count);

    LAPIC_TIMER_ACTIVE.store(true, Ordering::SeqCst);

    // Mask legacy 8259 PIT IRQ 0 now that LAPIC MMIO timer drives OS ticks
    crate::arch::x86_64::pic::mask(0);

    kinfo!(
        "Local APIC MMIO Timer Active: {} ticks/ms ({} Hz, Vector {}), legacy PIT IRQ 0 masked",
        ticks_per_ms,
        hz,
        LAPIC_TIMER_VECTOR
    );
}

/// Signal End-of-Interrupt to Local APIC (safe no-op if MMIO LAPIC is not mapped yet).
pub fn eoi() {
    if get_lapic_base_virt() != 0 {
        unsafe {
            write_reg(REG_EOI, 0);
        }
    }
}

/// Send Inter-Processor Interrupt (IPI) via ICR and wait cleanly for delivery idle.
pub fn send_ipi(dest_apic_id: u8, vector: u8, delivery_mode: u32) {
    unsafe {
        write_reg(REG_ESR, 0);
        write_reg(REG_ICR_HIGH, (dest_apic_id as u32) << 24);
        // Delivery mode at bits 8..10, level assert at bit 14
        write_reg(REG_ICR_LOW, (delivery_mode << 8) | (vector as u32) | (1 << 14));

        // Wait for Send Pending bit (bit 12) to clear
        for _ in 0..100_000 {
            if (read_reg(REG_ICR_LOW) & (1 << 12)) == 0 {
                break;
            }
            core::hint::spin_loop();
        }
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
