pub mod keyboard;
pub mod pci;

use crate::kinfo;

pub fn init() {
    keyboard::init();
    pci::init();
    kinfo!("Device drivers initialized");
}
