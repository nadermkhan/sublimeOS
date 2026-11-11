pub mod audio;
pub mod keyboard;
pub mod mouse;
pub mod net;
pub mod pci;
pub mod storage;
pub mod usb;
pub mod vbe;

use crate::kinfo;

pub fn init() {
    keyboard::init();
    pci::init();
    let _ = vbe::init(1024, 768);
    audio::init();
    storage::init();
    usb::init();
    mouse::init(1024, 768);
    kinfo!("Device drivers initialized (Keyboard, Mouse, PCI, Audio, Storage, USB)");
}
