extern crate alloc;
use crate::drivers::net::e1000::E1000Device;
use crate::drivers::net::wifi_hw::WifiDevice;
use crate::drivers::pci::PCI;
use crate::net::device::NET_MANAGER;
use crate::kinfo;

pub mod e1000;
pub mod wifi_hw;

/// Discover and initialize network controller hardware (Ethernet & Wi-Fi).
pub fn init() {
    let pci = PCI.lock();
    let devices = pci.devices.clone();
    drop(pci);

    let mut found_wifi = false;

    for dev in &devices {
        if dev.class_code == 0x02 {
            // Network Controller
            if dev.subclass == 0x00 && dev.vendor_id == 0x8086 {
                // Intel Gigabit Ethernet (e1000 / e1000e)
                if let Ok(e1000) = E1000Device::new(dev.clone()) {
                    let mac = e1000.mac;
                    kinfo!(
                        "Intel e1000 Gigabit Ethernet online: en0 (MAC: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X})",
                        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
                    );
                    NET_MANAGER.lock().register(e1000);
                }
            } else if dev.subclass == 0x80 || is_wifi_pci_device(dev.vendor_id, dev.device_id) {
                // PCIe Wi-Fi Controller
                let wifi = WifiDevice::from_pci(dev);
                let mac = wifi.mac;
                kinfo!(
                    "PCIe Wireless Adapter online: {} -> wlan0 (MAC: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X})",
                    wifi.chip_name,
                    mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
                );
                NET_MANAGER.lock().register(wifi);
                found_wifi = true;
            }
        }
    }

    // If no physical Wi-Fi card present on PCI bus, provide the default IEEE 802.11 adapter
    if !found_wifi {
        let default_wifi = WifiDevice::new([0x3C, 0xF8, 0x62, 0x11, 0x22, 0x33], "Sublime 802.11ax Wireless Adapter");
        NET_MANAGER.lock().register(default_wifi);
    }
}

fn is_wifi_pci_device(vendor: u16, device: u16) -> bool {
    // Intel Wi-Fi: AX200, AX210, 9260, 8265, 7265, 3165
    if vendor == 0x8086 && matches!(device, 0x2723 | 0x2725 | 0x2526 | 0x095A | 0x3165 | 0x24F3) {
        return true;
    }
    // Realtek Wi-Fi: RTL8821CE, RTL8188, RTL8812
    if vendor == 0x10EC && matches!(device, 0xC821 | 0x8179 | 0x8812) {
        return true;
    }
    false
}
