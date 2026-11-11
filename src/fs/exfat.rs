extern crate alloc;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::drivers::storage::BlockDevice;
use crate::fs::vfs::{DirEntry, FileType, InodeOperations, Stat};
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

pub struct ExFatFileSystem {
    pub device: Arc<dyn BlockDevice>,
    pub bytes_per_sector: u32,
    pub sectors_per_cluster: u32,
    pub fat_offset: u64,
    pub fat_length: u32,
    pub cluster_heap_offset: u64,
    pub cluster_count: u32,
    pub root_dir_cluster: u32,
    pub volume_label: String,
}

impl ExFatFileSystem {
    pub fn mount(device: Arc<dyn BlockDevice>) -> Result<Arc<Self>, &'static str> {
        let mut vbr = [0u8; 512];
        device.read_sectors(0, 1, &mut vbr)?;

        if &vbr[3..11] != b"EXFAT   " {
            return Err("Invalid exFAT OEM name");
        }

        if vbr[510] != 0x55 || vbr[511] != 0xAA {
            return Err("Invalid exFAT boot signature");
        }

        let fat_offset = (vbr[80] as u64)
            | ((vbr[81] as u64) << 8)
            | ((vbr[82] as u64) << 16)
            | ((vbr[83] as u64) << 24);

        let fat_length = (vbr[84] as u32)
            | ((vbr[85] as u32) << 8)
            | ((vbr[86] as u32) << 16)
            | ((vbr[87] as u32) << 24);

        let cluster_heap_offset = (vbr[88] as u64)
            | ((vbr[89] as u64) << 8)
            | ((vbr[90] as u64) << 16)
            | ((vbr[91] as u64) << 24);

        let cluster_count = (vbr[92] as u32)
            | ((vbr[93] as u32) << 8)
            | ((vbr[94] as u32) << 16)
            | ((vbr[95] as u32) << 24);

        let root_dir_cluster = (vbr[96] as u32)
            | ((vbr[97] as u32) << 8)
            | ((vbr[98] as u32) << 16)
            | ((vbr[99] as u32) << 24);

        let bytes_per_sector_shift = vbr[108];
        let sectors_per_cluster_shift = vbr[109];

        if bytes_per_sector_shift < 9 || bytes_per_sector_shift > 12 {
            return Err("Invalid exFAT sector shift");
        }

        let bytes_per_sector = 1u32 << bytes_per_sector_shift;
        let sectors_per_cluster = 1u32 << sectors_per_cluster_shift;

        let mut fs = Self {
            device,
            bytes_per_sector,
            sectors_per_cluster,
            fat_offset,
            fat_length,
            cluster_heap_offset,
            cluster_count,
            root_dir_cluster,
            volume_label: String::from("exFAT Volume"),
        };

        // Scan root directory for volume label (type 0x83)
        if let Ok(label) = fs.read_volume_label() {
            if !label.is_empty() {
                fs.volume_label = label;
            }
        }

        kinfo!(
            "exFAT Filesystem Mounted from {}: Label '{}', Cluster: {} sectors, Root: Cluster {}, Heap LBA: 0x{:X}",
            fs.device.name(),
            fs.volume_label,
            sectors_per_cluster,
            root_dir_cluster,
            cluster_heap_offset
        );

        Ok(Arc::new(fs))
    }

    pub fn cluster_to_lba(&self, cluster: u32) -> u64 {
        self.cluster_heap_offset + ((cluster.saturating_sub(2)) as u64 * self.sectors_per_cluster as u64)
    }

    pub fn next_cluster(&self, cluster: u32) -> Result<Option<u32>, &'static str> {
        let fat_byte_offset = (cluster as u64) * 4;
        let fat_sector = self.fat_offset + (fat_byte_offset / 512);
        let entry_offset = (fat_byte_offset % 512) as usize;

        let mut buf = [0u8; 512];
        self.device.read_sectors(fat_sector, 1, &mut buf)?;

        let raw = (buf[entry_offset] as u32)
            | ((buf[entry_offset + 1] as u32) << 8)
            | ((buf[entry_offset + 2] as u32) << 16)
            | ((buf[entry_offset + 3] as u32) << 24);

        if raw >= 0xFFFFFFF8 {
            Ok(None)
        } else if raw == 0 || raw == 0xFFFFFFF7 {
            Err("exFAT bad cluster or end reached unexpectedly")
        } else {
            Ok(Some(raw))
        }
    }

    fn read_volume_label(&self) -> Result<String, &'static str> {
        let cluster_bytes = (self.sectors_per_cluster * 512) as usize;
        let mut cluster_buf = [0u8; 4096];
        let lba = self.cluster_to_lba(self.root_dir_cluster);
        let sectors = self.sectors_per_cluster.min(8);

        self.device.read_sectors(lba, sectors, &mut cluster_buf[..((sectors * 512) as usize)])?;

        for i in 0..(cluster_bytes / 32) {
            let off = i * 32;
            let entry_type = cluster_buf[off];
            if entry_type == 0x00 {
                break;
            }
            if entry_type == 0x83 {
                // Volume Label Entry
                let char_count = cluster_buf[off + 1] as usize;
                let mut label = String::new();
                for c in 0..char_count.min(11) {
                    let ch = (cluster_buf[off + 2 + c * 2] as u16) | ((cluster_buf[off + 3 + c * 2] as u16) << 8);
                    if ch != 0 {
                        label.push(ch as u8 as char);
                    }
                }
                return Ok(label);
            }
        }

        Ok(String::from("exFAT Drive"))
    }
}

pub struct ExFatInode {
    fs: Arc<ExFatFileSystem>,
    start_cluster: u32,
    size: usize,
    no_fat_chain: bool,
    file_type: FileType,
    pub name: String,
}

impl ExFatInode {
    pub fn new_root(fs: Arc<ExFatFileSystem>) -> Arc<IrqSafeSpinlock<dyn InodeOperations>> {
        let root_cluster = fs.root_dir_cluster;
        Arc::new(IrqSafeSpinlock::new(Self {
            fs,
            start_cluster: root_cluster,
            size: 0,
            no_fat_chain: false,
            file_type: FileType::Directory,
            name: String::from("/"),
        }))
    }
}

impl InodeOperations for ExFatInode {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        if self.file_type == FileType::Directory {
            return Err("Cannot read raw bytes from directory");
        }

        if offset >= self.size {
            return Ok(0);
        }

        let bytes_to_read = buf.len().min(self.size - offset);
        let cluster_bytes = (self.fs.sectors_per_cluster * 512) as usize;

        let mut curr_cluster = self.start_cluster;
        let mut cluster_skip = offset / cluster_bytes;
        let mut cluster_offset = offset % cluster_bytes;

        if self.no_fat_chain {
            curr_cluster += cluster_skip as u32;
        } else {
            while cluster_skip > 0 {
                match self.fs.next_cluster(curr_cluster)? {
                    Some(next) => curr_cluster = next,
                    None => return Ok(0),
                }
                cluster_skip -= 1;
            }
        }

        let mut bytes_read = 0;
        let mut cluster_buf = [0u8; 4096];

        while bytes_read < bytes_to_read {
            let chunk = (bytes_to_read - bytes_read).min(cluster_bytes - cluster_offset);
            let lba = self.fs.cluster_to_lba(curr_cluster);
            let sectors = self.fs.sectors_per_cluster.min(8);

            self.fs.device.read_sectors(lba, sectors, &mut cluster_buf[..((sectors * 512) as usize)])?;
            buf[bytes_read..bytes_read + chunk].copy_from_slice(&cluster_buf[cluster_offset..cluster_offset + chunk]);
            bytes_read += chunk;
            cluster_offset = 0;

            if bytes_read < bytes_to_read {
                if self.no_fat_chain {
                    curr_cluster += 1;
                } else {
                    match self.fs.next_cluster(curr_cluster)? {
                        Some(next) => curr_cluster = next,
                        None => break,
                    }
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
        for (item_name, item_cluster, item_size, no_fat_chain, is_dir) in entries {
            if item_name.eq_ignore_ascii_case(name) {
                return Ok(Arc::new(IrqSafeSpinlock::new(ExFatInode {
                    fs: self.fs.clone(),
                    start_cluster: item_cluster,
                    size: item_size,
                    no_fat_chain,
                    file_type: if is_dir { FileType::Directory } else { FileType::Regular },
                    name: item_name,
                })));
            }
        }

        Err("File not found in exFAT directory")
    }

    fn readdir(&self) -> Result<Vec<DirEntry>, &'static str> {
        if self.file_type != FileType::Directory {
            return Err("Not a directory");
        }

        let mut result = Vec::new();
        let entries = self.readdir_internal()?;

        for (idx, (item_name, _cluster, _size, _no_chain, is_dir)) in entries.into_iter().enumerate() {
            result.push(DirEntry {
                name: item_name,
                inode: (idx + 2000) as u64,
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

impl ExFatInode {
    fn readdir_internal(&self) -> Result<Vec<(String, u32, usize, bool, bool)>, &'static str> {
        let mut list = Vec::new();
        let mut curr_cluster = self.start_cluster;
        let mut cluster_buf = [0u8; 4096];
        let cluster_bytes = (self.fs.sectors_per_cluster * 512) as usize;

        loop {
            let lba = self.fs.cluster_to_lba(curr_cluster);
            let sectors = self.fs.sectors_per_cluster.min(8);
            self.fs.device.read_sectors(lba, sectors, &mut cluster_buf[..((sectors * 512) as usize)])?;

            let mut idx = 0;
            while idx < (cluster_bytes / 32) {
                let off = idx * 32;
                let entry_type = cluster_buf[off];

                if entry_type == 0x00 {
                    // End of directory
                    return Ok(list);
                }

                // Check for File Directory Entry (0x85)
                if entry_type == 0x85 {
                    let secondary_count = cluster_buf[off + 1] as usize;
                    let file_attr = (cluster_buf[off + 4] as u16) | ((cluster_buf[off + 5] as u16) << 8);
                    let is_dir = (file_attr & 0x10) != 0;

                    let mut file_cluster = 0u32;
                    let mut file_size = 0usize;
                    let mut no_fat_chain = false;
                    let mut file_name = String::new();

                    for s in 1..=secondary_count {
                        if idx + s >= (cluster_bytes / 32) {
                            break;
                        }
                        let s_off = (idx + s) * 32;
                        let s_type = cluster_buf[s_off];

                        if s_type == 0xC0 {
                            // Stream Extension Entry
                            let flags = cluster_buf[s_off + 1];
                            no_fat_chain = (flags & 0x02) != 0;
                            file_cluster = (cluster_buf[s_off + 20] as u32)
                                | ((cluster_buf[s_off + 21] as u32) << 8)
                                | ((cluster_buf[s_off + 22] as u32) << 16)
                                | ((cluster_buf[s_off + 23] as u32) << 24);

                            let sz = (cluster_buf[s_off + 24] as u64)
                                | ((cluster_buf[s_off + 25] as u64) << 8)
                                | ((cluster_buf[s_off + 26] as u64) << 16)
                                | ((cluster_buf[s_off + 27] as u64) << 24)
                                | ((cluster_buf[s_off + 28] as u64) << 32)
                                | ((cluster_buf[s_off + 29] as u64) << 40)
                                | ((cluster_buf[s_off + 30] as u64) << 48)
                                | ((cluster_buf[s_off + 31] as u64) << 56);
                            file_size = sz as usize;
                        } else if s_type == 0xC1 {
                            // File Name Entry (15 UTF-16LE characters)
                            for c in 0..15 {
                                let ch = (cluster_buf[s_off + 2 + c * 2] as u16)
                                    | ((cluster_buf[s_off + 3 + c * 2] as u16) << 8);
                                if ch != 0 {
                                    file_name.push(ch as u8 as char);
                                }
                            }
                        }
                    }

                    if !file_name.is_empty() {
                        list.push((file_name, file_cluster, file_size, no_fat_chain, is_dir));
                    }

                    idx += secondary_count;
                }

                idx += 1;
            }

            if self.no_fat_chain {
                curr_cluster += 1;
            } else {
                match self.fs.next_cluster(curr_cluster)? {
                    Some(next) => curr_cluster = next,
                    None => break,
                }
            }
        }

        Ok(list)
    }
}
