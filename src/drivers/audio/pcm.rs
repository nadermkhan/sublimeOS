extern crate alloc;
use alloc::vec::Vec;

// 65-entry quarter-wave sine table for 0 to pi/2 (values: 32767 * sin(k * pi / 128))
const QUARTER_SINE: [i16; 65] = [
    0, 804, 1608, 2410, 3209, 4005, 4796, 5582, 6362, 7134, 7898, 8653, 9398, 10132, 10854,
    11564, 12260, 12942, 13610, 14262, 14899, 15519, 16122, 16707, 17274, 17822, 18351, 18860,
    19349, 19817, 20264, 20689, 21092, 21473, 21831, 22166, 22478, 22766, 23030, 23270, 23485,
    23675, 23840, 23979, 24093, 24180, 24242, 24278, 24287, 24270, 24227, 24158, 24062, 23940,
    23791, 23617, 23416, 23189, 22936, 22657, 22352, 22022, 21666, 21285, 32767,
];

/// Fast integer sine function for phase 0..255 representing 0..2*pi.
pub fn integer_sin(phase: u8) -> i16 {
    let quad = phase >> 6; // 0..3
    let idx = (phase & 0x3F) as usize; // 0..63

    match quad {
        0 => QUARTER_SINE[idx],
        1 => QUARTER_SINE[64 - idx],
        2 => -QUARTER_SINE[idx],
        _ => -QUARTER_SINE[64 - idx],
    }
}

#[derive(Debug, Clone)]
pub struct WavAudio {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub samples: Vec<i16>,
}

/// Parse a standard RIFF/WAVE PCM audio file.
pub fn parse_wav(data: &[u8]) -> Result<WavAudio, &'static str> {
    if data.len() < 44 {
        return Err("WAV data too short (less than 44 bytes)");
    }

    // 1. Check RIFF header
    if &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return Err("Invalid WAV file: missing RIFF/WAVE header");
    }

    let mut idx = 12;
    let mut sample_rate = 44100u32;
    let mut channels = 2u16;
    let mut bits_per_sample = 16u16;
    let mut pcm_samples = Vec::new();

    // Iterate through chunks
    while idx + 8 <= data.len() {
        let chunk_id = &data[idx..idx + 4];
        let chunk_len = u32::from_le_bytes([data[idx + 4], data[idx + 5], data[idx + 6], data[idx + 7]]) as usize;
        idx += 8;

        if chunk_id == b"fmt " {
            if idx + 16 > data.len() {
                return Err("Malformed fmt chunk");
            }
            let format_tag = u16::from_le_bytes([data[idx], data[idx + 1]]);
            if format_tag != 1 {
                return Err("Unsupported WAV compression: only uncompressed PCM (1) supported");
            }
            channels = u16::from_le_bytes([data[idx + 2], data[idx + 3]]);
            sample_rate = u32::from_le_bytes([data[idx + 4], data[idx + 5], data[idx + 6], data[idx + 7]]);
            bits_per_sample = u16::from_le_bytes([data[idx + 14], data[idx + 15]]);
        } else if chunk_id == b"data" {
            let data_len = chunk_len.min(data.len() - idx);
            let sample_bytes = &data[idx..idx + data_len];

            if bits_per_sample == 16 {
                pcm_samples.reserve(sample_bytes.len() / 2);
                for i in (0..sample_bytes.len().saturating_sub(1)).step_by(2) {
                    let sample = i16::from_le_bytes([sample_bytes[i], sample_bytes[i + 1]]);
                    pcm_samples.push(sample);
                }
            } else if bits_per_sample == 8 {
                pcm_samples.reserve(sample_bytes.len());
                for &b in sample_bytes {
                    // 8-bit unsigned PCM to 16-bit signed
                    let sample = ((b as i16) - 128) << 8;
                    pcm_samples.push(sample);
                }
            }
            break;
        }

        idx += chunk_len;
    }

    if pcm_samples.is_empty() {
        return Err("No PCM audio samples found in WAV file");
    }

    Ok(WavAudio {
        sample_rate,
        channels,
        bits_per_sample,
        samples: pcm_samples,
    })
}

/// Synthesize a pure sine wave audio buffer (16-bit signed PCM).
pub fn generate_sine_wave(freq_hz: u32, duration_ms: u32, sample_rate: u32) -> Vec<i16> {
    let total_samples = ((sample_rate as u64 * duration_ms as u64) / 1000) as usize;
    let mut buffer = Vec::with_capacity(total_samples * 2); // Stereo

    let phase_step = ((freq_hz as u64 * 256) / sample_rate as u64) as u32;
    let mut phase = 0u32;

    for i in 0..total_samples {
        let sample = integer_sin((phase & 0xFF) as u8);

        // Apply gentle linear attack and decay envelope to prevent popping
        let envelope = if i < 200 {
            (i as i32 * 32767) / 200
        } else if i > total_samples.saturating_sub(400) {
            ((total_samples - i) as i32 * 32767) / 400
        } else {
            32767
        };

        let attenuated = ((sample as i32 * envelope) >> 15) as i16;
        buffer.push(attenuated); // Left channel
        buffer.push(attenuated); // Right channel

        phase = phase.wrapping_add(phase_step);
    }

    buffer
}

/// Synthesize the iconic CosmicOS Sublime startup chime (F-Major Harmonic Chord: F3, C4, F4, A4).
pub fn generate_cosmic_chime(sample_rate: u32) -> Vec<i16> {
    let duration_ms = 2200; // 2.2 seconds resonant decay
    let total_samples = ((sample_rate as u64 * duration_ms as u64) / 1000) as usize;
    let mut buffer = Vec::with_capacity(total_samples * 2);

    // Harmonic F-Major Chord frequencies (Hz)
    let freqs: [u32; 4] = [174, 261, 349, 440]; // F3, C4, F4, A4
    let weights: [i32; 4] = [9000, 8000, 7500, 6000];

    for i in 0..total_samples {
        let mut chord_sample: i32 = 0;

        for j in 0..4 {
            let phase = (((i as u64 * freqs[j] as u64 * 256) / sample_rate as u64) & 0xFF) as u8;
            let val = integer_sin(phase) as i32;
            chord_sample += (val * weights[j]) >> 15;
        }

        // Exponential-like decay envelope
        let decay = if i < 150 {
            // Rapid attack (first 3.4 ms)
            (i as i32 * 32767) / 150
        } else {
            // Smooth natural decay curve
            let remaining = (total_samples - i) as i64;
            ((remaining * remaining * 32767) / (total_samples as i64 * total_samples as i64)) as i32
        };

        let final_sample = ((chord_sample * decay) >> 15).clamp(-32767, 32767) as i16;
        buffer.push(final_sample); // Left
        buffer.push(final_sample); // Right
    }

    buffer
}

/// Synthesize speech formants for phonetic speech synthesis ("say" command).
pub fn generate_phoneme_tone(char_code: u8, duration_ms: u32, sample_rate: u32) -> Vec<i16> {
    // Map characters to phonetic formant frequencies (vowels & consonants)
    let freq = match char_code.to_ascii_uppercase() {
        b'A' => 850,
        b'E' => 600,
        b'I' => 350,
        b'O' => 500,
        b'U' => 300,
        b'S' | b'C' | b'Z' => 1800,
        b'T' | b'D' | b'K' => 1200,
        b'L' | b'R' | b'W' => 450,
        b'M' | b'N' => 250,
        b' ' => 0, // Silence
        _ => 400 + ((char_code as u32 % 10) * 80),
    };

    if freq == 0 {
        let count = ((sample_rate as u64 * duration_ms as u64) / 1000) as usize;
        alloc::vec![0i16; count * 2]
    } else {
        generate_sine_wave(freq, duration_ms, sample_rate)
    }
}
