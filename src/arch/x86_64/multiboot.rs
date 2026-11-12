use crate::mm::phys_to_virt;
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

pub const MULTIBOOT1_MAGIC: u32 = 0x2BADB002;
pub const MULTIBOOT2_MAGIC: u32 = 0x36D76289;
pub const MAX_MMAP_ENTRIES: usize = 128;

pub const MMAP_TYPE_AVAILABLE: u32 = 1;
pub const MMAP_TYPE_RESERVED: u32 = 2;
pub const MMAP_TYPE_ACPI_RECLAIMABLE: u32 = 3;
pub const MMAP_TYPE_ACPI_NVS: u32 = 4;
pub const MMAP_TYPE_BADRAM: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryMapEntry {
    pub base_addr: u64,
    pub length: u64,
    pub region_type: u32,
}

impl MemoryMapEntry {
    pub const fn empty() -> Self {
        Self {
            base_addr: 0,
            length: 0,
            region_type: 0,
        }
    }

    #[inline]
    pub const fn is_available(&self) -> bool {
        self.region_type == MMAP_TYPE_AVAILABLE && self.length > 0
    }

    #[inline]
    pub const fn end_addr(&self) -> u64 {
        self.base_addr.saturating_add(self.length)
    }

    pub const fn type_name(&self) -> &'static str {
        match self.region_type {
            MMAP_TYPE_AVAILABLE => "Available RAM",
            MMAP_TYPE_RESERVED => "Reserved",
            MMAP_TYPE_ACPI_RECLAIMABLE => "ACPI Reclaimable",
            MMAP_TYPE_ACPI_NVS => "ACPI NVS",
            MMAP_TYPE_BADRAM => "Bad RAM",
            _ => "OEM / Reserved",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FramebufferInfo {
    pub addr: u64,
    pub pitch: u32,
    pub width: u32,
    pub height: u32,
    pub bpp: u8,
    pub fb_type: u8,
    pub red_pos: u8,
    pub red_mask: u8,
    pub green_pos: u8,
    pub green_mask: u8,
    pub blue_pos: u8,
    pub blue_mask: u8,
}

#[derive(Debug, Clone, Copy)]
pub struct MultibootInfoData {
    pub magic: u32,
    pub raw_addr: u64,
    pub info_total_bytes: u64,
    pub mem_lower_kb: u32,
    pub mem_upper_kb: u32,
    pub mmap_addr_phys: u64,
    pub mmap_length_bytes: u64,
    pub mmap_entries: [MemoryMapEntry; MAX_MMAP_ENTRIES],
    pub mmap_count: usize,
    pub acpi_rsdp_phys: Option<u64>,
    pub smbios_tables: Option<(u64, usize, u8, u8)>,
    pub framebuffer: Option<FramebufferInfo>,
    pub bootloader_name: Option<&'static str>,
    pub bootloader_name_buf: [u8; 64],
    pub bootloader_name_len: usize,
}

impl MultibootInfoData {
    pub const fn empty() -> Self {
        Self {
            magic: 0,
            raw_addr: 0,
            info_total_bytes: 0,
            mem_lower_kb: 0,
            mem_upper_kb: 0,
            mmap_addr_phys: 0,
            mmap_length_bytes: 0,
            mmap_entries: [MemoryMapEntry::empty(); MAX_MMAP_ENTRIES],
            mmap_count: 0,
            acpi_rsdp_phys: None,
            smbios_tables: None,
            framebuffer: None,
            bootloader_name: None,
            bootloader_name_buf: [0; 64],
            bootloader_name_len: 0,
        }
    }

    pub fn get_bootloader_name(&self) -> Option<&str> {
        if self.bootloader_name_len > 0 {
            core::str::from_utf8(&self.bootloader_name_buf[..self.bootloader_name_len]).ok()
        } else {
            self.bootloader_name
        }
    }

    pub fn memory_map_tags(&self) -> &[MemoryMapEntry] {
        &self.mmap_entries[..self.mmap_count]
    }

    pub fn push_mmap_entry(&mut self, entry: MemoryMapEntry) {
        if entry.length > 0 && self.mmap_count < MAX_MMAP_ENTRIES {
            self.mmap_entries[self.mmap_count] = entry;
            self.mmap_count += 1;
        }
    }

    pub fn total_available_bytes(&self) -> u64 {
        let mut total = 0u64;
        for entry in self.memory_map_tags() {
            if entry.is_available() {
                total = total.saturating_add(entry.length);
            }
        }
        total
    }
}

pub static MULTIBOOT: IrqSafeSpinlock<MultibootInfoData> =
    IrqSafeSpinlock::new(MultibootInfoData::empty());

/// Early boot direct map window (first 64 GiB mapped at PHYSICAL_MEMORY_OFFSET).
pub const EARLY_DIRECT_MAP_LIMIT: u64 = 64 * 1024 * 1024 * 1024;

/// Parse Multiboot 1 or Multiboot 2 info structure passed by bootloader (GRUB, Ventoy, QEMU).
pub fn parse_boot_info(magic: u64, mb_info_phys: u64) {
    let mut data = MultibootInfoData::empty();
    data.magic = magic as u32;
    data.raw_addr = mb_info_phys;

    if mb_info_phys == 0 || mb_info_phys >= EARLY_DIRECT_MAP_LIMIT {
        *MULTIBOOT.lock() = data;
        return;
    }

    if magic as u32 == MULTIBOOT1_MAGIC {
        parse_multiboot1(mb_info_phys, &mut data);
    } else if magic as u32 == MULTIBOOT2_MAGIC {
        parse_multiboot2(mb_info_phys, &mut data);
    }

    if data.mmap_count > 0 {
        kinfo!(
            "Multiboot E820 Memory Map: {} region(s), {} MiB usable RAM reported",
            data.mmap_count,
            data.total_available_bytes() / (1024 * 1024)
        );
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
    out.info_total_bytes = 4096;
    let virt_addr = phys_to_virt(mb_info_phys);
    let ptr = virt_addr as *const u32;

    unsafe {
        let flags = core::ptr::read_unaligned(ptr);

        // Flags bit 0: mem_lower and mem_upper are valid
        if (flags & 0x01) != 0 {
            out.mem_lower_kb = core::ptr::read_unaligned(ptr.add(1));
            out.mem_upper_kb = core::ptr::read_unaligned(ptr.add(2));
        }

        // Flags bit 6 (0x40): E820 Memory Map (mmap_length @ +44, mmap_addr @ +48)
        if (flags & (1 << 6)) != 0 {
            let mmap_length = core::ptr::read_unaligned(ptr.add(11)) as usize;
            let mmap_addr = core::ptr::read_unaligned(ptr.add(12)) as u64;
            if mmap_addr != 0
                && mmap_length >= 24
                && mmap_addr.saturating_add(mmap_length as u64) < EARLY_DIRECT_MAP_LIMIT
            {
                out.mmap_addr_phys = mmap_addr;
                out.mmap_length_bytes = mmap_length as u64;
                let mmap_virt = phys_to_virt(mmap_addr) as *const u8;
                let mut offset = 0usize;

                while offset + 24 <= mmap_length {
                    let entry_ptr = mmap_virt.add(offset);
                    let size = core::ptr::read_unaligned(entry_ptr as *const u32) as usize;
                    if size < 20 || offset + 4 + size > mmap_length {
                        break;
                    }
                    let base_addr = core::ptr::read_unaligned(entry_ptr.add(4) as *const u64);
                    let length = core::ptr::read_unaligned(entry_ptr.add(12) as *const u64);
                    let region_type = core::ptr::read_unaligned(entry_ptr.add(20) as *const u32);

                    out.push_mmap_entry(MemoryMapEntry {
                        base_addr,
                        length,
                        region_type,
                    });

                    offset += 4 + size;
                }
            }
        }

        // Flags bit 12 (0x1000): Linear framebuffer table is valid
        if (flags & (1 << 12)) != 0 {
            let fb_ptr = (virt_addr + 88) as *const u8;
            let fb_addr = core::ptr::read_unaligned(fb_ptr as *const u64);
            let pitch = core::ptr::read_unaligned(fb_ptr.add(8) as *const u32);
            let width = core::ptr::read_unaligned(fb_ptr.add(12) as *const u32);
            let height = core::ptr::read_unaligned(fb_ptr.add(16) as *const u32);
            let bpp = *fb_ptr.add(20);
            let fb_type = *fb_ptr.add(21);

            if fb_addr != 0 && fb_type != 2 && width > 0 && height > 0 && bpp >= 24 {
                out.framebuffer = Some(FramebufferInfo {
                    addr: fb_addr,
                    pitch,
                    width,
                    height,
                    bpp,
                    fb_type,
                    red_pos: 16,
                    red_mask: 8,
                    green_pos: 8,
                    green_mask: 8,
                    blue_pos: 0,
                    blue_mask: 8,
                });
            }
        }
        // Flags bit 11 (0x800): VBE Mode info is valid (Fallback)
        else if (flags & (1 << 11)) != 0 {
            let vbe_mode_info_phys = core::ptr::read_unaligned(ptr.add(19)) as u64;
            if vbe_mode_info_phys != 0 && vbe_mode_info_phys + 64 < EARLY_DIRECT_MAP_LIMIT {
                let mode_info_virt = phys_to_virt(vbe_mode_info_phys) as *const u8;
                let pitch = core::ptr::read_unaligned(mode_info_virt.add(16) as *const u16) as u32;
                let width = core::ptr::read_unaligned(mode_info_virt.add(18) as *const u16) as u32;
                let height = core::ptr::read_unaligned(mode_info_virt.add(20) as *const u16) as u32;
                let bpp = *mode_info_virt.add(25);
                let phys_base = core::ptr::read_unaligned(mode_info_virt.add(40) as *const u32) as u64;
                let r_mask = *mode_info_virt.add(31);
                let r_pos = *mode_info_virt.add(32);
                let g_mask = *mode_info_virt.add(33);
                let g_pos = *mode_info_virt.add(34);
                let b_mask = *mode_info_virt.add(35);
                let b_pos = *mode_info_virt.add(36);
                let (red_pos, red_mask, green_pos, green_mask, blue_pos, blue_mask) =
                    if r_mask > 0 && g_mask > 0 && b_mask > 0 {
                        (r_pos, r_mask, g_pos, g_mask, b_pos, b_mask)
                    } else {
                        (16, 8, 8, 8, 0, 8)
                    };

                if phys_base != 0 && width > 0 && height > 0 && bpp >= 24 {
                    out.framebuffer = Some(FramebufferInfo {
                        addr: phys_base,
                        pitch,
                        width,
                        height,
                        bpp,
                        fb_type: 1,
                        red_pos,
                        red_mask,
                        green_pos,
                        green_mask,
                        blue_pos,
                        blue_mask,
                    });
                }
            }
        }
    }
}

fn parse_multiboot2(mb_info_phys: u64, out: &mut MultibootInfoData) {
    let virt_addr = phys_to_virt(mb_info_phys);
    let total_size = unsafe { core::ptr::read_unaligned(virt_addr as *const u32) } as usize;
    if total_size < 16 || mb_info_phys.saturating_add(total_size as u64) >= EARLY_DIRECT_MAP_LIMIT {
        return;
    }
    out.info_total_bytes = total_size as u64;

    let mut offset = 8; // Skip total_size and reserved
    while offset + 8 <= total_size {
        let tag_ptr = unsafe { (virt_addr as *const u8).add(offset) };
        let tag_type = unsafe { core::ptr::read_unaligned(tag_ptr as *const u32) };
        let tag_size = unsafe { core::ptr::read_unaligned(tag_ptr.add(4) as *const u32) } as usize;

        if tag_type == 0 {
            break; // End tag
        }
        if tag_size < 8 || offset + tag_size > total_size {
            break;
        }

        match tag_type {
            // Tag 2: Bootloader Name
            2 if tag_size > 8 => {
                let name_len = (tag_size - 8).min(63);
                let mut actual_len = 0;
                while actual_len < name_len && unsafe { *tag_ptr.add(8 + actual_len) } != 0 {
                    out.bootloader_name_buf[actual_len] = unsafe { *tag_ptr.add(8 + actual_len) };
                    actual_len += 1;
                }
                out.bootloader_name_len = actual_len;
            }
            // Tag 4: Basic Memory Information
            4 if tag_size >= 16 => {
                out.mem_lower_kb = unsafe { core::ptr::read_unaligned(tag_ptr.add(8) as *const u32) };
                out.mem_upper_kb = unsafe { core::ptr::read_unaligned(tag_ptr.add(12) as *const u32) };
            }
            // Tag 6: Memory Map
            6 if tag_size >= 16 => {
                let entry_size = unsafe { core::ptr::read_unaligned(tag_ptr.add(8) as *const u32) } as usize;
                if entry_size >= 24 {
                    let mut e_off = 16usize;
                    while e_off + 24 <= tag_size {
                        let e_ptr = unsafe { tag_ptr.add(e_off) };
                        let base_addr = unsafe { core::ptr::read_unaligned(e_ptr as *const u64) };
                        let length = unsafe { core::ptr::read_unaligned(e_ptr.add(8) as *const u64) };
                        let region_type = unsafe { core::ptr::read_unaligned(e_ptr.add(16) as *const u32) };
                        out.push_mmap_entry(MemoryMapEntry {
                            base_addr,
                            length,
                            region_type,
                        });
                        e_off += entry_size;
                    }
                }
            }
            // Tag 8: Framebuffer Info
            8 if tag_size >= 30 => {
                let fb_addr = unsafe { core::ptr::read_unaligned(tag_ptr.add(8) as *const u64) };
                let pitch = unsafe { core::ptr::read_unaligned(tag_ptr.add(16) as *const u32) };
                let width = unsafe { core::ptr::read_unaligned(tag_ptr.add(20) as *const u32) };
                let height = unsafe { core::ptr::read_unaligned(tag_ptr.add(24) as *const u32) };
                let bpp = unsafe { *tag_ptr.add(28) };
                let fb_type = unsafe { *tag_ptr.add(29) };
                let (red_pos, red_mask, green_pos, green_mask, blue_pos, blue_mask) =
                    if fb_type == 1 && tag_size >= 38 {
                        let r_pos = unsafe { *tag_ptr.add(32) };
                        let r_mask = unsafe { *tag_ptr.add(33) };
                        let g_pos = unsafe { *tag_ptr.add(34) };
                        let g_mask = unsafe { *tag_ptr.add(35) };
                        let b_pos = unsafe { *tag_ptr.add(36) };
                        let b_mask = unsafe { *tag_ptr.add(37) };
                        if r_mask > 0 && g_mask > 0 && b_mask > 0 {
                            (r_pos, r_mask, g_pos, g_mask, b_pos, b_mask)
                        } else {
                            (16, 8, 8, 8, 0, 8)
                        }
                    } else {
                        (16, 8, 8, 8, 0, 8)
                    };

                if fb_addr != 0 && fb_type != 2 && width > 0 && height > 0 && bpp >= 24 {
                    out.framebuffer = Some(FramebufferInfo {
                        addr: fb_addr,
                        pitch,
                        width,
                        height,
                        bpp,
                        fb_type,
                        red_pos,
                        red_mask,
                        green_pos,
                        green_mask,
                        blue_pos,
                        blue_mask,
                    });
                }
            }
            // Tag 13: SMBIOS Tables
            13 if tag_size >= 16 => {
                let major = unsafe { *tag_ptr.add(8) };
                let minor = unsafe { *tag_ptr.add(9) };
                let table_len = tag_size - 16;
                let table_phys = mb_info_phys + (offset + 16) as u64;
                out.smbios_tables = Some((table_phys, table_len, major, minor));
            }
            // Tag 14: ACPI 1.0 Old RSDP, Tag 15: ACPI 2.0+ New RSDP (UEFI)
            14 | 15 if tag_size >= 28 => {
                let rsdp_phys = mb_info_phys + (offset + 8) as u64;
                // Prefer ACPI 2.0+ (tag 15) over ACPI 1.0 (tag 14)
                if tag_type == 15 || out.acpi_rsdp_phys.is_none() {
                    out.acpi_rsdp_phys = Some(rsdp_phys);
                }
            }
            _ => {}
        }

        // Align tag size up to 8 bytes
        offset += (tag_size + 7) & !7;
    }
}
