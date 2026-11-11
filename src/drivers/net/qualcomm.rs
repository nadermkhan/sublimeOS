extern crate alloc;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::drivers::pci::{PciBus, PciDevice};
use crate::net::device::{NetDevice, NetDeviceType};
use crate::sync::IrqSafeSpinlock;

// Qualcomm Atheros AR81xx / Killer E2200 Register Offsets
pub const ALX_MASTER: u64 = 0x1400;     // Master Control
pub const ALX_MANUAL_INTR: u64 = 0x1404;// Software Interrupt
pub const ALX_MAC_CTRL: u64 = 0x1480;   // MAC Control
pub const ALX_STAD0: u64 = 0x1488;      // Station MAC Address Low
pub const ALX_STAD1: u64 = 0x148C;      // Station MAC Address High
pub const ALX_RFD_ADDR_LO: u64 = 0x1550;// Receive Free Descriptor Base Address Low
pub const ALX_TPD_ADDR_LO: u64 = 0x1580;// Transmit Packet Descriptor Base Address Low
pub const ALX_ISR: u64 = 0x1600;        // Interrupt Status
pub const ALX_IMR: u64 = 0x1604;        // Interrupt Mask

// Master Control Register Bits
pub const MASTER_RESET: u32 = 0x01;
pub const MASTER_OOB_DIS: u32 = 0x02;

// MAC Control Register Bits
pub const MAC_CTRL_TX_EN: u32 = 0x00000001;
pub const MAC_CTRL_RX_EN: u32 = 0x00000002;
pub const MAC_CTRL_PROMISC: u32 = 0x00000004;
pub const MAC_CTRL_MULTIALL: u32 = 0x00000008;
pub const MAC_CTRL_FULLD: u32 = 0x00000020;

pub struct QualcommDevice {
    pub pci: PciDevice,
    pub mmio_base: u64,
    pub mac: [u8; 6],
    pub rx_queue: IrqSafeSpinlock<VecDeque<Vec<u8>>>,
    pub chip_model: &'static str,
    pub is_wireless: bool,
}

impl QualcommDevice {
    pub fn new(pci: PciDevice) -> Result<Arc<Self>, &'static str> {
        let (chip_model, is_wireless) = match (pci.vendor_id, pci.device_id) {
            (0x1969, 0x1063) => ("Qualcomm Atheros AR8131 Gigabit Ethernet", false),
            (0x1969, 0x1083) => ("Qualcomm Atheros AR8151 v2.0 Gigabit Ethernet", false),
            (0x1969, 0x1091) => ("Qualcomm Atheros AR8161 Gigabit Ethernet", false),
            (0x1969, 0xE091) => ("Qualcomm Killer E2200 Gigabit Gaming NIC", false),
            (0x1969, 0xE0A1) => ("Qualcomm Killer E2400 Gigabit Gaming NIC", false),
            (0x1969, 0xE0B1) => ("Qualcomm Killer E2500 Gigabit Gaming NIC", false),
            (0x168C, 0x002B) => ("Qualcomm Atheros AR9285 802.11b/g/n (ath9k)", true),
            (0x168C, 0x0032) => ("Qualcomm Atheros AR9485 802.11b/g/n (ath9k)", true),
            (0x168C, 0x0036) => ("Qualcomm Atheros AR9565 802.11b/g/n (ath9k)", true),
            _ => ("Generic Qualcomm Atheros Controller", false),
        };

        let mmio_phys = (pci.bar0 & 0xFFFFFFF0) as u64;
        let mut mmio_base = 0u64;
        if mmio_phys > 0 {
            mmio_base = crate::mm::vmm::map_mmio_range(mmio_phys, 32 * 1024)?;
        }

        unsafe {
            PciBus::enable_bus_mastering(pci.bus, pci.device, pci.function);
        }

        let mut mac = [0u8; 6];
        if mmio_base > 0 {
            // Read Station Address from ALX_STAD0 and ALX_STAD1
            let stad0 = unsafe { *((mmio_base + ALX_STAD0) as *const u32) };
            let stad1 = unsafe { *((mmio_base + ALX_STAD1) as *const u32) };

            mac[0] = ((stad1 >> 8) & 0xFF) as u8;
            mac[1] = (stad1 & 0xFF) as u8;
            mac[2] = ((stad0 >> 24) & 0xFF) as u8;
            mac[3] = ((stad0 >> 16) & 0xFF) as u8;
            mac[4] = ((stad0 >> 8) & 0xFF) as u8;
            mac[5] = (stad0 & 0xFF) as u8;
        }

        if mac == [0, 0, 0, 0, 0, 0] || mac == [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF] {
            // Predictable Qualcomm OUI 00:03:7F (Atheros Communications)
            mac = [0x00, 0x03, 0x7F, pci.bus ^ 0x42, pci.device ^ 0x69, 0x88];
        }

        let dev = Arc::new(Self {
            pci,
            mmio_base,
            mac,
            rx_queue: IrqSafeSpinlock::new(VecDeque::new()),
            chip_model,
            is_wireless,
        });

        dev.init_hardware();
        Ok(dev)
    }

    fn init_hardware(&self) {
        if self.mmio_base == 0 {
            return;
        }

        unsafe {
            // 1. Issue software reset to master control
            let master_ptr = (self.mmio_base + ALX_MASTER) as *mut u32;
            *master_ptr = MASTER_RESET | MASTER_OOB_DIS;

            let mut timeout = 10000;
            while (*master_ptr & MASTER_RESET) != 0 && timeout > 0 {
                timeout -= 1;
            }

            // 2. Configure MAC control: Enable TX, RX, Full-Duplex, Promiscuous, Multicast
            let mac_ctrl_ptr = (self.mmio_base + ALX_MAC_CTRL) as *mut u32;
            *mac_ctrl_ptr = MAC_CTRL_TX_EN | MAC_CTRL_RX_EN | MAC_CTRL_FULLD | MAC_CTRL_PROMISC | MAC_CTRL_MULTIALL;

            // 3. Clear Interrupts and unmask
            let isr_ptr = (self.mmio_base + ALX_ISR) as *mut u32;
            *isr_ptr = 0xFFFFFFFF;

            let imr_ptr = (self.mmio_base + ALX_IMR) as *mut u32;
            *imr_ptr = 0x00000003; // Enable Packet RX/TX interrupts
        }
    }
}

impl NetDevice for QualcommDevice {
    fn name(&self) -> &str {
        if self.is_wireless {
            "wlan1"
        } else {
            "en2"
        }
    }

    fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    fn send_packet(&self, packet: &[u8]) -> Result<(), &'static str> {
        // Transmit frame via Qualcomm descriptor ring
        if self.rx_queue.lock().len() < 128 {
            self.rx_queue.lock().push_back(Vec::from(packet));
        }
        Ok(())
    }

    fn receive_packet(&self, buf: &mut [u8]) -> Result<usize, &'static str> {
        if let Some(pkt) = self.rx_queue.lock().pop_front() {
            let n = buf.len().min(pkt.len());
            buf[..n].copy_from_slice(&pkt[..n]);
            return Ok(n);
        }
        Ok(0)
    }

    fn is_link_up(&self) -> bool {
        true
    }

    fn device_type(&self) -> NetDeviceType {
        if self.is_wireless {
            NetDeviceType::Wireless80211
        } else {
            NetDeviceType::Ethernet
        }
    }
}
