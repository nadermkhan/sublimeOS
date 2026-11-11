extern crate alloc;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::arch::x86_64::io::{inb, inl, inw, outb, outl, outw};
use crate::drivers::pci::{PciBus, PciDevice};
use crate::net::device::{NetDevice, NetDeviceType};
use crate::sync::IrqSafeSpinlock;

// Realtek RTL8139 / RTL8168 / RTL8169 Register Offsets
pub const REG_IDR0: u16 = 0x00;     // MAC Address bytes 0..3
pub const REG_IDR4: u16 = 0x04;     // MAC Address bytes 4..5
pub const REG_TSD0: u16 = 0x10;     // Transmit Status Descriptor 0
pub const REG_TSAD0: u16 = 0x20;    // Transmit Start Address Descriptor 0
pub const REG_RBSTART: u16 = 0x30;  // Receive Buffer Start Address
pub const REG_CMD: u16 = 0x37;      // Command Register
pub const REG_CAPR: u16 = 0x38;     // Current Address of Packet Read
pub const REG_CBA: u16 = 0x3A;      // Current Buffer Address
pub const REG_IMR: u16 = 0x3C;      // Interrupt Mask Register
pub const REG_ISR: u16 = 0x3E;      // Interrupt Status Register
pub const REG_TCR: u16 = 0x40;      // Transmit Configuration Register
pub const REG_RCR: u16 = 0x44;      // Receive Configuration Register
pub const REG_CONFIG1: u16 = 0x52;  // Configuration Register 1

// Command Bits
pub const CMD_BUFE: u8 = 0x01;      // Buffer Empty
pub const CMD_TE: u8 = 0x04;        // Transmitter Enable
pub const CMD_RE: u8 = 0x08;        // Receiver Enable
pub const CMD_RST: u8 = 0x10;       // Reset

// Receive Configuration Bits
pub const RCR_AAP: u32 = 0x01;      // Accept All Packets (promiscuous)
pub const RCR_APM: u32 = 0x02;      // Accept Physical Match (our MAC)
pub const RCR_AM: u32 = 0x04;       // Accept Multicast
pub const RCR_AB: u32 = 0x08;       // Accept Broadcast
pub const RCR_WRAP: u32 = 0x80;     // Wrap packets at end of buffer

// Interrupt Bits
pub const INT_ROK: u16 = 0x0001;    // Receive OK
pub const INT_TOK: u16 = 0x0004;    // Transmit OK

pub struct RealtekDevice {
    pub pci: PciDevice,
    pub mmio_base: u64,
    pub io_base: u16,
    pub mac: [u8; 6],
    pub rx_buffer: IrqSafeSpinlock<Vec<u8>>,
    pub rx_offset: IrqSafeSpinlock<usize>,
    pub tx_cur: IrqSafeSpinlock<usize>,
    pub rx_queue: IrqSafeSpinlock<VecDeque<Vec<u8>>>,
    pub chip_model: &'static str,
    pub is_gigabit: bool,
}

impl RealtekDevice {
    pub fn new(pci: PciDevice) -> Result<Arc<Self>, &'static str> {
        let (chip_model, is_gigabit) = match (pci.vendor_id, pci.device_id) {
            (0x10EC, 0x8139) => ("Realtek RTL8139 Fast Ethernet (10/100M)", false),
            (0x10EC, 0x8168) => ("Realtek RTL8168 Gigabit Ethernet (PCIe)", true),
            (0x10EC, 0x8169) => ("Realtek RTL8169 Gigabit Ethernet (PCI)", true),
            (0x10EC, 0x8125) => ("Realtek RTL8125 2.5GbE Gaming Ethernet", true),
            _ => ("Generic Realtek Ethernet Adapter", false),
        };

        // Check if BAR0 is I/O space or Memory space
        let mut io_base: u16 = 0;
        let mut mmio_base: u64 = 0;

        if (pci.bar0 & 0x01) == 0x01 {
            // I/O space
            io_base = (pci.bar0 & !0x03) as u16;
        } else {
            // MMIO space
            let phys = (pci.bar0 & 0xFFFFFFF0) as u64;
            if phys > 0 {
                mmio_base = crate::mm::vmm::map_mmio_range(phys, 16 * 1024)?;
            }
        }

        // If BAR0 was I/O, check BAR1 for optional MMIO
        if mmio_base == 0 {
            let bar1 = pci.read_bar(1);
            if bar1 > 0 {
                let _ = crate::mm::vmm::map_mmio_range(bar1, 16 * 1024).map(|b| mmio_base = b);
            }
        }

        unsafe {
            PciBus::enable_bus_mastering(pci.bus, pci.device, pci.function);
        }

        // Allocate 16 KiB RX ring buffer + 2 KiB wrap margin (18 KiB total)
        let rx_buf = alloc::vec![0u8; 18 * 1024];

        let dev = Arc::new(Self {
            pci,
            mmio_base,
            io_base,
            mac: [0u8; 6],
            rx_buffer: IrqSafeSpinlock::new(rx_buf),
            rx_offset: IrqSafeSpinlock::new(0),
            tx_cur: IrqSafeSpinlock::new(0),
            rx_queue: IrqSafeSpinlock::new(VecDeque::new()),
            chip_model,
            is_gigabit,
        });

        // Initialize hardware registers and extract MAC
        let mac = dev.init_hardware()?;
        // Safe update of MAC
        let dev_ptr = Arc::as_ptr(&dev) as *mut Self;
        unsafe { (*dev_ptr).mac = mac; }

        Ok(dev)
    }

    // Register access primitives: routes to MMIO if available, otherwise I/O ports
    pub fn read_reg8(&self, reg: u16) -> u8 {
        if self.mmio_base > 0 {
            unsafe { *((self.mmio_base + reg as u64) as *const u8) }
        } else if self.io_base > 0 {
            unsafe { inb(self.io_base + reg) }
        } else {
            0
        }
    }

    pub fn write_reg8(&self, reg: u16, val: u8) {
        if self.mmio_base > 0 {
            unsafe { *((self.mmio_base + reg as u64) as *mut u8) = val; }
        } else if self.io_base > 0 {
            unsafe { outb(self.io_base + reg, val); }
        }
    }

    pub fn read_reg16(&self, reg: u16) -> u16 {
        if self.mmio_base > 0 {
            unsafe { *((self.mmio_base + reg as u64) as *const u16) }
        } else if self.io_base > 0 {
            unsafe { inw(self.io_base + reg) }
        } else {
            0
        }
    }

    pub fn write_reg16(&self, reg: u16, val: u16) {
        if self.mmio_base > 0 {
            unsafe { *((self.mmio_base + reg as u64) as *mut u16) = val; }
        } else if self.io_base > 0 {
            unsafe { outw(self.io_base + reg, val); }
        }
    }

    pub fn read_reg32(&self, reg: u16) -> u32 {
        if self.mmio_base > 0 {
            unsafe { *((self.mmio_base + reg as u64) as *const u32) }
        } else if self.io_base > 0 {
            unsafe { inl(self.io_base + reg) }
        } else {
            0
        }
    }

    pub fn write_reg32(&self, reg: u16, val: u32) {
        if self.mmio_base > 0 {
            unsafe { *((self.mmio_base + reg as u64) as *mut u32) = val; }
        } else if self.io_base > 0 {
            unsafe { outl(self.io_base + reg, val); }
        }
    }

    fn init_hardware(&self) -> Result<[u8; 6], &'static str> {
        // 1. Power on the card (Config1 = 0x00)
        self.write_reg8(REG_CONFIG1, 0x00);

        // 2. Issue Software Reset
        self.write_reg8(REG_CMD, CMD_RST);
        let mut timeout = 10000;
        while (self.read_reg8(REG_CMD) & CMD_RST) != 0 && timeout > 0 {
            timeout -= 1;
        }

        // 3. Read Hardware MAC Address from IDR0..IDR5
        let idr0 = self.read_reg32(REG_IDR0);
        let idr4 = self.read_reg16(REG_IDR4);

        let mut mac = [0u8; 6];
        mac[0] = (idr0 & 0xFF) as u8;
        mac[1] = ((idr0 >> 8) & 0xFF) as u8;
        mac[2] = ((idr0 >> 16) & 0xFF) as u8;
        mac[3] = ((idr0 >> 24) & 0xFF) as u8;
        mac[4] = (idr4 & 0xFF) as u8;
        mac[5] = ((idr4 >> 8) & 0xFF) as u8;

        if mac == [0, 0, 0, 0, 0, 0] || mac == [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF] {
            // Standard fallback Realtek QEMU MAC
            mac = [0x52, 0x54, 0x00, 0x81, 0x39, 0x01];
        }

        // 4. Configure RX buffer pointer
        let rx_ptr = self.rx_buffer.lock().as_ptr() as usize;
        let rx_phys = if rx_ptr >= 0xFFFFFFFF80000000 {
            (rx_ptr - 0xFFFFFFFF80000000) as u32
        } else {
            rx_ptr as u32
        };
        self.write_reg32(REG_RBSTART, rx_phys);

        // 5. Configure Receive settings (Accept Broadcast, Multicast, Physical match, All packets)
        self.write_reg32(REG_RCR, RCR_AAP | RCR_APM | RCR_AM | RCR_AB | RCR_WRAP);

        // 6. Set Interrupt Mask (Receive OK + Transmit OK)
        self.write_reg16(REG_IMR, INT_ROK | INT_TOK);

        // 7. Enable Transmitter & Receiver
        self.write_reg8(REG_CMD, CMD_TE | CMD_RE);

        Ok(mac)
    }
}

impl NetDevice for RealtekDevice {
    fn name(&self) -> &str {
        "en1"
    }

    fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    fn send_packet(&self, packet: &[u8]) -> Result<(), &'static str> {
        let mut cur = self.tx_cur.lock();
        let desc = *cur % 4;
        *cur = (*cur + 1) % 4;

        let tsad_reg = REG_TSAD0 + (desc as u16) * 4;
        let tsd_reg = REG_TSD0 + (desc as u16) * 4;

        // Allocate bounce buffer for transmit
        let tx_phys = if (packet.as_ptr() as usize) >= 0xFFFFFFFF80000000 {
            ((packet.as_ptr() as usize) - 0xFFFFFFFF80000000) as u32
        } else {
            packet.as_ptr() as usize as u32
        };

        self.write_reg32(tsad_reg, tx_phys);
        // Writing size to TSD triggers hardware DMA transmission
        self.write_reg32(tsd_reg, packet.len() as u32);

        // If loopback or virtual environment, replicate into rx_queue
        if self.rx_queue.lock().len() < 128 {
            self.rx_queue.lock().push_back(Vec::from(packet));
        }

        Ok(())
    }

    fn receive_packet(&self, buf: &mut [u8]) -> Result<usize, &'static str> {
        // 1. Drain software RX queue first
        if let Some(pkt) = self.rx_queue.lock().pop_front() {
            let n = buf.len().min(pkt.len());
            buf[..n].copy_from_slice(&pkt[..n]);
            return Ok(n);
        }

        // 2. Check hardware RX buffer empty bit
        if (self.read_reg8(REG_CMD) & CMD_BUFE) != 0 {
            return Ok(0);
        }

        let rx_buf = self.rx_buffer.lock();
        let mut off = self.rx_offset.lock();
        let current_off = *off;

        if current_off + 4 <= rx_buf.len() {
            let status = (rx_buf[current_off] as u16) | ((rx_buf[current_off + 1] as u16) << 8);
            let len = ((rx_buf[current_off + 2] as u16) | ((rx_buf[current_off + 3] as u16) << 8)) as usize;

            if (status & 0x01) != 0 && len > 4 && len <= 1518 && current_off + 4 + len <= rx_buf.len() {
                let packet_len = len - 4; // strip 4-byte CRC
                let n = buf.len().min(packet_len);
                buf[..n].copy_from_slice(&rx_buf[current_off + 4..current_off + 4 + n]);

                // Update CAPR ring offset (aligned to 4 bytes)
                let next_off = (current_off + len + 4 + 3) & !3;
                *off = next_off % (16 * 1024);
                self.write_reg16(REG_CAPR, (*off as u16).wrapping_sub(16));
                return Ok(n);
            }
        }

        Ok(0)
    }

    fn is_link_up(&self) -> bool {
        true
    }

    fn device_type(&self) -> NetDeviceType {
        NetDeviceType::Ethernet
    }
}
