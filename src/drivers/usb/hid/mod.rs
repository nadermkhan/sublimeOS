extern crate alloc;
pub mod keyboard;
pub mod trackpad;

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::drivers::keyboard::KEYBOARD;
use crate::drivers::mouse::MOUSE;
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbHidType {
    Keyboard,
    Mouse,
}

pub struct UsbHidDevice {
    pub name: String,
    pub dev_type: UsbHidType,
    pub prev_modifiers: u8,
    pub prev_keys: [u8; 6],
    pub prev_buttons: u8,
}

impl UsbHidDevice {
    pub fn new(name: &str, dev_type: UsbHidType) -> Self {
        Self {
            name: String::from(name),
            dev_type,
            prev_modifiers: 0,
            prev_keys: [0; 6],
            prev_buttons: 0,
        }
    }

    /// Process standard 8-byte or Apple 10-byte USB HID Keyboard Report.
    pub fn process_keyboard_report(&mut self, report: &[u8]) {
        if report.len() < 8 {
            return;
        }

        let (modifiers, keys) = if report.len() >= 9 && report[0] <= 0x03 {
            let mut k = [0u8; 6];
            k.copy_from_slice(&report[3..9]);
            (report[1], k)
        } else {
            let mut k = [0u8; 6];
            k.copy_from_slice(&report[2..8]);
            (report[0], k)
        };

        let mut kb = KEYBOARD.lock();

        // 1. Process Modifiers
        // Left Ctrl (bit 0)
        Self::handle_modifier_bit(&mut kb, modifiers, self.prev_modifiers, 0x01, 0x1D);
        // Left Shift (bit 1)
        Self::handle_modifier_bit(&mut kb, modifiers, self.prev_modifiers, 0x02, 0x2A);
        // Left Alt (bit 2)
        Self::handle_modifier_bit(&mut kb, modifiers, self.prev_modifiers, 0x04, 0x38);
        // Right Ctrl (bit 4)
        Self::handle_modifier_bit(&mut kb, modifiers, self.prev_modifiers, 0x10, 0x1D);
        // Right Shift (bit 5)
        Self::handle_modifier_bit(&mut kb, modifiers, self.prev_modifiers, 0x20, 0x36);
        // Right Alt (bit 6)
        Self::handle_modifier_bit(&mut kb, modifiers, self.prev_modifiers, 0x40, 0x38);

        self.prev_modifiers = modifiers;

        // 2. Detect Released Keys (in prev_keys but not in new keys)
        for &old_k in &self.prev_keys {
            if old_k != 0 && !keys.contains(&old_k) {
                if let Some((scancode, extended)) = hid_usage_to_ps2(old_k) {
                    if extended {
                        kb.handle_scancode(0xE0);
                    }
                    kb.handle_scancode(scancode | 0x80); // Break code
                }
            }
        }

        // 3. Detect Newly Pressed Keys (in new keys but not in prev_keys)
        for &new_k in &keys {
            if new_k != 0 && !self.prev_keys.contains(&new_k) {
                if let Some((scancode, extended)) = hid_usage_to_ps2(new_k) {
                    if extended {
                        kb.handle_scancode(0xE0);
                    }
                    kb.handle_scancode(scancode); // Make code
                }
            }
        }

        self.prev_keys = keys;
    }

    fn handle_modifier_bit(
        kb: &mut crate::drivers::keyboard::KeyboardDriver,
        curr: u8,
        prev: u8,
        mask: u8,
        scancode: u8,
    ) {
        let is_now = (curr & mask) != 0;
        let was_before = (prev & mask) != 0;
        if is_now && !was_before {
            kb.handle_scancode(scancode); // Make
        } else if !is_now && was_before {
            kb.handle_scancode(scancode | 0x80); // Break
        }
    }

    /// Process standard 3 or 4-byte USB HID Boot Mouse Report:
    /// Byte 0: Buttons (Bit 0: Left, Bit 1: Right, Bit 2: Middle)
    /// Byte 1: X displacement (signed 8-bit i8)
    /// Byte 2: Y displacement (signed 8-bit i8)
    /// Byte 3: Optional scroll wheel (signed 8-bit i8)
    pub fn process_mouse_report(&mut self, report: &[u8]) {
        if report.len() < 3 {
            return;
        }

        let buttons = report[0];
        let dx = report[1] as i8;
        let dy = report[2] as i8;

        let mut mouse = MOUSE.lock();
        mouse.x = (mouse.x + dx as i32).clamp(0, mouse.screen_width - 1);
        mouse.y = (mouse.y + dy as i32).clamp(0, mouse.screen_height - 1);

        mouse.left_button = (buttons & 0x01) != 0;
        mouse.right_button = (buttons & 0x02) != 0;
        mouse.middle_button = (buttons & 0x04) != 0;

        self.prev_buttons = buttons;
    }
}

/// Convert USB HID Usage ID (page 0x07) to PS/2 Set 1 Scancode and extended flag.
pub fn hid_usage_to_ps2(usage: u8) -> Option<(u8, bool)> {
    match usage {
        // Letters A-Z
        0x04 => Some((0x1E, false)), // A
        0x05 => Some((0x30, false)), // B
        0x06 => Some((0x2E, false)), // C
        0x07 => Some((0x20, false)), // D
        0x08 => Some((0x12, false)), // E
        0x09 => Some((0x21, false)), // F
        0x0A => Some((0x22, false)), // G
        0x0B => Some((0x23, false)), // H
        0x0C => Some((0x17, false)), // I
        0x0D => Some((0x24, false)), // J
        0x0E => Some((0x25, false)), // K
        0x0F => Some((0x26, false)), // L
        0x10 => Some((0x32, false)), // M
        0x11 => Some((0x31, false)), // N
        0x12 => Some((0x18, false)), // O
        0x13 => Some((0x19, false)), // P
        0x14 => Some((0x10, false)), // Q
        0x15 => Some((0x13, false)), // R
        0x16 => Some((0x1F, false)), // S
        0x17 => Some((0x14, false)), // T
        0x18 => Some((0x16, false)), // U
        0x19 => Some((0x2F, false)), // V
        0x1A => Some((0x11, false)), // W
        0x1B => Some((0x2D, false)), // X
        0x1C => Some((0x15, false)), // Y
        0x1D => Some((0x2C, false)), // Z

        // Numbers 1-9, 0
        0x1E => Some((0x02, false)), // 1
        0x1F => Some((0x03, false)), // 2
        0x20 => Some((0x04, false)), // 3
        0x21 => Some((0x05, false)), // 4
        0x22 => Some((0x06, false)), // 5
        0x23 => Some((0x07, false)), // 6
        0x24 => Some((0x08, false)), // 7
        0x25 => Some((0x09, false)), // 8
        0x26 => Some((0x0A, false)), // 9
        0x27 => Some((0x0B, false)), // 0

        // Common keys
        0x28 => Some((0x1C, false)), // Enter
        0x29 => Some((0x01, false)), // Escape
        0x2A => Some((0x0E, false)), // Backspace
        0x2B => Some((0x0F, false)), // Tab
        0x2C => Some((0x39, false)), // Space
        0x2D => Some((0x0C, false)), // - / _
        0x2E => Some((0x0D, false)), // = / +
        0x2F => Some((0x1A, false)), // [
        0x30 => Some((0x1B, false)), // ]
        0x31 => Some((0x2B, false)), // \
        0x33 => Some((0x27, false)), // ;
        0x34 => Some((0x28, false)), // '
        0x35 => Some((0x29, false)), // `
        0x36 => Some((0x33, false)), // ,
        0x37 => Some((0x34, false)), // .
        0x38 => Some((0x35, false)), // /

        // Function keys F1-F12
        0x3A => Some((0x3B, false)), // F1
        0x3B => Some((0x3C, false)), // F2
        0x3C => Some((0x3D, false)), // F3
        0x3D => Some((0x3E, false)), // F4
        0x3E => Some((0x3F, false)), // F5
        0x3F => Some((0x40, false)), // F6
        0x40 => Some((0x41, false)), // F7
        0x41 => Some((0x42, false)), // F8
        0x42 => Some((0x43, false)), // F9
        0x43 => Some((0x44, false)), // F10
        0x44 => Some((0x57, false)), // F11
        0x45 => Some((0x58, false)), // F12

        // Extended navigation / cursor keys
        0x4A => Some((0x47, true)),  // Home
        0x4C => Some((0x53, true)),  // Delete
        0x4D => Some((0x4F, true)),  // End
        0x4F => Some((0x4D, true)),  // Right Arrow
        0x50 => Some((0x4B, true)),  // Left Arrow
        0x51 => Some((0x50, true)),  // Down Arrow
        0x52 => Some((0x48, true)),  // Up Arrow

        _ => None,
    }
}

pub struct UsbHidManager {
    pub devices: Vec<Arc<IrqSafeSpinlock<UsbHidDevice>>>,
}

impl UsbHidManager {
    pub const fn new() -> Self {
        Self {
            devices: Vec::new(),
        }
    }

    pub fn register(&mut self, dev: UsbHidDevice) -> Arc<IrqSafeSpinlock<UsbHidDevice>> {
        let name = dev.name.clone();
        let dev_type = dev.dev_type;
        let arc = Arc::new(IrqSafeSpinlock::new(dev));
        self.devices.push(arc.clone());
        kinfo!("Registered USB HID {:?} Input Device: {}", dev_type, name);
        arc
    }

    pub fn get_device(&self, name: &str) -> Option<Arc<IrqSafeSpinlock<UsbHidDevice>>> {
        for dev in &self.devices {
            if dev.lock().name == name {
                return Some(dev.clone());
            }
        }
        None
    }
}

pub static USB_HID: IrqSafeSpinlock<UsbHidManager> = IrqSafeSpinlock::new(UsbHidManager::new());
