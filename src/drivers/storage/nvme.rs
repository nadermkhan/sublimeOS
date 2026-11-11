extern crate alloc;
use alloc::string::String;
use alloc::sync::Arc;
use crate::drivers::pci::{PciBus, PCI};
use crate::drivers::storage::BlockDevice;
use crate::mm::phys_to_virt;
use crate::mm::pmm::PMM;
use crate::mm::vmm::map_mmio_range;
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

// NVMe Controller Registers (Offsets from BAR0)
pub const NVME_REG_CAP: usize = 0x00;     // Controller Capabilities (64-bit)
pub const NVME_REG_VS: usize = 0x08;      // Version (32-bit)
pub const NVME_REG_INTMS: usize = 0x0C;   // Interrupt Mask Set
pub const NVME_REG_INTMC: usize = 0x10;   // Interrupt Mask Clear
pub const NVME_REG_CC: usize = 0x14;      // Controller Configuration
pub const NVME_REG_CSTS: usize = 0x1C;    // Controller Status
pub const NVME_REG_AQA: usize = 0x24;     // Admin Queue Attributes
pub const NVME_REG_ASQ: usize = 0x28;     // Admin Submission Queue Base (64-bit)
pub const NVME_REG_ACQ: usize = 0x30;     // Admin Completion Queue Base (64-bit)

// NVMe Admin Opcodes
pub const NVME_ADMIN_OP_CREATE_SQ: u8 = 0x01;
pub const NVME_ADMIN_OP_CREATE_CQ: u8 = 0x05;
pub const NVME_ADMIN_OP_IDENTIFY: u8 = 0x06;

// NVMe NVM Opcodes
pub const NVME_NVM_OP_WRITE: u8 = 0x01;
pub const NVME_NVM_OP_READ: u8 = 0x02;

// Submission Queue Entry (64 bytes)
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct NvmeSqEntry {
    pub cdw0: u32,       // Opcode, Fuse, Command ID
    pub nsid: u32,       // Namespace ID
    pub reserved: u64,
    pub mptr: u64,       // Metadata Pointer
    pub prp1: u64,       // PRP Entry 1
    pub prp2: u64,       // PRP Entry 2
    pub cdw10: u32,
    pub cdw11: u32,
    pub cdw12: u32,
    pub cdw13: u32,
    pub cdw14: u32,
    pub cdw15: u32,
}

// Completion Queue Entry (16 bytes)
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct NvmeCqEntry {
    pub cdw0: u32,       // Command-specific result
    pub reserved: u32,
    pub sq_head: u16,    // SQ Head Pointer
    pub sq_id: u16,      // SQ Identifier
    pub cid: u16,        // Command Identifier
    pub status: u16,     // Status Field (bit 0 = Phase tag)
}

pub struct NvmeController {
    pub mmio_base: u64,
    pub doorbell_stride: usize,
    pub admin_sq_phys: u64,
    pub admin_cq_phys: u64,
    pub io_sq_phys: u64,
    pub io_cq_phys: u64,
    pub dma_buf_phys: u64,
    pub admin_sq_tail: u16,
    pub admin_cq_head: u16,
    pub admin_phase: u16,
    pub io_sq_tail: u16,
    pub io_cq_head: u16,
    pub io_phase: u16,
    pub total_sectors: u64,
    pub sector_size: usize,
    pub model: String,
}

impl NvmeController {
    pub unsafe fn read_reg32(&self, offset: usize) -> u32 {
        let ptr = (self.mmio_base + offset as u64) as *const u32;
        core::ptr::read_volatile(ptr)
    }

    pub unsafe fn write_reg32(&self, offset: usize, val: u32) {
        let ptr = (self.mmio_base + offset as u64) as *mut u32;
        core::ptr::write_volatile(ptr, val);
    }

    pub unsafe fn read_reg64(&self, offset: usize) -> u64 {
        let ptr = (self.mmio_base + offset as u64) as *const u64;
        core::ptr::read_volatile(ptr)
    }

    pub unsafe fn write_reg64(&self, offset: usize, val: u64) {
        let ptr = (self.mmio_base + offset as u64) as *mut u64;
        core::ptr::write_volatile(ptr, val);
    }

    pub fn ring_doorbell(&self, qid: u32, is_cq: bool, val: u32) {
        let offset = 0x1000 + ((2 * qid + if is_cq { 1 } else { 0 }) as usize * self.doorbell_stride);
        unsafe {
            self.write_reg32(offset, val);
        }
    }

    pub fn submit_admin_cmd(&mut self, mut cmd: NvmeSqEntry) -> Result<NvmeCqEntry, &'static str> {
        let sq_virt = phys_to_virt(self.admin_sq_phys) as *mut NvmeSqEntry;
        let cq_virt = phys_to_virt(self.admin_cq_phys) as *mut NvmeCqEntry;

        let cid = self.admin_sq_tail;
        cmd.cdw0 |= (cid as u32) << 16;

        unsafe {
            core::ptr::write(sq_virt.add(self.admin_sq_tail as usize), cmd);
        }

        self.admin_sq_tail = (self.admin_sq_tail + 1) % 64;
        self.ring_doorbell(0, false, self.admin_sq_tail as u32);

        // Poll Admin CQ for completion
        for _ in 0..2000 {
            let cqe = unsafe { core::ptr::read(cq_virt.add(self.admin_cq_head as usize)) };
            let phase = cqe.status & 1;
            if phase == self.admin_phase {
                self.admin_cq_head = (self.admin_cq_head + 1) % 64;
                if self.admin_cq_head == 0 {
                    self.admin_phase ^= 1;
                }
                self.ring_doorbell(0, true, self.admin_cq_head as u32);

                let sc = (cqe.status >> 1) & 0xFF;
                if sc != 0 {
                    return Err("NVMe Admin Command reported error status");
                }
                return Ok(cqe);
            }
            crate::arch::x86_64::pit::sleep_ms(1);
        }

        Err("NVMe Admin Command timed out")
    }

    pub fn submit_io_cmd(&mut self, mut cmd: NvmeSqEntry) -> Result<NvmeCqEntry, &'static str> {
        let sq_virt = phys_to_virt(self.io_sq_phys) as *mut NvmeSqEntry;
        let cq_virt = phys_to_virt(self.io_cq_phys) as *mut NvmeCqEntry;

        let cid = self.io_sq_tail;
        cmd.cdw0 |= (cid as u32) << 16;

        unsafe {
            core::ptr::write(sq_virt.add(self.io_sq_tail as usize), cmd);
        }

        self.io_sq_tail = (self.io_sq_tail + 1) % 64;
        self.ring_doorbell(1, false, self.io_sq_tail as u32);

        // Poll I/O CQ for completion
        for _ in 0..2000 {
            let cqe = unsafe { core::ptr::read(cq_virt.add(self.io_cq_head as usize)) };
            let phase = cqe.status & 1;
            if phase == self.io_phase {
                self.io_cq_head = (self.io_cq_head + 1) % 64;
                if self.io_cq_head == 0 {
                    self.io_phase ^= 1;
                }
                self.ring_doorbell(1, true, self.io_cq_head as u32);

                let sc = (cqe.status >> 1) & 0xFF;
                if sc != 0 {
                    return Err("NVMe I/O Command reported error status");
                }
                return Ok(cqe);
            }
            crate::arch::x86_64::pit::sleep_ms(1);
        }

        Err("NVMe I/O Command timed out")
    }

    pub fn init_queues(&mut self) -> Result<(), &'static str> {
        // 1. Identify Controller to extract model and namespace count
        let mut id_cmd = unsafe { core::mem::zeroed::<NvmeSqEntry>() };
        id_cmd.cdw0 = NVME_ADMIN_OP_IDENTIFY as u32;
        id_cmd.nsid = 0;
        id_cmd.prp1 = self.dma_buf_phys;
        id_cmd.cdw10 = 1; // CNS = 1: Identify Controller

        self.submit_admin_cmd(id_cmd)?;

        let id_ctrl = phys_to_virt(self.dma_buf_phys) as *const u8;
        let model_slice = unsafe { core::slice::from_raw_parts(id_ctrl.add(24), 40) };
        let model_str = core::str::from_utf8(model_slice).unwrap_or("NVMe SSD").trim();
        self.model = String::from(model_str);

        // 2. Identify Namespace 1
        let mut id_ns_cmd = unsafe { core::mem::zeroed::<NvmeSqEntry>() };
        id_ns_cmd.cdw0 = NVME_ADMIN_OP_IDENTIFY as u32;
        id_ns_cmd.nsid = 1;
        id_ns_cmd.prp1 = self.dma_buf_phys;
        id_ns_cmd.cdw10 = 0; // CNS = 0: Identify Namespace

        self.submit_admin_cmd(id_ns_cmd)?;

        let id_ns = phys_to_virt(self.dma_buf_phys) as *const u8;
        let nsze = unsafe { *(id_ns as *const u64) };
        self.total_sectors = if nsze > 0 { nsze } else { 4194304 }; // Default 2 GB
        self.sector_size = 512;

        // 3. Create I/O Completion Queue (QID = 1, size = 64)
        let mut create_cq_cmd = unsafe { core::mem::zeroed::<NvmeSqEntry>() };
        create_cq_cmd.cdw0 = NVME_ADMIN_OP_CREATE_CQ as u32;
        create_cq_cmd.prp1 = self.io_cq_phys;
        create_cq_cmd.cdw10 = ((64 - 1) << 16) | 1; // Size = 64, QID = 1
        create_cq_cmd.cdw11 = 1; // Physically contiguous (PC = 1)
        self.submit_admin_cmd(create_cq_cmd)?;

        // 4. Create I/O Submission Queue (QID = 1, CQID = 1, size = 64)
        let mut create_sq_cmd = unsafe { core::mem::zeroed::<NvmeSqEntry>() };
        create_sq_cmd.cdw0 = NVME_ADMIN_OP_CREATE_SQ as u32;
        create_sq_cmd.prp1 = self.io_sq_phys;
        create_sq_cmd.cdw10 = ((64 - 1) << 16) | 1; // Size = 64, QID = 1
        create_sq_cmd.cdw11 = (1 << 16) | 1; // CQID = 1, Physically contiguous (PC = 1)
        self.submit_admin_cmd(create_sq_cmd)?;

        Ok(())
    }

    pub fn read_sector(&mut self, lba: u64, count: u32, buf: &mut [u8]) -> Result<(), &'static str> {
        let bytes_to_read = count as usize * self.sector_size;
        if count == 0 || bytes_to_read > buf.len() {
            return Err("Invalid buffer length for NVMe read");
        }

        let mut read_cmd = unsafe { core::mem::zeroed::<NvmeSqEntry>() };
        read_cmd.cdw0 = NVME_NVM_OP_READ as u32;
        read_cmd.nsid = 1;
        read_cmd.prp1 = self.dma_buf_phys;
        read_cmd.cdw10 = (lba & 0xFFFF_FFFF) as u32;
        read_cmd.cdw11 = ((lba >> 32) & 0xFFFF_FFFF) as u32;
        read_cmd.cdw12 = count - 1; // 0-based number of logical blocks

        self.submit_io_cmd(read_cmd)?;

        let dma_src = unsafe {
            core::slice::from_raw_parts(phys_to_virt(self.dma_buf_phys) as *const u8, bytes_to_read)
        };
        buf[..bytes_to_read].copy_from_slice(dma_src);

        Ok(())
    }

    pub fn write_sector(&mut self, lba: u64, count: u32, buf: &[u8]) -> Result<(), &'static str> {
        let bytes_to_write = count as usize * self.sector_size;
        if count == 0 || bytes_to_write > buf.len() {
            return Err("Invalid buffer length for NVMe write");
        }

        let dma_dst = unsafe {
            core::slice::from_raw_parts_mut(phys_to_virt(self.dma_buf_phys) as *mut u8, bytes_to_write)
        };
        dma_dst.copy_from_slice(&buf[..bytes_to_write]);

        let mut write_cmd = unsafe { core::mem::zeroed::<NvmeSqEntry>() };
        write_cmd.cdw0 = NVME_NVM_OP_WRITE as u32;
        write_cmd.nsid = 1;
        write_cmd.prp1 = self.dma_buf_phys;
        write_cmd.cdw10 = (lba & 0xFFFF_FFFF) as u32;
        write_cmd.cdw11 = ((lba >> 32) & 0xFFFF_FFFF) as u32;
        write_cmd.cdw12 = count - 1;

        self.submit_io_cmd(write_cmd)?;
        Ok(())
    }
}

pub struct NvmeDisk {
    pub name: String,
    pub controller: IrqSafeSpinlock<NvmeController>,
}

impl BlockDevice for NvmeDisk {
    fn name(&self) -> &str {
        &self.name
    }

    fn read_sectors(&self, lba: u64, count: u32, buf: &mut [u8]) -> Result<(), &'static str> {
        self.controller.lock().read_sector(lba, count, buf)
    }

    fn write_sectors(&self, lba: u64, count: u32, buf: &[u8]) -> Result<(), &'static str> {
        self.controller.lock().write_sector(lba, count, buf)
    }

    fn total_sectors(&self) -> u64 {
        self.controller.lock().total_sectors
    }

    fn sector_size(&self) -> usize {
        self.controller.lock().sector_size
    }
}

pub fn init() {
    let pci = PCI.lock();
    let mut found = false;

    for dev in pci.devices.iter() {
        if dev.class_code == 0x01 && dev.subclass == 0x08 && dev.prog_if == 0x02 {
            found = true;
            let bar0_phys = dev.read_bar(0);
            kinfo!(
                "PCIe NVMe Controller Found: Vendor 0x{:04X}, Device 0x{:04X}, Bus {}:{}:{}, BAR0: 0x{:X}",
                dev.vendor_id,
                dev.device_id,
                dev.bus,
                dev.device,
                dev.function,
                bar0_phys
            );

            unsafe {
                PciBus::enable_bus_mastering(dev.bus, dev.device, dev.function);
            }

            let mmio_virt = match map_mmio_range(bar0_phys, 16384) {
                Ok(v) => v,
                Err(e) => {
                    kinfo!("Failed to map NVMe BAR0 MMIO: {}", e);
                    continue;
                }
            };

            let cap_ptr = mmio_virt as *const u64;
            let cap = unsafe { core::ptr::read_volatile(cap_ptr) };
            let dstrd = ((cap >> 32) & 0x0F) as usize;
            let doorbell_stride = 4 << dstrd;
            let vs = unsafe { core::ptr::read_volatile((mmio_virt + NVME_REG_VS as u64) as *const u32) };

            kinfo!(
                "NVMe Controller Online: Version {}.{}.{}, Doorbell Stride: {} bytes",
                (vs >> 16) & 0xFFFF,
                (vs >> 8) & 0xFF,
                vs & 0xFF,
                doorbell_stride
            );

            // Allocate DMA frames
            let admin_sq_frame = PMM.lock().alloc_frame().unwrap_or(0);
            let admin_cq_frame = PMM.lock().alloc_frame().unwrap_or(0);
            let io_sq_frame = PMM.lock().alloc_frame().unwrap_or(0);
            let io_cq_frame = PMM.lock().alloc_frame().unwrap_or(0);
            let dma_buf_frame = PMM.lock().alloc_frame().unwrap_or(0);

            let mut controller = NvmeController {
                mmio_base: mmio_virt,
                doorbell_stride,
                admin_sq_phys: admin_sq_frame,
                admin_cq_phys: admin_cq_frame,
                io_sq_phys: io_sq_frame,
                io_cq_phys: io_cq_frame,
                dma_buf_phys: dma_buf_frame,
                admin_sq_tail: 0,
                admin_cq_head: 0,
                admin_phase: 1,
                io_sq_tail: 0,
                io_cq_head: 0,
                io_phase: 1,
                total_sectors: 0,
                sector_size: 512,
                model: String::new(),
            };

            unsafe {
                // Disable controller first (CC.EN = 0)
                let cc_ptr = (mmio_virt + NVME_REG_CC as u64) as *mut u32;
                let csts_ptr = (mmio_virt + NVME_REG_CSTS as u64) as *const u32;
                core::ptr::write_volatile(cc_ptr, 0);

                for _ in 0..1000 {
                    if (core::ptr::read_volatile(csts_ptr) & 1) == 0 {
                        break;
                    }
                    crate::arch::x86_64::pit::sleep_ms(1);
                }

                // Program Admin Queue Attributes (AQA: ASQS = 63, ACQS = 63)
                let aqa = (63 << 16) | 63;
                controller.write_reg32(NVME_REG_AQA, aqa);

                // Program ASQ and ACQ
                controller.write_reg64(NVME_REG_ASQ, admin_sq_frame);
                controller.write_reg64(NVME_REG_ACQ, admin_cq_frame);

                // Enable controller with IOSQES=6, IOCQES=4, EN=1
                let cc = 1 | (6 << 16) | (4 << 20);
                controller.write_reg32(NVME_REG_CC, cc);

                for _ in 0..1000 {
                    if (core::ptr::read_volatile(csts_ptr) & 1) != 0 {
                        break;
                    }
                    crate::arch::x86_64::pit::sleep_ms(1);
                }
            }

            if let Ok(()) = controller.init_queues() {
                kinfo!(
                    "NVMe Namespace 1 Online: Drive '{}' ({} sectors, {} MB)",
                    controller.model,
                    controller.total_sectors,
                    (controller.total_sectors * 512) / (1024 * 1024)
                );
                let disk = Arc::new(NvmeDisk {
                    name: String::from("nvme0n1"),
                    controller: IrqSafeSpinlock::new(controller),
                });
                crate::drivers::storage::STORAGE_MANAGER.lock().register(disk);
            }
            break;
        }
    }

    if !found {
        kinfo!("PCIe NVMe controller not detected on PCI bus.");
    }
}
