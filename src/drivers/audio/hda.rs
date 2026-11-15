extern crate alloc;
use alloc::sync::Arc;
use alloc::vec::Vec;
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
    pub sample_rate: u32,
    pub stream_fmt: u16,
    pub is_ready: bool,
    pub codec_vendor_id: u32,
    pub codec_subsys_id: u32,
    pub cs4206_apple_quirk: bool,
    pub dma_ring: HdaDmaRing,
    stream_lock: IrqSafeSpinlock<()>,
}

unsafe impl Send for HdaController {}
unsafe impl Sync for HdaController {}

/// Build the HDA `(nid, verb)` sequence verified on Apple Inc. MacBookPro8,2 Cirrus Logic CS4206 (`0x10134206`):
/// enables AFG (`0x01`) GPIOs `1 & 3` (`0x0A`), binds Stream 1 Ch 0 @ `48 kHz 16-bit 2ch` (`0x20011`) across
/// Headphone (`0x02->0x09`), Bass Speaker (`0x03->0x0a`), and Main Speaker (`0x04->0x0b`), unmuted at `0x72`.
pub fn build_cs4206_apple_init_verbs() -> Vec<(u8, u32)> {
    alloc::vec![
        // AFG power & GPIO rails (IO[1]=1, IO[3]=1 -> 0x0A)
        (0x01, 0x70500),
        (0x01, 0x7160A),
        (0x01, 0x7170A),
        (0x01, 0x7150A),
        // DAC 0x02 (Headphone), 0x03 (Bass Speaker), 0x04 (Main Speaker)
        (0x02, 0x70500),
        (0x02, 0x70610),
        (0x02, 0x20011),
        (0x02, 0x3B072),
        (0x03, 0x70500),
        (0x03, 0x70610),
        (0x03, 0x20011),
        (0x03, 0x3B072),
        (0x04, 0x70500),
        (0x04, 0x70610),
        (0x04, 0x20011),
        (0x04, 0x3B072),
        // Output Pins: 0x09 (HP Jack), 0x0a (Bass Speaker), 0x0b (Main Speaker)
        (0x09, 0x70500),
        (0x09, 0x707C0),
        (0x0A, 0x70500),
        (0x0A, 0x70740),
        (0x0A, 0x70C02),
        (0x0B, 0x70500),
        (0x0B, 0x70740),
        (0x0B, 0x70C02),
    ]
}

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
            pci.ensure_d0_power_state();
            PciBus::enable_bus_mastering(pci.bus, pci.device, pci.function);

            // Intel 6-Series / C200 PCH HDA coherency setup: force TCSEL (0x44) -> TC0 and clear DEVCFG (0x48) No-Snoop
            if pci.vendor_id == 0x8086 {
                let tcsel = PciBus::read_config_byte(pci.bus, pci.device, pci.function, 0x44);
                if (tcsel & 0x07) != 0 {
                    PciBus::write_config_byte(pci.bus, pci.device, pci.function, 0x44, tcsel & !0x07);
                }
                let devcfg = PciBus::read_config_word(pci.bus, pci.device, pci.function, 0x48);
                if (devcfg & (1 << 11)) != 0 {
                    PciBus::write_config_word(pci.bus, pci.device, pci.function, 0x48, devcfg & !(1 << 11));
                }
            }

            let mut controller = Self {
                pci,
                mmio_base,
                num_iss: 0,
                num_oss: 0,
                output_stream_base: 0,
                sample_rate: 44100,
                stream_fmt: 0x4011,
                is_ready: false,
                codec_vendor_id: 0,
                codec_subsys_id: 0,
                cs4206_apple_quirk: false,
                dma_ring,
                stream_lock: IrqSafeSpinlock::new(()),
            };

            controller.init_hardware()?;
            Ok(Arc::new(controller))
        }
    }

    pub fn active_sample_rate(&self) -> u32 {
        self.sample_rate
    }

    #[inline]
    unsafe fn read_sd_ctl24(&self, sd_base: u64) -> u32 {
        read_volatile((sd_base + SD_CTL) as *const u32) & 0x00FF_FFFF
    }

    /// Write 24-bit SD_CTL safely without modifying byte offset 0x03 (W1C SD_STS).
    /// When asserting RUN or SRST, upper byte [23:16] (stream ID) is written first so STRM latches before RUN.
    #[inline]
    unsafe fn write_sd_ctl24(&self, sd_base: u64, val: u32) {
        let v = val & 0x00FF_FFFF;
        let lo16 = (v & 0xFFFF) as u16;
        let hi8 = ((v >> 16) & 0xFF) as u8;
        if (v & (SD_CTL_RUN | SD_CTL_SRST)) != 0 {
            write_volatile((sd_base + SD_CTL + 2) as *mut u8, hi8);
            write_volatile((sd_base + SD_CTL) as *mut u16, lo16);
        } else {
            write_volatile((sd_base + SD_CTL) as *mut u16, lo16);
            write_volatile((sd_base + SD_CTL + 2) as *mut u8, hi8);
        }
    }

    unsafe fn send_immediate_verb(&self, codec_addr: u8, nid: u8, verb: u32) -> Option<u32> {
        let icis_ptr = (self.mmio_base + REG_ICIS) as *mut u16;
        let icoi_ptr = (self.mmio_base + REG_ICOI) as *mut u32;
        let icii_ptr = (self.mmio_base + REG_ICII) as *const u32;

        // Wait if Immediate Command Busy is set (up to ~5 ms)
        let mut busy_waits = 0u32;
        while (read_volatile(icis_ptr) & 0x0001) != 0 {
            if busy_waits < 32 {
                core::hint::spin_loop();
            } else if busy_waits < 532 {
                crate::arch::x86_64::pit::delay_us(10);
            } else {
                return None;
            }
            busy_waits += 1;
        }

        // Clear prior Immediate Result Valid (bit 1) + submit verb
        write_volatile(icis_ptr, 0x0002);
        let cmd = ((codec_addr as u32 & 0x0F) << 28)
            | ((nid as u32) << 20)
            | (verb & 0x000F_FFFF);
        write_volatile(icoi_ptr, cmd);
        write_volatile(icis_ptr, 0x0001);

        for attempt in 0..532u32 {
            let st = read_volatile(icis_ptr);
            if (st & 0x0003) == 0x0002 {
                return Some(read_volatile(icii_ptr));
            }
            if attempt < 32 {
                core::hint::spin_loop();
            } else {
                crate::arch::x86_64::pit::delay_us(10);
            }
        }
        None
    }

    #[inline]
    fn unity_gain_from_caps(caps: u32) -> u32 {
        let offset = caps & 0x7F;
        let num_steps = (caps >> 8) & 0x7F;
        offset.min(num_steps).min(0x7F)
    }

    unsafe fn decode_connection_list(&self, cad: u8, nid: u8) -> Vec<u8> {
        let mut out = Vec::new();
        let len_resp = self.send_immediate_verb(cad, nid, 0xF000E).unwrap_or(0);
        let long_form = (len_resp & 0x80) != 0;
        let count = (len_resp & 0x7F) as usize;
        if count == 0 {
            return out;
        }
        let step = if long_form { 2 } else { 4 };
        let mut idx = 0usize;
        let mut prev_nid: Option<u8> = None;
        while idx < count {
            let word = self
                .send_immediate_verb(cad, nid, 0xF0200 | (idx as u32 & 0xFF))
                .unwrap_or(0);
            if long_form {
                for lane in 0..2 {
                    if idx + lane >= count {
                        break;
                    }
                    let raw = ((word >> (lane * 16)) & 0xFFFF) as u16;
                    let is_range = (raw & 0x8000) != 0;
                    let entry = (raw & 0x00FF) as u8;
                    if is_range {
                        if let Some(prev) = prev_nid {
                            for n in prev.saturating_add(1)..=entry {
                                out.push(n);
                            }
                        }
                    } else {
                        out.push(entry);
                    }
                    prev_nid = Some(entry);
                }
            } else {
                for lane in 0..4 {
                    if idx + lane >= count {
                        break;
                    }
                    let raw = ((word >> (lane * 8)) & 0xFF) as u8;
                    let is_range = (raw & 0x80) != 0;
                    let entry = raw & 0x7F;
                    if is_range {
                        if let Some(prev) = prev_nid {
                            for n in prev.saturating_add(1)..=entry {
                                out.push(n);
                            }
                        }
                    } else {
                        out.push(entry);
                    }
                    prev_nid = Some(entry);
                }
            }
            idx += step;
        }
        out
    }

    unsafe fn unmute_output_amp(&self, cad: u8, nid: u8, wcaps: u32, afg_out_caps: u32) {
        if (wcaps & (1 << 2)) == 0 {
            return;
        }
        let caps = if (wcaps & (1 << 3)) != 0 {
            self.send_immediate_verb(cad, nid, 0xF0012).unwrap_or(afg_out_caps)
        } else {
            afg_out_caps
        };
        let gain = Self::unity_gain_from_caps(caps);
        let _ = self.send_immediate_verb(cad, nid, 0x3B000 | gain);
    }

    unsafe fn unmute_input_amp(&self, cad: u8, nid: u8, idx: usize, wcaps: u32, afg_in_caps: u32) {
        if (wcaps & (1 << 1)) == 0 {
            return;
        }
        let caps = if (wcaps & (1 << 3)) != 0 {
            self.send_immediate_verb(cad, nid, 0xF000D).unwrap_or(afg_in_caps)
        } else {
            afg_in_caps
        };
        let gain = Self::unity_gain_from_caps(caps);
        let _ = self.send_immediate_verb(cad, nid, 0x37000 | ((idx as u32 & 0x0F) << 8) | gain);
    }

    fn select_pcm_format(pcm_caps: u32) -> Option<(u32, u16)> {
        if (pcm_caps & (1 << 17)) == 0 {
            return None;
        }
        if (pcm_caps & (1 << 5)) != 0 {
            Some((44100, 0x4011))
        } else if (pcm_caps & (1 << 6)) != 0 {
            Some((48000, 0x0011))
        } else {
            None
        }
    }

    unsafe fn configure_codecs(&mut self) {
        let statests = read_volatile((self.mmio_base + REG_STATESTS) as *const u16);
        if statests == 0 {
            return;
        }

        #[derive(Clone)]
        struct WidgetInfo {
            nid: u8,
            wcaps: u32,
            wtype: u8,
            pincaps: u32,
            def_cfg: u32,
            conns: Vec<u8>,
        }

        for cad in 0u8..15 {
            if (statests & (1 << cad)) == 0 {
                continue;
            }
            let vendor_id = self.send_immediate_verb(cad, 0, 0xF0000).unwrap_or(0);
            if vendor_id != 0 {
                self.codec_vendor_id = vendor_id;
            }
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
                let subsys_id = self.send_immediate_verb(cad, afg, 0xF2000).unwrap_or(0);
                if subsys_id != 0 {
                    self.codec_subsys_id = subsys_id;
                }
                let _ = self.send_immediate_verb(cad, afg, 0x70500);

                let is_apple = (subsys_id >> 16) == 0x106B
                    || crate::drivers::platform::apple::ApplePlatformHardware::is_smc_present();
                if vendor_id == 0x1013_4206 && is_apple {
                    crate::arch::x86_64::pit::delay_ms(10);
                    let verbs = build_cs4206_apple_init_verbs();
                    for (idx, &(nid, verb)) in verbs.iter().enumerate() {
                        let target_nid = if nid == 0x01 { afg } else { nid };
                        let _ = self.send_immediate_verb(cad, target_nid, verb);
                        // Allow speaker amplifier power rails (GPIO 0x0A) to settle before configuring DACs/Pins
                        if idx == 3 {
                            crate::arch::x86_64::pit::delay_ms(10);
                        }
                    }
                    self.sample_rate = 48000;
                    self.stream_fmt = 0x0011;
                    self.cs4206_apple_quirk = true;
                    return;
                }
                let afg_pcm_caps = self.send_immediate_verb(cad, afg, 0xF000A).unwrap_or(0);
                let afg_in_caps = self.send_immediate_verb(cad, afg, 0xF000D).unwrap_or(0);
                let afg_out_caps = self.send_immediate_verb(cad, afg, 0xF0012).unwrap_or(0);

                let widgets_resp = self.send_immediate_verb(cad, afg, 0xF0004).unwrap_or(0);
                let start_nid = ((widgets_resp >> 16) & 0xFF) as u8;
                let num_nids = (widgets_resp & 0xFF) as u8;

                let mut widgets = Vec::with_capacity(num_nids as usize);
                for nid in start_nid..start_nid.saturating_add(num_nids) {
                    let wcaps = self.send_immediate_verb(cad, nid, 0xF0009).unwrap_or(0);
                    let wtype = ((wcaps >> 20) & 0x0F) as u8;
                    let pincaps = if wtype == 0x04 {
                        self.send_immediate_verb(cad, nid, 0xF000C).unwrap_or(0)
                    } else {
                        0
                    };
                    let def_cfg = if wtype == 0x04 {
                        self.send_immediate_verb(cad, nid, 0xF1C00).unwrap_or(0)
                    } else {
                        0
                    };
                    let conns = if (wcaps & (1 << 8)) != 0 || wtype == 0x04 || wtype == 0x02 || wtype == 0x03 {
                        self.decode_connection_list(cad, nid)
                    } else {
                        Vec::new()
                    };
                    widgets.push(WidgetInfo { nid, wcaps, wtype, pincaps, def_cfg, conns });
                }

                // Collect valid analog output pins: prioritize Line Out (0x00), Speaker (0x01), HP Out (0x02).
                let mut priority_pins = Vec::new();
                let mut fallback_pins = Vec::new();
                for (i, w) in widgets.iter().enumerate() {
                    if w.wtype != 0x04 || (w.pincaps & (1 << 4)) == 0 || (w.wcaps & (1 << 9)) != 0 {
                        continue;
                    }
                    let port_conn = (w.def_cfg >> 30) & 0x03;
                    if port_conn == 1 {
                        continue; // Jack has no physical connection
                    }
                    let dev = ((w.def_cfg >> 20) & 0x0F) as u8;
                    if dev == 0x00 || dev == 0x01 || dev == 0x02 {
                        priority_pins.push(i);
                    } else {
                        fallback_pins.push(i);
                    }
                }
                let chosen_pins = if !priority_pins.is_empty() { priority_pins } else { fallback_pins };
                if chosen_pins.is_empty() {
                    continue;
                }

                let find_idx = |nid: u8| -> Option<usize> {
                    widgets.iter().position(|w| w.nid == nid)
                };

                let mut configured_any = false;
                for pin_wi in chosen_pins {
                    // BFS upstream from output pin to nearest non-digital Audio Output DAC (wtype == 0x00).
                    let mut queue: Vec<(usize, Vec<(usize, usize)>)> = Vec::new();
                    let mut visited = alloc::vec![false; widgets.len()];
                    visited[pin_wi] = true;
                    queue.push((pin_wi, Vec::new()));
                    let mut found_path: Option<(usize, Vec<(usize, usize)>)> = None;
                    let mut head = 0usize;
                    while head < queue.len() && found_path.is_none() {
                        let (cur_wi, path) = (queue[head].0, queue[head].1.clone());
                        head += 1;
                        if path.len() >= 6 {
                            continue;
                        }
                        for (conn_idx, &src_nid) in widgets[cur_wi].conns.iter().enumerate() {
                            let Some(src_wi) = find_idx(src_nid) else { continue };
                            if (widgets[src_wi].wcaps & (1 << 9)) != 0 {
                                continue;
                            }
                            let mut next_path = path.clone();
                            next_path.push((cur_wi, conn_idx));
                            if widgets[src_wi].wtype == 0x00 {
                                found_path = Some((src_wi, next_path));
                                break;
                            }
                            if !visited[src_wi] {
                                visited[src_wi] = true;
                                queue.push((src_wi, next_path));
                            }
                        }
                    }

                    let Some((dac_wi, hops)) = found_path else { continue };
                    let dac = &widgets[dac_wi];
                    let dac_pcm_caps = if (dac.wcaps & (1 << 10)) != 0 {
                        self.send_immediate_verb(cad, dac.nid, 0xF000A).unwrap_or(afg_pcm_caps)
                    } else {
                        afg_pcm_caps
                    };
                    if let Some((sr, fmt)) = Self::select_pcm_format(dac_pcm_caps) {
                        self.sample_rate = sr;
                        self.stream_fmt = fmt;
                    } else if let Some((sr, fmt)) = Self::select_pcm_format(afg_pcm_caps) {
                        self.sample_rate = sr;
                        self.stream_fmt = fmt;
                    }

                    // Power D0 + configure DAC
                    let _ = self.send_immediate_verb(cad, dac.nid, 0x70500);
                    let _ = self.send_immediate_verb(cad, dac.nid, 0x70610);
                    let _ = self.send_immediate_verb(cad, dac.nid, 0x20000 | (self.stream_fmt as u32));
                    self.unmute_output_amp(cad, dac.nid, dac.wcaps, afg_out_caps);

                    // Configure intermediate widgets & Pin along discovered upstream path
                    for &(dst_wi, conn_idx) in hops.iter().rev() {
                        let w = &widgets[dst_wi];
                        let _ = self.send_immediate_verb(cad, w.nid, 0x70500);
                        if w.conns.len() > 1 && (w.wtype == 0x03 || w.wtype == 0x04) {
                            let _ = self.send_immediate_verb(cad, w.nid, 0x70100 | (conn_idx as u32 & 0xFF));
                        }
                        if w.wtype == 0x02 {
                            self.unmute_input_amp(cad, w.nid, conn_idx, w.wcaps, afg_in_caps);
                        }
                        self.unmute_output_amp(cad, w.nid, w.wcaps, afg_out_caps);
                    }

                    let pin = &widgets[pin_wi];
                    let pin_ctl = if (pin.pincaps & (1 << 3)) != 0 { 0x707C0 } else { 0x70740 };
                    let _ = self.send_immediate_verb(cad, pin.nid, pin_ctl);
                    if (pin.pincaps & (1 << 16)) != 0 {
                        let _ = self.send_immediate_verb(cad, pin.nid, 0x70C02);
                    }
                    configured_any = true;
                }

                if configured_any {
                    return;
                }
            }
        }
    }

    unsafe fn reset_output_stream(&self, sd_base: u64) {
        let sd_sts_ptr = (sd_base + SD_STS) as *mut u8;

        // Clear RUN bit and wait for engine halt
        let ctl = self.read_sd_ctl24(sd_base) & !SD_CTL_RUN;
        self.write_sd_ctl24(sd_base, ctl);
        let mut timeout = 10_000;
        while (self.read_sd_ctl24(sd_base) & SD_CTL_RUN) != 0 && timeout > 0 {
            core::hint::spin_loop();
            timeout -= 1;
        }

        // Assert Stream Reset (SRST = 1), wait for SRST latch, then deassert
        let srst_ctl = self.read_sd_ctl24(sd_base) | SD_CTL_SRST;
        self.write_sd_ctl24(sd_base, srst_ctl);
        timeout = 10_000;
        while (self.read_sd_ctl24(sd_base) & SD_CTL_SRST) == 0 && timeout > 0 {
            core::hint::spin_loop();
            timeout -= 1;
        }

        let clear_srst = self.read_sd_ctl24(sd_base) & !SD_CTL_SRST;
        self.write_sd_ctl24(sd_base, clear_srst);
        timeout = 10_000;
        while (self.read_sd_ctl24(sd_base) & SD_CTL_SRST) != 0 && timeout > 0 {
            core::hint::spin_loop();
            timeout -= 1;
        }

        // Clear write-1-to-clear stream status flags (BCIS | FIFOE | DESE = 0x1C)
        write_volatile(sd_sts_ptr, 0x1C);
    }

    unsafe fn program_output_stream(&self, sd_base: u64, cbl_bytes: u32, lvi: u16) {
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

        // Cyclic Buffer Length, Last Valid Index, Negotiated Format (44.1kHz or 48kHz, 16-bit, stereo)
        write_volatile(sd_cbl_ptr, cbl_bytes);
        write_volatile(sd_lvi_ptr, lvi);
        write_volatile(sd_fmt_ptr, self.stream_fmt);

        // Assign Stream Number = 1 (bits 23:20)
        let ctl = (self.read_sd_ctl24(sd_base) & !(0x0F << SD_CTL_STRM_SHIFT)) | (1 << SD_CTL_STRM_SHIFT);
        self.write_sd_ctl24(sd_base, ctl);
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

        // Allow codecs >= 521 us per Intel HDA Spec Section 4.3 to assert STATESTS frame wakeup
        crate::arch::x86_64::pit::delay_us(1000);

        // 3. Read GCAP to determine 64-bit DMA support (bit 0 = 64OK) and stream counts
        let gcap = read_volatile((self.mmio_base + REG_GCAP) as *const u16);
        let has_64ok = (gcap & 0x0001) != 0;
        if !has_64ok && ((self.dma_ring.bdl_phys >> 32) != 0 || (self.dma_ring.pcm_phys >> 32) != 0) {
            return Err("Intel HDA controller lacks 64OK support for >=4GiB DMA frame");
        }
        self.num_iss = ((gcap >> 8) & 0x0F) as u8;
        self.num_oss = ((gcap >> 12) & 0x0F) as u8;
        if self.num_oss == 0 {
            return Err("Intel HDA controller reports zero output streams");
        }

        // Output streams begin immediately after input streams: offset 0x80 + (num_iss * 0x20)
        self.output_stream_base = self.mmio_base + 0x80 + (self.num_iss as u64 * 0x20);

        // 4. Configure DAC / Mixer / Selector / Pin codec routing if an HDA codec responded
        self.configure_codecs();

        // 5. Reset Output Stream 0 and program real BDL physical DMA ring descriptors
        let sd_base = self.output_stream_base;
        self.reset_output_stream(sd_base);
        self.program_output_stream(sd_base, HDA_RING_BYTES as u32, (HDA_BDL_ENTRIES - 1) as u16);

        self.is_ready = true;
        let (bdl_lo, bdl_hi) = self.read_bdl_registers();
        kinfo!(
            "Intel HDA Hardware DMA Ring active (Codec: 0x{:08X}, Subsys: 0x{:08X}, CS4206-Apple: {}, Rate: {} Hz, 64OK: {}, Streams: {} In/{} Out, BDL: 0x{:08X}_{:08X})",
            self.codec_vendor_id,
            self.codec_subsys_id,
            self.cs4206_apple_quirk,
            self.sample_rate,
            has_64ok,
            self.num_iss,
            self.num_oss,
            bdl_hi,
            bdl_lo
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
            self.reset_output_stream(sd_base);

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
            self.write_sd_ctl24(sd_base, active_ctl);
            let _ = self.read_sd_ctl24(sd_base);
        }
    }

    pub fn stop_stream(&self) {
        if !self.is_ready {
            return;
        }
        let _guard = self.stream_lock.lock();
        unsafe {
            let ctl = self.read_sd_ctl24(self.output_stream_base) & !SD_CTL_RUN;
            self.write_sd_ctl24(self.output_stream_base, ctl);
            let _ = self.read_sd_ctl24(self.output_stream_base);
        }
    }

    /// Non-blocking stop for IRQ context: clears SD_CTL.RUN without spin-wait or blocking lock acquire.
    pub fn try_stop_stream(&self) -> bool {
        if !self.is_ready {
            return false;
        }
        let Some(_guard) = self.stream_lock.try_lock() else {
            return false;
        };
        unsafe {
            let ctl = self.read_sd_ctl24(self.output_stream_base) & !SD_CTL_RUN;
            self.write_sd_ctl24(self.output_stream_base, ctl);
            let _ = self.read_sd_ctl24(self.output_stream_base);
        }
        true
    }

    pub fn is_stream_running(&self) -> bool {
        if !self.is_ready {
            return false;
        }
        for _ in 0..5_000 {
            if unsafe { (self.read_sd_ctl24(self.output_stream_base) & SD_CTL_RUN) != 0 } {
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

    pub fn is_running_fast(&self) -> bool {
        if !self.is_ready {
            return false;
        }
        unsafe { (self.read_sd_ctl24(self.output_stream_base) & SD_CTL_RUN) != 0 }
    }

    /// Write raw interleaved PCM bytes into the cyclic hardware DMA ring at byte `ring_offset`.
    pub fn write_ring_bytes(&self, ring_offset: usize, data: &[u8]) {
        if !self.is_ready || data.is_empty() {
            return;
        }
        let ring = self.dma_ring.ring_bytes;
        let off = ring_offset % ring;
        let first = data.len().min(ring - off);
        unsafe {
            let dst = self.dma_ring.pcm_virt as *mut u8;
            core::ptr::copy_nonoverlapping(data.as_ptr(), dst.add(off), first);
            if first < data.len() {
                let rem = (data.len() - first).min(ring);
                core::ptr::copy_nonoverlapping(data.as_ptr().add(first), dst, rem);
            }
            fence(Ordering::SeqCst);
        }
    }

    /// Zero a byte range `[ring_offset .. ring_offset + len)` inside the cyclic DMA ring.
    pub fn zero_ring_bytes(&self, ring_offset: usize, len: usize) {
        if !self.is_ready || len == 0 {
            return;
        }
        let ring = self.dma_ring.ring_bytes;
        let off = ring_offset % ring;
        let clamped = len.min(ring);
        let first = clamped.min(ring - off);
        unsafe {
            let dst = self.dma_ring.pcm_virt as *mut u8;
            core::ptr::write_bytes(dst.add(off), 0, first);
            if first < clamped {
                core::ptr::write_bytes(dst, 0, clamped - first);
            }
            fence(Ordering::SeqCst);
        }
    }

    /// Reset output stream (clearing LPIB to 0) and arm Intel HDA DMA ring continuously.
    pub fn ensure_ring_running(&self) {
        if !self.is_ready || self.is_running_fast() {
            return;
        }
        let _guard = self.stream_lock.lock();
        let sd_base = self.output_stream_base;
        unsafe {
            self.reset_output_stream(sd_base);
            self.program_output_stream(sd_base, HDA_RING_BYTES as u32, (HDA_BDL_ENTRIES - 1) as u16);
            let active_ctl = (1 << SD_CTL_STRM_SHIFT) | SD_CTL_IOCE | SD_CTL_RUN;
            self.write_sd_ctl24(sd_base, active_ctl);
            let _ = self.read_sd_ctl24(sd_base);
        }
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

    /// Return `(sd_ctl24, sd_sts, sd_lpib)` for bare-metal DMA stream health diagnostics.
    pub fn debug_stream_snapshot(&self) -> (u32, u8, u32) {
        if !self.is_ready || self.output_stream_base == 0 {
            return (0, 0, 0);
        }
        unsafe {
            let ctl = self.read_sd_ctl24(self.output_stream_base);
            let sts = read_volatile((self.output_stream_base + SD_STS) as *const u8);
            let lpib = read_volatile((self.output_stream_base + SD_LPIB) as *const u32);
            (ctl, sts, lpib)
        }
    }
}
