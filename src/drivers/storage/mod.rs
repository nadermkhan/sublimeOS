extern crate alloc;
use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

pub mod ata;
pub mod ahci;
pub mod nvme;

pub trait BlockDevice: Send + Sync {
    fn name(&self) -> &str;
    fn read_sectors(&self, lba: u64, count: u32, buf: &mut [u8]) -> Result<(), &'static str>;
    fn write_sectors(&self, lba: u64, count: u32, buf: &[u8]) -> Result<(), &'static str>;
    fn total_sectors(&self) -> u64;
    fn sector_size(&self) -> usize {
        512
    }
}

pub struct StorageManager {
    pub devices: Vec<Arc<dyn BlockDevice>>,
}

impl StorageManager {
    pub const fn new() -> Self {
        Self { devices: Vec::new() }
    }

    pub fn register(&mut self, dev: Arc<dyn BlockDevice>) {
        kinfo!(
            "Registered Block Device: {} ({} MB, {} sectors)",
            dev.name(),
            (dev.total_sectors() * 512) / (1024 * 1024),
            dev.total_sectors()
        );
        self.devices.push(dev);
    }

    pub fn get_device(&self, index: usize) -> Option<Arc<dyn BlockDevice>> {
        self.devices.get(index).cloned()
    }
}

pub static STORAGE_MANAGER: IrqSafeSpinlock<StorageManager> =
    IrqSafeSpinlock::new(StorageManager::new());

/// Initialize physical storage subsystems (PCIe NVMe, AHCI SATA, and conditional PCI IDE).
pub fn init() {
    kinfo!("Scanning storage controllers (NVMe, AHCI SATA, conditional PCI IDE)...");

    // 1. Scan PCIe NVMe controllers via PCI first
    nvme::init();

    // 2. Scan AHCI SATA controllers via PCI
    ahci::init();

    // 3. Only invoke legacy ATA/IDE when PCI explicitly enumerated a Mass Storage IDE controller (0x01:0x01)
    let has_pci_ide = crate::drivers::pci::PCI
        .lock()
        .devices
        .iter()
        .any(|d| d.class_code == 0x01 && d.subclass == 0x01);

    if has_pci_ide {
        ata::init();
    } else {
        kinfo!("Storage: No PCI IDE controller present; legacy ATA/PATA channels skipped.");
    }
}
