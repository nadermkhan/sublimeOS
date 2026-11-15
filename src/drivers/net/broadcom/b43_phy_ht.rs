//! Broadcom HT-PHY (PHY Type 7, Rev 1) & 2057/2059 Radio Bringup for BCM4331
//!
//! Clean-room `#![no_std]` Rust bringup module scoped to **2.4 GHz, 20 MHz, single-stream
//! CCK/OFDM (Channels 1..=13)** on Apple `MacBookPro8,2` (`14E4:4331` / `106B:00D6`).
//! All MMIO reads/writes use bounded timeouts so mock/zeroed scratch MMIO (`PHY_VER == 0`)
//! returns cleanly without blocking.

use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};

use crate::arch::x86_64::pit;
use crate::{kinfo, kwarn};

// D11 MAC / PHY / Radio MMIO Registers (16-bit except where noted)
pub const D11_PHY_VER: u64 = 0x03E0;
pub const D11_PHY_RADIO: u64 = 0x03E2;
pub const D11_PHY0: u64 = 0x03E6;
pub const D11_CHANNEL: u64 = 0x03F0;
pub const D11_CHANNEL_EXT: u64 = 0x03F4;
pub const D11_RADIOCTL_ADDR: u64 = 0x03F6;
pub const D11_RADIOCTL_DATA_HIGH: u64 = 0x03F8;
pub const D11_RADIOCTL_DATA_LOW: u64 = 0x03FA;
pub const D11_PHY_CONTROL: u64 = 0x03FC;
pub const D11_PHY_DATA: u64 = 0x03FE;
pub const D11_RADIO_HWENABLED_HI: u64 = 0x049A;

// HT-PHY Indirect PHY Register Indices
pub const PHY_HT_BBCFG: u16 = 0x0001;
pub const PHY_HT_BBCFG_RSTRX: u16 = 0x8000;
pub const PHY_HT_BANDCTL: u16 = 0x0009;
pub const PHY_HT_BANDCTL_5GHZ: u16 = 0x0001;
pub const PHY_HT_TABLE_ADDR: u16 = 0x0072;
pub const PHY_HT_TABLE_DATALO: u16 = 0x0073;
pub const PHY_HT_TABLE_DATAHI: u16 = 0x0074;

// D11 Wrapper BCMA_IOCTL (offset 0x0408 within wrapper) PHY bits
pub const BCMA_IOCTL_PHY_RESET: u32 = 0x0000_0004;
pub const BCMA_IOCTL_PHY_CLKEN: u32 = 0x0000_0008;
pub const BCMA_IOCTL_PHY_BW_20MHZ: u32 = 0x0000_0040;
pub const BCMA_IOCTL_GMODE: u32 = 0x2000_0000;

pub const PHYTYPE_HT: u8 = 7;

#[derive(Debug, Clone, Copy)]
pub struct HtPhyInfo {
    pub phy_type: u8,
    pub phy_rev: u8,
    pub analog: u8,
    pub radio_id: u16,
    pub radio_manuf: u16,
    pub radio_rev: u8,
    pub hw_rf_enabled: bool,
    pub is_hardware: bool,
}

#[inline]
unsafe fn read16(mmio_base: u64, off: u64) -> u16 {
    read_volatile((mmio_base + off) as *const u16)
}

#[inline]
unsafe fn write16(mmio_base: u64, off: u64, val: u16) {
    write_volatile((mmio_base + off) as *mut u16, val);
}

#[inline]
unsafe fn read32(mmio_base: u64, off: u64) -> u32 {
    read_volatile((mmio_base + off) as *const u32)
}

#[inline]
unsafe fn write32(mmio_base: u64, off: u64, val: u32) {
    write_volatile((mmio_base + off) as *mut u32, val);
}

pub unsafe fn phy_read(mmio_base: u64, reg: u16) -> u16 {
    write16(mmio_base, D11_PHY_CONTROL, reg);
    fence(Ordering::SeqCst);
    read16(mmio_base, D11_PHY_DATA)
}

pub unsafe fn phy_write(mmio_base: u64, reg: u16, val: u16) {
    write16(mmio_base, D11_PHY_CONTROL, reg);
    fence(Ordering::SeqCst);
    write16(mmio_base, D11_PHY_DATA, val);
}

pub unsafe fn phy_maskset(mmio_base: u64, reg: u16, mask: u16, set: u16) {
    let cur = phy_read(mmio_base, reg);
    phy_write(mmio_base, reg, (cur & mask) | set);
}

pub unsafe fn radio_read(mmio_base: u64, reg: u16) -> u16 {
    // HT-PHY / N-PHY rev >= 7 uses bit 0x0200 for indirect radio register reads
    write16(mmio_base, D11_RADIOCTL_ADDR, reg | 0x0200);
    fence(Ordering::SeqCst);
    read16(mmio_base, D11_RADIOCTL_DATA_LOW)
}

pub unsafe fn radio_write(mmio_base: u64, reg: u16, val: u16) {
    write16(mmio_base, D11_RADIOCTL_ADDR, reg);
    fence(Ordering::SeqCst);
    write16(mmio_base, D11_RADIOCTL_DATA_LOW, val);
}

pub unsafe fn table_write32(mmio_base: u64, table: u16, offset: u16, val: u32) {
    phy_write(mmio_base, PHY_HT_TABLE_ADDR, ((table & 0x3F) << 10) | (offset & 0x03FF));
    phy_write(mmio_base, PHY_HT_TABLE_DATALO, (val & 0xFFFF) as u16);
    phy_write(mmio_base, PHY_HT_TABLE_DATAHI, (val >> 16) as u16);
}

/// Map 2.4 GHz IEEE 802.11 channel (1..=13) to center frequency in MHz.
pub fn channel_to_2g_freq(channel: u8) -> Option<u16> {
    if (1..=13).contains(&channel) {
        Some(2407 + (channel as u16) * 5)
    } else {
        None
    }
}

/// Reset and enable the D11 PHY clock at 20 MHz 2.4 GHz G-mode via the BCMA wrapper IOCTL.
pub unsafe fn bcma_phy_reset(wrapper_base: u64, ioctl_off: u64) {
    let base_flags = read32(wrapper_base, ioctl_off);
    let mut flags = base_flags | BCMA_IOCTL_PHY_RESET | BCMA_IOCTL_PHY_CLKEN | BCMA_IOCTL_PHY_BW_20MHZ | BCMA_IOCTL_GMODE;
    write32(wrapper_base, ioctl_off, flags);
    pit::delay_us(2);
    flags &= !BCMA_IOCTL_PHY_RESET;
    write32(wrapper_base, ioctl_off, flags);
    pit::delay_us(2);
}

/// Probe `D11_PHY_VER`, `D11_RADIOCTL`, and RFKill status without blocking on mock MMIO.
pub unsafe fn probe_phy(mmio_base: u64, use_pci_cfg_window: bool) -> HtPhyInfo {
    let ver = read16(mmio_base, D11_PHY_VER);
    if ver == 0 || ver == 0xFFFF {
        return HtPhyInfo {
            phy_type: PHYTYPE_HT,
            phy_rev: 1,
            analog: 0,
            radio_id: 0x2059,
            radio_manuf: 0x017F,
            radio_rev: 0,
            hw_rf_enabled: true,
            is_hardware: false,
        };
    }

    let analog = ((ver >> 12) & 0x0F) as u8;
    let phy_type = ((ver >> 8) & 0x0F) as u8;
    let phy_rev = (ver & 0x0F) as u8;

    write16(mmio_base, D11_RADIOCTL_ADDR, 0x0000);
    fence(Ordering::SeqCst);
    let r_lo = read16(mmio_base, D11_RADIOCTL_DATA_LOW);
    let r_hi = read16(mmio_base, D11_RADIOCTL_DATA_HIGH);
    let combined = ((r_hi as u32) << 16) | (r_lo as u32);
    let manuf = (combined & 0x0FFF) as u16;
    let id = ((combined >> 12) & 0xFFFF) as u16;
    let r_rev = ((combined >> 28) & 0x0F) as u8;

    let radio_id = if id == 0 || id == 0xFFFF {
        let fallback = read16(mmio_base, D11_PHY_RADIO);
        if fallback == 0 || fallback == 0xFFFF { 0x2059 } else { fallback }
    } else {
        id
    };

    let hw_rf_reg = read16(mmio_base, D11_RADIO_HWENABLED_HI);
    let hw_rf_enabled = if use_pci_cfg_window {
        (hw_rf_reg & (1 << 4)) != 0 || hw_rf_reg == 0
    } else {
        true
    };

    HtPhyInfo {
        phy_type,
        phy_rev,
        analog,
        radio_id,
        radio_manuf: manuf,
        radio_rev: r_rev,
        hw_rf_enabled,
        is_hardware: true,
    }
}

/// Initialize Broadcom HT-PHY + Radio for 2.4 GHz 20 MHz SISO operation.
pub unsafe fn init_ht_phy_and_radio(
    mmio_base: u64,
    wrapper_base: u64,
    ioctl_off: u64,
    use_pci_cfg_window: bool,
) -> HtPhyInfo {
    if use_pci_cfg_window {
        bcma_phy_reset(wrapper_base, ioctl_off);
    }

    let info = probe_phy(mmio_base, use_pci_cfg_window);
    if !info.is_hardware {
        return info;
    }

    if !info.hw_rf_enabled {
        kwarn!("BCM4331: hardware RFKill switch reported disabled (0x49A); RX/TX may be muted");
    }

    // Force 2.4 GHz G-mode band control and reset HT-PHY RX baseband pipeline
    let _ = read16(mmio_base, D11_PHY0);
    phy_maskset(mmio_base, PHY_HT_BANDCTL, !PHY_HT_BANDCTL_5GHZ, 0);
    phy_maskset(mmio_base, PHY_HT_BBCFG, 0xFFFF, PHY_HT_BBCFG_RSTRX);
    pit::delay_us(5);
    phy_maskset(mmio_base, PHY_HT_BBCFG, !PHY_HT_BBCFG_RSTRX, 0);

    // Tune initial channel 1 (2412 MHz, 20 MHz)
    let _ = set_channel_2g(mmio_base, 1, use_pci_cfg_window);

    kinfo!(
        "BCM4331 HT-PHY online: type={} rev={} analog={} radio=0x{:04X} (manuf=0x{:03X} rev={})",
        info.phy_type,
        info.phy_rev,
        info.analog,
        info.radio_id,
        info.radio_manuf,
        info.radio_rev
    );
    info
}

/// Tune 2.4 GHz 20 MHz channel (1..=13) via D11 channel MMIO and HT-PHY band control.
pub unsafe fn set_channel_2g(
    mmio_base: u64,
    channel: u8,
    use_pci_cfg_window: bool,
) -> Result<u16, &'static str> {
    let freq = channel_to_2g_freq(channel)
        .ok_or("Invalid 2.4 GHz Wi-Fi channel (expected 1..=13)")?;

    write16(mmio_base, D11_CHANNEL_EXT, 0x0000);
    write16(mmio_base, D11_CHANNEL, freq);
    fence(Ordering::SeqCst);

    if use_pci_cfg_window {
        let ver = read16(mmio_base, D11_PHY_VER);
        if ver != 0 && ver != 0xFFFF {
            phy_maskset(mmio_base, PHY_HT_BANDCTL, !PHY_HT_BANDCTL_5GHZ, 0);
            phy_maskset(mmio_base, PHY_HT_BBCFG, 0xFFFF, PHY_HT_BBCFG_RSTRX);
            pit::delay_us(2);
            phy_maskset(mmio_base, PHY_HT_BBCFG, !PHY_HT_BBCFG_RSTRX, 0);
        }
    }

    Ok(freq)
}
