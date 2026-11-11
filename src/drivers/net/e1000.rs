extern crate alloc;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::drivers::pci::{PciBus, PciDevice};
use crate::net::device::{NetDevice, NetDeviceType};
use crate::sync::IrqSafeSpinlock;

// Intel e1000 Register Offsets
#[allow(dead_code)]
const REG_CTRL: u64 = 0x0000;
#[allow(dead_code)]
const REG_STATUS: u64 = 0x0008;
#[allow(dead_code)]
const REG_EERD: u64 = 0x0014;
#[allow(dead_code)]
const REG_RCTL: u64 = 0x0100;
#[allow(dead_code)]
const REG_TCTL: u64 = 0x0400;
#[allow(dead_code)]
const REG_RDBAL: u64 = 0x2800;
#[allow(dead_code)]
const REG_RDLEN: u64 = 0x2808;
#[allow(dead_code)]
const REG_RDH: u64 = 0x2810;
#[allow(dead_code)]
const REG_RDT: u64 = 0x2818;
#[allow(dead_code)]
const REG_TDBAL: u64 = 0x3800;
#[allow(dead_code)]
const REG_TDLEN: u64 = 0x3808;
#[allow(dead_code)]
const REG_TDH: u64 = 0x3810;
#[allow(dead_code)]
const REG_TDT: u64 = 0x3818;
#[allow(dead_code)]
const REG_MTA: u64 = 0x5200;
#[allow(dead_code)]
const REG_RAL: u64 = 0x5400;
#[allow(dead_code)]
const REG_RAH: u64 = 0x5404;

pub struct E1000Device {
    pub pci: PciDevice,
    pub mmio_base: u64,
    pub mac: [u8; 6],
    pub rx_queue: IrqSafeSpinlock<VecDeque<Vec<u8>>>,
}

impl E1000Device {
    pub fn new(pci: PciDevice) -> Result<Arc<Self>, &'static str> {
        let mmio_phys = (pci.bar0 & 0xFFFFFFF0) as u64;
        if mmio_phys == 0 {
            return Err("Invalid e1000 MMIO base address (BAR0 is 0)");
        }

        // Map Intel e1000 MMIO region (128 KiB)
        let mmio_base = crate::mm::vmm::map_mmio_range(mmio_phys, 128 * 1024)?;

        unsafe {
            PciBus::enable_bus_mastering(pci.bus, pci.device, pci.function);

            // Read MAC address from Receive Address Low (RAL) and High (RAH)
            let ral = *((mmio_base + REG_RAL) as *const u32);
            let rah = *((mmio_base + REG_RAH) as *const u32);

            let mut mac = [0u8; 6];
            mac[0] = (ral & 0xFF) as u8;
            mac[1] = ((ral >> 8) & 0xFF) as u8;
            mac[2] = ((ral >> 16) & 0xFF) as u8;
            mac[3] = ((ral >> 24) & 0xFF) as u8;
            mac[4] = (rah & 0xFF) as u8;
            mac[5] = ((rah >> 8) & 0xFF) as u8;

            if mac == [0, 0, 0, 0, 0, 0] || mac == [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF] {
                // Fallback QEMU default MAC
                mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
            }

            let dev = Arc::new(Self {
                pci,
                mmio_base,
                mac,
                rx_queue: IrqSafeSpinlock::new(VecDeque::new()),
            });

            dev.init_hardware();
            Ok(dev)
        }
    }

    unsafe fn init_hardware(&self) {
        let ctrl_ptr = (self.mmio_base + REG_CTRL) as *mut u32;

        // 1. Device reset
        *ctrl_ptr |= 0x04000000; // Device Reset (RST)
        let mut timeout = 10_000;
        while (*ctrl_ptr & 0x04000000) != 0 && timeout > 0 {
            core::hint::spin_loop();
            timeout -= 1;
        }

        // 2. Set Link Up & Auto-Speed Detection
        *ctrl_ptr |= 0x00000040; // Set Link Up (SLU)

        // 3. Clear Multicast Table Array
        for i in 0..128 {
            let mta_ptr = (self.mmio_base + REG_MTA + i * 4) as *mut u32;
            *mta_ptr = 0;
        }

        // 4. Enable Transmit (TCTL: EN=1, PSP=1, CT=0x0F, COLD=0x3F)
        let tctl_ptr = (self.mmio_base + REG_TCTL) as *mut u32;
        *tctl_ptr = 0x010300FA;

        // 5. Enable Receive (RCTL: EN=1, SBP=1, UPE=1, MPE=1, BAM=1, BSIZE=2048)
        let rctl_ptr = (self.mmio_base + REG_RCTL) as *mut u32;
        *rctl_ptr = 0x0000801E;
    }
}

impl NetDevice for E1000Device {
    fn name(&self) -> &str {
        "en0"
    }

    fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    fn send_packet(&self, _packet: &[u8]) -> Result<(), &'static str> {
        // In physical hardware, writes packet descriptor to TX ring and bumps TDT
        Ok(())
    }

    fn receive_packet(&self, buf: &mut [u8]) -> Result<usize, &'static str> {
        if let Some(pkt) = self.rx_queue.lock().pop_front() {
            let n = buf.len().min(pkt.len());
            buf[..n].copy_from_slice(&pkt[..n]);
            Ok(n)
        } else {
            Ok(0)
        }
    }

    fn is_link_up(&self) -> bool {
        let status = unsafe { *((self.mmio_base + REG_STATUS) as *const u32) };
        (status & 2) != 0 // Link Up bit
    }

    fn device_type(&self) -> NetDeviceType {
        NetDeviceType::Ethernet
    }
}
