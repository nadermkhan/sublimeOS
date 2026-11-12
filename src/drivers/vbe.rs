use crate::mm::vmm::{VMM, PAGE_PRESENT, PAGE_WRITABLE, PAGE_WRITE_COMBINING};
use crate::mm::PAGE_SIZE;
use crate::{kinfo, kwarn};

pub const FRAMEBUFFER_VIRT_BASE: u64 = 0xFFFFFFFFD0000000;

pub struct VbeDisplay {
    pub width: u32,
    pub height: u32,
    pub bpp: u32,
    pub pitch: u32,
    pub lfb_phys: u64,
    pub framebuffer: *mut u32,
    pub text_mode_fallback: bool,
    pub red_pos: u8,
    pub red_mask: u8,
    pub green_pos: u8,
    pub green_mask: u8,
    pub blue_pos: u8,
    pub blue_mask: u8,
}

unsafe impl Send for VbeDisplay {}
unsafe impl Sync for VbeDisplay {}

impl VbeDisplay {
    pub const fn new() -> Self {
        Self {
            width: 0,
            height: 0,
            bpp: 0,
            pitch: 0,
            lfb_phys: 0,
            framebuffer: core::ptr::null_mut(),
            text_mode_fallback: false,
            red_pos: 16,
            red_mask: 8,
            green_pos: 8,
            green_mask: 8,
            blue_pos: 0,
            blue_mask: 8,
        }
    }
}

pub static VBE: crate::sync::IrqSafeSpinlock<VbeDisplay> =
    crate::sync::IrqSafeSpinlock::new(VbeDisplay::new());

/// Return the active framebuffer dimensions `(width, height)`, defaulting to `1024x768` before init.
pub fn dimensions() -> (u32, u32) {
    let vbe = VBE.lock();
    if vbe.width > 0 && vbe.height > 0 {
        (vbe.width, vbe.height)
    } else {
        (1024, 768)
    }
}

/// True if no graphical bootloader GOP/Multiboot LFB was supplied and the kernel stays in VGA Text Mode (0xB8000).
pub fn is_text_mode_fallback() -> bool {
    VBE.lock().text_mode_fallback
}

fn map_physical_framebuffer(phys_base: u64, total_bytes: usize) -> Result<(), &'static str> {
    let total_pages = (total_bytes + PAGE_SIZE - 1) / PAGE_SIZE;
    let vmm = VMM.lock();
    let kernel_pml4 = vmm.get_kernel_pml4();
    let flags = PAGE_PRESENT | PAGE_WRITABLE | PAGE_WRITE_COMBINING;

    for page in 0..total_pages {
        let virt = FRAMEBUFFER_VIRT_BASE + (page * PAGE_SIZE) as u64;
        let phys = phys_base + (page * PAGE_SIZE) as u64;
        vmm.map_page(kernel_pml4, virt, phys, flags)?;
    }
    Ok(())
}

fn set_framebuffer_state(
    width: u32,
    height: u32,
    bpp: u32,
    pitch: u32,
    lfb_phys: u64,
    text_fallback: bool,
    red_pos: u8,
    red_mask: u8,
    green_pos: u8,
    green_mask: u8,
    blue_pos: u8,
    blue_mask: u8,
) {
    let mut vbe = VBE.lock();
    vbe.width = width;
    vbe.height = height;
    vbe.bpp = bpp;
    vbe.pitch = pitch;
    vbe.lfb_phys = lfb_phys;
    vbe.framebuffer = FRAMEBUFFER_VIRT_BASE as *mut u32;
    vbe.text_mode_fallback = text_fallback;
    vbe.red_pos = red_pos;
    vbe.red_mask = red_mask;
    vbe.green_pos = green_pos;
    vbe.green_mask = green_mask;
    vbe.blue_pos = blue_pos;
    vbe.blue_mask = blue_mask;
}

/// Map bootloader-provided UEFI GOP / Multiboot Linear Framebuffer immediately after VMM init
/// so kernel boot logs and early panic traces appear visibly on hardware without legacy VGA/COM1.
pub fn init_early_gop() -> bool {
    if !VBE.lock().framebuffer.is_null() {
        return !VBE.lock().text_mode_fallback;
    }

    let mb_fb = crate::arch::x86_64::multiboot::MULTIBOOT.lock().framebuffer;
    if let Some(fb) = mb_fb {
        if fb.fb_type != 2 && fb.addr != 0 && fb.width > 0 && fb.height > 0 && fb.bpp >= 24 {
            let bytes_per_pixel = ((fb.bpp as u32) / 8).max(3);
            let pitch = if fb.pitch >= fb.width.saturating_mul(bytes_per_pixel) {
                fb.pitch
            } else {
                fb.width.saturating_mul(bytes_per_pixel)
            };
            let total_bytes = (pitch as usize).saturating_mul(fb.height as usize);
            if map_physical_framebuffer(fb.addr, total_bytes).is_ok() {
                set_framebuffer_state(
                    fb.width,
                    fb.height,
                    fb.bpp as u32,
                    pitch,
                    fb.addr,
                    false,
                    fb.red_pos,
                    fb.red_mask,
                    fb.green_pos,
                    fb.green_mask,
                    fb.blue_pos,
                    fb.blue_mask,
                );
                crate::drivers::gop_console::init_from_framebuffer(
                    FRAMEBUFFER_VIRT_BASE as *mut u8,
                    fb.width,
                    fb.height,
                    pitch,
                    fb.bpp as u32,
                    fb.red_pos,
                    fb.green_pos,
                    fb.blue_pos,
                );
                kinfo!(
                    "UEFI GOP / Multiboot LFB active: {}x{}@{}bpp, Pitch: {}, RGB({},{},{}) Phys: 0x{:X}",
                    fb.width,
                    fb.height,
                    fb.bpp,
                    pitch,
                    fb.red_pos,
                    fb.green_pos,
                    fb.blue_pos,
                    fb.addr
                );
                return true;
            }
        }
    }
    false
}

/// Initialize UEFI GOP / Multiboot Linear Framebuffer:
/// 1. Map the raw GOP / Multiboot physical framebuffer directly using firmware-provided
///    address, resolution, scanline pitch, bpp, and RGB/BGR color channel layout.
/// 2. Never issue unvalidated Bochs/legacy VBE I/O port writes (`0x1CE`/`0x1CF`) onto physical GPUs.
/// 3. If no graphical GOP/Multiboot framebuffer was provided, cleanly remain in VGA Text Mode (`0xB8000`)
///    with a RAM-backed virtual `/dev/fb0` canvas so `/dev/fb0` consumers stay functional without MMIO faults.
pub fn init(fallback_width: u32, fallback_height: u32) -> Result<(), &'static str> {
    if !VBE.lock().framebuffer.is_null() {
        return Ok(());
    }

    // 1. Prefer bootloader-provided UEFI GOP / Multiboot Linear Framebuffer (fb_type != 2 excludes EGA text)
    if init_early_gop() {
        return Ok(());
    }

    // 2. Graceful Bare-Metal Fallback: stay in VGA Text Mode (0xB8000) and back /dev/fb0 in RAM
    kwarn!(
        "Graphics: No UEFI GOP / Multiboot LFB detected; operating in VGA Text Mode (0xB8000) with RAM-backed /dev/fb0."
    );

    let width = fallback_width.max(640);
    let height = fallback_height.max(480);
    let pitch = width * 4;
    let total_bytes = (pitch as usize).saturating_mul(height as usize);
    let total_pages = (total_bytes + PAGE_SIZE - 1) / PAGE_SIZE;

    {
        let vmm = VMM.lock();
        let kernel_pml4 = vmm.get_kernel_pml4();
        let flags = PAGE_PRESENT | PAGE_WRITABLE;

        for page in 0..total_pages {
            let virt = FRAMEBUFFER_VIRT_BASE + (page * PAGE_SIZE) as u64;
            // Allocate and zero each virtual framebuffer page outside PMM lock before mapping page tables
            let phys = crate::mm::pmm::alloc_zeroed_frame()
                .ok_or("Out of memory for virtual framebuffer")?;
            vmm.map_page(kernel_pml4, virt, phys, flags)?;
        }
    }

    set_framebuffer_state(width, height, 32, pitch, 0, true, 16, 8, 8, 8, 0, 8);
    Ok(())
}
