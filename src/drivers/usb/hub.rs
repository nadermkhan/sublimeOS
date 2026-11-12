//! USB 2.0 Standard Descriptor Parser & USB Hub Class (0x09) Enumeration
//!
//! Enables recursive enumeration downstream of USB 2.0 Hubs (such as the
//! Intel HM65 Rate Matching Hubs `0x8087:0x0024` on MacBookPro8,2) so internal
//! and external HID Keyboards and Apple Wellspring Trackpads behind hubs are
//! properly powered, reset, addressed, configured, and bound to EHCI Interrupt IN pipes.

extern crate alloc;
use alloc::vec::Vec;
use crate::drivers::usb::hid::trackpad::{is_apple_wellspring_trackpad, UsbSetupPacket};
use crate::drivers::usb::hid::UsbHidType;

pub const USB_DESC_TYPE_DEVICE: u8 = 0x01;
pub const USB_DESC_TYPE_CONFIG: u8 = 0x02;
pub const USB_DESC_TYPE_INTERFACE: u8 = 0x04;
pub const USB_DESC_TYPE_ENDPOINT: u8 = 0x05;
pub const USB_DESC_TYPE_HUB: u8 = 0x29;

pub const USB_CLASS_HID: u8 = 0x03;
pub const USB_CLASS_HUB: u8 = 0x09;

// Hub Port Feature Selectors (USB 2.0 Spec Section 11.24.2)
pub const HUB_FEAT_PORT_RESET: u16 = 4;
pub const HUB_FEAT_PORT_POWER: u16 = 8;
pub const HUB_FEAT_C_PORT_CONNECTION: u16 = 16;
pub const HUB_FEAT_C_PORT_RESET: u16 = 20;

// wPortStatus bitmasks (USB 2.0 Spec Table 11-21)
pub const HUB_PORT_STAT_CONNECTION: u16 = 1 << 0;
pub const HUB_PORT_STAT_ENABLE: u16 = 1 << 1;
pub const HUB_PORT_STAT_RESET: u16 = 1 << 4;
pub const HUB_PORT_STAT_POWER: u16 = 1 << 8;
pub const HUB_PORT_STAT_LOW_SPEED: u16 = 1 << 9;
pub const HUB_PORT_STAT_HIGH_SPEED: u16 = 1 << 10;

/// USB Standard Device Descriptor (18-byte wire format decoded safely).
#[derive(Debug, Clone, Copy)]
pub struct UsbDeviceDescriptor {
    pub b_length: u8,
    pub b_descriptor_type: u8,
    pub bcd_usb: u16,
    pub b_device_class: u8,
    pub b_device_sub_class: u8,
    pub b_device_protocol: u8,
    pub b_max_packet_size0: u8,
    pub id_vendor: u16,
    pub id_product: u16,
    pub bcd_device: u16,
    pub i_manufacturer: u8,
    pub i_product: u8,
    pub i_serial_number: u8,
    pub b_num_configurations: u8,
}

impl UsbDeviceDescriptor {
    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < 18 || buf[0] < 18 || buf[1] != USB_DESC_TYPE_DEVICE {
            return None;
        }
        Some(Self {
            b_length: buf[0],
            b_descriptor_type: buf[1],
            bcd_usb: u16::from_le_bytes([buf[2], buf[3]]),
            b_device_class: buf[4],
            b_device_sub_class: buf[5],
            b_device_protocol: buf[6],
            b_max_packet_size0: buf[7],
            id_vendor: u16::from_le_bytes([buf[8], buf[9]]),
            id_product: u16::from_le_bytes([buf[10], buf[11]]),
            bcd_device: u16::from_le_bytes([buf[12], buf[13]]),
            i_manufacturer: buf[14],
            i_product: buf[15],
            i_serial_number: buf[16],
            b_num_configurations: buf[17],
        })
    }
}

/// USB Standard Configuration Descriptor header (9-byte wire format decoded safely).
#[derive(Debug, Clone, Copy)]
pub struct UsbConfigDescriptor {
    pub b_length: u8,
    pub b_descriptor_type: u8,
    pub w_total_length: u16,
    pub b_num_interfaces: u8,
    pub b_configuration_value: u8,
    pub i_configuration: u8,
    pub bm_attributes: u8,
    pub b_max_power: u8,
}

impl UsbConfigDescriptor {
    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < 9 || buf[0] < 9 || buf[1] != USB_DESC_TYPE_CONFIG {
            return None;
        }
        Some(Self {
            b_length: buf[0],
            b_descriptor_type: buf[1],
            w_total_length: u16::from_le_bytes([buf[2], buf[3]]),
            b_num_interfaces: buf[4],
            b_configuration_value: buf[5],
            i_configuration: buf[6],
            bm_attributes: buf[7],
            b_max_power: buf[8],
        })
    }
}

/// USB Hub Class Descriptor header (decoded safely up to 32 downstream ports).
#[derive(Debug, Clone, Copy)]
pub struct UsbHubDescriptor {
    pub b_desc_length: u8,
    pub b_descriptor_type: u8,
    pub b_nbr_ports: u8,
    pub w_hub_characteristics: u16,
    pub b_pwr_on_2_pwr_good: u8,
    pub b_hub_contr_current: u8,
    pub device_removable_mask: u32,
}

impl UsbHubDescriptor {
    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < 7 || buf[0] < 7 || buf[1] != USB_DESC_TYPE_HUB {
            return None;
        }
        let nbr_ports = buf[2];
        let mut removable_mask = 0u32;
        // Parse DeviceRemovable bitmask if present (bytes 7.. up to 4 bytes / 31 ports)
        if buf.len() >= 8 {
            let num_bytes = ((nbr_ports as usize / 8) + 1).min(4).min(buf.len() - 7);
            for i in 0..num_bytes {
                removable_mask |= (buf[7 + i] as u32) << (i * 8);
            }
        }
        Some(Self {
            b_desc_length: buf[0],
            b_descriptor_type: buf[1],
            b_nbr_ports: nbr_ports,
            w_hub_characteristics: u16::from_le_bytes([buf[3], buf[4]]),
            b_pwr_on_2_pwr_good: buf[5],
            b_hub_contr_current: buf[6],
            device_removable_mask: removable_mask,
        })
    }

    /// Returns true if downstream port is a removable external port, false if fixed internal.
    pub fn is_port_removable(&self, port: u8) -> bool {
        if port == 0 || port > self.b_nbr_ports {
            return false;
        }
        // Bit N = 0 means removable; Bit N = 1 means non-removable / internal
        (self.device_removable_mask & (1 << port)) == 0
    }
}

/// Discovered HID Interrupt IN endpoint extracted from a device's configuration tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiscoveredHidEndpoint {
    pub kind: UsbHidType,
    pub interface_num: u8,
    pub interface_subclass: u8,
    pub endpoint_num: u8,
    pub max_packet_size: u16,
    pub interval_ms: u8,
    pub is_apple_wellspring: bool,
}

/// Standard USB Control Setup helpers for enumeration & Hub Class control.
pub mod setup {
    use super::*;

    pub const fn get_descriptor(desc_type: u8, desc_index: u8, len: u16) -> UsbSetupPacket {
        UsbSetupPacket {
            bm_request_type: 0x80, // Device-to-Host | Standard | Device
            b_request: 0x06,       // GET_DESCRIPTOR
            w_value: ((desc_type as u16) << 8) | (desc_index as u16),
            w_index: 0,
            w_length: len,
        }
    }

    pub const fn set_address(address: u8) -> UsbSetupPacket {
        UsbSetupPacket {
            bm_request_type: 0x00, // Host-to-Device | Standard | Device
            b_request: 0x05,       // SET_ADDRESS
            w_value: address as u16,
            w_index: 0,
            w_length: 0,
        }
    }

    pub const fn set_configuration(config_value: u8) -> UsbSetupPacket {
        UsbSetupPacket {
            bm_request_type: 0x00,
            b_request: 0x09, // SET_CONFIGURATION
            w_value: config_value as u16,
            w_index: 0,
            w_length: 0,
        }
    }

    pub const fn get_hub_descriptor(len: u16) -> UsbSetupPacket {
        UsbSetupPacket {
            bm_request_type: 0xA0, // Device-to-Host | Class | Device
            b_request: 0x06,       // GET_DESCRIPTOR
            w_value: (USB_DESC_TYPE_HUB as u16) << 8,
            w_index: 0,
            w_length: len,
        }
    }

    pub const fn get_port_status(port: u8) -> UsbSetupPacket {
        UsbSetupPacket {
            bm_request_type: 0xA3, // Device-to-Host | Class | Other (Port)
            b_request: 0x00,       // GET_STATUS
            w_value: 0,
            w_index: port as u16,
            w_length: 4,
        }
    }

    pub const fn set_port_feature(port: u8, feature: u16) -> UsbSetupPacket {
        UsbSetupPacket {
            bm_request_type: 0x23, // Host-to-Device | Class | Other (Port)
            b_request: 0x03,       // SET_FEATURE
            w_value: feature,
            w_index: port as u16,
            w_length: 0,
        }
    }

    pub const fn get_hub_status() -> UsbSetupPacket {
        UsbSetupPacket {
            bm_request_type: 0xA0, // Device-to-Host | Class | Device
            b_request: 0x00,       // GET_STATUS
            w_value: 0,
            w_index: 0,
            w_length: 4,
        }
    }

    pub const fn clear_hub_feature(feature: u16) -> UsbSetupPacket {
        UsbSetupPacket {
            bm_request_type: 0x20, // Host-to-Device | Class | Device
            b_request: 0x01,       // CLEAR_FEATURE
            w_value: feature,
            w_index: 0,
            w_length: 0,
        }
    }

    pub const fn clear_port_feature(port: u8, feature: u16) -> UsbSetupPacket {
        UsbSetupPacket {
            bm_request_type: 0x23, // Host-to-Device | Class | Other (Port)
            b_request: 0x01,       // CLEAR_FEATURE
            w_value: feature,
            w_index: port as u16,
            w_length: 0,
        }
    }
}

/// Helper function to provide human-readable device names for known USB vendor and product IDs.
pub fn get_usb_product_description(vid: u16, pid: u16, class_code: u8) -> &'static str {
    match (vid, pid) {
        (0x8087, 0x0024) | (0x8087, 0x0020) => "Intel 6/7 Series Integrated Rate Matching Hub (RMH)",
        (0x0424, 0x2513) => "Microchip Technology / SMSC 2.0 Hub",
        (0x05AC, 0x0245) => "Apple Internal Keyboard / Wellspring 5 Trackpad (ANSI)",
        (0x05AC, 0x0246) => "Apple Internal Keyboard / Wellspring 5 Trackpad (ISO)",
        (0x05AC, 0x0247) => "Apple Internal Keyboard / Wellspring 5 Trackpad (JIS)",
        (0x05AC, 0x0252) => "Apple Internal Keyboard / Wellspring 6 Trackpad (ANSI)",
        (0x05AC, 0x0253) => "Apple Internal Keyboard / Wellspring 6 Trackpad (ISO)",
        (0x05AC, 0x0254) => "Apple Internal Keyboard / Wellspring 6 Trackpad (JIS)",
        (0x05AC, 0x0250) => "Apple Aluminum Keyboard with Built-in Hub",
        (0x05AC, 0x821A) => "Apple Broadcom BCM20702A0 Bluetooth 4.0 HCI",
        (0x05AC, 0x8242) => "Apple Built-in IR Receiver",
        (0x05AC, 0x8509) => "Apple FaceTime HD Camera (UVC)",
        (0x046D, 0xC077) => "Logitech M105/M90 Optical USB Mouse",
        (0x046D, 0xC31C) => "Logitech K120 USB Keyboard",
        (0x046D, 0xC52B) => "Logitech Unifying Wireless USB Receiver",
        (0x046D, _) => "Logitech USB Peripheral",
        (0x1B1C, _) => "Corsair Gaming USB Device",
        (0x1532, _) => "Razer USB Gaming Peripheral",
        (0x8086, _) => "Intel Corporation USB Device",
        _ => match class_code {
            0x09 => "Standard USB 2.0 Hub",
            0x03 => "USB Human Interface Device (HID)",
            0x08 => "USB Mass Storage Device",
            0x0E => "USB Video Class (Webcam / UVC)",
            0xE0 => "USB Wireless Controller (Bluetooth HCI)",
            _ => "Generic USB Peripheral",
        },
    }
}

/// Returns true when the device or any interface inside `cfg_buf` implements USB Hub Class `0x09`.
pub fn is_hub_device(dev_class: u8, cfg_buf: &[u8]) -> bool {
    if dev_class == USB_CLASS_HUB {
        return true;
    }
    let mut off = 0usize;
    while off + 2 <= cfg_buf.len() {
        let len = cfg_buf[off] as usize;
        if len < 2 || off + len > cfg_buf.len() {
            break;
        }
        if cfg_buf[off + 1] == USB_DESC_TYPE_INTERFACE && len >= 9 && cfg_buf[off + 5] == USB_CLASS_HUB {
            return true;
        }
        off += len;
    }
    false
}

/// Parse a full configuration descriptor tree and extract all HID Keyboard / Mouse Interrupt IN endpoints.
pub fn parse_hid_interrupt_endpoints(
    vendor_id: u16,
    product_id: u16,
    cfg_buf: &[u8],
) -> Vec<DiscoveredHidEndpoint> {
    let mut results = Vec::new();
    let wellspring = is_apple_wellspring_trackpad(vendor_id, product_id);

    let mut cur_iface_num = 0u8;
    let mut cur_iface_sub = 0u8;
    let mut cur_kind: Option<UsbHidType> = None;

    let mut off = 0usize;
    while off + 2 <= cfg_buf.len() {
        let len = cfg_buf[off] as usize;
        if len < 2 || off + len > cfg_buf.len() {
            break;
        }
        let dtype = cfg_buf[off + 1];

        match dtype {
            USB_DESC_TYPE_INTERFACE if len >= 9 => {
                cur_iface_num = cfg_buf[off + 2];
                let alt_setting = cfg_buf[off + 3];
                let iface_class = cfg_buf[off + 5];
                cur_iface_sub = cfg_buf[off + 6];
                let iface_proto = cfg_buf[off + 7];

                cur_kind = if alt_setting == 0 && iface_class == USB_CLASS_HID {
                    match iface_proto {
                        0x01 => Some(UsbHidType::Keyboard),
                        0x02 => Some(UsbHidType::Mouse),
                        _ => {
                            if (vendor_id == 0x05AC && cur_iface_num == 0)
                                || (cur_iface_sub == 0x01 && cur_iface_num == 0)
                            {
                                Some(UsbHidType::Keyboard)
                            } else if (wellspring && cur_iface_num == 2)
                                || (cur_iface_sub == 0x01 && cur_iface_num == 1)
                            {
                                Some(UsbHidType::Mouse)
                            } else {
                                None
                            }
                        }
                    }
                } else {
                    None
                };
            }
            USB_DESC_TYPE_ENDPOINT if len >= 7 => {
                if let Some(kind) = cur_kind {
                    let ep_addr = cfg_buf[off + 2];
                    let bm_attrs = cfg_buf[off + 3];
                    let max_pkt = u16::from_le_bytes([cfg_buf[off + 4], cfg_buf[off + 5]]) & 0x07FF;
                    let interval = cfg_buf[off + 6];

                    let is_in = (ep_addr & 0x80) != 0;
                    let is_interrupt = (bm_attrs & 0x03) == 0x03;
                    if is_in && is_interrupt {
                        results.push(DiscoveredHidEndpoint {
                            kind,
                            interface_num: cur_iface_num,
                            interface_subclass: cur_iface_sub,
                            endpoint_num: ep_addr & 0x0F,
                            max_packet_size: max_pkt.max(4),
                            interval_ms: interval.clamp(2, 16),
                            is_apple_wellspring: wellspring && kind == UsbHidType::Mouse,
                        });
                    }
                }
            }
            _ => {}
        }

        off += len;
    }

    results
}
