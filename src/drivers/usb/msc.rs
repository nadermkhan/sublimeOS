extern crate alloc;
use alloc::string::String;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicU32, Ordering};
use crate::drivers::storage::BlockDevice;

pub const CBW_SIGNATURE: u32 = 0x43425355; // "USBC"
pub const CSW_SIGNATURE: u32 = 0x53425355; // "USBS"

pub const SCSI_TEST_UNIT_READY: u8 = 0x00;
pub const SCSI_INQUIRY: u8 = 0x12;
pub const SCSI_READ_CAPACITY_10: u8 = 0x25;
pub const SCSI_READ_10: u8 = 0x28;
pub const SCSI_WRITE_10: u8 = 0x2A;

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct CommandBlockWrapper {
    pub signature: u32,
    pub tag: u32,
    pub data_transfer_length: u32,
    pub flags: u8,
    pub lun: u8,
    pub cb_length: u8,
    pub cdb: [u8; 16],
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct CommandStatusWrapper {
    pub signature: u32,
    pub tag: u32,
    pub data_residue: u32,
    pub status: u8,
}

pub trait UsbMassStorageTransport: Send + Sync {
    fn bulk_out(&self, data: &[u8]) -> Result<(), &'static str>;
    fn bulk_in(&self, buf: &mut [u8]) -> Result<usize, &'static str>;
}

pub struct UsbBlockDevice {
    pub name: String,
    pub total_sectors: u64,
    pub sector_size: usize,
    pub vendor: String,
    pub model: String,
    pub transport: Arc<dyn UsbMassStorageTransport>,
    pub tag_counter: AtomicU32,
}

impl UsbBlockDevice {
    pub fn new(
        name: String,
        total_sectors: u64,
        sector_size: usize,
        vendor: String,
        model: String,
        transport: Arc<dyn UsbMassStorageTransport>,
    ) -> Self {
        Self {
            name,
            total_sectors,
            sector_size,
            vendor,
            model,
            transport,
            tag_counter: AtomicU32::new(1),
        }
    }

    fn next_tag(&self) -> u32 {
        self.tag_counter.fetch_add(1, Ordering::Relaxed)
    }

    /// Query a newly attached USB Mass Storage device using SCSI INQUIRY and READ CAPACITY.
    pub fn probe(
        transport: Arc<dyn UsbMassStorageTransport>,
        name: String,
    ) -> Result<Arc<Self>, &'static str> {
        let tag = 0x1001;

        // 1. SCSI Inquiry (query device vendor and model)
        let mut inquiry_cdb = [0u8; 16];
        inquiry_cdb[0] = SCSI_INQUIRY;
        inquiry_cdb[4] = 36; // Allocation length: 36 bytes

        let cbw = CommandBlockWrapper {
            signature: CBW_SIGNATURE,
            tag,
            data_transfer_length: 36,
            flags: 0x80, // IN (device to host)
            lun: 0,
            cb_length: 6,
            cdb: inquiry_cdb,
        };

        let cbw_bytes = unsafe {
            core::slice::from_raw_parts(&cbw as *const _ as *const u8, core::mem::size_of::<CommandBlockWrapper>())
        };
        transport.bulk_out(cbw_bytes)?;

        let mut inq_buf = [0u8; 36];
        let _ = transport.bulk_in(&mut inq_buf)?;

        let mut csw_buf = [0u8; 13];
        let _ = transport.bulk_in(&mut csw_buf)?;

        let vendor_str = core::str::from_utf8(&inq_buf[8..16]).unwrap_or("USB").trim();
        let model_str = core::str::from_utf8(&inq_buf[16..32]).unwrap_or("Flash Drive").trim();

        // 2. SCSI Test Unit Ready
        let mut tur_cdb = [0u8; 16];
        tur_cdb[0] = SCSI_TEST_UNIT_READY;
        let cbw_tur = CommandBlockWrapper {
            signature: CBW_SIGNATURE,
            tag: tag + 1,
            data_transfer_length: 0,
            flags: 0x00,
            lun: 0,
            cb_length: 6,
            cdb: tur_cdb,
        };
        let cbw_tur_bytes = unsafe {
            core::slice::from_raw_parts(&cbw_tur as *const _ as *const u8, 31)
        };
        let _ = transport.bulk_out(cbw_tur_bytes);
        let _ = transport.bulk_in(&mut csw_buf);

        // 3. SCSI Read Capacity 10
        let mut cap_cdb = [0u8; 16];
        cap_cdb[0] = SCSI_READ_CAPACITY_10;
        let cbw_cap = CommandBlockWrapper {
            signature: CBW_SIGNATURE,
            tag: tag + 2,
            data_transfer_length: 8,
            flags: 0x80,
            lun: 0,
            cb_length: 10,
            cdb: cap_cdb,
        };
        let cbw_cap_bytes = unsafe {
            core::slice::from_raw_parts(&cbw_cap as *const _ as *const u8, 31)
        };
        transport.bulk_out(cbw_cap_bytes)?;

        let mut cap_buf = [0u8; 8];
        transport.bulk_in(&mut cap_buf)?;
        transport.bulk_in(&mut csw_buf)?;

        let last_lba = ((cap_buf[0] as u64) << 24)
            | ((cap_buf[1] as u64) << 16)
            | ((cap_buf[2] as u64) << 8)
            | (cap_buf[3] as u64);

        let block_size = ((cap_buf[4] as usize) << 24)
            | ((cap_buf[5] as usize) << 16)
            | ((cap_buf[6] as usize) << 8)
            | (cap_buf[7] as usize);

        let total_sectors = if last_lba > 0 { last_lba + 1 } else { 2048 };
        let sector_size = if block_size >= 512 && block_size <= 4096 { block_size } else { 512 };

        Ok(Arc::new(Self::new(
            name,
            total_sectors,
            sector_size,
            String::from(vendor_str),
            String::from(model_str),
            transport,
        )))
    }
}

impl BlockDevice for UsbBlockDevice {
    fn name(&self) -> &str {
        &self.name
    }

    fn read_sectors(&self, lba: u64, count: u32, buf: &mut [u8]) -> Result<(), &'static str> {
        let tag = self.next_tag();
        let total_bytes = count * (self.sector_size as u32);

        let mut cdb = [0u8; 16];
        cdb[0] = SCSI_READ_10;
        cdb[2] = ((lba >> 24) & 0xFF) as u8;
        cdb[3] = ((lba >> 16) & 0xFF) as u8;
        cdb[4] = ((lba >> 8) & 0xFF) as u8;
        cdb[5] = (lba & 0xFF) as u8;
        cdb[7] = ((count >> 8) & 0xFF) as u8;
        cdb[8] = (count & 0xFF) as u8;

        let cbw = CommandBlockWrapper {
            signature: CBW_SIGNATURE,
            tag,
            data_transfer_length: total_bytes,
            flags: 0x80, // IN
            lun: 0,
            cb_length: 10,
            cdb,
        };

        let cbw_bytes = unsafe {
            core::slice::from_raw_parts(&cbw as *const _ as *const u8, 31)
        };

        // 1. Send Command Block Wrapper (CBW)
        self.transport.bulk_out(cbw_bytes)?;

        // 2. Read Sector Data
        let bytes_received = self.transport.bulk_in(buf)?;
        if bytes_received < total_bytes as usize {
            return Err("USB Mass Storage: Incomplete sector read");
        }

        // 3. Receive Command Status Wrapper (CSW)
        let mut csw_buf = [0u8; 13];
        self.transport.bulk_in(&mut csw_buf)?;

        let csw_sig = (csw_buf[0] as u32)
            | ((csw_buf[1] as u32) << 8)
            | ((csw_buf[2] as u32) << 16)
            | ((csw_buf[3] as u32) << 24);
        let csw_status = csw_buf[12];

        if csw_sig != CSW_SIGNATURE || csw_status != 0 {
            return Err("USB Mass Storage: SCSI Read(10) failed");
        }

        Ok(())
    }

    fn write_sectors(&self, lba: u64, count: u32, buf: &[u8]) -> Result<(), &'static str> {
        let tag = self.next_tag();
        let total_bytes = count * (self.sector_size as u32);

        let mut cdb = [0u8; 16];
        cdb[0] = SCSI_WRITE_10;
        cdb[2] = ((lba >> 24) & 0xFF) as u8;
        cdb[3] = ((lba >> 16) & 0xFF) as u8;
        cdb[4] = ((lba >> 8) & 0xFF) as u8;
        cdb[5] = (lba & 0xFF) as u8;
        cdb[7] = ((count >> 8) & 0xFF) as u8;
        cdb[8] = (count & 0xFF) as u8;

        let cbw = CommandBlockWrapper {
            signature: CBW_SIGNATURE,
            tag,
            data_transfer_length: total_bytes,
            flags: 0x00, // OUT
            lun: 0,
            cb_length: 10,
            cdb,
        };

        let cbw_bytes = unsafe {
            core::slice::from_raw_parts(&cbw as *const _ as *const u8, 31)
        };

        // 1. Send Command Block Wrapper (CBW)
        self.transport.bulk_out(cbw_bytes)?;

        // 2. Write Sector Data
        self.transport.bulk_out(buf)?;

        // 3. Receive Command Status Wrapper (CSW)
        let mut csw_buf = [0u8; 13];
        self.transport.bulk_in(&mut csw_buf)?;

        let csw_sig = (csw_buf[0] as u32)
            | ((csw_buf[1] as u32) << 8)
            | ((csw_buf[2] as u32) << 16)
            | ((csw_buf[3] as u32) << 24);
        let csw_status = csw_buf[12];

        if csw_sig != CSW_SIGNATURE || csw_status != 0 {
            return Err("USB Mass Storage: SCSI Write(10) failed");
        }

        Ok(())
    }

    fn total_sectors(&self) -> u64 {
        self.total_sectors
    }

    fn sector_size(&self) -> usize {
        self.sector_size
    }
}
