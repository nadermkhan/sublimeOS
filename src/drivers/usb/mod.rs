extern crate alloc;
use alloc::vec::Vec;
use crate::drivers::pci::PCI;
use crate::drivers::usb::ehci::EhciController;
use crate::drivers::usb::xhci::XhciController;
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

pub mod ehci;
pub mod msc;
pub mod xhci;

pub struct UsbSubsystem {
    pub xhci_controllers: Vec<XhciController>,
    pub ehci_controllers: Vec<EhciController>,
}

impl UsbSubsystem {
    pub const fn new() -> Self {
        Self {
            xhci_controllers: Vec::new(),
            ehci_controllers: Vec::new(),
        }
    }
}

pub static USB: IrqSafeSpinlock<UsbSubsystem> = IrqSafeSpinlock::new(UsbSubsystem::new());

/// Scan PCI bus and initialize all detected USB host controllers (xHCI & EHCI).
pub fn init() {
    let pci = PCI.lock();
    let devices = pci.devices.clone();
    drop(pci);

    let mut usb = USB.lock();

    for dev in devices {
        // USB Host Controllers: Class 0x0C (Serial Bus), Subclass 0x03 (USB)
        if dev.class_code == 0x0C && dev.subclass == 0x03 {
            match dev.prog_if {
                0x30 => {
                    // xHCI (USB 3.0 / Extensible Host Controller)
                    match XhciController::new(dev.clone()) {
                        Ok(mut xhci) => {
                            kinfo!(
                                "xHCI USB 3.0 Controller online: BAR0 0x{:08X}, {} ports, {} slots",
                                dev.bar0,
                                xhci.max_ports,
                                xhci.max_slots
                            );
                            xhci.scan_ports();
                            usb.xhci_controllers.push(xhci);
                        }
                        Err(e) => {
                            kinfo!("xHCI controller initialization error: {}", e);
                        }
                    }
                }
                0x20 => {
                    // EHCI (USB 2.0 / Enhanced Host Controller)
                    match EhciController::new(dev.clone()) {
                        Ok(mut ehci) => {
                            kinfo!(
                                "EHCI USB 2.0 Controller online: BAR0 0x{:08X}, {} ports",
                                dev.bar0,
                                ehci.num_ports
                            );
                            ehci.scan_ports();
                            usb.ehci_controllers.push(ehci);
                        }
                        Err(e) => {
                            kinfo!("EHCI controller initialization error: {}", e);
                        }
                    }
                }
                0x00 => {
                    kinfo!("UHCI USB 1.1 companion controller detected (Bus {}, Dev {})", dev.bus, dev.device);
                }
                0x10 => {
                    kinfo!("OHCI USB 1.1 companion controller detected (Bus {}, Dev {})", dev.bus, dev.device);
                }
                _ => {}
            }
        }
    }
}
