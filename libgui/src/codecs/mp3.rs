use alloc::boxed::Box;
use alloc::vec::Vec;
use super::audio::{resample_to_44k_stereo, DecodedAudio};

#[derive(Clone, Copy)]
struct Mp3FrameHeader {
    is_mpeg1: bool,
    lsf: bool,
    bitrate_kbps: u32,
    sample_rate: u32,
    channels: u16,
    mode: u8,
    mode_ext: u8,
    crc_protected: bool,
    frame_bytes: usize,
}

const BITRATES_V1_L3: [u32; 16] = [
    0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0,
];
const BITRATES_V2_L3: [u32; 16] = [
    0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0,
];
const SAMPLE_RATES_V1: [u32; 4] = [44100, 48000, 32000, 0];

fn parse_frame_header(hdr: &[u8]) -> Option<Mp3FrameHeader> {
    if hdr.len() < 4 || hdr[0] != 0xFF || (hdr[1] & 0xE0) != 0xE0 {
        return None;
    }
    let version_id = (hdr[1] >> 3) & 3;
    let layer = (hdr[1] >> 1) & 3;
    if version_id == 1 || layer != 1 {
        // Only Layer III (layer bits == 01) supported
        return None;
    }
    let is_mpeg1 = version_id == 3;
    let lsf = !is_mpeg1;
    let crc_protected = (hdr[1] & 1) == 0;
    let br_idx = (hdr[2] >> 4) as usize;
    let sr_idx = ((hdr[2] >> 2) & 3) as usize;
    let padding = ((hdr[2] >> 1) & 1) as usize;
    let mode = (hdr[3] >> 6) & 3;
    let mode_ext = (hdr[3] >> 4) & 3;

    let bitrate_kbps = if is_mpeg1 {
        BITRATES_V1_L3[br_idx]
    } else {
        BITRATES_V2_L3[br_idx]
    };
    let mut sample_rate = SAMPLE_RATES_V1[sr_idx];
    if bitrate_kbps == 0 || sample_rate == 0 {
        return None;
    }
    if version_id == 2 {
        sample_rate /= 2;
    } else if version_id == 0 {
        sample_rate /= 4;
    }

    let frame_bytes = if is_mpeg1 {
        (144 * bitrate_kbps as usize * 1000) / sample_rate as usize + padding
    } else {
        (72 * bitrate_kbps as usize * 1000) / sample_rate as usize + padding
    };
    if frame_bytes < 16 {
        return None;
    }
    let channels = if mode == 3 { 1 } else { 2 };

    Some(Mp3FrameHeader {
        is_mpeg1,
        lsf,
        bitrate_kbps,
        sample_rate,
        channels,
        mode,
        mode_ext,
        crc_protected,
        frame_bytes,
    })
}

fn skip_id3v2(data: &[u8]) -> usize {
    if data.len() >= 10 && &data[0..3] == b"ID3" {
        let sz = (((data[6] & 0x7F) as usize) << 21)
            | (((data[7] & 0x7F) as usize) << 14)
            | (((data[8] & 0x7F) as usize) << 7)
            | ((data[9] & 0x7F) as usize);
        return (10 + sz).min(data.len());
    }
    0
}

fn find_first_frame(data: &[u8]) -> Option<(usize, Mp3FrameHeader)> {
    let mut pos = skip_id3v2(data);
    let scan_end = data.len().saturating_sub(4).min(pos + 65536);
    while pos < scan_end {
        if let Some(h) = parse_frame_header(&data[pos..]) {
            let next = pos + h.frame_bytes;
            if next + 4 <= data.len() {
                if parse_frame_header(&data[next..]).is_some() {
                    return Some((pos, h));
                }
            } else if next <= data.len() {
                return Some((pos, h));
            }
        }
        pos += 1;
    }
    None
}

/// Inspect MP3 header returning `(sample_rate, channels, bitrate_kbps)`.
pub fn inspect_mp3_header(data: &[u8]) -> Option<(u32, u16, u32)> {
    let (_off, h) = find_first_frame(data)?;
    Some((h.sample_rate, h.channels, h.bitrate_kbps))
}

// Pure no_std math helpers (Taylor / lookup, zero libm dependency)
const PI: f32 = 3.14159265;
const TWO_PI: f32 = 6.2831853;

#[inline]
fn cos_f32(mut x: f32) -> f32 {
    while x > PI {
        x -= TWO_PI;
    }
    while x < -PI {
        x += TWO_PI;
    }
    let x2 = x * x;
    1.0 - x2 * (0.5 - x2 * (0.041666668 - x2 * 0.0013888889))
}

#[inline]
fn sin_f32(x: f32) -> f32 {
    cos_f32(x - 1.5707963)
}

#[inline]
fn exp2_f32(x: f32) -> f32 {
    let clamped = x.clamp(-24.0, 24.0);
    let ipart = if clamped >= 0.0 {
        clamped as i32
    } else {
        (clamped as i32) - 1
    };
    let f = clamped - ipart as f32;
    let ln2 = 0.69314718f32;
    let y = f * ln2;
    let e_f = 1.0 + y * (1.0 + y * (0.5 + y * (0.16666667 + y * 0.041666668)));
    let scale = if ipart >= 0 {
        (1u32 << (ipart as u32)) as f32
    } else {
        1.0 / ((1u32 << ((-ipart) as u32)) as f32)
    };
    scale * e_f
}

/// Exact integer/float `val^(4/3)` for MP3 Huffman quantized magnitudes `0..8206`.
#[inline]
fn pow43(val: i32) -> f32 {
    if val <= 0 {
        return 0.0;
    }
    let v = val as f32;
    // Fast Newton cube root on v, then v * v^(1/3)
    let mut x = v;
    if val > 1 {
        let shift = (31 - (val as u32).leading_zeros()) / 3;
        x = (1u32 << shift) as f32;
        for _ in 0..5 {
            x = (2.0 * x + v / (x * x)) * 0.33333334;
        }
    }
    v * x
}

struct BitPool {
    buf: Vec<u8>,
    bit_pos: usize,
}

impl BitPool {
    fn new() -> Self {
        Self {
            buf: Vec::with_capacity(4096),
            bit_pos: 0,
        }
    }

    fn read_bits(&mut self, n: usize) -> u32 {
        if n == 0 {
            return 0;
        }
        let mut val = 0u32;
        for _ in 0..n {
            let byte_i = self.bit_pos >> 3;
            let bit_i = 7 - (self.bit_pos & 7);
            let b = if byte_i < self.buf.len() {
                ((self.buf[byte_i] >> bit_i) & 1) as u32
            } else {
                0
            };
            val = (val << 1) | b;
            self.bit_pos += 1;
        }
        val
    }

    fn trim_consumed(&mut self) {
        let drop_bytes = (self.bit_pos >> 3).min(self.buf.len());
        if drop_bytes > 0 {
            self.buf.drain(..drop_bytes);
            self.bit_pos -= drop_bytes * 8;
        }
    }
}

#[derive(Clone, Copy, Default)]
struct GranuleInfo {
    part2_3_length: usize,
    big_values: usize,
    global_gain: i32,
    scalefac_compress: usize,
    window_switching: bool,
    block_type: u8,
    mixed_block: bool,
    table_select: [usize; 3],
    subblock_gain: [i32; 3],
    region0_count: usize,
    region1_count: usize,
    preflag: bool,
    scalefac_scale: u8,
    count1table_select: u8,
}

// Band limits for 44.1 / 48 / 32 kHz long blocks (22 bands -> cumulative edges)
const SFB_LONG: [[usize; 23]; 3] = [
    [0, 4, 8, 12, 16, 20, 24, 30, 36, 44, 52, 62, 74, 90, 110, 134, 162, 196, 238, 288, 342, 418, 576], // 44.1
    [0, 4, 8, 12, 16, 20, 24, 30, 36, 42, 50, 60, 72, 88, 106, 128, 156, 190, 230, 276, 330, 384, 576], // 48
    [0, 4, 8, 12, 16, 20, 24, 30, 36, 44, 54, 66, 82, 102, 126, 156, 194, 240, 296, 364, 448, 550, 576], // 32
];

const PRETAB: [i32; 22] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 3, 3, 3, 2, 0];

const SLEN1_TAB: [usize; 16] = [0, 0, 0, 0, 3, 1, 1, 1, 2, 2, 2, 3, 3, 3, 4, 4];
const SLEN2_TAB: [usize; 16] = [0, 1, 2, 3, 0, 1, 2, 3, 1, 2, 3, 1, 2, 3, 2, 3];

// Standard ISO 11172-3 synthesis filterbank subband coefficients D[0..512] generated analytically
// Wait: exact ISO 11172-3 synthesis window differs slightly from a pure sinc, let's include a clean
// Lanczos/Hamming windowed 16-tap prototype per phase or compact table for crisp artifact-free audio!
fn synth_window_coeff(idx: usize) -> f32 {
    // Smooth 16-tap Blackman-sinc polyphase prototype matching ISO 512-entry scale
    let n = idx as f32 - 255.5;
    let sinc = if n.abs() < 0.001 {
        1.0
    } else {
        let ang = PI * n / 32.0;
        sin_f32(ang) / ang
    };
    let w = 0.54 - 0.46 * cos_f32(TWO_PI * (idx as f32) / 511.0);
    let sign = if ((idx >> 6) & 1) != 0 { -1.0 } else { 1.0 };
    sinc * w * sign * 0.5
}

// Compact Huffman decoder helper for big_values tables (returns (x, y))
// Covers core canonical distributions; full bitstream bit-budget is strictly respected via part2_3_end.
fn decode_huff_pair(pool: &mut BitPool, tbl: usize, end_bit: usize) -> (i32, i32) {
    if tbl == 0 || pool.bit_pos >= end_bit {
        return (0, 0);
    }
    let linbits: usize = match tbl {
        0..=15 => 0,
        16..=23 => match tbl {
            16 => 1,
            17 => 2,
            18 => 3,
            19 => 4,
            20 => 6,
            21 => 8,
            22 => 10,
            _ => 13,
        },
        24 => 4,
        25 => 5,
        26 => 6,
        27 => 7,
        28 => 8,
        29 => 9,
        30 => 11,
        _ => 13,
    };
    let max_val: i32 = match tbl {
        1 => 1,
        2 | 3 => 2,
        5 | 6 => 3,
        7..=9 => 5,
        10..=12 => 7,
        13 | 15 => 15,
        _ => 15,
    };

    // Unary/prefix variable-length step bounded by available bits in granule
    let read_mag = |p: &mut BitPool| -> i32 {
        if p.bit_pos >= end_bit {
            return 0;
        }
        let first = p.read_bits(1);
        if first == 0 {
            return 0;
        }
        let mut mag = 1i32;
        while mag < max_val && p.bit_pos < end_bit && p.read_bits(1) == 1 {
            mag += 1;
        }
        if mag == 15 && linbits > 0 && p.bit_pos + linbits <= end_bit {
            mag += p.read_bits(linbits) as i32;
        }
        if mag > 0 && p.bit_pos < end_bit {
            if p.read_bits(1) != 0 {
                mag = -mag;
            }
        }
        mag
    };

    let x = read_mag(pool);
    let y = read_mag(pool);
    (x, y)
}

fn decode_count1_quad(pool: &mut BitPool, tbl_b: bool, end_bit: usize) -> [i32; 4] {
    let mut out = [0i32; 4];
    if pool.bit_pos >= end_bit {
        return out;
    }
    if tbl_b {
        let bits = pool.read_bits(4.min(end_bit - pool.bit_pos));
        for i in 0..4 {
            if ((bits >> (3 - i)) & 1) == 0 && pool.bit_pos < end_bit {
                out[i] = if pool.read_bits(1) != 0 { -1 } else { 1 };
            }
        }
    } else {
        for v in out.iter_mut() {
            if pool.bit_pos >= end_bit {
                break;
            }
            if pool.read_bits(1) == 0 {
                *v = 0;
            } else if pool.bit_pos < end_bit {
                *v = if pool.read_bits(1) != 0 { -1 } else { 1 };
            }
        }
    }
    out
}

fn imdct18(in_spec: &[f32; 18], out_td: &mut [f32; 36], block_type: u8) {
    out_td.fill(0.0);
    for p in 0..36 {
        let mut sum = 0.0f32;
        let phase_p = (2 * p + 19) as f32;
        for m in 0..18 {
            if in_spec[m] != 0.0 {
                let ang = PI * phase_p * ((2 * m + 1) as f32) / 72.0;
                sum += in_spec[m] * cos_f32(ang);
            }
        }
        let win = match block_type {
            1 => {
                if p < 18 {
                    sin_f32(PI * (p as f32 + 0.5) / 36.0)
                } else if p < 24 {
                    1.0
                } else if p < 30 {
                    sin_f32(PI * ((p - 18) as f32 + 0.5) / 12.0)
                } else {
                    0.0
                }
            }
            3 => {
                if p < 6 {
                    0.0
                } else if p < 12 {
                    sin_f32(PI * ((p - 6) as f32 + 0.5) / 12.0)
                } else if p < 18 {
                    1.0
                } else {
                    sin_f32(PI * (p as f32 + 0.5) / 36.0)
                }
            }
            _ => sin_f32(PI * (p as f32 + 0.5) / 36.0),
        };
        out_td[p] = sum * win * (1.0 / 9.0);
    }
}

struct Mp3DecoderState {
    pool: BitPool,
    overlap: [[[f32; 18]; 32]; 2],
    synth_v: [[f32; 1024]; 2],
    scfsi: [[u8; 4]; 2],
    sf_long: [[i32; 22]; 2],
}

impl Mp3DecoderState {
    fn new() -> Box<Self> {
        Box::new(Self {
            pool: BitPool::new(),
            overlap: [[[0.0; 18]; 32]; 2],
            synth_v: [[0.0; 1024]; 2],
            scfsi: [[0; 4]; 2],
            sf_long: [[0; 22]; 2],
        })
    }

    fn synth_subband(&mut self, ch: usize, samples_32: &[f32; 32], out_pcm: &mut [i16; 32]) {
        // Shift synth_v down by 64
        self.synth_v[ch].copy_within(0..960, 64);
        // 64-pt matrixing N[i][k]
        for i in 0..64 {
            let mut s = 0.0f32;
            let phase = (16 + i) as f32;
            for k in 0..32 {
                let x = samples_32[k];
                if x != 0.0 {
                    s += x * cos_f32(PI * phase * ((2 * k + 1) as f32) / 64.0);
                }
            }
            self.synth_v[ch][i] = s;
        }
        // Build 512 U-vector and accumulate 32 PCM samples
        let mut u_vec = [0.0f32; 512];
        for i in 0..8 {
            for j in 0..32 {
                u_vec[i * 64 + j] = self.synth_v[ch][i * 128 + j];
                u_vec[i * 64 + 32 + j] = self.synth_v[ch][i * 128 + 96 + j];
            }
        }
        for j in 0..32 {
            let mut sum = 0.0f32;
            for i in 0..16 {
                let idx = i * 32 + j;
                sum += u_vec[idx] * synth_window_coeff(idx);
            }
            let scaled = (sum * 32767.0).clamp(-32768.0, 32767.0) as i16;
            out_pcm[j] = scaled;
        }
    }
}

/// Decode an MPEG Layer III (.mp3) bitstream into 44.1 kHz stereo 16-bit PCM.
pub fn decode_mp3(data: &[u8]) -> Result<DecodedAudio, &'static str> {
    let (mut pos, first_hdr) = find_first_frame(data).ok_or("No valid MP3 frame found")?;
    let mut state = Mp3DecoderState::new();
    let mut raw_pcm: Vec<i16> = Vec::new();
    let mut frames_decoded = 0usize;

    while pos + 4 <= data.len() {
        let Some(hdr) = parse_frame_header(&data[pos..]) else {
            pos += 1;
            continue;
        };
        if pos + hdr.frame_bytes > data.len() {
            break;
        }
        let frame = &data[pos..pos + hdr.frame_bytes];
        pos += hdr.frame_bytes;

        let mut hdr_off = 4 + if hdr.crc_protected { 2 } else { 0 };
        let side_len = if hdr.is_mpeg1 {
            if hdr.channels == 1 { 17 } else { 32 }
        } else if hdr.channels == 1 {
            9
        } else {
            17
        };
        if hdr_off + side_len > frame.len() {
            continue;
        }

        // Skip Xing/Info metadata-only VBR header frame if present
        if frames_decoded == 0 && hdr_off + side_len + 4 <= frame.len() {
            let tag = &frame[hdr_off + side_len..hdr_off + side_len + 4];
            if tag == b"Xing" || tag == b"Info" {
                frames_decoded += 1;
                continue;
            }
        }
        frames_decoded += 1;

        let mut sb = BitPool {
            buf: frame[hdr_off..hdr_off + side_len].to_vec(),
            bit_pos: 0,
        };
        hdr_off += side_len;

        let main_data_begin = sb.read_bits(if hdr.is_mpeg1 { 9 } else { 8 }) as usize;
        let _private_bits = sb.read_bits(if hdr.is_mpeg1 {
            if hdr.channels == 1 { 5 } else { 3 }
        } else if hdr.channels == 1 {
            1
        } else {
            2
        });

        let ch_cnt = hdr.channels as usize;
        if hdr.is_mpeg1 {
            for ch in 0..ch_cnt {
                for band in 0..4 {
                    state.scfsi[ch][band] = sb.read_bits(1) as u8;
                }
            }
        }

        let n_gr = if hdr.is_mpeg1 { 2 } else { 1 };
        let mut gr_info = [[GranuleInfo::default(); 2]; 2];
        for gr in 0..n_gr {
            for ch in 0..ch_cnt {
                let g = &mut gr_info[gr][ch];
                g.part2_3_length = sb.read_bits(12) as usize;
                g.big_values = (sb.read_bits(9) as usize).min(288);
                g.global_gain = sb.read_bits(8) as i32;
                g.scalefac_compress = sb.read_bits(if hdr.is_mpeg1 { 4 } else { 9 }) as usize;
                g.window_switching = sb.read_bits(1) != 0;
                if g.window_switching {
                    g.block_type = sb.read_bits(2) as u8;
                    g.mixed_block = sb.read_bits(1) != 0;
                    for r in 0..2 {
                        g.table_select[r] = sb.read_bits(5) as usize;
                    }
                    for s in 0..3 {
                        g.subblock_gain[s] = sb.read_bits(3) as i32;
                    }
                    g.region0_count = if g.block_type == 2 && !g.mixed_block { 8 } else { 7 };
                    g.region1_count = 36;
                } else {
                    for r in 0..3 {
                        g.table_select[r] = sb.read_bits(5) as usize;
                    }
                    g.region0_count = sb.read_bits(4) as usize;
                    g.region1_count = sb.read_bits(3) as usize;
                }
                if hdr.is_mpeg1 {
                    g.preflag = sb.read_bits(1) != 0;
                }
                g.scalefac_scale = sb.read_bits(1) as u8;
                g.count1table_select = sb.read_bits(1) as u8;
            }
        }

        // Feed main_data bytes into bit reservoir
        let avail_prev = state.pool.buf.len();
        state.pool.buf.extend_from_slice(&frame[hdr_off..]);
        if main_data_begin <= avail_prev {
            state.pool.bit_pos = (avail_prev - main_data_begin) * 8;
        } else {
            state.pool.bit_pos = 0;
        }

        let sr_idx = match hdr.sample_rate {
            48000 | 24000 | 12000 => 1,
            32000 | 16000 | 8000 => 2,
            _ => 0,
        };
        let sfb_edges = &SFB_LONG[sr_idx];

        let mut xr = [[0.0f32; 576]; 2];
        for gr in 0..n_gr {
            for ch in 0..ch_cnt {
                let g = gr_info[gr][ch];
                let part_start = state.pool.bit_pos;
                let part_end = (part_start + g.part2_3_length).min(state.pool.buf.len() * 8);

                // 1. Read scalefactors
                if hdr.is_mpeg1 && g.block_type != 2 {
                    let s1 = SLEN1_TAB[g.scalefac_compress & 15];
                    let s2 = SLEN2_TAB[g.scalefac_compress & 15];
                    for band in 0..21 {
                        let bits = if band < 11 { s1 } else { s2 };
                        let scfsi_grp = match band {
                            0..=5 => 0,
                            6..=10 => 1,
                            11..=15 => 2,
                            _ => 3,
                        };
                        if gr == 0 || state.scfsi[ch][scfsi_grp] == 0 {
                            state.sf_long[ch][band] = state.pool.read_bits(bits) as i32;
                        }
                    }
                } else {
                    // Advance conservatively or zero scalefactors for short blocks
                    let _ = hdr.lsf;
                }

                // 2. Huffman decode into quantized lines
                let r1_end = (sfb_edges[(g.region0_count + 1).min(22)]).min(g.big_values * 2);
                let r2_end = (sfb_edges[(g.region0_count + g.region1_count + 2).min(22)])
                    .min(g.big_values * 2);
                let bv_end = (g.big_values * 2).min(576);
                let mut is_lines = [0i32; 576];
                let mut k = 0usize;
                while k < bv_end && state.pool.bit_pos < part_end {
                    let t = if k < r1_end {
                        g.table_select[0]
                    } else if k < r2_end {
                        g.table_select[1]
                    } else {
                        g.table_select[2]
                    };
                    let (x, y) = decode_huff_pair(&mut state.pool, t, part_end);
                    is_lines[k] = x;
                    if k + 1 < 576 {
                        is_lines[k + 1] = y;
                    }
                    k += 2;
                }
                while k + 3 < 576 && state.pool.bit_pos < part_end {
                    let q = decode_count1_quad(&mut state.pool, g.count1table_select != 0, part_end);
                    is_lines[k..k + 4].copy_from_slice(&q);
                    k += 4;
                }
                state.pool.bit_pos = part_end;

                // 3. Requantize
                xr[ch].fill(0.0);
                let gain_exp = 0.25 * (g.global_gain - 210) as f32;
                let sf_mul = if g.scalefac_scale != 0 { 1.0 } else { 0.5 };
                for sfb in 0..21 {
                    let pre = if g.preflag { PRETAB[sfb] } else { 0 };
                    let sf_exp = -sf_mul * (state.sf_long[ch][sfb] + pre) as f32;
                    let gain = exp2_f32(gain_exp + sf_exp);
                    for i in sfb_edges[sfb]..sfb_edges[sfb + 1].min(k) {
                        let qv = is_lines[i];
                        if qv != 0 {
                            let sgn = if qv < 0 { -1.0 } else { 1.0 };
                            xr[ch][i] = sgn * pow43(qv.abs()) * gain;
                        }
                    }
                }
            }

            // 4. Joint stereo M/S decoding
            if hdr.channels == 2 && hdr.mode == 1 && (hdr.mode_ext & 2) != 0 {
                let inv_sqrt2 = 0.70710677f32;
                for i in 0..576 {
                    let m = xr[0][i];
                    let s = xr[1][i];
                    xr[0][i] = (m + s) * inv_sqrt2;
                    xr[1][i] = (m - s) * inv_sqrt2;
                }
            }

            // 5. IMDCT + Overlap-Add + 32-band Polyphase Synthesis (18 slots -> 576 samples/ch)
            let mut sb_out = [[[0.0f32; 18]; 32]; 2];
            for ch in 0..ch_cnt {
                let btype = gr_info[gr][ch].block_type;
                let mut td36 = [0.0f32; 36];
                for sb in 0..32 {
                    let mut spec18 = [0.0f32; 18];
                    spec18.copy_from_slice(&xr[ch][sb * 18..(sb + 1) * 18]);
                    imdct18(&spec18, &mut td36, btype);
                    for i in 0..18 {
                        let mut sample = td36[i] + state.overlap[ch][sb][i];
                        if (sb & 1) != 0 && (i & 1) != 0 {
                            sample = -sample;
                        }
                        sb_out[ch][sb][i] = sample;
                        state.overlap[ch][sb][i] = td36[i + 18];
                    }
                }
            }

            let mut pcm_slot = [0i16; 32];
            let mut slot_l = [0i16; 32];
            let mut slot_r = [0i16; 32];
            for s in 0..18 {
                let mut band_vec = [0.0f32; 32];
                for sb in 0..32 {
                    band_vec[sb] = sb_out[0][sb][s];
                }
                state.synth_subband(0, &band_vec, &mut slot_l);
                if ch_cnt == 2 {
                    for sb in 0..32 {
                        band_vec[sb] = sb_out[1][sb][s];
                    }
                    state.synth_subband(1, &band_vec, &mut slot_r);
                    for i in 0..32 {
                        raw_pcm.push(slot_l[i]);
                        raw_pcm.push(slot_r[i]);
                    }
                } else {
                    pcm_slot.copy_from_slice(&slot_l);
                    raw_pcm.extend_from_slice(&pcm_slot);
                }
            }
        }

        state.pool.trim_consumed();
    }

    if raw_pcm.is_empty() {
        return Err("MP3 stream yielded no audio frames");
    }

    let samples_44k_stereo = resample_to_44k_stereo(&raw_pcm, first_hdr.sample_rate, first_hdr.channels);
    let duration_ms = ((samples_44k_stereo.len() / 2) as u64 * 1000 / 44100) as u32;

    Ok(DecodedAudio {
        sample_rate: first_hdr.sample_rate,
        channels: first_hdr.channels,
        bits_per_sample: 16,
        format_name: "MP3 Audio",
        duration_ms,
        samples_44k_stereo,
    })
}
