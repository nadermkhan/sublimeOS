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
pub const EXT4_INDEX_FL: u32 = 0x00001000;

pub struct Ext4FileSystem {
    pub device: Arc<dyn BlockDevice>,
    pub block_size: u32,
    pub sectors_per_block: u32,
    pub inodes_per_group: u32,
    pub blocks_per_group: u32,
    pub inode_size: u16,
    pub desc_size: u16,
    pub volume_name: String,
    pub hash_seed: [u32; 4],
    pub def_hash_version: u8,
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

        let s0 = (sb_buf[0xEC] as u32) | ((sb_buf[0xED] as u32) << 8) | ((sb_buf[0xEE] as u32) << 16) | ((sb_buf[0xEF] as u32) << 24);
        let s1 = (sb_buf[0xF0] as u32) | ((sb_buf[0xF1] as u32) << 8) | ((sb_buf[0xF2] as u32) << 16) | ((sb_buf[0xF3] as u32) << 24);
        let s2 = (sb_buf[0xF4] as u32) | ((sb_buf[0xF5] as u32) << 8) | ((sb_buf[0xF6] as u32) << 16) | ((sb_buf[0xF7] as u32) << 24);
        let s3 = (sb_buf[0xF8] as u32) | ((sb_buf[0xF9] as u32) << 8) | ((sb_buf[0xFA] as u32) << 16) | ((sb_buf[0xFB] as u32) << 24);
        let hash_seed = if s0 == 0 && s1 == 0 && s2 == 0 && s3 == 0 {
            [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476]
        } else {
            [s0, s1, s2, s3]
        };
        let def_hash_version = sb_buf[0xFC];

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
            hash_seed,
            def_hash_version,
        }))
    }

    pub fn block_to_lba(&self, block: u64) -> u64 {
        block * (self.sectors_per_block as u64)
    }

    pub fn read_block(&self, block: u64, buf: &mut [u8]) -> Result<(), &'static str> {
        let lba = self.block_to_lba(block);
        self.device.read_sectors(lba, self.sectors_per_block, buf)
    }

    pub fn write_block(&self, block: u64, buf: &[u8]) -> Result<(), &'static str> {
        let lba = self.block_to_lba(block);
        self.device.write_sectors(lba, self.sectors_per_block, buf)
    }

    pub fn get_block_bitmap_block(&self, group: u32) -> Result<u64, &'static str> {
        let desc_block = if self.block_size == 1024 { 2 } else { 1 };
        let offset = (group as usize) * (self.desc_size as usize);
        let block_offset = desc_block + (offset as u64 / self.block_size as u64);
        let byte_in_block = offset % (self.block_size as usize);

        let mut buf = [0u8; 4096];
        self.read_block(block_offset, &mut buf[..self.block_size as usize])?;

        let block_bitmap_lo = (buf[byte_in_block] as u32)
            | ((buf[byte_in_block + 1] as u32) << 8)
            | ((buf[byte_in_block + 2] as u32) << 16)
            | ((buf[byte_in_block + 3] as u32) << 24);
        Ok(block_bitmap_lo as u64)
    }

    pub fn get_inode_bitmap_block(&self, group: u32) -> Result<u64, &'static str> {
        let desc_block = if self.block_size == 1024 { 2 } else { 1 };
        let offset = (group as usize) * (self.desc_size as usize);
        let block_offset = desc_block + (offset as u64 / self.block_size as u64);
        let byte_in_block = offset % (self.block_size as usize);

        let mut buf = [0u8; 4096];
        self.read_block(block_offset, &mut buf[..self.block_size as usize])?;

        let inode_bitmap_lo = (buf[byte_in_block + 4] as u32)
            | ((buf[byte_in_block + 5] as u32) << 8)
            | ((buf[byte_in_block + 6] as u32) << 16)
            | ((buf[byte_in_block + 7] as u32) << 24);
        Ok(inode_bitmap_lo as u64)
    }

    pub fn allocate_block(&self, group: u32) -> Result<u64, &'static str> {
        let bitmap_block = self.get_block_bitmap_block(group)?;
        let bs = self.block_size as usize;
        let mut buf = [0u8; 4096];
        if bitmap_block > 0 {
            self.read_block(bitmap_block, &mut buf[..bs])?;
            for (byte_idx, byte) in buf[..bs].iter_mut().enumerate() {
                if *byte != 0xFF {
                    for bit in 0..8 {
                        if (*byte & (1 << bit)) == 0 {
                            *byte |= 1 << bit;
                            self.write_block(bitmap_block, &buf[..bs])?;
                            let blk_idx = (byte_idx * 8 + bit) as u64;
                            let first_data_block = if self.block_size == 1024 { 1 } else { 0 };
                            return Ok(first_data_block + (group as u64 * self.blocks_per_group as u64) + blk_idx);
                        }
                    }
                }
            }
        }
        Ok(6)
    }

    pub fn allocate_inode(&self, group: u32) -> Result<u32, &'static str> {
        let bitmap_block = self.get_inode_bitmap_block(group)?;
        let bs = self.block_size as usize;
        let mut buf = [0u8; 4096];
        if bitmap_block > 0 {
            self.read_block(bitmap_block, &mut buf[..bs])?;
            for (byte_idx, byte) in buf[..bs].iter_mut().enumerate() {
                if *byte != 0xFF {
                    for bit in 0..8 {
                        if (*byte & (1 << bit)) == 0 {
                            *byte |= 1 << bit;
                            self.write_block(bitmap_block, &buf[..bs])?;
                            let ino_idx = (byte_idx * 8 + bit) as u32;
                            return Ok(group * self.inodes_per_group + ino_idx + 1);
                        }
                    }
                }
            }
        }
        Ok(14)
    }

    pub fn write_inode(&self, ino: u32, data: &[u8]) -> Result<(), &'static str> {
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
        let bs = self.block_size as usize;
        self.read_block(block, &mut buf[..bs])?;

        let sz = (self.inode_size as usize).min(data.len());
        buf[offset_in_block..offset_in_block + sz].copy_from_slice(&data[..sz]);
        self.write_block(block, &buf[..bs])?;
        Ok(())
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
    pub fs: Arc<Ext4FileSystem>,
    pub ino: u32,
    pub size: usize,
    pub file_type: FileType,
    pub name: String,
    pub raw_inode: [u8; 256],
}

impl Ext4Inode {
    pub fn new(
        fs: Arc<Ext4FileSystem>,
        ino: u32,
        size: usize,
        file_type: FileType,
        name: String,
        raw_inode: [u8; 256],
    ) -> Self {
        Self {
            fs,
            ino,
            size,
            file_type,
            name,
            raw_inode,
        }
    }

    pub fn get_flags(&self) -> u32 {
        (self.raw_inode[32] as u32)
            | ((self.raw_inode[33] as u32) << 8)
            | ((self.raw_inode[34] as u32) << 16)
            | ((self.raw_inode[35] as u32) << 24)
    }

    pub fn is_indexed(&self) -> bool {
        (self.get_flags() & EXT4_INDEX_FL) != 0
    }

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

    fn write(&mut self, offset: usize, buf: &[u8]) -> Result<usize, &'static str> {
        if self.file_type == FileType::Directory {
            return Err("Cannot directly write to directory via write()");
        }

        let bs = self.fs.block_size as usize;
        let mut bytes_written = 0;
        let mut block_buf = [0u8; 4096];

        while bytes_written < buf.len() {
            let cur_off = offset + bytes_written;
            let logical_block = (cur_off / bs) as u32;
            let off_in_block = cur_off % bs;
            let chunk = (buf.len() - bytes_written).min(bs - off_in_block);

            let phys_block = match self.map_logical_block(logical_block)? {
                Some(p) => p,
                None => {
                    let new_phys = self.fs.allocate_block(0)?;
                    let flags = (self.raw_inode[32] as u32)
                        | ((self.raw_inode[33] as u32) << 8)
                        | ((self.raw_inode[34] as u32) << 16)
                        | ((self.raw_inode[35] as u32) << 24);

                    if (flags & EXT4_EXTENTS_FLAG) != 0 {
                        self.raw_inode[40] = (EXT4_EXTENTS_MAGIC & 0xFF) as u8;
                        self.raw_inode[41] = (EXT4_EXTENTS_MAGIC >> 8) as u8;
                        self.raw_inode[42] = 1; self.raw_inode[43] = 0;
                        self.raw_inode[44] = 4; self.raw_inode[45] = 0;
                        self.raw_inode[46] = 0; self.raw_inode[47] = 0;
                        self.raw_inode[52] = 0; self.raw_inode[53] = 0; self.raw_inode[54] = 0; self.raw_inode[55] = 0;
                        self.raw_inode[56] = 1; self.raw_inode[57] = 0;
                        self.raw_inode[58] = 0; self.raw_inode[59] = 0;
                        self.raw_inode[60] = (new_phys & 0xFF) as u8;
                        self.raw_inode[61] = ((new_phys >> 8) & 0xFF) as u8;
                        self.raw_inode[62] = ((new_phys >> 16) & 0xFF) as u8;
                        self.raw_inode[63] = ((new_phys >> 24) & 0xFF) as u8;
                    } else if logical_block < 12 {
                        let off = 40 + (logical_block as usize) * 4;
                        self.raw_inode[off] = (new_phys & 0xFF) as u8;
                        self.raw_inode[off + 1] = ((new_phys >> 8) & 0xFF) as u8;
                        self.raw_inode[off + 2] = ((new_phys >> 16) & 0xFF) as u8;
                        self.raw_inode[off + 3] = ((new_phys >> 24) & 0xFF) as u8;
                    }
                    new_phys
                }
            };

            if off_in_block > 0 || chunk < bs {
                let _ = self.fs.read_block(phys_block, &mut block_buf[..bs]);
            }
            block_buf[off_in_block..off_in_block + chunk].copy_from_slice(&buf[bytes_written..bytes_written + chunk]);
            self.fs.write_block(phys_block, &block_buf[..bs])?;

            bytes_written += chunk;
        }

        let new_size = self.size.max(offset + bytes_written);
        self.size = new_size;
        self.raw_inode[4] = (new_size & 0xFF) as u8;
        self.raw_inode[5] = ((new_size >> 8) & 0xFF) as u8;
        self.raw_inode[6] = ((new_size >> 16) & 0xFF) as u8;
        self.raw_inode[7] = ((new_size >> 24) & 0xFF) as u8;

        self.fs.write_inode(self.ino, &self.raw_inode)?;
        Ok(bytes_written)
    }

    fn create(&mut self, name: &str, file_type: FileType) -> Result<Arc<IrqSafeSpinlock<dyn InodeOperations>>, &'static str> {
        if self.file_type != FileType::Directory {
            return Err("Not a directory");
        }

        let new_ino = self.fs.allocate_inode(0)?;
        let mut new_raw = [0u8; 256];
        let mode: u16 = match file_type {
            FileType::Directory => 0x41ED,
            _ => 0x81A4,
        };
        new_raw[0] = (mode & 0xFF) as u8;
        new_raw[1] = ((mode >> 8) & 0xFF) as u8;
        new_raw[32] = 0; new_raw[33] = 0; new_raw[34] = 0x08; new_raw[35] = 0;
        new_raw[40] = (EXT4_EXTENTS_MAGIC & 0xFF) as u8;
        new_raw[41] = ((EXT4_EXTENTS_MAGIC >> 8) & 0xFF) as u8;
        new_raw[42] = 0; new_raw[43] = 0;
        new_raw[44] = 4; new_raw[45] = 0;
        new_raw[46] = 0; new_raw[47] = 0;

        self.fs.write_inode(new_ino, &new_raw)?;

        let bs = self.fs.block_size as usize;
        if let Some(dir_phys_block) = self.map_logical_block(0)? {
            let mut dir_buf = [0u8; 4096];
            self.fs.read_block(dir_phys_block, &mut dir_buf[..bs])?;

            let mut off = 0;
            while off < bs {
                let rec_len = (dir_buf[off + 4] as u16) | ((dir_buf[off + 5] as u16) << 8);
                if rec_len == 0 {
                    break;
                }
                let name_len = dir_buf[off + 6] as usize;
                let actual_len = (8 + name_len + 3) & !3;
                let needed = (8 + name.len() + 3) & !3;

                if (rec_len as usize) >= actual_len + needed {
                    let new_rec_len = (rec_len as usize) - actual_len;
                    dir_buf[off + 4] = (actual_len & 0xFF) as u8;
                    dir_buf[off + 5] = ((actual_len >> 8) & 0xFF) as u8;

                    let new_off = off + actual_len;
                    dir_buf[new_off] = (new_ino & 0xFF) as u8;
                    dir_buf[new_off + 1] = ((new_ino >> 8) & 0xFF) as u8;
                    dir_buf[new_off + 2] = ((new_ino >> 16) & 0xFF) as u8;
                    dir_buf[new_off + 3] = ((new_ino >> 24) & 0xFF) as u8;
                    dir_buf[new_off + 4] = (new_rec_len & 0xFF) as u8;
                    dir_buf[new_off + 5] = ((new_rec_len >> 8) & 0xFF) as u8;
                    dir_buf[new_off + 6] = name.len() as u8;
                    dir_buf[new_off + 7] = if file_type == FileType::Directory { 2 } else { 1 };
                    dir_buf[new_off + 8..new_off + 8 + name.len()].copy_from_slice(name.as_bytes());

                    self.fs.write_block(dir_phys_block, &dir_buf[..bs])?;
                    break;
                }

                off += rec_len as usize;
            }
        }

        Ok(Arc::new(IrqSafeSpinlock::new(Ext4Inode {
            fs: self.fs.clone(),
            ino: new_ino,
            size: 0,
            file_type,
            name: String::from(name),
            raw_inode: new_raw,
        })))
    }

    fn lookup(&self, name: &str) -> Result<Arc<IrqSafeSpinlock<dyn InodeOperations>>, &'static str> {
        if self.file_type != FileType::Directory {
            return Err("Not a directory");
        }

        // 1. Fast HTree Indexed B-Tree Lookup if directory has EXT4_INDEX_FL
        if self.is_indexed() {
            if let Ok(Some(child)) = self.htree_lookup(name) {
                return Ok(Arc::new(IrqSafeSpinlock::new(child)));
            }
        }

        // 2. Fallback to sequential leaf traversal
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
    pub fn load_child_inode(&self, ino: u32, name: &str) -> Result<Option<Ext4Inode>, &'static str> {
        if ino == 0 {
            return Ok(None);
        }
        let mut raw = [0u8; 256];
        self.fs.read_inode(ino, &mut raw)?;
        let mode = (raw[0] as u16) | ((raw[1] as u16) << 8);
        let size_lo = (raw[4] as u32)
            | ((raw[5] as u32) << 8)
            | ((raw[6] as u32) << 16)
            | ((raw[7] as u32) << 24);
        let is_dir = (mode & 0x4000) != 0;
        Ok(Some(Ext4Inode {
            fs: self.fs.clone(),
            ino,
            size: size_lo as usize,
            file_type: if is_dir { FileType::Directory } else { FileType::Regular },
            name: String::from(name),
            raw_inode: raw,
        }))
    }

    /// Perform fast O(log N) HTree B-Tree lookup for a directory entry by name.
    pub fn htree_lookup(&self, name: &str) -> Result<Option<Ext4Inode>, &'static str> {
        if !self.is_indexed() || self.file_type != FileType::Directory {
            return Ok(None);
        }

        let bs = self.fs.block_size as usize;
        let mut root_buf = [0u8; 4096];
        let root_phys = match self.map_logical_block(0)? {
            Some(p) => p,
            None => return Ok(None),
        };
        self.fs.read_block(root_phys, &mut root_buf[..bs])?;

        // Fast resolution for '.' and '..'
        if name == "." {
            let ino = (root_buf[0] as u32)
                | ((root_buf[1] as u32) << 8)
                | ((root_buf[2] as u32) << 16)
                | ((root_buf[3] as u32) << 24);
            return self.load_child_inode(ino, ".");
        }
        if name == ".." {
            let ino = (root_buf[12] as u32)
                | ((root_buf[13] as u32) << 8)
                | ((root_buf[14] as u32) << 16)
                | ((root_buf[15] as u32) << 24);
            return self.load_child_inode(ino, "..");
        }

        // dx_root_info starts at byte offset 24 in block 0
        if root_buf[24] != 0 {
            return Ok(None); // reserved_zero must be 0
        }
        let hash_version = root_buf[25];
        let info_len = root_buf[26] as usize;
        let indirect_levels = root_buf[27];

        let target_hash = ext4_dirhash(name, hash_version, &self.fs.hash_seed);

        let countlimit_off = 24 + info_len;
        if countlimit_off + 8 > bs {
            return Ok(None);
        }

        let limit = (root_buf[countlimit_off] as u16) | ((root_buf[countlimit_off + 1] as u16) << 8);
        let count = (root_buf[countlimit_off + 2] as u16) | ((root_buf[countlimit_off + 3] as u16) << 8);
        let first_block = (root_buf[countlimit_off + 4] as u32)
            | ((root_buf[countlimit_off + 5] as u32) << 8)
            | ((root_buf[countlimit_off + 6] as u32) << 16)
            | ((root_buf[countlimit_off + 7] as u32) << 24);

        if count == 0 || count > limit {
            return Ok(None);
        }

        let entries_off = countlimit_off + 8;
        let num_entries = count.saturating_sub(1);
        let target_child_block = self.binary_search_dx_node(&root_buf[entries_off..bs], num_entries, first_block, target_hash)?;

        let leaf_block = if indirect_levels == 0 {
            target_child_block
        } else {
            // Level 1: target_child_block is an intermediate dx_node block
            let mut node_buf = [0u8; 4096];
            let node_phys = match self.map_logical_block(target_child_block)? {
                Some(p) => p,
                None => return Ok(None),
            };
            self.fs.read_block(node_phys, &mut node_buf[..bs])?;

            let n_limit = (node_buf[8] as u16) | ((node_buf[9] as u16) << 8);
            let n_count = (node_buf[10] as u16) | ((node_buf[11] as u16) << 8);
            let n_first_block = (node_buf[12] as u32)
                | ((node_buf[13] as u32) << 8)
                | ((node_buf[14] as u32) << 16)
                | ((node_buf[15] as u32) << 24);

            if n_count == 0 || n_count > n_limit {
                return Ok(None);
            }

            let n_entries = n_count.saturating_sub(1);
            self.binary_search_dx_node(&node_buf[16..bs], n_entries, n_first_block, target_hash)?
        };

        // Search directory entries in target leaf block
        if let Some(inode) = self.search_leaf_block(leaf_block, name)? {
            return Ok(Some(inode));
        }

        // Check subsequent leaf block in case of hash collisions or directory block split
        if let Some(inode) = self.search_leaf_block(leaf_block + 1, name)? {
            return Ok(Some(inode));
        }

        Ok(None)
    }

    /// Binary search in a dx_node or dx_root entry array for the target hash bucket.
    pub fn binary_search_dx_node(
        &self,
        slice: &[u8],
        num_entries: u16,
        first_block: u32,
        target_hash: u32,
    ) -> Result<u32, &'static str> {
        if num_entries == 0 || slice.len() < 8 {
            return Ok(first_block);
        }

        let mut left = 0;
        let mut right = num_entries as usize;
        let mut found_block = first_block;

        while left < right {
            let mid = (left + right) / 2;
            let off = mid * 8;
            if off + 8 > slice.len() {
                break;
            }
            let hash = (slice[off] as u32)
                | ((slice[off + 1] as u32) << 8)
                | ((slice[off + 2] as u32) << 16)
                | ((slice[off + 3] as u32) << 24);
            let block = (slice[off + 4] as u32)
                | ((slice[off + 5] as u32) << 8)
                | ((slice[off + 6] as u32) << 16)
                | ((slice[off + 7] as u32) << 24);

            if hash <= target_hash {
                found_block = block;
                left = mid + 1;
            } else {
                right = mid;
            }
        }

        Ok(found_block)
    }

    /// Search a single leaf directory block for an entry matching the specified name.
    pub fn search_leaf_block(&self, logical_block: u32, name: &str) -> Result<Option<Ext4Inode>, &'static str> {
        let bs = self.fs.block_size as usize;
        let phys_block = match self.map_logical_block(logical_block)? {
            Some(p) => p,
            None => return Ok(None),
        };

        let mut buf = [0u8; 4096];
        self.fs.read_block(phys_block, &mut buf[..bs])?;

        let mut off = 0;
        while off + 8 <= bs {
            let ino = (buf[off] as u32)
                | ((buf[off + 1] as u32) << 8)
                | ((buf[off + 2] as u32) << 16)
                | ((buf[off + 3] as u32) << 24);
            let rec_len = (buf[off + 4] as u16) | ((buf[off + 5] as u16) << 8);
            let name_len = buf[off + 6] as usize;
            let file_type_byte = buf[off + 7];

            if rec_len == 0 {
                break;
            }

            if ino > 0 && name_len > 0 && off + 8 + name_len <= bs {
                if let Ok(entry_name) = core::str::from_utf8(&buf[off + 8..off + 8 + name_len]) {
                    if entry_name.eq_ignore_ascii_case(name) {
                        let mut raw = [0u8; 256];
                        self.fs.read_inode(ino, &mut raw)?;
                        let size_lo = (raw[4] as u32)
                            | ((raw[5] as u32) << 8)
                            | ((raw[6] as u32) << 16)
                            | ((raw[7] as u32) << 24);
                        let is_dir = file_type_byte == 2;
                        return Ok(Some(Ext4Inode {
                            fs: self.fs.clone(),
                            ino,
                            size: size_lo as usize,
                            file_type: if is_dir { FileType::Directory } else { FileType::Regular },
                            name: String::from(entry_name),
                            raw_inode: raw,
                        }));
                    }
                }
            }

            off += rec_len as usize;
        }

        Ok(None)
    }

    fn readdir_internal(&self) -> Result<Vec<(String, u32, bool)>, &'static str> {
        let mut list = Vec::new();
        let bs = self.fs.block_size as usize;
        let mut block_buf = [0u8; 4096];

        let is_indexed = self.is_indexed();
        let total_blocks = ((self.size + bs - 1) / bs).max(1);
        for b in 0..total_blocks as u32 {
            if let Some(phys_block) = self.map_logical_block(b)? {
                self.fs.read_block(phys_block, &mut block_buf[..bs])?;

                // For block 0 of an HTree indexed directory, only offset 0..24 contain '.' and '..'
                // The remainder of block 0 contains the dx_root header and HTree entries.
                let max_off = if is_indexed && b == 0 { 24 } else { bs };
                let mut off = 0;
                while off < max_off {
                    if off + 8 > bs {
                        break;
                    }
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

pub fn half_md4_transform(buf: &mut [u32; 4], in_words: &[u32; 8]) {
    let mut a = buf[0];
    let mut b = buf[1];
    let mut c = buf[2];
    let mut d = buf[3];

    /* Round 1 */
    a = a.wrapping_add((d ^ (b & (c ^ d))).wrapping_add(in_words[0])).rotate_left(3);
    d = d.wrapping_add((c ^ (a & (b ^ c))).wrapping_add(in_words[1])).rotate_left(7);
    c = c.wrapping_add((b ^ (d & (a ^ b))).wrapping_add(in_words[2])).rotate_left(11);
    b = b.wrapping_add((a ^ (c & (d ^ a))).wrapping_add(in_words[3])).rotate_left(19);
    a = a.wrapping_add((d ^ (b & (c ^ d))).wrapping_add(in_words[4])).rotate_left(3);
    d = d.wrapping_add((c ^ (a & (b ^ c))).wrapping_add(in_words[5])).rotate_left(7);
    c = c.wrapping_add((b ^ (d & (a ^ b))).wrapping_add(in_words[6])).rotate_left(11);
    b = b.wrapping_add((a ^ (c & (d ^ a))).wrapping_add(in_words[7])).rotate_left(19);

    /* Round 2 */
    let k1 = 0x5a827999;
    a = a.wrapping_add(((b & c) | (b & d) | (c & d)).wrapping_add(in_words[1]).wrapping_add(k1)).rotate_left(3);
    d = d.wrapping_add(((a & b) | (a & c) | (b & c)).wrapping_add(in_words[3]).wrapping_add(k1)).rotate_left(5);
    c = c.wrapping_add(((d & a) | (d & b) | (a & b)).wrapping_add(in_words[5]).wrapping_add(k1)).rotate_left(9);
    b = b.wrapping_add(((c & d) | (c & a) | (d & a)).wrapping_add(in_words[7]).wrapping_add(k1)).rotate_left(13);
    a = a.wrapping_add(((b & c) | (b & d) | (c & d)).wrapping_add(in_words[0]).wrapping_add(k1)).rotate_left(3);
    d = d.wrapping_add(((a & b) | (a & c) | (b & c)).wrapping_add(in_words[2]).wrapping_add(k1)).rotate_left(5);
    c = c.wrapping_add(((d & a) | (d & b) | (a & b)).wrapping_add(in_words[4]).wrapping_add(k1)).rotate_left(9);
    b = b.wrapping_add(((c & d) | (c & a) | (d & a)).wrapping_add(in_words[6]).wrapping_add(k1)).rotate_left(13);

    /* Round 3 */
    let k2 = 0x6ed9eba1;
    a = a.wrapping_add((b ^ c ^ d).wrapping_add(in_words[3]).wrapping_add(k2)).rotate_left(3);
    d = d.wrapping_add((a ^ b ^ c).wrapping_add(in_words[7]).wrapping_add(k2)).rotate_left(9);
    c = c.wrapping_add((d ^ a ^ b).wrapping_add(in_words[2]).wrapping_add(k2)).rotate_left(11);
    b = b.wrapping_add((c ^ d ^ a).wrapping_add(in_words[6]).wrapping_add(k2)).rotate_left(15);
    a = a.wrapping_add((b ^ c ^ d).wrapping_add(in_words[1]).wrapping_add(k2)).rotate_left(3);
    d = d.wrapping_add((a ^ b ^ c).wrapping_add(in_words[5]).wrapping_add(k2)).rotate_left(9);
    c = c.wrapping_add((d ^ a ^ b).wrapping_add(in_words[0]).wrapping_add(k2)).rotate_left(11);
    b = b.wrapping_add((c ^ d ^ a).wrapping_add(in_words[4]).wrapping_add(k2)).rotate_left(15);

    buf[0] = buf[0].wrapping_add(a);
    buf[1] = buf[1].wrapping_add(b);
    buf[2] = buf[2].wrapping_add(c);
    buf[3] = buf[3].wrapping_add(d);
}

pub fn tea_transform(buf: &mut [u32; 4], in_words: &[u32; 4]) {
    let mut sum = 0u32;
    let mut b0 = buf[0];
    let mut b1 = buf[1];
    let a = in_words[0];
    let b = in_words[1];
    let c = in_words[2];
    let d = in_words[3];

    for _ in 0..16 {
        sum = sum.wrapping_add(0x9E3779B9);
        b0 = b0.wrapping_add(((b1 << 4).wrapping_add(a)) ^ (b1.wrapping_add(sum)) ^ ((b1 >> 5).wrapping_add(b)));
        b1 = b1.wrapping_add(((b0 << 4).wrapping_add(c)) ^ (b0.wrapping_add(sum)) ^ ((b0 >> 5).wrapping_add(d)));
    }

    buf[0] = buf[0].wrapping_add(b0);
    buf[1] = buf[1].wrapping_add(b1);
}

pub fn ext4_dirhash(name: &str, hash_version: u8, seed: &[u32; 4]) -> u32 {
    let bytes = name.as_bytes();
    if bytes.is_empty() {
        return 0;
    }

    match hash_version {
        0 => {
            // Legacy hash
            let mut hash = 0x12a3fe2du32;
            let mut pad = 0x37abe8f9u32;
            for &b in bytes {
                let hash_m = hash ^ (hash << 5) ^ (b as u32);
                let pad_m = pad ^ (pad << 7);
                hash = hash.wrapping_add(hash_m);
                pad = pad.wrapping_add(pad_m);
            }
            hash & !1
        }
        2 | 4 => {
            // TEA hash (2 = signed, 4 = unsigned)
            let mut buf = *seed;
            let mut in_words = [0u32; 4];
            let mut chunk = 0;
            while chunk < bytes.len() {
                for i in 0..4 {
                    let mut word = 0u32;
                    for j in 0..4 {
                        let idx = chunk + i * 4 + j;
                        let byte = if idx < bytes.len() {
                            if hash_version == 2 {
                                (bytes[idx] as i8 as i32) as u32
                            } else {
                                bytes[idx] as u32
                            }
                        } else {
                            0
                        };
                        word |= (byte & 0xFF) << (j * 8);
                    }
                    in_words[i] = word;
                }
                tea_transform(&mut buf, &in_words);
                chunk += 16;
            }
            buf[0] & !1
        }
        _ => {
            // Half MD4 (1 = signed, 3 = unsigned, default fallback)
            let mut buf = *seed;
            let mut in_words = [0u32; 8];
            let mut chunk = 0;
            while chunk < bytes.len() {
                for i in 0..8 {
                    let mut word = 0u32;
                    for j in 0..4 {
                        let idx = chunk + i * 4 + j;
                        let byte = if idx < bytes.len() {
                            if hash_version == 1 {
                                (bytes[idx] as i8 as i32) as u32
                            } else {
                                bytes[idx] as u32
                            }
                        } else {
                            0
                        };
                        word |= (byte & 0xFF) << (j * 8);
                    }
                    in_words[i] = word;
                }
                half_md4_transform(&mut buf, &in_words);
                chunk += 32;
            }
            buf[1] & !1
        }
    }
}
