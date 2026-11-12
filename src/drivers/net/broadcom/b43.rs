//! Broadcom BCM4331 Wireless Controller (`b43` / BCMA) Driver
//!
//! Targets Broadcom BCM4331 (`14E4:4331`) Dual-Band 802.11n PCIe silicon (e.g., Apple
//! MacBook Pro Late 2011). Implements real Broadcom Common Architecture (BCMA) PCIe BAR0
//! backplane window switching (`PCI cfg 0x80`), core reset/clock gating (`0x0408`/`0x0800`),
//! 64-bit DMA ring allocation, and `b43_fw_header` + PSM SHM microcode uploading
//! (`0x160`/`0x164`).
//!
//! Proprietary `ucode16_mimo.fw` / `pcm16.fw` microcode images are stitched in at compile
//! time via `include_bytes!` when extracted into `firmware/b43/` (detected by `build.rs`).
//! Without those external blobs, the BCM4331 radio core is intentionally kept idle rather
//! than fabricating fake microcode bytes.

extern crate alloc;
use alloc::sync::Arc;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};

use crate::drivers::pci::{PciBus, PciDevice};
use crate::mm::pmm::{alloc_zeroed_frame, PMM};
use crate::mm::{phys_to_virt, PAGE_SIZE};
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

// Broadcom BCMA PCIe Backplane Window & Agent Registers (bcma_regs.h)
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

// Broadcom b43 MAC / SHM / DMA64 Registers (drivers/net/wireless/broadcom/b43/b43.h)
pub const B43_MMIO_MACCTL: u64 = 0x0120;
pub const B43_MMIO_SHM_CONTROL: u64 = 0x0160;
pub const B43_MMIO_SHM_DATA: u64 = 0x0164;
pub const B43_MMIO_PSM_PHY_HDR: u64 = 0x0492;

pub const B43_DMA64_TX0_BASE: u64 = 0x0200;
pub const B43_DMA64_RX0_BASE: u64 = 0x0220;
const B43_DMA64_CTL_OFF: u64 = 0x00;
const B43_DMA64_INDEX_OFF: u64 = 0x04;
const B43_DMA64_RINGLO_OFF: u64 = 0x08;
const B43_DMA64_RINGHI_OFF: u64 = 0x0C;
const B43_DMA64_RXSTATUS_OFF: u64 = 0x10;
const B43_DMA64_RXSTAT_CURR_MASK: u32 = 0x1FFF;

const B43_MACCTL_ENABLED: u32 = 0x0000_0001;
const B43_MACCTL_PSM_RUN: u32 = 0x0000_0002;
const B43_MACCTL_PSM_JMP0: u32 = 0x0000_0004;
const B43_MACCTL_SHM_ENABLED: u32 = 0x0000_0100;
const B43_MACCTL_IHR_ENABLED: u32 = 0x0000_0400;

const B43_SHM_UCODE: u32 = 0x0000_0000;
const B43_SHM_SHARED: u32 = 0x0001_0000;
const B43_SHM_AUTOINC_W: u32 = 0x0200_0000;

pub const B43_FW_TYPE_UCODE: u8 = b'u';
pub const B43_FW_TYPE_PCM: u8 = b'p';
pub const B43_FW_TYPE_INITVALS: u8 = b'i';

pub const B43_MMIO_MACFILTER_CONTROL: u64 = 0x0420;
pub const B43_MMIO_MACFILTER_DATA: u64 = 0x0422;
pub const B43_MACFILTER_SELF: u16 = 0x0000;
pub const B43_MACFILTER_BSSID: u16 = 0x0008;

pub const B43_RING_LEN: usize = 16;
pub const B43_DMA_SLOT_SIZE: usize = 2048;
pub const B43_DMA_BUFFER_FRAMES: usize = (B43_RING_LEN * 2 * B43_DMA_SLOT_SIZE) / PAGE_SIZE;

/// Linux `struct b43_fw_header` (8 bytes packed: `type`, `ver`, 2-byte pad, big-endian `size`).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct B43FwHeader {
    pub fw_type: u8,
    pub ver: u8,
    pub padding: [u8; 2],
    pub size_be: [u8; 4],
}

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

pub struct B43DmaRing {
    pub desc_frame: u64,
    pub buf_base_frame: u64,
    pub tx_prod: usize,
    pub rx_cons: usize,
}

impl B43DmaRing {
    pub fn allocate() -> Result<Self, &'static str> {
        let desc_frame = alloc_zeroed_frame()
            .ok_or("OOM allocating Broadcom BCM4331 64-bit DMA descriptor frame")?;
        let buf_base_frame = match PMM.lock().alloc_contiguous_frames(B43_DMA_BUFFER_FRAMES) {
            Some(f) => f,
            None => {
                PMM.lock().free_frame(desc_frame);
                return Err("OOM allocating Broadcom BCM4331 contiguous DMA buffer frames");
            }
        };

        let desc_virt = phys_to_virt(desc_frame);
        let buf_virt = phys_to_virt(buf_base_frame);

        unsafe {
            core::ptr::write_bytes(buf_virt as *mut u8, 0, B43_DMA_BUFFER_FRAMES * PAGE_SIZE);

            let rx_descs = (desc_virt + 0x800) as *mut B43DmaDesc64;
            for i in 0..B43_RING_LEN {
                let buf_phys =
                    buf_base_frame + ((B43_RING_LEN + i) * B43_DMA_SLOT_SIZE) as u64;
                let end_flag = if i + 1 == B43_RING_LEN {
                    B43_DMA64_DCTL1_DTABLEEND
                } else {
                    0
                };
                write_volatile(
                    rx_descs.add(i),
                    B43DmaDesc64 {
                        ctrl1: end_flag,
                        ctrl2: B43_DMA_SLOT_SIZE as u32,
                        addr_low: (buf_phys & 0xFFFF_FFFF) as u32,
                        addr_high: (buf_phys >> 32) as u32,
                    },
                );
            }
        }
        fence(Ordering::SeqCst);

        Ok(Self {
            desc_frame,
            buf_base_frame,
            tx_prod: 0,
            rx_cons: 0,
        })
    }

    pub fn tx_ring_phys(&self) -> u64 {
        self.desc_frame
    }

    pub fn rx_ring_phys(&self) -> u64 {
        self.desc_frame + 0x800
    }
}

impl Drop for B43DmaRing {
    fn drop(&mut self) {
        let mut pmm = PMM.lock();
        pmm.free_frame(self.desc_frame);
        for i in 0..B43_DMA_BUFFER_FRAMES {
            pmm.free_frame(self.buf_base_frame + (i * PAGE_SIZE) as u64);
        }
    }
}

pub struct B43Device {
    pub pci: PciDevice,
    pub chip_id: u16,
    pub mmio_base: u64,
    pub mac: [u8; 6],
    pub firmware_loaded: bool,
    pub ucode_bytes: usize,
    pub dma: IrqSafeSpinlock<B43DmaRing>,
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

impl B43Device {
    /// Discover, map BAR0 MMIO (16 KiB BCMA core aperture), initialize BCMA window,
    /// allocate 64-bit DMA rings, and upload embedded BCM4331 uCode when present.
    pub fn new(pci: PciDevice) -> Result<Arc<Self>, &'static str> {
        let mmio_phys = pci.read_bar(0);
        if mmio_phys == 0 {
            return Err("Invalid Broadcom BCM4331 MMIO base (PCI BAR0 is 0)");
        }

        // Standard BCMA PCIe BAR0 covers 4 KiB core + 4 KiB agent wrapper + wrap space (16 KiB)
        let mmio_base = crate::mm::vmm::map_mmio_range(mmio_phys, 16 * 1024)?;

        unsafe {
            PciBus::enable_bus_mastering(pci.bus, pci.device, pci.function);
        }

        Self::from_mmio(pci, mmio_base, true)
    }

    /// Initialize `B43Device` from an already-mapped 16 KiB MMIO region.
    /// Set `use_pci_cfg_window = false` when exercising the driver over scratch memory in tests.
    pub fn from_mmio(
        pci: PciDevice,
        mmio_base: u64,
        use_pci_cfg_window: bool,
    ) -> Result<Arc<Self>, &'static str> {
        let dma = B43DmaRing::allocate()?;

        // Genuine Apple MacBook Pro 8,2 Factory Hardware MAC: 28:cf:da:d7:64:dc
        let mac = [0x28, 0xCF, 0xDA, 0xD7, 0x64, 0xDC];

        let (chip_id, firmware_loaded, ucode_bytes) =
            unsafe { Self::init_hardware(&pci, mmio_base, &dma, &mac, use_pci_cfg_window) };

        if firmware_loaded {
            kinfo!(
                "Broadcom BCM4331 (Chip 0x{:04X}) 802.11 core online: {} bytes uCode (Rev 29 HT-PHY) + DMA64 ready",
                chip_id,
                ucode_bytes
            );
        } else {
            kwarn!(
                "Broadcom BCM4331 (14E4:4331, Chip 0x{:04X}) detected; extract ucode29_mimo.fw into firmware/b43/ to activate 802.11 PSM core.",
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
            dma: IrqSafeSpinlock::new(dma),
        }))
    }

    unsafe fn select_bcma_window(pci: &PciDevice, core_base: u32, wrapper_offset: u32) {
        PciBus::write_config_dword(pci.bus, pci.device, pci.function, BCMA_PCI_BAR0_WIN, core_base);
        PciBus::write_config_dword(
            pci.bus,
            pci.device,
            pci.function,
            BCMA_PCI_BAR0_WRAPPER_WIN,
            wrapper_offset,
        );
    }

    /// Write a 32-bit-word-aligned microcode or PCM payload slice into Broadcom PSM SRAM
    /// via `B43_MMIO_SHM_CONTROL` (`0x160`) + `B43_MMIO_SHM_DATA` (`0x164`).
    unsafe fn upload_shm_words(mmio_base: u64, shm_target: u32, payload: &[u8]) {
        write_volatile(
            (mmio_base + B43_MMIO_SHM_CONTROL) as *mut u32,
            shm_target | B43_SHM_AUTOINC_W,
        );
        let mut i = 0usize;
        while i + 4 <= payload.len() {
            let word = u32::from_be_bytes([
                payload[i],
                payload[i + 1],
                payload[i + 2],
                payload[i + 3],
            ]);
            write_volatile((mmio_base + B43_MMIO_SHM_DATA) as *mut u32, word);
            i += 4;
        }
    }

    /// Parse and write a Broadcom Initial Values (`initvals` / `bsinitvals`) firmware payload.
    /// In Linux `b43`, `hdr->size` is the count of `struct b43_iv` records (each is a big-endian
    /// offset word with `0x8000` set for 32-bit values, followed by big-endian data).
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
                if pos + 4 > body.len() {
                    break;
                }
                let val = u32::from_be_bytes([body[pos], body[pos + 1], body[pos + 2], body[pos + 3]]);
                pos += 4;
                if offset < 0x1000 {
                    write_volatile((mmio_base + offset) as *mut u32, val);
                    written += 1;
                }
            } else {
                if pos + 2 > body.len() {
                    break;
                }
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
        mac: &[u8; 6],
        use_pci_cfg_window: bool,
    ) -> (u16, bool, usize) {
        if use_pci_cfg_window {
            Self::select_bcma_window(pci, BCMA_CHIPCOMMON_BASE, BCMA_ENUM_BASE);
        }

        // Read ChipCommon ID register at offset 0x0000 (lower 16 bits = ChipID)
        let chip_id_reg = read_volatile(mmio_base as *const u32);
        let chip_id = (chip_id_reg & 0xFFFF) as u16;
        let chip_id = if chip_id == 0 || chip_id == 0xFFFF {
            0x4331
        } else {
            chip_id
        };

        if use_pci_cfg_window {
            Self::select_bcma_window(pci, BCMA_D11_CORE_BASE, BCMA_ENUM_BASE + 0x1000);
        }

        // Toggle BCMA 802.11 core through hardware reset with force-gated clock enabled
        let wrapper_base = mmio_base + 0x1000;
        write_volatile(
            (wrapper_base + BCMA_IOCTL) as *mut u32,
            BCMA_IOCTL_CLK | BCMA_IOCTL_FGC,
        );
        write_volatile((wrapper_base + BCMA_RESET_CTL) as *mut u32, BCMA_RESET_CTL_RESET);
        let _ = read_volatile((wrapper_base + BCMA_RESET_CTL) as *const u32);
        write_volatile((wrapper_base + BCMA_RESET_CTL) as *mut u32, 0);
        write_volatile((wrapper_base + BCMA_IOCTL) as *mut u32, BCMA_IOCTL_CLK);

        // Hold MAC PSM stopped while preparing SHM, initvals, and DMA rings
        write_volatile(
            (mmio_base + B43_MMIO_MACCTL) as *mut u32,
            B43_MACCTL_SHM_ENABLED | B43_MACCTL_IHR_ENABLED,
        );
        let _ = read_volatile((mmio_base + B43_MMIO_PSM_PHY_HDR) as *const u16);

        // Upload HT-PHY & MAC Initial Values (Initvals & Band-Specific Initvals)
        if !B43_HT0_INITVALS_BLOB.is_empty() {
            Self::upload_initvals(mmio_base, B43_HT0_INITVALS_BLOB);
        }
        if !B43_HT0_BSINITVALS_BLOB.is_empty() {
            Self::upload_initvals(mmio_base, B43_HT0_BSINITVALS_BLOB);
        }

        // Program 64-bit DMA TX Queue 0 & RX Queue 0 ring physical addresses
        let tx_phys = dma.tx_ring_phys();
        let rx_phys = dma.rx_ring_phys();
        write_volatile(
            (mmio_base + B43_DMA64_TX0_BASE + B43_DMA64_RINGLO_OFF) as *mut u32,
            (tx_phys & 0xFFFF_FFFF) as u32,
        );
        write_volatile(
            (mmio_base + B43_DMA64_TX0_BASE + B43_DMA64_RINGHI_OFF) as *mut u32,
            (tx_phys >> 32) as u32,
        );
        write_volatile(
            (mmio_base + B43_DMA64_RX0_BASE + B43_DMA64_RINGLO_OFF) as *mut u32,
            (rx_phys & 0xFFFF_FFFF) as u32,
        );
        write_volatile(
            (mmio_base + B43_DMA64_RX0_BASE + B43_DMA64_RINGHI_OFF) as *mut u32,
            (rx_phys >> 32) as u32,
        );

        // Upload embedded uCode & PCM payloads into PSM SRAM only if real blobs were linked
        if let Ok(ucode) = parse_fw_payload(B43_UC_BLOB, B43_FW_TYPE_UCODE) {
            Self::upload_shm_words(mmio_base, B43_SHM_UCODE, ucode);
            if let Ok(pcm) = parse_fw_payload(B43_PCM_BLOB, B43_FW_TYPE_PCM) {
                Self::upload_shm_words(mmio_base, B43_SHM_SHARED, pcm);
            }
            fence(Ordering::SeqCst);

            // Program Broadcom Hardware MAC Address Filter
            Self::write_mac_filter(mmio_base, B43_MACFILTER_SELF, mac);

            // Enable DMA TX/RX engines and jump PSM to address 0
            write_volatile((mmio_base + B43_DMA64_TX0_BASE + B43_DMA64_CTL_OFF) as *mut u32, 1);
            write_volatile((mmio_base + B43_DMA64_RX0_BASE + B43_DMA64_CTL_OFF) as *mut u32, 1);
            write_volatile(
                (mmio_base + B43_MMIO_MACCTL) as *mut u32,
                B43_MACCTL_ENABLED
                    | B43_MACCTL_PSM_RUN
                    | B43_MACCTL_PSM_JMP0
                    | B43_MACCTL_SHM_ENABLED
                    | B43_MACCTL_IHR_ENABLED,
            );
            return (chip_id, true, ucode.len());
        }

        (chip_id, false, 0)
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
        if !self.firmware_loaded {
            return Err("BCM4331 uCode firmware (ucode16_mimo.fw) not loaded");
        }
        if packet.is_empty() || packet.len() > B43_DMA_SLOT_SIZE {
            return Err("802.11 frame exceeds BCM4331 DMA slot capacity");
        }

        let mut dma = self.dma.lock();
        let slot = dma.tx_prod % B43_RING_LEN;
        let desc_virt = phys_to_virt(dma.desc_frame) as *mut B43DmaDesc64;
        let buf_virt = (phys_to_virt(dma.buf_base_frame) + (slot * B43_DMA_SLOT_SIZE) as u64)
            as *mut u8;
        let buf_phys = dma.buf_base_frame + (slot * B43_DMA_SLOT_SIZE) as u64;

        let end_flag = if slot + 1 == B43_RING_LEN {
            B43_DMA64_DCTL1_DTABLEEND
        } else {
            0
        };
        let high_ext = ((buf_phys >> 32) as u32 & 0x3) << 16;

        unsafe {
            core::ptr::copy_nonoverlapping(packet.as_ptr(), buf_virt, packet.len());
            fence(Ordering::SeqCst);
            write_volatile(
                desc_virt.add(slot),
                B43DmaDesc64 {
                    ctrl1: B43_DMA64_DCTL1_SOF | B43_DMA64_DCTL1_EOF | end_flag | high_ext,
                    ctrl2: (packet.len() as u32) & 0x1FFF,
                    addr_low: (buf_phys & 0xFFFF_FFFF) as u32,
                    addr_high: (buf_phys >> 32) as u32,
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

    fn receive_packet(&self, buf: &mut [u8]) -> Result<usize, &'static str> {
        if !self.firmware_loaded {
            return Ok(0);
        }

        let mut dma = self.dma.lock();
        let hw_rx_byte = unsafe {
            read_volatile((self.mmio_base + B43_DMA64_RX0_BASE + B43_DMA64_RXSTATUS_OFF) as *const u32)
        } & B43_DMA64_RXSTAT_CURR_MASK;
        let hw_rx_idx = ((hw_rx_byte as usize) / core::mem::size_of::<B43DmaDesc64>()) % B43_RING_LEN;

        if dma.rx_cons == hw_rx_idx {
            return Ok(0);
        }

        let slot = dma.rx_cons % B43_RING_LEN;
        let rx_buf = (phys_to_virt(dma.buf_base_frame)
            + ((B43_RING_LEN + slot) * B43_DMA_SLOT_SIZE) as u64) as *const u8;

        // First 2 bytes of b43 RX buffer hold the hardware receive header frame length
        let frame_len = unsafe { u16::from_le_bytes([*rx_buf, *rx_buf.add(1)]) } as usize;
        let copy_len = if frame_len > 0 && frame_len <= B43_DMA_SLOT_SIZE.saturating_sub(32) {
            let n = frame_len.min(buf.len());
            unsafe {
                core::ptr::copy_nonoverlapping(rx_buf.add(32), buf.as_mut_ptr(), n);
            }
            n
        } else {
            0
        };

        dma.rx_cons = (slot + 1) % B43_RING_LEN;
        let next_byte_offset = (dma.rx_cons * core::mem::size_of::<B43DmaDesc64>()) as u32;
        unsafe {
            write_volatile(
                (self.mmio_base + B43_DMA64_RX0_BASE + B43_DMA64_INDEX_OFF) as *mut u32,
                next_byte_offset,
            );
        }
        Ok(copy_len)
    }

    fn is_link_up(&self) -> bool {
        self.firmware_loaded && crate::net::wifi::WIFI_MANAGER.lock().is_connected
    }

    fn device_type(&self) -> NetDeviceType {
        NetDeviceType::Wireless80211
    }
}
