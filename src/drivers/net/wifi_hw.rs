extern crate alloc;
use alloc::string::String;
use alloc::sync::Arc;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};
use crate::drivers::pci::{PciBus, PciDevice};
use crate::mm::pmm::{alloc_zeroed_frame, PMM};
use crate::mm::{phys_to_virt, PAGE_SIZE};
use crate::net::device::{NetDevice, NetDeviceType};
use crate::sync::IrqSafeSpinlock;
use crate::{kinfo, kwarn};

/// 16 TX + 16 RX 802.11 hardware DMA descriptors & packet buffers (16 * 2048 * 2 = 64 KiB = 16 pages)
pub const WIFI_RING_LEN: usize = 16;
pub const WIFI_DMA_SLOT_SIZE: usize = 2048;
pub const WIFI_DMA_BUFFER_FRAMES: usize = (WIFI_RING_LEN * 2 * WIFI_DMA_SLOT_SIZE) / PAGE_SIZE; // 16 pages

const DESC_OWN_HW: u32 = 1 << 31;
const DESC_FIRST_SEG: u32 = 1 << 27;
const DESC_LAST_SEG: u32 = 1 << 26;
const RX_LEN_MASK: u32 = 0x3FFF;

// Realtek RTL8188EE / RTL8192CE PCIe MMIO Register Offsets (BAR2 / BAR0 MMIO)
const RTL_REG_SYS_FUNC_EN: u64 = 0x0002;
const RTL_REG_CR: u64 = 0x0100;
const RTL_REG_MGQ_DESA: u64 = 0x0318;
const RTL_REG_BEQ_DESA: u64 = 0x0330;
const RTL_REG_RXQ_DESA: u64 = 0x0340;
const RTL_REG_PCIE_CTRL_INT_MIG: u64 = 0x0300;
const RTL_REG_TCR: u64 = 0x0604;
const RTL_REG_RCR: u64 = 0x0608;
const RTL_REG_MACID: u64 = 0x0610;

// Qualcomm Atheros ath9k PCIe MMIO Register Offsets (BAR0 MMIO)
const ATH_AR_CR: u64 = 0x0008;
const ATH_AR_RXDP: u64 = 0x0078;
const ATH_AR_Q0_TXDP: u64 = 0x0800;
const ATH_AR_Q_TXE: u64 = 0x0840;
const ATH_AR_STA_ID0: u64 = 0x8000;
const ATH_AR_STA_ID1: u64 = 0x8004;
const ATH_AR_RX_FILTER: u64 = 0x803C;

/// Hardware TX Descriptor (32-byte aligned, mapped into non-paginated DMA memory).
#[repr(C, align(32))]
#[derive(Debug, Clone, Copy)]
pub struct WifiTxDesc {
    pub status_len: u32,
    pub flags: u32,
    pub buf_addr_low: u32,
    pub buf_addr_high: u32,
    pub next_desc_low: u32,
    pub next_desc_high: u32,
    pub rsvd0: u32,
    pub rsvd1: u32,
}

/// Hardware RX Descriptor (32-byte aligned, owned by NIC DMA until frame arrival clears OWN bit).
#[repr(C, align(32))]
#[derive(Debug, Clone, Copy)]
pub struct WifiRxDesc {
    pub status_len: u32,
    pub phy_info: u32,
    pub buf_addr_low: u32,
    pub buf_addr_high: u32,
    pub next_desc_low: u32,
    pub next_desc_high: u32,
    pub tsf_low: u32,
    pub rsvd0: u32,
}

/// Physical TX + RX PCIe DMA Descriptor Rings for IEEE 802.11 hardware.
pub struct WifiDmaRing {
    pub desc_page_phys: u64,
    pub desc_page_virt: u64,
    pub buf_base_phys: u64,
    pub buf_base_virt: u64,
    pub tx_tail: usize,
    pub rx_head: usize,
}

impl WifiDmaRing {
    /// Allocate 1 physical 4 KiB descriptor table frame (TX at +0x000, RX at +0x800)
    /// and 16 contiguous 4 KiB physical packet frames (16 TX slots + 16 RX slots).
    pub fn allocate() -> Result<Self, &'static str> {
        let desc_page_phys = alloc_zeroed_frame()
            .ok_or("Out of physical frames for Wi-Fi DMA descriptor ring")?;
        let buf_base_phys = match PMM.lock().alloc_contiguous_frames(WIFI_DMA_BUFFER_FRAMES) {
            Some(p) => p,
            None => {
                PMM.lock().free_frame(desc_page_phys);
                return Err("Out of contiguous physical frames for Wi-Fi DMA buffers");
            }
        };

        let desc_page_virt = phys_to_virt(desc_page_phys);
        let buf_base_virt = phys_to_virt(buf_base_phys);

        unsafe {
            core::ptr::write_bytes(buf_base_virt as *mut u8, 0, WIFI_DMA_BUFFER_FRAMES * PAGE_SIZE);

            let tx_descs = desc_page_virt as *mut WifiTxDesc;
            let rx_descs = (desc_page_virt + 0x800) as *mut WifiRxDesc;
            let tx_desc_phys = desc_page_phys;
            let rx_desc_phys = desc_page_phys + 0x800;

            for i in 0..WIFI_RING_LEN {
                let next_idx = (i + 1) % WIFI_RING_LEN;

                // TX circular ring descriptor
                let tx_buf_phys = buf_base_phys + (i * WIFI_DMA_SLOT_SIZE) as u64;
                let tx_next_phys = tx_desc_phys + (next_idx * core::mem::size_of::<WifiTxDesc>()) as u64;
                write_volatile(
                    tx_descs.add(i),
                    WifiTxDesc {
                        status_len: 0,
                        flags: 0,
                        buf_addr_low: (tx_buf_phys & 0xFFFF_FFFF) as u32,
                        buf_addr_high: (tx_buf_phys >> 32) as u32,
                        next_desc_low: (tx_next_phys & 0xFFFF_FFFF) as u32,
                        next_desc_high: (tx_next_phys >> 32) as u32,
                        rsvd0: 0,
                        rsvd1: 0,
                    },
                );

                // RX circular ring descriptor (pre-armed OWN = HW)
                let rx_buf_phys = buf_base_phys + ((WIFI_RING_LEN + i) * WIFI_DMA_SLOT_SIZE) as u64;
                let rx_next_phys = rx_desc_phys + (next_idx * core::mem::size_of::<WifiRxDesc>()) as u64;
                write_volatile(
                    rx_descs.add(i),
                    WifiRxDesc {
                        status_len: DESC_OWN_HW | (WIFI_DMA_SLOT_SIZE as u32),
                        phy_info: 0,
                        buf_addr_low: (rx_buf_phys & 0xFFFF_FFFF) as u32,
                        buf_addr_high: (rx_buf_phys >> 32) as u32,
                        next_desc_low: (rx_next_phys & 0xFFFF_FFFF) as u32,
                        next_desc_high: (rx_next_phys >> 32) as u32,
                        tsf_low: 0,
                        rsvd0: 0,
                    },
                );
            }
        }
        fence(Ordering::SeqCst);

        Ok(Self {
            desc_page_phys,
            desc_page_virt,
            buf_base_phys,
            buf_base_virt,
            tx_tail: 0,
            rx_head: 0,
        })
    }

    pub fn tx_ring_phys(&self) -> u64 {
        self.desc_page_phys
    }

    pub fn rx_ring_phys(&self) -> u64 {
        self.desc_page_phys + 0x800
    }

    /// Stage an 802.11 frame into the next physical TX DMA slot and mark OWN for NIC hardware.
    pub fn enqueue_tx(&mut self, frame: &[u8]) -> Result<usize, &'static str> {
        if frame.is_empty() || frame.len() > WIFI_DMA_SLOT_SIZE {
            return Err("802.11 frame exceeds physical DMA slot capacity");
        }

        let slot = self.tx_tail;
        let tx_descs = self.desc_page_virt as *mut WifiTxDesc;
        let buf_virt = (self.buf_base_virt + (slot * WIFI_DMA_SLOT_SIZE) as u64) as *mut u8;

        unsafe {
            core::ptr::copy_nonoverlapping(frame.as_ptr(), buf_virt, frame.len());
            fence(Ordering::SeqCst);
            let mut desc = read_volatile(tx_descs.add(slot));
            desc.status_len =
                DESC_OWN_HW | DESC_FIRST_SEG | DESC_LAST_SEG | ((frame.len() as u32) & RX_LEN_MASK);
            write_volatile(tx_descs.add(slot), desc);
        }
        fence(Ordering::SeqCst);

        self.tx_tail = (slot + 1) % WIFI_RING_LEN;
        Ok(slot)
    }

    /// Poll the current RX descriptor; if hardware cleared OWN, copy out completed frame and re-arm OWN.
    pub fn poll_rx(&mut self, out_buf: &mut [u8]) -> usize {
        let slot = self.rx_head;
        let rx_descs = (self.desc_page_virt + 0x800) as *mut WifiRxDesc;

        unsafe {
            let mut desc = read_volatile(rx_descs.add(slot));
            if (desc.status_len & DESC_OWN_HW) != 0 {
                return 0; // Hardware still owns this RX descriptor
            }
            let pkt_len = ((desc.status_len & RX_LEN_MASK) as usize).min(WIFI_DMA_SLOT_SIZE);
            let copy_len = pkt_len.min(out_buf.len());
            if copy_len > 0 {
                let buf_virt = (self.buf_base_virt
                    + ((WIFI_RING_LEN + slot) * WIFI_DMA_SLOT_SIZE) as u64)
                    as *const u8;
                core::ptr::copy_nonoverlapping(buf_virt, out_buf.as_mut_ptr(), copy_len);
            }

            // Return RX descriptor ownership to NIC DMA
            desc.status_len = DESC_OWN_HW | (WIFI_DMA_SLOT_SIZE as u32);
            desc.phy_info = 0;
            write_volatile(rx_descs.add(slot), desc);
            fence(Ordering::SeqCst);

            self.rx_head = (slot + 1) % WIFI_RING_LEN;
            copy_len
        }
    }

    pub fn read_tx_desc(&self, slot: usize) -> Option<WifiTxDesc> {
        if slot >= WIFI_RING_LEN {
            return None;
        }
        unsafe { Some(read_volatile((self.desc_page_virt as *const WifiTxDesc).add(slot))) }
    }

    pub fn read_rx_desc(&self, slot: usize) -> Option<WifiRxDesc> {
        if slot >= WIFI_RING_LEN {
            return None;
        }
        unsafe { Some(read_volatile(((self.desc_page_virt + 0x800) as *const WifiRxDesc).add(slot))) }
    }
}

impl Drop for WifiDmaRing {
    fn drop(&mut self) {
        let mut pmm = PMM.lock();
        pmm.free_frame(self.desc_page_phys);
        for i in 0..WIFI_DMA_BUFFER_FRAMES {
            pmm.free_frame(self.buf_base_phys + (i * PAGE_SIZE) as u64);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WifiChipsetFamily {
    RealtekRtl8188,
    AtherosAth9k,
    IntelIwl,
}

pub struct WifiDevice {
    pub name: String,
    pub mac: [u8; 6],
    pub chip_name: String,
    pub family: WifiChipsetFamily,
    pub mmio_base: u64,
    pub dma_ring: IrqSafeSpinlock<WifiDmaRing>,
}

unsafe impl Send for WifiDevice {}
unsafe impl Sync for WifiDevice {}

impl WifiDevice {
    pub fn from_pci(pci: &PciDevice) -> Result<Arc<Self>, &'static str> {
        let (chip_name, family) = match (pci.vendor_id, pci.device_id) {
            (0x10EC, 0x8179) => ("Realtek RTL8188EE 802.11n PCIe", WifiChipsetFamily::RealtekRtl8188),
            (0x10EC, 0x8176) => ("Realtek RTL8188CE 802.11n PCIe", WifiChipsetFamily::RealtekRtl8188),
            (0x10EC, 0x8178) => ("Realtek RTL8192CE 802.11n PCIe", WifiChipsetFamily::RealtekRtl8188),
            (0x10EC, 0xC821) => ("Realtek RTL8821CE 802.11ac PCIe", WifiChipsetFamily::RealtekRtl8188),
            (0x10EC, 0xB822) => ("Realtek RTL8822BE 802.11ac PCIe", WifiChipsetFamily::RealtekRtl8188),
            (0x168C, 0x002B) => ("Qualcomm Atheros AR9285 (ath9k)", WifiChipsetFamily::AtherosAth9k),
            (0x168C, 0x002E) => ("Qualcomm Atheros AR9287 (ath9k)", WifiChipsetFamily::AtherosAth9k),
            (0x168C, 0x0032) => ("Qualcomm Atheros AR9485 (ath9k)", WifiChipsetFamily::AtherosAth9k),
            (0x168C, 0x0036) => ("Qualcomm Atheros QCA9565 (ath9k)", WifiChipsetFamily::AtherosAth9k),
            (0x8086, 0x2723) => ("Intel Wi-Fi 6 AX200 (802.11ax)", WifiChipsetFamily::IntelIwl),
            (0x8086, 0x2725) => ("Intel Wi-Fi 6E AX210 (802.11ax)", WifiChipsetFamily::IntelIwl),
            (0x8086, 0x2526) => ("Intel Wireless-AC 9260 (802.11ac)", WifiChipsetFamily::IntelIwl),
            (0x8086, 0x095A) => ("Intel Dual Band Wireless-AC 7265", WifiChipsetFamily::IntelIwl),
            (0x8086, 0x3165) => ("Intel Wireless 3165 (802.11ac)", WifiChipsetFamily::IntelIwl),
            _ => return Err("Unsupported PCIe 802.11 controller without register definition"),
        };

        // Realtek PCIe cards place MMIO on BAR2 when BAR0 is legacy I/O; ath9k/Intel use BAR0 MMIO
        let mmio_phys = [2u8, 0u8, 1u8]
            .iter()
            .filter_map(|&idx| {
                let raw = unsafe {
                    PciBus::read_config_dword(pci.bus, pci.device, pci.function, 0x10 + idx * 4)
                };
                if (raw & 0x01) == 0 && (raw & 0xFFFF_FFF0) != 0 {
                    Some(pci.read_bar(idx))
                } else {
                    None
                }
            })
            .next()
            .ok_or("No valid PCIe MMIO BAR found on 802.11 wireless adapter")?;

        let mmio_base = crate::mm::vmm::map_mmio_range(mmio_phys, 0x10000)?;
        let dma_ring = WifiDmaRing::allocate()?;

        unsafe {
            PciBus::enable_bus_mastering(pci.bus, pci.device, pci.function);
        }

        let mac = unsafe { Self::program_hardware_rings(family, mmio_base, &dma_ring, pci) };

        kinfo!(
            "Wi-Fi PCIe MMIO rings online: {} (MMIO: 0x{:X}, TX: 0x{:X}, RX: 0x{:X})",
            chip_name,
            mmio_base,
            dma_ring.tx_ring_phys(),
            dma_ring.rx_ring_phys()
        );

        Ok(Arc::new(Self {
            name: String::from("wlan0"),
            mac,
            chip_name: String::from(chip_name),
            family,
            mmio_base,
            dma_ring: IrqSafeSpinlock::new(dma_ring),
        }))
    }

    unsafe fn program_hardware_rings(
        family: WifiChipsetFamily,
        mmio_base: u64,
        ring: &WifiDmaRing,
        pci: &PciDevice,
    ) -> [u8; 6] {
        let tx_phys = ring.tx_ring_phys();
        let rx_phys = ring.rx_ring_phys();
        let mut mac = [0u8; 6];

        match family {
            WifiChipsetFamily::RealtekRtl8188 => {
                // Enable Realtek PCIe MAC + HCI DMA path
                write_volatile((mmio_base + RTL_REG_SYS_FUNC_EN) as *mut u16, 0x00FF);
                write_volatile((mmio_base + RTL_REG_CR) as *mut u32, 0x0000_02FF);

                // Program physical 64-bit TX Management & BE Queue + RX Queue descriptor ring bases
                write_volatile((mmio_base + RTL_REG_MGQ_DESA) as *mut u32, tx_phys as u32);
                write_volatile((mmio_base + RTL_REG_MGQ_DESA + 4) as *mut u32, (tx_phys >> 32) as u32);
                write_volatile((mmio_base + RTL_REG_BEQ_DESA) as *mut u32, tx_phys as u32);
                write_volatile((mmio_base + RTL_REG_BEQ_DESA + 4) as *mut u32, (tx_phys >> 32) as u32);
                write_volatile((mmio_base + RTL_REG_RXQ_DESA) as *mut u32, rx_phys as u32);
                write_volatile((mmio_base + RTL_REG_RXQ_DESA + 4) as *mut u32, (rx_phys >> 32) as u32);

                // Configure RX Control (accept management beacons, probe resp, broadcast & AP data) & TCR
                write_volatile((mmio_base + RTL_REG_TCR) as *mut u32, 0x0000_2F00);
                write_volatile((mmio_base + RTL_REG_RCR) as *mut u32, 0x0000_1F0E);

                for i in 0..6 {
                    mac[i] = read_volatile((mmio_base + RTL_REG_MACID + i as u64) as *const u8);
                }
            }
            WifiChipsetFamily::AtherosAth9k => {
                // Program ath9k TX Queue 0 and RX descriptor physical pointers
                write_volatile((mmio_base + ATH_AR_Q0_TXDP) as *mut u32, tx_phys as u32);
                write_volatile((mmio_base + ATH_AR_RXDP) as *mut u32, rx_phys as u32);
                write_volatile((mmio_base + ATH_AR_RX_FILTER) as *mut u32, 0x0000_0037); // UCAST|BCAST|MCAST|BEACON|PROBERESP
                write_volatile((mmio_base + ATH_AR_CR) as *mut u32, 0x0000_0004); // RXE

                let id0 = read_volatile((mmio_base + ATH_AR_STA_ID0) as *const u32);
                let id1 = read_volatile((mmio_base + ATH_AR_STA_ID1) as *const u32);
                mac = [
                    (id0 & 0xFF) as u8,
                    ((id0 >> 8) & 0xFF) as u8,
                    ((id0 >> 16) & 0xFF) as u8,
                    ((id0 >> 24) & 0xFF) as u8,
                    (id1 & 0xFF) as u8,
                    ((id1 >> 8) & 0xFF) as u8,
                ];
            }
            WifiChipsetFamily::IntelIwl => {
                // Intel iwlwifi requires signed microcode firmware blob in addition to FH DMA ring pointers
                kwarn!(
                    "Intel iwlwifi PCIe NIC detected (MMIO: 0x{:X}); requires external iwlwifi uCode firmware.",
                    mmio_base
                );
            }
        }

        // If EEPROM reported all-00 or all-FF on an unprogrammed card, derive deterministic local MAC
        if mac == [0; 6] || mac == [0xFF; 6] {
            mac = [0x02, (pci.vendor_id >> 8) as u8, pci.vendor_id as u8, pci.bus, pci.device, 0x01];
        }
        mac
    }

    #[inline]
    unsafe fn ring_tx_doorbell(&self, slot: usize) {
        match self.family {
            WifiChipsetFamily::RealtekRtl8188 => {
                // Kick Realtek PCIe TX DMA polling register for MGQ / BEQ
                write_volatile((self.mmio_base + RTL_REG_PCIE_CTRL_INT_MIG) as *mut u32, 1 << (slot % 16));
            }
            WifiChipsetFamily::AtherosAth9k => {
                write_volatile((self.mmio_base + ATH_AR_Q0_TXDP) as *mut u32, self.dma_ring.lock().tx_ring_phys() as u32);
                write_volatile((self.mmio_base + ATH_AR_Q_TXE) as *mut u32, 0x0000_0001);
            }
            WifiChipsetFamily::IntelIwl => {}
        }
    }
}

impl NetDevice for WifiDevice {
    fn name(&self) -> &str {
        &self.name
    }

    fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    fn send_packet(&self, packet: &[u8]) -> Result<(), &'static str> {
        let slot = self.dma_ring.lock().enqueue_tx(packet)?;
        unsafe {
            self.ring_tx_doorbell(slot);
        }
        Ok(())
    }

    fn receive_packet(&self, buf: &mut [u8]) -> Result<usize, &'static str> {
        Ok(self.dma_ring.lock().poll_rx(buf))
    }

    fn is_link_up(&self) -> bool {
        crate::net::wifi::WIFI_MANAGER.lock().is_connected
    }

    fn device_type(&self) -> NetDeviceType {
        NetDeviceType::Wireless80211
    }
}
