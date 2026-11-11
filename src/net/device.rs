extern crate alloc;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetDeviceType {
    Loopback,
    Ethernet,
    Wireless80211,
}

pub trait NetDevice: Send + Sync {
    fn name(&self) -> &str;
    fn mac_address(&self) -> [u8; 6];
    fn send_packet(&self, packet: &[u8]) -> Result<(), &'static str>;
    fn receive_packet(&self, buf: &mut [u8]) -> Result<usize, &'static str>;
    fn is_link_up(&self) -> bool;
    fn device_type(&self) -> NetDeviceType;
}

#[derive(Debug, Clone, Copy)]
pub struct InterfaceConfig {
    pub ipv4: [u8; 4],
    pub netmask: [u8; 4],
    pub gateway: [u8; 4],
    pub dns: [u8; 4],
    pub dhcp_enabled: bool,
    pub tx_packets: u64,
    pub rx_packets: u64,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
}

impl InterfaceConfig {
    pub const fn new() -> Self {
        Self {
            ipv4: [0, 0, 0, 0],
            netmask: [255, 255, 255, 0],
            gateway: [0, 0, 0, 0],
            dns: [8, 8, 8, 8],
            dhcp_enabled: true,
            tx_packets: 0,
            rx_packets: 0,
            tx_bytes: 0,
            rx_bytes: 0,
        }
    }
}

pub struct LoopbackDevice {
    queue: IrqSafeSpinlock<VecDeque<Vec<u8>>>,
}

impl LoopbackDevice {
    pub fn new() -> Self {
        Self {
            queue: IrqSafeSpinlock::new(VecDeque::new()),
        }
    }
}

impl NetDevice for LoopbackDevice {
    fn name(&self) -> &str {
        "lo0"
    }

    fn mac_address(&self) -> [u8; 6] {
        [0, 0, 0, 0, 0, 0]
    }

    fn send_packet(&self, packet: &[u8]) -> Result<(), &'static str> {
        self.queue.lock().push_back(alloc::vec::Vec::from(packet));
        Ok(())
    }

    fn receive_packet(&self, buf: &mut [u8]) -> Result<usize, &'static str> {
        if let Some(pkt) = self.queue.lock().pop_front() {
            let n = buf.len().min(pkt.len());
            buf[..n].copy_from_slice(&pkt[..n]);
            Ok(n)
        } else {
            Ok(0)
        }
    }

    fn is_link_up(&self) -> bool {
        true
    }

    fn device_type(&self) -> NetDeviceType {
        NetDeviceType::Loopback
    }
}

pub struct NetworkManager {
    pub devices: Vec<(Arc<dyn NetDevice>, InterfaceConfig)>,
    pub default_idx: Option<usize>,
}

impl NetworkManager {
    pub const fn new() -> Self {
        Self {
            devices: Vec::new(),
            default_idx: None,
        }
    }

    pub fn register(&mut self, dev: Arc<dyn NetDevice>) {
        let is_loopback = dev.device_type() == NetDeviceType::Loopback;
        let mut cfg = InterfaceConfig::new();

        if is_loopback {
            cfg.ipv4 = [127, 0, 0, 1];
            cfg.netmask = [255, 0, 0, 0];
            cfg.dhcp_enabled = false;
        }

        let mac = dev.mac_address();
        kinfo!(
            "Registered Network Interface: {} ({:?}, MAC: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X})",
            dev.name(),
            dev.device_type(),
            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
        );

        self.devices.push((dev, cfg));

        if self.default_idx.is_none() && !is_loopback {
            self.default_idx = Some(self.devices.len() - 1);
        }
    }

    pub fn get_device(&self, name: &str) -> Option<(Arc<dyn NetDevice>, InterfaceConfig)> {
        for (dev, cfg) in &self.devices {
            if dev.name() == name {
                return Some((dev.clone(), *cfg));
            }
        }
        None
    }

    pub fn get_device_for_ip(&self, dst_ip: [u8; 4]) -> Option<(Arc<dyn NetDevice>, InterfaceConfig)> {
        if dst_ip[0] == 127 {
            return self.get_device("lo0");
        }
        // Check if destination matches any non-loopback interface subnet
        for (dev, cfg) in &self.devices {
            if dev.device_type() != NetDeviceType::Loopback && cfg.ipv4 != [0, 0, 0, 0] {
                let matches = (0..4).all(|i| (dst_ip[i] & cfg.netmask[i]) == (cfg.ipv4[i] & cfg.netmask[i]));
                if matches {
                    return Some((dev.clone(), *cfg));
                }
            }
        }
        // Fallback to default device
        self.get_default_device()
    }

    pub fn get_default_device(&self) -> Option<(Arc<dyn NetDevice>, InterfaceConfig)> {
        if let Some(idx) = self.default_idx {
            self.devices.get(idx).map(|(d, c)| (d.clone(), *c))
        } else {
            self.devices.first().map(|(d, c)| (d.clone(), *c))
        }
    }

    pub fn set_interface_config(&mut self, name: &str, new_cfg: InterfaceConfig) {
        for (dev, cfg) in &mut self.devices {
            if dev.name() == name {
                *cfg = new_cfg;
                break;
            }
        }
    }
}

pub static NET_MANAGER: IrqSafeSpinlock<NetworkManager> = IrqSafeSpinlock::new(NetworkManager::new());
