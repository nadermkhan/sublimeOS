use alloc::vec;
use alloc::vec::Vec;
use super::audio::resample_to_44k_stereo;
use super::DecodedImage;

#[derive(Clone)]
pub struct AviMovie {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub video_frames_raw: Vec<Vec<u8>>,
    pub is_mjpeg: bool,
    pub audio_samples_44k_stereo: Vec<i16>,
    pub duration_ms: u32,
}

impl AviMovie {
    /// Decode frame `index` (MJPEG or uncompressed BGR DIB) into ARGB8888 pixels.
    pub fn decode_frame(&self, index: usize) -> Result<DecodedImage, &'static str> {
        let raw = self.video_frames_raw.get(index).ok_or("AVI frame index out of range")?;
        if self.is_mjpeg {
            return super::jpeg::decode_jpeg(raw);
        }
        // Uncompressed BGR24 DIB (bottom-up)
        let w = self.width as usize;
        let h = self.height as usize;
        let stride = ((w * 3) + 3) & !3;
        if raw.len() < stride * h {
            return Err("Truncated uncompressed AVI video frame");
        }
        let mut pixels = vec![0xFF00_0000u32; w * h];
        for row in 0..h {
            let src_row = h - 1 - row;
            for col in 0..w {
                let p = src_row * stride + col * 3;
                let b = raw[p] as u32;
                let g = raw[p + 1] as u32;
                let r = raw[p + 2] as u32;
                pixels[row * w + col] = 0xFF00_0000 | (r << 16) | (g << 8) | b;
            }
        }
        Ok(DecodedImage {
            width: self.width,
            height: self.height,
            pixels,
            format_name: "AVI Frame",
        })
    }
}

/// Inspect RIFF AVI container header without decoding frames.
pub fn inspect_avi(data: &[u8]) -> Option<(u32, u32, u32, &'static str)> {
    if data.len() < 64 || &data[0..4] != b"RIFF" || &data[8..12] != b"AVI " {
        return None;
    }
    let mut pos = 12usize;
    while pos + 12 <= data.len() {
        let id = &data[pos..pos + 4];
        let size = u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]) as usize;
        if id == b"LIST" && &data[pos + 8..pos + 12] == b"hdrl" {
            let mut p = pos + 12;
            let end = (pos + 8 + size).min(data.len());
            while p + 8 <= end {
                let cid = &data[p..p + 4];
                let clen = u32::from_le_bytes([data[p + 4], data[p + 5], data[p + 6], data[p + 7]]) as usize;
                p += 8;
                if cid == b"avih" && p + 40 <= end {
                    let us_per_frame = u32::from_le_bytes([data[p], data[p + 1], data[p + 2], data[p + 3]]).max(1);
                    let w = u32::from_le_bytes([data[p + 32], data[p + 33], data[p + 34], data[p + 35]]);
                    let h = u32::from_le_bytes([data[p + 36], data[p + 37], data[p + 38], data[p + 39]]);
                    let fps = (1_000_000 / us_per_frame).clamp(1, 120);
                    if w > 0 && h > 0 {
                        return Some((w, h, fps, "AVI MJPEG Video"));
                    }
                }
                if cid == b"LIST" {
                    p += 4;
                } else {
                    p = p.saturating_add(clen + (clen & 1));
                }
            }
            break;
        }
        pos = pos.saturating_add(8 + size + (size & 1));
    }
    None
}

/// Demux a RIFF `AVI ` container (MJPEG / RGB video stream + PCM/ADPCM audio chunks).
pub fn decode_avi(data: &[u8]) -> Result<AviMovie, &'static str> {
    if data.len() < 64 || &data[0..4] != b"RIFF" || &data[8..12] != b"AVI " {
        return Err("Invalid RIFF AVI signature");
    }

    let mut width = 0u32;
    let mut height = 0u32;
    let mut fps = 15u32;
    let mut is_mjpeg = true;

    let mut aud_format = 1u16;
    let mut aud_ch = 2u16;
    let mut aud_rate = 44100u32;
    let mut aud_block = 4usize;
    let mut aud_bits = 16u16;
    let mut cur_stream_kind: [u8; 4] = [0; 4];

    let mut video_frames_raw: Vec<Vec<u8>> = Vec::new();
    let mut audio_raw_bytes: Vec<u8> = Vec::new();

    fn walk_chunks(
        slice: &[u8],
        base: usize,
        limit: usize,
        width: &mut u32,
        height: &mut u32,
        fps: &mut u32,
        is_mjpeg: &mut bool,
        cur_stream_kind: &mut [u8; 4],
        aud_format: &mut u16,
        aud_ch: &mut u16,
        aud_rate: &mut u32,
        aud_block: &mut usize,
        aud_bits: &mut u16,
        video_frames_raw: &mut Vec<Vec<u8>>,
        audio_raw_bytes: &mut Vec<u8>,
    ) {
        let mut pos = base;
        while pos + 8 <= limit {
            let cid = &slice[pos..pos + 4];
            let clen = u32::from_le_bytes([slice[pos + 4], slice[pos + 5], slice[pos + 6], slice[pos + 7]]) as usize;
            let data_start = pos + 8;
            let data_end = data_start.saturating_add(clen).min(limit);

            if cid == b"LIST" && data_start + 4 <= data_end {
                walk_chunks(
                    slice,
                    data_start + 4,
                    data_end,
                    width,
                    height,
                    fps,
                    is_mjpeg,
                    cur_stream_kind,
                    aud_format,
                    aud_ch,
                    aud_rate,
                    aud_block,
                    aud_bits,
                    video_frames_raw,
                    audio_raw_bytes,
                );
            } else {
                let payload = &slice[data_start..data_end];
                if cid == b"avih" && payload.len() >= 40 {
                    let us = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]).max(1);
                    *fps = (1_000_000 / us).clamp(1, 120);
                    *width = u32::from_le_bytes([payload[32], payload[33], payload[34], payload[35]]);
                    *height = u32::from_le_bytes([payload[36], payload[37], payload[38], payload[39]]);
                } else if cid == b"strh" && payload.len() >= 8 {
                    cur_stream_kind.copy_from_slice(&payload[0..4]);
                    if cur_stream_kind == b"vids" {
                        let handler = &payload[4..8];
                        if handler == b"DIB " || handler == b"\0\0\0\0" {
                            *is_mjpeg = false;
                        } else {
                            *is_mjpeg = true;
                        }
                    }
                } else if cid == b"strf" {
                    if cur_stream_kind == b"vids" && payload.len() >= 20 {
                        let bi_comp = &payload[16..20];
                        *is_mjpeg = bi_comp == b"MJPG" || bi_comp == b"mjpg" || bi_comp == b"JPEG";
                    } else if cur_stream_kind == b"auds" && payload.len() >= 16 {
                        *aud_format = u16::from_le_bytes([payload[0], payload[1]]);
                        *aud_ch = u16::from_le_bytes([payload[2], payload[3]]).clamp(1, 2);
                        *aud_rate = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
                        *aud_block = u16::from_le_bytes([payload[12], payload[13]]) as usize;
                        *aud_bits = u16::from_le_bytes([payload[14], payload[15]]);
                    }
                } else if (cid == b"00dc" || cid == b"00db") && !payload.is_empty() {
                    if payload.len() >= 2 && payload[0] == 0xFF && payload[1] == 0xD8 {
                        *is_mjpeg = true;
                    }
                    video_frames_raw.push(payload.to_vec());
                } else if cid == b"01wb" && !payload.is_empty() {
                    audio_raw_bytes.extend_from_slice(payload);
                }
            }
            pos = data_end + (clen & 1);
        }
    }

    walk_chunks(
        data,
        12,
        data.len(),
        &mut width,
        &mut height,
        &mut fps,
        &mut is_mjpeg,
        &mut cur_stream_kind,
        &mut aud_format,
        &mut aud_ch,
        &mut aud_rate,
        &mut aud_block,
        &mut aud_bits,
        &mut video_frames_raw,
        &mut audio_raw_bytes,
    );

    if width == 0 || height == 0 || video_frames_raw.is_empty() {
        return Err("AVI container has no video frames");
    }

    let audio_samples_44k_stereo = if !audio_raw_bytes.is_empty() {
        if let Ok(raw) = super::audio::decode_wav_raw_chunk(
            aud_format,
            aud_ch,
            aud_block,
            aud_bits,
            &audio_raw_bytes,
        ) {
            resample_to_44k_stereo(&raw, aud_rate, aud_ch)
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let duration_ms = ((video_frames_raw.len() as u32) * 1000) / fps.max(1);
    Ok(AviMovie {
        width,
        height,
        fps,
        video_frames_raw,
        is_mjpeg,
        audio_samples_44k_stereo,
        duration_ms,
    })
}

fn push_chunk(out: &mut Vec<u8>, tag: &[u8; 4], payload: &[u8]) {
    out.extend_from_slice(tag);
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    if (payload.len() & 1) != 0 {
        out.push(0);
    }
}

fn push_list(out: &mut Vec<u8>, list_type: &[u8; 4], inner: &[u8]) {
    out.extend_from_slice(b"LIST");
    out.extend_from_slice(&((4 + inner.len()) as u32).to_le_bytes());
    out.extend_from_slice(list_type);
    out.extend_from_slice(inner);
    if (inner.len() & 1) != 0 {
        out.push(0);
    }
}

/// Build a valid RIFF AVI (MJPEG video + optional 16-bit PCM stereo 44.1kHz audio) file.
pub fn build_mjpeg_avi(
    width: u32,
    height: u32,
    fps: u32,
    frames_argb: &[Vec<u32>],
    audio_pcm_stereo_44k: &[i16],
) -> Result<Vec<u8>, &'static str> {
    if frames_argb.is_empty() {
        return Err("Cannot build AVI without frames");
    }
    let fps = fps.clamp(1, 60);
    let us_per_frame = 1_000_000 / fps;

    // Encode each frame as Baseline JPEG
    let mut jpeg_frames = Vec::with_capacity(frames_argb.len());
    let mut max_frame_bytes = 0usize;
    for f in frames_argb {
        let jpg = super::jpeg::encode_baseline_jpeg(width, height, f)?;
        max_frame_bytes = max_frame_bytes.max(jpg.len());
        jpeg_frames.push(jpg);
    }

    // Build hdrl
    let mut avih = vec![0u8; 56];
    avih[0..4].copy_from_slice(&us_per_frame.to_le_bytes());
    avih[4..8].copy_from_slice(&((max_frame_bytes as u32) * fps).to_le_bytes());
    avih[16..20].copy_from_slice(&(frames_argb.len() as u32).to_le_bytes());
    avih[24..28].copy_from_slice(&(if audio_pcm_stereo_44k.is_empty() { 1u32 } else { 2u32 }).to_le_bytes());
    avih[28..32].copy_from_slice(&(max_frame_bytes as u32).to_le_bytes());
    avih[32..36].copy_from_slice(&width.to_le_bytes());
    avih[36..40].copy_from_slice(&height.to_le_bytes());

    let mut hdrl_body = Vec::new();
    push_chunk(&mut hdrl_body, b"avih", &avih);

    // Video strl
    let mut strh_v = vec![0u8; 56];
    strh_v[0..4].copy_from_slice(b"vids");
    strh_v[4..8].copy_from_slice(b"MJPG");
    strh_v[20..24].copy_from_slice(&1u32.to_le_bytes()); // dwScale
    strh_v[24..28].copy_from_slice(&fps.to_le_bytes()); // dwRate
    strh_v[32..36].copy_from_slice(&(frames_argb.len() as u32).to_le_bytes());
    let mut strf_v = vec![0u8; 40];
    strf_v[0..4].copy_from_slice(&40u32.to_le_bytes());
    strf_v[4..8].copy_from_slice(&(width as i32).to_le_bytes());
    strf_v[8..12].copy_from_slice(&(height as i32).to_le_bytes());
    strf_v[12..14].copy_from_slice(&1u16.to_le_bytes());
    strf_v[14..16].copy_from_slice(&24u16.to_le_bytes());
    strf_v[16..20].copy_from_slice(b"MJPG");
    let mut strl_v = Vec::new();
    push_chunk(&mut strl_v, b"strh", &strh_v);
    push_chunk(&mut strl_v, b"strf", &strf_v);
    push_list(&mut hdrl_body, b"strl", &strl_v);

    // Audio strl if present
    if !audio_pcm_stereo_44k.is_empty() {
        let mut strh_a = vec![0u8; 56];
        strh_a[0..4].copy_from_slice(b"auds");
        strh_a[20..24].copy_from_slice(&4u32.to_le_bytes());
        strh_a[24..28].copy_from_slice(&(44100u32 * 4).to_le_bytes());
        strh_a[44..48].copy_from_slice(&4u32.to_le_bytes());
        let mut strf_a = vec![0u8; 18];
        strf_a[0..2].copy_from_slice(&1u16.to_le_bytes()); // PCM
        strf_a[2..4].copy_from_slice(&2u16.to_le_bytes()); // stereo
        strf_a[4..8].copy_from_slice(&44100u32.to_le_bytes());
        strf_a[8..12].copy_from_slice(&(44100u32 * 4).to_le_bytes());
        strf_a[12..14].copy_from_slice(&4u16.to_le_bytes());
        strf_a[14..16].copy_from_slice(&16u16.to_le_bytes());
        let mut strl_a = Vec::new();
        push_chunk(&mut strl_a, b"strh", &strh_a);
        push_chunk(&mut strl_a, b"strf", &strf_a);
        push_list(&mut hdrl_body, b"strl", &strl_a);
    }

    // Build movi interleaved chunks
    let mut movi_body = Vec::new();
    let samples_per_frame = if frames_argb.is_empty() { 0 } else { (audio_pcm_stereo_44k.len() / 2) / frames_argb.len() };
    let mut audio_pos = 0usize;
    for (idx, jpg) in jpeg_frames.iter().enumerate() {
        push_chunk(&mut movi_body, b"00dc", jpg);
        if !audio_pcm_stereo_44k.is_empty() {
            let end_sample = if idx + 1 == jpeg_frames.len() {
                audio_pcm_stereo_44k.len() / 2
            } else {
                (audio_pos + samples_per_frame).min(audio_pcm_stereo_44k.len() / 2)
            };
            if end_sample > audio_pos {
                let slice = &audio_pcm_stereo_44k[audio_pos * 2..end_sample * 2];
                let mut pcm_bytes = Vec::with_capacity(slice.len() * 2);
                for &s in slice {
                    pcm_bytes.extend_from_slice(&s.to_le_bytes());
                }
                push_chunk(&mut movi_body, b"01wb", &pcm_bytes);
                audio_pos = end_sample;
            }
        }
    }

    let mut riff_body = Vec::new();
    riff_body.extend_from_slice(b"AVI ");
    push_list(&mut riff_body, b"hdrl", &hdrl_body);
    push_list(&mut riff_body, b"movi", &movi_body);

    let mut out = Vec::with_capacity(8 + riff_body.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(riff_body.len() as u32).to_le_bytes());
    out.extend_from_slice(&riff_body);
    Ok(out)
}
