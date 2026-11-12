//! Hardware Platform & Embedded Controller Drivers.

pub mod apple;
pub mod smbios;

/// Probe and initialize platform-specific embedded controllers (Apple SMC / gmux) and SMBIOS.
pub fn init() {
    smbios::init();
    apple::ApplePlatformHardware::init();
}
