extern crate alloc;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use crate::arch::x86_64::io::{inb, outb};
use crate::drivers::pci::PCI;
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

pub mod hda;
pub mod pcm;

pub struct AudioManager {
    pub hda_controller: Option<Arc<hda::HdaController>>,
    pub is_pc_speaker_available: bool,
    pub volume_percent: u8,
    pub muted: bool,
    pub stream_queue: VecDeque<i16>,
    pub ring_write_pos: usize,
    pub last_hw_lpib: usize,
    pub total_ring_ahead: usize,
    pub real_audio_bytes_in_ring: usize,
    pub trailing_silence_bytes: usize,
    pub total_streamed_samples: u64,
    resampler_phase_fp: u32,
    resamp_primed: bool,
    prev_sample_l: i16,
    prev_sample_r: i16,
}

impl AudioManager {
    pub const fn new() -> Self {
        Self {
            hda_controller: None,
            is_pc_speaker_available: true,
            volume_percent: 80,
            muted: false,
            stream_queue: VecDeque::new(),
            ring_write_pos: 0,
            last_hw_lpib: 0,
            total_ring_ahead: 0,
            real_audio_bytes_in_ring: 0,
            trailing_silence_bytes: 0,
            total_streamed_samples: 0,
            resampler_phase_fp: 0,
            resamp_primed: false,
            prev_sample_l: 0,
            prev_sample_r: 0,
        }
    }

    #[inline]
    fn reset_ring_tracking(&mut self) {
        self.ring_write_pos = 0;
        self.last_hw_lpib = 0;
        self.total_ring_ahead = 0;
        self.real_audio_bytes_in_ring = 0;
        self.trailing_silence_bytes = 0;
        self.resampler_phase_fp = 0;
        self.resamp_primed = false;
        self.prev_sample_l = 0;
        self.prev_sample_r = 0;
    }
}

pub static AUDIO_MANAGER: IrqSafeSpinlock<AudioManager> = IrqSafeSpinlock::new(AudioManager::new());

/// Return detected active audio output controller name.
pub fn active_device_name() -> &'static str {
    let mgr = AUDIO_MANAGER.lock();
    if mgr.hda_controller.is_some() {
        "Intel High Definition Audio (HDA)"
    } else if mgr.is_pc_speaker_available {
        "PC Speaker / Legacy Timer Audio"
    } else {
        "No Audio Output Device"
    }
}

pub fn get_volume() -> u8 {
    AUDIO_MANAGER.lock().volume_percent
}

pub fn set_volume(pct: u8) {
    AUDIO_MANAGER.lock().volume_percent = pct.clamp(0, 100);
}

pub fn is_muted() -> bool {
    AUDIO_MANAGER.lock().muted
}

pub fn set_muted(muted: bool) {
    AUDIO_MANAGER.lock().muted = muted;
}

/// Initialize the Audio Subsystem: scans PCI for Intel HDA and configures fallback PC Speaker.
pub fn init() {
    let mut hda_found = None;

    let hda_candidates: alloc::vec::Vec<crate::drivers::pci::PciDevice> = {
        let pci = PCI.lock();
        pci.devices
            .iter()
            .filter(|dev| dev.class_code == 0x04 && dev.subclass == 0x03)
            .cloned()
            .collect()
    };

    for dev in hda_candidates {
        if let Ok(controller) = hda::HdaController::new(dev) {
            hda_found = Some(controller);
            break;
        }
    }

    let mut mgr = AUDIO_MANAGER.lock();
    mgr.hda_controller = hda_found;

    if mgr.hda_controller.is_some() {
        kinfo!("Audio Subsystem initialized (Intel HDA Primary, PC Speaker Secondary)");
    } else {
        kinfo!("Audio Subsystem initialized (PC Speaker Primary / Simulated Audio)");
    }
}

/// PC Speaker low-level hardware sound generation via 8254 PIT Timer 2 (Port 0x42, 0x61).
pub fn pc_speaker_beep(freq_hz: u32, duration_ms: u64) {
    if freq_hz == 0 {
        crate::arch::x86_64::pit::sleep_ms(duration_ms);
        return;
    }

    let divisor = (1193182 / freq_hz) as u16;

    unsafe {
        // Set PIT 2 to Mode 3 (Square wave)
        outb(0x43, 0xB6);
        outb(0x42, (divisor & 0xFF) as u8);
        outb(0x42, ((divisor >> 8) & 0xFF) as u8);

        // Turn on speaker (bits 0 and 1 of port 0x61)
        let tmp = inb(0x61);
        if (tmp & 3) != 3 {
            outb(0x61, tmp | 3);
        }

        crate::arch::x86_64::pit::sleep_ms(duration_ms);

        // Turn off speaker
        let tmp = inb(0x61);
        outb(0x61, tmp & !3);
    }
}

/// Play a pure sine tone through Intel HDA or PC Speaker.
pub fn play_tone(freq_hz: u32, duration_ms: u32) {
    let has_hda = AUDIO_MANAGER.lock().hda_controller.is_some();
    if has_hda {
        let samples = pcm::generate_sine_wave(freq_hz, duration_ms, 44100);
        stream_replace_pcm(&samples);
    } else {
        pc_speaker_beep(freq_hz, duration_ms as u64);
    }
}

/// Play the iconic SublimeOS startup chime.
pub fn play_sublime_chime() {
    let has_hda = AUDIO_MANAGER.lock().hda_controller.is_some();
    if has_hda {
        let chime_samples = pcm::generate_cosmic_chime(44100);
        stream_replace_pcm(&chime_samples);
    } else {
        let notes = [174, 261, 349, 440];
        for &note in &notes {
            pc_speaker_beep(note, 120);
        }
    }
}

/// Backwards compatibility alias
pub fn play_cosmic_chime() {
    play_sublime_chime();
}

/// Synthesize speech audio from text through the audio subsystem.
pub fn play_speech(text: &str) {
    let has_hda = AUDIO_MANAGER.lock().hda_controller.is_some();
    if has_hda {
        let mut full_speech: alloc::vec::Vec<i16> = alloc::vec::Vec::new();
        for b in text.bytes() {
            let phoneme = pcm::generate_phoneme_tone(b, 65, 44100);
            full_speech.extend_from_slice(&phoneme);
        }
        stream_replace_pcm(&full_speech);
    } else {
        for b in text.bytes() {
            if b == b' ' {
                crate::arch::x86_64::pit::sleep_ms(40);
            } else {
                let freq = 200 + ((b as u32 % 12) * 60);
                pc_speaker_beep(freq, 40);
            }
        }
    }
}

/// Play any supported audio file (`WAV`, `IMA-ADPCM`, `QOA`, or `MP3`) via non-blocking ring streaming.
pub fn play_wav_file(path: &str) -> Result<(), &'static str> {
    play_audio_file(path)
}

/// Decode and enqueue an audio file (`.wav`, `.qoa`, `.mp3`) into the non-blocking HDA stream ring.
pub fn play_audio_file(path: &str) -> Result<(), &'static str> {
    let file = {
        let vfs = crate::fs::vfs::VFS.lock();
        vfs.open(path, 0)?
    };
    let size_hint = (file.stat().size as usize).clamp(4096, 8 * 1024 * 1024);
    let mut buf = alloc::vec::Vec::with_capacity(size_hint);
    let mut chunk = alloc::vec![0u8; 32768];
    loop {
        match file.read(&mut chunk)? {
            0 => break,
            n => {
                if buf.len() + n > 8 * 1024 * 1024 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
        }
    }
    if buf.is_empty() {
        return Err("Audio file is empty");
    }

    let decoded = libgui::codecs::decode_audio(&buf)?;
    let pushed = stream_replace_pcm(&decoded.samples_44k_stereo);
    if pushed == 0 {
        return Err("Audio queue full");
    }
    Ok(())
}

const MAX_QUEUED_PCM_SAMPLES: usize = 44100 * 2 * 15; // Up to 15s queued stereo 44.1kHz PCM

/// Atomically replace active audio queue and start DMA playback with zero lock-drop races.
pub fn stream_replace_pcm(samples: &[i16]) -> usize {
    let even_len = samples.len() & !1;
    let take = even_len.min(MAX_QUEUED_PCM_SAMPLES & !1);
    {
        let mut mgr = AUDIO_MANAGER.lock();
        mgr.stream_queue.clear();
        mgr.reset_ring_tracking();
        if let Some(ref hda) = mgr.hda_controller {
            hda.stop_stream();
            hda.zero_ring_bytes(0, hda::HDA_RING_BYTES);
        }
        if take > 0 {
            mgr.stream_queue.extend(samples[..take].iter().copied());
        }
    }
    if take > 0 {
        poll_audio_stream();
    }
    take
}

/// Enqueue interleaved 44.1 kHz stereo signed 16-bit PCM samples without blocking the caller.
pub fn stream_push_pcm(samples: &[i16]) -> usize {
    let even_len = samples.len() & !1;
    if even_len == 0 {
        return 0;
    }
    let need_start;
    let take;
    {
        let mut mgr = AUDIO_MANAGER.lock();
        let avail = MAX_QUEUED_PCM_SAMPLES.saturating_sub(mgr.stream_queue.len()) & !1;
        take = even_len.min(avail);
        if take > 0 {
            mgr.stream_queue.extend(samples[..take].iter().copied());
            mgr.trailing_silence_bytes = 0;
        }
        need_start = mgr
            .hda_controller
            .as_ref()
            .map(|h| !h.is_running_fast())
            .unwrap_or(false);
    }
    if need_start && take > 0 {
        poll_audio_stream();
    }
    take
}

/// Clear pending audio queue and reset ring/resampler state cleanly for immediate seek or restart.
pub fn stream_clear() {
    let mut mgr = AUDIO_MANAGER.lock();
    mgr.stream_queue.clear();
    mgr.reset_ring_tracking();
    if let Some(ref hda) = mgr.hda_controller {
        hda.stop_stream();
        hda.zero_ring_bytes(0, hda::HDA_RING_BYTES);
    }
}

/// Stop audio DMA stream and flush queue immediately.
pub fn stream_stop() {
    let mut mgr = AUDIO_MANAGER.lock();
    mgr.stream_queue.clear();
    mgr.reset_ring_tracking();
    if let Some(ref hda) = mgr.hda_controller {
        hda.stop_stream();
        hda.zero_ring_bytes(0, hda::HDA_RING_BYTES);
    }
}

/// Return `(queued_samples, total_streamed_samples, is_running, volume_percent)`.
pub fn stream_status() -> (usize, u64, bool, u8) {
    let mgr = AUDIO_MANAGER.lock();
    let running = mgr
        .hda_controller
        .as_ref()
        .map(|h| h.is_running_fast())
        .unwrap_or(false);
    (
        mgr.stream_queue.len(),
        mgr.total_streamed_samples,
        running,
        if mgr.muted { 0 } else { mgr.volume_percent },
    )
}

#[inline]
fn next_resamp_stereo_frame(mgr: &mut AudioManager, hw_rate: u32) -> Option<(i16, i16)> {
    if hw_rate != 48000 {
        if mgr.stream_queue.len() < 2 {
            return None;
        }
        let l = mgr.stream_queue.pop_front()?;
        let r = mgr.stream_queue.pop_front()?;
        mgr.prev_sample_l = l;
        mgr.prev_sample_r = r;
        return Some((l, r));
    }

    if !mgr.resamp_primed {
        if mgr.stream_queue.len() < 2 {
            return None;
        }
        mgr.prev_sample_l = mgr.stream_queue.pop_front()?;
        mgr.prev_sample_r = mgr.stream_queue.pop_front()?;
        mgr.resampler_phase_fp = 0;
        mgr.resamp_primed = true;
    }

    while mgr.resampler_phase_fp >= 65536 {
        if mgr.stream_queue.len() < 2 {
            mgr.resamp_primed = false;
            return None;
        }
        mgr.prev_sample_l = mgr.stream_queue.pop_front()?;
        mgr.prev_sample_r = mgr.stream_queue.pop_front()?;
        mgr.resampler_phase_fp -= 65536;
    }

    let (next_l, next_r) = if mgr.stream_queue.len() >= 2 {
        (mgr.stream_queue[0], mgr.stream_queue[1])
    } else {
        (mgr.prev_sample_l, mgr.prev_sample_r)
    };
    let frac = (mgr.resampler_phase_fp & 0xFFFF) as i32;
    let out_l = (mgr.prev_sample_l as i32
        + (((next_l as i32 - mgr.prev_sample_l as i32) * frac) >> 16)) as i16;
    let out_r = (mgr.prev_sample_r as i32
        + (((next_r as i32 - mgr.prev_sample_r as i32) * frac) >> 16)) as i16;

    const STEP_44K_TO_48K: u32 = ((44100u64 << 16) / 48000u64) as u32; // 60211
    mgr.resampler_phase_fp = mgr.resampler_phase_fp.wrapping_add(STEP_44K_TO_48K);
    Some((out_l, out_r))
}

fn fill_stage_from_queue(mgr: &mut AudioManager, hw_rate: u32, stage: &mut [u8]) -> usize {
    let eff_vol = if mgr.muted { 0 } else { mgr.volume_percent as i32 };
    let mut off = 0usize;
    while off + 4 <= stage.len() {
        let Some((l, r)) = next_resamp_stereo_frame(mgr, hw_rate) else {
            break;
        };
        let sl = ((l as i32 * eff_vol) / 100) as i16;
        let sr = ((r as i32 * eff_vol) / 100) as i16;
        stage[off..off + 2].copy_from_slice(&sl.to_le_bytes());
        stage[off + 2..off + 4].copy_from_slice(&sr.to_le_bytes());
        off += 4;
    }
    off
}

fn poll_audio_stream_inner(blocking: bool) {
    let mut mgr = if blocking {
        AUDIO_MANAGER.lock()
    } else {
        match AUDIO_MANAGER.try_lock() {
            Some(g) => g,
            None => return,
        }
    };

    let Some(hda) = mgr.hda_controller.clone() else {
        return;
    };
    if !hda.is_ready {
        return;
    }

    let ring_bytes = hda.dma_ring.ring_bytes;
    let hw_rate = hda.active_sample_rate();
    let running = hda.is_running_fast();

    if !running {
        if mgr.stream_queue.len() < 2 || !blocking {
            return;
        }
        mgr.reset_ring_tracking();
        hda.zero_ring_bytes(0, ring_bytes);
        let mut stage = [0u8; 2048];
        let mut filled = 0usize;
        while filled < ring_bytes {
            let max_slice = (ring_bytes - filled).min(stage.len()) & !3;
            let n = fill_stage_from_queue(&mut mgr, hw_rate, &mut stage[..max_slice]);
            if n == 0 {
                break;
            }
            hda.write_ring_bytes(filled, &stage[..n]);
            filled += n;
            mgr.total_streamed_samples = mgr.total_streamed_samples.saturating_add((n / 2) as u64);
        }
        if filled == 0 {
            return;
        }
        mgr.ring_write_pos = filled % ring_bytes;
        mgr.last_hw_lpib = 0;
        mgr.total_ring_ahead = filled;
        mgr.real_audio_bytes_in_ring = filled;
        hda.ensure_ring_running();
        return;
    }

    let lpib = (hda.link_position() as usize) % ring_bytes;
    let hw_delta = (lpib + ring_bytes - mgr.last_hw_lpib) % ring_bytes;
    mgr.last_hw_lpib = lpib;
    mgr.total_ring_ahead = mgr.total_ring_ahead.saturating_sub(hw_delta);
    mgr.real_audio_bytes_in_ring = mgr.real_audio_bytes_in_ring.saturating_sub(hw_delta);

    const RING_SAFETY_CUSHION: usize = 4096;
    if mgr.total_ring_ahead == 0 {
        mgr.ring_write_pos = ((lpib + RING_SAFETY_CUSHION) & !3) % ring_bytes;
        hda.zero_ring_bytes(lpib, RING_SAFETY_CUSHION);
        mgr.total_ring_ahead = RING_SAFETY_CUSHION;
    }

    let free_bytes = ring_bytes
        .saturating_sub(mgr.total_ring_ahead)
        .saturating_sub(RING_SAFETY_CUSHION)
        & !3;

    let mut stage = [0u8; 2048];
    let mut rem = free_bytes;
    while rem >= 4 && mgr.stream_queue.len() >= 2 {
        let max_slice = rem.min(stage.len()) & !3;
        let n = fill_stage_from_queue(&mut mgr, hw_rate, &mut stage[..max_slice]);
        if n == 0 {
            break;
        }
        let wp = mgr.ring_write_pos;
        hda.write_ring_bytes(wp, &stage[..n]);
        mgr.ring_write_pos = (wp + n) % ring_bytes;
        mgr.total_ring_ahead += n;
        mgr.real_audio_bytes_in_ring += n;
        mgr.trailing_silence_bytes = 0;
        mgr.total_streamed_samples = mgr.total_streamed_samples.saturating_add((n / 2) as u64);
        rem -= n;
    }

    if mgr.stream_queue.len() < 2 {
        if mgr.real_audio_bytes_in_ring > 0 {
            if rem > 0 {
                let wp = mgr.ring_write_pos;
                hda.zero_ring_bytes(wp, rem);
                mgr.ring_write_pos = (wp + rem) % ring_bytes;
                mgr.total_ring_ahead += rem;
                mgr.trailing_silence_bytes = mgr.trailing_silence_bytes.saturating_add(rem);
            }
        } else {
            let stopped = if blocking {
                hda.stop_stream();
                true
            } else {
                hda.try_stop_stream()
            };
            if stopped {
                hda.zero_ring_bytes(0, ring_bytes);
                mgr.reset_ring_tracking();
            }
        }
    }
}

/// Feed queued PCM samples ahead of Intel HDA hardware `SD_LPIB` cursor (blocking lock).
pub fn poll_audio_stream() {
    poll_audio_stream_inner(true);
}

/// IRQ-safe zero-allocation audio refill using `try_lock()` without spin-waits.
pub fn poll_audio_stream_nonblocking() {
    poll_audio_stream_inner(false);
}
