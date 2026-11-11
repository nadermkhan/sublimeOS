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
