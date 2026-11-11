extern crate alloc;
use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

pub mod ata;
pub mod ahci;

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

/// Initialize physical storage subsystems (ATA/IDE and AHCI SATA controllers).
pub fn init() {
    kinfo!("Scanning storage controllers (ATA/IDE and AHCI SATA)...");

    // 1. Scan ATA/IDE primary and secondary buses
    ata::init();

    // 2. Scan AHCI SATA controllers via PCI
    ahci::init();
}
