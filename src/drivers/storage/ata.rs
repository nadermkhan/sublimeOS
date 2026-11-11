extern crate alloc;
use alloc::string::String;
use alloc::sync::Arc;
use crate::arch::x86_64::io::{inb, inw, outb, outw};
use crate::drivers::storage::{BlockDevice, STORAGE_MANAGER};
use crate::kinfo;

const STATUS_BSY: u8 = 0x80;
const _STATUS_DRDY: u8 = 0x40;
const STATUS_DRQ: u8 = 0x08;
const STATUS_ERR: u8 = 0x01;
const STATUS_DF: u8 = 0x20;

const CMD_READ_SECTORS: u8 = 0x20;
const CMD_WRITE_SECTORS: u8 = 0x30;
const CMD_IDENTIFY: u8 = 0xEC;
const CMD_CACHE_FLUSH: u8 = 0xE7;

pub struct AtaDrive {
    pub name: String,
    pub io_base: u16,
    pub ctrl_base: u16,
    pub is_slave: bool,
    pub total_sectors: u64,
    pub model: String,
}

impl AtaDrive {
    fn select_drive(&self, lba: u64) {
        let drive_sel = if self.is_slave { 0xB0 } else { 0xA0 };
        let head = ((lba >> 24) & 0x0F) as u8;
        unsafe {
            outb(self.io_base + 6, drive_sel | head | 0x40); // 0x40 = LBA mode
        }
        self.io_wait();
    }

    fn io_wait(&self) {
        for _ in 0..4 {
            unsafe {
                inb(self.ctrl_base);
            }
        }
    }

    fn wait_not_busy(&self) -> Result<u8, &'static str> {
        for _ in 0..100_000 {
            let status = unsafe { inb(self.io_base + 7) };
            if (status & STATUS_BSY) == 0 {
                return Ok(status);
            }
        }
        Err("ATA timeout: drive remained busy")
    }

    fn wait_ready(&self) -> Result<(), &'static str> {
        for _ in 0..100_000 {
            let status = unsafe { inb(self.io_base + 7) };
            if (status & STATUS_ERR) != 0 || (status & STATUS_DF) != 0 {
                return Err("ATA drive reported hardware error");
            }
            if (status & STATUS_BSY) == 0 && (status & STATUS_DRQ) != 0 {
                return Ok(());
            }
        }
        Err("ATA timeout: DRQ not asserted")
    }
}

impl BlockDevice for AtaDrive {
    fn name(&self) -> &str {
        &self.name
    }

    fn total_sectors(&self) -> u64 {
        self.total_sectors
    }

    fn read_sectors(&self, lba: u64, count: u32, buf: &mut [u8]) -> Result<(), &'static str> {
        if (count as usize) * 512 > buf.len() {
            return Err("Buffer too small for requested sector read");
        }

        for i in 0..count {
            let current_lba = lba + i as u64;
            self.wait_not_busy()?;
            self.select_drive(current_lba);

            unsafe {
                outb(self.io_base + 2, 1); // Sector count = 1
                outb(self.io_base + 3, current_lba as u8);
                outb(self.io_base + 4, (current_lba >> 8) as u8);
                outb(self.io_base + 5, (current_lba >> 16) as u8);
                outb(self.io_base + 7, CMD_READ_SECTORS);
            }

            self.wait_ready()?;

            let offset = (i as usize) * 512;
            let target = &mut buf[offset..offset + 512];

            for word_idx in 0..256 {
                let word = unsafe { inw(self.io_base) };
                target[word_idx * 2] = word as u8;
                target[word_idx * 2 + 1] = (word >> 8) as u8;
            }
        }

        Ok(())
    }

    fn write_sectors(&self, lba: u64, count: u32, buf: &[u8]) -> Result<(), &'static str> {
        if (count as usize) * 512 > buf.len() {
            return Err("Buffer too small for requested sector write");
        }

        for i in 0..count {
            let current_lba = lba + i as u64;
            self.wait_not_busy()?;
            self.select_drive(current_lba);

            unsafe {
                outb(self.io_base + 2, 1);
                outb(self.io_base + 3, current_lba as u8);
                outb(self.io_base + 4, (current_lba >> 8) as u8);
                outb(self.io_base + 5, (current_lba >> 16) as u8);
                outb(self.io_base + 7, CMD_WRITE_SECTORS);
            }

            self.wait_ready()?;

            let offset = (i as usize) * 512;
            let src = &buf[offset..offset + 512];

            for word_idx in 0..256 {
                let word = (src[word_idx * 2] as u16) | ((src[word_idx * 2 + 1] as u16) << 8);
                unsafe {
                    outw(self.io_base, word);
                }
            }

            // Flush cache
            unsafe {
                outb(self.io_base + 7, CMD_CACHE_FLUSH);
            }
            let _ = self.wait_not_busy();
        }

        Ok(())
    }
}

pub fn init() {
    let buses = [
        (0x1F0u16, 0x3F6u16, "Primary"),
        (0x170u16, 0x376u16, "Secondary"),
    ];

    let mut disk_id = 0;

    for (io_base, ctrl_base, _bus_name) in buses.iter() {
        for &is_slave in &[false, true] {
            if let Some(drive) = probe_ata_drive(*io_base, *ctrl_base, is_slave, disk_id) {
                kinfo!(
                    "ATA Drive Detected: {} [Model: {}] ({} MB, {} sectors)",
                    drive.name,
                    drive.model,
                    (drive.total_sectors * 512) / (1024 * 1024),
                    drive.total_sectors
                );
                STORAGE_MANAGER.lock().register(Arc::new(drive));
                disk_id += 1;
            }
        }
    }
}

fn probe_ata_drive(io_base: u16, ctrl_base: u16, is_slave: bool, id: usize) -> Option<AtaDrive> {
    unsafe {
        let drive_sel = if is_slave { 0xB0 } else { 0xA0 };
        outb(io_base + 6, drive_sel);
        for _ in 0..4 {
            inb(ctrl_base);
        }

        outb(io_base + 2, 0);
        outb(io_base + 3, 0);
        outb(io_base + 4, 0);
        outb(io_base + 5, 0);

        outb(io_base + 7, CMD_IDENTIFY);
        for _ in 0..4 {
            inb(ctrl_base);
        }

        let status = inb(io_base + 7);
        if status == 0 || status == 0xFF {
            return None; // No drive connected
        }

        // Wait until BSY clears
        for _ in 0..10_000 {
            let s = inb(io_base + 7);
            if (s & STATUS_BSY) == 0 {
                break;
            }
        }

        let mid = inb(io_base + 4);
        let high = inb(io_base + 5);
        if (mid == 0x14 && high == 0xEB) || (mid == 0x69 && high == 0x96) {
            // ATAPI optical drive - skip for raw sector block storage
            return None;
        }

        let mut ready = false;
        for _ in 0..10_000 {
            let s = inb(io_base + 7);
            if (s & STATUS_DRQ) != 0 {
                ready = true;
                break;
            }
            if (s & STATUS_ERR) != 0 {
                return None;
            }
        }

        if !ready {
            return None;
        }

        // Read 256 words of IDENTIFY data
        let mut id_data = [0u16; 256];
        for i in 0..256 {
            id_data[i] = inw(io_base);
        }

        // Extract total sectors (Word 60-61 for 28-bit, or Word 100-103 for 48-bit)
        let lba28_sectors = (id_data[60] as u64) | ((id_data[61] as u64) << 16);
        let lba48_sectors = (id_data[100] as u64)
            | ((id_data[101] as u64) << 16)
            | ((id_data[102] as u64) << 32)
            | ((id_data[103] as u64) << 48);

        let total_sectors = if lba48_sectors > 0 {
            lba48_sectors
        } else {
            lba28_sectors
        };

        if total_sectors == 0 {
            return None;
        }

        // Extract model string (words 27-46, 40 bytes)
        let mut model_bytes = [0u8; 40];
        for i in 0..20 {
            let word = id_data[27 + i];
            model_bytes[i * 2] = (word >> 8) as u8;
            model_bytes[i * 2 + 1] = word as u8;
        }
        let model_str = core::str::from_utf8(&model_bytes)
            .unwrap_or("Generic ATA Drive")
            .trim();
        let model = String::from(model_str);

        let mut name = String::from("disk");
        name.push((b'0' + (id as u8)) as char);

        Some(AtaDrive {
            name,
            io_base,
            ctrl_base,
            is_slave,
            total_sectors,
            model,
        })
    }
}
