extern crate alloc;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::drivers::storage::BlockDevice;
use crate::fs::vfs::{DirEntry, FileType, InodeOperations, Stat};
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

const EXT4_MAGIC: u16 = 0xEF53;
const EXT4_EXTENTS_MAGIC: u16 = 0xF30A;
const EXT4_EXTENTS_FLAG: u32 = 0x80000;

pub struct Ext4FileSystem {
    pub device: Arc<dyn BlockDevice>,
    pub block_size: u32,
    pub sectors_per_block: u32,
    pub inodes_per_group: u32,
    pub blocks_per_group: u32,
    pub inode_size: u16,
    pub desc_size: u16,
    pub volume_name: String,
}

impl Ext4FileSystem {
    pub fn mount(device: Arc<dyn BlockDevice>) -> Result<Arc<Self>, &'static str> {
        let mut sb_buf = [0u8; 1024];

        // Superblock is at byte offset 1024 (LBA 2 for 512-byte sectors)
        device.read_sectors(2, 2, &mut sb_buf)?;

        let magic = (sb_buf[0x38] as u16) | ((sb_buf[0x39] as u16) << 8);
        if magic != EXT4_MAGIC {
            return Err("Invalid ext4 superblock magic");
        }

        let log_block_size = (sb_buf[0x18] as u32)
            | ((sb_buf[0x19] as u32) << 8)
            | ((sb_buf[0x1A] as u32) << 16)
            | ((sb_buf[0x1B] as u32) << 24);
        let block_size = 1024u32 << log_block_size;
        let sectors_per_block = (block_size / 512).max(1);

        let blocks_per_group = (sb_buf[0x20] as u32)
            | ((sb_buf[0x21] as u32) << 8)
            | ((sb_buf[0x22] as u32) << 16)
            | ((sb_buf[0x23] as u32) << 24);

        let inodes_per_group = (sb_buf[0x28] as u32)
            | ((sb_buf[0x29] as u32) << 8)
            | ((sb_buf[0x2A] as u32) << 16)
            | ((sb_buf[0x2B] as u32) << 24);

        let inode_size = (sb_buf[0x58] as u16) | ((sb_buf[0x59] as u16) << 8);
        let desc_size_raw = (sb_buf[0xFE] as u16) | ((sb_buf[0xFF] as u16) << 8);
        let desc_size = if desc_size_raw > 0 { desc_size_raw } else { 32 };

        let mut vol_name = String::from("Linux ext4");
        if let Ok(name) = core::str::from_utf8(&sb_buf[0x78..0x88]) {
            let trimmed = name.trim_matches('\0').trim();
            if !trimmed.is_empty() {
                vol_name = String::from(trimmed);
            }
        }

        kinfo!(
            "ext4 Filesystem Mounted from {}: Volume '{}', Block: {} bytes, Inode Size: {}, Inodes/Group: {}",
            device.name(),
            vol_name,
            block_size,
            inode_size,
            inodes_per_group
        );

        Ok(Arc::new(Self {
            device,
            block_size,
            sectors_per_block,
            inodes_per_group,
            blocks_per_group,
            inode_size,
            desc_size,
            volume_name: vol_name,
        }))
    }

    pub fn block_to_lba(&self, block: u64) -> u64 {
        block * (self.sectors_per_block as u64)
    }

    pub fn read_block(&self, block: u64, buf: &mut [u8]) -> Result<(), &'static str> {
        let lba = self.block_to_lba(block);
        self.device.read_sectors(lba, self.sectors_per_block, buf)
    }

    pub fn get_inode_table_block(&self, group: u32) -> Result<u64, &'static str> {
        // Block group descriptor table starts immediately after superblock
        let desc_block = if self.block_size == 1024 { 2 } else { 1 };
        let offset = (group as usize) * (self.desc_size as usize);
        let block_offset = desc_block + (offset as u64 / self.block_size as u64);
        let byte_in_block = offset % (self.block_size as usize);

        let mut buf = [0u8; 4096];
        self.read_block(block_offset, &mut buf[..self.block_size as usize])?;

        let inode_table_lo = (buf[byte_in_block + 8] as u32)
            | ((buf[byte_in_block + 9] as u32) << 8)
            | ((buf[byte_in_block + 10] as u32) << 16)
            | ((buf[byte_in_block + 11] as u32) << 24);

        let inode_table_hi = if self.desc_size >= 64 {
            (buf[byte_in_block + 40] as u32)
                | ((buf[byte_in_block + 41] as u32) << 8)
                | ((buf[byte_in_block + 42] as u32) << 16)
                | ((buf[byte_in_block + 43] as u32) << 24)
        } else {
            0
        };

        Ok(((inode_table_hi as u64) << 32) | (inode_table_lo as u64))
    }

    pub fn read_inode(&self, ino: u32, out: &mut [u8]) -> Result<(), &'static str> {
        if ino == 0 {
            return Err("Invalid ext4 inode 0");
        }

        let group = (ino - 1) / self.inodes_per_group;
        let index = (ino - 1) % self.inodes_per_group;

        let table_start_block = self.get_inode_table_block(group)?;
        let byte_offset = (index as u64) * (self.inode_size as u64);
        let block = table_start_block + (byte_offset / self.block_size as u64);
        let offset_in_block = (byte_offset % self.block_size as u64) as usize;

        let mut buf = [0u8; 4096];
        self.read_block(block, &mut buf[..self.block_size as usize])?;

        let sz = (self.inode_size as usize).min(out.len());
        out[..sz].copy_from_slice(&buf[offset_in_block..offset_in_block + sz]);
        Ok(())
    }
}

pub struct Ext4Inode {
    fs: Arc<Ext4FileSystem>,
    ino: u32,
    size: usize,
    file_type: FileType,
    pub name: String,
    raw_inode: [u8; 256],
}

impl Ext4Inode {
    pub fn new_root(fs: Arc<Ext4FileSystem>) -> Result<Arc<IrqSafeSpinlock<dyn InodeOperations>>, &'static str> {
        let mut raw = [0u8; 256];
        fs.read_inode(2, &mut raw)?; // Inode 2 is always root '/'

        let mode = (raw[0] as u16) | ((raw[1] as u16) << 8);
        let size_lo = (raw[4] as u32)
            | ((raw[5] as u32) << 8)
            | ((raw[6] as u32) << 16)
            | ((raw[7] as u32) << 24);

        let is_dir = (mode & 0x4000) != 0;

        Ok(Arc::new(IrqSafeSpinlock::new(Self {
            fs,
            ino: 2,
            size: size_lo as usize,
            file_type: if is_dir { FileType::Directory } else { FileType::Regular },
            name: String::from("/"),
            raw_inode: raw,
        })))
    }

    /// Map logical file block to physical disk block using Extents or Direct Blocks.
    pub fn map_logical_block(&self, logical_block: u32) -> Result<Option<u64>, &'static str> {
        let flags = (self.raw_inode[32] as u32)
            | ((self.raw_inode[33] as u32) << 8)
            | ((self.raw_inode[34] as u32) << 16)
            | ((self.raw_inode[35] as u32) << 24);

        if (flags & EXT4_EXTENTS_FLAG) != 0 {
            // Extents Tree parser
            let ext_header = &self.raw_inode[40..52];
            let magic = (ext_header[0] as u16) | ((ext_header[1] as u16) << 8);
            if magic != EXT4_EXTENTS_MAGIC {
                return Err("Corrupt ext4 extent header magic");
            }
            let entries = (ext_header[2] as u16) | ((ext_header[3] as u16) << 8);
            let depth = (ext_header[6] as u16) | ((ext_header[7] as u16) << 8);

            self.parse_extents_node(&self.raw_inode[40..100], entries, depth, logical_block)
        } else {
            // Legacy direct block pointers
            if logical_block < 12 {
                let off = 40 + (logical_block as usize) * 4;
                let phys = (self.raw_inode[off] as u32)
                    | ((self.raw_inode[off + 1] as u32) << 8)
                    | ((self.raw_inode[off + 2] as u32) << 16)
                    | ((self.raw_inode[off + 3] as u32) << 24);
                if phys > 0 {
                    Ok(Some(phys as u64))
                } else {
                    Ok(None)
                }
            } else {
                Ok(None)
            }
        }
    }

    fn parse_extents_node(
        &self,
        node_bytes: &[u8],
        entries: u16,
        depth: u16,
        target_block: u32,
    ) -> Result<Option<u64>, &'static str> {
        if depth == 0 {
            // Leaf node: array of ext4_extent (12 bytes each, starting at offset 12)
            for i in 0..entries as usize {
                let off = 12 + i * 12;
                if off + 12 > node_bytes.len() {
                    break;
                }
                let ee_block = (node_bytes[off] as u32)
                    | ((node_bytes[off + 1] as u32) << 8)
                    | ((node_bytes[off + 2] as u32) << 16)
                    | ((node_bytes[off + 3] as u32) << 24);
                let ee_len = (node_bytes[off + 4] as u16) | ((node_bytes[off + 5] as u16) << 8);
                let ee_start_hi = (node_bytes[off + 6] as u16) | ((node_bytes[off + 7] as u16) << 8);
                let ee_start_lo = (node_bytes[off + 8] as u32)
                    | ((node_bytes[off + 9] as u32) << 8)
                    | ((node_bytes[off + 10] as u32) << 16)
                    | ((node_bytes[off + 11] as u32) << 24);

                let start_phys = ((ee_start_hi as u64) << 32) | (ee_start_lo as u64);
                let len = if ee_len <= 32768 { ee_len as u32 } else { (ee_len - 32768) as u32 };

                if target_block >= ee_block && target_block < ee_block + len {
                    let offset_in_extent = target_block - ee_block;
                    return Ok(Some(start_phys + offset_in_extent as u64));
                }
            }
            Ok(None)
        } else {
            // Index node: array of ext4_extent_idx (12 bytes each)
            let mut next_phys_block = 0u64;
            for i in 0..entries as usize {
                let off = 12 + i * 12;
                if off + 12 > node_bytes.len() {
                    break;
                }
                let ei_block = (node_bytes[off] as u32)
                    | ((node_bytes[off + 1] as u32) << 8)
                    | ((node_bytes[off + 2] as u32) << 16)
                    | ((node_bytes[off + 3] as u32) << 24);
                let ei_leaf_lo = (node_bytes[off + 4] as u32)
                    | ((node_bytes[off + 5] as u32) << 8)
                    | ((node_bytes[off + 6] as u32) << 16)
                    | ((node_bytes[off + 7] as u32) << 24);
                let ei_leaf_hi = (node_bytes[off + 8] as u16) | ((node_bytes[off + 9] as u16) << 8);

                let leaf_block = ((ei_leaf_hi as u64) << 32) | (ei_leaf_lo as u64);
                if target_block >= ei_block {
                    next_phys_block = leaf_block;
                } else {
                    break;
                }
            }

            if next_phys_block > 0 {
                let mut child_buf = [0u8; 4096];
                self.fs.read_block(next_phys_block, &mut child_buf[..self.fs.block_size as usize])?;
                let c_entries = (child_buf[2] as u16) | ((child_buf[3] as u16) << 8);
                let c_depth = (child_buf[6] as u16) | ((child_buf[7] as u16) << 8);
                self.parse_extents_node(&child_buf[..self.fs.block_size as usize], c_entries, c_depth, target_block)
            } else {
                Ok(None)
            }
        }
    }
}

impl InodeOperations for Ext4Inode {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        if self.file_type == FileType::Directory {
            return Err("Cannot read raw bytes from directory");
        }

        if offset >= self.size {
            return Ok(0);
        }

        let bytes_to_read = buf.len().min(self.size - offset);
        let bs = self.fs.block_size as usize;

        let mut bytes_read = 0;
        let mut block_buf = [0u8; 4096];

        while bytes_read < bytes_to_read {
            let current_offset = offset + bytes_read;
            let logical_block = (current_offset / bs) as u32;
            let offset_in_block = current_offset % bs;
            let chunk = (bytes_to_read - bytes_read).min(bs - offset_in_block);

            if let Some(phys_block) = self.map_logical_block(logical_block)? {
                self.fs.read_block(phys_block, &mut block_buf[..bs])?;
                buf[bytes_read..bytes_read + chunk].copy_from_slice(&block_buf[offset_in_block..offset_in_block + chunk]);
            } else {
                // Sparse hole: zero filled
                buf[bytes_read..bytes_read + chunk].fill(0);
            }

            bytes_read += chunk;
        }

        Ok(bytes_read)
    }

    fn lookup(&self, name: &str) -> Result<Arc<IrqSafeSpinlock<dyn InodeOperations>>, &'static str> {
        if self.file_type != FileType::Directory {
            return Err("Not a directory");
        }

        let entries = self.readdir_internal()?;
        for (item_name, item_ino, is_dir) in entries {
            if item_name.eq_ignore_ascii_case(name) {
                let mut raw = [0u8; 256];
                self.fs.read_inode(item_ino, &mut raw)?;

                let size_lo = (raw[4] as u32)
                    | ((raw[5] as u32) << 8)
                    | ((raw[6] as u32) << 16)
                    | ((raw[7] as u32) << 24);

                return Ok(Arc::new(IrqSafeSpinlock::new(Ext4Inode {
                    fs: self.fs.clone(),
                    ino: item_ino,
                    size: size_lo as usize,
                    file_type: if is_dir { FileType::Directory } else { FileType::Regular },
                    name: item_name,
                    raw_inode: raw,
                })));
            }
        }

        Err("File not found in ext4 directory")
    }

    fn readdir(&self) -> Result<Vec<DirEntry>, &'static str> {
        if self.file_type != FileType::Directory {
            return Err("Not a directory");
        }

        let mut result = Vec::new();
        let entries = self.readdir_internal()?;

        for (item_name, item_ino, is_dir) in entries {
            result.push(DirEntry {
                name: item_name,
                inode: item_ino as u64,
                file_type: if is_dir { FileType::Directory } else { FileType::Regular },
            });
        }

        Ok(result)
    }

    fn stat(&self) -> Stat {
        Stat {
            inode: self.ino as u64,
            size: self.size,
            file_type: self.file_type,
            mode: if self.file_type == FileType::Directory { 0o755 } else { 0o644 },
        }
    }
}

impl Ext4Inode {
    fn readdir_internal(&self) -> Result<Vec<(String, u32, bool)>, &'static str> {
        let mut list = Vec::new();
        let bs = self.fs.block_size as usize;
        let mut block_buf = [0u8; 4096];

        let total_blocks = ((self.size + bs - 1) / bs).max(1);
        for b in 0..total_blocks as u32 {
            if let Some(phys_block) = self.map_logical_block(b)? {
                self.fs.read_block(phys_block, &mut block_buf[..bs])?;

                let mut off = 0;
                while off < bs {
                    let ino = (block_buf[off] as u32)
                        | ((block_buf[off + 1] as u32) << 8)
                        | ((block_buf[off + 2] as u32) << 16)
                        | ((block_buf[off + 3] as u32) << 24);
                    let rec_len = (block_buf[off + 4] as u16) | ((block_buf[off + 5] as u16) << 8);
                    let name_len = block_buf[off + 6] as usize;
                    let file_type_byte = block_buf[off + 7];

                    if rec_len == 0 {
                        break;
                    }

                    if ino > 0 && name_len > 0 && off + 8 + name_len <= bs {
                        if let Ok(entry_name) = core::str::from_utf8(&block_buf[off + 8..off + 8 + name_len]) {
                            if entry_name != "." && entry_name != ".." {
                                let is_dir = file_type_byte == 2;
                                list.push((String::from(entry_name), ino, is_dir));
                            }
                        }
                    }

                    off += rec_len as usize;
                }
            }
        }

        Ok(list)
    }
}
