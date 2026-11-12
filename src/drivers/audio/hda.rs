extern crate alloc;
use alloc::sync::Arc;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};
use crate::drivers::pci::{PciBus, PciDevice};
use crate::mm::pmm::{alloc_zeroed_frame, PMM};
use crate::mm::{phys_to_virt, PAGE_SIZE};
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

// Intel HDA Controller Global Registers (relative to BAR0)
const REG_GCAP: u64 = 0x00;      // Global Capabilities (2 bytes)
const REG_GCTL: u64 = 0x08;      // Global Control (4 bytes)
const REG_STATESTS: u64 = 0x0E;  // State Change Status (2 bytes)
const REG_ICOI: u64 = 0x60;      // Immediate Command Output Interface (4 bytes)
const REG_ICII: u64 = 0x64;      // Immediate Command Input Interface (4 bytes)
const REG_ICIS: u64 = 0x68;      // Immediate Command Status (2 bytes)

// Stream Descriptor Register Offsets (relative to Stream Base)
const SD_CTL: u64 = 0x00;        // Stream Descriptor Control (4 bytes / 24-bit + RO)
const SD_STS: u64 = 0x03;        // Stream Descriptor Status (1 byte, W1C 0x1C)
const SD_LPIB: u64 = 0x04;       // Link Position in Buffer (4 bytes)
const SD_CBL: u64 = 0x08;        // Cyclic Buffer Length (4 bytes)
const SD_LVI: u64 = 0x0C;        // Last Valid Index (2 bytes)
const SD_FMT: u64 = 0x12;        // Stream Format (2 bytes)
const SD_BDLPL: u64 = 0x18;      // BDL Pointer Lower Base Address (4 bytes)
const SD_BDLPU: u64 = 0x1C;      // BDL Pointer Upper Base Address (4 bytes)

const SD_CTL_SRST: u32 = 1 << 0;
const SD_CTL_RUN: u32 = 1 << 1;
const SD_CTL_IOCE: u32 = 1 << 2;
const SD_CTL_STRM_SHIFT: u32 = 20;

pub const HDA_BDL_ENTRIES: usize = 16;
pub const HDA_RING_FRAMES: usize = 16;
pub const HDA_RING_BYTES: usize = HDA_RING_FRAMES * PAGE_SIZE; // 65536 bytes

/// Intel HDA Buffer Descriptor List Entry (16 bytes, BDL table must be 128-byte aligned).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct BdlEntry {
    pub addr_low: u32,
    pub addr_high: u32,
    pub length: u32,
    pub ioc: u32, // Interrupt on Completion (bit 0)
}

/// Physical page-aligned BDL + cyclic PCM DMA ring allocation (zero-copy hardware DMA backing).
pub struct HdaDmaRing {
    pub bdl_phys: u64,
    pub bdl_virt: u64,
    pub pcm_phys: u64,
    pub pcm_virt: u64,
    pub ring_bytes: usize,
    pub bdl_entries: usize,
}

impl HdaDmaRing {
    /// Allocate a 4 KiB page-aligned BDL frame (>128-byte aligned per Intel HDA spec)
    /// and 16 contiguous 4 KiB physical PCM frames (64 KiB), then populate all BDL entries.
    pub fn allocate() -> Result<Self, &'static str> {
        let bdl_phys = alloc_zeroed_frame().ok_or("Out of physical frames for HDA BDL")?;
        let pcm_phys = match PMM.lock().alloc_contiguous_frames(HDA_RING_FRAMES) {
            Some(p) => p,
            None => {
                PMM.lock().free_frame(bdl_phys);
                return Err("Out of contiguous physical frames for HDA PCM DMA ring");
            }
        };

        let bdl_virt = phys_to_virt(bdl_phys);
        let pcm_virt = phys_to_virt(pcm_phys);

        unsafe {
            core::ptr::write_bytes(pcm_virt as *mut u8, 0, HDA_RING_BYTES);
            let entries = bdl_virt as *mut BdlEntry;
            for i in 0..HDA_BDL_ENTRIES {
                let buf_phys = pcm_phys + (i * PAGE_SIZE) as u64;
                core::ptr::write_volatile(
                    entries.add(i),
                    BdlEntry {
                        addr_low: (buf_phys & 0xFFFF_FFFF) as u32,
                        addr_high: (buf_phys >> 32) as u32,
                        length: PAGE_SIZE as u32,
                        ioc: if i == 7 || i == (HDA_BDL_ENTRIES - 1) { 1 } else { 0 },
                    },
                );
            }
        }
        fence(Ordering::SeqCst);

        Ok(Self {
            bdl_phys,
            bdl_virt,
            pcm_phys,
            pcm_virt,
            ring_bytes: HDA_RING_BYTES,
            bdl_entries: HDA_BDL_ENTRIES,
        })
    }

    pub fn read_entry(&self, index: usize) -> Option<BdlEntry> {
        if index >= self.bdl_entries {
            return None;
        }
        unsafe { Some(core::ptr::read_volatile((self.bdl_virt as *const BdlEntry).add(index))) }
    }
}

impl Drop for HdaDmaRing {
    fn drop(&mut self) {
        let mut pmm = PMM.lock();
        pmm.free_frame(self.bdl_phys);
        for i in 0..HDA_RING_FRAMES {
            pmm.free_frame(self.pcm_phys + (i * PAGE_SIZE) as u64);
        }
    }
}

pub struct HdaController {
    pub pci: PciDevice,
    pub mmio_base: u64,
    pub num_iss: u8,
    pub num_oss: u8,
    pub output_stream_base: u64,
    pub is_ready: bool,
    pub dma_ring: HdaDmaRing,
    stream_lock: IrqSafeSpinlock<()>,
}

unsafe impl Send for HdaController {}
unsafe impl Sync for HdaController {}

impl HdaController {
    pub fn new(pci: PciDevice) -> Result<Arc<Self>, &'static str> {
        let mmio_phys = pci.read_bar(0);
        if mmio_phys == 0 {
            return Err("Invalid Intel HDA MMIO base address (BAR0 is 0)");
        }

        // Map Intel HDA MMIO space (16 KiB)
        let mmio_base = crate::mm::vmm::map_mmio_range(mmio_phys, 16 * 1024)?;

        // Allocate physical page-aligned BDL + 64 KiB contiguous PCM DMA ring
        let dma_ring = HdaDmaRing::allocate()?;

        unsafe {
            PciBus::enable_bus_mastering(pci.bus, pci.device, pci.function);

            let mut controller = Self {
                pci,
                mmio_base,
                num_iss: 0,
                num_oss: 0,
                output_stream_base: 0,
                is_ready: false,
                dma_ring,
                stream_lock: IrqSafeSpinlock::new(()),
            };

            controller.init_hardware()?;
            Ok(Arc::new(controller))
        }
    }

    unsafe fn send_immediate_verb(&self, codec_addr: u8, nid: u8, verb: u32) -> Option<u32> {
        let icis_ptr = (self.mmio_base + REG_ICIS) as *mut u16;
        let icoi_ptr = (self.mmio_base + REG_ICOI) as *mut u32;
        let icii_ptr = (self.mmio_base + REG_ICII) as *const u32;

        // Wait if Immediate Command Busy is set
        let mut timeout = 2_000;
        while (read_volatile(icis_ptr) & 0x0001) != 0 && timeout > 0 {
            core::hint::spin_loop();
            timeout -= 1;
        }
        if timeout == 0 {
            return None;
        }

        // Clear prior Immediate Result Valid (bit 1) + submit verb
        write_volatile(icis_ptr, 0x0002);
        let cmd = ((codec_addr as u32 & 0x0F) << 28)
            | ((nid as u32) << 20)
            | (verb & 0x000F_FFFF);
        write_volatile(icoi_ptr, cmd);
        write_volatile(icis_ptr, 0x0001);

        timeout = 4_000;
        while timeout > 0 {
            let st = read_volatile(icis_ptr);
            if (st & 0x0003) == 0x0002 {
                return Some(read_volatile(icii_ptr));
            }
            core::hint::spin_loop();
            timeout -= 1;
        }
        None
    }

    unsafe fn configure_first_codec(&self) {
        let statests = read_volatile((self.mmio_base + REG_STATESTS) as *const u16);
        if statests == 0 {
            return;
        }

        for cad in 0u8..4 {
            if (statests & (1 << cad)) == 0 {
                continue;
            }
            // Query Root Node subordinate node count (param 0x04)
            let sub = match self.send_immediate_verb(cad, 0, 0xF0004) {
                Some(v) => v,
                None => continue,
            };
            let start_afg = ((sub >> 16) & 0xFF) as u8;
            let count_afg = (sub & 0xFF) as u8;

            for afg in start_afg..start_afg.saturating_add(count_afg) {
                let func_grp = self.send_immediate_verb(cad, afg, 0xF0005).unwrap_or(0);
                if (func_grp & 0xFF) != 0x01 {
                    continue;
                }
                // Power D0 on AFG (verb 0x705, data 0x00)
                let _ = self.send_immediate_verb(cad, afg, 0x70500);

                let widgets = self.send_immediate_verb(cad, afg, 0xF0004).unwrap_or(0);
                let start_nid = ((widgets >> 16) & 0xFF) as u8;
                let num_nids = (widgets & 0xFF) as u8;

                let mut dac_configured = false;
                let mut pin_configured = false;
                for nid in start_nid..start_nid.saturating_add(num_nids) {
                    let wcaps = self.send_immediate_verb(cad, nid, 0xF0009).unwrap_or(0);
                    let wtype = (wcaps >> 20) & 0x0F;
                    if wtype == 0x00 && !dac_configured {
                        // Audio Output (DAC): power D0, bind stream 1 ch 0, set format 0x4011
                        let _ = self.send_immediate_verb(cad, nid, 0x70500);
                        let _ = self.send_immediate_verb(cad, nid, 0x70610);
                        let _ = self.send_immediate_verb(cad, nid, 0x24011);
                        let _ = self.send_immediate_verb(cad, nid, 0x3B07F); // unmute out amp L/R
                        dac_configured = true;
                    } else if wtype == 0x04 && !pin_configured {
                        // Pin Complex (Line Out / Speaker / HP): power D0 + Pin Control OUT (0x40) + unmute
                        let pincaps = self.send_immediate_verb(cad, nid, 0xF000C).unwrap_or(0);
                        if (pincaps & (1 << 4)) != 0 {
                            let _ = self.send_immediate_verb(cad, nid, 0x70500);
                            let _ = self.send_immediate_verb(cad, nid, 0x70740);
                            let _ = self.send_immediate_verb(cad, nid, 0x3B07F);
                            pin_configured = true;
                        }
                    }
                    if dac_configured && pin_configured {
                        return;
                    }
                }
            }
        }
    }

    unsafe fn reset_output_stream(&self, sd_base: u64) {
        let sd_ctl_ptr = (sd_base + SD_CTL) as *mut u32;
        let sd_sts_ptr = (sd_base + SD_STS) as *mut u8;

        // Clear RUN bit and wait for engine halt
        let mut ctl = read_volatile(sd_ctl_ptr);
        ctl &= !SD_CTL_RUN;
        write_volatile(sd_ctl_ptr, ctl);
        let mut timeout = 10_000;
        while (read_volatile(sd_ctl_ptr) & SD_CTL_RUN) != 0 && timeout > 0 {
            core::hint::spin_loop();
            timeout -= 1;
        }

        // Assert Stream Reset (SRST = 1), wait for SRST latch, then deassert
        ctl = read_volatile(sd_ctl_ptr) | SD_CTL_SRST;
        write_volatile(sd_ctl_ptr, ctl);
        timeout = 10_000;
        while (read_volatile(sd_ctl_ptr) & SD_CTL_SRST) == 0 && timeout > 0 {
            core::hint::spin_loop();
            timeout -= 1;
        }

        ctl = read_volatile(sd_ctl_ptr) & !SD_CTL_SRST;
        write_volatile(sd_ctl_ptr, ctl);
        timeout = 10_000;
        while (read_volatile(sd_ctl_ptr) & SD_CTL_SRST) != 0 && timeout > 0 {
            core::hint::spin_loop();
            timeout -= 1;
        }

        // Clear write-1-to-clear stream status flags (BCIS | FIFOE | DESE = 0x1C)
        write_volatile(sd_sts_ptr, 0x1C);
    }

    unsafe fn program_output_stream(&self, sd_base: u64, cbl_bytes: u32, lvi: u16) {
        let sd_ctl_ptr = (sd_base + SD_CTL) as *mut u32;
        let sd_bdlpl_ptr = (sd_base + SD_BDLPL) as *mut u32;
        let sd_bdlpu_ptr = (sd_base + SD_BDLPU) as *mut u32;
        let sd_cbl_ptr = (sd_base + SD_CBL) as *mut u32;
        let sd_lvi_ptr = (sd_base + SD_LVI) as *mut u16;
        let sd_fmt_ptr = (sd_base + SD_FMT) as *mut u16;

        // Program BDL physical base (64-bit) while RUN == 0 per Intel HDA Spec Section 3.3.41
        let bdl_low = (self.dma_ring.bdl_phys & 0xFFFF_FFFF) as u32;
        let bdl_high = (self.dma_ring.bdl_phys >> 32) as u32;
        write_volatile(sd_bdlpl_ptr, bdl_low);
        write_volatile(sd_bdlpu_ptr, bdl_high);

        // Cyclic Buffer Length, Last Valid Index, Format (44.1kHz, 16-bit, stereo = 0x4011)
        write_volatile(sd_cbl_ptr, cbl_bytes);
        write_volatile(sd_lvi_ptr, lvi);
        write_volatile(sd_fmt_ptr, 0x4011);

        // Assign Stream Number = 1 (bits 23:20)
        let ctl = (read_volatile(sd_ctl_ptr) & !(0x0F << SD_CTL_STRM_SHIFT)) | (1 << SD_CTL_STRM_SHIFT);
        write_volatile(sd_ctl_ptr, ctl);
    }

    unsafe fn init_hardware(&mut self) -> Result<(), &'static str> {
        let gctl_ptr = (self.mmio_base + REG_GCTL) as *mut u32;

        // 1. Enter reset state (CRST = 0)
        write_volatile(gctl_ptr, read_volatile(gctl_ptr) & !0x01);
        let mut timeout = 20_000;
        while (read_volatile(gctl_ptr) & 0x01) != 0 && timeout > 0 {
            core::hint::spin_loop();
            timeout -= 1;
        }

        // 2. Bring controller out of reset (CRST = 1)
        write_volatile(gctl_ptr, read_volatile(gctl_ptr) | 0x01);
        timeout = 20_000;
        while (read_volatile(gctl_ptr) & 0x01) == 0 && timeout > 0 {
            core::hint::spin_loop();
            timeout -= 1;
        }
        if (read_volatile(gctl_ptr) & 0x01) == 0 {
            return Err("Intel HDA controller failed to exit CRST reset");
        }

        // Allow codecs brief time to assert STATESTS frame wakeup
        for _ in 0..2_000 {
            core::hint::spin_loop();
        }

        // 3. Read GCAP to determine number of input/output stream descriptors
        let gcap = read_volatile((self.mmio_base + REG_GCAP) as *const u16);
        self.num_iss = ((gcap >> 8) & 0x0F) as u8;
        self.num_oss = ((gcap >> 12) & 0x0F) as u8;
        if self.num_oss == 0 {
            return Err("Intel HDA controller reports zero output streams");
        }

        // Output streams begin immediately after input streams: offset 0x80 + (num_iss * 0x20)
        self.output_stream_base = self.mmio_base + 0x80 + (self.num_iss as u64 * 0x20);

        // 4. Configure DAC / Pin codec routing if an HDA codec responded
        self.configure_first_codec();

        // 5. Reset Output Stream 0 and program real BDL physical DMA ring descriptors
        let sd_base = self.output_stream_base;
        self.reset_output_stream(sd_base);
        self.program_output_stream(sd_base, HDA_RING_BYTES as u32, (HDA_BDL_ENTRIES - 1) as u16);

        self.is_ready = true;
        let (bdl_lo, bdl_hi) = self.read_bdl_registers();
        kinfo!(
            "Intel HDA Hardware DMA Ring active (BAR0: 0x{:X}, Streams: {} In/{} Out, BDL: 0x{:08X}_{:08X}, PCM: 0x{:X})",
            self.mmio_base,
            self.num_iss,
            self.num_oss,
            bdl_hi,
            bdl_lo,
            self.dma_ring.pcm_phys
        );

        Ok(())
    }

    /// Copy 16-bit PCM samples into the page-aligned physical DMA ring and trigger asynchronous
    /// Intel HDA hardware DMA playback (SD_CTL.RUN = 1) without CPU busy-wait loops.
    pub fn play_pcm_samples(&self, samples: &[i16]) {
        if !self.is_ready || samples.is_empty() {
            return;
        }

        let _guard = self.stream_lock.lock();
        let sd_base = self.output_stream_base;

        unsafe {
            // Stop previous transfer cleanly before rewriting PCM ring descriptors
            let sd_ctl_ptr = (sd_base + SD_CTL) as *mut u32;
            let ctl = read_volatile(sd_ctl_ptr);
            if (ctl & SD_CTL_RUN) != 0 {
                write_volatile(sd_ctl_ptr, ctl & !SD_CTL_RUN);
                let mut timeout = 5_000;
                while (read_volatile(sd_ctl_ptr) & SD_CTL_RUN) != 0 && timeout > 0 {
                    core::hint::spin_loop();
                    timeout -= 1;
                }
            }

            // Copy up to HDA_RING_BYTES of 16-bit PCM samples into physical DMA memory; zero remainder
            let max_i16 = HDA_RING_BYTES / 2;
            let copy_i16 = samples.len().min(max_i16);
            let dst = self.dma_ring.pcm_virt as *mut i16;
            core::ptr::copy_nonoverlapping(samples.as_ptr(), dst, copy_i16);
            if copy_i16 < max_i16 {
                core::ptr::write_bytes(dst.add(copy_i16), 0, max_i16 - copy_i16);
            }
            fence(Ordering::SeqCst);

            // Re-arm stream registers & clear pending status bits
            write_volatile((sd_base + SD_STS) as *mut u8, 0x1C);
            self.program_output_stream(sd_base, HDA_RING_BYTES as u32, (HDA_BDL_ENTRIES - 1) as u16);

            // Start asynchronous autonomous hardware DMA (Stream 1 + IOCE + RUN); CPU returns immediately!
            let active_ctl = (1 << SD_CTL_STRM_SHIFT) | SD_CTL_IOCE | SD_CTL_RUN;
            write_volatile(sd_ctl_ptr, active_ctl);
            // Read back to flush posted PCIe MMIO write buffer
            let _ = read_volatile(sd_ctl_ptr);
        }
    }

    pub fn stop_stream(&self) {
        if !self.is_ready {
            return;
        }
        let _guard = self.stream_lock.lock();
        unsafe {
            let sd_ctl_ptr = (self.output_stream_base + SD_CTL) as *mut u32;
            write_volatile(sd_ctl_ptr, read_volatile(sd_ctl_ptr) & !SD_CTL_RUN);
            let _ = read_volatile(sd_ctl_ptr);
        }
    }

    pub fn is_stream_running(&self) -> bool {
        if !self.is_ready {
            return false;
        }
        for _ in 0..5_000 {
            if unsafe { (read_volatile((self.output_stream_base + SD_CTL) as *const u32) & SD_CTL_RUN) != 0 } {
                return true;
            }
            core::hint::spin_loop();
        }
        false
    }

    pub fn link_position(&self) -> u32 {
        if !self.is_ready {
            return 0;
        }
        unsafe { read_volatile((self.output_stream_base + SD_LPIB) as *const u32) }
    }

    pub fn read_bdl_registers(&self) -> (u32, u32) {
        if self.output_stream_base == 0 {
            return (0, 0);
        }
        unsafe {
            let low = read_volatile((self.output_stream_base + SD_BDLPL) as *const u32);
            let high = read_volatile((self.output_stream_base + SD_BDLPU) as *const u32);
            (low, high)
        }
    }
}
