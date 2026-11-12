//! Broadcom BCM57765 / NetXtreme (Tigon3 / tg3) Gigabit Ethernet Driver
//!
//! Pure `#![no_std]` bare-metal driver targeting the Broadcom NetXtreme BCM57765
//! (`14E4:16B4`, subsystem `14E4:16B0`) and closely related BCM57761/57766/57781/57785 silicon.
//! Uses PCI BAR0 MMIO, Host Coalescing DMA Status Block, Send/Receive Producer/Return rings,
//! and GRC high-priority doorbell mailboxes without external runtime firmware blobs.

extern crate alloc;
use alloc::sync::Arc;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};

use crate::drivers::pci::{PciBus, PciDevice};
use crate::mm::pmm::{alloc_zeroed_frame, PMM};
use crate::mm::{phys_to_virt, PAGE_SIZE};
use crate::net::device::{NetDevice, NetDeviceType};
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

pub const TG3_PCI_VENDOR: u16 = 0x14E4;
pub const TG3_BCM57761_DEV: u16 = 0x16B0;
pub const TG3_BCM57781_DEV: u16 = 0x16B1;
pub const TG3_BCM57765_DEV: u16 = 0x16B4;
pub const TG3_BCM57785_DEV: u16 = 0x16B5;
pub const TG3_BCM57762_DEV: u16 = 0x1682;
pub const TG3_BCM57766_DEV: u16 = 0x1686;

/// Check whether a Broadcom PCI device ID belongs to the supported BCM57765/tg3 family.
pub fn is_supported_device(device_id: u16) -> bool {
    matches!(
        device_id,
        TG3_BCM57765_DEV
            | TG3_BCM57761_DEV
            | TG3_BCM57781_DEV
            | TG3_BCM57785_DEV
            | TG3_BCM57762_DEV
            | TG3_BCM57766_DEV
    )
}

/// Human-readable silicon name for supported Tigon3 models.
pub fn model_name(device_id: u16) -> &'static str {
    match device_id {
        TG3_BCM57765_DEV => "Broadcom NetXtreme BCM57765 Gigabit Ethernet",
        TG3_BCM57761_DEV => "Broadcom NetXtreme BCM57761 Gigabit Ethernet",
        TG3_BCM57781_DEV => "Broadcom NetXtreme BCM57781 Gigabit Ethernet",
        TG3_BCM57785_DEV => "Broadcom NetXtreme BCM57785 Gigabit Ethernet",
        TG3_BCM57762_DEV => "Broadcom NetXtreme BCM57762 Gigabit Ethernet",
        TG3_BCM57766_DEV => "Broadcom NetXtreme BCM57766 Gigabit Ethernet",
        _ => "Broadcom NetXtreme Tigon3 Gigabit Ethernet",
    }
}

// PCI Config / MMIO Window Register Offsets (per Broadcom 57785-PG106 & Linux tg3.h)
pub const TG3_MISC_HOST_CTRL: u64 = 0x0068;
#[allow(dead_code)]
pub const TG3_DMA_RW_CTRL: u64 = 0x006C;
pub const TG3_MEM_WIN_BASE_ADDR: u64 = 0x007C;

// MISC_HOST_CTRL bits
const MISC_HOST_CTRL_MASK_PCI_INT: u32 = 1 << 0;
const MISC_HOST_CTRL_WORD_SWAP: u32 = 1 << 1;
const MISC_HOST_CTRL_ENABLE_PCI_STATE_RW: u32 = 1 << 4;
const MISC_HOST_CTRL_INDIR_ACCESS: u32 = 1 << 7;

// High-Priority GRC Mailbox Offsets (+0x04 selects the low 32-bit word on LE x86_64)
pub const TG3_GRCMBOX_INTERRUPT_0_LO: u64 = 0x5804;
pub const TG3_GRCMBOX_RCV_STD_PROD_LO: u64 = 0x586C;
pub const TG3_GRCMBOX_RCVRET_CON_0_LO: u64 = 0x5884;
pub const TG3_GRCMBOX_SNDHOST_PROD_0_LO: u64 = 0x5904;

// MAC, DMA, Buffer Manager, Host Coalescing & GRC Registers
pub const TG3_MAC_MODE: u64 = 0x0400;
pub const TG3_MAC_STATUS: u64 = 0x0404;
pub const TG3_MAC_ADDR_0_HIGH: u64 = 0x0410;
pub const TG3_MAC_ADDR_0_LOW: u64 = 0x0414;
pub const TG3_MAC_TX_MODE: u64 = 0x0458;
pub const TG3_MAC_RX_MODE: u64 = 0x0468;
pub const TG3_SNDDATAI_MODE: u64 = 0x0C00;

pub const TG3_RCVDBDI_STD_BD_HI: u64 = 0x2450;
pub const TG3_RCVDBDI_STD_BD_LO: u64 = 0x2454;
pub const TG3_RCVDBDI_STD_BD_FLAGS: u64 = 0x2458;
pub const TG3_RCVDBDI_STD_BD_NIC: u64 = 0x245C;

pub const TG3_HOSTCC_MODE: u64 = 0x3C00;
pub const TG3_HOSTCC_STATUS_BLK_HI: u64 = 0x3C38;
pub const TG3_HOSTCC_STATUS_BLK_LO: u64 = 0x3C3C;

pub const TG3_MEMARB_MODE: u64 = 0x4000;
pub const TG3_BUFMGR_MODE: u64 = 0x4400;
pub const TG3_RDMAC_MODE: u64 = 0x4800;
pub const TG3_WDMAC_MODE: u64 = 0x4C00;
pub const TG3_GRC_MODE: u64 = 0x6800;
#[allow(dead_code)]
pub const TG3_GRC_MISC_CFG: u64 = 0x6804;
pub const TG3_DMA_RW_MODE: u64 = 0x6C00;

// NIC SRAM Direct Window (when TG3_MEM_WIN_BASE_ADDR == 0x0000)
const TG3_NIC_SRAM_WIN_BASE: u64 = 0x8000;
const TG3_NIC_SRAM_SEND_RCB_0: u64 = 0x0100;
const TG3_NIC_SRAM_RCV_RET_RCB_0: u64 = 0x0200;

// Key Register Flags
const MAC_STATUS_PCS_SYNCED: u32 = 1 << 0;
const TXD_LEN_SHIFT: u32 = 16;
const TXD_FLAG_END: u32 = 0x0000_0004;
const RXD_LEN_MASK: u32 = 0xFFFF;
const RXD_ERR_MASK: u32 = 0xFFFF_0000;
const BDINFO_FLAG_USE_EXT_RECV_BD: u32 = 0x0000_0001;

/// 16 TX + 16 RX packet slots * 2048 bytes = 64 KiB = 16 contiguous PMM frames.
pub const TG3_RING_LEN: usize = 16;
pub const TG3_DMA_SLOT_SIZE: usize = 2048;
pub const TG3_DMA_BUFFER_FRAMES: usize = (TG3_RING_LEN * 2 * TG3_DMA_SLOT_SIZE) / PAGE_SIZE;

/// Tigon3 Hardware Status Block index pair (little-endian layout matching `struct tg3_hw_statidx`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Tg3StatIdx {
    pub rx_producer: u16,
    pub tx_consumer: u16,
}

/// Tigon3 Host Coalescing Hardware Status Block (`struct tg3_hw_status`), 64-byte aligned.
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy)]
pub struct Tg3StatusBlock {
    pub status: u32,
    pub status_tag: u32,
    pub rx_consumer: u16,
    pub rx_jumbo_consumer: u16,
    pub reserved: u16,
    pub rx_mini_consumer: u16,
    pub idx: [Tg3StatIdx; 16],
}

/// Tigon3 Transmit Buffer Descriptor (`struct tg3_tx_buffer_desc`, 16 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Tg3TxDesc {
    pub addr_hi: u32,
    pub addr_lo: u32,
    pub len_flags: u32,
    pub vlan_tag: u32,
}

/// Tigon3 Receive Buffer Descriptor (`struct tg3_rx_buffer_desc`, 32 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Tg3RxDesc {
    pub addr_hi: u32,
    pub addr_lo: u32,
    pub idx_len: u32,
    pub type_flags: u32,
    pub err_vlan: u32,
    pub ip_tcp_csum: u32,
    pub reserved: u32,
    pub opaque: u32,
}

/// Physical DMA frames & ring indices owned by a Broadcom Tigon3 device.
pub struct Tg3DmaState {
    pub status_block_frame: u64,
    pub tx_ring_frame: u64,
    pub rx_prod_ring_frame: u64,
    pub rx_ret_ring_frame: u64,
    pub buf_base_frame: u64,
    pub tx_prod_idx: u32,
    pub rx_prod_idx: u32,
    pub rx_ret_cons_idx: u32,
}

impl Tg3DmaState {
    /// Allocate zeroed physical frames via PMM for Status Block, TX/RX/Return rings,
    /// and pre-arm all Standard RX Producer descriptors with physical 2 KiB slots.
    pub fn allocate() -> Result<Self, &'static str> {
        let status_block_frame = alloc_zeroed_frame()
            .ok_or("OOM allocating BCM57765 DMA Status Block frame")?;

        let tx_ring_frame = match alloc_zeroed_frame() {
            Some(f) => f,
            None => {
                PMM.lock().free_frame(status_block_frame);
                return Err("OOM allocating BCM57765 TX Ring frame");
            }
        };

        let rx_prod_ring_frame = match alloc_zeroed_frame() {
            Some(f) => f,
            None => {
                let mut pmm = PMM.lock();
                pmm.free_frame(status_block_frame);
                pmm.free_frame(tx_ring_frame);
                return Err("OOM allocating BCM57765 RX Producer Ring frame");
            }
        };

        let rx_ret_ring_frame = match alloc_zeroed_frame() {
            Some(f) => f,
            None => {
                let mut pmm = PMM.lock();
                pmm.free_frame(status_block_frame);
                pmm.free_frame(tx_ring_frame);
                pmm.free_frame(rx_prod_ring_frame);
                return Err("OOM allocating BCM57765 RX Return Ring frame");
            }
        };

        let buf_base_frame = match PMM.lock().alloc_contiguous_frames(TG3_DMA_BUFFER_FRAMES) {
            Some(f) => f,
            None => {
                let mut pmm = PMM.lock();
                pmm.free_frame(status_block_frame);
                pmm.free_frame(tx_ring_frame);
                pmm.free_frame(rx_prod_ring_frame);
                pmm.free_frame(rx_ret_ring_frame);
                return Err("OOM allocating BCM57765 contiguous DMA packet buffers");
            }
        };

        unsafe {
            core::ptr::write_bytes(
                phys_to_virt(buf_base_frame) as *mut u8,
                0,
                TG3_DMA_BUFFER_FRAMES * PAGE_SIZE,
            );

            let rx_prod_ptr = phys_to_virt(rx_prod_ring_frame) as *mut Tg3RxDesc;
            for i in 0..TG3_RING_LEN {
                let buf_phys = buf_base_frame + ((TG3_RING_LEN + i) * TG3_DMA_SLOT_SIZE) as u64;
                write_volatile(
                    rx_prod_ptr.add(i),
                    Tg3RxDesc {
                        addr_hi: (buf_phys >> 32) as u32,
                        addr_lo: (buf_phys & 0xFFFF_FFFF) as u32,
                        idx_len: (i as u32) << 16 | (TG3_DMA_SLOT_SIZE as u32),
                        type_flags: 0,
                        err_vlan: 0,
                        ip_tcp_csum: 0,
                        reserved: 0,
                        opaque: i as u32,
                    },
                );
            }
        }
        fence(Ordering::SeqCst);

        Ok(Self {
            status_block_frame,
            tx_ring_frame,
            rx_prod_ring_frame,
            rx_ret_ring_frame,
            buf_base_frame,
            tx_prod_idx: 0,
            rx_prod_idx: (TG3_RING_LEN - 1) as u32,
            rx_ret_cons_idx: 0,
        })
    }

    pub fn tx_slot_virt(&self, idx: usize) -> *mut u8 {
        (phys_to_virt(self.buf_base_frame) + ((idx % TG3_RING_LEN) * TG3_DMA_SLOT_SIZE) as u64)
            as *mut u8
    }

    pub fn tx_slot_phys(&self, idx: usize) -> u64 {
        self.buf_base_frame + ((idx % TG3_RING_LEN) * TG3_DMA_SLOT_SIZE) as u64
    }

    pub fn rx_slot_virt(&self, idx: usize) -> *const u8 {
        (phys_to_virt(self.buf_base_frame)
            + ((TG3_RING_LEN + (idx % TG3_RING_LEN)) * TG3_DMA_SLOT_SIZE) as u64)
            as *const u8
    }

    pub fn rx_slot_phys(&self, idx: usize) -> u64 {
        self.buf_base_frame + ((TG3_RING_LEN + (idx % TG3_RING_LEN)) * TG3_DMA_SLOT_SIZE) as u64
    }

    pub fn status_block(&self) -> *const Tg3StatusBlock {
        phys_to_virt(self.status_block_frame) as *const Tg3StatusBlock
    }
}

impl Drop for Tg3DmaState {
    fn drop(&mut self) {
        let mut pmm = PMM.lock();
        pmm.free_frame(self.status_block_frame);
        pmm.free_frame(self.tx_ring_frame);
        pmm.free_frame(self.rx_prod_ring_frame);
        pmm.free_frame(self.rx_ret_ring_frame);
        for i in 0..TG3_DMA_BUFFER_FRAMES {
            pmm.free_frame(self.buf_base_frame + (i * PAGE_SIZE) as u64);
        }
    }
}

pub struct Tg3Device {
    pub pci: PciDevice,
    pub chip_model: &'static str,
    pub mmio_base: u64,
    pub mac: [u8; 6],
    pub dma: IrqSafeSpinlock<Tg3DmaState>,
}

unsafe impl Send for Tg3Device {}
unsafe impl Sync for Tg3Device {}

impl Tg3Device {
    /// Discover, map BAR0 MMIO (64 KiB), allocate PMM DMA structures, and initialize
    /// a Broadcom BCM57765 / NetXtreme controller discovered on PCI.
    pub fn new(pci: PciDevice) -> Result<Arc<Self>, &'static str> {
        let mmio_phys = pci.read_bar(0);
        if mmio_phys == 0 {
            return Err("Invalid Broadcom BCM57765 MMIO base (PCI BAR0 is 0)");
        }

        // BCM57765 exposes a 64 KiB BAR0 window (registers 0x0000..0x7FFF, SRAM win 0x8000..0xFFFF)
        let mmio_base = crate::mm::vmm::map_mmio_range(mmio_phys, 64 * 1024)?;

        unsafe {
            PciBus::enable_bus_mastering(pci.bus, pci.device, pci.function);
        }

        Self::from_mmio(pci, mmio_base)
    }

    /// Initialize a `Tg3Device` over a pre-mapped 64 KiB register/SRAM MMIO aperture.
    pub fn from_mmio(pci: PciDevice, mmio_base: u64) -> Result<Arc<Self>, &'static str> {
        let dma = Tg3DmaState::allocate()?;
        let chip_model = model_name(pci.device_id);

        let mac = unsafe { Self::init_hardware(mmio_base, &dma, &pci) };

        kinfo!(
            "{} DMA rings online (MMIO: 0x{:X}, SB: 0x{:X}, TX: 0x{:X}, RX: 0x{:X})",
            chip_model,
            mmio_base,
            dma.status_block_frame,
            dma.tx_ring_frame,
            dma.rx_prod_ring_frame
        );

        Ok(Arc::new(Self {
            pci,
            chip_model,
            mmio_base,
            mac,
            dma: IrqSafeSpinlock::new(dma),
        }))
    }

    #[inline(always)]
    unsafe fn reg_read32(mmio: u64, offset: u64) -> u32 {
        read_volatile((mmio + offset) as *const u32)
    }

    #[inline(always)]
    unsafe fn reg_write32(mmio: u64, offset: u64, val: u32) {
        write_volatile((mmio + offset) as *mut u32, val);
    }

    /// Perform PCIe-posted write-flush via read-back on MISC_HOST_CTRL / target mailbox.
    #[inline(always)]
    unsafe fn reg_write32_flush(mmio: u64, offset: u64, val: u32) {
        Self::reg_write32(mmio, offset, val);
        let _ = Self::reg_read32(mmio, offset);
    }

    /// Initialize Tigon3 host endianness, MAC address readback, Status Block,
    /// Ring Control Blocks, DMA engines, and doorbell mailboxes.
    unsafe fn init_hardware(mmio: u64, dma: &Tg3DmaState, pci: &PciDevice) -> [u8; 6] {
        // 1. Enable Little-Endian Word Swapping + Indirect Access + Mask PCI Interrupts
        let mut host_ctrl = Self::reg_read32(mmio, TG3_MISC_HOST_CTRL);
        host_ctrl |= MISC_HOST_CTRL_MASK_PCI_INT
            | MISC_HOST_CTRL_WORD_SWAP
            | MISC_HOST_CTRL_ENABLE_PCI_STATE_RW
            | MISC_HOST_CTRL_INDIR_ACCESS;
        Self::reg_write32_flush(mmio, TG3_MISC_HOST_CTRL, host_ctrl);

        // 2. Read hardware MAC address from MAC_ADDR_0_HIGH / LOW
        let hi = Self::reg_read32(mmio, TG3_MAC_ADDR_0_HIGH);
        let lo = Self::reg_read32(mmio, TG3_MAC_ADDR_0_LOW);
        let mut mac = [
            ((hi >> 8) & 0xFF) as u8,
            (hi & 0xFF) as u8,
            ((lo >> 24) & 0xFF) as u8,
            ((lo >> 16) & 0xFF) as u8,
            ((lo >> 8) & 0xFF) as u8,
            (lo & 0xFF) as u8,
        ];

        if mac == [0; 6] || mac == [0xFF; 6] {
            // Locally administered fallback when uninitialized MMIO reports all-0 or all-F
            mac = [0x02, 0x14, 0xE4, pci.bus, pci.device, 0x65];
        }

        // 3. Configure GRC Mode & DMA Read/Write Control (host-coherent 64-bit DMA, little-endian)
        let grc_mode = Self::reg_read32(mmio, TG3_GRC_MODE);
        Self::reg_write32(mmio, TG3_GRC_MODE, grc_mode | 0x0000_000E);
        Self::reg_write32(mmio, TG3_DMA_RW_MODE, 0x761B_4000);

        // 4. Program Host Coalescing Status Block physical host address
        Self::reg_write32(
            mmio,
            TG3_HOSTCC_STATUS_BLK_HI,
            (dma.status_block_frame >> 32) as u32,
        );
        Self::reg_write32(
            mmio,
            TG3_HOSTCC_STATUS_BLK_LO,
            (dma.status_block_frame & 0xFFFF_FFFF) as u32,
        );

        // 5. Program Standard Receive Producer Ring BD Info (RCVDBDI_STD_BD)
        Self::reg_write32(
            mmio,
            TG3_RCVDBDI_STD_BD_HI,
            (dma.rx_prod_ring_frame >> 32) as u32,
        );
        Self::reg_write32(
            mmio,
            TG3_RCVDBDI_STD_BD_LO,
            (dma.rx_prod_ring_frame & 0xFFFF_FFFF) as u32,
        );
        Self::reg_write32(
            mmio,
            TG3_RCVDBDI_STD_BD_FLAGS,
            ((TG3_RING_LEN as u32) << 16) | BDINFO_FLAG_USE_EXT_RECV_BD,
        );
        Self::reg_write32(mmio, TG3_RCVDBDI_STD_BD_NIC, 0x0000_6000);

        // 6. Map NIC SRAM direct window to offset 0 and write Send & RX Return RCBs
        Self::reg_write32(mmio, TG3_MEM_WIN_BASE_ADDR, 0x0000_0000);

        let send_rcb = TG3_NIC_SRAM_WIN_BASE + TG3_NIC_SRAM_SEND_RCB_0;
        Self::reg_write32(mmio, send_rcb, (dma.tx_ring_frame >> 32) as u32);
        Self::reg_write32(mmio, send_rcb + 4, (dma.tx_ring_frame & 0xFFFF_FFFF) as u32);
        Self::reg_write32(mmio, send_rcb + 8, (TG3_RING_LEN as u32) << 16);
        Self::reg_write32(mmio, send_rcb + 12, 0x0000_4000);

        let rx_ret_rcb = TG3_NIC_SRAM_WIN_BASE + TG3_NIC_SRAM_RCV_RET_RCB_0;
        Self::reg_write32(mmio, rx_ret_rcb, (dma.rx_ret_ring_frame >> 32) as u32);
        Self::reg_write32(
            mmio,
            rx_ret_rcb + 4,
            (dma.rx_ret_ring_frame & 0xFFFF_FFFF) as u32,
        );
        Self::reg_write32(mmio, rx_ret_rcb + 8, (TG3_RING_LEN as u32) << 16);
        Self::reg_write32(mmio, rx_ret_rcb + 12, 0);

        // 7. Reset Producer/Consumer Mailboxes, then announce pre-armed RX Producer descriptors
        Self::reg_write32_flush(mmio, TG3_GRCMBOX_SNDHOST_PROD_0_LO, 0);
        Self::reg_write32_flush(mmio, TG3_GRCMBOX_RCVRET_CON_0_LO, 0);
        Self::reg_write32_flush(mmio, TG3_GRCMBOX_RCV_STD_PROD_LO, dma.rx_prod_idx);

        // 8. Enable Memory Arbiter, Buffer Manager, Read/Write DMA, Send Data Initiator,
        //    Host Coalescing Engine, and MAC RX/TX datapaths
        Self::reg_write32(mmio, TG3_MEMARB_MODE, 0x0000_0002);
        Self::reg_write32(mmio, TG3_BUFMGR_MODE, 0x0000_0002);
        Self::reg_write32(mmio, TG3_WDMAC_MODE, 0x0000_0002);
        Self::reg_write32(mmio, TG3_RDMAC_MODE, 0x0000_0002);
        Self::reg_write32(mmio, TG3_SNDDATAI_MODE, 0x0000_0002);
        Self::reg_write32(mmio, TG3_HOSTCC_MODE, 0x0000_0002);

        Self::reg_write32(mmio, TG3_MAC_MODE, 0x0000_0008); // GMII / Gigabit port mode
        Self::reg_write32(mmio, TG3_MAC_RX_MODE, 0x0000_0002); // Enable MAC RX
        Self::reg_write32(mmio, TG3_MAC_TX_MODE, 0x0000_0002); // Enable MAC TX

        // 9. Acknowledge interrupt mailbox 0
        Self::reg_write32_flush(mmio, TG3_GRCMBOX_INTERRUPT_0_LO, 0);

        mac
    }
}

impl NetDevice for Tg3Device {
    fn name(&self) -> &str {
        "en3"
    }

    fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    fn send_packet(&self, packet: &[u8]) -> Result<(), &'static str> {
        if packet.is_empty() || packet.len() > TG3_DMA_SLOT_SIZE {
            return Err("Ethernet frame exceeds BCM57765 2 KiB DMA slot capacity");
        }

        let mut dma = self.dma.lock();
        let slot = (dma.tx_prod_idx as usize) % TG3_RING_LEN;
        let tx_ring_virt = phys_to_virt(dma.tx_ring_frame) as *mut Tg3TxDesc;
        let buf_virt = dma.tx_slot_virt(slot);
        let buf_phys = dma.tx_slot_phys(slot);

        unsafe {
            core::ptr::copy_nonoverlapping(packet.as_ptr(), buf_virt, packet.len());
            fence(Ordering::SeqCst);

            write_volatile(
                tx_ring_virt.add(slot),
                Tg3TxDesc {
                    addr_hi: (buf_phys >> 32) as u32,
                    addr_lo: (buf_phys & 0xFFFF_FFFF) as u32,
                    len_flags: ((packet.len() as u32) << TXD_LEN_SHIFT) | TXD_FLAG_END,
                    vlan_tag: 0,
                },
            );
            fence(Ordering::SeqCst);

            let next_prod = ((slot + 1) % TG3_RING_LEN) as u32;
            dma.tx_prod_idx = next_prod;
            Self::reg_write32_flush(self.mmio_base, TG3_GRCMBOX_SNDHOST_PROD_0_LO, next_prod);
        }

        Ok(())
    }

    fn receive_packet(&self, buf: &mut [u8]) -> Result<usize, &'static str> {
        let mut dma = self.dma.lock();

        // Poll Status Block updated asynchronously by BCM57765 Host Coalescing DMA
        let hw_rx_ret_prod = unsafe { read_volatile(&(*dma.status_block()).idx[0].rx_producer) } as u32
            % TG3_RING_LEN as u32;

        if dma.rx_ret_cons_idx == hw_rx_ret_prod {
            return Ok(0);
        }

        let ret_idx = (dma.rx_ret_cons_idx as usize) % TG3_RING_LEN;
        let ret_ring_virt = phys_to_virt(dma.rx_ret_ring_frame) as *const Tg3RxDesc;
        let desc = unsafe { read_volatile(ret_ring_virt.add(ret_idx)) };

        let pkt_len = ((desc.idx_len & RXD_LEN_MASK) as usize).min(TG3_DMA_SLOT_SIZE);
        let copy_len = if (desc.err_vlan & RXD_ERR_MASK) == 0 && pkt_len > 0 {
            let slot = (desc.opaque as usize) % TG3_RING_LEN;
            let n = pkt_len.min(buf.len());
            unsafe {
                core::ptr::copy_nonoverlapping(dma.rx_slot_virt(slot), buf.as_mut_ptr(), n);
            }
            n
        } else {
            0
        };

        // Advance RX Return consumer index and recycle RX Producer descriptor slot
        let next_ret_cons = ((ret_idx + 1) % TG3_RING_LEN) as u32;
        dma.rx_ret_cons_idx = next_ret_cons;

        let next_rx_prod = ((dma.rx_prod_idx as usize + 1) % TG3_RING_LEN) as u32;
        dma.rx_prod_idx = next_rx_prod;

        unsafe {
            Self::reg_write32_flush(self.mmio_base, TG3_GRCMBOX_RCVRET_CON_0_LO, next_ret_cons);
            Self::reg_write32_flush(self.mmio_base, TG3_GRCMBOX_RCV_STD_PROD_LO, next_rx_prod);
        }

        Ok(copy_len)
    }

    fn is_link_up(&self) -> bool {
        let status = unsafe { Self::reg_read32(self.mmio_base, TG3_MAC_STATUS) };
        (status & MAC_STATUS_PCS_SYNCED) != 0
    }

    fn device_type(&self) -> NetDeviceType {
        NetDeviceType::Ethernet
    }
}
