//! GPU & Display Controller Drivers
//!
//! Provides hardware-accelerated display engine support, hardware cursor overlays,
//! atomic page-flipping, and panel multiplexing.

pub mod i915;

pub fn init() {
    i915::init();
}
