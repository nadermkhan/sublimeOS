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
    pub total_clusters: u32,
    pub volume_label: String,
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

        let total_sectors_32 = (sector_buf[32] as u32)
            | ((sector_buf[33] as u32) << 8)
            | ((sector_buf[34] as u32) << 16)
            | ((sector_buf[35] as u32) << 24);

        if bytes_per_sector != 512 || sectors_per_cluster == 0 || num_fats == 0 || sectors_per_fat == 0 {
            return Err("Invalid FAT32 BPB parameters");
        }

        let fat_start_lba = part_lba + reserved_sectors as u64;
        let data_start_lba = fat_start_lba + (num_fats as u64 * sectors_per_fat as u64);
        let total_clusters = if total_sectors_32 > data_start_lba as u32 {
            (total_sectors_32 - data_start_lba as u32) / sectors_per_cluster
        } else {
            1024
        };

        // Extract volume label from BPB offset 71 (11 bytes)
        let mut label_str = String::from("NO NAME");
        if let Ok(label) = core::str::from_utf8(&sector_buf[71..82]) {
            let trimmed = label.trim();
            if !trimmed.is_empty() && trimmed != "NO NAME" {
                label_str = String::from(trimmed);
            }
        }

        kinfo!(
            "FAT32 Filesystem Mounted from {}: Label '{}', Cluster: {} sectors, Root: Cluster {}, Data LBA: 0x{:X}",
            device.name(),
            label_str,
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
            total_clusters,
            volume_label: label_str,
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

    /// Allocate a new cluster in the FAT table, marking it EOC (0x0FFFFFFF).
    pub fn alloc_cluster(&self) -> Result<u32, &'static str> {
        let mut buf = [0u8; 512];
        // Scan clusters starting at 2
        for c in 2..self.total_clusters.max(1000) {
            let fat_offset = (c as u64) * 4;
            let fat_sector = self.fat_start_lba + (fat_offset / 512);
            let entry_offset = (fat_offset % 512) as usize;

            self.device.read_sectors(fat_sector, 1, &mut buf)?;
            let raw = (buf[entry_offset] as u32)
                | ((buf[entry_offset + 1] as u32) << 8)
                | ((buf[entry_offset + 2] as u32) << 16)
                | ((buf[entry_offset + 3] as u32) << 24);

            if (raw & 0x0FFF_FFFF) == 0 {
                // Free cluster found! Mark as End of Chain (0x0FFFFFFF)
                let val: u32 = 0x0FFF_FFFF;
                buf[entry_offset] = (val & 0xFF) as u8;
                buf[entry_offset + 1] = ((val >> 8) & 0xFF) as u8;
                buf[entry_offset + 2] = ((val >> 16) & 0xFF) as u8;
                buf[entry_offset + 3] = ((val >> 24) & 0x0F) as u8;
                self.device.write_sectors(fat_sector, 1, &buf)?;

                // Zero out the newly allocated cluster on disk
                let lba = self.cluster_to_lba(c);
                let zeros = [0u8; 512];
                for s in 0..self.sectors_per_cluster {
                    self.device.write_sectors(lba + s as u64, 1, &zeros)?;
                }

                return Ok(c);
            }
        }

        Err("FAT32 volume full (no free clusters)")
    }

    /// Link a cluster to point to the next cluster in the FAT.
    pub fn link_cluster(&self, current: u32, next: u32) -> Result<(), &'static str> {
        let fat_offset = (current as u64) * 4;
        let fat_sector = self.fat_start_lba + (fat_offset / 512);
        let entry_offset = (fat_offset % 512) as usize;

        let mut buf = [0u8; 512];
        self.device.read_sectors(fat_sector, 1, &mut buf)?;

        let val = next & 0x0FFF_FFFF;
        buf[entry_offset] = (val & 0xFF) as u8;
        buf[entry_offset + 1] = ((val >> 8) & 0xFF) as u8;
        buf[entry_offset + 2] = ((val >> 16) & 0xFF) as u8;
        buf[entry_offset + 3] = (buf[entry_offset + 3] & 0xF0) | (((val >> 24) & 0x0F) as u8);

        self.device.write_sectors(fat_sector, 1, &buf)
    }

    /// Free a cluster chain starting at given cluster.
    pub fn free_chain(&self, mut cluster: u32) -> Result<(), &'static str> {
        while cluster >= 2 && cluster < 0x0FFF_FFF8 {
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

            // Clear entry to 0
            buf[entry_offset] = 0;
            buf[entry_offset + 1] = 0;
            buf[entry_offset + 2] = 0;
            buf[entry_offset + 3] = buf[entry_offset + 3] & 0xF0;
            self.device.write_sectors(fat_sector, 1, &buf)?;

            cluster = next;
        }
        Ok(())
    }
}

pub struct Fat32Inode {
    fs: Arc<Fat32FileSystem>,
    start_cluster: u32,
    size: usize,
    file_type: FileType,
    pub name: String,
    parent_cluster: u32,
    dir_entry_offset: usize, // byte offset in parent directory cluster chain
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
            parent_cluster: root_cluster,
            dir_entry_offset: 0,
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

    fn write(&mut self, offset: usize, buf: &[u8]) -> Result<usize, &'static str> {
        if self.file_type == FileType::Directory {
            return Err("Cannot write raw bytes to directory");
        }

        let cluster_bytes = (self.fs.sectors_per_cluster * 512) as usize;
        if self.start_cluster == 0 {
            // Allocate initial cluster for empty file
            self.start_cluster = self.fs.alloc_cluster()?;
            self.update_parent_dir_entry()?;
        }

        let mut curr_cluster = self.start_cluster;
        let mut cluster_skip = offset / cluster_bytes;
        let mut cluster_offset = offset % cluster_bytes;

        while cluster_skip > 0 {
            match self.fs.next_cluster(curr_cluster)? {
                Some(next) => curr_cluster = next,
                None => {
                    // Need to allocate next cluster to reach offset
                    let new_cluster = self.fs.alloc_cluster()?;
                    self.fs.link_cluster(curr_cluster, new_cluster)?;
                    curr_cluster = new_cluster;
                }
            }
            cluster_skip -= 1;
        }

        let mut bytes_written = 0;
        let mut cluster_buf = [0u8; 4096];

        while bytes_written < buf.len() {
            let chunk = (buf.len() - bytes_written).min(cluster_bytes - cluster_offset);
            let lba = self.fs.cluster_to_lba(curr_cluster);
            let sectors = self.fs.sectors_per_cluster.min(8);

            // Read existing sector content if partial overwrite
            if cluster_offset > 0 || chunk < cluster_bytes {
                let _ = self.fs.device.read_sectors(lba, sectors, &mut cluster_buf[..((sectors * 512) as usize)]);
            }

            cluster_buf[cluster_offset..cluster_offset + chunk].copy_from_slice(&buf[bytes_written..bytes_written + chunk]);
            self.fs.device.write_sectors(lba, sectors, &cluster_buf[..((sectors * 512) as usize)])?;

            bytes_written += chunk;
            cluster_offset = 0;

            if bytes_written < buf.len() {
                match self.fs.next_cluster(curr_cluster)? {
                    Some(next) => curr_cluster = next,
                    None => {
                        let new_cluster = self.fs.alloc_cluster()?;
                        self.fs.link_cluster(curr_cluster, new_cluster)?;
                        curr_cluster = new_cluster;
                    }
                }
            }
        }

        if offset + bytes_written > self.size {
            self.size = offset + bytes_written;
            self.update_parent_dir_entry()?;
        }

        Ok(bytes_written)
    }

    fn lookup(&self, name: &str) -> Result<Arc<IrqSafeSpinlock<dyn InodeOperations>>, &'static str> {
        if self.file_type != FileType::Directory {
            return Err("Not a directory");
        }

        let entries = self.readdir_internal()?;
        for (item_name, item_cluster, item_size, is_dir, dir_offset) in entries {
            if item_name.eq_ignore_ascii_case(name) {
                return Ok(Arc::new(IrqSafeSpinlock::new(Fat32Inode {
                    fs: self.fs.clone(),
                    start_cluster: item_cluster,
                    size: item_size,
                    file_type: if is_dir { FileType::Directory } else { FileType::Regular },
                    name: item_name,
                    parent_cluster: self.start_cluster,
                    dir_entry_offset: dir_offset,
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

        for (idx, (item_name, _cluster, _size, is_dir, _)) in entries.into_iter().enumerate() {
            result.push(DirEntry {
                name: item_name,
                inode: (idx + 1000) as u64,
                file_type: if is_dir { FileType::Directory } else { FileType::Regular },
            });
        }

        Ok(result)
    }

    fn create(&mut self, name: &str, file_type: FileType) -> Result<Arc<IrqSafeSpinlock<dyn InodeOperations>>, &'static str> {
        if self.file_type != FileType::Directory {
            return Err("Not a directory");
        }

        let is_dir = file_type == FileType::Directory;
        let new_cluster = if is_dir { self.fs.alloc_cluster()? } else { 0 };

        // Allocate directory entry slot in this directory
        let dir_offset = self.allocate_dir_entry(name, new_cluster, 0, is_dir)?;

        if is_dir {
            // Initialize `.` and `..` entries inside newly created directory
            let mut dot_buf = [0u8; 512];
            // `.` entry
            dot_buf[0..11].copy_from_slice(b".          ");
            dot_buf[11] = 0x10;
            dot_buf[26] = (new_cluster & 0xFF) as u8;
            dot_buf[27] = ((new_cluster >> 8) & 0xFF) as u8;
            dot_buf[20] = ((new_cluster >> 16) & 0xFF) as u8;
            dot_buf[21] = ((new_cluster >> 24) & 0xFF) as u8;

            // `..` entry
            dot_buf[32..43].copy_from_slice(b"..         ");
            dot_buf[43] = 0x10;
            let parent_c = if self.start_cluster == self.fs.root_cluster { 0 } else { self.start_cluster };
            dot_buf[58] = (parent_c & 0xFF) as u8;
            dot_buf[59] = ((parent_c >> 8) & 0xFF) as u8;
            dot_buf[52] = ((parent_c >> 16) & 0xFF) as u8;
            dot_buf[53] = ((parent_c >> 24) & 0xFF) as u8;

            let lba = self.fs.cluster_to_lba(new_cluster);
            self.fs.device.write_sectors(lba, 1, &dot_buf)?;
        }

        Ok(Arc::new(IrqSafeSpinlock::new(Fat32Inode {
            fs: self.fs.clone(),
            start_cluster: new_cluster,
            size: 0,
            file_type,
            name: String::from(name),
            parent_cluster: self.start_cluster,
            dir_entry_offset: dir_offset,
        })))
    }

    fn unlink(&mut self, name: &str) -> Result<(), &'static str> {
        if self.file_type != FileType::Directory {
            return Err("Not a directory");
        }

        let entries = self.readdir_internal()?;
        for (item_name, item_cluster, _size, _is_dir, dir_offset) in entries {
            if item_name.eq_ignore_ascii_case(name) {
                // Free cluster chain
                if item_cluster >= 2 {
                    self.fs.free_chain(item_cluster)?;
                }

                // Mark directory entry as deleted (0xE5)
                let cluster_bytes = (self.fs.sectors_per_cluster * 512) as usize;
                let cluster_idx = dir_offset / cluster_bytes;
                let offset_in_cluster = dir_offset % cluster_bytes;

                let mut curr_cluster = self.start_cluster;
                for _ in 0..cluster_idx {
                    if let Some(next) = self.fs.next_cluster(curr_cluster)? {
                        curr_cluster = next;
                    }
                }

                let lba = self.fs.cluster_to_lba(curr_cluster) + (offset_in_cluster / 512) as u64;
                let mut sector = [0u8; 512];
                self.fs.device.read_sectors(lba, 1, &mut sector)?;
                sector[offset_in_cluster % 512] = 0xE5;
                self.fs.device.write_sectors(lba, 1, &sector)?;

                return Ok(());
            }
        }

        Err("File not found")
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
    fn update_parent_dir_entry(&self) -> Result<(), &'static str> {
        if self.dir_entry_offset == 0 {
            return Ok(());
        }

        let cluster_bytes = (self.fs.sectors_per_cluster * 512) as usize;
        let cluster_idx = self.dir_entry_offset / cluster_bytes;
        let offset_in_cluster = self.dir_entry_offset % cluster_bytes;

        let mut curr_cluster = self.parent_cluster;
        for _ in 0..cluster_idx {
            if let Some(next) = self.fs.next_cluster(curr_cluster)? {
                curr_cluster = next;
            }
        }

        let lba = self.fs.cluster_to_lba(curr_cluster) + (offset_in_cluster / 512) as u64;
        let mut sector = [0u8; 512];
        self.fs.device.read_sectors(lba, 1, &mut sector)?;

        let off = offset_in_cluster % 512;
        // Update cluster
        sector[off + 26] = (self.start_cluster & 0xFF) as u8;
        sector[off + 27] = ((self.start_cluster >> 8) & 0xFF) as u8;
        sector[off + 20] = ((self.start_cluster >> 16) & 0xFF) as u8;
        sector[off + 21] = ((self.start_cluster >> 24) & 0xFF) as u8;

        // Update size (32-bit little endian)
        let sz = self.size as u32;
        sector[off + 28] = (sz & 0xFF) as u8;
        sector[off + 29] = ((sz >> 8) & 0xFF) as u8;
        sector[off + 30] = ((sz >> 16) & 0xFF) as u8;
        sector[off + 31] = ((sz >> 24) & 0xFF) as u8;

        self.fs.device.write_sectors(lba, 1, &sector)
    }

    fn allocate_dir_entry(&mut self, name: &str, cluster: u32, size: usize, is_dir: bool) -> Result<usize, &'static str> {
        let mut curr_cluster = self.start_cluster;
        let cluster_bytes = (self.fs.sectors_per_cluster * 512) as usize;
        let mut cluster_buf = [0u8; 4096];
        let mut global_offset = 0;

        loop {
            let lba = self.fs.cluster_to_lba(curr_cluster);
            let sectors = self.fs.sectors_per_cluster.min(8);
            self.fs.device.read_sectors(lba, sectors, &mut cluster_buf[..((sectors * 512) as usize)])?;

            for entry_idx in 0..(cluster_bytes / 32) {
                let off = entry_idx * 32;
                let fb = cluster_buf[off];
                if fb == 0x00 || fb == 0xE5 {
                    // Found empty slot! Write 8.3 entry
                    let mut entry = [0u8; 32];
                    let (name_part, ext_part) = split_filename(name);
                    entry[0..8].copy_from_slice(&name_part);
                    entry[8..11].copy_from_slice(&ext_part);
                    entry[11] = if is_dir { 0x10 } else { 0x20 }; // Archive or Directory

                    entry[20] = ((cluster >> 16) & 0xFF) as u8;
                    entry[21] = ((cluster >> 24) & 0xFF) as u8;
                    entry[26] = (cluster & 0xFF) as u8;
                    entry[27] = ((cluster >> 8) & 0xFF) as u8;

                    let sz = size as u32;
                    entry[28] = (sz & 0xFF) as u8;
                    entry[29] = ((sz >> 8) & 0xFF) as u8;
                    entry[30] = ((sz >> 16) & 0xFF) as u8;
                    entry[31] = ((sz >> 24) & 0xFF) as u8;

                    cluster_buf[off..off + 32].copy_from_slice(&entry);
                    self.fs.device.write_sectors(lba, sectors, &cluster_buf[..((sectors * 512) as usize)])?;

                    return Ok(global_offset + off);
                }
            }

            global_offset += cluster_bytes;
            match self.fs.next_cluster(curr_cluster)? {
                Some(next) => curr_cluster = next,
                None => {
                    // Extend directory with a new cluster
                    let new_cluster = self.fs.alloc_cluster()?;
                    self.fs.link_cluster(curr_cluster, new_cluster)?;
                    curr_cluster = new_cluster;
                }
            }
        }
    }

    fn readdir_internal(&self) -> Result<Vec<(String, u32, usize, bool, usize)>, &'static str> {
        let mut list = Vec::new();
        let mut curr_cluster = self.start_cluster;
        let mut cluster_buf = [0u8; 4096];
        let cluster_bytes = (self.fs.sectors_per_cluster * 512) as usize;
        let mut global_offset = 0;

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
                    extract_lfn_chars(&cluster_buf[offset..offset + 32], &mut lfn_chars);
                    continue;
                }

                // Skip Volume Label (0x08)
                if (attr & 0x08) != 0 {
                    lfn_chars.clear();
                    continue;
                }

                let is_dir = (attr & 0x10) != 0;

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
                    list.push((name, start_cluster, size, is_dir, global_offset + offset));
                }
            }

            global_offset += cluster_bytes;
            match self.fs.next_cluster(curr_cluster)? {
                Some(next) => curr_cluster = next,
                None => break,
            }
        }

        Ok(list)
    }
}

fn split_filename(name: &str) -> ([u8; 8], [u8; 3]) {
    let mut name_part = [b' '; 8];
    let mut ext_part = [b' '; 3];

    let mut parts = name.split('.');
    let base = parts.next().unwrap_or("").to_ascii_uppercase();
    let ext = parts.next().unwrap_or("").to_ascii_uppercase();

    for (i, &b) in base.as_bytes().iter().take(8).enumerate() {
        name_part[i] = b;
    }
    for (i, &b) in ext.as_bytes().iter().take(3).enumerate() {
        ext_part[i] = b;
    }

    (name_part, ext_part)
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
