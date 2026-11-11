extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::drivers::storage::BlockDevice;

/// A virtual BlockDevice representing a sub-slice / partition of an underlying physical disk.
pub struct PartitionBlockDevice {
    pub parent: Arc<dyn BlockDevice>,
    pub start_lba: u64,
    pub num_sectors: u64,
    pub name: String,
    pub partition_type: u8,
}

impl PartitionBlockDevice {
    pub fn new(
        parent: Arc<dyn BlockDevice>,
        start_lba: u64,
        num_sectors: u64,
        name: String,
        partition_type: u8,
    ) -> Self {
        Self {
            parent,
            start_lba,
            num_sectors,
            name,
            partition_type,
        }
    }
}

impl BlockDevice for PartitionBlockDevice {
    fn name(&self) -> &str {
        &self.name
    }

    fn read_sectors(&self, lba: u64, count: u32, buf: &mut [u8]) -> Result<(), &'static str> {
        if lba + count as u64 > self.num_sectors {
            return Err("Read out of partition bounds");
        }
        self.parent.read_sectors(self.start_lba + lba, count, buf)
    }

    fn write_sectors(&self, lba: u64, count: u32, buf: &[u8]) -> Result<(), &'static str> {
        if lba + count as u64 > self.num_sectors {
            return Err("Write out of partition bounds");
        }
        self.parent.write_sectors(self.start_lba + lba, count, buf)
    }

    fn total_sectors(&self) -> u64 {
        self.num_sectors
    }

    fn sector_size(&self) -> usize {
        self.parent.sector_size()
    }
}

/// Scan a block device for MBR or GPT partitions.
/// Returns a list of partition devices. If no partition table is found,
/// returns a single partition device spanning the entire disk.
pub fn scan_device_partitions(device: Arc<dyn BlockDevice>) -> Vec<Arc<dyn BlockDevice>> {
    let mut partitions: Vec<Arc<dyn BlockDevice>> = Vec::new();
    let mut sector0 = [0u8; 512];

    if device.read_sectors(0, 1, &mut sector0).is_err() {
        return partitions;
    }

    // Check for valid boot signature 0x55 0xAA at offset 510
    if sector0[510] != 0x55 || sector0[511] != 0xAA {
        // Unpartitioned raw drive
        partitions.push(device.clone());
        return partitions;
    }

    // 1. Check for GPT Protective MBR (Partition type 0xEE)
    let is_gpt_protective = sector0[446 + 4] == 0xEE;
    if is_gpt_protective {
        if let Some(gpt_parts) = scan_gpt_partitions(device.clone()) {
            if !gpt_parts.is_empty() {
                return gpt_parts;
            }
        }
    }

    // 2. Scan MBR Partitions (4 entries at offset 446)
    for i in 0..4 {
        let offset = 446 + i * 16;
        let part_type = sector0[offset + 4];
        let start_lba = (sector0[offset + 8] as u32)
            | ((sector0[offset + 9] as u32) << 8)
            | ((sector0[offset + 10] as u32) << 16)
            | ((sector0[offset + 11] as u32) << 24);
        let num_sectors = (sector0[offset + 12] as u32)
            | ((sector0[offset + 13] as u32) << 8)
            | ((sector0[offset + 14] as u32) << 16)
            | ((sector0[offset + 15] as u32) << 24);

        if part_type != 0 && start_lba > 0 && num_sectors > 0 {
            if part_type == 0x05 || part_type == 0x0F {
                // Extended partition container: scan EBR chain
                let mut ebr_lba = start_lba as u64;
                let ext_base = ebr_lba;
                let mut ebr_buf = [0u8; 512];
                let mut logical_idx = 5;

                while ebr_lba > 0 && logical_idx <= 16 {
                    if device.read_sectors(ebr_lba, 1, &mut ebr_buf).is_err() {
                        break;
                    }
                    if ebr_buf[510] != 0x55 || ebr_buf[511] != 0xAA {
                        break;
                    }

                    let log_type = ebr_buf[446 + 4];
                    let log_start = (ebr_buf[446 + 8] as u32)
                        | ((ebr_buf[446 + 9] as u32) << 8)
                        | ((ebr_buf[446 + 10] as u32) << 16)
                        | ((ebr_buf[446 + 11] as u32) << 24);
                    let log_len = (ebr_buf[446 + 12] as u32)
                        | ((ebr_buf[446 + 13] as u32) << 8)
                        | ((ebr_buf[446 + 14] as u32) << 16)
                        | ((ebr_buf[446 + 15] as u32) << 24);

                    if log_type != 0 && log_len > 0 {
                        let log_name = format!("{}p{}", device.name(), logical_idx);
                        partitions.push(Arc::new(PartitionBlockDevice::new(
                            device.clone(),
                            ebr_lba + log_start as u64,
                            log_len as u64,
                            log_name,
                            log_type,
                        )));
                        logical_idx += 1;
                    }

                    let next_rel = (ebr_buf[446 + 16 + 8] as u32)
                        | ((ebr_buf[446 + 16 + 9] as u32) << 8)
                        | ((ebr_buf[446 + 16 + 10] as u32) << 16)
                        | ((ebr_buf[446 + 16 + 11] as u32) << 24);

                    if next_rel > 0 {
                        ebr_lba = ext_base + next_rel as u64;
                    } else {
                        break;
                    }
                }
            } else {
                let part_name = format!("{}p{}", device.name(), i + 1);
                let part_dev = Arc::new(PartitionBlockDevice::new(
                    device.clone(),
                    start_lba as u64,
                    num_sectors as u64,
                    part_name,
                    part_type,
                ));
                partitions.push(part_dev);
            }
        }
    }

    // If no MBR partitions found, treat whole disk as single volume (e.g. superfloppy / direct VBR)
    if partitions.is_empty() {
        partitions.push(device.clone());
    }

    partitions
}

/// Scan GPT (GUID Partition Table) starting at LBA 1.
fn scan_gpt_partitions(device: Arc<dyn BlockDevice>) -> Option<Vec<Arc<dyn BlockDevice>>> {
    let mut header_buf = [0u8; 512];
    device.read_sectors(1, 1, &mut header_buf).ok()?;

    // Check GPT signature: "EFI PART"
    if &header_buf[0..8] != b"EFI PART" {
        return None;
    }

    let part_entry_lba = (header_buf[72] as u64)
        | ((header_buf[73] as u64) << 8)
        | ((header_buf[74] as u64) << 16)
        | ((header_buf[75] as u64) << 24)
        | ((header_buf[76] as u64) << 32)
        | ((header_buf[77] as u64) << 40)
        | ((header_buf[78] as u64) << 48)
        | ((header_buf[79] as u64) << 56);

    let num_part_entries = (header_buf[80] as u32)
        | ((header_buf[81] as u32) << 8)
        | ((header_buf[82] as u32) << 16)
        | ((header_buf[83] as u32) << 24);

    let entry_size = (header_buf[84] as u32)
        | ((header_buf[85] as u32) << 8)
        | ((header_buf[86] as u32) << 16)
        | ((header_buf[87] as u32) << 24);

    if entry_size < 128 || num_part_entries == 0 {
        return None;
    }

    let mut partitions: Vec<Arc<dyn BlockDevice>> = Vec::new();
    let entries_per_sector = 512 / entry_size;
    let sectors_to_read = (num_part_entries + entries_per_sector - 1) / entries_per_sector;
    let mut sector_buf = [0u8; 512];

    let mut entry_idx = 0;
    for s in 0..sectors_to_read.min(32) {
        if device.read_sectors(part_entry_lba + s as u64, 1, &mut sector_buf).is_err() {
            break;
        }

        for i in 0..entries_per_sector {
            if entry_idx >= num_part_entries {
                break;
            }
            let off = (i * entry_size) as usize;

            // Check if Partition Type GUID is all zero (unused entry)
            let is_unused = sector_buf[off..off + 16].iter().all(|&b| b == 0);
            if !is_unused {
                let start_lba = (sector_buf[off + 32] as u64)
                    | ((sector_buf[off + 33] as u64) << 8)
                    | ((sector_buf[off + 34] as u64) << 16)
                    | ((sector_buf[off + 35] as u64) << 24)
                    | ((sector_buf[off + 36] as u64) << 32)
                    | ((sector_buf[off + 37] as u64) << 40)
                    | ((sector_buf[off + 38] as u64) << 48)
                    | ((sector_buf[off + 39] as u64) << 56);

                let end_lba = (sector_buf[off + 40] as u64)
                    | ((sector_buf[off + 41] as u64) << 8)
                    | ((sector_buf[off + 42] as u64) << 16)
                    | ((sector_buf[off + 43] as u64) << 24)
                    | ((sector_buf[off + 44] as u64) << 32)
                    | ((sector_buf[off + 45] as u64) << 40)
                    | ((sector_buf[off + 46] as u64) << 48)
                    | ((sector_buf[off + 47] as u64) << 56);

                if end_lba >= start_lba {
                    let num_sectors = end_lba - start_lba + 1;
                    let part_name = format!("{}p{}", device.name(), partitions.len() + 1);
                    let part_dev = Arc::new(PartitionBlockDevice::new(
                        device.clone(),
                        start_lba,
                        num_sectors,
                        part_name,
                        0xEE,
                    ));
                    partitions.push(part_dev);
                }
            }
            entry_idx += 1;
        }
    }

    Some(partitions)
}
