use alloc::string::String;
use alloc::vec::Vec;

#[derive(Debug, Clone)]
pub struct DecodedAudio {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub format_name: &'static str,
    pub duration_ms: u32,
    /// Interleaved 44,100 Hz 16-bit signed stereo PCM ready for Intel HDA DMA ring.
    pub samples_44k_stereo: Vec<i16>,
}

#[derive(Debug, Clone)]
pub struct AudioMetadata {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub format_name: &'static str,
    pub sample_rate: u32,
    pub channels: u16,
    pub duration_ms: u32,
}

/// Inspect audio container/stream header without decoding the entire payload.
pub fn inspect_audio(data: &[u8]) -> Option<(u32, u16, &'static str)> {
    if data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WAVE" {
        let mut idx = 12usize;
        while idx + 8 <= data.len() {
            let cid = &data[idx..idx + 4];
            let clen = u32::from_le_bytes([data[idx + 4], data[idx + 5], data[idx + 6], data[idx + 7]]) as usize;
            idx += 8;
            if cid == b"fmt " && idx + 16 <= data.len() {
                let tag = u16::from_le_bytes([data[idx], data[idx + 1]]);
                let ch = u16::from_le_bytes([data[idx + 2], data[idx + 3]]);
                let rate = u32::from_le_bytes([data[idx + 4], data[idx + 5], data[idx + 6], data[idx + 7]]);
                let name = match tag {
                    1 => "WAV PCM",
                    3 => "WAV Float32",
                    0x0011 => "WAV IMA-ADPCM",
                    0x0055 => "WAV MP3",
                    _ => "WAV Audio",
                };
                return Some((rate, ch, name));
            }
            idx = idx.saturating_add(clen + (clen & 1));
        }
    }
    if data.len() >= 16 && &data[0..4] == b"qoaf" {
        let channels = data[8] as u16;
        let rate = u32::from_be_bytes([0, data[9], data[10], data[11]]);
        if channels > 0 && rate > 0 {
            return Some((rate, channels, "QOA Audio"));
        }
    }
    if let Some((rate, ch, _br)) = super::mp3::inspect_mp3_header(data) {
        return Some((rate, ch, "MP3 Audio"));
    }
    None
}

/// Resample arbitrary mono/stereo signed 16-bit PCM to 44,100 Hz interleaved stereo `i16`.
pub fn resample_to_44k_stereo(samples: &[i16], src_rate: u32, channels: u16) -> Vec<i16> {
    let ch = channels.max(1) as usize;
    let src_frames = samples.len() / ch;
    if src_frames == 0 || src_rate == 0 {
        return Vec::new();
    }

    if src_rate == 44100 && ch == 2 {
        return samples.to_vec();
    }

    let dst_frames = ((src_frames as u64 * 44100) / src_rate as u64).max(1) as usize;
    let mut out = Vec::with_capacity(dst_frames * 2);

    for i in 0..dst_frames {
        // 16.16 fixed-point linear interpolation
        let src_fp = ((i as u64 * src_rate as u64) << 16) / 44100;
        let idx0 = ((src_fp >> 16) as usize).min(src_frames - 1);
        let idx1 = (idx0 + 1).min(src_frames - 1);
        let frac = (src_fp & 0xFFFF) as i32;

        let l0 = samples[idx0 * ch] as i32;
        let l1 = samples[idx1 * ch] as i32;
        let r0 = if ch >= 2 { samples[idx0 * ch + 1] as i32 } else { l0 };
        let r1 = if ch >= 2 { samples[idx1 * ch + 1] as i32 } else { l1 };

        let l = (l0 + (((l1 - l0) * frac) >> 16)).clamp(-32768, 32767) as i16;
        let r = (r0 + (((r1 - r0) * frac) >> 16)).clamp(-32768, 32767) as i16;
        out.push(l);
        out.push(r);
    }

    out
}

const IMA_INDEX_TABLE: [i32; 16] = [
    -1, -1, -1, -1, 2, 4, 6, 8,
    -1, -1, -1, -1, 2, 4, 6, 8,
];

const IMA_STEP_TABLE: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45,
    50, 55, 60, 66, 73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307,
    337, 371, 408, 449, 494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066,
    2272, 2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493, 10442, 11487, 12635, 13899,
    15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];

fn ima_step(nibble: u8, pred: &mut i32, idx: &mut i32) -> i16 {
    let step = IMA_STEP_TABLE[*idx as usize];
    let mut diff = step >> 3;
    if (nibble & 1) != 0 { diff += step >> 2; }
    if (nibble & 2) != 0 { diff += step >> 1; }
    if (nibble & 4) != 0 { diff += step; }
    if (nibble & 8) != 0 {
        *pred = (*pred - diff).clamp(-32768, 32767);
    } else {
        *pred = (*pred + diff).clamp(-32768, 32767);
    }
    *idx = (*idx + IMA_INDEX_TABLE[(nibble & 0x0F) as usize]).clamp(0, 88);
    *pred as i16
}

fn decode_ima_adpcm(data: &[u8], channels: usize, block_align: usize) -> Result<Vec<i16>, &'static str> {
    if channels == 0 || channels > 2 || block_align < channels * 4 {
        return Err("Unsupported IMA-ADPCM block layout");
    }
    let mut out = Vec::new();
    let mut off = 0usize;
    while off + channels * 4 <= data.len() {
        let block_end = (off + block_align).min(data.len());
        let mut pred = [0i32; 2];
        let mut step_idx = [0i32; 2];
        for c in 0..channels {
            let p = off + c * 4;
            pred[c] = i16::from_le_bytes([data[p], data[p + 1]]) as i32;
            step_idx[c] = (data[p + 2] as i32).clamp(0, 88);
            out.push(pred[c] as i16);
        }
        off += channels * 4;

        if channels == 1 {
            while off < block_end {
                let b = data[off];
                off += 1;
                out.push(ima_step(b & 0x0F, &mut pred[0], &mut step_idx[0]));
                out.push(ima_step(b >> 4, &mut pred[0], &mut step_idx[0]));
            }
        } else {
            while off + 8 <= block_end {
                let mut ch_samples = [[0i16; 8]; 2];
                for c in 0..2 {
                    for byte_i in 0..4 {
                        let b = data[off + c * 4 + byte_i];
                        ch_samples[c][byte_i * 2] = ima_step(b & 0x0F, &mut pred[c], &mut step_idx[c]);
                        ch_samples[c][byte_i * 2 + 1] = ima_step(b >> 4, &mut pred[c], &mut step_idx[c]);
                    }
                }
                for s in 0..8 {
                    out.push(ch_samples[0][s]);
                    out.push(ch_samples[1][s]);
                }
                off += 8;
            }
        }
        off = block_end;
    }
    Ok(out)
}

#[inline]
fn f32_bits_to_i16(bits: u32) -> i16 {
    let sign = (bits >> 31) != 0;
    let exp = ((bits >> 23) & 0xFF) as i32 - 127;
    let frac = (bits & 0x7F_FFFF) | 0x80_0000;
    if exp < -15 {
        return 0;
    }
    if exp >= 0 {
        return if sign { -32767 } else { 32767 };
    }
    let shift = (23 - 15 - exp) as u32;
    let mag = if shift >= 32 { 0 } else { (frac >> shift) as i32 };
    let val = if sign { -mag } else { mag };
    val.clamp(-32768, 32767) as i16
}

/// Decode WAV audio (PCM 8/16/24-bit, IEEE float32, or IMA-ADPCM 0x0011).
pub fn decode_wav(data: &[u8]) -> Result<DecodedAudio, &'static str> {
    if data.len() < 36 || &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return Err("Invalid WAV header");
    }
    let mut idx = 12usize;
    let mut format_tag = 1u16;
    let mut channels = 2u16;
    let mut sample_rate = 44100u32;
    let mut block_align = 4usize;
    let mut bits_per_sample = 16u16;
    let mut raw_samples: Vec<i16> = Vec::new();
    let mut format_name = "WAV PCM";

    while idx + 8 <= data.len() {
        let cid = &data[idx..idx + 4];
        let clen = u32::from_le_bytes([data[idx + 4], data[idx + 5], data[idx + 6], data[idx + 7]]) as usize;
        idx += 8;
        let end = idx.saturating_add(clen).min(data.len());
        if cid == b"fmt " && end - idx >= 16 {
            format_tag = u16::from_le_bytes([data[idx], data[idx + 1]]);
            channels = u16::from_le_bytes([data[idx + 2], data[idx + 3]]).clamp(1, 2);
            sample_rate = u32::from_le_bytes([data[idx + 4], data[idx + 5], data[idx + 6], data[idx + 7]]);
            block_align = u16::from_le_bytes([data[idx + 12], data[idx + 13]]) as usize;
            bits_per_sample = u16::from_le_bytes([data[idx + 14], data[idx + 15]]);
        } else if cid == b"data" {
            let chunk = &data[idx..end];
            raw_samples = decode_wav_raw_chunk(
                format_tag,
                channels,
                block_align,
                bits_per_sample,
                chunk,
            )?;
            format_name = match format_tag {
                1 => "WAV PCM",
                3 => "WAV Float32",
                0x0011 => "WAV IMA-ADPCM",
                0x0055 => "WAV MP3",
                _ => "WAV Audio",
            };
            break;
        }
        idx = end + (clen & 1);
    }

    if raw_samples.is_empty() {
        return Err("WAV contains no playable samples");
    }
    let samples_44k_stereo = resample_to_44k_stereo(&raw_samples, sample_rate, channels);
    let duration_ms = ((samples_44k_stereo.len() / 2) as u64 * 1000 / 44100) as u32;
    Ok(DecodedAudio {
        sample_rate,
        channels,
        bits_per_sample,
        format_name,
        duration_ms,
        samples_44k_stereo,
    })
}

pub fn decode_wav_raw_chunk(
    format_tag: u16,
    channels: u16,
    block_align: usize,
    bits_per_sample: u16,
    chunk: &[u8],
) -> Result<Vec<i16>, &'static str> {
    match format_tag {
        1 => {
            if bits_per_sample == 16 {
                let mut out = Vec::with_capacity(chunk.len() / 2);
                for pair in chunk.chunks_exact(2) {
                    out.push(i16::from_le_bytes([pair[0], pair[1]]));
                }
                Ok(out)
            } else if bits_per_sample == 8 {
                Ok(chunk.iter().map(|&b| ((b as i16) - 128) << 8).collect())
            } else if bits_per_sample == 24 {
                let mut out = Vec::with_capacity(chunk.len() / 3);
                for tri in chunk.chunks_exact(3) {
                    out.push(i16::from_le_bytes([tri[1], tri[2]]));
                }
                Ok(out)
            } else {
                Err("Unsupported WAV PCM bit depth")
            }
        }
        3 => {
            let mut out = Vec::with_capacity(chunk.len() / 4);
            for quad in chunk.chunks_exact(4) {
                let bits = u32::from_le_bytes([quad[0], quad[1], quad[2], quad[3]]);
                out.push(f32_bits_to_i16(bits));
            }
            Ok(out)
        }
        0x0011 => decode_ima_adpcm(chunk, channels as usize, block_align),
        0x0055 => {
            let mp3 = super::mp3::decode_mp3(chunk)?;
            Ok(mp3.samples_44k_stereo)
        }
        _ => Err("Unsupported WAV format tag"),
    }
}

// =========================================================================
// Quite OK Audio (QOA) Codec
// =========================================================================

const QOA_DEQUANT_TAB: [[i32; 8]; 16] = [
    [   1,    -1,    3,    -3,    5,    -5,     7,     -7],
    [   5,    -5,   18,   -18,   32,   -32,    49,    -49],
    [  16,   -16,   53,   -53,   95,   -95,   147,   -147],
    [  34,   -34,  113,  -113,  203,  -203,   315,   -315],
    [  63,   -63,  210,  -210,  378,  -378,   588,   -588],
    [ 104,  -104,  345,  -345,  621,  -621,   966,   -966],
    [ 158,  -158,  528,  -528,  950,  -950,  1477,  -1477],
    [ 228,  -228,  760,  -760, 1368, -1368,  2128,  -2128],
    [ 316,  -316, 1053, -1053, 1895, -1895,  2947,  -2947],
    [ 422,  -422, 1405, -1405, 2529, -2529,  3934,  -3934],
    [ 548,  -548, 1828, -1828, 3290, -3290,  5117,  -5117],
    [ 696,  -696, 2320, -2320, 4176, -4176,  6496,  -6496],
    [ 868,  -868, 2893, -2893, 5207, -5207,  8099,  -8099],
    [1064, -1064, 3548, -3548, 6386, -6386,  9933,  -9933],
    [1286, -1286, 4288, -4288, 7718, -7718, 12005, -12005],
    [1536, -1536, 5120, -5120, 9216, -9216, 14336, -14336],
];

#[derive(Clone, Copy, Default)]
struct QoaLms {
    history: [i32; 4],
    weights: [i32; 4],
}

impl QoaLms {
    fn predict(&self) -> i32 {
        let mut p = 0i32;
        for i in 0..4 {
            p += self.weights[i] * self.history[i];
        }
        p >> 13
    }

    fn update(&mut self, sample: i16, residual: i32) {
        let delta = residual >> 4;
        for i in 0..4 {
            self.weights[i] += if self.history[i] < 0 { -delta } else { delta };
        }
        for i in 0..3 {
            self.history[i] = self.history[i + 1];
        }
        self.history[3] = sample as i32;
    }
}

/// Decode a Quite OK Audio (`qoaf`) container into 44.1 kHz stereo PCM.
pub fn decode_qoa(data: &[u8]) -> Result<DecodedAudio, &'static str> {
    if data.len() < 16 || &data[0..4] != b"qoaf" {
        return Err("Invalid QOA header");
    }
    let _total_samples = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let mut pos = 8usize;
    let mut out = Vec::new();
    let mut first_rate = 44100u32;
    let mut first_ch = 1u16;

    while pos + 8 <= data.len() {
        let ch = data[pos] as usize;
        let rate = u32::from_be_bytes([0, data[pos + 1], data[pos + 2], data[pos + 3]]);
        let samples_per_ch = u16::from_be_bytes([data[pos + 4], data[pos + 5]]) as usize;
        let frame_size = u16::from_be_bytes([data[pos + 6], data[pos + 7]]) as usize;
        if ch == 0 || ch > 2 || rate == 0 || pos + frame_size > data.len() || frame_size < 8 + ch * 16 {
            break;
        }
        if out.is_empty() {
            first_rate = rate;
            first_ch = ch as u16;
        }
        let mut p = pos + 8;
        let mut lms = [QoaLms::default(); 2];
        for c in 0..ch {
            for i in 0..4 {
                lms[c].history[i] = i16::from_be_bytes([data[p + i * 2], data[p + i * 2 + 1]]) as i32;
                lms[c].weights[i] = i16::from_be_bytes([data[p + 8 + i * 2], data[p + 8 + i * 2 + 1]]) as i32;
            }
            p += 16;
        }

        let slices = (samples_per_ch + 19) / 20;
        let frame_base = out.len();
        out.resize(frame_base + samples_per_ch * ch, 0);

        for s_idx in 0..slices {
            let slice_start = s_idx * 20;
            let slice_len = (samples_per_ch - slice_start).min(20);
            for c in 0..ch {
                if p + 8 > pos + frame_size {
                    return Err("Truncated QOA slice");
                }
                let mut slice = u64::from_be_bytes([
                    data[p], data[p + 1], data[p + 2], data[p + 3],
                    data[p + 4], data[p + 5], data[p + 6], data[p + 7],
                ]);
                p += 8;
                let sf = ((slice >> 60) & 0x0F) as usize;
                for si in 0..slice_len {
                    let predicted = lms[c].predict();
                    let quantized = ((slice >> (57 - si * 3)) & 0x07) as usize;
                    let dequantized = QOA_DEQUANT_TAB[sf][quantized];
                    let reconstructed = (predicted + dequantized).clamp(-32768, 32767) as i16;
                    lms[c].update(reconstructed, dequantized);
                    out[frame_base + (slice_start + si) * ch + c] = reconstructed;
                }
                let _ = &mut slice;
            }
        }
        pos += frame_size;
    }

    if out.is_empty() {
        return Err("Empty QOA audio");
    }
    let samples_44k_stereo = resample_to_44k_stereo(&out, first_rate, first_ch);
    let duration_ms = ((samples_44k_stereo.len() / 2) as u64 * 1000 / 44100) as u32;
    Ok(DecodedAudio {
        sample_rate: first_rate,
        channels: first_ch,
        bits_per_sample: 16,
        format_name: "QOA Audio",
        duration_ms,
        samples_44k_stereo,
    })
}

/// Encode mono 16-bit PCM into a valid QOA (`qoaf`) byte stream.
pub fn encode_qoa_mono(sample_rate: u32, samples: &[i16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() / 2 + 64);
    out.extend_from_slice(b"qoaf");
    out.extend_from_slice(&(samples.len() as u32).to_be_bytes());

    let mut lms = QoaLms {
        history: [0; 4],
        weights: [0, 0, -(1 << 13), 1 << 14],
    };

    let mut pos = 0usize;
    while pos < samples.len() {
        let frame_samples = (samples.len() - pos).min(256 * 20);
        let slices = (frame_samples + 19) / 20;
        let frame_size = 8 + 16 + slices * 8;
        out.push(1); // 1 channel
        let sr = sample_rate.to_be_bytes();
        out.extend_from_slice(&sr[1..4]);
        out.extend_from_slice(&(frame_samples as u16).to_be_bytes());
        out.extend_from_slice(&(frame_size as u16).to_be_bytes());

        for i in 0..4 {
            out.extend_from_slice(&(lms.history[i] as i16).to_be_bytes());
        }
        for i in 0..4 {
            out.extend_from_slice(&(lms.weights[i] as i16).to_be_bytes());
        }

        for s in 0..slices {
            let chunk = &samples[pos + s * 20..(pos + (s + 1) * 20).min(pos + frame_samples)];
            let mut best_rank = u64::MAX;
            let mut best_slice = 0u64;
            let mut best_lms = lms;

            for sf in 0..16 {
                let mut test_lms = lms;
                let mut err = 0u64;
                let mut slice_val = (sf as u64) << 60;
                for (si, &sample) in chunk.iter().enumerate() {
                    let pred = test_lms.predict();
                    let diff = sample as i32 - pred;
                    let mut best_q = 0usize;
                    let mut best_q_err = i64::MAX;
                    let mut best_rec = 0i16;
                    let mut best_dq = 0i32;
                    for q in 0..8 {
                        let dq = QOA_DEQUANT_TAB[sf][q];
                        let rec = (pred + dq).clamp(-32768, 32767) as i16;
                        let e = (sample as i64 - rec as i64).abs();
                        if e < best_q_err {
                            best_q_err = e;
                            best_q = q;
                            best_rec = rec;
                            best_dq = dq;
                        }
                    }
                    let _ = diff;
                    err = err.saturating_add((best_q_err * best_q_err) as u64);
                    test_lms.update(best_rec, best_dq);
                    slice_val |= (best_q as u64) << (57 - si * 3);
                }
                if err < best_rank {
                    best_rank = err;
                    best_slice = slice_val;
                    best_lms = test_lms;
                }
            }
            lms = best_lms;
            out.extend_from_slice(&best_slice.to_be_bytes());
        }
        pos += frame_samples;
    }

    out
}

/// Decode any supported audio file (WAV PCM/Float/IMA-ADPCM, QOA, or MP3).
pub fn decode_audio(data: &[u8]) -> Result<DecodedAudio, &'static str> {
    if data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WAVE" {
        return decode_wav(data);
    }
    if data.len() >= 16 && &data[0..4] == b"qoaf" {
        return decode_qoa(data);
    }
    if super::mp3::inspect_mp3_header(data).is_some() {
        return super::mp3::decode_mp3(data);
    }
    Err("Unsupported audio format (use WAV, IMA-ADPCM, QOA, or MP3)")
}

fn trim_meta_str(raw: &[u8]) -> String {
    let clean: Vec<u8> = raw
        .iter()
        .copied()
        .take_while(|&b| b != 0)
        .map(|b| if (0x20..=0x7E).contains(&b) { b } else { b' ' })
        .collect();
    String::from(core::str::from_utf8(&clean).unwrap_or("").trim())
}

fn stem_from_path(path: &str) -> String {
    let file = path.rsplit('/').next().unwrap_or(path);
    match file.rfind('.') {
        Some(dot) if dot > 0 => String::from(&file[..dot]),
        _ => String::from(file),
    }
}

/// Inspect audio header, duration, and container tags (WAV LIST/INFO, QOA header, MP3 ID3v1/v2).
pub fn inspect_audio_metadata(path: &str, data: &[u8]) -> AudioMetadata {
    let mut meta = AudioMetadata {
        title: stem_from_path(path),
        artist: String::from("Unknown Artist"),
        album: String::from("SublimeOS Library"),
        format_name: "Audio",
        sample_rate: 44100,
        channels: 2,
        duration_ms: 0,
    };

    if data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WAVE" {
        let mut idx = 12usize;
        let mut byte_rate = 0u32;
        let mut block_align = 0u32;
        let mut format_tag = 1u16;
        let mut data_bytes = 0u64;
        while idx + 8 <= data.len() {
            let cid = &data[idx..idx + 4];
            let clen = u32::from_le_bytes([data[idx + 4], data[idx + 5], data[idx + 6], data[idx + 7]]) as usize;
            idx += 8;
            let end = idx.saturating_add(clen).min(data.len());
            if cid == b"fmt " && end - idx >= 16 {
                format_tag = u16::from_le_bytes([data[idx], data[idx + 1]]);
                meta.channels = u16::from_le_bytes([data[idx + 2], data[idx + 3]]).clamp(1, 2);
                meta.sample_rate = u32::from_le_bytes([data[idx + 4], data[idx + 5], data[idx + 6], data[idx + 7]]).max(1);
                byte_rate = u32::from_le_bytes([data[idx + 8], data[idx + 9], data[idx + 10], data[idx + 11]]);
                block_align = u16::from_le_bytes([data[idx + 12], data[idx + 13]]) as u32;
                meta.format_name = match format_tag {
                    1 => "WAV PCM",
                    3 => "WAV Float32",
                    0x0011 => "WAV IMA-ADPCM",
                    0x0055 => "WAV MP3",
                    _ => "WAV Audio",
                };
            } else if cid == b"data" {
                data_bytes = (end - idx) as u64;
            } else if cid == b"LIST" && end - idx >= 4 && &data[idx..idx + 4] == b"INFO" {
                let mut sub = idx + 4;
                while sub + 8 <= end {
                    let scid = &data[sub..sub + 4];
                    let sclen = u32::from_le_bytes([data[sub + 4], data[sub + 5], data[sub + 6], data[sub + 7]]) as usize;
                    sub += 8;
                    let send = sub.saturating_add(sclen).min(end);
                    let txt = trim_meta_str(&data[sub..send]);
                    if !txt.is_empty() {
                        if scid == b"INAM" {
                            meta.title = txt;
                        } else if scid == b"IART" {
                            meta.artist = txt;
                        } else if scid == b"IPRD" {
                            meta.album = txt;
                        }
                    }
                    sub = sub.saturating_add(sclen + (sclen & 1));
                }
            }
            idx = idx.saturating_add(clen + (clen & 1));
        }
        if format_tag == 0x0011 && block_align > 4 && meta.channels == 1 && meta.sample_rate > 0 {
            let blocks = data_bytes / block_align as u64;
            let samples_per_block = 1 + (block_align as u64 - 4) * 2;
            meta.duration_ms = ((blocks * samples_per_block * 1000) / meta.sample_rate as u64) as u32;
        } else if byte_rate > 0 {
            meta.duration_ms = ((data_bytes * 1000) / byte_rate as u64) as u32;
        }
        return meta;
    }

    if data.len() >= 16 && &data[0..4] == b"qoaf" {
        let qoa_samples = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        meta.channels = (data[8] as u16).clamp(1, 2);
        meta.sample_rate = u32::from_be_bytes([0, data[9], data[10], data[11]]).max(1);
        meta.format_name = "QOA Audio";
        meta.duration_ms = ((qoa_samples as u64 * 1000) / meta.sample_rate as u64) as u32;
        return meta;
    }

    if let Some((sr, ch, br_kbps)) = super::mp3::inspect_mp3_header(data) {
        meta.sample_rate = sr;
        meta.channels = ch;
        meta.format_name = "MP3 Audio";
        let mut payload_start = 0usize;
        if data.len() >= 10 && &data[0..3] == b"ID3" {
            let ver = data[3];
            let tag_sz = (((data[6] as usize & 0x7F) << 21)
                | ((data[7] as usize & 0x7F) << 14)
                | ((data[8] as usize & 0x7F) << 7)
                | (data[9] as usize & 0x7F))
                .min(data.len().saturating_sub(10));
            payload_start = 10 + tag_sz;
            if ver == 3 || ver == 4 {
                let mut fpos = 10usize;
                let tag_end = (10 + tag_sz).min(data.len());
                while fpos + 10 <= tag_end {
                    let fid = &data[fpos..fpos + 4];
                    if fid[0] == 0 {
                        break;
                    }
                    let fsz = if ver == 4 {
                        ((data[fpos + 4] as usize & 0x7F) << 21)
                            | ((data[fpos + 5] as usize & 0x7F) << 14)
                            | ((data[fpos + 6] as usize & 0x7F) << 7)
                            | (data[fpos + 7] as usize & 0x7F)
                    } else {
                        u32::from_be_bytes([data[fpos + 4], data[fpos + 5], data[fpos + 6], data[fpos + 7]]) as usize
                    };
                    fpos += 10;
                    if fsz == 0 || fpos + fsz > tag_end {
                        break;
                    }
                    let body = &data[fpos..fpos + fsz];
                    if body.len() > 1 && (body[0] == 0 || body[0] == 3) {
                        let txt = trim_meta_str(&body[1..]);
                        if !txt.is_empty() {
                            if fid == b"TIT2" {
                                meta.title = txt;
                            } else if fid == b"TPE1" {
                                meta.artist = txt;
                            } else if fid == b"TALB" {
                                meta.album = txt;
                            }
                        }
                    }
                    fpos += fsz;
                }
            }
        }
        let mut payload_end = data.len();
        if data.len() >= 128 && &data[data.len() - 128..data.len() - 125] == b"TAG" {
            let base = data.len() - 128;
            payload_end = base;
            let t = trim_meta_str(&data[base + 3..base + 33]);
            let a = trim_meta_str(&data[base + 33..base + 63]);
            let al = trim_meta_str(&data[base + 63..base + 93]);
            if !t.is_empty() { meta.title = t; }
            if !a.is_empty() { meta.artist = a; }
            if !al.is_empty() { meta.album = al; }
        }
        if br_kbps > 0 {
            let audio_bytes = payload_end.saturating_sub(payload_start) as u64;
            meta.duration_ms = ((audio_bytes * 8) / br_kbps as u64) as u32;
        }
    }

    meta
}

#[inline]
fn ima_encode_sample(sample: i16, pred: &mut i32, idx: &mut i32) -> u8 {
    let step = IMA_STEP_TABLE[*idx as usize];
    let diff = sample as i32 - *pred;
    let mut nibble = 0u8;
    let mut mag = if diff < 0 {
        nibble = 8;
        -diff
    } else {
        diff
    };
    let mut delta = step >> 3;
    if mag >= step {
        nibble |= 4;
        mag -= step;
        delta += step;
    }
    if mag >= (step >> 1) {
        nibble |= 2;
        mag -= step >> 1;
        delta += step >> 1;
    }
    if mag >= (step >> 2) {
        nibble |= 1;
        delta += step >> 2;
    }
    *pred = if (nibble & 8) != 0 {
        (*pred - delta).clamp(-32768, 32767)
    } else {
        (*pred + delta).clamp(-32768, 32767)
    };
    *idx = (*idx + IMA_INDEX_TABLE[nibble as usize]).clamp(0, 88);
    nibble
}

fn push_info_subchunk(list_payload: &mut Vec<u8>, tag: &[u8; 4], text: &str) {
    if text.is_empty() {
        return;
    }
    let raw = text.as_bytes();
    let len = raw.len() + 1; // include NUL terminator
    list_payload.extend_from_slice(tag);
    list_payload.extend_from_slice(&(len as u32).to_le_bytes());
    list_payload.extend_from_slice(raw);
    list_payload.push(0);
    if (len & 1) != 0 {
        list_payload.push(0);
    }
}

/// Encode mono 16-bit PCM into a compact RIFF/WAVE IMA-ADPCM (`0x0011`) file with RIFF INFO tags.
pub fn encode_ima_adpcm_mono(
    sample_rate: u32,
    samples: &[i16],
    title: &str,
    artist: &str,
    album: &str,
) -> Vec<u8> {
    const BLOCK_ALIGN: usize = 256;
    const SAMPLES_PER_BLOCK: usize = 1 + (BLOCK_ALIGN - 4) * 2; // 505 samples/block

    let mut data_chunk = Vec::new();
    let mut pos = 0usize;
    let mut pred = if samples.is_empty() { 0i32 } else { samples[0] as i32 };
    let mut step_idx = 0i32;

    while pos < samples.len() {
        if pos > 0 {
            pred = samples[pos] as i32;
        }
        data_chunk.extend_from_slice(&(pred as i16).to_le_bytes());
        data_chunk.push(step_idx as u8);
        data_chunk.push(0);
        pos += 1;

        for _ in 0..(BLOCK_ALIGN - 4) {
            let s0 = if pos < samples.len() { samples[pos] } else { 0 };
            pos += 1;
            let s1 = if pos < samples.len() { samples[pos] } else { 0 };
            pos += 1;
            let n0 = ima_encode_sample(s0, &mut pred, &mut step_idx);
            let n1 = ima_encode_sample(s1, &mut pred, &mut step_idx);
            data_chunk.push((n0 & 0x0F) | ((n1 & 0x0F) << 4));
        }
    }

    let num_blocks = data_chunk.len() / BLOCK_ALIGN;
    let byte_rate = if SAMPLES_PER_BLOCK > 0 {
        ((sample_rate as u64 * BLOCK_ALIGN as u64) / SAMPLES_PER_BLOCK as u64) as u32
    } else {
        sample_rate / 2
    };

    let mut list_payload = Vec::new();
    list_payload.extend_from_slice(b"INFO");
    push_info_subchunk(&mut list_payload, b"INAM", title);
    push_info_subchunk(&mut list_payload, b"IART", artist);
    push_info_subchunk(&mut list_payload, b"IPRD", album);

    let total_chunks = 4 + 28 + (8 + data_chunk.len()) + if list_payload.len() > 4 { 8 + list_payload.len() } else { 0 };
    let _ = num_blocks;

    let mut out = Vec::with_capacity(8 + total_chunks);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(total_chunks as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&20u32.to_le_bytes());
    out.extend_from_slice(&0x0011u16.to_le_bytes()); // IMA-ADPCM
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&(BLOCK_ALIGN as u16).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes()); // cbSize
    out.extend_from_slice(&(SAMPLES_PER_BLOCK as u16).to_le_bytes());

    if list_payload.len() > 4 {
        out.extend_from_slice(b"LIST");
        out.extend_from_slice(&(list_payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&list_payload);
    }

    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_chunk.len() as u32).to_le_bytes());
    out.extend_from_slice(&data_chunk);
    out
}
