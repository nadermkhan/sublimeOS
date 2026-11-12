//! Intel HD 3000 / Sandy Bridge (Gen6) Display Engine & Hardware Cursor Driver
//!
//! Provides bare-metal hardware cursor plane management (`CURACNTR`, `CURABASE`, `CURAPOS`),
//! hardware page flipping (`DSPASURF`), and vertical blank status (`DEISR`) for
//! Intel 6-Series integrated graphics controllers (PCI `0x8086:0x0116`, `0x8086:0x0126`, etc.).

use core::ptr::{read_volatile, write_volatile};
use crate::drivers::pci::{PciBus, PciDevice, PCI};
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

// Intel Gen6 / Sandy Bridge Display Engine MMIO Register Offsets
pub const I915_DEISR: u64 = 0x44000;    // Display Engine Interrupt Status
pub const I915_DEIMR: u64 = 0x44004;    // Display Engine Interrupt Mask
pub const I915_DEIER: u64 = 0x44008;    // Display Engine Interrupt Enable

pub const I915_CURACNTR: u64 = 0x70080; // Cursor A Control
pub const I915_CURABASE: u64 = 0x70084; // Cursor A Base Address
pub const I915_CURAPOS: u64 = 0x70088;  // Cursor A Position

pub const I915_DSPACNTR: u64 = 0x70180; // Display Plane A Control
pub const I915_DSPASURF: u64 = 0x7019C; // Display Plane A Surface Base (Page Flip)

pub const CURSOR_MODE_64X64_ARGB: u32 = 0x0000_0027; // 64x64 32bpp ARGB cursor
pub const CURSOR_ENABLE_BIT: u32 = 1 << 31;
pub const DE_PIPEA_VBLANK: u32 = 1 << 0;
pub const DE_PIPEB_VBLANK: u32 = 1 << 5;

pub struct IntelGen6Gpu {
    pub pci: Option<PciDevice>,
    pub mmio_base: u64,
    pub active: bool,
    pub cursor_enabled: bool,
}

unsafe impl Send for IntelGen6Gpu {}
unsafe impl Sync for IntelGen6Gpu {}

impl IntelGen6Gpu {
    pub const fn new() -> Self {
        Self {
            pci: None,
            mmio_base: 0,
            active: false,
            cursor_enabled: false,
        }
    }

    #[inline(always)]
    pub unsafe fn read_reg(&self, offset: u64) -> u32 {
        if self.mmio_base != 0 {
            read_volatile((self.mmio_base + offset) as *const u32)
        } else {
            0
        }
    }

    #[inline(always)]
    pub unsafe fn write_reg(&self, offset: u64, val: u32) {
        if self.mmio_base != 0 {
            write_volatile((self.mmio_base + offset) as *mut u32, val);
        }
    }
}

pub static INTEL_I915: IrqSafeSpinlock<IntelGen6Gpu> = IrqSafeSpinlock::new(IntelGen6Gpu::new());

/// Returns `true` if `dev` is an Intel integrated display controller (PCI Class 0x03).
pub fn is_intel_vga(dev: &PciDevice) -> bool {
    dev.vendor_id == 0x8086 && dev.class_code == 0x03
}

/// Initialize the Intel Gen6 Display Engine driver if an Intel integrated GPU is present.
pub fn init() {
    let pci_dev = {
        let pci = PCI.lock();
        pci.devices.iter().find(|d| is_intel_vga(d)).cloned()
    };

    let dev = match pci_dev {
        Some(d) => d,
        None => return,
    };

    let bar0_phys = dev.read_bar(0);
    if bar0_phys == 0 {
        return;
    }

    // Intel Sandy Bridge / Ivy Bridge BAR0 is 2 MiB / 4 MiB MMIO
    let mmio_base = match crate::mm::vmm::map_mmio_range(bar0_phys, 4 * 1024 * 1024) {
        Ok(addr) => addr,
        Err(_) => return,
    };

    unsafe {
        PciBus::enable_bus_mastering(dev.bus, dev.device, dev.function);
    }

    let mut gpu = INTEL_I915.lock();
    gpu.pci = Some(dev.clone());
    gpu.mmio_base = mmio_base;
    gpu.active = true;

    kinfo!(
        "Intel Gen6 HD Graphics (Device 8086:{:04X}) Display Engine online (MMIO: 0x{:X})",
        dev.device_id,
        bar0_phys
    );
}

/// Update hardware cursor overlay position via single 32-bit register write (`CURAPOS 0x70088`).
/// Operates in sub-microsecond time with zero redrawing of underlying framebuffer pixels.
pub fn set_cursor_position(x: i32, y: i32) {
    let gpu = match INTEL_I915.try_lock() {
        Some(g) => g,
        None => return,
    };
    if !gpu.active || gpu.mmio_base == 0 {
        return;
    }

    let mut pos_val: u32 = 0;
    if x < 0 {
        pos_val |= (1 << 15) | ((-x) as u32 & 0x7FF);
    } else {
        pos_val |= x as u32 & 0xFFF;
    }

    if y < 0 {
        pos_val |= (1 << 31) | (((-y) as u32 & 0x7FF) << 16);
    } else {
        pos_val |= (y as u32 & 0xFFF) << 16;
    }

    unsafe {
        gpu.write_reg(I915_CURAPOS, pos_val);
    }
}

/// Enable Intel hardware cursor plane pointing to `cursor_base_phys` (64x64 32bpp ARGB buffer).
pub fn enable_cursor(cursor_base_phys: u32) {
    let mut gpu = INTEL_I915.lock();
    if !gpu.active || gpu.mmio_base == 0 {
        return;
    }
    unsafe {
        gpu.write_reg(I915_CURABASE, cursor_base_phys);
        gpu.write_reg(I915_CURACNTR, CURSOR_MODE_64X64_ARGB | CURSOR_ENABLE_BIT);
    }
    gpu.cursor_enabled = true;
}

/// Disable hardware cursor plane.
pub fn disable_cursor() {
    let mut gpu = INTEL_I915.lock();
    if !gpu.active || gpu.mmio_base == 0 {
        return;
    }
    unsafe {
        gpu.write_reg(I915_CURACNTR, 0);
    }
    gpu.cursor_enabled = false;
}

/// Perform an atomic hardware page flip by updating Display Plane A Surface Base (`DSPASURF 0x7019C`).
pub fn page_flip(surface_phys: u32) {
    let gpu = match INTEL_I915.try_lock() {
        Some(g) => g,
        None => return,
    };
    if !gpu.active || gpu.mmio_base == 0 {
        return;
    }
    unsafe {
        gpu.write_reg(I915_DSPASURF, surface_phys);
    }
}

/// Bounded spin wait for next vertical blank event on Pipe A via `DEISR 0x44000`.
pub fn wait_vblank() {
    let gpu = match INTEL_I915.try_lock() {
        Some(g) => g,
        None => return,
    };
    if !gpu.active || gpu.mmio_base == 0 {
        return;
    }
    for _ in 0..100_000 {
        let deisr = unsafe { gpu.read_reg(I915_DEISR) };
        if (deisr & DE_PIPEA_VBLANK) != 0 {
            // Clear status bit by writing 1
            unsafe {
                gpu.write_reg(I915_DEISR, DE_PIPEA_VBLANK);
            }
            break;
        }
        core::hint::spin_loop();
    }
}
