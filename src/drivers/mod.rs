pub mod audio;
pub mod crashdump;
pub mod gop_console;
pub mod gpu;
pub mod keyboard;
pub mod mouse;
pub mod net;
pub mod pci;
pub mod platform;
pub mod storage;
pub mod usb;
pub mod vbe;

use crate::kinfo;

pub fn init() {
    keyboard::init();
    pci::init();
    platform::init();
    gpu::init();
    let _ = vbe::init(1024, 768);
    let (fb_w, fb_h) = vbe::dimensions();
    audio::init();
    storage::init();
    usb::init();
    mouse::init(fb_w as i32, fb_h as i32);
    kinfo!("Device drivers initialized (Keyboard, Mouse, PCI, Platform, GPU, Audio, Storage, USB)");
}
