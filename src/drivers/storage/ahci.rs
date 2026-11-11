extern crate alloc;
use crate::drivers::pci::PCI;
use crate::kinfo;

pub fn init() {
    let pci = PCI.lock();
    let mut found = false;

    for dev in pci.devices.iter() {
        if dev.class_code == 0x01 && dev.subclass == 0x06 && dev.prog_if == 0x01 {
            // AHCI SATA Controller found
            found = true;
            let abar_phys = dev.bar0 as u64; // In AHCI, BAR5 is at offset 0x24
            kinfo!(
                "AHCI SATA Controller Found: Vendor 0x{:04X}, Device 0x{:04X}, Bus {}:{}:{}, BAR5: 0x{:X}",
                dev.vendor_id,
                dev.device_id,
                dev.bus,
                dev.device,
                dev.function,
                abar_phys
            );
            break;
        }
    }

    if !found {
        kinfo!("AHCI SATA controller not detected (using ATA/IDE storage bus).");
    }
}
