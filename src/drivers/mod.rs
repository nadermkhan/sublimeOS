pub mod keyboard;
pub mod mouse;
pub mod pci;
pub mod vbe;

use crate::kinfo;

pub fn init() {
    keyboard::init();
    pci::init();
    mouse::init(1024, 768);
    kinfo!("Device drivers initialized (Keyboard, Mouse, PCI)");
}
