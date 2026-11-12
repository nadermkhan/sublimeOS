extern crate alloc;
use crate::drivers::net::broadcom::b43::B43Device;
use crate::drivers::net::broadcom::tg3::Tg3Device;
use crate::drivers::net::e1000::E1000Device;
use crate::drivers::net::qualcomm::QualcommDevice;
use crate::drivers::net::realtek::RealtekDevice;
use crate::drivers::net::wifi_hw::WifiDevice;
use crate::drivers::pci::PCI;
use crate::net::device::{NetDevice, NET_MANAGER};
use crate::kinfo;

pub mod broadcom;
pub mod e1000;
pub mod qualcomm;
pub mod realtek;
pub mod wifi_hw;

/// Discover and initialize physical network controller hardware (Ethernet & PCIe Wi-Fi).
pub fn init() {
    let pci = PCI.lock();
    let devices = pci.devices.clone();
    drop(pci);

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
            } else if dev.vendor_id == 0x10EC && (dev.subclass == 0x00 || matches!(dev.device_id, 0x8139 | 0x8168 | 0x8169 | 0x8125)) {
                // Realtek Ethernet (RTL8139 / RTL8168 / RTL8169 / RTL8125)
                if let Ok(realtek) = RealtekDevice::new(dev.clone()) {
                    let mac = realtek.mac;
                    kinfo!(
                        "Realtek Ethernet online: {} -> en1 (MAC: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X})",
                        realtek.chip_model,
                        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
                    );
                    NET_MANAGER.lock().register(realtek);
                }
            } else if dev.vendor_id == 0x1969 {
                // Qualcomm Atheros Wired AR81xx / Killer E2200/E2400/E2500
                if let Ok(qualcomm) = QualcommDevice::new(dev.clone()) {
                    let mac = qualcomm.mac;
                    let ifname = qualcomm.name();
                    kinfo!(
                        "Qualcomm Atheros Ethernet online: {} -> {} (MAC: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X})",
                        qualcomm.chip_model,
                        ifname,
                        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
                    );
                    NET_MANAGER.lock().register(qualcomm);
                }
            } else if broadcom::is_broadcom_ethernet(dev.vendor_id, dev.device_id) {
                // Broadcom NetXtreme BCM57765 / Tigon3 Gigabit Ethernet
                if let Ok(tg3) = Tg3Device::new(dev.clone()) {
                    let mac = tg3.mac;
                    kinfo!(
                        "{} online: en3 (MAC: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X})",
                        tg3.chip_model,
                        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
                    );
                    NET_MANAGER.lock().register(tg3);
                }
            } else if broadcom::is_broadcom_wifi(dev.vendor_id, dev.device_id) {
                // Broadcom BCM4331 802.11n Wireless (BCMA + b43)
                if let Ok(b43) = B43Device::new(dev.clone()) {
                    let mac = b43.mac;
                    kinfo!(
                        "Broadcom BCM4331 Wireless online: wlan2 (MAC: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}, uCode: {})",
                        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5],
                        if b43.firmware_loaded { "loaded" } else { "awaiting firmware/b43" }
                    );
                    NET_MANAGER.lock().register(b43);
                }
            } else if dev.subclass == 0x80 || is_wifi_pci_device(dev.vendor_id, dev.device_id) {
                // Real PCIe Wi-Fi Controller with hardware MMIO & physical TX/RX DMA rings (RTL8188EE / ath9k / iwl)
                if let Ok(wifi) = WifiDevice::from_pci(dev) {
                    let mac = wifi.mac;
                    kinfo!(
                        "PCIe Wireless Adapter online: {} -> wlan0 (MAC: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X})",
                        wifi.chip_name,
                        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
                    );
                    NET_MANAGER.lock().register(wifi);
                }
            }
        }
    }
}

fn is_wifi_pci_device(vendor: u16, device: u16) -> bool {
    // Realtek Wi-Fi: RTL8188EE/CE, RTL8192CE, RTL8821CE, RTL8822BE
    if vendor == 0x10EC && matches!(device, 0x8179 | 0x8176 | 0x8178 | 0xC821 | 0xB822) {
        return true;
    }
    // Qualcomm Atheros ath9k: AR9285, AR9287, AR9485, QCA9565
    if vendor == 0x168C && matches!(device, 0x002B | 0x002E | 0x0032 | 0x0036) {
        return true;
    }
    // Intel Wi-Fi: AX200, AX210, 9260, 8265, 7265, 3165
    if vendor == 0x8086 && matches!(device, 0x2723 | 0x2725 | 0x2526 | 0x095A | 0x3165 | 0x24F3) {
        return true;
    }
    false
}
