//! Broadcom PCIe / BCMA Network Hardware Drivers
//!
//! - [`tg3`]: Broadcom BCM57765 / NetXtreme Gigabit Ethernet (`14E4:16B4`, `14E4:16B0`, etc.)
//! - [`b43`]: Broadcom BCM4331 Dual-Band 802.11n Wireless (`14E4:4331`) via BCMA & b43 microcode

pub mod b43;
pub mod tg3;

pub const BROADCOM_PCI_VENDOR: u16 = 0x14E4;

/// Returns `true` if `(vendor_id, device_id)` is a supported Broadcom Tigon3 (NetXtreme) Ethernet NIC.
pub fn is_broadcom_ethernet(vendor_id: u16, device_id: u16) -> bool {
    vendor_id == BROADCOM_PCI_VENDOR && tg3::is_supported_device(device_id)
}

/// Returns `true` if `(vendor_id, device_id)` is a supported Broadcom `b43` Wireless adapter.
pub fn is_broadcom_wifi(vendor_id: u16, device_id: u16) -> bool {
    vendor_id == BROADCOM_PCI_VENDOR && b43::is_supported_device(device_id)
}
