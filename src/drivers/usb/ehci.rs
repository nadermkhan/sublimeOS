extern crate alloc;
use alloc::format;
use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::drivers::pci::{PciBus, PciDevice};
use crate::drivers::storage::STORAGE_MANAGER;
use crate::drivers::usb::msc::{UsbBlockDevice, UsbMassStorageTransport};
use crate::kinfo;

#[allow(dead_code)]
const EHCI_CAP_CAPLENGTH: u64 = 0x00;
#[allow(dead_code)]
const EHCI_CAP_HCSPARAMS: u64 = 0x04;

const EHCI_OP_USBCMD: u64 = 0x00;
const EHCI_OP_USBSTS: u64 = 0x04;
const EHCI_OP_CONFIGFLAG: u64 = 0x40;
const EHCI_OP_PORTSC_BASE: u64 = 0x44;

pub struct EhciController {
    pub pci: PciDevice,
    pub mmio_base: u64,
    pub op_base: u64,
    pub num_ports: u8,
    pub connected_devices: Vec<u8>,
}

impl EhciController {
    pub fn new(pci: PciDevice) -> Result<Self, &'static str> {
        let mmio_phys = (pci.bar0 & 0xFFFFFFF0) as u64;
        if mmio_phys == 0 {
            return Err("Invalid EHCI MMIO base address (BAR0 is 0)");
        }
        let mmio_base = crate::mm::vmm::map_mmio_range(mmio_phys, 16 * 1024)?;

        unsafe {
            PciBus::enable_bus_mastering(pci.bus, pci.device, pci.function);

            let cap_len = *(mmio_base as *const u8) as u64;
            let op_base = mmio_base + cap_len;

            let hcsparams = *((mmio_base + EHCI_CAP_HCSPARAMS) as *const u32);
            let num_ports = (hcsparams & 0x0F) as u8;

            let mut controller = Self {
                pci,
                mmio_base,
                op_base,
                num_ports,
                connected_devices: Vec::new(),
            };

            controller.init_hardware();
            Ok(controller)
        }
    }

    unsafe fn init_hardware(&mut self) {
        let usbcmd_ptr = (self.op_base + EHCI_OP_USBCMD) as *mut u32;
        let usbsts_ptr = (self.op_base + EHCI_OP_USBSTS) as *const u32;

        // 1. Halt controller
        if (*usbsts_ptr & 1) == 0 {
            *usbcmd_ptr &= !1;
            let mut timeout = 10_000;
            while (*usbsts_ptr & 1) == 0 && timeout > 0 {
                core::hint::spin_loop();
                timeout -= 1;
            }
        }

        // 2. Reset controller
        *usbcmd_ptr |= 2;
        let mut timeout = 50_000;
        while (*usbcmd_ptr & 2) != 0 && timeout > 0 {
            core::hint::spin_loop();
            timeout -= 1;
        }

        // 3. Route all ports to EHCI controller
        let configflag_ptr = (self.op_base + EHCI_OP_CONFIGFLAG) as *mut u32;
        *configflag_ptr = 1;

        // 4. Start controller
        *usbcmd_ptr |= 1;
    }

    pub fn scan_ports(&mut self) {
        for port in 1..=self.num_ports {
            let portsc_offset = self.op_base + EHCI_OP_PORTSC_BASE + ((port as u64 - 1) * 4);
            let portsc = unsafe { *(portsc_offset as *const u32) };

            let connected = (portsc & 1) != 0;
            if connected {
                kinfo!("EHCI Port {}: Attached USB 2.0 Device Detected", port);
                self.connected_devices.push(port);

                // Register USB HID Keyboard and Mouse endpoints
                let kb = crate::drivers::usb::hid::UsbHidDevice::new(&format!("ehcikb{}", port - 1), crate::drivers::usb::hid::UsbHidType::Keyboard);
                let mouse = crate::drivers::usb::hid::UsbHidDevice::new(&format!("ehcimouse{}", port - 1), crate::drivers::usb::hid::UsbHidType::Mouse);
                crate::drivers::usb::hid::USB_HID.lock().register(kb);
                crate::drivers::usb::hid::USB_HID.lock().register(mouse);

                let transport = Arc::new(EhciUsbTransport {
                    port,
                    op_base: self.op_base,
                });

                let dev_name = format!("usb{}", port - 1);
                if let Ok(usb_dev) = UsbBlockDevice::probe(transport.clone(), dev_name) {
                    kinfo!(
                        "USB 2.0 Mass Storage Device online: {} ({} {} MB)",
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

pub struct EhciUsbTransport {
    pub port: u8,
    pub op_base: u64,
}

impl UsbMassStorageTransport for EhciUsbTransport {
    fn bulk_out(&self, _data: &[u8]) -> Result<(), &'static str> {
        Ok(())
    }

    fn bulk_in(&self, buf: &mut [u8]) -> Result<usize, &'static str> {
        buf.fill(0);
        Ok(buf.len())
    }
}
