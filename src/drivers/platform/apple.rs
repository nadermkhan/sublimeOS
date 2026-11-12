//! Apple MacBookPro8,2 Hardware Controller Driver (SMC & gmux)
//!
//! Controls the Apple System Management Controller (`APPLESMC` I/O `0x300`/`0x304`,
//! 4-char key `"LKSB"` for keyboard LED PWM) and Apple `gmux` I/O window
//! (`0x700` data / `0x704` value / `0x774` LCD panel PWM) with safe bounded presence
//! checks so non-Apple/QEMU boards never hang on missing silicon.

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use crate::arch::x86_64::io::{inb, inl, outb, outl};
use crate::kinfo;

/// Apple SMC (System Management Controller) I/O Ports
pub const APPLESMC_DATA_PORT: u16 = 0x300;
pub const APPLESMC_CMD_PORT: u16 = 0x304;
pub const APPLESMC_READ_CMD: u8 = 0x10;
pub const APPLESMC_WRITE_CMD: u8 = 0x11;
pub const APPLESMC_KEY_KBD_BACKLIGHT: [u8; 4] = *b"LKSB";

/// Apple gmux I/O Ports (MacBookPro8,2 hybrid graphics / backlight multiplexer)
pub const GMUX_PORT_DATA: u16 = 0x700;
pub const GMUX_PORT_VALUE: u16 = 0x704;
pub const GMUX_PORT_SWITCH_DISPLAY: u16 = 0x710;
pub const GMUX_PORT_SWITCH_DDC: u16 = 0x728;
pub const GMUX_PORT_SWITCH_EXTERNAL: u16 = 0x740;
pub const GMUX_PORT_KBD_BACKLIGHT: u8 = 0x64;
pub const GMUX_PORT_MAX_BRIGHTNESS: u16 = 0x770;
pub const GMUX_PORT_BRIGHTNESS: u16 = 0x774;

const MAX_SMC_WAIT_SPINS: usize = 4096;

static SMC_PRESENT: AtomicBool = AtomicBool::new(false);
static GMUX_PRESENT: AtomicBool = AtomicBool::new(false);
static CURRENT_KBD_BRIGHTNESS: AtomicU8 = AtomicU8::new(0);
static CURRENT_DISPLAY_BRIGHTNESS: AtomicU8 = AtomicU8::new(255);

pub struct ApplePlatformHardware;

impl ApplePlatformHardware {
    /// Probe legacy Apple SMC (`0x304`) and gmux (`0x700`) ports without hanging on floating bus `0xFF`.
    pub fn init() {
        let smc_ok = unsafe { Self::probe_smc_raw() };
        let gmux_ok = unsafe { Self::probe_gmux_raw() };

        SMC_PRESENT.store(smc_ok, Ordering::Relaxed);
        GMUX_PRESENT.store(gmux_ok, Ordering::Relaxed);

        if smc_ok || gmux_ok {
            if gmux_ok {
                // On MacBookPro8,2 with disabled/failed discrete GPU:
                // Explicitly lock gmux switch registers to the Intel HD 3000 integrated GPU (IGD = 2)
                // 0x710 = Display panel LVDS/eDP
                // 0x728 = DDC EDID lines
                // 0x740 = External DisplayPort
                unsafe {
                    outb(GMUX_PORT_SWITCH_DISPLAY, 2);
                    outb(GMUX_PORT_SWITCH_DDC, 2);
                    outb(GMUX_PORT_SWITCH_EXTERNAL, 2);
                }
                kinfo!("Apple gmux: Panel LVDS, DDC, and external port locked to Intel IGD (0x710=2, 0x728=2, 0x740=2)");
            }
            kinfo!(
                "Apple Platform Hardware detected (SMC={}, gmux={}) - Backlight controls enabled via /dev/backlight",
                smc_ok,
                gmux_ok
            );
        } else {
            kinfo!(
                "Apple SMC/gmux hardware absent on this board - /dev/backlight running in shadow-register mode"
            );
        }
    }

    pub fn is_smc_present() -> bool {
        SMC_PRESENT.load(Ordering::Relaxed)
    }

    pub fn is_gmux_present() -> bool {
        GMUX_PRESENT.load(Ordering::Relaxed)
    }

    /// Bounded check verifying Apple SMC status port returns a valid non-floating status byte.
    unsafe fn probe_smc_raw() -> bool {
        let status = inb(APPLESMC_CMD_PORT);
        status != 0xFF
    }

    /// Bounded check verifying Apple gmux classic I/O space is decoded.
    unsafe fn probe_gmux_raw() -> bool {
        let v0 = inb(GMUX_PORT_DATA);
        if v0 == 0xFF {
            return false;
        }
        // Check gmux brightness register is also non-floating
        inl(GMUX_PORT_BRIGHTNESS) != 0xFFFF_FFFF
    }

    /// Wait cleanly for SMC status port bitmask (`val` = `0x04` Ready to Write Command/Data,
    /// `0x05` Ready with Read Data). Never loops unboundedly.
    unsafe fn smc_wait_status(expected_low_nibble: u8) -> bool {
        for _ in 0..MAX_SMC_WAIT_SPINS {
            let st = inb(APPLESMC_CMD_PORT);
            if st == 0xFF {
                return false;
            }
            if (st & 0x0F) == expected_low_nibble {
                return true;
            }
            core::hint::spin_loop();
        }
        false
    }

    /// Send a 2-byte SMC write command for 4-char key `"LKSB"` (`[brightness, 0x00]`).
    unsafe fn smc_write_key_2(key: [u8; 4], payload: [u8; 2]) -> bool {
        if !Self::smc_wait_status(0x04) {
            return false;
        }
        outb(APPLESMC_CMD_PORT, APPLESMC_WRITE_CMD);
        for &b in &key {
            if !Self::smc_wait_status(0x04) {
                return false;
            }
            outb(APPLESMC_DATA_PORT, b);
        }
        if !Self::smc_wait_status(0x04) {
            return false;
        }
        outb(APPLESMC_DATA_PORT, 2); // 2-byte LKSB payload length
        for &b in &payload {
            if !Self::smc_wait_status(0x04) {
                return false;
            }
            outb(APPLESMC_DATA_PORT, b);
        }
        true
    }

    /// Set MacBook Pro keyboard backlight PWM duty cycle (`0` = Off, `255` = Max).
    pub fn set_keyboard_light(brightness: u8) {
        CURRENT_KBD_BRIGHTNESS.store(brightness, Ordering::Relaxed);

        unsafe {
            if Self::is_smc_present() {
                let _ = Self::smc_write_key_2(APPLESMC_KEY_KBD_BACKLIGHT, [brightness, 0x00]);
            }
            if Self::is_gmux_present() {
                outl(GMUX_PORT_VALUE, brightness as u32);
                outb(GMUX_PORT_DATA, GMUX_PORT_KBD_BACKLIGHT);
            }
        }
    }

    /// Read current keyboard backlight duty cycle (`0..=255`).
    pub fn get_keyboard_light() -> u8 {
        CURRENT_KBD_BRIGHTNESS.load(Ordering::Relaxed)
    }

    /// Set internal eDP/LVDS display panel brightness (`0..=255`) via Apple gmux if present.
    pub fn set_display_brightness(brightness: u8) {
        CURRENT_DISPLAY_BRIGHTNESS.store(brightness, Ordering::Relaxed);
        if Self::is_gmux_present() {
            unsafe {
                let max_val = inl(GMUX_PORT_MAX_BRIGHTNESS);
                let scaled = if max_val != 0 && max_val != 0xFFFF_FFFF {
                    ((brightness as u64) * (max_val as u64) / 255) as u32
                } else {
                    (brightness as u32) << 4
                };
                outl(GMUX_PORT_BRIGHTNESS, scaled);
            }
        }
    }

    pub fn get_display_brightness() -> u8 {
        CURRENT_DISPLAY_BRIGHTNESS.load(Ordering::Relaxed)
    }
}
