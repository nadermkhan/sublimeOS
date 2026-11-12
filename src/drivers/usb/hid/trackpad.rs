//! USB HID Boot Mouse & Apple Wellspring Trackpad Emulation Driver.
//!
//! Late 2011 MacBookPro8,2 exposes an Apple Internal Keyboard/Trackpad composite
//! USB device (VID `0x05AC`, BCM5974 Wellspring 5: PID `0x0245`/`0x0246`/`0x0247`).
//! Sending `SET_PROTOCOL(0)` (Boot Protocol) switches its Boot Mouse HID interface
//! into 3/4-byte relative `[buttons, dx, dy, (wheel)]` Boot Mouse Emulation Mode,
//! allowing pointer input before full proprietary multi-touch calibration.

pub const APPLE_USB_VID: u16 = 0x05AC;

/// Known Apple Wellspring internal USB Trackpad product IDs (including MacBookPro8,2 BCM5974).
pub const APPLE_WELLSPRING_PIDS: &[u16] = &[
    0x0236, 0x0237, 0x0238, // Wellspring 3
    0x0240, 0x0241, 0x0242, // Wellspring 4
    0x0245, 0x0246, 0x0247, // Wellspring 5 (MacBookPro8,1 / 8,2 / 8,3 ANSI/ISO/JIS)
    0x0249, 0x024A, 0x024B, // Wellspring 5A
    0x0252, 0x0253, 0x0254, // Wellspring 6
    0x0259, 0x025A, 0x025B, // Wellspring 7
    0x0262, 0x0263, 0x0264, // Wellspring 7A
    0x0272, 0x0273, 0x0274, // Wellspring 8
];

/// USB Standard Control Setup Packet (8 bytes).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsbSetupPacket {
    pub bm_request_type: u8,
    pub b_request: u8,
    pub w_value: u16,
    pub w_index: u16,
    pub w_length: u16,
}

impl UsbSetupPacket {
    /// USB HID Class Request `SET_PROTOCOL` (`bmRequestType=0x21`, `bRequest=0x0B`):
    /// `protocol = 0` switches the interface to Boot Protocol (Mouse/Keyboard Emulation),
    /// `protocol = 1` selects Report Protocol.
    pub const fn hid_set_protocol(interface: u16, boot_protocol: bool) -> Self {
        Self {
            bm_request_type: 0x21, // Host-to-device | Class | Interface
            b_request: 0x0B,       // SET_PROTOCOL
            w_value: if boot_protocol { 0x0000 } else { 0x0001 },
            w_index: interface,
            w_length: 0,
        }
    }

    /// USB HID Class Request `SET_IDLE` (`bmRequestType=0x21`, `bRequest=0x0A`) to silence redundant
    /// duplicate reports when pointer/keys are idle (`duration_4ms = 0` -> indefinite until state change).
    pub const fn hid_set_idle(interface: u16, duration_4ms: u8, report_id: u8) -> Self {
        Self {
            bm_request_type: 0x21,
            b_request: 0x0A,
            w_value: ((duration_4ms as u16) << 8) | (report_id as u16),
            w_index: interface,
            w_length: 0,
        }
    }

    pub const fn to_bytes(self) -> [u8; 8] {
        [
            self.bm_request_type,
            self.b_request,
            (self.w_value & 0xFF) as u8,
            (self.w_value >> 8) as u8,
            (self.w_index & 0xFF) as u8,
            (self.w_index >> 8) as u8,
            (self.w_length & 0xFF) as u8,
            (self.w_length >> 8) as u8,
        ]
    }
}

/// Returns `true` when `(vid, pid)` matches an Apple USB Wellspring trackpad.
pub fn is_apple_wellspring_trackpad(vid: u16, pid: u16) -> bool {
    vid == APPLE_USB_VID && APPLE_WELLSPRING_PIDS.contains(&pid)
}

/// Build the 8-byte setup request to switch a HID mouse/trackpad interface into Boot Mouse mode.
pub const fn build_boot_protocol_setup(interface: u16) -> [u8; 8] {
    UsbSetupPacket::hid_set_protocol(interface, true).to_bytes()
}

/// Apple Wellspring & generic USB Boot Mouse driver forwarding relative displacements to `MOUSE`.
#[derive(Debug, Clone)]
pub struct AppleTrackpadDriver {
    pub vendor_id: u16,
    pub product_id: u16,
    pub boot_emulation_enabled: bool,
    pub reports_processed: u64,
}

impl AppleTrackpadDriver {
    pub const fn new(vendor_id: u16, product_id: u16) -> Self {
        Self {
            vendor_id,
            product_id,
            boot_emulation_enabled: false,
            reports_processed: 0,
        }
    }

    /// Construct a default driver configured for MacBookPro8,2 Boot Mouse Emulation.
    pub const fn new_boot_mouse() -> Self {
        Self::new(APPLE_USB_VID, 0x0245)
    }

    pub fn is_apple_wellspring(&self) -> bool {
        is_apple_wellspring_trackpad(self.vendor_id, self.product_id)
    }

    pub fn mark_boot_emulation_active(&mut self, active: bool) {
        self.boot_emulation_enabled = active;
    }

    /// Decode a standard 3-byte, 4-byte, or 8-byte USB HID Boot Mouse report.
    pub fn process_report(&mut self, report: &[u8]) {
        if report.len() < 3 {
            return;
        }
        let buttons = report[0] & 0x07;
        let dx = report[1] as i8;
        let dy = report[2] as i8;
        crate::drivers::mouse::apply_relative_event(buttons, dx, dy);
        self.reports_processed = self.reports_processed.wrapping_add(1);
    }
}
