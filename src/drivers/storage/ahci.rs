extern crate alloc;
use alloc::string::String;
use alloc::sync::Arc;
use crate::drivers::pci::{PciBus, PCI};
use crate::drivers::storage::BlockDevice;
use crate::mm::phys_to_virt;
use crate::mm::pmm::alloc_zeroed_frame;
use crate::mm::vmm::map_mmio_range;
use crate::sync::IrqSafeSpinlock;
use crate::{kinfo, kwarn};

// Generic Host Control Register Offsets
pub const HOST_CAP: usize = 0x00;
pub const HOST_GHC: usize = 0x04;
pub const HOST_IS: usize = 0x08;
pub const HOST_PI: usize = 0x0C;
pub const HOST_VS: usize = 0x10;

// Port Register Offsets (relative to port base: 0x100 + port * 0x80)
pub const PORT_CLB: usize = 0x00;
pub const PORT_CLBU: usize = 0x04;
pub const PORT_FB: usize = 0x08;
pub const PORT_FBU: usize = 0x0C;
pub const PORT_IS: usize = 0x10;
pub const PORT_IE: usize = 0x14;
pub const PORT_CMD: usize = 0x18;
pub const PORT_TFD: usize = 0x20;
pub const PORT_SIG: usize = 0x24;
pub const PORT_SSTS: usize = 0x28;
pub const PORT_SCTL: usize = 0x2C;
pub const PORT_SERR: usize = 0x30;
pub const PORT_SACT: usize = 0x34;
pub const PORT_CI: usize = 0x38;

// ATA Commands
pub const ATA_CMD_READ_DMA_EX: u8 = 0x25;
pub const ATA_CMD_WRITE_DMA_EX: u8 = 0x35;
pub const ATA_CMD_IDENTIFY: u8 = 0xEC;

// FIS Types
pub const FIS_TYPE_REG_H2D: u8 = 0x27;

// Port Command Bits
pub const PORT_CMD_ST: u32 = 1 << 0;  // Start
pub const PORT_CMD_FRE: u32 = 1 << 4; // FIS Receive Enable
pub const PORT_CMD_FR: u32 = 1 << 14; // FIS Receive Running
pub const PORT_CMD_CR: u32 = 1 << 15; // Command List Running

// Port Task File Data Bits
pub const PORT_TFD_BSY: u32 = 1 << 7;
pub const PORT_TFD_DRQ: u32 = 1 << 3;
pub const PORT_TFD_ERR: u32 = 1 << 0;

// Signatures
pub const SATA_SIG_ATA: u32 = 0x00000101;
pub const SATA_SIG_ATAPI: u32 = 0xEB140101;

#[repr(C, packed)]
pub struct AhciCmdHeader {
    pub flags: u16,
    pub prdtl: u16,
    pub prdbc: u32,
    pub ctba: u32,
    pub ctbau: u32,
    pub reserved: [u32; 4],
}

#[repr(C, packed)]
pub struct AhciPrdtEntry {
    pub dba: u32,
    pub dbau: u32,
    pub reserved: u32,
    pub dbc: u32, // Byte count (0-indexed: size - 1), bit 31 = IOC
}

#[repr(C, packed)]
pub struct AhciCmdTable {
    pub cfis: [u8; 64],
    pub acmd: [u8; 16],
    pub reserved: [u8; 48],
    pub prdt: [AhciPrdtEntry; 1],
}

pub struct AhciPort {
    pub port_no: usize,
    pub mmio_base: u64,
    pub cmd_list_phys: u64,
    pub fis_phys: u64,
    pub cmd_table_phys: u64,
    pub dma_buf_phys: u64,
    pub total_sectors: u64,
    pub model: String,
}

impl AhciPort {
    pub unsafe fn read_reg(&self, offset: usize) -> u32 {
        let port_base = self.mmio_base + 0x100 + (self.port_no as u64 * 0x80);
        let ptr = (port_base + offset as u64) as *const u32;
        core::ptr::read_volatile(ptr)
    }

    pub unsafe fn write_reg(&self, offset: usize, val: u32) {
        let port_base = self.mmio_base + 0x100 + (self.port_no as u64 * 0x80);
        let ptr = (port_base + offset as u64) as *mut u32;
        core::ptr::write_volatile(ptr, val);
    }

    pub fn stop_port(&self) {
        unsafe {
            let mut cmd = self.read_reg(PORT_CMD);
            cmd &= !PORT_CMD_ST;
            cmd &= !PORT_CMD_FRE;
            self.write_reg(PORT_CMD, cmd);

            // Wait until FR and CR are clear
            for _ in 0..1000 {
                let status = self.read_reg(PORT_CMD);
                if (status & (PORT_CMD_FR | PORT_CMD_CR)) == 0 {
                    break;
                }
                crate::arch::x86_64::pit::sleep_ms(1);
            }
        }
    }

    pub fn start_port(&self) {
        unsafe {
            // Wait until CR is clear
            for _ in 0..1000 {
                if (self.read_reg(PORT_CMD) & PORT_CMD_CR) == 0 {
                    break;
                }
                crate::arch::x86_64::pit::sleep_ms(1);
            }

            let mut cmd = self.read_reg(PORT_CMD);
            cmd |= PORT_CMD_FRE;
            self.write_reg(PORT_CMD, cmd);
            cmd |= PORT_CMD_ST;
            self.write_reg(PORT_CMD, cmd);
        }
    }

    pub fn issue_command(&self, slot: u32) -> Result<(), &'static str> {
        unsafe {
            // Clear interrupt status
            self.write_reg(PORT_IS, 0xFFFF_FFFF);

            // Issue command
            self.write_reg(PORT_CI, 1 << slot);

            // Poll for completion
            for _ in 0..2000 {
                let ci = self.read_reg(PORT_CI);
                if (ci & (1 << slot)) == 0 {
                    let tfd = self.read_reg(PORT_TFD);
                    if (tfd & PORT_TFD_ERR) != 0 {
                        return Err("AHCI: Command reported error in TFD");
                    }
                    return Ok(());
                }
                crate::arch::x86_64::pit::sleep_ms(1);
            }
        }
        Err("AHCI: Command timed out")
    }

    pub fn identify(&mut self) -> Result<u64, &'static str> {
        let cmd_hdr_virt = phys_to_virt(self.cmd_list_phys) as *mut AhciCmdHeader;
        let cmd_tbl_virt = phys_to_virt(self.cmd_table_phys) as *mut AhciCmdTable;
        let dma_buf_virt = phys_to_virt(self.dma_buf_phys) as *mut u8;

        unsafe {
            core::ptr::write_bytes(cmd_tbl_virt, 0, 1);
            core::ptr::write_bytes(dma_buf_virt, 0, 512);

            let tbl = &mut *cmd_tbl_virt;
            // Build Register H2D FIS for ATA IDENTIFY
            tbl.cfis[0] = FIS_TYPE_REG_H2D;
            tbl.cfis[1] = 0x80; // Command bit set
            tbl.cfis[2] = ATA_CMD_IDENTIFY;

            // PRDT pointing to 512-byte DMA buffer
            tbl.prdt[0].dba = (self.dma_buf_phys & 0xFFFF_FFFF) as u32;
            tbl.prdt[0].dbau = ((self.dma_buf_phys >> 32) & 0xFFFF_FFFF) as u32;
            tbl.prdt[0].dbc = 512 - 1; // 512 bytes

            let hdr = &mut *cmd_hdr_virt;
            hdr.flags = 5; // 5 Dwords in CFIS
            hdr.prdtl = 1;
            hdr.prdbc = 0;
            hdr.ctba = (self.cmd_table_phys & 0xFFFF_FFFF) as u32;
            hdr.ctbau = ((self.cmd_table_phys >> 32) & 0xFFFF_FFFF) as u32;
        }

        self.issue_command(0)?;

        // Parse IDENTIFY response
        let identify_words = unsafe {
            core::slice::from_raw_parts(dma_buf_virt as *const u16, 256)
        };

        // Words 100-103: Total 48-bit sectors
        let sectors_48 = (identify_words[100] as u64)
            | ((identify_words[101] as u64) << 16)
            | ((identify_words[102] as u64) << 32)
            | ((identify_words[103] as u64) << 48);

        let sectors_28 = (identify_words[60] as u64) | ((identify_words[61] as u64) << 16);
        let total_sectors = if sectors_48 > 0 { sectors_48 } else { sectors_28 };

        // Words 27-46: Model string (40 ASCII chars, big-endian pairs)
        let mut model_bytes = [0u8; 40];
        for i in 0..20 {
            let word = identify_words[27 + i];
            model_bytes[i * 2] = (word >> 8) as u8;
            model_bytes[i * 2 + 1] = (word & 0xFF) as u8;
        }
        let model_str = core::str::from_utf8(&model_bytes)
            .unwrap_or("SATA Disk")
            .trim();

        self.model = String::from(model_str);
        self.total_sectors = if total_sectors > 0 { total_sectors } else { 2097152 }; // Default 1 GB

        Ok(self.total_sectors)
    }

    pub fn read_sector_dma(&self, lba: u64, count: u32, buf: &mut [u8]) -> Result<(), &'static str> {
        if count == 0 || (count as usize * 512) > buf.len() {
            return Err("Invalid buffer length for AHCI read");
        }

        let cmd_hdr_virt = phys_to_virt(self.cmd_list_phys) as *mut AhciCmdHeader;
        let cmd_tbl_virt = phys_to_virt(self.cmd_table_phys) as *mut AhciCmdTable;
        let bytes_to_read = count * 512;

        unsafe {
            core::ptr::write_bytes(cmd_tbl_virt, 0, 1);
            let tbl = &mut *cmd_tbl_virt;

            tbl.cfis[0] = FIS_TYPE_REG_H2D;
            tbl.cfis[1] = 0x80; // Command
            tbl.cfis[2] = ATA_CMD_READ_DMA_EX;

            tbl.cfis[4] = (lba & 0xFF) as u8;
            tbl.cfis[5] = ((lba >> 8) & 0xFF) as u8;
            tbl.cfis[6] = ((lba >> 16) & 0xFF) as u8;
            tbl.cfis[7] = 0x40; // Device register: LBA mode

            tbl.cfis[8] = ((lba >> 24) & 0xFF) as u8;
            tbl.cfis[9] = ((lba >> 32) & 0xFF) as u8;
            tbl.cfis[10] = ((lba >> 40) & 0xFF) as u8;

            tbl.cfis[12] = (count & 0xFF) as u8;
            tbl.cfis[13] = ((count >> 8) & 0xFF) as u8;

            tbl.prdt[0].dba = (self.dma_buf_phys & 0xFFFF_FFFF) as u32;
            tbl.prdt[0].dbau = ((self.dma_buf_phys >> 32) & 0xFFFF_FFFF) as u32;
            tbl.prdt[0].dbc = bytes_to_read - 1;

            let hdr = &mut *cmd_hdr_virt;
            hdr.flags = 5;
            hdr.prdtl = 1;
            hdr.prdbc = 0;
            hdr.ctba = (self.cmd_table_phys & 0xFFFF_FFFF) as u32;
            hdr.ctbau = ((self.cmd_table_phys >> 32) & 0xFFFF_FFFF) as u32;
        }

        self.issue_command(0)?;

        // Copy DMA buffer to caller
        let dma_src = unsafe {
            core::slice::from_raw_parts(phys_to_virt(self.dma_buf_phys) as *const u8, bytes_to_read as usize)
        };
        buf[..bytes_to_read as usize].copy_from_slice(dma_src);

        Ok(())
    }

    pub fn write_sector_dma(&self, lba: u64, count: u32, buf: &[u8]) -> Result<(), &'static str> {
        let bytes_to_write = count * 512;
        if count == 0 || bytes_to_write as usize > buf.len() {
            return Err("Invalid buffer length for AHCI write");
        }

        // Copy caller data to DMA buffer
        let dma_dst = unsafe {
            core::slice::from_raw_parts_mut(phys_to_virt(self.dma_buf_phys) as *mut u8, bytes_to_write as usize)
        };
        dma_dst.copy_from_slice(&buf[..bytes_to_write as usize]);

        let cmd_hdr_virt = phys_to_virt(self.cmd_list_phys) as *mut AhciCmdHeader;
        let cmd_tbl_virt = phys_to_virt(self.cmd_table_phys) as *mut AhciCmdTable;

        unsafe {
            core::ptr::write_bytes(cmd_tbl_virt, 0, 1);
            let tbl = &mut *cmd_tbl_virt;

            tbl.cfis[0] = FIS_TYPE_REG_H2D;
            tbl.cfis[1] = 0x80;
            tbl.cfis[2] = ATA_CMD_WRITE_DMA_EX;

            tbl.cfis[4] = (lba & 0xFF) as u8;
            tbl.cfis[5] = ((lba >> 8) & 0xFF) as u8;
            tbl.cfis[6] = ((lba >> 16) & 0xFF) as u8;
            tbl.cfis[7] = 0x40;

            tbl.cfis[8] = ((lba >> 24) & 0xFF) as u8;
            tbl.cfis[9] = ((lba >> 32) & 0xFF) as u8;
            tbl.cfis[10] = ((lba >> 40) & 0xFF) as u8;

            tbl.cfis[12] = (count & 0xFF) as u8;
            tbl.cfis[13] = ((count >> 8) & 0xFF) as u8;

            tbl.prdt[0].dba = (self.dma_buf_phys & 0xFFFF_FFFF) as u32;
            tbl.prdt[0].dbau = ((self.dma_buf_phys >> 32) & 0xFFFF_FFFF) as u32;
            tbl.prdt[0].dbc = bytes_to_write - 1;

            let hdr = &mut *cmd_hdr_virt;
            hdr.flags = 5 | (1 << 6); // Bit 6 = Write
            hdr.prdtl = 1;
            hdr.prdbc = 0;
            hdr.ctba = (self.cmd_table_phys & 0xFFFF_FFFF) as u32;
            hdr.ctbau = ((self.cmd_table_phys >> 32) & 0xFFFF_FFFF) as u32;
        }

        self.issue_command(0)?;
        Ok(())
    }
}

pub struct AhciDisk {
    pub name: String,
    pub port: IrqSafeSpinlock<AhciPort>,
}

impl BlockDevice for AhciDisk {
    fn name(&self) -> &str {
        &self.name
    }

    fn read_sectors(&self, lba: u64, count: u32, buf: &mut [u8]) -> Result<(), &'static str> {
        self.port.lock().read_sector_dma(lba, count, buf)
    }

    fn write_sectors(&self, lba: u64, count: u32, buf: &[u8]) -> Result<(), &'static str> {
        let is_panicked = crate::arch::x86_64::vga::PANICKED.load(core::sync::atomic::Ordering::Relaxed);
        if is_panicked {
            unsafe { self.port.force_unlock(); }
        }
        self.port.lock().write_sector_dma(lba, count, buf)
    }

    fn total_sectors(&self) -> u64 {
        self.port.lock().total_sectors
    }
}

pub fn init() {
    let ahci_devs: alloc::vec::Vec<_> = {
        let pci = PCI.lock();
        pci.devices
            .iter()
            .filter(|dev| dev.class_code == 0x01 && dev.subclass == 0x06 && dev.prog_if == 0x01)
            .cloned()
            .collect()
    };

    if ahci_devs.is_empty() {
        kinfo!("AHCI: No SATA AHCI controller discovered on PCI bus.");
        return;
    }

    for dev in ahci_devs {
        let abar_phys = dev.read_bar(5);
        if abar_phys == 0 {
            kwarn!("AHCI: Controller at Bus {}:{}:{} has null BAR5 ABAR; skipping.", dev.bus, dev.device, dev.function);
            continue;
        }
        kinfo!(
            "AHCI SATA Controller Found: Vendor 0x{:04X}, Device 0x{:04X}, Bus {}:{}:{}, ABAR: 0x{:X}",
            dev.vendor_id,
            dev.device_id,
            dev.bus,
            dev.device,
            dev.function,
            abar_phys
        );

        // Enable PCI Bus Mastering and Memory Space
        unsafe {
            PciBus::enable_bus_mastering(dev.bus, dev.device, dev.function);
        }

        let mmio_virt = match map_mmio_range(abar_phys, 8192) {
            Ok(v) => v,
            Err(e) => {
                kwarn!("Failed to map AHCI ABAR MMIO: {}", e);
                continue;
            }
        };

        // Read Generic Host Control
        let ghc_ptr = (mmio_virt + HOST_GHC as u64) as *mut u32;
        let pi_ptr = (mmio_virt + HOST_PI as u64) as *const u32;
        let vs_ptr = (mmio_virt + HOST_VS as u64) as *const u32;

        unsafe {
            // Enable AHCI mode (GHC.AE = 1)
            core::ptr::write_volatile(ghc_ptr, core::ptr::read_volatile(ghc_ptr) | (1 << 31));
        }

        let pi = unsafe { core::ptr::read_volatile(pi_ptr) };
        let vs = unsafe { core::ptr::read_volatile(vs_ptr) };
        kinfo!("AHCI Controller Online: Version {}.{}, Implemented Ports Mask: 0x{:08X}", (vs >> 16), vs & 0xFFFF, pi);

        let mut disk_idx = 0;
        for port_no in 0..32 {
            if (pi & (1 << port_no)) != 0 {
                let port_base = mmio_virt + 0x100 + (port_no as u64 * 0x80);
                let ssts = unsafe { core::ptr::read_volatile((port_base + PORT_SSTS as u64) as *const u32) };
                let sig = unsafe { core::ptr::read_volatile((port_base + PORT_SIG as u64) as *const u32) };

                // DET: bits 0..3 == 3, IPM: bits 8..11 == 1
                let det = ssts & 0x0F;
                let ipm = (ssts >> 8) & 0x0F;

                if det == 3 && ipm == 1 && sig == SATA_SIG_ATA {
                    let (Some(cmd_list_frame), Some(fis_frame), Some(cmd_table_frame), Some(dma_buf_frame)) = (
                        alloc_zeroed_frame(),
                        alloc_zeroed_frame(),
                        alloc_zeroed_frame(),
                        alloc_zeroed_frame(),
                    ) else {
                        kwarn!("AHCI: Out of DMA frames for port {}", port_no);
                        continue;
                    };

                    let mut port = AhciPort {
                        port_no,
                        mmio_base: mmio_virt,
                        cmd_list_phys: cmd_list_frame,
                        fis_phys: fis_frame,
                        cmd_table_phys: cmd_table_frame,
                        dma_buf_phys: dma_buf_frame,
                        total_sectors: 0,
                        model: String::new(),
                    };

                    port.stop_port();

                    unsafe {
                        port.write_reg(PORT_CLB, (cmd_list_frame & 0xFFFF_FFFF) as u32);
                        port.write_reg(PORT_CLBU, ((cmd_list_frame >> 32) & 0xFFFF_FFFF) as u32);
                        port.write_reg(PORT_FB, (fis_frame & 0xFFFF_FFFF) as u32);
                        port.write_reg(PORT_FBU, ((fis_frame >> 32) & 0xFFFF_FFFF) as u32);
                        port.write_reg(PORT_SERR, 0xFFFF_FFFF);
                        port.write_reg(PORT_IS, 0xFFFF_FFFF);
                    }

                    port.start_port();

                    let disk_name = alloc::format!("sda{}", disk_idx);
                    if let Ok(sectors) = port.identify() {
                        kinfo!(
                            "AHCI Port {}: Attached SATA Drive '{}' ({} sectors, {} MB)",
                            port_no,
                            port.model,
                            sectors,
                            (sectors * 512) / (1024 * 1024)
                        );
                        let disk = Arc::new(AhciDisk {
                            name: disk_name,
                            port: IrqSafeSpinlock::new(port),
                        });
                        crate::drivers::storage::STORAGE_MANAGER.lock().register(disk);
                        disk_idx += 1;
                    }
                }
            }
        }
        break;
    }
}
