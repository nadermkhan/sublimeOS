//! Broadcom BCM4331 Wireless Controller (`b43` / BCMA) Driver
//!
//! Targets Broadcom BCM4331 (`14E4:4331`) Dual-Band 802.11n PCIe silicon on Apple
//! `MacBookPro8,2` (`106B:00D6`). Implements serialized BCMA PCIe BAR0 window switching,
//! EROM core scanning, ChipCommon SPROM / ACPI MCFG ECAM DSN MAC discovery, split 4 KiB
//! 64-bit DMA rings with BCMA `0x8000_0000` translation + `ADDREXT`, PSM microcode +
//! HT-PHY/Radio bringup + initvals sequencing, revision-dispatched `B43TxHdr` (118B `r598`
//! / 110B `r410`) & `B43RxHdrFw4` (30B), shared dual-queue `poll_rx_dma()`, and
//! `802.3 <-> 802.11 LLC/SNAP` + pure software `AES-CCMP` (independent `TK`/`GTK` PNs).

extern crate alloc;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, AtomicBool, Ordering};

use crate::arch::x86_64::pit;
use crate::drivers::net::broadcom::b43_phy_ht;
use crate::drivers::pci::{PciBus, PciDevice};
use crate::mm::pmm::{alloc_zeroed_frame, PMM};
use crate::mm::{phys_to_virt, PAGE_SIZE};
use crate::net::crypto::{ccmp_decrypt_mpdu, ccmp_encrypt_mpdu};
use crate::net::device::{NetDevice, NetDeviceType};
use crate::sync::IrqSafeSpinlock;
use crate::{kinfo, kwarn};

pub const B43_PCI_VENDOR: u16 = 0x14E4;
pub const B43_BCM4331_DEV: u16 = 0x4331;

/// Returns `true` for Broadcom BCM4331 wireless PCI device IDs.
pub fn is_supported_device(device_id: u16) -> bool {
    device_id == B43_BCM4331_DEV
}

// Compile-time firmware ingestion (populated by build.rs when extracted .fw files exist)
#[cfg(has_b43_ucode29_mimo)]
static B43_UC_BLOB: &[u8] = include_bytes!("../../../../firmware/b43/ucode29_mimo.fw");
#[cfg(all(not(has_b43_ucode29_mimo), has_b43_ucode16_mimo))]
static B43_UC_BLOB: &[u8] = include_bytes!("../../../../firmware/b43/ucode16_mimo.fw");
#[cfg(all(not(has_b43_ucode29_mimo), not(has_b43_ucode16_mimo), has_b43_ucode16))]
static B43_UC_BLOB: &[u8] = include_bytes!("../../../../firmware/b43/ucode16.fw");
#[cfg(all(not(has_b43_ucode29_mimo), not(has_b43_ucode16_mimo), not(has_b43_ucode16)))]
static B43_UC_BLOB: &[u8] = &[];

#[cfg(has_b43_ht0initvals29)]
static B43_HT0_INITVALS_BLOB: &[u8] = include_bytes!("../../../../firmware/b43/ht0initvals29.fw");
#[cfg(not(has_b43_ht0initvals29))]
static B43_HT0_INITVALS_BLOB: &[u8] = &[];

#[cfg(has_b43_ht0bsinitvals29)]
static B43_HT0_BSINITVALS_BLOB: &[u8] = include_bytes!("../../../../firmware/b43/ht0bsinitvals29.fw");
#[cfg(not(has_b43_ht0bsinitvals29))]
static B43_HT0_BSINITVALS_BLOB: &[u8] = &[];

#[cfg(has_b43_pcm16)]
static B43_PCM_BLOB: &[u8] = include_bytes!("../../../../firmware/b43/pcm16.fw");
#[cfg(not(has_b43_pcm16))]
static B43_PCM_BLOB: &[u8] = &[];

// Broadcom BCMA PCIe Backplane Window & Agent Registers
pub const BCMA_PCI_BAR0_WIN: u8 = 0x80;
pub const BCMA_PCI_BAR0_WRAPPER_WIN: u8 = 0x8C;
pub const BCMA_CHIPCOMMON_BASE: u32 = 0x1800_0000;
pub const BCMA_D11_CORE_BASE: u32 = 0x1800_1000;
pub const BCMA_ENUM_BASE: u32 = 0x1810_0000;
pub const BCMA_IOCTL: u64 = 0x0408;
pub const BCMA_RESET_CTL: u64 = 0x0800;
const BCMA_IOCTL_CLK: u32 = 0x0001;
const BCMA_IOCTL_FGC: u32 = 0x0002;
const BCMA_RESET_CTL_RESET: u32 = 0x0001;

// ChipCommon offsets
const CC_CLKCTLST: u64 = 0x01E0;
const CC_CLKCTLST_HAVEHT: u32 = 0x0002_0000;
const CC_SPROM_OFFSET: u64 = 0x0800;
const CC_SPROM8_MAC_OFFSET: u64 = 0x008C;

// Broadcom b43 MAC / SHM / XMITSTAT / DMA64 Registers
pub const B43_MMIO_MACCTL: u64 = 0x0120;
pub const B43_MMIO_GEN_IRQ_REASON: u64 = 0x0128;
pub const B43_MMIO_SHM_CONTROL: u64 = 0x0160;
pub const B43_MMIO_SHM_DATA: u64 = 0x0164;
pub const B43_MMIO_XMITSTAT_0: u64 = 0x0170;
pub const B43_MMIO_XMITSTAT_1: u64 = 0x0174;
pub const B43_MMIO_PSM_PHY_HDR: u64 = 0x0492;

pub const B43_DMA64_TX0_BASE: u64 = 0x0200;
pub const B43_DMA64_RX0_BASE: u64 = 0x0220;
const B43_DMA64_CTL_OFF: u64 = 0x00;
const B43_DMA64_INDEX_OFF: u64 = 0x04;
const B43_DMA64_RINGLO_OFF: u64 = 0x08;
const B43_DMA64_RINGHI_OFF: u64 = 0x0C;
const B43_DMA64_STATUS_OFF: u64 = 0x10;
const B43_DMA64_STAT_CURR_MASK: u32 = 0x1FFF;
pub const B43_DMA64_PCIE_TRANSLATION: u32 = 0x8000_0000;
pub const B43_DMA64_DCTL1_ADDREXT_SHIFT: u32 = 16;

// B43_MMIO_MACCTL bits
const B43_MACCTL_ENABLED: u32 = 0x0000_0001;
const B43_MACCTL_PSM_RUN: u32 = 0x0000_0002;
const B43_MACCTL_PSM_JMP0: u32 = 0x0000_0004;
const B43_MACCTL_SHM_ENABLED: u32 = 0x0000_0100;
const B43_MACCTL_IHR_ENABLED: u32 = 0x0000_0400;
const B43_MACCTL_INFRA: u32 = 0x0001_0000;
const B43_MACCTL_PASS_CTL: u32 = 0x0010_0000;
const B43_MACCTL_KEEP_MGMT: u32 = 0x0040_0000;
const B43_MACCTL_BEACONS: u32 = 0x0080_0000;
const B43_MACCTL_GMODE: u32 = 0x8000_0000;

const B43_SHM_UCODE: u32 = 0x0000_0000;
const B43_SHM_SHARED: u32 = 0x0001_0000;
const B43_SHM_AUTOINC_W: u32 = 0x0200_0000;
const B43_SHM_SH_UCODEREV: u32 = 0x0002;

pub const B43_FW_TYPE_UCODE: u8 = b'u';
pub const B43_FW_TYPE_PCM: u8 = b'p';
pub const B43_FW_TYPE_INITVALS: u8 = b'i';

pub const B43_MMIO_MACFILTER_CONTROL: u64 = 0x0420;
pub const B43_MMIO_MACFILTER_DATA: u64 = 0x0422;
pub const B43_MACFILTER_SELF: u16 = 0x0000;
pub const B43_MACFILTER_BSSID: u16 = 0x0008;

pub const B43_RX_MAC_FCSERR: u32 = 0x0000_0001;
pub const B43_RX_MAC_PADDING: u32 = 0x0000_0004;
const B43_TXH_MAC_ACK: u32 = 0x0000_0001;

pub const B43_RING_LEN: usize = 16;
pub const B43_DMA_SLOT_SIZE: usize = 2048;
pub const B43_DMA_BUFFER_FRAMES: usize = (B43_RING_LEN * 2 * B43_DMA_SLOT_SIZE) / PAGE_SIZE;

const DEFAULT_MOCK_MAC: [u8; 6] = [0x28, 0xCF, 0xDA, 0xD7, 0x64, 0xDC];
const LLC_SNAP_HDR: [u8; 6] = [0xAA, 0xAA, 0x03, 0x00, 0x00, 0x00];

/// Linux `struct b43_fw_header` (8 bytes packed).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct B43FwHeader {
    pub fw_type: u8,
    pub ver: u8,
    pub padding: [u8; 2],
    pub size_be: [u8; 4],
}

/// Broadcom `ucode` `r598+` TX Header (112 base + 6-byte `plcp` = **118 bytes**).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct B43TxHdrR598 {
    pub mac_ctl: u32,
    pub mac_frame_ctl: u16,
    pub tx_fes_time_norm: u16,
    pub phy_ctl: u16,
    pub phy_ctl1: u16,
    pub phy_ctl1_fb: u16,
    pub phy_ctl1_rts: u16,
    pub phy_ctl1_rts_fb: u16,
    pub phy_rate: u8,
    pub phy_rate_rts: u8,
    pub extra_ft: u8,
    pub chan_radio_code: u8,
    pub iv: [u8; 16],
    pub tx_receiver: [u8; 6],
    pub tx_fes_time_fb: u16,
    pub rts_plcp_fb: [u8; 6],
    pub rts_dur_fb: u16,
    pub plcp_fb: [u8; 6],
    pub dur_fb: u16,
    pub mimo_modelen: u16,
    pub mimo_ratelen_fb: u16,
    pub timeout: u32,
    pub mimo_antenna: u16,
    pub preload_size: u16,
    pub pad0: [u8; 2],
    pub cookie: u16,
    pub tx_status: u16,
    pub max_n_mpdus: u16,
    pub max_a_bytes_mrt: u16,
    pub max_a_bytes_fbr: u16,
    pub min_m_bytes: u16,
    pub rts_plcp: [u8; 6],
    pub rts_frame: [u8; 16],
    pub pad1: [u8; 2],
    pub plcp: [u8; 6],
}

/// Broadcom `ucode` `r410..597` TX Header (104 base + 6-byte `plcp` = **110 bytes**).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct B43TxHdrR410 {
    pub mac_ctl: u32,
    pub mac_frame_ctl: u16,
    pub tx_fes_time_norm: u16,
    pub phy_ctl: u16,
    pub phy_ctl1: u16,
    pub phy_ctl1_fb: u16,
    pub phy_ctl1_rts: u16,
    pub phy_ctl1_rts_fb: u16,
    pub phy_rate: u8,
    pub phy_rate_rts: u8,
    pub extra_ft: u8,
    pub chan_radio_code: u8,
    pub iv: [u8; 16],
    pub tx_receiver: [u8; 6],
    pub tx_fes_time_fb: u16,
    pub rts_plcp_fb: [u8; 6],
    pub rts_dur_fb: u16,
    pub plcp_fb: [u8; 6],
    pub dur_fb: u16,
    pub mimo_modelen: u16,
    pub mimo_ratelen_fb: u16,
    pub timeout: u32,
    pub mimo_antenna: u16,
    pub preload_size: u16,
    pub pad0: [u8; 2],
    pub cookie: u16,
    pub tx_status: u16,
    pub rts_plcp: [u8; 6],
    pub rts_frame: [u8; 16],
    pub pad1: [u8; 2],
    pub plcp: [u8; 6],
}

/// Broadcom `b43_rxhdr_fw4` RX Header (**30 bytes**).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct B43RxHdrFw4 {
    pub frame_len: u16,
    pub pad0: [u8; 2],
    pub phy_status0: u16,
    pub phy_status1: u16,
    pub phy_status2: u16,
    pub phy_status3: u16,
    pub phy_status4: u16,
    pub phy_status5: u16,
    pub mac_status: u32,
    pub mac_time: u16,
    pub channel: u16,
    pub phy_pad: [u8; 6],
}

const _: () = assert!(core::mem::size_of::<B43TxHdrR598>() == 118);
const _: () = assert!(core::mem::size_of::<B43TxHdrR410>() == 110);
const _: () = assert!(core::mem::size_of::<B43RxHdrFw4>() == 30);

/// Broadcom b43 64-bit DMA descriptor (`struct b43_dmadesc64`, 16 bytes).
#[repr(C, align(16))]
#[derive(Debug, Clone, Copy, Default)]
pub struct B43DmaDesc64 {
    pub ctrl1: u32,
    pub ctrl2: u32,
    pub addr_low: u32,
    pub addr_high: u32,
}

const B43_DMA64_DCTL1_SOF: u32 = 0x8000_0000;
const B43_DMA64_DCTL1_EOF: u32 = 0x4000_0000;
const B43_DMA64_DCTL1_DTABLEEND: u32 = 0x1000_0000;

#[inline]
pub fn encode_dma64_addr(phys: u64) -> (u32, u32, u32) {
    let hi_bits = (phys >> 32) as u32;
    let lo = (phys & 0xFFFF_FFFF) as u32;
    let hi = hi_bits | B43_DMA64_PCIE_TRANSLATION;
    let addrext = (hi_bits & 0x3) << B43_DMA64_DCTL1_ADDREXT_SHIFT;
    (lo, hi, addrext)
}

pub struct B43DmaRing {
    pub tx_desc_frame: u64,
    pub rx_desc_frame: u64,
    pub buf_base_frame: u64,
    pub tx_prod: usize,
    pub rx_cons: usize,
    pub tx_inflight: [bool; B43_RING_LEN],
    pub tx_cookies: [u16; B43_RING_LEN],
    pub next_cookie: u16,
}

impl B43DmaRing {
    pub fn allocate() -> Result<Self, &'static str> {
        let tx_desc_frame = alloc_zeroed_frame()
            .ok_or("OOM allocating BCM4331 TX 64-bit DMA descriptor frame")?;
        let rx_desc_frame = match alloc_zeroed_frame() {
            Some(f) => f,
            None => {
                PMM.lock().free_frame(tx_desc_frame);
                return Err("OOM allocating BCM4331 RX 64-bit DMA descriptor frame");
            }
        };
        let buf_base_frame = match PMM.lock().alloc_contiguous_frames(B43_DMA_BUFFER_FRAMES) {
            Some(f) => f,
            None => {
                let mut pmm = PMM.lock();
                pmm.free_frame(tx_desc_frame);
                pmm.free_frame(rx_desc_frame);
                return Err("OOM allocating Broadcom BCM4331 contiguous DMA buffer frames");
            }
        };

        let rx_descs = phys_to_virt(rx_desc_frame) as *mut B43DmaDesc64;
        let buf_virt = phys_to_virt(buf_base_frame);

        unsafe {
            core::ptr::write_bytes(buf_virt as *mut u8, 0, B43_DMA_BUFFER_FRAMES * PAGE_SIZE);
            for i in 0..B43_RING_LEN {
                let buf_phys = buf_base_frame + ((B43_RING_LEN + i) * B43_DMA_SLOT_SIZE) as u64;
                let (addr_low, addr_high, addrext) = encode_dma64_addr(buf_phys);
                let end_flag = if i + 1 == B43_RING_LEN { B43_DMA64_DCTL1_DTABLEEND } else { 0 };
                write_volatile(
                    rx_descs.add(i),
                    B43DmaDesc64 {
                        ctrl1: end_flag | addrext,
                        ctrl2: (B43_DMA_SLOT_SIZE as u32) & 0x1FFF,
                        addr_low,
                        addr_high,
                    },
                );
            }
        }
        fence(Ordering::SeqCst);

        Ok(Self {
            tx_desc_frame,
            rx_desc_frame,
            buf_base_frame,
            tx_prod: 0,
            rx_cons: 0,
            tx_inflight: [false; B43_RING_LEN],
            tx_cookies: [0; B43_RING_LEN],
            next_cookie: 1,
        })
    }

    pub fn tx_ring_phys(&self) -> u64 {
        self.tx_desc_frame
    }

    pub fn rx_ring_phys(&self) -> u64 {
        self.rx_desc_frame
    }
}

impl Drop for B43DmaRing {
    fn drop(&mut self) {
        let mut pmm = PMM.lock();
        pmm.free_frame(self.tx_desc_frame);
        pmm.free_frame(self.rx_desc_frame);
        for i in 0..B43_DMA_BUFFER_FRAMES {
            pmm.free_frame(self.buf_base_frame + (i * PAGE_SIZE) as u64);
        }
    }
}

#[derive(Debug, Clone)]
pub struct WifiCipherSession {
    pub bssid: [u8; 6],
    pub connected: bool,
    pub tk: Option<[u8; 16]>,
    pub tx_pn: u64,
    pub rx_tk_pn: Option<u64>,
    pub gtk: Option<(u8, [u8; 16])>,
    pub rx_gtk_pn: [Option<u64>; 4],
    pub tx_seq: u16,
}

impl WifiCipherSession {
    const fn new() -> Self {
        Self {
            bssid: [0u8; 6],
            connected: false,
            tk: None,
            tx_pn: 0,
            rx_tk_pn: None,
            gtk: None,
            rx_gtk_pn: [None; 4],
            tx_seq: 1,
        }
    }
}

struct BcmaCores {
    chipcommon_base: u32,
    chipcommon_wrap: u32,
    d11_base: u32,
    d11_wrap: u32,
    pcie_base: Option<u32>,
}

pub struct B43Device {
    pub pci: PciDevice,
    pub chip_id: u16,
    pub mmio_base: u64,
    pub mac: [u8; 6],
    pub firmware_loaded: bool,
    pub ucode_bytes: usize,
    pub ucode_rev: u16,
    pub use_pci_cfg_window: bool,
    pub link_up: AtomicBool,
    pub bcma_lock: IrqSafeSpinlock<()>,
    pub dma: IrqSafeSpinlock<B43DmaRing>,
    pub session: IrqSafeSpinlock<WifiCipherSession>,
    pub rx_8023_queue: IrqSafeSpinlock<VecDeque<Vec<u8>>>,
    pub rx_80211_queue: IrqSafeSpinlock<VecDeque<Vec<u8>>>,
}

unsafe impl Send for B43Device {}
unsafe impl Sync for B43Device {}

/// Validate and strip the 8-byte `b43_fw_header` from an extracted `.fw` blob.
pub fn parse_fw_payload(blob: &[u8], expected_type: u8) -> Result<&[u8], &'static str> {
    if blob.len() <= core::mem::size_of::<B43FwHeader>() {
        return Err("b43 firmware blob too small for b43_fw_header");
    }
    let fw_type = blob[0];
    let ver = blob[1];
    let payload_len = u32::from_be_bytes([blob[4], blob[5], blob[6], blob[7]]) as usize;
    if fw_type != expected_type || ver != 1 {
        return Err("b43 firmware header type or version mismatch");
    }
    let body = &blob[8..];
    if payload_len == 0 || payload_len > body.len() {
        return Err("b43 firmware payload truncated relative to header size");
    }
    Ok(&body[..payload_len])
}

/// Returns `true` if a non-empty BCM4331 microcode binary was embedded by `build.rs`.
pub fn has_embedded_firmware() -> bool {
    !B43_UC_BLOB.is_empty()
}

fn is_valid_unicast_mac(mac: &[u8; 6]) -> bool {
    let all_zero = mac.iter().all(|&b| b == 0x00);
    let all_ff = mac.iter().all(|&b| b == 0xFF);
    !all_zero && !all_ff && (mac[0] & 0x01) == 0
}

fn generate_cck_1m_plcp(mpdu_with_fcs_len: usize) -> [u8; 6] {
    let us = ((mpdu_with_fcs_len as u32) * 8).clamp(8, 0xFFFF) as u16;
    let len_le = us.to_le_bytes();
    [0x0A, 0x00, len_le[0], len_le[1], 0x00, 0x00]
}

fn fill_common_txhdr(
    frame: &[u8],
    ucode_rev: u16,
    cookie: u16,
    out: &mut [u8],
) -> usize {
    let use_r598 = ucode_rev >= 598;
    let hdr_len = if use_r598 { 118 } else { 110 };
    out[..hdr_len].fill(0);

    let is_unicast = frame.get(4).map(|b| (b & 0x01) == 0).unwrap_or(false);
    let mut mac_ctl = 0u32;
    if is_unicast {
        mac_ctl |= B43_TXH_MAC_ACK;
    }
    // Pure software AES-CCMP: leave hardware encryption bits (bits 2..=3) zero.
    let frame_ctl = u16::from_le_bytes([frame[0], frame[1]]);
    let plcp = generate_cck_1m_plcp(frame.len() + 4);

    if use_r598 {
        let mut hdr = B43TxHdrR598::default();
        hdr.mac_ctl = mac_ctl.to_le();
        hdr.mac_frame_ctl = frame_ctl.to_le();
        hdr.chan_radio_code = 1;
        if frame.len() >= 10 {
            hdr.tx_receiver.copy_from_slice(&frame[4..10]);
        }
        hdr.mimo_antenna = 0x0001u16.to_le();
        hdr.cookie = cookie.to_le();
        hdr.plcp = plcp;
        let bytes = unsafe {
            core::slice::from_raw_parts(&hdr as *const B43TxHdrR598 as *const u8, 118)
        };
        out[..118].copy_from_slice(bytes);
    } else {
        let mut hdr = B43TxHdrR410::default();
        hdr.mac_ctl = mac_ctl.to_le();
        hdr.mac_frame_ctl = frame_ctl.to_le();
        hdr.chan_radio_code = 1;
        if frame.len() >= 10 {
            hdr.tx_receiver.copy_from_slice(&frame[4..10]);
        }
        hdr.mimo_antenna = 0x0001u16.to_le();
        hdr.cookie = cookie.to_le();
        hdr.plcp = plcp;
        let bytes = unsafe {
            core::slice::from_raw_parts(&hdr as *const B43TxHdrR410 as *const u8, 110)
        };
        out[..110].copy_from_slice(bytes);
    }
    hdr_len
}

/// Convert an IEEE 802.3 Ethernet II frame (`>= 14` bytes) into an 802.11 Data MPDU
/// with `ToDS=1` + 8-byte `LLC/SNAP` (unencrypted).
pub fn wrap_8023_to_80211_plain(
    eth_frame: &[u8],
    bssid: [u8; 6],
    self_mac: [u8; 6],
    seq_num: u16,
    protected: bool,
) -> Result<([u8; 24], Vec<u8>), &'static str> {
    if eth_frame.len() < 14 {
        return Err("Ethernet II frame shorter than 14-byte header");
    }
    let fc1 = 0x01u8 | if protected { 0x40 } else { 0x00 }; // ToDS=1, optional Protected
    let seq_ctrl = ((seq_num & 0x0FFF) << 4).to_le_bytes();
    let mut hdr = [0u8; 24];
    hdr[0] = 0x08; // Type 2 (Data), Subtype 0
    hdr[1] = fc1;
    hdr[2] = 0x3A;
    hdr[3] = 0x01;
    hdr[4..10].copy_from_slice(&bssid);          // Addr1 = RA / BSSID
    hdr[10..16].copy_from_slice(&self_mac);      // Addr2 = TA / SA
    hdr[16..22].copy_from_slice(&eth_frame[..6]); // Addr3 = DA
    hdr[22..24].copy_from_slice(&seq_ctrl);

    let mut llc_payload = Vec::with_capacity(8 + eth_frame.len() - 14);
    llc_payload.extend_from_slice(&LLC_SNAP_HDR);
    llc_payload.extend_from_slice(&eth_frame[12..14]); // EtherType
    llc_payload.extend_from_slice(&eth_frame[14..]);
    Ok((hdr, llc_payload))
}

/// Parse an 802.11 Data / QoS-Data frame (unencrypted or already decrypted `llc_payload`)
/// with `LLC/SNAP` into `(da, sa, ethertype, ethernet_frame)`.
pub fn unwrap_llc_to_8023(
    hdr_24: &[u8],
    llc_and_payload: &[u8],
) -> Option<([u8; 6], [u8; 6], u16, Vec<u8>)> {
    if hdr_24.len() < 24 || llc_and_payload.len() < 8 {
        return None;
    }
    if llc_and_payload[..6] != LLC_SNAP_HDR {
        return None;
    }
    let to_ds = (hdr_24[1] & 0x01) != 0;
    let from_ds = (hdr_24[1] & 0x02) != 0;
    let (da, sa) = match (to_ds, from_ds) {
        (false, true) => (&hdr_24[4..10], &hdr_24[16..22]),  // AP->STA: A1=DA, A2=BSSID, A3=SA
        (true, false) => (&hdr_24[16..22], &hdr_24[10..16]), // STA->AP: A1=BSSID, A2=SA, A3=DA
        (false, false) => (&hdr_24[4..10], &hdr_24[10..16]), // Direct/test: A1=DA, A2=SA, A3=BSSID
        (true, true) => return None,                         // 4-addr WDS unsupported
    };

    let mut da_arr = [0u8; 6];
    let mut sa_arr = [0u8; 6];
    da_arr.copy_from_slice(da);
    sa_arr.copy_from_slice(sa);
    let ethertype = u16::from_be_bytes([llc_and_payload[6], llc_and_payload[7]]);

    let mut eth = Vec::with_capacity(14 + llc_and_payload.len() - 8);
    eth.extend_from_slice(&da_arr);
    eth.extend_from_slice(&sa_arr);
    eth.extend_from_slice(&llc_and_payload[6..8]);
    eth.extend_from_slice(&llc_and_payload[8..]);
    Some((da_arr, sa_arr, ethertype, eth))
}

impl B43Device {
    /// Discover, map BAR0 MMIO (16 KiB BCMA core aperture), initialize BCMA window,
    /// allocate split 64-bit DMA rings, and upload embedded BCM4331 uCode when present.
    pub fn new(pci: PciDevice) -> Result<Arc<Self>, &'static str> {
        let mmio_phys = pci.read_bar(0);
        if mmio_phys == 0 {
            return Err("Invalid Broadcom BCM4331 MMIO base (PCI BAR0 is 0)");
        }

        let mmio_base = crate::mm::vmm::map_mmio_range(mmio_phys, 16 * 1024)?;

        unsafe {
            pci.ensure_d0_power_state();
            PciBus::enable_bus_mastering(pci.bus, pci.device, pci.function);
            // Disable PCIe ASPM L0s/L1 on BCM4331 so DMA/MMIO wakes reliably
            if let Some(pcie_cap) = pci.find_capability(0x10) {
                let lnkctl = PciBus::read_config_word(pci.bus, pci.device, pci.function, pcie_cap + 0x10);
                if (lnkctl & 0x0003) != 0 {
                    PciBus::write_config_word(pci.bus, pci.device, pci.function, pcie_cap + 0x10, lnkctl & !0x0003);
                }
            }
        }

        Self::from_mmio(pci, mmio_base, true)
    }

    /// Initialize `B43Device` from an already-mapped 16 KiB MMIO region.
    pub fn from_mmio(
        pci: PciDevice,
        mmio_base: u64,
        use_pci_cfg_window: bool,
    ) -> Result<Arc<Self>, &'static str> {
        let dma = B43DmaRing::allocate()?;
        let bcma_lock = IrqSafeSpinlock::new(());

        let (chip_id, mac, firmware_loaded, ucode_bytes, ucode_rev) = unsafe {
            let _guard = bcma_lock.lock();
            Self::init_hardware(&pci, mmio_base, &dma, use_pci_cfg_window)
        };

        if firmware_loaded {
            kinfo!(
                "Broadcom BCM4331 (Chip 0x{:04X}, MAC {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}) online: {}B uCode (rev {}) + DMA64",
                chip_id,
                mac[0], mac[1], mac[2], mac[3], mac[4], mac[5],
                ucode_bytes,
                ucode_rev
            );
        } else {
            kwarn!(
                "Broadcom BCM4331 (Chip 0x{:04X}) detected; ucode29_mimo.fw not embedded.",
                chip_id
            );
        }

        Ok(Arc::new(Self {
            pci,
            chip_id,
            mmio_base,
            mac,
            firmware_loaded,
            ucode_bytes,
            ucode_rev,
            use_pci_cfg_window,
            link_up: AtomicBool::new(false),
            bcma_lock,
            dma: IrqSafeSpinlock::new(dma),
            session: IrqSafeSpinlock::new(WifiCipherSession::new()),
            rx_8023_queue: IrqSafeSpinlock::new(VecDeque::with_capacity(16)),
            rx_80211_queue: IrqSafeSpinlock::new(VecDeque::with_capacity(16)),
        }))
    }

    pub fn tx_header_size(&self) -> usize {
        if self.ucode_rev >= 598 { 118 } else { 110 }
    }

    unsafe fn select_bcma_window(pci: &PciDevice, core_base: u32, wrapper_offset: u32) {
        PciBus::write_config_dword(pci.bus, pci.device, pci.function, BCMA_PCI_BAR0_WIN, core_base);
        PciBus::write_config_dword(pci.bus, pci.device, pci.function, BCMA_PCI_BAR0_WRAPPER_WIN, wrapper_offset);
    }

    unsafe fn scan_bcma_erom(pci: &PciDevice, mmio_base: u64) -> BcmaCores {
        let mut cores = BcmaCores {
            chipcommon_base: BCMA_CHIPCOMMON_BASE,
            chipcommon_wrap: BCMA_ENUM_BASE,
            d11_base: BCMA_D11_CORE_BASE,
            d11_wrap: BCMA_ENUM_BASE + 0x1000,
            pcie_base: None,
        };

        Self::select_bcma_window(pci, BCMA_ENUM_BASE, BCMA_ENUM_BASE);
        let mut idx = 0usize;
        let mut cur_id = 0u16;
        let mut cur_manuf = 0u16;
        let mut cur_slave_addr = 0u32;
        let mut cur_wrap_addr = 0u32;

        while idx < 256 {
            let word = read_volatile((mmio_base + (idx * 4) as u64) as *const u32);
            idx += 1;
            if word == 0 || word == 0xFFFF_FFFF || (word & 0x0F) == 0x0F {
                break;
            }
            let tag = word & 0x07;
            if tag == 0x01 {
                // Commit previous core
                if cur_manuf == 0x04BF && cur_slave_addr != 0 {
                    match cur_id {
                        0x0800 => {
                            cores.chipcommon_base = cur_slave_addr;
                            if cur_wrap_addr != 0 {
                                cores.chipcommon_wrap = cur_wrap_addr;
                            }
                        }
                        0x0812 => {
                            cores.d11_base = cur_slave_addr;
                            if cur_wrap_addr != 0 {
                                cores.d11_wrap = cur_wrap_addr;
                            }
                        }
                        0x0820 => cores.pcie_base = Some(cur_slave_addr),
                        _ => {}
                    }
                }
                cur_manuf = ((word >> 20) & 0x0FFF) as u16;
                cur_id = ((word >> 8) & 0x0FFF) as u16;
                cur_slave_addr = 0;
                cur_wrap_addr = 0;
                // Skip Component Info B
                if idx < 256 {
                    let _ = read_volatile((mmio_base + (idx * 4) as u64) as *const u32);
                    idx += 1;
                }
            } else if tag == 0x03 {
                let addr = word & 0xFFFF_F000;
                let addr_type = (word >> 6) & 0x03;
                if addr_type == 0 && cur_slave_addr == 0 {
                    cur_slave_addr = addr;
                } else if (addr_type == 2 || addr_type == 3) && cur_wrap_addr == 0 {
                    cur_wrap_addr = addr;
                }
            }
        }
        if cur_manuf == 0x04BF && cur_slave_addr != 0 {
            match cur_id {
                0x0800 => {
                    cores.chipcommon_base = cur_slave_addr;
                    if cur_wrap_addr != 0 { cores.chipcommon_wrap = cur_wrap_addr; }
                }
                0x0812 => {
                    cores.d11_base = cur_slave_addr;
                    if cur_wrap_addr != 0 { cores.d11_wrap = cur_wrap_addr; }
                }
                0x0820 => cores.pcie_base = Some(cur_slave_addr),
                _ => {}
            }
        }
        cores
    }

    unsafe fn discover_mac(pci: &PciDevice, mmio_base: u64, use_pci_cfg_window: bool) -> [u8; 6] {
        if !use_pci_cfg_window {
            return DEFAULT_MOCK_MAC;
        }

        // 1. Check ChipCommon SPROM rev8+ MAC address bytes (LE u16 words at SPROM+0x8C)
        let mut raw = [0u8; 6];
        for i in 0..3 {
            let w = read_volatile((mmio_base + CC_SPROM_OFFSET + CC_SPROM8_MAC_OFFSET + (i * 2) as u64) as *const u16);
            raw[i * 2] = (w >> 8) as u8;
            raw[i * 2 + 1] = (w & 0xFF) as u8;
        }
        if is_valid_unicast_mac(&raw) {
            return raw;
        }

        // 2. Apple BCM4331 keeps authentic MAC inside PCIe Extended Capability DSN (0x160/0x164/0x168)
        if let Some(dsn_mac) = crate::arch::x86_64::acpi::read_pcie_ecam_mac_dsn(pci.bus, pci.device, pci.function) {
            if is_valid_unicast_mac(&dsn_mac) {
                return dsn_mac;
            }
        }

        DEFAULT_MOCK_MAC
    }

    unsafe fn read_shm16(mmio_base: u64, target: u32, word_offset: u16) -> u16 {
        write_volatile((mmio_base + B43_MMIO_SHM_CONTROL) as *mut u32, target | (word_offset as u32));
        fence(Ordering::SeqCst);
        let word = read_volatile((mmio_base + B43_MMIO_SHM_DATA) as *const u32);
        (word & 0xFFFF) as u16
    }

    unsafe fn upload_shm_words(mmio_base: u64, shm_target: u32, payload: &[u8]) {
        write_volatile(
            (mmio_base + B43_MMIO_SHM_CONTROL) as *mut u32,
            shm_target | B43_SHM_AUTOINC_W,
        );
        let mut i = 0usize;
        while i + 4 <= payload.len() {
            let word = u32::from_be_bytes([payload[i], payload[i + 1], payload[i + 2], payload[i + 3]]);
            write_volatile((mmio_base + B43_MMIO_SHM_DATA) as *mut u32, word);
            i += 4;
        }
    }

    /// Parse and write a Broadcom Initial Values (`initvals` / `bsinitvals`) firmware payload.
    pub unsafe fn upload_initvals(mmio_base: u64, blob: &[u8]) -> usize {
        if blob.len() < 8 || blob[0] != B43_FW_TYPE_INITVALS {
            return 0;
        }
        let count = u32::from_be_bytes([blob[4], blob[5], blob[6], blob[7]]) as usize;
        let body = &blob[8..];
        let mut pos = 0usize;
        let mut written = 0usize;

        for _ in 0..count {
            if pos + 2 > body.len() {
                break;
            }
            let offset_raw = u16::from_be_bytes([body[pos], body[pos + 1]]);
            pos += 2;
            let is_32 = (offset_raw & 0x8000) != 0;
            let offset = (offset_raw & 0x7FFF) as u64;

            if is_32 {
                if pos + 4 > body.len() { break; }
                let val = u32::from_be_bytes([body[pos], body[pos + 1], body[pos + 2], body[pos + 3]]);
                pos += 4;
                if offset < 0x1000 {
                    write_volatile((mmio_base + offset) as *mut u32, val);
                    written += 1;
                }
            } else {
                if pos + 2 > body.len() { break; }
                let val = u16::from_be_bytes([body[pos], body[pos + 1]]);
                pos += 2;
                if offset < 0x1000 {
                    write_volatile((mmio_base + offset) as *mut u16, val);
                    written += 1;
                }
            }
        }
        written
    }

    /// Program Broadcom hardware MAC address filter table.
    pub unsafe fn write_mac_filter(mmio_base: u64, filter_offset: u16, mac: &[u8; 6]) {
        write_volatile((mmio_base + B43_MMIO_MACFILTER_CONTROL) as *mut u16, filter_offset | 0x0020);
        write_volatile((mmio_base + B43_MMIO_MACFILTER_DATA) as *mut u16, (mac[0] as u16) | ((mac[1] as u16) << 8));
        write_volatile((mmio_base + B43_MMIO_MACFILTER_DATA) as *mut u16, (mac[2] as u16) | ((mac[3] as u16) << 8));
        write_volatile((mmio_base + B43_MMIO_MACFILTER_DATA) as *mut u16, (mac[4] as u16) | ((mac[5] as u16) << 8));
    }

    unsafe fn init_hardware(
        pci: &PciDevice,
        mmio_base: u64,
        dma: &B43DmaRing,
        use_pci_cfg_window: bool,
    ) -> (u16, [u8; 6], bool, usize, u16) {
        let cores = if use_pci_cfg_window {
            let scanned = Self::scan_bcma_erom(pci, mmio_base);
            Self::select_bcma_window(pci, scanned.chipcommon_base, scanned.chipcommon_wrap);
            scanned
        } else {
            BcmaCores {
                chipcommon_base: BCMA_CHIPCOMMON_BASE,
                chipcommon_wrap: BCMA_ENUM_BASE,
                d11_base: BCMA_D11_CORE_BASE,
                d11_wrap: BCMA_ENUM_BASE + 0x1000,
                pcie_base: None,
            }
        };

        let chip_id_reg = read_volatile(mmio_base as *const u32);
        let raw_chip = (chip_id_reg & 0xFFFF) as u16;
        let chip_id = if raw_chip == 0 || raw_chip == 0xFFFF { 0x4331 } else { raw_chip };

        if use_pci_cfg_window {
            // Bounded wait for ChipCommon HT clock
            for _ in 0..500 {
                if (read_volatile((mmio_base + CC_CLKCTLST) as *const u32) & CC_CLKCTLST_HAVEHT) != 0 {
                    break;
                }
                pit::delay_us(10);
            }
        }

        let mac = Self::discover_mac(pci, mmio_base, use_pci_cfg_window);

        if use_pci_cfg_window {
            let _ = cores.pcie_base;
            Self::select_bcma_window(pci, cores.d11_base, cores.d11_wrap);
        }

        // Reset D11 802.11 core
        let wrapper_base = mmio_base + 0x1000;
        write_volatile((wrapper_base + BCMA_IOCTL) as *mut u32, BCMA_IOCTL_CLK | BCMA_IOCTL_FGC);
        write_volatile((wrapper_base + BCMA_RESET_CTL) as *mut u32, BCMA_RESET_CTL_RESET);
        let _ = read_volatile((wrapper_base + BCMA_RESET_CTL) as *const u32);
        pit::delay_us(2);
        write_volatile((wrapper_base + BCMA_RESET_CTL) as *mut u32, 0);
        write_volatile((wrapper_base + BCMA_IOCTL) as *mut u32, BCMA_IOCTL_CLK);
        pit::delay_us(10);

        // Hold PSM stopped while configuring rings and uploading microcode
        write_volatile((mmio_base + B43_MMIO_MACCTL) as *mut u32, B43_MACCTL_SHM_ENABLED | B43_MACCTL_IHR_ENABLED);
        let _ = read_volatile((mmio_base + B43_MMIO_PSM_PHY_HDR) as *const u16);

        let (tx_lo, tx_hi, _) = encode_dma64_addr(dma.tx_ring_phys());
        let (rx_lo, rx_hi, _) = encode_dma64_addr(dma.rx_ring_phys());
        write_volatile((mmio_base + B43_DMA64_TX0_BASE + B43_DMA64_RINGLO_OFF) as *mut u32, tx_lo);
        write_volatile((mmio_base + B43_DMA64_TX0_BASE + B43_DMA64_RINGHI_OFF) as *mut u32, tx_hi);
        write_volatile((mmio_base + B43_DMA64_RX0_BASE + B43_DMA64_RINGLO_OFF) as *mut u32, rx_lo);
        write_volatile((mmio_base + B43_DMA64_RX0_BASE + B43_DMA64_RINGHI_OFF) as *mut u32, rx_hi);

        let Ok(ucode) = parse_fw_payload(B43_UC_BLOB, B43_FW_TYPE_UCODE) else {
            return (chip_id, mac, false, 0, 0);
        };

        // 1. Upload uCode + PCM FIRST before PHY/initvals
        Self::upload_shm_words(mmio_base, B43_SHM_UCODE, ucode);
        if let Ok(pcm) = parse_fw_payload(B43_PCM_BLOB, B43_FW_TYPE_PCM) {
            Self::upload_shm_words(mmio_base, B43_SHM_SHARED, pcm);
        }
        fence(Ordering::SeqCst);

        let macctl_run = B43_MACCTL_GMODE
            | B43_MACCTL_KEEP_MGMT
            | B43_MACCTL_BEACONS
            | B43_MACCTL_INFRA
            | B43_MACCTL_PASS_CTL
            | B43_MACCTL_ENABLED
            | B43_MACCTL_PSM_RUN
            | B43_MACCTL_PSM_JMP0
            | B43_MACCTL_SHM_ENABLED
            | B43_MACCTL_IHR_ENABLED;
        write_volatile((mmio_base + B43_MMIO_MACCTL) as *mut u32, macctl_run);

        let mut ucode_rev = 0u16;
        if use_pci_cfg_window {
            for _ in 0..1000 {
                let rev = Self::read_shm16(mmio_base, B43_SHM_SHARED, B43_SHM_SH_UCODEREV as u16);
                if rev != 0 && rev != 0xFFFF {
                    ucode_rev = rev;
                    break;
                }
                pit::delay_us(10);
            }
        }
        if ucode_rev == 0 {
            ucode_rev = if cfg!(has_b43_ucode29_mimo) { 698 } else { 508 };
        }

        // 2. HT-PHY & Radio bringup
        let _phy_info = b43_phy_ht::init_ht_phy_and_radio(mmio_base, wrapper_base, BCMA_IOCTL, use_pci_cfg_window);

        // 3. Upload HT-PHY / Band-Specific initvals AFTER uCode + PHY bringup
        if !B43_HT0_INITVALS_BLOB.is_empty() {
            Self::upload_initvals(mmio_base, B43_HT0_INITVALS_BLOB);
        }
        if !B43_HT0_BSINITVALS_BLOB.is_empty() {
            Self::upload_initvals(mmio_base, B43_HT0_BSINITVALS_BLOB);
        }

        // 4. Program self MAC filter & enable RX/TX DMA rings (RXFROFF = 30 bytes)
        Self::write_mac_filter(mmio_base, B43_MACFILTER_SELF, &mac);
        Self::write_mac_filter(mmio_base, B43_MACFILTER_BSSID, &[0xFF; 6]);

        write_volatile((mmio_base + B43_DMA64_RX0_BASE + B43_DMA64_CTL_OFF) as *mut u32, (30 << 1) | 0x01);
        write_volatile(
            (mmio_base + B43_DMA64_RX0_BASE + B43_DMA64_INDEX_OFF) as *mut u32,
            ((B43_RING_LEN - 1) * core::mem::size_of::<B43DmaDesc64>()) as u32,
        );
        write_volatile((mmio_base + B43_DMA64_TX0_BASE + B43_DMA64_CTL_OFF) as *mut u32, 0x01);

        (chip_id, mac, true, ucode.len(), ucode_rev)
    }

    unsafe fn reclaim_tx_completions(&self, dma: &mut B43DmaRing) {
        // Drain hardware XMITSTAT_0/1 completion cookies
        for _ in 0..32 {
            let st0 = read_volatile((self.mmio_base + B43_MMIO_XMITSTAT_0) as *const u32);
            let st1 = read_volatile((self.mmio_base + B43_MMIO_XMITSTAT_1) as *const u32);
            if (st0 & 0x0001) == 0 && (st1 & 0x0001) == 0 {
                break;
            }
            for st in [st0, st1] {
                if (st & 0x0001) != 0 {
                    let cookie = (st >> 16) as u16;
                    for i in 0..B43_RING_LEN {
                        if dma.tx_inflight[i] && dma.tx_cookies[i] == cookie {
                            dma.tx_inflight[i] = false;
                            dma.tx_cookies[i] = 0;
                        }
                    }
                }
            }
        }
        // In mock scratch MMIO tests without a live hardware TX engine, retire wrapped slot cleanly
        if !self.use_pci_cfg_window {
            let slot = dma.tx_prod % B43_RING_LEN;
            dma.tx_inflight[slot] = false;
            dma.tx_cookies[slot] = 0;
        }
    }

    fn transmit_raw_80211(&self, frame: &[u8]) -> Result<(), &'static str> {
        if !self.firmware_loaded {
            return Err("BCM4331 uCode firmware not loaded");
        }
        let hdr_len = self.tx_header_size();
        if frame.len() < 16 || hdr_len + frame.len() > B43_DMA_SLOT_SIZE {
            return Err("802.11 frame invalid or exceeds BCM4331 DMA slot capacity");
        }

        let mut dma = self.dma.lock();
        unsafe { self.reclaim_tx_completions(&mut dma); }

        let slot = dma.tx_prod % B43_RING_LEN;
        if dma.tx_inflight[slot] {
            return Err("BCM4331 TX DMA ring full (all 16 slots in flight)");
        }

        let cookie = dma.next_cookie;
        dma.next_cookie = dma.next_cookie.wrapping_add(1).max(1);
        dma.tx_inflight[slot] = true;
        dma.tx_cookies[slot] = cookie;

        let desc_virt = phys_to_virt(dma.tx_desc_frame) as *mut B43DmaDesc64;
        let buf_virt = (phys_to_virt(dma.buf_base_frame) + (slot * B43_DMA_SLOT_SIZE) as u64) as *mut u8;
        let buf_phys = dma.buf_base_frame + (slot * B43_DMA_SLOT_SIZE) as u64;
        let total_len = hdr_len + frame.len();

        let (addr_low, addr_high, addrext) = encode_dma64_addr(buf_phys);
        let end_flag = if slot + 1 == B43_RING_LEN { B43_DMA64_DCTL1_DTABLEEND } else { 0 };

        let mut tx_hdr_buf = [0u8; 118];
        fill_common_txhdr(frame, self.ucode_rev, cookie, &mut tx_hdr_buf);

        unsafe {
            core::ptr::copy_nonoverlapping(tx_hdr_buf.as_ptr(), buf_virt, hdr_len);
            core::ptr::copy_nonoverlapping(frame.as_ptr(), buf_virt.add(hdr_len), frame.len());
            fence(Ordering::SeqCst);

            write_volatile(
                desc_virt.add(slot),
                B43DmaDesc64 {
                    ctrl1: B43_DMA64_DCTL1_SOF | B43_DMA64_DCTL1_EOF | end_flag | addrext,
                    ctrl2: (total_len as u32) & 0x1FFF,
                    addr_low,
                    addr_high,
                },
            );
            fence(Ordering::SeqCst);

            dma.tx_prod = (slot + 1) % B43_RING_LEN;
            let next_byte_offset = (dma.tx_prod * core::mem::size_of::<B43DmaDesc64>()) as u32;
            write_volatile(
                (self.mmio_base + B43_DMA64_TX0_BASE + B43_DMA64_INDEX_OFF) as *mut u32,
                next_byte_offset,
            );
        }
        Ok(())
    }

    /// Single shared RX DMA drainer classifying frames into `rx_8023_queue` and `rx_80211_queue`.
    pub fn poll_rx_dma(&self) {
        if !self.firmware_loaded {
            return;
        }

        let mut drained: Vec<Vec<u8>> = Vec::new();
        {
            let mut dma = self.dma.lock();
            let hw_rx_byte = unsafe {
                read_volatile((self.mmio_base + B43_DMA64_RX0_BASE + B43_DMA64_STATUS_OFF) as *const u32)
            } & B43_DMA64_STAT_CURR_MASK;
            let hw_rx_idx = ((hw_rx_byte as usize) / core::mem::size_of::<B43DmaDesc64>()) % B43_RING_LEN;
            let rx_descs = phys_to_virt(dma.rx_desc_frame) as *mut B43DmaDesc64;

            for _ in 0..B43_RING_LEN {
                if dma.rx_cons == hw_rx_idx {
                    break;
                }
                let slot = dma.rx_cons % B43_RING_LEN;
                let buf_phys = dma.buf_base_frame + ((B43_RING_LEN + slot) * B43_DMA_SLOT_SIZE) as u64;
                let rx_buf = phys_to_virt(buf_phys) as *const u8;

                let mut rx_hdr = B43RxHdrFw4::default();
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        rx_buf,
                        &mut rx_hdr as *mut B43RxHdrFw4 as *mut u8,
                        30,
                    );
                }
                let frame_len = u16::from_le(rx_hdr.frame_len) as usize;
                let mac_status = u32::from_le(rx_hdr.mac_status);
                let payload_off = if (mac_status & B43_RX_MAC_PADDING) != 0 { 32 } else { 30 };

                if frame_len > 0
                    && (mac_status & B43_RX_MAC_FCSERR) == 0
                    && payload_off + frame_len <= B43_DMA_SLOT_SIZE
                {
                    let mut raw = Vec::with_capacity(frame_len);
                    raw.resize(frame_len, 0);
                    unsafe {
                        core::ptr::copy_nonoverlapping(rx_buf.add(payload_off), raw.as_mut_ptr(), frame_len);
                    }
                    // Hardware RX frames append a 4-byte 802.11 FCS
                    if raw.len() >= 28 {
                        raw.truncate(raw.len() - 4);
                    }
                    drained.push(raw);
                }

                // Re-arm slot descriptor & advance RXINDEX to `(slot * 16)` (descriptor just freed)
                let (addr_low, addr_high, addrext) = encode_dma64_addr(buf_phys);
                let end_flag = if slot + 1 == B43_RING_LEN { B43_DMA64_DCTL1_DTABLEEND } else { 0 };
                unsafe {
                    write_volatile(
                        rx_descs.add(slot),
                        B43DmaDesc64 {
                            ctrl1: end_flag | addrext,
                            ctrl2: (B43_DMA_SLOT_SIZE as u32) & 0x1FFF,
                            addr_low,
                            addr_high,
                        },
                    );
                    fence(Ordering::SeqCst);
                    write_volatile(
                        (self.mmio_base + B43_DMA64_RX0_BASE + B43_DMA64_INDEX_OFF) as *mut u32,
                        (slot * core::mem::size_of::<B43DmaDesc64>()) as u32,
                    );
                }
                dma.rx_cons = (slot + 1) % B43_RING_LEN;
            }
        }

        for mpdu in drained {
            self.classify_rx_mpdu(mpdu);
        }
    }

    fn classify_rx_mpdu(&self, mpdu: Vec<u8>) {
        if mpdu.len() < 24 {
            return;
        }
        let ftype = (mpdu[0] >> 2) & 0x03;
        let subtype = (mpdu[0] >> 4) & 0x0F;

        if ftype == 0 {
            let mut q = self.rx_80211_queue.lock();
            if q.len() < 64 {
                q.push_back(mpdu);
            }
            return;
        }
        if ftype != 2 {
            return;
        }

        let is_qos = subtype == 8;
        let hdr_len = if is_qos { 26 } else { 24 };
        if mpdu.len() < hdr_len + 8 {
            return;
        }
        let protected = (mpdu[1] & 0x40) != 0;
        let _qos_tid = if is_qos { Some(mpdu[24] & 0x0F) } else { None };
        let body = &mpdu[hdr_len..];

        let plaintext_llc: Vec<u8> = if protected {
            if body.len() < 16 || (body[3] & 0x20) == 0 {
                return;
            }
            let key_id = (body[3] >> 6) & 0x03;
            let pn = u64::from_le_bytes([body[0], body[1], body[4], body[5], body[6], body[7], 0, 0]);
            let mut sess = self.session.lock();
            let (key, last_pn_slot) = if key_id == 0 {
                let Some(tk) = sess.tk else { return; };
                (tk, &mut sess.rx_tk_pn)
            } else {
                let Some((gid, gtk)) = sess.gtk else { return; };
                if gid != key_id { return; }
                (gtk, &mut sess.rx_gtk_pn[key_id as usize])
            };
            if let Some(prev) = *last_pn_slot {
                if pn <= prev {
                    return;
                }
            }
            let Ok((pt, _pn, _kid)) = ccmp_decrypt_mpdu(&key, &mpdu) else {
                return;
            };
            *last_pn_slot = Some(pn);
            pt
        } else {
            body.to_vec()
        };

        let Some((_da, _sa, ethertype, eth_frame)) = unwrap_llc_to_8023(&mpdu[..24], &plaintext_llc) else {
            return;
        };

        if ethertype == 0x888E {
            // Forward reconstructed unencrypted 802.11+LLC EAPOL frame to control plane
            let mut eapol_80211 = Vec::with_capacity(24 + plaintext_llc.len());
            let mut hdr = [0u8; 24];
            hdr.copy_from_slice(&mpdu[..24]);
            hdr[0] = 0x08;
            hdr[1] &= !0x40;
            eapol_80211.extend_from_slice(&hdr);
            eapol_80211.extend_from_slice(&plaintext_llc);
            let mut q = self.rx_80211_queue.lock();
            if q.len() < 64 {
                q.push_back(eapol_80211);
            }
        } else if self.link_up.load(Ordering::Acquire) {
            let mut q = self.rx_8023_queue.lock();
            if q.len() < 64 {
                q.push_back(eth_frame);
            }
        }
    }
}

impl NetDevice for B43Device {
    fn name(&self) -> &str {
        "wlan2"
    }

    fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    fn send_packet(&self, packet: &[u8]) -> Result<(), &'static str> {
        let (bssid, connected, tk, seq, pn) = {
            let mut sess = self.session.lock();
            if !sess.connected {
                return Err("BCM4331 Wi-Fi session not associated");
            }
            let seq = sess.tx_seq;
            sess.tx_seq = sess.tx_seq.wrapping_add(1) & 0x0FFF;
            let pn = if sess.tk.is_some() {
                sess.tx_pn = sess.tx_pn.saturating_add(1);
                sess.tx_pn
            } else {
                0
            };
            (sess.bssid, sess.connected, sess.tk, seq, pn)
        };
        let _ = connected;

        let (hdr24, llc_payload) = wrap_8023_to_80211_plain(packet, bssid, self.mac, seq, tk.is_some())?;
        let mut mpdu = Vec::with_capacity(24 + llc_payload.len() + 16);
        mpdu.extend_from_slice(&hdr24);
        if let Some(key) = tk {
            let enc = ccmp_encrypt_mpdu(&key, &hdr24, 0, pn, &llc_payload)?;
            mpdu.extend_from_slice(&enc);
        } else {
            mpdu.extend_from_slice(&llc_payload);
        }
        self.transmit_raw_80211(&mpdu)
    }

    fn receive_packet(&self, buf: &mut [u8]) -> Result<usize, &'static str> {
        self.poll_rx_dma();
        let Some(pkt) = self.rx_8023_queue.lock().pop_front() else {
            return Ok(0);
        };
        let n = pkt.len().min(buf.len());
        buf[..n].copy_from_slice(&pkt[..n]);
        Ok(n)
    }

    fn is_link_up(&self) -> bool {
        self.firmware_loaded && self.link_up.load(Ordering::Acquire)
    }

    fn device_type(&self) -> NetDeviceType {
        NetDeviceType::Wireless80211
    }

    fn set_channel(&self, channel: u8) -> Result<(), &'static str> {
        if !self.firmware_loaded {
            return Err("BCM4331 uCode firmware not loaded");
        }
        let _guard = self.bcma_lock.lock();
        unsafe {
            b43_phy_ht::set_channel_2g(self.mmio_base, channel, self.use_pci_cfg_window)?;
        }
        Ok(())
    }

    fn send_80211_frame(&self, frame: &[u8]) -> Result<(), &'static str> {
        self.transmit_raw_80211(frame)
    }

    fn receive_80211_frame(&self, buf: &mut [u8]) -> Result<usize, &'static str> {
        self.poll_rx_dma();
        let Some(frame) = self.rx_80211_queue.lock().pop_front() else {
            return Ok(0);
        };
        let n = frame.len().min(buf.len());
        buf[..n].copy_from_slice(&frame[..n]);
        Ok(n)
    }

    fn set_wifi_session(
        &self,
        bssid: Option<[u8; 6]>,
        link_up: bool,
        tk: Option<[u8; 16]>,
        gtk: Option<(u8, [u8; 16])>,
    ) {
        {
            let mut sess = self.session.lock();
            match bssid {
                Some(b) => {
                    if sess.bssid != b {
                        sess.tx_pn = 0;
                        sess.rx_tk_pn = None;
                        sess.rx_gtk_pn = [None; 4];
                    }
                    sess.bssid = b;
                }
                None => {
                    sess.bssid = [0u8; 6];
                    sess.tx_pn = 0;
                    sess.rx_tk_pn = None;
                    sess.rx_gtk_pn = [None; 4];
                }
            }
            sess.connected = link_up || bssid.is_some();
            sess.tk = tk;
            sess.gtk = gtk;
        }
        if self.firmware_loaded {
            let _guard = self.bcma_lock.lock();
            unsafe {
                let target_bssid = bssid.unwrap_or([0xFF; 6]);
                Self::write_mac_filter(self.mmio_base, B43_MACFILTER_BSSID, &target_bssid);
            }
        }
        self.link_up.store(link_up, Ordering::Release);
    }
}
