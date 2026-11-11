extern crate alloc;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::drivers::storage::BlockDevice;
use crate::fs::vfs::{DirEntry, FileType, InodeOperations, Stat};
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

pub struct Fat32FileSystem {
    pub device: Arc<dyn BlockDevice>,
    pub partition_lba: u64,
    pub bytes_per_sector: u32,
    pub sectors_per_cluster: u32,
    pub fat_start_lba: u64,
    pub data_start_lba: u64,
    pub root_cluster: u32,
}

impl Fat32FileSystem {
    pub fn mount(device: Arc<dyn BlockDevice>) -> Result<Arc<Self>, &'static str> {
        let mut sector_buf = [0u8; 512];

        // 1. Read LBA 0 (check for MBR or direct VBR)
        device.read_sectors(0, 1, &mut sector_buf)?;

        if sector_buf[510] != 0x55 || sector_buf[511] != 0xAA {
            return Err("Invalid boot signature (not FAT32/MBR)");
        }

        let mut part_lba = 0u64;

        // Check if sector 0 is MBR partition table (offset 446)
        let part1_type = sector_buf[446 + 4];
        let part1_lba = (sector_buf[446 + 8] as u32)
            | ((sector_buf[446 + 9] as u32) << 8)
            | ((sector_buf[446 + 10] as u32) << 16)
            | ((sector_buf[446 + 11] as u32) << 24);

        if (part1_type == 0x0B || part1_type == 0x0C || part1_type == 0x07 || part1_type == 0x83)
            && part1_lba > 0
        {
            part_lba = part1_lba as u64;
            device.read_sectors(part_lba, 1, &mut sector_buf)?;
        }

        // 2. Parse BPB (BIOS Parameter Block)
        let bytes_per_sector = (sector_buf[11] as u32) | ((sector_buf[12] as u32) << 8);
        let sectors_per_cluster = sector_buf[13] as u32;
        let reserved_sectors = (sector_buf[14] as u32) | ((sector_buf[15] as u32) << 8);
        let num_fats = sector_buf[16] as u32;
        let sectors_per_fat = (sector_buf[36] as u32)
            | ((sector_buf[37] as u32) << 8)
            | ((sector_buf[38] as u32) << 16)
            | ((sector_buf[39] as u32) << 24);
        let root_cluster = (sector_buf[44] as u32)
            | ((sector_buf[45] as u32) << 8)
            | ((sector_buf[46] as u32) << 16)
            | ((sector_buf[47] as u32) << 24);

        if bytes_per_sector != 512 || sectors_per_cluster == 0 || num_fats == 0 || sectors_per_fat == 0 {
            return Err("Invalid FAT32 BPB parameters");
        }

        let fat_start_lba = part_lba + reserved_sectors as u64;
        let data_start_lba = fat_start_lba + (num_fats as u64 * sectors_per_fat as u64);

        kinfo!(
            "FAT32 Filesystem Mounted from {}: Sectors/Cluster: {}, Root Cluster: {}, Data LBA: 0x{:X}",
            device.name(),
            sectors_per_cluster,
            root_cluster,
            data_start_lba
        );

        Ok(Arc::new(Self {
            device,
            partition_lba: part_lba,
            bytes_per_sector,
            sectors_per_cluster,
            fat_start_lba,
            data_start_lba,
            root_cluster,
        }))
    }

    pub fn cluster_to_lba(&self, cluster: u32) -> u64 {
        self.data_start_lba + ((cluster.saturating_sub(2)) as u64 * self.sectors_per_cluster as u64)
    }

    pub fn next_cluster(&self, cluster: u32) -> Result<Option<u32>, &'static str> {
        let fat_offset = (cluster as u64) * 4;
        let fat_sector = self.fat_start_lba + (fat_offset / 512);
        let entry_offset = (fat_offset % 512) as usize;

        let mut buf = [0u8; 512];
        self.device.read_sectors(fat_sector, 1, &mut buf)?;

        let raw = (buf[entry_offset] as u32)
            | ((buf[entry_offset + 1] as u32) << 8)
            | ((buf[entry_offset + 2] as u32) << 16)
            | ((buf[entry_offset + 3] as u32) << 24);

        let next = raw & 0x0FFF_FFFF;
        if next >= 0x0FFF_FFF8 {
            Ok(None) // End of Cluster Chain (EOC)
        } else if next == 0 || next == 0x0FFF_FFF7 {
            Err("FAT32 cluster corrupted or bad cluster encountered")
        } else {
            Ok(Some(next))
        }
    }
}

pub struct Fat32Inode {
    fs: Arc<Fat32FileSystem>,
    start_cluster: u32,
    size: usize,
    file_type: FileType,
    pub name: String,
}

impl Fat32Inode {
    pub fn new_root(fs: Arc<Fat32FileSystem>) -> Arc<IrqSafeSpinlock<dyn InodeOperations>> {
        let root_cluster = fs.root_cluster;
        Arc::new(IrqSafeSpinlock::new(Self {
            fs,
            start_cluster: root_cluster,
            size: 0,
            file_type: FileType::Directory,
            name: String::from("/"),
        }))
    }
}

impl InodeOperations for Fat32Inode {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        if self.file_type == FileType::Directory {
            return Err("Cannot read raw bytes directly from directory Inode");
        }

        if offset >= self.size {
            return Ok(0);
        }

        let bytes_to_read = buf.len().min(self.size - offset);
        let cluster_bytes = (self.fs.sectors_per_cluster * 512) as usize;

        let mut curr_cluster = self.start_cluster;
        let mut cluster_skip = offset / cluster_bytes;
        let mut cluster_offset = offset % cluster_bytes;

        // Skip to the cluster containing offset
        while cluster_skip > 0 {
            match self.fs.next_cluster(curr_cluster)? {
                Some(next) => curr_cluster = next,
                None => return Ok(0),
            }
            cluster_skip -= 1;
        }

        let mut bytes_read = 0;
        let mut cluster_buf = [0u8; 4096]; // Buffer up to 8 sectors per cluster

        while bytes_read < bytes_to_read {
            let chunk = (bytes_to_read - bytes_read).min(cluster_bytes - cluster_offset);
            let lba = self.fs.cluster_to_lba(curr_cluster);

            let sectors = self.fs.sectors_per_cluster.min(8);
            self.fs.device.read_sectors(lba, sectors, &mut cluster_buf[..((sectors * 512) as usize)])?;

            buf[bytes_read..bytes_read + chunk].copy_from_slice(&cluster_buf[cluster_offset..cluster_offset + chunk]);
            bytes_read += chunk;
            cluster_offset = 0;

            if bytes_read < bytes_to_read {
                match self.fs.next_cluster(curr_cluster)? {
                    Some(next) => curr_cluster = next,
                    None => break,
                }
            }
        }

        Ok(bytes_read)
    }

    fn lookup(&self, name: &str) -> Result<Arc<IrqSafeSpinlock<dyn InodeOperations>>, &'static str> {
        if self.file_type != FileType::Directory {
            return Err("Not a directory");
        }

        let entries = self.readdir_internal()?;
        for (item_name, item_cluster, item_size, is_dir) in entries {
            if item_name.eq_ignore_ascii_case(name) {
                return Ok(Arc::new(IrqSafeSpinlock::new(Fat32Inode {
                    fs: self.fs.clone(),
                    start_cluster: item_cluster,
                    size: item_size,
                    file_type: if is_dir { FileType::Directory } else { FileType::Regular },
                    name: item_name,
                })));
            }
        }

        Err("File not found in FAT32 directory")
    }

    fn readdir(&self) -> Result<Vec<DirEntry>, &'static str> {
        if self.file_type != FileType::Directory {
            return Err("Not a directory");
        }

        let mut result = Vec::new();
        let entries = self.readdir_internal()?;

        for (idx, (item_name, _cluster, _size, is_dir)) in entries.into_iter().enumerate() {
            result.push(DirEntry {
                name: item_name,
                inode: (idx + 1000) as u64,
                file_type: if is_dir { FileType::Directory } else { FileType::Regular },
            });
        }

        Ok(result)
    }

    fn stat(&self) -> Stat {
        Stat {
            inode: self.start_cluster as u64,
            size: self.size,
            file_type: self.file_type,
            mode: if self.file_type == FileType::Directory { 0o755 } else { 0o644 },
        }
    }
}

impl Fat32Inode {
    fn readdir_internal(&self) -> Result<Vec<(String, u32, usize, bool)>, &'static str> {
        let mut list = Vec::new();
        let mut curr_cluster = self.start_cluster;
        let mut cluster_buf = [0u8; 4096];
        let cluster_bytes = (self.fs.sectors_per_cluster * 512) as usize;

        let mut lfn_chars: Vec<char> = Vec::new();

        loop {
            let lba = self.fs.cluster_to_lba(curr_cluster);
            let sectors = self.fs.sectors_per_cluster.min(8);
            self.fs.device.read_sectors(lba, sectors, &mut cluster_buf[..((sectors * 512) as usize)])?;

            for entry_idx in 0..(cluster_bytes / 32) {
                let offset = entry_idx * 32;
                let first_byte = cluster_buf[offset];

                if first_byte == 0x00 {
                    // No more entries in directory
                    return Ok(list);
                }
                if first_byte == 0xE5 {
                    // Deleted entry, skip
                    lfn_chars.clear();
                    continue;
                }

                let attr = cluster_buf[offset + 11];

                // Check for LFN entry
                if attr == 0x0F {
                    // Extract characters from LFN entry
                    extract_lfn_chars(&cluster_buf[offset..offset + 32], &mut lfn_chars);
                    continue;
                }

                // Skip Volume Label (0x08)
                if (attr & 0x08) != 0 {
                    lfn_chars.clear();
                    continue;
                }

                let is_dir = (attr & 0x10) != 0;

                // Formulate entry name
                let name = if !lfn_chars.is_empty() {
                    let s: String = lfn_chars.iter().rev().collect();
                    lfn_chars.clear();
                    s
                } else {
                    format_8_3_name(&cluster_buf[offset..offset + 11])
                };

                let high_cluster = (cluster_buf[offset + 20] as u32) | ((cluster_buf[offset + 21] as u32) << 8);
                let low_cluster = (cluster_buf[offset + 26] as u32) | ((cluster_buf[offset + 27] as u32) << 8);
                let start_cluster = (high_cluster << 16) | low_cluster;

                let size = (cluster_buf[offset + 28] as usize)
                    | ((cluster_buf[offset + 29] as usize) << 8)
                    | ((cluster_buf[offset + 30] as usize) << 16)
                    | ((cluster_buf[offset + 31] as usize) << 24);

                if name != "." && name != ".." {
                    list.push((name, start_cluster, size, is_dir));
                }
            }

            match self.fs.next_cluster(curr_cluster)? {
                Some(next) => curr_cluster = next,
                None => break,
            }
        }

        Ok(list)
    }
}

fn extract_lfn_chars(entry: &[u8], out: &mut Vec<char>) {
    let indices: [usize; 13] = [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];
    for &idx in indices.iter().rev() {
        let ch = (entry[idx] as u16) | ((entry[idx + 1] as u16) << 8);
        if ch != 0 && ch != 0xFFFF {
            out.push(ch as u8 as char);
        }
    }
}

fn format_8_3_name(bytes: &[u8]) -> String {
    let mut s = String::new();
    let name_part = core::str::from_utf8(&bytes[0..8]).unwrap_or("").trim_end();
    let ext_part = core::str::from_utf8(&bytes[8..11]).unwrap_or("").trim_end();

    s.push_str(name_part);
    if !ext_part.is_empty() {
        s.push('.');
        s.push_str(ext_part);
    }
    s
}
