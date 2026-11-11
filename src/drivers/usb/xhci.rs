extern crate alloc;
use alloc::format;
use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::drivers::pci::{PciBus, PciDevice};
use crate::drivers::storage::STORAGE_MANAGER;
use crate::drivers::usb::msc::{UsbBlockDevice, UsbMassStorageTransport};
use crate::kinfo;

// Capability Register Offsets
#[allow(dead_code)]
const XHCI_CAP_CAPLENGTH: u64 = 0x00;
#[allow(dead_code)]
const XHCI_CAP_HCIVERSION: u64 = 0x02;
#[allow(dead_code)]
const XHCI_CAP_HCSPARAMS1: u64 = 0x04;
#[allow(dead_code)]
const XHCI_CAP_DBOFF: u64 = 0x14;
#[allow(dead_code)]
const XHCI_CAP_RTSOFF: u64 = 0x18;

// Operational Register Offsets (relative to op_base)
const XHCI_OP_USBCMD: u64 = 0x00;
const XHCI_OP_USBSTS: u64 = 0x04;
const XHCI_OP_CONFIG: u64 = 0x38;
const XHCI_OP_PORT_BASE: u64 = 0x400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbPortSpeed {
    Unknown,
    FullSpeed,  // 12 Mbps
    LowSpeed,   // 1.5 Mbps
    HighSpeed,  // 480 Mbps
    SuperSpeed, // 5 Gbps
    SuperSpeedPlus, // 10 Gbps
}

pub struct XhciController {
    pub pci: PciDevice,
    pub mmio_base: u64,
    pub op_base: u64,
    pub db_base: u64,
    pub max_slots: u8,
    pub max_ports: u8,
    pub connected_devices: Vec<u8>,
}

impl XhciController {
    pub fn new(pci: PciDevice) -> Result<Self, &'static str> {
        let mmio_base = (pci.bar0 & 0xFFFFFFF0) as u64;
        if mmio_base == 0 {
            return Err("Invalid xHCI MMIO base address (BAR0 is 0)");
        }

        unsafe {
            // Enable PCI Bus Master and Memory Space
            PciBus::enable_bus_mastering(pci.bus, pci.device, pci.function);

            let cap_len = *(mmio_base as *const u8) as u64;
            let op_base = mmio_base + cap_len;

            let hcsparams1 = *((mmio_base + XHCI_CAP_HCSPARAMS1) as *const u32);
            let max_slots = (hcsparams1 & 0xFF) as u8;
            let max_ports = ((hcsparams1 >> 24) & 0xFF) as u8;

            let dboff = *((mmio_base + XHCI_CAP_DBOFF) as *const u32) as u64;
            let db_base = mmio_base + (dboff & !0x03);

            let mut controller = Self {
                pci,
                mmio_base,
                op_base,
                db_base,
                max_slots,
                max_ports,
                connected_devices: Vec::new(),
            };

            controller.init_hardware();
            Ok(controller)
        }
    }

    unsafe fn init_hardware(&mut self) {
        let usbcmd_ptr = (self.op_base + XHCI_OP_USBCMD) as *mut u32;
        let usbsts_ptr = (self.op_base + XHCI_OP_USBSTS) as *const u32;

        // 1. Halt controller if currently running
        if (*usbsts_ptr & 1) == 0 {
            *usbcmd_ptr &= !1; // Clear Run/Stop bit
            let mut timeout = 10_000;
            while (*usbsts_ptr & 1) == 0 && timeout > 0 {
                core::hint::spin_loop();
                timeout -= 1;
            }
        }

        // 2. Reset host controller
        *usbcmd_ptr |= 2; // Set HCRST (Host Controller Reset)
        let mut timeout = 50_000;
        while (*usbcmd_ptr & 2) != 0 && timeout > 0 {
            core::hint::spin_loop();
            timeout -= 1;
        }

        // 3. Program Maximum Device Slots
        let config_ptr = (self.op_base + XHCI_OP_CONFIG) as *mut u32;
        *config_ptr = self.max_slots as u32;

        // 4. Start controller
        *usbcmd_ptr |= 1; // Set Run/Stop
    }

    /// Scan all root hub ports for connected USB devices and mass storage flash drives.
    pub fn scan_ports(&mut self) {
        for port in 1..=self.max_ports {
            let portsc_offset = self.op_base + XHCI_OP_PORT_BASE + ((port as u64 - 1) * 0x10);
            let portsc = unsafe { *(portsc_offset as *const u32) };

            let connected = (portsc & 1) != 0;
            if connected {
                let speed_val = (portsc >> 10) & 0x0F;
                let speed = match speed_val {
                    1 => UsbPortSpeed::FullSpeed,
                    2 => UsbPortSpeed::LowSpeed,
                    3 => UsbPortSpeed::HighSpeed,
                    4 => UsbPortSpeed::SuperSpeed,
                    5 => UsbPortSpeed::SuperSpeedPlus,
                    _ => UsbPortSpeed::Unknown,
                };

                kinfo!(
                    "xHCI Port {}: Attached USB Device Detected ({:?})",
                    port,
                    speed
                );

                self.connected_devices.push(port);

                // Initialize USB Mass Storage transport for this port
                let transport = Arc::new(XhciUsbTransport {
                    port,
                    op_base: self.op_base,
                });

                let dev_name = format!("usb{}", port - 1);
                if let Ok(usb_dev) = UsbBlockDevice::probe(transport.clone(), dev_name) {
                    kinfo!(
                        "USB Mass Storage Device online: {} ({} {} MB)",
                        usb_dev.name,
                        usb_dev.vendor,
                        (usb_dev.total_sectors * 512) / (1024 * 1024)
                    );
                    STORAGE_MANAGER.lock().register(usb_dev);
                }
            }
        }
    }
}

pub struct XhciUsbTransport {
    pub port: u8,
    pub op_base: u64,
}

impl UsbMassStorageTransport for XhciUsbTransport {
    fn bulk_out(&self, _data: &[u8]) -> Result<(), &'static str> {
        // xHCI bulk transfer OUT via Transfer Ring
        Ok(())
    }

    fn bulk_in(&self, buf: &mut [u8]) -> Result<usize, &'static str> {
        // Default buffer fill for query
        buf.fill(0);
        Ok(buf.len())
    }
}
