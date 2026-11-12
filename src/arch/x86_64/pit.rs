use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::arch::x86_64::idt::{register_irq_handler, InterruptContext};
use crate::arch::x86_64::io::{inb, outb};

const PIT_CHANNEL_0: u16 = 0x40;
const PIT_CHANNEL_2: u16 = 0x42;
const PIT_COMMAND: u16 = 0x43;
const PORT_SYSTEM_CONTROL_B: u16 = 0x61;
const PIT_BASE_FREQUENCY: u32 = 1193182;

static TICKS: AtomicU64 = AtomicU64::new(0);
static TSC_KHZ: AtomicU64 = AtomicU64::new(2_000_000); // Conservative 2.0 GHz default
static TSC_CALIBRATED: AtomicBool = AtomicBool::new(false);

#[inline(always)]
pub fn read_tsc() -> u64 {
    let lo: u32;
    let hi: u32;
    unsafe {
        core::arch::asm!(
            "rdtsc",
            out("eax") lo,
            out("edx") hi,
            options(nomem, nostack, preserves_flags)
        );
    }
    ((hi as u64) << 32) | (lo as u64)
}

#[inline(always)]
fn are_interrupts_enabled() -> bool {
    let rflags: u64;
    unsafe {
        core::arch::asm!("pushfq; pop {}", out(reg) rflags, options(nomem, preserves_flags));
    }
    (rflags & (1 << 9)) != 0
}

/// Calibrate TSC frequency using CPUID (leaves 0x15/0x16) or a bounded 10ms PIT Ch2 gate.
pub fn calibrate_tsc() {
    if TSC_CALIBRATED.load(Ordering::Acquire) {
        return;
    }

    // 1. Check CPUID max leaf and try Crystal Clock (0x15) / Base MHz (0x16)
    let max_leaf: u32;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "mov eax, 0",
            "cpuid",
            "pop rbx",
            out("eax") max_leaf,
            out("ecx") _,
            out("edx") _,
            options(nomem)
        );
    }

    if max_leaf >= 0x15 {
        let denom: u32;
        let numer: u32;
        let crystal_hz: u32;
        unsafe {
            core::arch::asm!(
                "push rbx",
                "mov eax, 0x15",
                "cpuid",
                "mov {denom:e}, eax",
                "mov {numer:e}, ebx",
                "pop rbx",
                denom = out(reg) denom,
                numer = out(reg) numer,
                out("eax") _,
                out("ecx") crystal_hz,
                out("edx") _,
                options(nomem)
            );
        }
        if denom > 0 && numer > 0 && crystal_hz > 0 {
            let hz = ((crystal_hz as u64).saturating_mul(numer as u64)) / (denom as u64);
            let khz = (hz / 1000).clamp(100_000, 10_000_000);
            TSC_KHZ.store(khz, Ordering::Release);
            TSC_CALIBRATED.store(true, Ordering::Release);
            return;
        }
    }

    if max_leaf >= 0x16 {
        let base_mhz: u32;
        unsafe {
            core::arch::asm!(
                "push rbx",
                "mov eax, 0x16",
                "cpuid",
                "pop rbx",
                out("eax") base_mhz,
                out("ecx") _,
                out("edx") _,
                options(nomem)
            );
        }
        if base_mhz >= 100 && base_mhz <= 10_000 {
            TSC_KHZ.store((base_mhz as u64) * 1000, Ordering::Release);
            TSC_CALIBRATED.store(true, Ordering::Release);
            return;
        }
    }

    // 2. Try bounded PIT Channel 2 (10 ms) calibration if legacy port 0x61 responds
    let t_start = read_tsc();
    if try_pit_ch2_wait_10ms() {
        let t_end = read_tsc();
        let delta = t_end.saturating_sub(t_start);
        if delta > 10_000 {
            let khz = (delta / 10).clamp(100_000, 10_000_000);
            TSC_KHZ.store(khz, Ordering::Release);
            TSC_CALIBRATED.store(true, Ordering::Release);
            return;
        }
    }

    TSC_CALIBRATED.store(true, Ordering::Release);
}

/// Bounded 10ms one-shot wait using PIT Channel 2 (returns true if PIT hardware responded).
fn try_pit_ch2_wait_10ms() -> bool {
    unsafe {
        let sys_b = inb(PORT_SYSTEM_CONTROL_B);
        if sys_b == 0xFF {
            return false; // Legacy port 0x61 absent on modern bare-metal board
        }

        // Disable speaker output, enable Ch2 gate
        outb(PORT_SYSTEM_CONTROL_B, (sys_b & !0x02) | 0x01);
        // Channel 2, lobyte/hibyte, mode 0 (interrupt on terminal count)
        outb(PIT_COMMAND, 0xB0);
        // 11932 ticks ~= 10.000 ms
        let count: u16 = 11932;
        outb(PIT_CHANNEL_2, (count & 0xFF) as u8);
        outb(PIT_CHANNEL_2, ((count >> 8) & 0xFF) as u8);

        // Reset gate pulse cleanly to start countdown
        let cur = inb(PORT_SYSTEM_CONTROL_B) & !0x01;
        outb(PORT_SYSTEM_CONTROL_B, cur);
        outb(PORT_SYSTEM_CONTROL_B, cur | 0x01);

        // Wait for OUT2 high (bit 5 of 0x61), bounded to prevent hanging if PIT is broken
        for _ in 0..50_000_000u32 {
            let val = inb(PORT_SYSTEM_CONTROL_B);
            if val == 0xFF {
                return false;
            }
            if (val & 0x20) != 0 {
                // Restore speaker/gate bits safely
                outb(PORT_SYSTEM_CONTROL_B, sys_b & !0x03);
                return true;
            }
            core::hint::spin_loop();
        }
        outb(PORT_SYSTEM_CONTROL_B, sys_b & !0x03);
    }
    false
}

/// Wait ~10ms for timer calibration (uses PIT Ch2 when present, else calibrated TSC delay).
pub fn measure_calibration_window_10ms() {
    calibrate_tsc();
    if !try_pit_ch2_wait_10ms() {
        delay_ms(10);
    }
}

/// Interrupt-independent busy wait in microseconds using RDTSC.
pub fn delay_us(us: u64) {
    if us == 0 {
        return;
    }
    if !TSC_CALIBRATED.load(Ordering::Acquire) {
        calibrate_tsc();
    }
    let khz = TSC_KHZ.load(Ordering::Relaxed);
    let ticks_per_us = (khz / 1000).max(100);
    let target = read_tsc().saturating_add(us.saturating_mul(ticks_per_us));
    while read_tsc() < target {
        core::hint::spin_loop();
    }
}

/// Interrupt-independent busy wait in milliseconds (safe when IF=0 or before LAPIC/PIT IRQs run).
pub fn delay_ms(ms: u64) {
    delay_us(ms.saturating_mul(1000));
}

/// Initialize the 8254 PIT to fire at the requested frequency (Hz) as an early fallback
/// before Local APIC MMIO Timer takes over on modern systems.
pub fn init(frequency_hz: u32) {
    calibrate_tsc();

    let divisor = (PIT_BASE_FREQUENCY / frequency_hz.max(1)) as u16;
    unsafe {
        // Channel 0, Lobyte/Hibyte access, Mode 3 (Square Wave), Binary mode
        outb(PIT_COMMAND, 0x36);
        outb(PIT_CHANNEL_0, (divisor & 0xFF) as u8);
        outb(PIT_CHANNEL_0, ((divisor >> 8) & 0xFF) as u8);
    }

    // Register IRQ 0 fallback handler (legacy PIT IRQ 0 stays masked; LAPIC timer drives Vector 48)
    register_irq_handler(0, timer_irq_handler);
}

pub static IN_KERNEL_TESTS: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Advance global OS millisecond tick counter and notify the scheduler.
pub fn advance_system_tick(ctx: &mut InterruptContext) {
    let ticks = TICKS.fetch_add(1, Ordering::Relaxed) + 1;
    if (ticks & 3) == 0 && !IN_KERNEL_TESTS.load(Ordering::Relaxed) {
        crate::drivers::usb::poll_hid_nonblocking();
    }
    crate::task::scheduler::on_timer_tick(ticks, ctx);
}

fn timer_irq_handler(ctx: &mut InterruptContext) {
    // Only drive ticks from PIT IRQ0 when Local APIC MMIO Timer has not taken ownership
    if !crate::arch::x86_64::apic::is_lapic_timer_active() {
        advance_system_tick(ctx);
    }
}

/// Get total timer ticks elapsed since boot (1000 Hz = 1 ms per tick).
pub fn get_ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

/// Wait for `ms` milliseconds. Safely uses interrupt-driven tick waiting once `IF=1`,
/// or calibrated RDTSC delay when interrupts are disabled (early boot / AP bring-up / drivers).
pub fn sleep_ms(ms: u64) {
    if ms == 0 {
        return;
    }

    if !are_interrupts_enabled() {
        delay_ms(ms);
        return;
    }

    let khz = TSC_KHZ.load(Ordering::Relaxed);
    let safety_tsc_limit = read_tsc().saturating_add(ms.saturating_add(500).saturating_mul(khz));
    let target = get_ticks().saturating_add(ms);

    while get_ticks() < target {
        if read_tsc() >= safety_tsc_limit {
            break;
        }
        core::hint::spin_loop();
    }
}
