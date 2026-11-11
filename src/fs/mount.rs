extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::drivers::storage::{BlockDevice, STORAGE_MANAGER};
use crate::fs::exfat::{ExFatFileSystem, ExFatInode};
use crate::fs::ext4::{Ext4FileSystem, Ext4Inode};
use crate::fs::fat32::{Fat32FileSystem, Fat32Inode};
use crate::fs::ntfs::{NtfsFileSystem, NtfsInode};
use crate::fs::partition::scan_device_partitions;
use crate::fs::vfs::{InodeOperations, VFS};
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

#[derive(Clone)]
pub struct MountEntry {
    pub device_name: String,
    pub mount_point: String,
    pub fs_type: String,
    pub options: String,
}

pub struct MountTable {
    pub entries: Vec<MountEntry>,
}

impl MountTable {
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn add(&mut self, device: &str, mount_point: &str, fs_type: &str, options: &str) {
        self.entries.push(MountEntry {
            device_name: String::from(device),
            mount_point: String::from(mount_point),
            fs_type: String::from(fs_type),
            options: String::from(options),
        });
    }

    pub fn list(&self) -> Vec<MountEntry> {
        self.entries.clone()
    }
}

pub static MOUNT_TABLE: IrqSafeSpinlock<MountTable> = IrqSafeSpinlock::new(MountTable::new());

pub enum DetectedFs {
    Fat32(Arc<Fat32FileSystem>, Arc<IrqSafeSpinlock<dyn InodeOperations>>),
    ExFat(Arc<ExFatFileSystem>, Arc<IrqSafeSpinlock<dyn InodeOperations>>),
    Ext4(Arc<Ext4FileSystem>, Arc<IrqSafeSpinlock<dyn InodeOperations>>),
    Ntfs(Arc<NtfsFileSystem>, Arc<IrqSafeSpinlock<dyn InodeOperations>>),
}

/// Sniff boot sector / superblock signatures to identify and mount filesystem.
pub fn probe_filesystem(
    device: Arc<dyn BlockDevice>,
) -> Result<(Arc<IrqSafeSpinlock<dyn InodeOperations>>, &'static str, String), &'static str> {
    let mut sector0 = [0u8; 512];
    device.read_sectors(0, 1, &mut sector0)?;

    // 1. Check for exFAT ("EXFAT   " at offset 3)
    if &sector0[3..11] == b"EXFAT   " {
        let fs = ExFatFileSystem::mount(device)?;
        let label = fs.volume_label.clone();
        let root = ExFatInode::new_root(fs);
        return Ok((root, "exfat", label));
    }

    // 2. Check for NTFS ("NTFS    " at offset 3)
    if &sector0[3..11] == b"NTFS    " {
        let fs = NtfsFileSystem::mount(device)?;
        let label = fs.volume_label.clone();
        let root = NtfsInode::new_root(fs)?;
        return Ok((root, "ntfs", label));
    }

    // 3. Check for ext4 (Superblock at byte 1024, magic 0xEF53 at offset 56)
    let mut sb_buf = [0u8; 512];
    if device.read_sectors(2, 1, &mut sb_buf).is_ok() {
        let magic = (sb_buf[0x38] as u16) | ((sb_buf[0x39] as u16) << 8);
        if magic == 0xEF53 {
            let fs = Ext4FileSystem::mount(device)?;
            let label = fs.volume_name.clone();
            let root = Ext4Inode::new_root(fs)?;
            return Ok((root, "ext4", label));
        }
    }

    // 4. Check for FAT32
    if sector0[510] == 0x55 && sector0[511] == 0xAA {
        if let Ok(fs) = Fat32FileSystem::mount(device) {
            let label = fs.volume_label.clone();
            let root = Fat32Inode::new_root(fs);
            return Ok((root, "fat32", label));
        }
    }

    Err("Unknown or unsupported filesystem")
}

/// Mount a single partition device into /Volumes/ if it contains a recognizable filesystem.
pub fn mount_partition_device(part: Arc<dyn BlockDevice>) -> Result<String, &'static str> {
    let (root_inode, fs_type, volume_label) = probe_filesystem(part.clone())?;

    let vfs = VFS.lock();
    let vol_dir = vfs.resolve_path("/Volumes")?;
    drop(vfs);

    let base_name = if !volume_label.is_empty() && volume_label != "NO NAME" && volume_label != "exFAT Drive" {
        volume_label.clone()
    } else {
        format!("{}_{}", fs_type.to_ascii_uppercase(), part.name())
    };

    // Ensure unique mount point name under /Volumes/
    let mut mount_name = base_name.clone();
    let mut suffix = 1;
    while vol_dir.lock().lookup(&mount_name).is_ok() {
        mount_name = format!("{} {}", base_name, suffix);
        suffix += 1;
    }

    vol_dir.lock().add_child_node(&mount_name, root_inode)?;
    let mount_path = format!("/Volumes/{}", mount_name);

    MOUNT_TABLE.lock().add(
        part.name(),
        &mount_path,
        fs_type,
        if fs_type == "fat32" || fs_type == "exfat" { "rw" } else { "ro" },
    );

    kinfo!(
        "Auto-Mounted {} ({}) at {} [{}]",
        part.name(),
        fs_type.to_ascii_uppercase(),
        mount_path,
        volume_label
    );

    Ok(mount_path)
}

/// Automatically detect and mount all partitions on a newly attached block device.
pub fn mount_device(parent_dev: Arc<dyn BlockDevice>) -> usize {
    let partitions = scan_device_partitions(parent_dev);
    let mut mounted_count = 0;
    for part in partitions {
        if mount_partition_device(part).is_ok() {
            mounted_count += 1;
        }
    }
    mounted_count
}

/// Automatically detect and mount all physical and virtual storage partitions into /Volumes/
pub fn mount_all_storage() {
    let storage = STORAGE_MANAGER.lock();
    let num_devices = storage.devices.len();
    drop(storage);

    for dev_idx in 0..num_devices {
        let dev = {
            let s = STORAGE_MANAGER.lock();
            s.get_device(dev_idx)
        };

        if let Some(parent_dev) = dev {
            mount_device(parent_dev);
        }
    }
}
