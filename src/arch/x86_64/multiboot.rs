use crate::mm::phys_to_virt;
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

pub const MULTIBOOT1_MAGIC: u32 = 0x2BADB002;
pub const MULTIBOOT2_MAGIC: u32 = 0x36D76289;

#[derive(Debug, Clone, Copy)]
pub struct FramebufferInfo {
    pub addr: u64,
    pub pitch: u32,
    pub width: u32,
    pub height: u32,
    pub bpp: u8,
    pub fb_type: u8,
}

#[derive(Debug, Clone, Copy)]
pub struct MultibootInfoData {
    pub magic: u32,
    pub raw_addr: u64,
    pub mem_lower_kb: u32,
    pub mem_upper_kb: u32,
    pub framebuffer: Option<FramebufferInfo>,
    pub bootloader_name: Option<&'static str>,
}

impl MultibootInfoData {
    pub const fn empty() -> Self {
        Self {
            magic: 0,
            raw_addr: 0,
            mem_lower_kb: 0,
            mem_upper_kb: 0,
            framebuffer: None,
            bootloader_name: None,
        }
    }
}

pub static MULTIBOOT: IrqSafeSpinlock<MultibootInfoData> =
    IrqSafeSpinlock::new(MultibootInfoData::empty());

/// Parse Multiboot 1 or Multiboot 2 info structure passed by bootloader (GRUB, Ventoy, QEMU).
pub fn parse_boot_info(magic: u64, mb_info_phys: u64) {
    let mut data = MultibootInfoData::empty();
    data.magic = magic as u32;
    data.raw_addr = mb_info_phys;

    if mb_info_phys == 0 {
        *MULTIBOOT.lock() = data;
        return;
    }

    if magic as u32 == MULTIBOOT1_MAGIC {
        parse_multiboot1(mb_info_phys, &mut data);
    } else if magic as u32 == MULTIBOOT2_MAGIC {
        parse_multiboot2(mb_info_phys, &mut data);
    }

    if let Some(fb) = data.framebuffer {
        kinfo!(
            "Multiboot Framebuffer Detected: {}x{}@{}bpp, Pitch: {}, PhysAddr: 0x{:X}",
            fb.width,
            fb.height,
            fb.bpp,
            fb.pitch,
            fb.addr
        );
    }

    *MULTIBOOT.lock() = data;
}

fn parse_multiboot1(mb_info_phys: u64, out: &mut MultibootInfoData) {
    let virt_addr = phys_to_virt(mb_info_phys);
    let ptr = virt_addr as *const u32;

    unsafe {
        let flags = *ptr;

        // Flags bit 0: mem_lower and mem_upper are valid
        if (flags & 0x01) != 0 {
            out.mem_lower_kb = *ptr.add(1);
            out.mem_upper_kb = *ptr.add(2);
        }

        // Flags bit 12 (0x1000): Linear framebuffer table is valid
        if (flags & (1 << 12)) != 0 {
            let fb_ptr = (virt_addr + 88) as *const u8;
            let fb_addr = *(fb_ptr as *const u64);
            let pitch = *(fb_ptr.add(8) as *const u32);
            let width = *(fb_ptr.add(12) as *const u32);
            let height = *(fb_ptr.add(16) as *const u32);
            let bpp = *fb_ptr.add(20);
            let fb_type = *fb_ptr.add(21);

            if fb_addr != 0 && width > 0 && height > 0 && bpp >= 24 {
                out.framebuffer = Some(FramebufferInfo {
                    addr: fb_addr,
                    pitch,
                    width,
                    height,
                    bpp,
                    fb_type,
                });
            }
        }
        // Flags bit 11 (0x800): VBE Mode info is valid (Fallback)
        else if (flags & (1 << 11)) != 0 {
            let vbe_mode_info_phys = *ptr.add(19);
            if vbe_mode_info_phys != 0 {
                let mode_info_virt = phys_to_virt(vbe_mode_info_phys as u64) as *const u8;
                let pitch = *(mode_info_virt.add(16) as *const u16) as u32;
                let width = *(mode_info_virt.add(18) as *const u16) as u32;
                let height = *(mode_info_virt.add(20) as *const u16) as u32;
                let bpp = *mode_info_virt.add(25);
                let phys_base = *(mode_info_virt.add(40) as *const u32) as u64;

                if phys_base != 0 && width > 0 && height > 0 && bpp >= 24 {
                    out.framebuffer = Some(FramebufferInfo {
                        addr: phys_base,
                        pitch,
                        width,
                        height,
                        bpp,
                        fb_type: 1,
                    });
                }
            }
        }
    }
}

fn parse_multiboot2(mb_info_phys: u64, out: &mut MultibootInfoData) {
    let virt_addr = phys_to_virt(mb_info_phys);
    let total_size = unsafe { *(virt_addr as *const u32) } as usize;

    let mut offset = 8; // Skip total_size and reserved
    while offset + 8 < total_size {
        let tag_ptr = (virt_addr + offset as u64) as *const u8;
        let tag_type = unsafe { *(tag_ptr as *const u32) };
        let tag_size = unsafe { *(tag_ptr.add(4) as *const u32) } as usize;

        if tag_type == 0 {
            break; // End tag
        }

        // Tag 8: Framebuffer info in Multiboot 2
        if tag_type == 8 && tag_size >= 28 {
            let fb_addr = unsafe { *(tag_ptr.add(8) as *const u64) };
            let pitch = unsafe { *(tag_ptr.add(16) as *const u32) };
            let width = unsafe { *(tag_ptr.add(20) as *const u32) };
            let height = unsafe { *(tag_ptr.add(24) as *const u32) };
            let bpp = unsafe { *tag_ptr.add(28) };
            let fb_type = unsafe { *tag_ptr.add(29) };

            if fb_addr != 0 && width > 0 && height > 0 && bpp >= 24 {
                out.framebuffer = Some(FramebufferInfo {
                    addr: fb_addr,
                    pitch,
                    width,
                    height,
                    bpp,
                    fb_type,
                });
            }
        }

        // Align tag size up to 8 bytes
        offset += (tag_size + 7) & !7;
    }
}
