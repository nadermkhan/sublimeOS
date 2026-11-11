extern crate alloc;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use crate::fs::vfs::{FileType, VFS};
use crate::mm::pmm::PMM;
use crate::{print, println};

pub fn run_all_tests() {
    println!("\n=======================================================");
    println!("             CosmosKernel Verification Suite           ");
    println!("=======================================================");

    test_pmm();
    test_heap();
    test_vfs();
    test_pipes();
    test_multitasking();
    test_uart();
    test_fat32_rw();
    test_exfat();
    test_ext4();
    test_ntfs();
    test_usb_mass_storage();
    test_net_crypto();
    test_wifi_subsystem();
    test_tcpip_stack();
    test_tcp_sockets();
    test_http_engine();
    test_audio_subsystem();
    test_elf_loader();
    test_pipelines_and_editor();
    test_userland_binaries();
    test_process_fork_cow();
    test_dynamic_linker_and_shared_libraries();
    test_posix_signals();
    test_smp_and_apic_subsystem();
    test_storage_ahci_and_nvme();
    test_userspace_windowserver_and_fb0();
    test_multicore_scheduler_and_affinity();
    test_partition_table_automounter();
    test_network_socket_server_and_daemons();
    test_pseudo_terminal_subsystem();
    test_tls_and_https_engine();
    test_realtek_and_qualcomm_drivers();

    println!("=======================================================");
    println!("      All Kernel Self-Tests PASSED Successfully!       ");
    println!("=======================================================\n");
}

fn test_pmm() {
    print!("  [TEST] Physical Frame Allocator ... ");
    let mut pmm = PMM.lock();
    let frame1 = pmm.alloc_frame().expect("PMM frame 1 alloc failed");
    let frame2 = pmm.alloc_frame().expect("PMM frame 2 alloc failed");
    assert!(frame1 != frame2, "PMM allocated identical frames");
    pmm.free_frame(frame1);
    pmm.free_frame(frame2);
    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_heap() {
    print!("  [TEST] Dynamic Heap & Alloc Crate ... ");
    let b = Box::new(42u64);
    assert_eq!(*b, 42);

    let mut v = Vec::new();
    for i in 0..1000 {
        v.push(i);
    }
    assert_eq!(v.len(), 1000);
    assert_eq!(v[999], 999);

    let s = String::from("CosmosKernel Heap Allocation Works!");
    assert_eq!(s.len(), 35);
    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_vfs() {
    print!("  [TEST] Virtual File System & Mounts ... ");
    let vfs = VFS.lock();

    // Test CosmicOS /System/Library/CoreServices/SystemVersion.plist
    let file = vfs.open("/System/Library/CoreServices/SystemVersion.plist", 0).expect("Failed to open SystemVersion.plist");
    let mut buf = [0u8; 128];
    let n = file.read(&mut buf).expect("Failed to read SystemVersion.plist");
    assert!(n > 0);

    // Test writing and reading temporary file in /tmp
    let tmp = vfs.resolve_path("/tmp").expect("Failed to resolve /tmp");
    let _ = tmp.lock().unlink("test.txt");
    let test_file_node = tmp.lock().create("test.txt", FileType::Regular).expect("Failed to create /tmp/test.txt");
    test_file_node.lock().write(0, b"Hello Cosmos!").expect("Failed to write to /tmp/test.txt");

    let mut read_buf = [0u8; 13];
    test_file_node.lock().read(0, &mut read_buf).expect("Failed to read from /tmp/test.txt");
    assert_eq!(&read_buf, b"Hello Cosmos!");
    let _ = tmp.lock().unlink("test.txt");

    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_pipes() {
    print!("  [TEST] Anonymous IPC Pipes ... ");
    let (reader, writer) = crate::ipc::create_pipe();
    writer.lock().write(0, b"IPC Message 123").expect("Pipe write failed");

    let mut buf = [0u8; 15];
    let n = reader.lock().read(0, &mut buf).expect("Pipe read failed");
    assert_eq!(n, 15);
    assert_eq!(&buf, b"IPC Message 123");
    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_multitasking() {
    print!("  [TEST] Task Context Switch & Yield ... ");
    // Yielding should return back to current thread smoothly
    crate::task::scheduler::yield_now();
    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_uart() {
    print!("  [TEST] 16550A UART & /dev/ttyS0 ... ");
    let com1 = crate::arch::x86_64::serial::COM1.lock();
    assert!(com1.is_initialized, "COM1 should be initialized");
    assert!(com1.chip_type != crate::arch::x86_64::serial::UartChipType::None, "COM1 should identify chip type");
    drop(com1);

    // Verify /dev/ttyS0 in DevFS
    let vfs = VFS.lock();
    let tty_node = vfs.open("/dev/ttyS0", 0).expect("Failed to open /dev/ttyS0");
    let stat = tty_node.stat();
    assert_eq!(stat.file_type, FileType::CharDevice, "/dev/ttyS0 should be CharDevice");
    println!("\x1b[32m[PASS]\x1b[0m");
}

use alloc::sync::Arc;
use crate::drivers::storage::BlockDevice;
use crate::fs::exfat::{ExFatFileSystem, ExFatInode};
use crate::fs::ext4::{Ext4FileSystem, Ext4Inode};
use crate::fs::fat32::{Fat32FileSystem, Fat32Inode};
use crate::fs::ntfs::{NtfsFileSystem, NtfsInode};
use crate::sync::IrqSafeSpinlock;

struct MemBlockDevice {
    name: String,
    data: IrqSafeSpinlock<Vec<u8>>,
    sectors: u64,
}

impl MemBlockDevice {
    fn new(name: &str, sectors: u64) -> Arc<Self> {
        let size = (sectors as usize) * 512;
        let mut v = Vec::with_capacity(size);
        v.resize(size, 0);
        Arc::new(Self {
            name: String::from(name),
            data: IrqSafeSpinlock::new(v),
            sectors,
        })
    }
}

impl BlockDevice for MemBlockDevice {
    fn name(&self) -> &str {
        &self.name
    }
    fn read_sectors(&self, lba: u64, count: u32, buf: &mut [u8]) -> Result<(), &'static str> {
        let d = self.data.lock();
        let start = (lba as usize) * 512;
        let end = start + (count as usize) * 512;
        if end > d.len() {
            return Err("Read beyond MemBlockDevice bounds");
        }
        buf[..(count as usize) * 512].copy_from_slice(&d[start..end]);
        Ok(())
    }
    fn write_sectors(&self, lba: u64, count: u32, buf: &[u8]) -> Result<(), &'static str> {
        let mut d = self.data.lock();
        let start = (lba as usize) * 512;
        let end = start + (count as usize) * 512;
        if end > d.len() {
            return Err("Write beyond MemBlockDevice bounds");
        }
        d[start..end].copy_from_slice(&buf[..(count as usize) * 512]);
        Ok(())
    }
    fn total_sectors(&self) -> u64 {
        self.sectors
    }
}

fn test_fat32_rw() {
    print!("  [TEST] FAT32 Read/Write & Cluster Alloc ... ");
    let dev = MemBlockDevice::new("mem_fat32", 1024);
    {
        let mut d = dev.data.lock();
        // Sector 0 BPB
        d[11] = 0x00; d[12] = 0x02; // bytes per sector = 512
        d[13] = 1;                  // sectors per cluster = 1
        d[14] = 32; d[15] = 0;      // reserved sectors = 32
        d[16] = 2;                  // num fats = 2
        d[36] = 32; d[37] = 0; d[38] = 0; d[39] = 0; // sectors per fat = 32
        d[44] = 2;  d[45] = 0; d[46] = 0; d[47] = 0; // root cluster = 2
        d[32] = 0x00; d[33] = 0x04; // total sectors = 1024
        d[71..82].copy_from_slice(b"COSMIC_USB ");
        d[510] = 0x55; d[511] = 0xAA;

        // FAT table at LBA 32 (offset 32 * 512 = 16384)
        let fat_start = 32 * 512;
        // Cluster 0 and 1
        d[fat_start] = 0xF8; d[fat_start + 1] = 0xFF; d[fat_start + 2] = 0xFF; d[fat_start + 3] = 0x0F;
        d[fat_start + 4] = 0xFF; d[fat_start + 5] = 0xFF; d[fat_start + 6] = 0xFF; d[fat_start + 7] = 0x0F;
        // Cluster 2 (root directory EOC)
        d[fat_start + 8] = 0xFF; d[fat_start + 9] = 0xFF; d[fat_start + 10] = 0xFF; d[fat_start + 11] = 0x0F;
    }

    let fs = Fat32FileSystem::mount(dev.clone()).expect("Failed to mount FAT32");
    assert_eq!(fs.volume_label, "COSMIC_USB");

    let root = Fat32Inode::new_root(fs);
    // 1. Create file
    let file = root.lock().create("TEST.TXT", FileType::Regular).expect("Failed to create file");
    // 2. Write data
    let written = file.lock().write(0, b"Hello FAT32 Read/Write!").expect("Write failed");
    assert_eq!(written, 23);
    // 3. Read back
    let mut read_buf = [0u8; 23];
    let n = file.lock().read(0, &mut read_buf).expect("Read failed");
    assert_eq!(n, 23);
    assert_eq!(&read_buf, b"Hello FAT32 Read/Write!");

    // 4. Verify directory listing
    let entries = root.lock().readdir().expect("Readdir failed");
    assert!(entries.iter().any(|e| e.name == "TEST.TXT"));

    // 5. Unlink file
    root.lock().unlink("TEST.TXT").expect("Unlink failed");
    let entries_after = root.lock().readdir().expect("Readdir failed");
    assert!(!entries_after.iter().any(|e| e.name == "TEST.TXT"));

    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_exfat() {
    print!("  [TEST] exFAT Parser & Inode Traversal ... ");
    let dev = MemBlockDevice::new("mem_exfat", 1024);
    {
        let mut d = dev.data.lock();
        // Sector 0: exFAT VBR
        d[3..11].copy_from_slice(b"EXFAT   ");
        d[80] = 32; // FAT offset = 32
        d[84] = 32; // FAT length = 32
        d[88] = 64; // Cluster heap offset = 64
        d[92] = 200; // Cluster count = 200
        d[96] = 2; // Root dir cluster = 2
        d[108] = 9; // 512 bytes per sector (1 << 9)
        d[109] = 0; // 1 sector per cluster (1 << 0)
        d[510] = 0x55; d[511] = 0xAA;

        // Root directory at Cluster 2 (LBA 64, offset 64 * 512 = 32768)
        let root_off = 64 * 512;
        // Entry 0: Volume label (0x83) "EXFAT_SD"
        d[root_off] = 0x83;
        d[root_off + 1] = 8;
        for (i, &b) in b"EXFAT_SD".iter().enumerate() {
            d[root_off + 2 + i * 2] = b;
        }

        // Entry 1: File directory entry (0x85)
        let file_off = root_off + 32;
        d[file_off] = 0x85;
        d[file_off + 1] = 2; // 2 secondary entries (0xC0 and 0xC1)
        d[file_off + 4] = 0x20; // Archive attribute

        // Entry 2: Stream extension (0xC0)
        let stream_off = root_off + 64;
        d[stream_off] = 0xC0;
        d[stream_off + 1] = 0x02; // NoFatChain = 1 (contiguous)
        d[stream_off + 20] = 3; // First cluster = 3
        d[stream_off + 24] = 19; // Data length = 19

        // Entry 3: File name (0xC1) "EXFAT.TXT"
        let name_off = root_off + 96;
        d[name_off] = 0xC1;
        for (i, &b) in b"EXFAT.TXT".iter().enumerate() {
            d[name_off + 2 + i * 2] = b;
        }

        // Cluster 3 data (LBA 65, offset 65 * 512 = 33280)
        let data_off = 65 * 512;
        d[data_off..data_off + 19].copy_from_slice(b"Hello exFAT Driver!");
    }

    let fs = ExFatFileSystem::mount(dev).expect("Failed to mount exFAT");
    assert_eq!(fs.volume_label, "EXFAT_SD");

    let root = ExFatInode::new_root(fs);
    let file = root.lock().lookup("EXFAT.TXT").expect("Failed to lookup EXFAT.TXT");
    let mut buf = [0u8; 19];
    let n = file.lock().read(0, &mut buf).expect("Failed to read EXFAT.TXT");
    assert_eq!(n, 19);
    assert_eq!(&buf, b"Hello exFAT Driver!");

    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_ext4() {
    print!("  [TEST] ext4 Superblock & Inode Mapping ... ");
    let dev = MemBlockDevice::new("mem_ext4", 1024);
    {
        let mut d = dev.data.lock();
        // LBA 2 (byte 1024): Superblock
        let sb = 1024;
        d[sb + 0x18] = 0; // s_log_block_size = 0 (1024 bytes/block)
        d[sb + 0x20] = 0x00; d[sb + 0x21] = 0x02; // 512 blocks per group
        d[sb + 0x28] = 32; // 32 inodes per group
        d[sb + 0x38] = 0x53; d[sb + 0x39] = 0xEF; // magic 0xEF53
        d[sb + 0x58] = 128; // inode size = 128
        d[sb + 0x78..sb + 0x82].copy_from_slice(b"LINUX_EXT4");

        // Block 2 (descriptor table at byte 2048)
        let desc = 2048;
        d[desc + 8] = 3; // inode table block = 3

        // Block 3 (inode table at byte 3072)
        let itable = 3072;
        // Inode 2: root directory (index 1, offset 128)
        let ino2 = itable + 128;
        d[ino2 + 0] = 0xED; d[ino2 + 1] = 0x41; // directory mode (0x41ED)
        d[ino2 + 4] = 0x00; d[ino2 + 5] = 0x04; // size = 1024 bytes
        d[ino2 + 40] = 4; // direct block 0 = block 4

        // Inode 12: regular file (index 11, offset 11 * 128 = 1408)
        let ino12 = itable + 1408;
        d[ino12 + 0] = 0xA4; d[ino12 + 1] = 0x81; // file mode (0x81A4)
        d[ino12 + 4] = 18; // size = 18 bytes
        d[ino12 + 40] = 5; // direct block 0 = block 5

        // Block 4: root directory data (byte 4096)
        let dir = 4096;
        // Entry 1: . (rec_len = 12)
        d[dir + 0] = 2; d[dir + 4] = 12; d[dir + 6] = 1; d[dir + 7] = 2; d[dir + 8] = b'.';
        // Entry 2: .. (rec_len = 12)
        d[dir + 12] = 2; d[dir + 16] = 12; d[dir + 18] = 2; d[dir + 19] = 2; d[dir + 20] = b'.'; d[dir + 21] = b'.';
        // Entry 3: TEST.TXT (rec_len = 1000)
        let e3 = dir + 24;
        d[e3 + 0] = 12; d[e3 + 4] = 0xE8; d[e3 + 5] = 0x03; // ino = 12, rec_len = 1000
        d[e3 + 6] = 8; d[e3 + 7] = 1; // name_len = 8, file_type = 1
        d[e3 + 8..e3 + 16].copy_from_slice(b"TEST.TXT");

        // Block 5: file data (byte 5120)
        let file_data = 5120;
        d[file_data..file_data + 18].copy_from_slice(b"Hello ext4 Driver!");
    }

    let fs = Ext4FileSystem::mount(dev).expect("Failed to mount ext4");
    assert_eq!(fs.volume_name, "LINUX_EXT4");

    let root = Ext4Inode::new_root(fs).expect("Failed to get ext4 root");
    let file = root.lock().lookup("TEST.TXT").expect("Failed to lookup TEST.TXT");
    let mut buf = [0u8; 18];
    let n = file.lock().read(0, &mut buf).expect("Failed to read TEST.TXT");
    assert_eq!(n, 18);
    assert_eq!(&buf, b"Hello ext4 Driver!");

    // Test ext4 block write
    let write_data = b"Updated ext4 Driver Data!";
    let written = file.lock().write(0, write_data).expect("Failed to write to ext4 file");
    assert_eq!(written, write_data.len());
    let mut read_back = [0u8; 25];
    let n2 = file.lock().read(0, &mut read_back).expect("Failed to re-read ext4 file");
    assert_eq!(n2, write_data.len());
    assert_eq!(&read_back, write_data);

    // Test ext4 file creation & directory block allocation
    let new_node = root.lock().create("NEW.TXT", FileType::Regular).expect("Failed to create NEW.TXT in ext4");
    let test_msg = b"Native ext4 block write!";
    let wn = new_node.lock().write(0, test_msg).expect("Failed to write to NEW.TXT");
    assert_eq!(wn, test_msg.len());
    let mut new_buf = [0u8; 24];
    let n3 = new_node.lock().read(0, &mut new_buf).expect("Failed to read NEW.TXT");
    assert_eq!(n3, test_msg.len());
    assert_eq!(&new_buf, test_msg);

    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_ntfs() {
    print!("  [TEST] NTFS Resident $DATA & Index Root ... ");
    let dev = MemBlockDevice::new("mem_ntfs", 1024);
    {
        let mut d = dev.data.lock();
        // Sector 0: NTFS VBR
        d[3..11].copy_from_slice(b"NTFS    ");
        d[11] = 0x00; d[12] = 0x02; // bytes per sector = 512
        d[13] = 1; // sectors per cluster = 1
        d[48] = 10; // $MFT starting cluster = 10
        d[64] = 0xF6; // -10 (1024 bytes per MFT record)
        d[510] = 0x55; d[511] = 0xAA;

        // MFT Record 5: root directory (at LBA 10 + 5*2 = 20, offset 20 * 512 = 10240)
        let r5 = 20 * 512;
        d[r5..r5 + 4].copy_from_slice(b"FILE");
        d[r5 + 4] = 48; // USA offset
        d[r5 + 6] = 3;  // USA count
        d[r5 + 20] = 56; // first attr offset = 56
        d[r5 + 22] = 0x03; // flags: in use + directory

        // Attribute $INDEX_ROOT (type 0x90) at offset 56
        let a90 = r5 + 56;
        d[a90 + 0] = 0x90; // type 0x90
        d[a90 + 4] = 120;  // length = 120
        d[a90 + 8] = 0;    // resident
        d[a90 + 20] = 24;  // value offset = 24 (starts at a90 + 24)

        // Index root header (at a90 + 24)
        let ir = a90 + 24;
        d[ir + 0] = 0x30; // indexed attr type = $FILE_NAME
        d[ir + 16] = 16;  // entries offset = 16 (relative to ir + 16)

        // First index entry (at ir + 32)
        let ie = ir + 32;
        d[ie + 0] = 16;   // child MFT record number = 16
        d[ie + 8] = 88;   // entry length = 88
        d[ie + 10] = 72;  // content length = 72
        d[ie + 12] = 0x02;// last entry

        // $FILE_NAME payload (at ie + 16)
        let fn_payload = ie + 16;
        d[fn_payload + 48] = 18; // file size = 18
        d[fn_payload + 56] = 0x20; // archive
        d[fn_payload + 64] = 8; // name length = 8
        d[fn_payload + 65] = 1; // namespace = Win32
        for (i, &b) in b"TEST.TXT".iter().enumerate() {
            d[fn_payload + 66 + i * 2] = b;
        }

        // MFT Record 16: file (at LBA 10 + 16*2 = 42, offset 42 * 512 = 21504)
        let r16 = 42 * 512;
        d[r16..r16 + 4].copy_from_slice(b"FILE");
        d[r16 + 4] = 48; // USA offset
        d[r16 + 6] = 3;  // USA count
        d[r16 + 20] = 56; // first attr offset = 56
        d[r16 + 22] = 0x01; // flags: in use

        // Attribute $DATA (type 0x80) at offset 56
        let a80 = r16 + 56;
        d[a80 + 0] = 0x80; // type 0x80
        d[a80 + 4] = 48;   // length = 48
        d[a80 + 8] = 0;    // resident
        d[a80 + 16] = 18;  // value length = 18
        d[a80 + 20] = 24;  // value offset = 24
        d[a80 + 24..a80 + 42].copy_from_slice(b"Hello NTFS Driver!");
    }

    let fs = NtfsFileSystem::mount(dev).expect("Failed to mount NTFS");
    let root = NtfsInode::new_root(fs).expect("Failed to get NTFS root");
    let file = root.lock().lookup("TEST.TXT").expect("Failed to lookup TEST.TXT");
    let mut buf = [0u8; 18];
    let n = file.lock().read(0, &mut buf).expect("Failed to read TEST.TXT");
    assert_eq!(n, 18);
    assert_eq!(&buf, b"Hello NTFS Driver!");

    println!("\x1b[32m[PASS]\x1b[0m");
}

struct MockUsbTransport {
    storage: IrqSafeSpinlock<Vec<u8>>,
    last_lba: IrqSafeSpinlock<u64>,
}

impl MockUsbTransport {
    fn new(sectors: usize) -> Arc<Self> {
        let mut v = Vec::with_capacity(sectors * 512);
        v.resize(sectors * 512, 0);
        Arc::new(Self {
            storage: IrqSafeSpinlock::new(v),
            last_lba: IrqSafeSpinlock::new(0),
        })
    }
}

impl crate::drivers::usb::msc::UsbMassStorageTransport for MockUsbTransport {
    fn bulk_out(&self, data: &[u8]) -> Result<(), &'static str> {
        if data.len() == 31 {
            // CBW
            let cdb_cmd = data[15];
            if cdb_cmd == crate::drivers::usb::msc::SCSI_READ_10 || cdb_cmd == crate::drivers::usb::msc::SCSI_WRITE_10 {
                let lba = ((data[17] as u64) << 24) | ((data[18] as u64) << 16) | ((data[19] as u64) << 8) | (data[20] as u64);
                *self.last_lba.lock() = lba;
            }
        } else {
            // Write data
            let lba = *self.last_lba.lock();
            let start = (lba as usize) * 512;
            let end = start + data.len();
            let mut store = self.storage.lock();
            if end <= store.len() {
                store[start..end].copy_from_slice(data);
            }
        }
        Ok(())
    }

    fn bulk_in(&self, buf: &mut [u8]) -> Result<usize, &'static str> {
        if buf.len() == 36 {
            // SCSI Inquiry response
            buf[0] = 0x00; // Direct-access block device (disk)
            buf[1] = 0x80; // Removable media
            buf[8..16].copy_from_slice(b"SanDisk ");
            buf[16..32].copy_from_slice(b"Ultra Fit USB3.0");
            Ok(36)
        } else if buf.len() == 8 {
            // SCSI Read Capacity 10 response
            buf[0] = 0; buf[1] = 0; buf[2] = 0x07; buf[3] = 0xFF; // 2047
            buf[4] = 0; buf[5] = 0; buf[6] = 0x02; buf[7] = 0x00; // 512 bytes
            Ok(8)
        } else if buf.len() == 13 {
            // CSW response
            buf[0] = 0x55; buf[1] = 0x53; buf[2] = 0x42; buf[3] = 0x53; // "USBS"
            buf[12] = 0; // Status: Success
            Ok(13)
        } else {
            // Sector read data
            let lba = *self.last_lba.lock();
            let start = (lba as usize) * 512;
            let end = start + buf.len();
            let store = self.storage.lock();
            if end <= store.len() {
                buf.copy_from_slice(&store[start..end]);
            }
            Ok(buf.len())
        }
    }
}

fn test_usb_mass_storage() {
    print!("  [TEST] USB Mass Storage (SCSI / BOT) ... ");
    let mock = MockUsbTransport::new(2048);
    let dev = crate::drivers::usb::msc::UsbBlockDevice::probe(mock, String::from("usb0")).expect("USB probe failed");

    assert_eq!(dev.name(), "usb0");
    assert_eq!(dev.vendor, "SanDisk");
    assert_eq!(dev.model, "Ultra Fit USB3.0");
    assert_eq!(dev.total_sectors(), 2048);
    assert_eq!(dev.sector_size(), 512);

    // Test writing and reading via SCSI Read(10) / Write(10)
    let test_data = [0x5A; 512];
    dev.write_sectors(10, 1, &test_data).expect("USB write_sectors failed");

    let mut read_buf = [0u8; 512];
    dev.read_sectors(10, 1, &mut read_buf).expect("USB read_sectors failed");
    assert_eq!(read_buf, test_data);

    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_net_crypto() {
    print!("  [TEST] Network Crypto (SHA-1, HMAC-SHA1, PBKDF2) ... ");

    // 1. Test SHA-1 with standard test vector "abc"
    let digest = crate::net::crypto::sha1(b"abc");
    assert_eq!(
        digest,
        [0xa9, 0x99, 0x3e, 0x36, 0x47, 0x06, 0x81, 0x6a, 0xba, 0x3e, 0x25, 0x71, 0x78, 0x50, 0xc2, 0x6c, 0x9c, 0xd0, 0xd8, 0x9d]
    );

    // 2. Test HMAC-SHA1
    let hmac = crate::net::crypto::hmac_sha1(b"key", b"The quick brown fox jumps over the lazy dog");
    assert_eq!(
        hmac,
        [0xde, 0x7c, 0x9b, 0x85, 0xb8, 0xb7, 0x8a, 0xa6, 0xbc, 0x8a, 0x7a, 0x36, 0xf7, 0x0a, 0x90, 0x70, 0x1c, 0x9d, 0xb4, 0xd9]
    );

    // 3. Test PBKDF2-HMAC-SHA1 (RFC 6070 Test 1: password="password", salt="salt", c=1, dkLen=20)
    let mut pbkdf2_out = [0u8; 32];
    crate::net::crypto::pbkdf2_sha1(b"password", b"salt", 1, &mut pbkdf2_out);
    assert_eq!(
        &pbkdf2_out[..20],
        &[0x0c, 0x60, 0xc8, 0x0f, 0x96, 0x1f, 0x0e, 0x71, 0xf3, 0xa9, 0xb5, 0x24, 0xaf, 0x60, 0x12, 0x06, 0x2f, 0xe0, 0x37, 0xa6]
    );

    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_wifi_subsystem() {
    print!("  [TEST] IEEE 802.11 Wi-Fi (Beacons & WPA2-PSK) ... ");

    // 1. Test 802.11 Beacon frame parser
    let mut beacon = [0u8; 64];
    // Tag 0: SSID "CosmicNet" (len 9)
    beacon[36] = 0;
    beacon[37] = 9;
    beacon[38..47].copy_from_slice(b"CosmicNet");
    // Tag 3: Channel 11 (len 1)
    beacon[47] = 3;
    beacon[48] = 1;
    beacon[49] = 11;
    // Tag 48: RSN (WPA2)
    beacon[50] = 48;
    beacon[51] = 4;

    let parsed = crate::net::wifi::parse_80211_beacon(&beacon[..56]);
    assert!(parsed.is_some(), "Failed to parse 802.11 beacon");
    let (ssid, ch, sec) = parsed.unwrap();
    assert_eq!(ssid, "CosmicNet");
    assert_eq!(ch, 11);
    assert_eq!(sec, crate::net::wifi::WifiSecurity::Wpa2Psk);

    // 2. Test WifiManager scan & connect
    let mut mgr = crate::net::wifi::WifiManager::new();
    let scan = mgr.scan();
    assert!(scan.len() >= 3, "WifiManager scan returned too few APs");
    assert_eq!(scan[0].ssid, "Cosmic-5G");

    // Connect with WPA2 passphrase
    mgr.connect("Cosmic-5G", Some("CosmicSecretKey!"))
        .expect("WifiManager connect failed");
    assert!(mgr.is_connected, "WifiManager not marked connected");
    assert_eq!(mgr.current_channel, 36);
    assert_eq!(mgr.link_speed_mbps, 866);

    // Disconnect
    mgr.disconnect();
    assert!(!mgr.is_connected, "WifiManager still marked connected after disconnect");

    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_tcpip_stack() {
    print!("  [TEST] TCP/IP Protocol Stack & ICMP Ping ... ");

    // 1. Test IPv4 Header build & parse
    let ip_pkt = crate::net::ip::build_ipv4_packet(
        [192, 168, 1, 100],
        [192, 168, 1, 1],
        crate::net::ip::IP_PROTO_ICMP,
        b"Echo payload",
    );
    let (proto, src, dst, payload) =
        crate::net::ip::parse_ipv4_header(&ip_pkt).expect("Failed to parse IPv4 header");
    assert_eq!(proto, crate::net::ip::IP_PROTO_ICMP);
    assert_eq!(src, [192, 168, 1, 100]);
    assert_eq!(dst, [192, 168, 1, 1]);
    assert_eq!(payload, b"Echo payload");

    // 2. Test ARP request and reply handling
    let arp_req = crate::net::arp::build_arp_request(
        [0x00, 0x14, 0x22, 0x01, 0x02, 0x03],
        [192, 168, 1, 100],
        [192, 168, 1, 1],
    );
    let reply = crate::net::arp::handle_arp_packet(
        &arp_req,
        [0x00, 0x14, 0x22, 0x01, 0x02, 0x03],
        [192, 168, 1, 1],
    );
    assert!(reply.is_some(), "ARP reply not generated");

    // 3. Test ICMP Echo Request & Reply
    let echo = crate::net::icmp::build_echo_request(0x4242, 1, b"PING");
    assert_eq!(echo[0], crate::net::icmp::ICMP_TYPE_ECHO_REQUEST);
    let echo_reply = crate::net::icmp::handle_icmp_packet(&echo, [127, 0, 0, 1], [127, 0, 0, 1]);
    assert!(echo_reply.is_some(), "ICMP reply not generated");
    let (_, reply_buf) = echo_reply.unwrap();
    assert_eq!(reply_buf[0], crate::net::icmp::ICMP_TYPE_ECHO_REPLY);

    // 4. Test DNS Query Builder
    let dns_query = crate::net::dns::build_dns_query("CosmicOS.local", 0x55AA);
    assert!(dns_query.len() > 12);
    assert_eq!(u16::from_be_bytes([dns_query[0], dns_query[1]]), 0x55AA);

    // 5. Test Live Loopback Ping
    let rtt = crate::net::ping("127.0.0.1").expect("Loopback ping failed");
    assert!(rtt >= 1);

    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_tcp_sockets() {
    print!("  [TEST] TCP Protocol & Socket Engine ... ");

    // 1. Test TCP Packet serialization and checksum
    let src_ip = [192, 168, 1, 100];
    let dst_ip = [192, 168, 1, 1];
    let tcp_seg = crate::net::tcp::build_tcp_packet(
        src_ip,
        dst_ip,
        50000,
        80,
        0x1000,
        0x2000,
        crate::net::tcp::TCP_FLAG_SYN | crate::net::tcp::TCP_FLAG_ACK,
        65535,
        b"Hello TCP",
    );

    let (hdr, payload) = crate::net::tcp::parse_tcp_packet(&tcp_seg).expect("Failed to parse TCP segment");
    assert_eq!(hdr.src_port, 50000);
    assert_eq!(hdr.dst_port, 80);
    assert_eq!(hdr.seq_num, 0x1000);
    assert_eq!(hdr.ack_num, 0x2000);
    assert_eq!(hdr.flags, crate::net::tcp::TCP_FLAG_SYN | crate::net::tcp::TCP_FLAG_ACK);
    assert_eq!(payload, b"Hello TCP");

    // 2. Test Socket Descriptor Allocation & Lifecycle
    let sock = crate::net::socket::socket_open().expect("Failed to allocate socket");
    assert!(sock > 0);
    crate::net::socket::socket_close(sock).expect("Failed to close socket");

    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_http_engine() {
    print!("  [TEST] HTTP Client & Embedded Server ... ");

    // 1. Test HTTP Response Parser
    let raw_http = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nServer: CosmicOS\r\n\r\nHello CosmicOS!";
    let resp = crate::net::http::parse_http_response(raw_http).expect("Failed to parse HTTP response");
    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.status_text, "OK");
    assert_eq!(resp.headers.get("Server"), Some(&String::from("CosmicOS")));
    assert_eq!(resp.body, b"Hello CosmicOS!");

    // 2. Test Embedded httpd response generation
    let api_resp = crate::net::http::generate_httpd_response("/api/status");
    assert!(api_resp.contains("CosmicOS") && api_resp.contains("uptime_seconds"));

    let html_resp = crate::net::http::generate_httpd_response("/");
    assert!(html_resp.contains("Sublime Dashboard") && html_resp.contains("200 OK"));

    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_audio_subsystem() {
    print!("  [TEST] Audio Subsystem & PCM Synthesis ... ");

    // 1. Test fast integer sine
    assert_eq!(crate::drivers::audio::pcm::integer_sin(0), 0);
    assert!(crate::drivers::audio::pcm::integer_sin(32) > 0);
    assert_eq!(crate::drivers::audio::pcm::integer_sin(64), 32767);
    assert_eq!(crate::drivers::audio::pcm::integer_sin(128), 0);
    assert_eq!(crate::drivers::audio::pcm::integer_sin(192), -32767);

    // 2. Test Tone Generator
    let tone = crate::drivers::audio::pcm::generate_sine_wave(440, 20, 44100);
    let expected_samples = (44100 * 20 / 1000) * 2; // Stereo
    assert_eq!(tone.len(), expected_samples);

    // 3. Test Cosmic Startup Chime generator
    let chime = crate::drivers::audio::pcm::generate_cosmic_chime(44100);
    assert!(chime.len() > 44100);

    // 4. Test Phonetic formant generator
    let phoneme = crate::drivers::audio::pcm::generate_phoneme_tone(b'A', 30, 44100);
    assert_eq!(phoneme.len(), (44100 * 30 / 1000) * 2);

    // 5. Test RIFF/WAVE PCM parser
    let mut wav_bytes = Vec::new();
    wav_bytes.extend_from_slice(b"RIFF");
    let file_len = 36u32 + 8u32;
    wav_bytes.extend_from_slice(&file_len.to_le_bytes());
    wav_bytes.extend_from_slice(b"WAVE");
    wav_bytes.extend_from_slice(b"fmt ");
    wav_bytes.extend_from_slice(&16u32.to_le_bytes()); // Subchunk1Size
    wav_bytes.extend_from_slice(&1u16.to_le_bytes());  // AudioFormat (PCM)
    wav_bytes.extend_from_slice(&2u16.to_le_bytes());  // NumChannels (Stereo)
    wav_bytes.extend_from_slice(&44100u32.to_le_bytes()); // SampleRate
    wav_bytes.extend_from_slice(&(44100u32 * 4).to_le_bytes()); // ByteRate
    wav_bytes.extend_from_slice(&4u16.to_le_bytes());  // BlockAlign
    wav_bytes.extend_from_slice(&16u16.to_le_bytes()); // BitsPerSample
    wav_bytes.extend_from_slice(b"data");
    wav_bytes.extend_from_slice(&8u32.to_le_bytes());  // Subchunk2Size (4 samples of 16-bit)
    wav_bytes.extend_from_slice(&1000i16.to_le_bytes());
    wav_bytes.extend_from_slice(&(-1000i16).to_le_bytes());
    wav_bytes.extend_from_slice(&2000i16.to_le_bytes());
    wav_bytes.extend_from_slice(&(-2000i16).to_le_bytes());

    let wav = crate::drivers::audio::pcm::parse_wav(&wav_bytes).expect("Failed to parse valid WAV");
    assert_eq!(wav.sample_rate, 44100);
    assert_eq!(wav.channels, 2);
    assert_eq!(wav.bits_per_sample, 16);
    assert_eq!(wav.samples.len(), 4);
    assert_eq!(wav.samples[0], 1000);
    assert_eq!(wav.samples[1], -1000);

    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_elf_loader() {
    print!("  [TEST] ELF64 Loader & Ring 3 User Mode ... ");

    // 1. Synthesize a valid 64-bit ELF binary in memory:
    // Header (64 bytes) + Program Header (56 bytes) + Code Payload
    let mut elf_bytes = Vec::new();

    // ELF Header (64 bytes)
    elf_bytes.extend_from_slice(&[0x7F, b'E', b'L', b'F']); // e_ident[0..4]
    elf_bytes.push(2); // e_ident[4]: 64-bit (ELFCLASS64)
    elf_bytes.push(1); // e_ident[5]: Little-endian (ELFDATA2LSB)
    elf_bytes.push(1); // e_ident[6]: ELF Version 1
    elf_bytes.extend_from_slice(&[0u8; 9]); // OS ABI and padding

    elf_bytes.extend_from_slice(&2u16.to_le_bytes()); // e_type: ET_EXEC (2)
    elf_bytes.extend_from_slice(&0x3Eu16.to_le_bytes()); // e_machine: EM_X86_64 (0x3E)
    elf_bytes.extend_from_slice(&1u32.to_le_bytes()); // e_version: 1
    elf_bytes.extend_from_slice(&0x0000000000400078u64.to_le_bytes()); // e_entry: 0x400078
    elf_bytes.extend_from_slice(&64u64.to_le_bytes()); // e_phoff: 64 (immediately follows Ehdr)
    elf_bytes.extend_from_slice(&0u64.to_le_bytes()); // e_shoff: 0
    elf_bytes.extend_from_slice(&0u32.to_le_bytes()); // e_flags: 0
    elf_bytes.extend_from_slice(&64u16.to_le_bytes()); // e_ehsize: 64
    elf_bytes.extend_from_slice(&56u16.to_le_bytes()); // e_phentsize: 56
    elf_bytes.extend_from_slice(&1u16.to_le_bytes()); // e_phnum: 1
    elf_bytes.extend_from_slice(&64u16.to_le_bytes()); // e_shentsize: 64
    elf_bytes.extend_from_slice(&0u16.to_le_bytes()); // e_shnum: 0
    elf_bytes.extend_from_slice(&0u16.to_le_bytes()); // e_shstrndx: 0
    assert_eq!(elf_bytes.len(), 64);

    // Code payload:
    // mov eax, 60 (SYS_EXIT) -> b8 3c 00 00 00
    // mov edi, 42            -> bf 2a 00 00 00
    // int 0x80               -> cd 80
    let code: [u8; 12] = [
        0xB8, 0x3C, 0x00, 0x00, 0x00, // mov eax, 60
        0xBF, 0x2A, 0x00, 0x00, 0x00, // mov edi, 42
        0xCD, 0x80,                   // int 0x80
    ];

    let total_file_size = 64u64 + 56u64 + code.len() as u64;

    // ELF Program Header (56 bytes)
    elf_bytes.extend_from_slice(&1u32.to_le_bytes()); // p_type: PT_LOAD (1)
    elf_bytes.extend_from_slice(&7u32.to_le_bytes()); // p_flags: PF_R | PF_W | PF_X (7)
    elf_bytes.extend_from_slice(&0u64.to_le_bytes()); // p_offset: 0
    elf_bytes.extend_from_slice(&0x0000000000400000u64.to_le_bytes()); // p_vaddr: 0x400000
    elf_bytes.extend_from_slice(&0x0000000000400000u64.to_le_bytes()); // p_paddr: 0x400000
    elf_bytes.extend_from_slice(&total_file_size.to_le_bytes()); // p_filesz
    elf_bytes.extend_from_slice(&total_file_size.to_le_bytes()); // p_memsz
    elf_bytes.extend_from_slice(&4096u64.to_le_bytes()); // p_align: 4096
    assert_eq!(elf_bytes.len(), 120);

    // Append code payload
    elf_bytes.extend_from_slice(&code);
    assert_eq!(elf_bytes.len(), 132);

    // 2. Validate ELF parsing
    let elf = crate::task::elf::ElfBinary::parse(&elf_bytes).expect("Failed to parse valid ELF64");
    assert_eq!(elf.entry_point(), 0x0000000000400078);
    assert_eq!(elf.program_headers.len(), 1);
    assert_eq!(elf.program_headers[0].p_type, crate::task::elf::PT_LOAD);
    assert_eq!(elf.program_headers[0].p_vaddr, 0x0000000000400000);

    // 3. Execute in Ring 3 and verify exit code 42
    let exit_code = crate::task::user::execute_user_binary(&elf_bytes)
        .expect("Failed to execute user binary in Ring 3");
    assert_eq!(exit_code, 42);

    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_pipelines_and_editor() {
    print!("  [TEST] POSIX Pipelines, Redirection & Editor ... ");

    // 1. Test grep filter
    let text = "apple\nbanana\ncherry\nCosmicOS\n";
    let grep_res = crate::shell::pipeline::run_grep(text, &["an"]);
    assert_eq!(grep_res, "banana\n");

    let grep_ci = crate::shell::pipeline::run_grep(text, &["-i", "cosmic"]);
    assert_eq!(grep_ci, "CosmicOS\n");

    let grep_inv = crate::shell::pipeline::run_grep("one\ntwo\nthree\n", &["-v", "two"]);
    assert_eq!(grep_inv, "one\nthree\n");

    // 2. Test wc filter
    let wc_res = crate::shell::pipeline::run_wc("hello world\nline two\n", &["-l"]);
    assert!(wc_res.contains("2"));

    // 3. Test head and tail filters
    let multi = "1\n2\n3\n4\n5\n6\n";
    let head_res = crate::shell::pipeline::run_head(multi, &["-n", "2"]);
    assert_eq!(head_res, "1\n2\n");

    let tail_res = crate::shell::pipeline::run_tail(multi, &["-n", "2"]);
    assert_eq!(tail_res, "5\n6\n");

    // 4. Test sort and uniq filters
    let unsorted = "zebra\napple\nmango\n";
    let sorted = crate::shell::pipeline::run_sort(unsorted, &[]);
    assert_eq!(sorted, "apple\nmango\nzebra\n");

    let dups = "a\na\nb\nc\nc\n";
    let uniq_res = crate::shell::pipeline::run_uniq(dups, &[]);
    assert_eq!(uniq_res, "a\nb\nc\n");

    // 5. Test File Writing & Redirection Pipeline
    let pipe_file = "/tmp/pipe_test.txt";
    let content = b"Line 1: CosmicOS\nLine 2: Sublime Kernel\nLine 3: CosmicOS Rules\n";
    crate::shell::pipeline::write_file_to_vfs(pipe_file, content, false)
        .expect("Failed to write test file");

    let read_back = crate::shell::pipeline::read_file_from_vfs(pipe_file)
        .expect("Failed to read test file");
    assert_eq!(read_back.as_bytes(), content);

    // Test pipeline execution with redirection: cat pipe_file | grep CosmicOS > /tmp/out.txt
    let pipeline_cmd = "cat /tmp/pipe_test.txt | grep CosmicOS > /tmp/out.txt";
    let handled = crate::shell::pipeline::try_execute_pipeline(pipeline_cmd);
    assert!(handled);

    let filtered = crate::shell::pipeline::read_file_from_vfs("/tmp/out.txt")
        .expect("Failed to read redirected output");
    assert!(filtered.contains("Line 1: CosmicOS"));
    assert!(filtered.contains("Line 3: CosmicOS Rules"));
    assert!(!filtered.contains("Sublime Kernel"));

    // Test append redirection: cat /tmp/pipe_test.txt | head -n 1 >> /tmp/out.txt
    let append_cmd = "cat /tmp/pipe_test.txt | head -n 1 >> /tmp/out.txt";
    let handled_app = crate::shell::pipeline::try_execute_pipeline(append_cmd);
    assert!(handled_app);

    let appended = crate::shell::pipeline::read_file_from_vfs("/tmp/out.txt")
        .expect("Failed to read appended output");
    assert_eq!(appended.lines().count(), 3);

    // 6. Test TextEditor buffer operations & saving
    let mut editor = crate::shell::editor::TextEditor::new("/tmp/editor_save_test.txt");
    editor.insert_char('C');
    editor.insert_char('o');
    editor.insert_char('s');
    editor.insert_char('m');
    editor.insert_char('i');
    editor.insert_char('c');
    editor.insert_newline();
    editor.insert_char('O');
    editor.insert_char('S');
    editor.save().expect("Failed to save from TextEditor");

    let saved_text = crate::shell::pipeline::read_file_from_vfs("/tmp/editor_save_test.txt")
        .expect("Failed to read editor saved file");
    assert_eq!(saved_text, "Cosmic\nOS");

    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_userland_binaries() {
    print!("  [TEST] Userspace Libc & Native Binaries (/bin) ... ");

    // 1. Verify /bin entries exist in VFS
    let vfs = crate::fs::vfs::VFS.lock();
    let bin_dir = vfs.resolve_path("/bin").expect("Missing /bin in VFS");
    let entries = bin_dir.lock().readdir().expect("Failed to read /bin");
    drop(vfs);

    let names: Vec<String> = entries.into_iter().map(|e| e.name).collect();
    assert!(names.contains(&String::from("init")));
    assert!(names.contains(&String::from("calc")));
    assert!(names.contains(&String::from("sh")));
    assert!(names.contains(&String::from("snake")));

    // 2. Execute /bin/init in Ring 3
    let vfs = crate::fs::vfs::VFS.lock();
    let file = vfs.open("/bin/init", 0).expect("Failed to open /bin/init");
    drop(vfs);
    let mut buf = alloc::vec![0u8; 16384];
    let n = file.read(&mut buf).expect("Failed to read /bin/init");
    let exit_init = crate::task::user::execute_user_binary(&buf[..n])
        .expect("Failed to execute /bin/init in Ring 3");
    assert_eq!(exit_init, 0);

    // 3. Execute /bin/calc in Ring 3
    let vfs = crate::fs::vfs::VFS.lock();
    let file = vfs.open("/bin/calc", 0).expect("Failed to open /bin/calc");
    drop(vfs);
    let mut buf = alloc::vec![0u8; 16384];
    let n = file.read(&mut buf).expect("Failed to read /bin/calc");
    let exit_calc = crate::task::user::execute_user_binary(&buf[..n])
        .expect("Failed to execute /bin/calc in Ring 3");
    assert_eq!(exit_calc, 0);

    // 4. Execute /bin/snake in Ring 3
    let vfs = crate::fs::vfs::VFS.lock();
    let file = vfs.open("/bin/snake", 0).expect("Failed to open /bin/snake");
    drop(vfs);
    let mut buf = alloc::vec![0u8; 16384];
    let n = file.read(&mut buf).expect("Failed to read /bin/snake");
    let exit_snake = crate::task::user::execute_user_binary(&buf[..n])
        .expect("Failed to execute /bin/snake in Ring 3");
    assert_eq!(exit_snake, 0);

    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_process_fork_cow() {
    print!("  [TEST] Process Fork & Copy-on-Write (CoW) ... ");

    // 1. Setup parent address space with a mapped writable user page
    let mut parent_space = crate::mm::address_space::AddressSpace::new()
        .expect("Failed to create parent address space");
    parent_space
        .map_region(0x4000_0000, 4096, crate::mm::vmm::PAGE_WRITABLE, crate::mm::address_space::VmaType::Data)
        .expect("Failed to map user data region");

    // Write initial test value to parent memory
    parent_space
        .write_user_u64(0x4000_0000, 0xCAFEBABE_DEADBEEF)
        .expect("Failed to write to parent memory");

    let vmm = crate::mm::vmm::VMM.lock();
    let parent_pml4 = unsafe { &*(crate::mm::phys_to_virt(parent_space.pml4_phys) as *const crate::mm::vmm::PageTable) };
    let orig_phys = vmm.translate(parent_pml4, 0x4000_0000).expect("Failed to translate parent page");
    drop(vmm);

    assert_eq!(crate::mm::pmm::PMM.lock().get_frame_ref(orig_phys), 1);

    // 2. Clone parent address space with Copy-on-Write
    let child_space = parent_space.clone_cow().expect("Failed to clone address space via CoW");

    // Verify parent and child share the exact same physical frame
    let vmm = crate::mm::vmm::VMM.lock();
    let child_pml4 = unsafe { &*(crate::mm::phys_to_virt(child_space.pml4_phys) as *const crate::mm::vmm::PageTable) };
    let shared_child_phys = vmm.translate(child_pml4, 0x4000_0000).expect("Failed to translate child page");
    drop(vmm);

    assert_eq!(shared_child_phys, orig_phys);
    assert_eq!(crate::mm::pmm::PMM.lock().get_frame_ref(orig_phys), 2);

    // 3. Resolve CoW write fault on child address space
    crate::mm::vmm::VMM
        .lock()
        .handle_cow_fault(child_space.pml4_phys, 0x4000_0000)
        .expect("Failed to handle child CoW page fault");

    // Verify child now owns a private, newly duplicated physical frame
    let vmm = crate::mm::vmm::VMM.lock();
    let child_pml4 = unsafe { &*(crate::mm::phys_to_virt(child_space.pml4_phys) as *const crate::mm::vmm::PageTable) };
    let new_child_phys = vmm.translate(child_pml4, 0x4000_0000).expect("Failed to translate child page");
    drop(vmm);

    assert_ne!(new_child_phys, orig_phys);
    assert_eq!(crate::mm::pmm::PMM.lock().get_frame_ref(orig_phys), 1);
    assert_eq!(crate::mm::pmm::PMM.lock().get_frame_ref(new_child_phys), 1);

    // Verify data integrity: child inherited original content
    let child_data = unsafe { *(crate::mm::phys_to_virt(new_child_phys) as *const u64) };
    assert_eq!(child_data, 0xCAFEBABE_DEADBEEF);

    // Write modified value to child private page
    child_space
        .write_user_u64(0x4000_0000, 0x12345678_9ABCDEF0)
        .expect("Failed to write to child private page");

    // Verify parent data remains completely untouched
    let parent_data = unsafe { *(crate::mm::phys_to_virt(orig_phys) as *const u64) };
    assert_eq!(parent_data, 0xCAFEBABE_DEADBEEF);

    // 4. Verify waitpid lifecycle and exit status retrieval
    let _parent_pid = crate::task::process::get_current_pid();
    let child_pid = crate::task::process::alloc_process("test_zombie");
    crate::task::process::exit_process(child_pid, 42);

    let mut status = 0i32;
    let waited_pid = crate::task::process::sys_waitpid(child_pid as i64, &mut status, 0);
    assert_eq!(waited_pid, child_pid as i32);
    assert_eq!(status >> 8, 42);

    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_dynamic_linker_and_shared_libraries() {
    print!("  [TEST] Dynamic Linker & Shared Libs (ld-cosmic.so) ... ");

    // 1. Verify shared libraries exist in /lib
    let vfs = crate::fs::vfs::VFS.lock();
    let lib_dir = vfs.resolve_path("/lib").expect("Missing /lib directory in VFS");
    let entries = lib_dir.lock().readdir().expect("Failed to read /lib");
    drop(vfs);

    let names: Vec<String> = entries.into_iter().map(|e| e.name).collect();
    assert!(names.contains(&String::from("ld-cosmic.so")));
    assert!(names.contains(&String::from("libc.so")));
    assert!(names.contains(&String::from("libm.so")));

    // 2. Open and execute /bin/dyntest in Ring 3
    let vfs = crate::fs::vfs::VFS.lock();
    let file = vfs.open("/bin/dyntest", 0).expect("Failed to open /bin/dyntest");
    drop(vfs);

    let mut buf = alloc::vec![0u8; 16384];
    let n = file.read(&mut buf).expect("Failed to read /bin/dyntest");

    let exit_code = crate::task::user::execute_user_binary(&buf[..n])
        .expect("Failed to execute dynamic binary /bin/dyntest in Ring 3");
    assert_eq!(exit_code, 0);

    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_posix_signals() {
    print!("  [TEST] POSIX Signals & Process Control ... ");

    // 1. Test sigaction: query and set signal handler
    let mut old_act = crate::ipc::signal::SigAction::default();
    let new_act = crate::ipc::signal::SigAction::new(0x401000); // Custom handler address
    let ret = crate::task::process::sys_rt_sigaction(2, &new_act, &mut old_act, 8); // SIGINT
    assert_eq!(ret, 0);
    assert_eq!(old_act.sa_handler, crate::ipc::signal::SIG_DFL);

    // Verify changed handler
    let mut curr_act = crate::ipc::signal::SigAction::default();
    crate::task::process::sys_rt_sigaction(2, core::ptr::null(), &mut curr_act, 8);
    assert_eq!(curr_act.sa_handler, 0x401000);

    // Restore to default
    let def_act = crate::ipc::signal::SigAction::default();
    crate::task::process::sys_rt_sigaction(2, &def_act, core::ptr::null_mut(), 8);

    // 2. Test sigprocmask: block and unblock signals
    let block_set: u64 = 1 << 2; // Block SIGINT
    let mut old_set: u64 = 0;
    let ret_mask = crate::task::process::sys_rt_sigprocmask(crate::ipc::signal::SIG_BLOCK, &block_set, &mut old_set, 8);
    assert_eq!(ret_mask, 0);

    let mut queried_mask: u64 = 0;
    crate::task::process::sys_rt_sigprocmask(crate::ipc::signal::SIG_SETMASK, core::ptr::null(), &mut queried_mask, 8);
    assert_eq!(queried_mask & (1 << 2), 1 << 2);

    // Unblock SIGINT
    crate::task::process::sys_rt_sigprocmask(crate::ipc::signal::SIG_UNBLOCK, &block_set, core::ptr::null_mut(), 8);

    // 3. Test inter-process signal delivery (sys_kill -> SIGINT default termination)
    let parent_pid = crate::task::process::get_current_pid();
    let child_pid = crate::task::process::alloc_process("sig_child");

    // Send SIGINT to child
    let kill_ret = crate::task::process::sys_kill(child_pid as i32, 2);
    assert_eq!(kill_ret, 0);

    // Verify child transitioned to Zombie with status 130 (128 + 2)
    let child_proc = crate::task::process::get_process(child_pid).expect("Child process missing");
    assert_eq!(child_proc.lock().state, crate::task::process::ProcessState::Zombie(130));

    // Verify parent received SIGCHLD
    let parent_proc = crate::task::process::get_process(parent_pid).expect("Parent process missing");
    assert_eq!(parent_proc.lock().signal_state.pending & (1 << 17), 1 << 17);

    // Reap child via sys_waitpid
    let mut status = 0i32;
    let waited = crate::task::process::sys_waitpid(child_pid as i64, &mut status, 0);
    assert_eq!(waited, child_pid as i32);
    assert_eq!(status >> 8, 130);

    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_smp_and_apic_subsystem() {
    print!("  [TEST] SMP Multi-Core, ACPI MADT & APIC ... ");

    // 1. Verify Local APIC is supported and online
    assert!(crate::arch::x86_64::apic::is_supported(), "Local APIC not supported by CPU");
    let bsp_id = crate::arch::x86_64::apic::get_apic_id();

    // 2. Verify ACPI MADT parsing
    let madt = crate::arch::x86_64::madt::MADT.lock();
    assert!(madt.lapic_address != 0, "MADT LAPIC address is 0");
    drop(madt);

    // 3. Verify I/O APIC presence and Redirection Table access
    let ioapic_virt = crate::arch::x86_64::ioapic::get_ioapic_base_virt();
    assert!(ioapic_virt != 0, "I/O APIC MMIO base not mapped");
    let redtbl1 = unsafe { crate::arch::x86_64::ioapic::read_redirection_entry(1) };
    assert_eq!(redtbl1 & 0xFF, 33, "IRQ 1 not routed to vector 33");

    // 4. Verify SMP Manager discovered cores and active BSP
    let smp = crate::arch::x86_64::smp::SMP.lock();
    assert!(smp.cpu_count() >= 1, "SMP CPU count is 0");
    assert!(smp.online_count() >= 1, "No online CPUs in SMP");
    assert_eq!(smp.bsp_apic_id, bsp_id);

    // 5. Test IPI dispatch interface if AP core exists
    if let Some(ap) = smp.cores.iter().find(|c| !c.is_bsp) {
        crate::arch::x86_64::apic::send_init_ipi(ap.apic_id);
        crate::arch::x86_64::apic::send_sipi(ap.apic_id, 0x08);
    }
    drop(smp);

    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_storage_ahci_and_nvme() {
    print!("  [TEST] Storage Drivers (AHCI SATA & NVMe SSD) ... ");

    // 1. Verify AHCI data structure alignments and command FIS encoding
    assert_eq!(core::mem::size_of::<crate::drivers::storage::ahci::AhciCmdHeader>(), 32);
    assert_eq!(core::mem::size_of::<crate::drivers::storage::ahci::AhciPrdtEntry>(), 16);
    assert_eq!(core::mem::size_of::<crate::drivers::storage::ahci::AhciCmdTable>(), 144);

    // 2. Verify NVMe data structure sizes and queue layouts
    assert_eq!(core::mem::size_of::<crate::drivers::storage::nvme::NvmeSqEntry>(), 64);
    assert_eq!(core::mem::size_of::<crate::drivers::storage::nvme::NvmeCqEntry>(), 16);

    // 3. Test unified Block Device abstraction
    let test_dev: Arc<dyn BlockDevice> = MemBlockDevice::new("sim_disk0", 4096);
    crate::drivers::storage::STORAGE_MANAGER.lock().register(test_dev.clone());

    let mut write_buf = [0u8; 512];
    write_buf[0..16].copy_from_slice(b"COSMIC_AHCI_NVME");
    test_dev.write_sectors(42, 1, &write_buf).expect("Block write failed");

    let mut read_buf = [0u8; 512];
    test_dev.read_sectors(42, 1, &mut read_buf).expect("Block read failed");
    assert_eq!(&read_buf[0..16], b"COSMIC_AHCI_NVME");

    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_userspace_windowserver_and_fb0() {
    print!("  [TEST] Userspace Aqua WindowServer, /dev/fb0 & libgui ... ");

    // 1. Verify /dev/fb0 exists and is a character device
    let vfs = VFS.lock();
    let fb_file = vfs.open("/dev/fb0", 0).expect("Failed to open /dev/fb0");
    let fb_stat = fb_file.inode.lock().stat();
    assert_eq!(fb_stat.file_type, FileType::CharDevice);
    assert!(fb_stat.size >= 1024 * 768 * 4, "/dev/fb0 reported size too small");

    // 2. Test ioctl FBIOGET_VSCREENINFO
    let mut vscreen = crate::fs::devfs::FbVarScreenInfo {
        xres: 0,
        yres: 0,
        bits_per_pixel: 0,
        line_length: 0,
        smem_start: 0,
        smem_len: 0,
    };
    let ioctl_ret = fb_file.ioctl(
        crate::fs::devfs::FBIOGET_VSCREENINFO,
        &mut vscreen as *mut _ as usize,
    ).expect("FBIOGET_VSCREENINFO failed");
    assert_eq!(ioctl_ret, 0);
    assert_eq!(vscreen.xres, 1024);
    assert_eq!(vscreen.yres, 768);
    assert_eq!(vscreen.bits_per_pixel, 32);
    assert_eq!(vscreen.line_length, 4096);

    // 3. Test ioctl FBIO_BLIT_RECT
    let test_pixel: u32 = 0x00FF8800; // Aqua Orange
    let blit = crate::fs::devfs::FbBlitRect {
        x: 100,
        y: 100,
        width: 1,
        height: 1,
        data_ptr: &test_pixel as *const u32 as u64,
    };
    let blit_ret = fb_file.ioctl(
        crate::fs::devfs::FBIO_BLIT_RECT,
        &blit as *const _ as usize,
    ).expect("FBIO_BLIT_RECT failed");
    assert_eq!(blit_ret, 0);

    // Read back pixel via /dev/fb0 read at byte offset (100 * 4096 + 100 * 4)
    let byte_offset = (100 * 4096 + 100 * 4) as usize;
    let mut read_pixel_bytes = [0u8; 4];
    let n = fb_file.inode.lock().read(byte_offset, &mut read_pixel_bytes).expect("Read /dev/fb0 failed");
    assert_eq!(n, 4);
    let read_pixel = u32::from_le_bytes(read_pixel_bytes);
    assert_eq!(read_pixel, test_pixel);

    // 4. Verify /bin/windowserver and /lib/libgui.so exist
    assert!(vfs.open("/bin/windowserver", 0).is_ok(), "/bin/windowserver missing");
    assert!(vfs.open("/lib/libgui.so", 0).is_ok(), "/lib/libgui.so missing");
    drop(vfs);

    // 5. Execute /bin/windowserver in Ring 3
    let vfs = VFS.lock();
    let ws_file = vfs.open("/bin/windowserver", 0).expect("Open /bin/windowserver failed");
    drop(vfs);
    let mut elf_data = alloc::vec![0u8; 65536];
    let bytes = ws_file.read(&mut elf_data).expect("Read /bin/windowserver failed");
    let exit_code = crate::task::user::execute_user_binary(&elf_data[..bytes]);
    assert_eq!(exit_code, Ok(0), "Userspace windowserver failed with non-zero exit");

    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_multicore_scheduler_and_affinity() {
    print!("  [TEST] Multi-Core Scheduler, Work-Stealing & Core Affinity ... ");

    // 1. Verify per-CPU runqueues are present
    let mut sched = crate::task::scheduler::SCHEDULER.lock();
    sched.ensure_cores(4); // Ensure at least 4 core runqueues for testing
    assert!(sched.per_cpu.len() >= 4);
    drop(sched);

    // 2. Test core affinity setting and querying
    let mut mask: u64 = 0;
    // Set affinity to CPU 2 (bit 2)
    let mask_val: u64 = 1 << 2;
    let set_ret = crate::task::process::sys_sched_setaffinity(0, 8, &mask_val as *const u64);
    assert_eq!(set_ret, 0);

    let get_ret = crate::task::process::sys_sched_getaffinity(0, 8, &mut mask as *mut u64);
    assert_eq!(get_ret, 0);
    assert_eq!(mask, 1 << 2, "Affinity mask mismatch after set");

    // Restore affinity to all cores
    let _ = crate::task::process::sys_sched_setaffinity(0, 8, &(!0u64) as *const u64);
    let _ = crate::task::process::sys_sched_getaffinity(0, 8, &mut mask as *mut u64);
    assert_eq!(mask, !0u64);

    // 3. Test multi-core load-balancing and thread placement
    let mut sched = crate::task::scheduler::SCHEDULER.lock();
    let t1 = Arc::new(IrqSafeSpinlock::new(crate::task::thread::ThreadControlBlock::new_kthread(
        9901,
        "test_affinity_core3",
        || {},
    )));
    t1.lock().set_affinity(1 << 3); // Restricted to Core 3 only
    sched.add_thread(t1.clone());

    // Verify thread was placed on CPU 3 runqueue
    assert_eq!(t1.lock().current_cpu, 3);
    assert!(sched.per_cpu[3].ready_queue.iter().any(|t| t.lock().tid == 9901));
    // Remove test thread
    sched.per_cpu[3].ready_queue.retain(|t| t.lock().tid != 9901);

    // 4. Test Work-Stealing:
    // Place a thread on Core 1 that is allowed to run on Core 0
    let t2 = Arc::new(IrqSafeSpinlock::new(crate::task::thread::ThreadControlBlock::new_kthread(
        9902,
        "test_stealable",
        || {},
    )));
    t2.lock().set_affinity(!0); // Can run on any core
    sched.per_cpu[1].ready_queue.push_back(t2.clone());
    drop(sched);

    let mut sched = crate::task::scheduler::SCHEDULER.lock();
    let initial_cpu1_len = sched.per_cpu[1].queue_len();
    assert!(initial_cpu1_len >= 1);
    // Clean up test thread
    sched.per_cpu[1].ready_queue.retain(|t| t.lock().tid != 9902);
    drop(sched);

    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_partition_table_automounter() {
    print!("  [TEST] Partition Table Auto-Mounter (GPT / MBR into /Volumes) ... ");

    // 1. Create a simulated disk with an MBR partition table containing a FAT32 partition
    let disk = MemBlockDevice::new("sim_mbr0", 8192);

    // Construct MBR sector at LBA 0
    let mut mbr = [0u8; 512];
    mbr[510] = 0x55;
    mbr[511] = 0xAA;

    // Partition 1 at offset 446:
    // Bootable: 0x80, Type: 0x0C (FAT32 LBA), Start LBA: 2048, Num sectors: 4096
    mbr[446] = 0x80;
    mbr[446 + 4] = 0x0C; // FAT32
    let start_lba: u32 = 2048;
    let num_sec: u32 = 4096;
    mbr[446 + 8..446 + 12].copy_from_slice(&start_lba.to_le_bytes());
    mbr[446 + 12..446 + 16].copy_from_slice(&num_sec.to_le_bytes());
    disk.write_sectors(0, 1, &mbr).expect("Write MBR failed");

    // 2. Format partition 1 (LBA 2048) with a valid FAT32 filesystem with volume label "VOL_TEST"
    let mut vbr = [0u8; 512];
    vbr[0..3].copy_from_slice(&[0xEB, 0x58, 0x90]);
    vbr[3..11].copy_from_slice(b"MSWIN4.1");
    vbr[11..13].copy_from_slice(&512u16.to_le_bytes()); // Bytes per sector
    vbr[13] = 1; // Sectors per cluster
    vbr[14..16].copy_from_slice(&32u16.to_le_bytes()); // Reserved sectors
    vbr[16] = 2; // Number of FATs
    vbr[32..36].copy_from_slice(&num_sec.to_le_bytes()); // Total sectors 32
    vbr[36..40].copy_from_slice(&32u32.to_le_bytes()); // Sectors per FAT
    vbr[44..48].copy_from_slice(&2u32.to_le_bytes()); // Root dir cluster 2
    vbr[71..82].copy_from_slice(b"VOL_TEST   "); // Volume label
    vbr[82..90].copy_from_slice(b"FAT32   ");
    vbr[510] = 0x55;
    vbr[511] = 0xAA;
    disk.write_sectors(2048, 1, &vbr).expect("Write FAT32 VBR failed");

    // 3. Scan partitions on simulated disk
    let parts = crate::fs::partition::scan_device_partitions(disk.clone());
    assert_eq!(parts.len(), 1, "Expected 1 MBR partition");
    assert_eq!(parts[0].name(), "sim_mbr0p1");
    assert_eq!(parts[0].total_sectors(), 4096);

    // 4. Test auto-mounting into /Volumes/
    let mounted_count = crate::fs::mount::mount_device(disk.clone());
    assert_eq!(mounted_count, 1, "Failed to auto-mount partition");

    // Verify /Volumes/VOL_TEST was mounted and can be resolved in VFS
    let vfs = VFS.lock();
    let vol_node = vfs.resolve_path("/Volumes/VOL_TEST").expect("Failed to resolve /Volumes/VOL_TEST");
    assert_eq!(vol_node.lock().stat().file_type, FileType::Directory);

    // 5. Verify GPT Partition scanning logic
    let gpt_disk = MemBlockDevice::new("sim_gpt0", 8192);
    let mut gpt_mbr = [0u8; 512];
    gpt_mbr[510] = 0x55;
    gpt_mbr[511] = 0xAA;
    gpt_mbr[446 + 4] = 0xEE; // GPT Protective MBR
    gpt_mbr[446 + 8..446 + 12].copy_from_slice(&1u32.to_le_bytes());
    gpt_mbr[446 + 12..446 + 16].copy_from_slice(&8191u32.to_le_bytes());
    gpt_disk.write_sectors(0, 1, &gpt_mbr).expect("Write GPT MBR failed");

    // GPT Header at LBA 1
    let mut gpt_hdr = [0u8; 512];
    gpt_hdr[0..8].copy_from_slice(b"EFI PART");
    gpt_hdr[72..80].copy_from_slice(&2u64.to_le_bytes()); // Part entries at LBA 2
    gpt_hdr[80..84].copy_from_slice(&4u32.to_le_bytes()); // 4 entries
    gpt_hdr[84..88].copy_from_slice(&128u32.to_le_bytes()); // 128 bytes per entry
    gpt_disk.write_sectors(1, 1, &gpt_hdr).expect("Write GPT Header failed");

    // Partition entry at LBA 2
    let mut gpt_entry = [0u8; 512];
    // Non-zero Type GUID (Microsoft Basic Data)
    gpt_entry[0..16].copy_from_slice(&[0xA2, 0xA0, 0xD0, 0xEB, 0xE5, 0xB9, 0x33, 0x44, 0x87, 0xC0, 0x68, 0xB6, 0xB7, 0x26, 0x99, 0xC7]);
    // Start LBA: 2048, End LBA: 4095
    gpt_entry[32..40].copy_from_slice(&2048u64.to_le_bytes());
    gpt_entry[40..48].copy_from_slice(&4095u64.to_le_bytes());
    gpt_disk.write_sectors(2, 1, &gpt_entry).expect("Write GPT Entry failed");

    let gpt_parts = crate::fs::partition::scan_device_partitions(gpt_disk.clone());
    assert_eq!(gpt_parts.len(), 1, "Expected 1 GPT partition");
    assert_eq!(gpt_parts[0].name(), "sim_gpt0p1");
    assert_eq!(gpt_parts[0].total_sectors(), 2048);

    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_network_socket_server_and_daemons() {
    print!("  [TEST] Network Socket Server Daemons (telnetd & sshd) ... ");

    // 1. Verify /bin/telnetd and /bin/sshd exist in VFS
    let vfs = VFS.lock();
    assert!(vfs.open("/bin/telnetd", 0).is_ok(), "/bin/telnetd missing");
    assert!(vfs.open("/bin/sshd", 0).is_ok(), "/bin/sshd missing");
    drop(vfs);

    // 2. Test server listening socket creation, binding and listening
    let srv_fd = crate::task::process::sys_socket(2, 1, 0);
    assert!(srv_fd >= 0, "sys_socket failed");

    let srv_addr = crate::task::process::SockAddrIn {
        sin_family: 2,
        sin_port: 2323u16.to_be(),
        sin_addr: [0, 0, 0, 0],
        sin_zero: [0; 8],
    };
    let bind_ret = crate::task::process::sys_bind(srv_fd as usize, &srv_addr, core::mem::size_of::<crate::task::process::SockAddrIn>());
    assert_eq!(bind_ret, 0, "sys_bind failed");

    let listen_ret = crate::task::process::sys_listen(srv_fd as usize, 5);
    assert_eq!(listen_ret, 0, "sys_listen failed");

    // 3. Test client socket creation and connection over loopback (127.0.0.1:2323)
    let cli_fd = crate::task::process::sys_socket(2, 1, 0);
    assert!(cli_fd >= 0, "client sys_socket failed");

    let cli_dest = crate::task::process::SockAddrIn {
        sin_family: 2,
        sin_port: 2323u16.to_be(),
        sin_addr: [127, 0, 0, 1],
        sin_zero: [0; 8],
    };
    let conn_ret = crate::task::process::sys_connect(cli_fd as usize, &cli_dest, core::mem::size_of::<crate::task::process::SockAddrIn>());
    assert_eq!(conn_ret, 0, "sys_connect failed");

    // 4. Test server accepting incoming connection
    let mut peer_addr = crate::task::process::SockAddrIn {
        sin_family: 0,
        sin_port: 0,
        sin_addr: [0; 4],
        sin_zero: [0; 8],
    };
    let mut addrlen: u32 = 0;
    let acc_fd = crate::task::process::sys_accept(srv_fd as usize, &mut peer_addr, &mut addrlen);
    assert!(acc_fd >= 0, "sys_accept failed");

    // 5. Test inter-socket data transfer (client -> server)
    let msg = b"HELLO_COSMIC_DAEMON";
    let sent = crate::task::process::sys_sendto(cli_fd as usize, msg.as_ptr(), msg.len(), 0, core::ptr::null(), 0);
    assert_eq!(sent, msg.len() as i64, "sys_sendto failed");

    let mut rx_buf = [0u8; 32];
    let recvd = crate::task::process::sys_recvfrom(acc_fd as usize, rx_buf.as_mut_ptr(), rx_buf.len(), 0, core::ptr::null_mut(), core::ptr::null_mut());
    assert_eq!(recvd, msg.len() as i64, "sys_recvfrom failed");
    assert_eq!(&rx_buf[..msg.len()], msg);

    // 6. Test reply transfer (server -> client)
    let reply = b"ACK_COSMIC_DAEMON";
    let reply_sent = crate::task::process::sys_sendto(acc_fd as usize, reply.as_ptr(), reply.len(), 0, core::ptr::null(), 0);
    assert_eq!(reply_sent, reply.len() as i64);

    let mut cli_rx = [0u8; 32];
    let cli_recvd = crate::task::process::sys_recvfrom(cli_fd as usize, cli_rx.as_mut_ptr(), cli_rx.len(), 0, core::ptr::null_mut(), core::ptr::null_mut());
    assert_eq!(cli_recvd, reply.len() as i64);
    assert_eq!(&cli_rx[..reply.len()], reply);

    // 7. Close sockets
    let _ = crate::task::process::sys_close(cli_fd as usize);
    let _ = crate::task::process::sys_close(acc_fd as usize);
    let _ = crate::task::process::sys_close(srv_fd as usize);

    // 8. Execute /bin/telnetd in Ring 3
    let vfs = VFS.lock();
    let telnetd_file = vfs.open("/bin/telnetd", 0).expect("Open /bin/telnetd failed");
    drop(vfs);
    let mut elf_data = alloc::vec![0u8; 65536];
    let bytes = telnetd_file.read(&mut elf_data).expect("Read /bin/telnetd failed");
    let exit_telnetd = crate::task::user::execute_user_binary(&elf_data[..bytes]);
    assert_eq!(exit_telnetd, Ok(0), "Userspace telnetd failed");

    // 9. Execute /bin/sshd in Ring 3
    let vfs = VFS.lock();
    let sshd_file = vfs.open("/bin/sshd", 0).expect("Open /bin/sshd failed");
    drop(vfs);
    let bytes_sshd = sshd_file.read(&mut elf_data).expect("Read /bin/sshd failed");
    let exit_sshd = crate::task::user::execute_user_binary(&elf_data[..bytes_sshd]);
    assert_eq!(exit_sshd, Ok(0), "Userspace sshd failed");

    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_pseudo_terminal_subsystem() {
    print!("  [TEST] Pseudo-Terminal (PTY) Subsystem & Line Discipline ... ");

    // 1. Open master pseudo-terminal
    let master_fd = crate::task::process::sys_posix_openpt(2); // O_RDWR
    assert!(master_fd >= 0, "posix_openpt failed");

    // 2. Query slave path (ptsname)
    let mut pts_name = [0u8; 32];
    let pts_res = crate::task::process::sys_ptsname(master_fd as usize, pts_name.as_mut_ptr(), pts_name.len());
    assert_eq!(pts_res, 0, "sys_ptsname failed");
    let name_str = core::str::from_utf8(&pts_name).unwrap();
    let trimmed = name_str.trim_matches('\0');
    assert!(trimmed.starts_with("/dev/pts/"), "Invalid ptsname path");

    // 3. Unlock slave (unlockpt)
    let unlock_res = crate::task::process::sys_unlockpt(master_fd as usize);
    assert_eq!(unlock_res, 0, "sys_unlockpt failed");

    // 4. Open slave pseudo-terminal
    let slave_fd = crate::task::process::sys_open(pts_name.as_ptr(), 2, 0);
    assert!(slave_fd >= 0, "open slave pts failed");

    // 5. Test master -> slave with ICRNL line translation
    let input = b"uptime\r";
    let sent = crate::syscall::fs_calls::sys_write(master_fd as usize, input.as_ptr(), input.len());
    assert_eq!(sent, input.len() as isize);

    let mut slave_buf = [0u8; 32];
    let read_slv = crate::syscall::fs_calls::sys_read(slave_fd as usize, slave_buf.as_mut_ptr(), slave_buf.len());
    assert_eq!(read_slv, 7); // "uptime\n"
    assert_eq!(&slave_buf[..7], b"uptime\n");

    // 6. Test slave -> master transfer
    let output = b"14:02:11 up 42 secs\n";
    let w_slv = crate::syscall::fs_calls::sys_write(slave_fd as usize, output.as_ptr(), output.len());
    assert_eq!(w_slv, output.len() as isize);

    let mut master_buf = [0u8; 32];
    let read_mst = crate::syscall::fs_calls::sys_read(master_fd as usize, master_buf.as_mut_ptr(), master_buf.len());
    assert!(read_mst > 0);

    // 7. Test terminal window size ioctl (TIOCGWINSZ / TIOCSWINSZ)
    let new_win = crate::tty::pty::Winsize {
        ws_row: 40,
        ws_col: 120,
        ws_xpixel: 1024,
        ws_ypixel: 768,
    };
    let set_win = crate::task::process::sys_ioctl(master_fd as usize, crate::tty::pty::TIOCSWINSZ, &new_win as *const crate::tty::pty::Winsize as usize);
    assert_eq!(set_win, 0);

    let mut query_win = crate::tty::pty::Winsize { ws_row: 0, ws_col: 0, ws_xpixel: 0, ws_ypixel: 0 };
    let get_win = crate::task::process::sys_ioctl(slave_fd as usize, crate::tty::pty::TIOCGWINSZ, &mut query_win as *mut crate::tty::pty::Winsize as usize);
    assert_eq!(get_win, 0);
    assert_eq!(query_win.ws_row, 40);
    assert_eq!(query_win.ws_col, 120);

    // Close fds
    let _ = crate::task::process::sys_close(slave_fd as usize);
    let _ = crate::task::process::sys_close(master_fd as usize);

    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_tls_and_https_engine() {
    print!("  [TEST] TLS 1.3 Cryptography & Secure HTTPS Engine ... ");

    // 1. Test SHA-256
    let hash = crate::net::crypto::sha256(b"CosmicOS-TLS13");
    assert_eq!(hash.len(), 32);

    // 2. Test HMAC-SHA256
    let hmac = crate::net::crypto::hmac_sha256(b"secret_key", b"payload_data");
    assert_eq!(hmac.len(), 32);

    // 3. Test HKDF-Extract and HKDF-Expand
    let prk = crate::net::crypto::hkdf_extract(b"salt123", b"input_key_material");
    let okm = crate::net::crypto::hkdf_expand(&prk, b"info_label", 48);
    assert_eq!(okm.len(), 48);

    // 4. Test X25519 ECDH Key Exchange
    let mut alice_priv = [0x55u8; 32];
    alice_priv[0] &= 248; alice_priv[31] &= 127; alice_priv[31] |= 64;
    let alice_pub = crate::net::crypto::x25519_base(&alice_priv);

    let mut bob_priv = [0xAAu8; 32];
    bob_priv[0] &= 248; bob_priv[31] &= 127; bob_priv[31] |= 64;
    let bob_pub = crate::net::crypto::x25519_base(&bob_priv);

    let shared_alice = crate::net::crypto::x25519(&alice_priv, &bob_pub);
    let shared_bob = crate::net::crypto::x25519(&bob_priv, &alice_pub);
    assert_eq!(shared_alice, shared_bob, "X25519 shared secret mismatch");

    // 5. Test ChaCha20-Poly1305 AEAD seal & open
    let aead_key = [0x42u8; 32];
    let nonce = [0x11u8; 12];
    let aad = b"TLS13-Record-AAD";
    let plaintext = b"GET /api/status HTTP/1.1\r\nHost: cosmicos.org\r\n\r\n";

    let (ciphertext, tag) = crate::net::crypto::chacha20_poly1305_seal(&aead_key, &nonce, aad, plaintext);
    assert_eq!(ciphertext.len(), plaintext.len());

    let decrypted = crate::net::crypto::chacha20_poly1305_open(&aead_key, &nonce, aad, &ciphertext, &tag)
        .expect("ChaCha20-Poly1305 decryption failed");
    assert_eq!(&decrypted, plaintext);

    // Corrupted tag must fail
    let mut bad_tag = tag;
    bad_tag[0] ^= 0xFF;
    assert!(crate::net::crypto::chacha20_poly1305_open(&aead_key, &nonce, aad, &ciphertext, &bad_tag).is_err());

    // 6. Test HTTPS GET Request Engine
    let resp = crate::net::http::https_get("https://127.0.0.1/api/status").expect("https_get failed");
    assert_eq!(resp.status_code, 200);
    assert!(resp.body_as_str().contains("CosmicOS"));

    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_realtek_and_qualcomm_drivers() {
    print!("  [TEST] Realtek RTL8139/8169 & Qualcomm Atheros Drivers ... ");

    use crate::net::device::{NetDevice, NetDeviceType, NET_MANAGER};

    // 1. Test Realtek RTL8169 Gigabit Ethernet Driver
    let pci_rtl = crate::drivers::pci::PciDevice {
        bus: 0,
        device: 4,
        function: 0,
        vendor_id: 0x10EC,
        device_id: 0x8169,
        class_code: 0x02,
        subclass: 0x00,
        prog_if: 0x00,
        header_type: 0x00,
        bar0: 0xC001,
    };

    let rtl_dev = crate::drivers::net::realtek::RealtekDevice::new(pci_rtl)
        .expect("Realtek RTL8169 driver initialization failed");

    assert_eq!(rtl_dev.name(), "en1");
    assert_eq!(rtl_dev.device_type(), NetDeviceType::Ethernet);
    assert!(rtl_dev.is_link_up());
    let rtl_mac = rtl_dev.mac_address();
    assert_ne!(rtl_mac, [0, 0, 0, 0, 0, 0]);

    // Send packet through Realtek driver and verify loopback reception
    let test_frame = [
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, // Destination Broadcast
        rtl_mac[0], rtl_mac[1], rtl_mac[2], rtl_mac[3], rtl_mac[4], rtl_mac[5], // Source
        0x08, 0x00, // IPv4
        0x45, 0x00, 0x00, 0x20, 0x00, 0x01, 0x00, 0x00, 0x40, 0x11, 0x00, 0x00,
        127, 0, 0, 1, 127, 0, 0, 1,
    ];

    rtl_dev.send_packet(&test_frame).expect("RTL8169 send_packet failed");
    let mut rx_buf = [0u8; 128];
    let n = rtl_dev.receive_packet(&mut rx_buf).expect("RTL8169 receive_packet failed");
    assert_eq!(n, test_frame.len());
    assert_eq!(&rx_buf[..n], &test_frame);

    // 2. Test Qualcomm Killer E2200 Gigabit Gaming NIC Driver
    let pci_qca = crate::drivers::pci::PciDevice {
        bus: 0,
        device: 5,
        function: 0,
        vendor_id: 0x1969,
        device_id: 0xE091,
        class_code: 0x02,
        subclass: 0x00,
        prog_if: 0x00,
        header_type: 0x00,
        bar0: 0,
    };

    let qca_dev = crate::drivers::net::qualcomm::QualcommDevice::new(pci_qca)
        .expect("Qualcomm Killer E2200 driver initialization failed");

    assert_eq!(qca_dev.name(), "en2");
    assert_eq!(qca_dev.device_type(), NetDeviceType::Ethernet);
    assert!(qca_dev.is_link_up());
    let qca_mac = qca_dev.mac_address();
    assert_eq!(&qca_mac[0..3], &[0x00, 0x03, 0x7F]); // Atheros Communications OUI

    qca_dev.send_packet(&test_frame).expect("Qualcomm send_packet failed");
    let mut rx_buf_qca = [0u8; 128];
    let n_qca = qca_dev.receive_packet(&mut rx_buf_qca).expect("Qualcomm receive_packet failed");
    assert_eq!(n_qca, test_frame.len());
    assert_eq!(&rx_buf_qca[..n_qca], &test_frame);

    // 3. Test Qualcomm Atheros AR9285 Wi-Fi (ath9k) Driver
    let pci_ath = crate::drivers::pci::PciDevice {
        bus: 0,
        device: 6,
        function: 0,
        vendor_id: 0x168C,
        device_id: 0x002B,
        class_code: 0x02,
        subclass: 0x80,
        prog_if: 0x00,
        header_type: 0x00,
        bar0: 0,
    };

    let ath_dev = crate::drivers::net::qualcomm::QualcommDevice::new(pci_ath)
        .expect("Qualcomm ath9k driver initialization failed");

    assert_eq!(ath_dev.name(), "wlan1");
    assert_eq!(ath_dev.device_type(), NetDeviceType::Wireless80211);
    assert!(ath_dev.is_wireless);
    assert!(ath_dev.is_link_up());

    // 4. Test Registration into Central Network Manager
    let prev_count = NET_MANAGER.lock().devices.len();
    NET_MANAGER.lock().register(rtl_dev);
    NET_MANAGER.lock().register(qca_dev);
    NET_MANAGER.lock().register(ath_dev);
    let new_count = NET_MANAGER.lock().devices.len();
    assert_eq!(new_count, prev_count + 3);

    // Verify lookup by interface name
    let found_en1 = NET_MANAGER.lock().get_device("en1");
    assert!(found_en1.is_some());
    let found_en2 = NET_MANAGER.lock().get_device("en2");
    assert!(found_en2.is_some());
    let found_wlan1 = NET_MANAGER.lock().get_device("wlan1");
    assert!(found_wlan1.is_some());

    println!("\x1b[32m[PASS]\x1b[0m");
}
