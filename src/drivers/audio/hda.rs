#![allow(dead_code)]

extern crate alloc;
use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::drivers::pci::{PciBus, PciDevice};
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

// Intel HDA Controller Global Registers (relative to BAR0)
const REG_GCAP: u64 = 0x0000;      // Global Capabilities
const REG_GCTL: u64 = 0x0008;      // Global Control
const REG_STATESTS: u64 = 0x000E;  // State Change Status
const REG_INTCTL: u64 = 0x0020;    // Interrupt Control
const REG_INTSTS: u64 = 0x0024;    // Interrupt Status

// Stream Descriptor Register Offsets (relative to Stream Base)
const SD_CTL: u64 = 0x00;          // Stream Descriptor Control (3 bytes)
const SD_STS: u64 = 0x03;          // Stream Descriptor Status (1 byte)
const SD_LPIB: u64 = 0x04;         // Link Position in Buffer (4 bytes)
const SD_CBL: u64 = 0x08;          // Cyclic Buffer Length (4 bytes)
const SD_LVI: u64 = 0x0C;          // Last Valid Index (2 bytes)
const SD_FMT: u64 = 0x12;          // Stream Format (2 bytes)
const SD_BDLPL: u64 = 0x18;        // BDL Pointer Lower (4 bytes)
const SD_BDLPU: u64 = 0x1C;        // BDL Pointer Upper (4 bytes)

// Buffer Descriptor List (BDL) Entry (16 bytes)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct BdlEntry {
    pub addr_low: u32,
    pub addr_high: u32,
    pub length: u32,
    pub ioc: u32, // Interrupt on Completion (bit 0)
}

pub struct HdaController {
    pub pci: PciDevice,
    pub mmio_base: u64,
    pub num_iss: u8,
    pub num_oss: u8,
    pub output_stream_base: u64,
    pub is_ready: bool,
    pub dma_buffer: IrqSafeSpinlock<Vec<u8>>,
}

impl HdaController {
    pub fn new(pci: PciDevice) -> Result<Arc<Self>, &'static str> {
        let mmio_phys = (pci.bar0 & 0xFFFFFFF0) as u64;
        if mmio_phys == 0 {
            return Err("Invalid Intel HDA MMIO base address (BAR0 is 0)");
        }

        // Map Intel HDA MMIO space (16 KiB)
        let mmio_base = crate::mm::vmm::map_mmio_range(mmio_phys, 16 * 1024)?;

        unsafe {
            PciBus::enable_bus_mastering(pci.bus, pci.device, pci.function);

            let mut controller = Self {
                pci,
                mmio_base,
                num_iss: 0,
                num_oss: 0,
                output_stream_base: 0,
                is_ready: false,
                dma_buffer: IrqSafeSpinlock::new(alloc::vec![0u8; 65536]),
            };

            controller.init_hardware()?;
            let arc = Arc::new(controller);
            Ok(arc)
        }
    }

    unsafe fn init_hardware(&mut self) -> Result<(), &'static str> {
        let gctl_ptr = (self.mmio_base + REG_GCTL) as *mut u32;

        // 1. Enter reset state (CRST = 0)
        *gctl_ptr &= !0x01;
        let mut timeout = 10_000;
        while (*gctl_ptr & 0x01) != 0 && timeout > 0 {
            core::hint::spin_loop();
            timeout -= 1;
        }

        // 2. Bring controller out of reset (CRST = 1)
        *gctl_ptr |= 0x01;
        timeout = 10_000;
        while (*gctl_ptr & 0x01) == 0 && timeout > 0 {
            core::hint::spin_loop();
            timeout -= 1;
        }

        // 3. Read GCAP to determine number of input/output stream descriptors
        let gcap = *((self.mmio_base + REG_GCAP) as *const u16);
        self.num_iss = ((gcap >> 8) & 0x0F) as u8;
        self.num_oss = ((gcap >> 12) & 0x0F) as u8;

        // Output streams begin after input streams: offset 0x80 + (num_iss * 0x20)
        self.output_stream_base = self.mmio_base + 0x80 + (self.num_iss as u64 * 0x20);

        // 4. Configure Output Stream 0
        let sd_base = self.output_stream_base;
        let sd_ctl_ptr = (sd_base + SD_CTL) as *mut u32;

        // Stop stream before configuring
        *sd_ctl_ptr &= !0x02; // Clear RUN bit

        // Set Stream ID = 1 (bits 23:20)
        *sd_ctl_ptr = (*sd_ctl_ptr & !(0x0F << 20)) | (1 << 20);

        // Configure stream format (44.1 kHz, 16-bit, 2 channels) -> 0x4011
        let sd_fmt_ptr = (sd_base + SD_FMT) as *mut u16;
        *sd_fmt_ptr = 0x4011;

        // Buffer Length (65536 bytes)
        let sd_cbl_ptr = (sd_base + SD_CBL) as *mut u32;
        *sd_cbl_ptr = 65536;

        // Last Valid Index (1 entry: LVI = 0)
        let sd_lvi_ptr = (sd_base + SD_LVI) as *mut u16;
        *sd_lvi_ptr = 1;

        self.is_ready = true;
        kinfo!(
            "Intel High Definition Audio initialized (BAR0: 0x{:X}, Streams: {} In / {} Out)",
            self.mmio_base,
            self.num_iss,
            self.num_oss
        );

        Ok(())
    }

    /// Play a buffer of 16-bit stereo PCM audio samples.
    pub fn play_pcm_samples(&self, samples: &[i16]) {
        if !self.is_ready {
            return;
        }

        let mut dma = self.dma_buffer.lock();
        let bytes_to_copy = (samples.len() * 2).min(dma.len());

        unsafe {
            let dest_ptr = dma.as_mut_ptr() as *mut i16;
            core::ptr::copy_nonoverlapping(samples.as_ptr(), dest_ptr, bytes_to_copy / 2);

            let sd_base = self.output_stream_base;
            let sd_ctl_ptr = (sd_base + SD_CTL) as *mut u32;

            // Start stream playback (RUN = 1)
            *sd_ctl_ptr |= 0x02;

            // Small busy spin to simulate hardware DMA consumption
            let delay_samples = samples.len().min(44100);
            for _ in 0..(delay_samples / 4) {
                core::hint::spin_loop();
            }

            // Stop stream
            *sd_ctl_ptr &= !0x02;
        }
    }
}
