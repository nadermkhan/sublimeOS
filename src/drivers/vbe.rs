use crate::arch::x86_64::io::{inw, outw};
use crate::drivers::pci::PCI;
use crate::mm::vmm::{VMM, PAGE_PRESENT, PAGE_WRITABLE, PAGE_WRITE_THROUGH};
use crate::mm::PAGE_SIZE;
use crate::kinfo;

pub const VBE_DISPI_IOPORT_INDEX: u16 = 0x1CE;
pub const VBE_DISPI_IOPORT_DATA: u16 = 0x1CF;

pub const VBE_DISPI_INDEX_ID: u16 = 0;
pub const VBE_DISPI_INDEX_XRES: u16 = 1;
pub const VBE_DISPI_INDEX_YRES: u16 = 2;
pub const VBE_DISPI_INDEX_BPP: u16 = 3;
pub const VBE_DISPI_INDEX_ENABLE: u16 = 4;
pub const VBE_DISPI_INDEX_BANK: u16 = 5;
pub const VBE_DISPI_INDEX_VIRT_WIDTH: u16 = 6;
pub const VBE_DISPI_INDEX_VIRT_HEIGHT: u16 = 7;
pub const VBE_DISPI_INDEX_X_OFFSET: u16 = 8;
pub const VBE_DISPI_INDEX_Y_OFFSET: u16 = 9;

pub const VBE_DISPI_DISABLED: u16 = 0x00;
pub const VBE_DISPI_ENABLED: u16 = 0x01;
pub const VBE_DISPI_LFB_ENABLED: u16 = 0x40;
pub const VBE_DISPI_NOCLEARMEM: u16 = 0x80;

pub const FRAMEBUFFER_VIRT_BASE: u64 = 0xFFFFFFFFD0000000;

pub struct VbeDisplay {
    pub width: u32,
    pub height: u32,
    pub bpp: u32,
    pub pitch: u32,
    pub lfb_phys: u64,
    pub framebuffer: *mut u32,
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
        }
    }
}

pub static VBE: crate::sync::IrqSafeSpinlock<VbeDisplay> =
    crate::sync::IrqSafeSpinlock::new(VbeDisplay::new());

unsafe fn vbe_write(index: u16, data: u16) {
    outw(VBE_DISPI_IOPORT_INDEX, index);
    outw(VBE_DISPI_IOPORT_DATA, data);
}

unsafe fn vbe_read(index: u16) -> u16 {
    outw(VBE_DISPI_IOPORT_INDEX, index);
    inw(VBE_DISPI_IOPORT_DATA)
}

/// Check if Bochs VBE display adapter is present.
pub fn is_available() -> bool {
    let id = unsafe { vbe_read(VBE_DISPI_INDEX_ID) };
    (id >= 0xB0C0) && (id <= 0xB0C6)
}

/// Initialize Bochs VBE display adapter to width x height x 32bpp and map framebuffer.
pub fn init(width: u32, height: u32) -> Result<(), &'static str> {
    if !VBE.lock().framebuffer.is_null() {
        return Ok(());
    }

    if !is_available() {
        return Err("Bochs VBE display adapter not detected");
    }

    // 1. Locate VGA display controller in PCI devices to get BAR0
    let mut lfb_phys = 0xFD000000u64; // Default fallback in QEMU
    {
        let pci = PCI.lock();
        for dev in pci.devices.iter() {
            if (dev.vendor_id == 0x1234 && dev.device_id == 0x1111) || dev.class_code == 0x03 {
                if dev.bar0 != 0 {
                    lfb_phys = dev.bar0 as u64;
                    break;
                }
            }
        }
    }

    // 2. Set Bochs VBE mode
    unsafe {
        vbe_write(VBE_DISPI_INDEX_ENABLE, VBE_DISPI_DISABLED);
        vbe_write(VBE_DISPI_INDEX_XRES, width as u16);
        vbe_write(VBE_DISPI_INDEX_YRES, height as u16);
        vbe_write(VBE_DISPI_INDEX_BPP, 32);
        vbe_write(VBE_DISPI_INDEX_ENABLE, VBE_DISPI_ENABLED | VBE_DISPI_LFB_ENABLED);
    }

    // 3. Map physical framebuffer into virtual address space
    let total_bytes = (width * height * 4) as usize;
    let total_pages = (total_bytes + PAGE_SIZE - 1) / PAGE_SIZE;

    {
        let vmm = VMM.lock();
        let kernel_pml4 = vmm.get_kernel_pml4();
        let flags = PAGE_PRESENT | PAGE_WRITABLE | PAGE_WRITE_THROUGH;

        for page in 0..total_pages {
            let virt = FRAMEBUFFER_VIRT_BASE + (page * PAGE_SIZE) as u64;
            let phys = lfb_phys + (page * PAGE_SIZE) as u64;
            vmm.map_page(kernel_pml4, virt, phys, flags)?;
        }
    }

    {
        let mut vbe = VBE.lock();
        vbe.width = width;
        vbe.height = height;
        vbe.bpp = 32;
        vbe.pitch = width * 4;
        vbe.lfb_phys = lfb_phys;
        vbe.framebuffer = FRAMEBUFFER_VIRT_BASE as *mut u32;
    }

    kinfo!(
        "Bochs VBE initialized: {}x{}x32bpp, LFB: 0x{:08X} -> 0x{:016X}",
        width,
        height,
        lfb_phys,
        FRAMEBUFFER_VIRT_BASE
    );

    Ok(())
}
