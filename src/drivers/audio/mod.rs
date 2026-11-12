extern crate alloc;
use alloc::sync::Arc;
use alloc::vec::Vec;
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
}

impl AudioManager {
    pub const fn new() -> Self {
        Self {
            hda_controller: None,
            is_pc_speaker_available: true,
            volume_percent: 80,
        }
    }
}

pub static AUDIO_MANAGER: IrqSafeSpinlock<AudioManager> = IrqSafeSpinlock::new(AudioManager::new());

/// Initialize the Audio Subsystem: scans PCI for Intel HDA and configures fallback PC Speaker.
pub fn init() {
    let mut hda_found = None;

    {
        let pci = PCI.lock();
        for dev in &pci.devices {
            // Class 0x04 = Multimedia Controller
            if dev.class_code == 0x04 {
                // Subclass 0x03 = High Definition Audio
                if dev.subclass == 0x03 {
                    if let Ok(controller) = hda::HdaController::new(dev.clone()) {
                        hda_found = Some(controller);
                        break;
                    }
                }
            }
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
    let mgr = AUDIO_MANAGER.lock();
    if let Some(ref hda) = mgr.hda_controller {
        let samples = pcm::generate_sine_wave(freq_hz, duration_ms, 44100);
        hda.play_pcm_samples(&samples);
    } else {
        drop(mgr);
        pc_speaker_beep(freq_hz, duration_ms as u64);
    }
}

/// Play the iconic SublimeOS startup chime.
pub fn play_sublime_chime() {
    let mgr = AUDIO_MANAGER.lock();
    if let Some(ref hda) = mgr.hda_controller {
        let chime_samples = pcm::generate_cosmic_chime(44100);
        hda.play_pcm_samples(&chime_samples);
    } else {
        drop(mgr);
        // Harmonic chord progression via PC speaker
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
    let mgr = AUDIO_MANAGER.lock();
    if let Some(ref hda) = mgr.hda_controller {
        let mut full_speech: Vec<i16> = Vec::new();
        for b in text.bytes() {
            let phoneme = pcm::generate_phoneme_tone(b, 65, 44100);
            full_speech.extend_from_slice(&phoneme);
        }
        hda.play_pcm_samples(&full_speech);
    } else {
        drop(mgr);
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

/// Play an uncompressed PCM WAV file from the Virtual File System.
pub fn play_wav_file(path: &str) -> Result<(), &'static str> {
    let vfs = crate::fs::vfs::VFS.lock();
    let file = vfs.open(path, 0)?;
    drop(vfs);

    let mut buf = alloc::vec![0u8; 131072]; // Read up to 128 KiB
    let bytes_read = file.read(&mut buf)?;
    if bytes_read == 0 {
        return Err("File is empty");
    }

    let wav = pcm::parse_wav(&buf[..bytes_read])?;

    let mgr = AUDIO_MANAGER.lock();
    if let Some(ref hda) = mgr.hda_controller {
        hda.play_pcm_samples(&wav.samples);
    } else {
        drop(mgr);
        // Play dominant frequency via PC speaker
        pc_speaker_beep(440, 500);
    }

    Ok(())
}
