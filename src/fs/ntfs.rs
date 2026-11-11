extern crate alloc;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::drivers::storage::BlockDevice;
use crate::fs::vfs::{DirEntry, FileType, InodeOperations, Stat};
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

pub struct NtfsFileSystem {
    pub device: Arc<dyn BlockDevice>,
    pub bytes_per_sector: u32,
    pub sectors_per_cluster: u32,
    pub mft_start_cluster: u64,
    pub mft_record_size: u32,
    pub volume_label: String,
}

impl NtfsFileSystem {
    pub fn mount(device: Arc<dyn BlockDevice>) -> Result<Arc<Self>, &'static str> {
        let mut vbr = [0u8; 512];
        device.read_sectors(0, 1, &mut vbr)?;

        if &vbr[3..11] != b"NTFS    " {
            return Err("Invalid NTFS OEM ID");
        }

        if vbr[510] != 0x55 || vbr[511] != 0xAA {
            return Err("Invalid NTFS boot signature");
        }

        let bytes_per_sector = (vbr[11] as u32) | ((vbr[12] as u32) << 8);
        let sectors_per_cluster = vbr[13] as u32;

        if bytes_per_sector != 512 || sectors_per_cluster == 0 {
            return Err("Invalid NTFS sector or cluster parameters");
        }

        let mft_start_cluster = (vbr[48] as u64)
            | ((vbr[49] as u64) << 8)
            | ((vbr[50] as u64) << 16)
            | ((vbr[51] as u64) << 24)
            | ((vbr[52] as u64) << 32)
            | ((vbr[53] as u64) << 40)
            | ((vbr[54] as u64) << 48)
            | ((vbr[55] as u64) << 56);

        let clusters_per_mft_record = vbr[64] as i8;
        let mft_record_size = if clusters_per_mft_record > 0 {
            (clusters_per_mft_record as u32) * sectors_per_cluster * bytes_per_sector
        } else {
            1u32 << ((-clusters_per_mft_record) as u32) // Typically 1 << 10 = 1024 bytes
        };

        let fs = Self {
            device,
            bytes_per_sector,
            sectors_per_cluster,
            mft_start_cluster,
            mft_record_size,
            volume_label: String::from("Windows NTFS"),
        };

        kinfo!(
            "NTFS Filesystem Mounted from {}: Cluster: {} sectors, Record Size: {} bytes, $MFT Cluster: 0x{:X}",
            fs.device.name(),
            sectors_per_cluster,
            mft_record_size,
            mft_start_cluster
        );

        Ok(Arc::new(fs))
    }

    pub fn cluster_to_lba(&self, cluster: u64) -> u64 {
        cluster * (self.sectors_per_cluster as u64)
    }

    /// Read an MFT record by its record index (e.g. record 5 for root directory).
    pub fn read_mft_record(&self, record_index: u64, out: &mut [u8]) -> Result<(), &'static str> {
        let sectors_per_record = (self.mft_record_size / 512).max(1);
        let mft_base_lba = self.cluster_to_lba(self.mft_start_cluster);
        let record_lba = mft_base_lba + (record_index * sectors_per_record as u64);

        let mut record_buf = [0u8; 1024];
        self.device.read_sectors(record_lba, sectors_per_record, &mut record_buf)?;

        if &record_buf[0..4] != b"FILE" {
            return Err("Invalid NTFS MFT record magic (not 'FILE')");
        }

        // Apply NTFS Update Sequence Array (USA fixups)
        let usa_offset = (record_buf[4] as usize) | ((record_buf[5] as usize) << 8);
        let usa_count = (record_buf[6] as usize) | ((record_buf[7] as usize) << 8);

        if usa_offset + usa_count * 2 <= record_buf.len() && usa_count >= 1 {
            let usn = (record_buf[usa_offset] as u16) | ((record_buf[usa_offset + 1] as u16) << 8);
            for s in 0..(usa_count - 1) {
                let sector_end = (s + 1) * 512 - 2;
                let written_usn = (record_buf[sector_end] as u16) | ((record_buf[sector_end + 1] as u16) << 8);
                if written_usn == usn {
                    let fixup = (record_buf[usa_offset + 2 + s * 2] as u16)
                        | ((record_buf[usa_offset + 3 + s * 2] as u16) << 8);
                    record_buf[sector_end] = (fixup & 0xFF) as u8;
                    record_buf[sector_end + 1] = ((fixup >> 8) & 0xFF) as u8;
                }
            }
        }

        let copy_sz = out.len().min(record_buf.len());
        out[..copy_sz].copy_from_slice(&record_buf[..copy_sz]);
        Ok(())
    }
}

pub struct NtfsInode {
    fs: Arc<NtfsFileSystem>,
    mft_record_num: u64,
    size: usize,
    file_type: FileType,
    pub name: String,
    raw_record: [u8; 1024],
}

impl NtfsInode {
    pub fn new_root(fs: Arc<NtfsFileSystem>) -> Result<Arc<IrqSafeSpinlock<dyn InodeOperations>>, &'static str> {
        let mut raw = [0u8; 1024];
        fs.read_mft_record(5, &mut raw)?; // Record 5 is always root directory '/'

        let flags = (raw[22] as u16) | ((raw[23] as u16) << 8);
        let is_dir = (flags & 0x02) != 0;

        Ok(Arc::new(IrqSafeSpinlock::new(Self {
            fs,
            mft_record_num: 5,
            size: 0,
            file_type: if is_dir { FileType::Directory } else { FileType::Regular },
            name: String::from("/"),
            raw_record: raw,
        })))
    }

    /// Read data from resident or non-resident $DATA attribute.
    fn read_data_attr(&self, offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        let mut attr_off = (self.raw_record[20] as usize) | ((self.raw_record[21] as usize) << 8);

        while attr_off + 8 <= self.raw_record.len() {
            let attr_type = (self.raw_record[attr_off] as u32)
                | ((self.raw_record[attr_off + 1] as u32) << 8)
                | ((self.raw_record[attr_off + 2] as u32) << 16)
                | ((self.raw_record[attr_off + 3] as u32) << 24);

            if attr_type == 0xFFFFFFFF {
                break;
            }

            let attr_len = (self.raw_record[attr_off + 4] as usize)
                | ((self.raw_record[attr_off + 5] as usize) << 8);
            if attr_len == 0 || attr_off + attr_len > self.raw_record.len() {
                break;
            }

            if attr_type == 0x80 {
                // $DATA attribute found!
                let non_resident = self.raw_record[attr_off + 8];
                if non_resident == 0 {
                    // Resident data stored directly inside the MFT record
                    let val_len = (self.raw_record[attr_off + 16] as usize)
                        | ((self.raw_record[attr_off + 17] as usize) << 8);
                    let val_off = (self.raw_record[attr_off + 20] as usize)
                        | ((self.raw_record[attr_off + 21] as usize) << 8);

                    if offset >= val_len {
                        return Ok(0);
                    }
                    let avail = val_len - offset;
                    let to_read = buf.len().min(avail);
                    let start = attr_off + val_off + offset;
                    if start + to_read <= self.raw_record.len() {
                        buf[..to_read].copy_from_slice(&self.raw_record[start..start + to_read]);
                        return Ok(to_read);
                    }
                } else {
                    // Non-resident data: decode NTFS data runs
                    let data_size = (self.raw_record[attr_off + 48] as u64)
                        | ((self.raw_record[attr_off + 49] as u64) << 8)
                        | ((self.raw_record[attr_off + 50] as u64) << 16)
                        | ((self.raw_record[attr_off + 51] as u64) << 24)
                        | ((self.raw_record[attr_off + 52] as u64) << 32)
                        | ((self.raw_record[attr_off + 53] as u64) << 40)
                        | ((self.raw_record[attr_off + 54] as u64) << 48)
                        | ((self.raw_record[attr_off + 55] as u64) << 56);

                    let data_sz = data_size as usize;
                    if offset >= data_sz {
                        return Ok(0);
                    }
                    let bytes_to_read = buf.len().min(data_sz - offset);
                    let runs_offset = (self.raw_record[attr_off + 32] as usize)
                        | ((self.raw_record[attr_off + 33] as usize) << 8);

                    return self.read_from_data_runs(attr_off + runs_offset, offset, &mut buf[..bytes_to_read]);
                }
            }

            attr_off += attr_len;
        }

        Ok(0)
    }

    /// Parse and decode NTFS mapping-pair data runs into a vector of (cluster_count, Option<lcn>).
    /// If Option<lcn> is None, it is a sparse run.
    pub fn decode_runlist(raw_record: &[u8], mut run_offset: usize) -> Vec<(u64, Option<i64>)> {
        let mut runs = Vec::new();
        let mut curr_lcn = 0i64;

        while run_offset < raw_record.len() {
            let header = raw_record[run_offset];
            if header == 0 {
                break;
            }
            run_offset += 1;

            let len_bytes = (header & 0x0F) as usize;
            let off_bytes = ((header >> 4) & 0x0F) as usize;

            if run_offset + len_bytes + off_bytes > raw_record.len() {
                break;
            }

            let mut run_len = 0u64;
            for b in 0..len_bytes {
                run_len |= (raw_record[run_offset + b] as u64) << (b * 8);
            }
            run_offset += len_bytes;

            if off_bytes == 0 {
                // Sparse run
                runs.push((run_len, None));
            } else {
                let mut lcn_delta = 0i64;
                for b in 0..off_bytes {
                    lcn_delta |= (raw_record[run_offset + b] as i64) << (b * 8);
                }
                // Sign extension for 2's complement
                if (raw_record[run_offset + off_bytes - 1] & 0x80) != 0 {
                    for b in off_bytes..8 {
                        lcn_delta |= 0xFFi64 << (b * 8);
                    }
                }
                run_offset += off_bytes;

                curr_lcn += lcn_delta;
                runs.push((run_len, Some(curr_lcn)));
            }
        }

        runs
    }

    /// Decode NTFS Data Runs and read bytes into output buffer.
    fn read_from_data_runs(&self, run_offset: usize, offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        let cluster_bytes = (self.fs.sectors_per_cluster * 512) as usize;
        let runs = Self::decode_runlist(&self.raw_record, run_offset);

        let mut logical_byte_pos = 0usize;
        let mut bytes_read = 0usize;
        let mut sector_buf = [0u8; 512];

        for (run_len, lcn_opt) in runs {
            let run_bytes = (run_len as usize) * cluster_bytes;
            let run_end = logical_byte_pos + run_bytes;

            if (offset + bytes_read) < run_end && (offset + bytes_read) >= logical_byte_pos {
                while bytes_read < buf.len() && (offset + bytes_read) < run_end {
                    let offset_in_run = (offset + bytes_read) - logical_byte_pos;
                    let sector_in_run = offset_in_run / 512;
                    let offset_in_sector = offset_in_run % 512;
                    let chunk = (buf.len() - bytes_read).min(512 - offset_in_sector);

                    match lcn_opt {
                        Some(lcn) => {
                            let cluster_in_run = sector_in_run / (self.fs.sectors_per_cluster as usize);
                            let sector_in_cluster = sector_in_run % (self.fs.sectors_per_cluster as usize);
                            let cluster = (lcn as u64) + (cluster_in_run as u64);
                            let lba = self.fs.cluster_to_lba(cluster) + (sector_in_cluster as u64);

                            self.fs.device.read_sectors(lba, 1, &mut sector_buf)?;
                            buf[bytes_read..bytes_read + chunk].copy_from_slice(&sector_buf[offset_in_sector..offset_in_sector + chunk]);
                        }
                        None => {
                            // Sparse run: fill with zeros
                            buf[bytes_read..bytes_read + chunk].fill(0);
                        }
                    }

                    bytes_read += chunk;
                }
            }

            logical_byte_pos = run_end;
            if bytes_read >= buf.len() {
                break;
            }
        }

        Ok(bytes_read)
    }

    /// Get exact file size by scanning the $DATA attribute (resident or non-resident).
    pub fn get_exact_size(&self) -> usize {
        let mut attr_off = (self.raw_record[20] as usize) | ((self.raw_record[21] as usize) << 8);

        while attr_off + 8 <= self.raw_record.len() {
            let attr_type = (self.raw_record[attr_off] as u32)
                | ((self.raw_record[attr_off + 1] as u32) << 8)
                | ((self.raw_record[attr_off + 2] as u32) << 16)
                | ((self.raw_record[attr_off + 3] as u32) << 24);

            if attr_type == 0xFFFFFFFF {
                break;
            }

            let attr_len = (self.raw_record[attr_off + 4] as usize)
                | ((self.raw_record[attr_off + 5] as usize) << 8);
            if attr_len == 0 || attr_off + attr_len > self.raw_record.len() {
                break;
            }

            if attr_type == 0x80 {
                let non_resident = self.raw_record[attr_off + 8];
                if non_resident == 0 {
                    let val_len = (self.raw_record[attr_off + 16] as usize)
                        | ((self.raw_record[attr_off + 17] as usize) << 8);
                    return val_len;
                } else {
                    let real_size = (self.raw_record[attr_off + 48] as u64)
                        | ((self.raw_record[attr_off + 49] as u64) << 8)
                        | ((self.raw_record[attr_off + 50] as u64) << 16)
                        | ((self.raw_record[attr_off + 51] as u64) << 24)
                        | ((self.raw_record[attr_off + 52] as u64) << 32)
                        | ((self.raw_record[attr_off + 53] as u64) << 40)
                        | ((self.raw_record[attr_off + 54] as u64) << 48)
                        | ((self.raw_record[attr_off + 55] as u64) << 56);
                    return real_size as usize;
                }
            }

            attr_off += attr_len;
        }

        self.size
    }
}

impl InodeOperations for NtfsInode {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        if self.file_type == FileType::Directory {
            return Err("Cannot read raw bytes from directory");
        }
        self.read_data_attr(offset, buf)
    }

    fn lookup(&self, name: &str) -> Result<Arc<IrqSafeSpinlock<dyn InodeOperations>>, &'static str> {
        if self.file_type != FileType::Directory {
            return Err("Not a directory");
        }

        let entries = self.readdir_internal()?;
        for (item_name, item_mft, item_size, is_dir) in entries {
            if item_name.eq_ignore_ascii_case(name) {
                let mut raw = [0u8; 1024];
                self.fs.read_mft_record(item_mft, &mut raw)?;

                return Ok(Arc::new(IrqSafeSpinlock::new(NtfsInode {
                    fs: self.fs.clone(),
                    mft_record_num: item_mft,
                    size: item_size,
                    file_type: if is_dir { FileType::Directory } else { FileType::Regular },
                    name: item_name,
                    raw_record: raw,
                })));
            }
        }

        Err("File not found in NTFS directory")
    }

    fn readdir(&self) -> Result<Vec<DirEntry>, &'static str> {
        if self.file_type != FileType::Directory {
            return Err("Not a directory");
        }

        let mut result = Vec::new();
        let entries = self.readdir_internal()?;

        for (item_name, item_mft, _size, is_dir) in entries {
            result.push(DirEntry {
                name: item_name,
                inode: item_mft,
                file_type: if is_dir { FileType::Directory } else { FileType::Regular },
            });
        }

        Ok(result)
    }

    fn stat(&self) -> Stat {
        Stat {
            inode: self.mft_record_num,
            size: self.get_exact_size(),
            file_type: self.file_type,
            mode: if self.file_type == FileType::Directory { 0o755 } else { 0o644 },
        }
    }
}

impl NtfsInode {
    fn readdir_internal(&self) -> Result<Vec<(String, u64, usize, bool)>, &'static str> {
        let mut list = Vec::new();
        let mut attr_off = (self.raw_record[20] as usize) | ((self.raw_record[21] as usize) << 8);

        while attr_off + 8 <= self.raw_record.len() {
            let attr_type = (self.raw_record[attr_off] as u32)
                | ((self.raw_record[attr_off + 1] as u32) << 8)
                | ((self.raw_record[attr_off + 2] as u32) << 16)
                | ((self.raw_record[attr_off + 3] as u32) << 24);

            if attr_type == 0xFFFFFFFF {
                break;
            }

            let attr_len = (self.raw_record[attr_off + 4] as usize)
                | ((self.raw_record[attr_off + 5] as usize) << 8);
            if attr_len == 0 || attr_off + attr_len > self.raw_record.len() {
                break;
            }

            if attr_type == 0x90 {
                // $INDEX_ROOT attribute: root of directory B-Tree
                let val_off = (self.raw_record[attr_off + 20] as usize)
                    | ((self.raw_record[attr_off + 21] as usize) << 8);
                let root_data = attr_off + val_off;

                if root_data + 32 <= self.raw_record.len() {
                    let entries_off = (self.raw_record[root_data + 16] as usize)
                        | ((self.raw_record[root_data + 17] as usize) << 8);
                    let mut entry_pos = root_data + 16 + entries_off;

                    while entry_pos + 16 <= self.raw_record.len() {
                        let mft_ref = (self.raw_record[entry_pos] as u64)
                            | ((self.raw_record[entry_pos + 1] as u64) << 8)
                            | ((self.raw_record[entry_pos + 2] as u64) << 16)
                            | ((self.raw_record[entry_pos + 3] as u64) << 24)
                            | ((self.raw_record[entry_pos + 4] as u64) << 32)
                            | ((self.raw_record[entry_pos + 5] as u64) << 40);

                        let entry_len = (self.raw_record[entry_pos + 8] as usize)
                            | ((self.raw_record[entry_pos + 9] as usize) << 8);
                        let content_len = (self.raw_record[entry_pos + 10] as usize)
                            | ((self.raw_record[entry_pos + 11] as usize) << 8);
                        let flags = (self.raw_record[entry_pos + 12] as u16)
                            | ((self.raw_record[entry_pos + 13] as u16) << 8);

                        if entry_len == 0 {
                            break;
                        }

                        if content_len > 66 && entry_pos + 16 + content_len <= self.raw_record.len() {
                            let fn_off = entry_pos + 16;
                            let data_sz = (self.raw_record[fn_off + 48] as usize)
                                | ((self.raw_record[fn_off + 49] as usize) << 8)
                                | ((self.raw_record[fn_off + 50] as usize) << 16)
                                | ((self.raw_record[fn_off + 51] as usize) << 24);

                            let file_attr = (self.raw_record[fn_off + 56] as u32)
                                | ((self.raw_record[fn_off + 57] as u32) << 8)
                                | ((self.raw_record[fn_off + 58] as u32) << 16)
                                | ((self.raw_record[fn_off + 59] as u32) << 24);
                            let is_dir = (file_attr & 0x10000000) != 0 || (file_attr & 0x10) != 0;

                            let name_len = self.raw_record[fn_off + 64] as usize;
                            let namespace = self.raw_record[fn_off + 65];

                            // Skip pure DOS 8.3 duplicates if namespace == 2 to avoid duplicate entries
                            if namespace != 2 && name_len > 0 {
                                let mut name_str = String::new();
                                for c in 0..name_len {
                                    let ch = (self.raw_record[fn_off + 66 + c * 2] as u16)
                                        | ((self.raw_record[fn_off + 67 + c * 2] as u16) << 8);
                                    if ch != 0 {
                                        name_str.push(ch as u8 as char);
                                    }
                                }

                                if name_str != "." && !name_str.starts_with('$') {
                                    list.push((name_str, mft_ref, data_sz, is_dir));
                                }
                            }
                        }

                        if (flags & 0x02) != 0 {
                            // Last entry in index node
                            break;
                        }
                        entry_pos += entry_len;
                    }
                }
            }

            attr_off += attr_len;
        }

        Ok(list)
    }
}
