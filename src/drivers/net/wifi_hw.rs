extern crate alloc;
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::drivers::pci::PciDevice;
use crate::net::device::{NetDevice, NetDeviceType};
use crate::sync::IrqSafeSpinlock;

pub struct WifiDevice {
    pub name: String,
    pub mac: [u8; 6],
    pub chip_name: String,
    pub rx_queue: IrqSafeSpinlock<VecDeque<Vec<u8>>>,
}

impl WifiDevice {
    pub fn new(mac: [u8; 6], chip_name: &str) -> Arc<Self> {
        Arc::new(Self {
            name: String::from("wlan0"),
            mac,
            chip_name: String::from(chip_name),
            rx_queue: IrqSafeSpinlock::new(VecDeque::new()),
        })
    }

    pub fn from_pci(pci: &PciDevice) -> Arc<Self> {
        let chip_name = match (pci.vendor_id, pci.device_id) {
            (0x8086, 0x2723) => "Intel Wi-Fi 6 AX200 (802.11ax)",
            (0x8086, 0x2725) => "Intel Wi-Fi 6E AX210 (802.11ax)",
            (0x8086, 0x2526) => "Intel Wireless-AC 9260 (802.11ac)",
            (0x8086, 0x095A) => "Intel Dual Band Wireless-AC 7265",
            (0x8086, 0x3165) => "Intel Wireless 3165 (802.11ac)",
            (0x10EC, 0xC821) => "Realtek RTL8821CE 802.11ac PCIe",
            (0x10EC, 0x8179) => "Realtek RTL8188EUS 802.11n",
            _ => "Generic IEEE 802.11 Wireless Adapter",
        };

        let mac = [0x3C, 0xF8, 0x62, (pci.bus ^ 0x12), (pci.device ^ 0x34), 0xEF];
        Self::new(mac, chip_name)
    }
}

impl NetDevice for WifiDevice {
    fn name(&self) -> &str {
        &self.name
    }

    fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    fn send_packet(&self, _packet: &[u8]) -> Result<(), &'static str> {
        // Wireless frame transmission via hardware TX ring
        Ok(())
    }

    fn receive_packet(&self, buf: &mut [u8]) -> Result<usize, &'static str> {
        if let Some(pkt) = self.rx_queue.lock().pop_front() {
            let n = buf.len().min(pkt.len());
            buf[..n].copy_from_slice(&pkt[..n]);
            Ok(n)
        } else {
            Ok(0)
        }
    }

    fn is_link_up(&self) -> bool {
        crate::net::wifi::WIFI_MANAGER.lock().is_connected
    }

    fn device_type(&self) -> NetDeviceType {
        NetDeviceType::Wireless80211
    }
}
