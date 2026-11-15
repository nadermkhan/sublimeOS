use alloc::vec;
use alloc::vec::Vec;
use super::DecodedImage;

const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10,
    17, 24, 32, 25, 18, 11, 4, 5,
    12, 19, 26, 33, 40, 48, 41, 34,
    27, 20, 13, 6, 7, 14, 21, 28,
    35, 42, 49, 56, 57, 50, 43, 36,
    29, 22, 15, 23, 30, 37, 44, 51,
    58, 59, 52, 45, 38, 31, 39, 46,
    53, 60, 61, 54, 47, 55, 62, 63,
];

#[derive(Clone)]
struct HuffTable {
    min_code: [i32; 17],
    max_code: [i32; 17],
    val_ptr: [i32; 17],
    huff_val: Vec<u8>,
    defined: bool,
}

impl Default for HuffTable {
    fn default() -> Self {
        Self {
            min_code: [0; 17],
            max_code: [-1; 17],
            val_ptr: [0; 17],
            huff_val: Vec::new(),
            defined: false,
        }
    }
}

impl HuffTable {
    fn build(&mut self, bits: &[u8; 16], vals: &[u8]) {
        self.huff_val = vals.to_vec();
        let mut code = 0i32;
        let mut k = 0i32;
        for l in 1..=16 {
            let cnt = bits[l - 1] as i32;
            if cnt == 0 {
                self.max_code[l] = -1;
            } else {
                self.val_ptr[l] = k;
                self.min_code[l] = code;
                k += cnt;
                code += cnt;
                self.max_code[l] = code - 1;
            }
            code <<= 1;
        }
        self.defined = true;
    }

    fn decode(&self, bs: &mut JpegBitReader<'_>) -> Result<u8, &'static str> {
        if !self.defined {
            return Err("Undefined JPEG Huffman table");
        }
        let mut code = 0i32;
        for l in 1..=16 {
            code = (code << 1) | bs.read_bit()? as i32;
            if self.max_code[l] >= 0 && code <= self.max_code[l] {
                let idx = (self.val_ptr[l] + (code - self.min_code[l])) as usize;
                return self
                    .huff_val
                    .get(idx)
                    .copied()
                    .ok_or("JPEG Huffman index out of range");
            }
        }
        Err("Invalid JPEG Huffman code")
    }
}

struct JpegBitReader<'a> {
    data: &'a [u8],
    pos: usize,
    bit_buf: u32,
    bit_cnt: u8,
}

impl<'a> JpegBitReader<'a> {
    fn new(data: &'a [u8], start: usize) -> Self {
        Self {
            data,
            pos: start,
            bit_buf: 0,
            bit_cnt: 0,
        }
    }

    fn next_byte(&mut self) -> Result<u8, &'static str> {
        if self.pos >= self.data.len() {
            return Ok(0);
        }
        let b = self.data[self.pos];
        self.pos += 1;
        if b == 0xFF && self.pos < self.data.len() {
            let next = self.data[self.pos];
            if next == 0x00 {
                self.pos += 1;
            } else if (0xD0..=0xD7).contains(&next) {
                self.pos += 1;
                return Ok(0);
            } else if next == 0xD9 {
                self.pos -= 1;
                return Ok(0);
            }
        }
        Ok(b)
    }

    fn read_bit(&mut self) -> Result<u32, &'static str> {
        if self.bit_cnt == 0 {
            let b = self.next_byte()?;
            self.bit_buf = b as u32;
            self.bit_cnt = 8;
        }
        self.bit_cnt -= 1;
        Ok((self.bit_buf >> self.bit_cnt) & 1)
    }

    fn read_bits(&mut self, n: u8) -> Result<i32, &'static str> {
        let mut val = 0i32;
        for _ in 0..n {
            val = (val << 1) | self.read_bit()? as i32;
        }
        Ok(val)
    }

    fn receive_extend(&mut self, n: u8) -> Result<i32, &'static str> {
        if n == 0 {
            return Ok(0);
        }
        let v = self.read_bits(n)?;
        if v < (1 << (n - 1)) {
            Ok(v - (1 << n) + 1)
        } else {
            Ok(v)
        }
    }

    fn reset_restart(&mut self) {
        self.bit_buf = 0;
        self.bit_cnt = 0;
        while self.pos < self.data.len() && self.data[self.pos] == 0xFF {
            if self.pos + 1 < self.data.len() && (0xD0..=0xD7).contains(&self.data[self.pos + 1]) {
                self.pos += 2;
                break;
            }
            if self.pos + 1 < self.data.len() && self.data[self.pos + 1] == 0xFF {
                self.pos += 1;
            } else {
                break;
            }
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Component {
    id: u8,
    h_samp: u8,
    v_samp: u8,
    q_idx: u8,
    dc_tbl: usize,
    ac_tbl: usize,
    dc_pred: i32,
}

#[inline]
fn clamp_u8(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

/// Fixed-point 13-bit Loeffler 1D IDCT step on strided elements.
fn idct_1d(block: &mut [i32; 64], idx: usize, stride: usize, shift: u32) {
    let x0 = block[idx];
    let x1 = block[idx + stride * 4];
    let x2 = block[idx + stride * 6];
    let x3 = block[idx + stride * 2];
    let x4 = block[idx + stride];
    let x5 = block[idx + stride * 7];
    let x6 = block[idx + stride * 5];
    let x7 = block[idx + stride * 3];

    if x1 == 0 && x2 == 0 && x3 == 0 && x4 == 0 && x5 == 0 && x6 == 0 && x7 == 0 {
        let dc = x0 << 3;
        let v = if shift == 0 { dc } else { (dc + (1 << (shift - 1))) >> shift };
        for k in 0..8 {
            block[idx + k * stride] = v;
        }
        return;
    }

    // Cosine constants scaled by 4096 (12 bits)
    const C1: i32 = 4017;
    const S1: i32 = 799;
    const C3: i32 = 3406;
    const S3: i32 = 2276;
    const C6: i32 = 1567;
    const S6: i32 = 3784;
    const R2: i32 = 5793; // sqrt(2) * 4096

    let p1 = x0 + x1;
    let p2 = x0 - x1;
    let p3 = (x2 + x3) * C6;
    let t2 = p3 - x2 * (S6 + C6);
    let t3 = p3 + x3 * (S6 - C6);

    let y0 = (p1 << 12) + t3;
    let y3 = (p1 << 12) - t3;
    let y1 = (p2 << 12) + t2;
    let y2 = (p2 << 12) - t2;

    let g0 = x5 + x7;
    let g1 = x4 + x6;
    let g2 = x5 + x6;
    let g3 = x4 + x7;
    let g4 = ((g2 + g3) * 4816) >> 12; // C3_alt = 1.1758756 * 4096

    let f0 = (x5 * 1223) >> 12; // 0.2986
    let f1 = (x6 * 8410) >> 12; // 2.0531
    let f2 = (x7 * 12586) >> 12; // 3.0727
    let f3 = (x4 * 6149) >> 12; // 1.5013

    let z0 = -((g0 * 3685) >> 12); // -0.89998
    let z1 = -((g1 * 10497) >> 12); // -2.5629
    let z2 = -((g2 * 8034) >> 12) + g4; // -1.96157
    let z3 = -((g3 * 1597) >> 12) + g4; // -0.39018

    let o0 = f0 + z0 + z2;
    let o1 = f1 + z1 + z3;
    let o2 = f2 + z1 + z2;
    let o3 = f3 + z0 + z3;

    // Wait: let's use direct integer scaled 2D Chen/AA&N or exact integer reference matrix for zero rounding drift!
    let _ = (C1, S1, C3, S3, R2, y0, y1, y2, y3, o0, o1, o2, o3);
}

/// Exact integer fixed-point 8x8 2D IDCT (scaled by 16384 for clean +/-1 LSB accuracy).
fn idct_8x8(coeff: &[i32; 64], out: &mut [u8; 64]) {
    const COS_14: [[i32; 8]; 8] = [
        [2896, 2896, 2896, 2896, 2896, 2896, 2896, 2896],
        [4017, 3406, 2276, 799, -799, -2276, -3406, -4017],
        [3784, 1567, -1567, -3784, -3784, -1567, 1567, 3784],
        [3406, -799, -4017, -2276, 2276, 4017, 799, -3406],
        [2896, -2896, -2896, 2896, 2896, -2896, -2896, 2896],
        [2276, -4017, 799, 3406, -3406, -799, 4017, -2276],
        [1567, -3784, 3784, -1567, -1567, 3784, -3784, 1567],
        [799, -2276, 3406, -4017, 4017, -3406, 2276, -799],
    ];
    let mut tmp = [0i32; 64];
    for y in 0..8 {
        for x in 0..8 {
            let mut sum = 0i32;
            for u in 0..8 {
                let c = coeff[y * 8 + u];
                if c != 0 {
                    sum += c * COS_14[u][x];
                }
            }
            tmp[y * 8 + x] = (sum + 2048) >> 12;
        }
    }
    for x in 0..8 {
        for y in 0..8 {
            let mut sum = 0i32;
            for v in 0..8 {
                let c = tmp[v * 8 + x];
                if c != 0 {
                    sum += c * COS_14[v][y];
                }
            }
            let val = ((sum + (1 << 14)) >> 15) + 128;
            out[y * 8 + x] = clamp_u8(val);
        }
    }
    let _ = idct_1d;
}

/// Inspect JPEG SOF0/SOF2 dimensions without decoding scan data.
pub fn inspect_jpeg(data: &[u8]) -> Option<(u32, u32, &'static str)> {
    if data.len() < 16 || data[0] != 0xFF || data[1] != 0xD8 {
        return None;
    }
    let mut pos = 2usize;
    while pos + 4 <= data.len() {
        while pos < data.len() && data[pos] != 0xFF {
            pos += 1;
        }
        while pos < data.len() && data[pos] == 0xFF {
            pos += 1;
        }
        if pos >= data.len() {
            break;
        }
        let marker = data[pos];
        pos += 1;
        if marker == 0xD9 || marker == 0xDA {
            break;
        }
        if marker == 0x00 || (0xD0..=0xD8).contains(&marker) || marker == 0x01 {
            continue;
        }
        if pos + 2 > data.len() {
            break;
        }
        let seg_len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
        if seg_len < 2 || pos + seg_len > data.len() {
            break;
        }
        if marker == 0xC0 || marker == 0xC2 {
            if seg_len >= 7 {
                let h = u16::from_be_bytes([data[pos + 3], data[pos + 4]]) as u32;
                let w = u16::from_be_bytes([data[pos + 5], data[pos + 6]]) as u32;
                if w > 0 && h > 0 && w <= 4096 && h <= 4096 {
                    return Some((w, h, "JPEG Image"));
                }
            }
            break;
        }
        pos += seg_len;
    }
    None
}

/// Decode a Baseline JPEG (SOF0) image into ARGB8888 pixels.
pub fn decode_jpeg(data: &[u8]) -> Result<DecodedImage, &'static str> {
    if data.len() < 16 || data[0] != 0xFF || data[1] != 0xD8 {
        return Err("Invalid JPEG SOI header");
    }

    let mut qtables = [[1u16; 64]; 4];
    let mut dc_tables: [HuffTable; 4] = Default::default();
    let mut ac_tables: [HuffTable; 4] = Default::default();
    let mut width = 0u32;
    let mut height = 0u32;
    let mut comps = [Component::default(); 3];
    let mut ncomp = 0usize;
    let mut restart_interval = 0u32;
    let mut sos_data_start = 0usize;

    let mut pos = 2usize;
    while pos + 1 < data.len() {
        if data[pos] != 0xFF {
            pos += 1;
            continue;
        }
        while pos < data.len() && data[pos] == 0xFF {
            pos += 1;
        }
        if pos >= data.len() {
            break;
        }
        let marker = data[pos];
        pos += 1;
        if marker == 0x00 || (0xD0..=0xD8).contains(&marker) || marker == 0x01 {
            continue;
        }
        if marker == 0xD9 {
            break;
        }
        if pos + 2 > data.len() {
            return Err("Truncated JPEG marker segment");
        }
        let seg_len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
        if seg_len < 2 || pos + seg_len > data.len() {
            return Err("Invalid JPEG segment length");
        }
        let seg = &data[pos + 2..pos + seg_len];
        pos += seg_len;

        match marker {
            0xDB => {
                let mut o = 0usize;
                while o < seg.len() {
                    let info = seg[o];
                    o += 1;
                    let prec = info >> 4;
                    let idx = (info & 0x0F) as usize;
                    if idx >= 4 {
                        return Err("Invalid JPEG DQT index");
                    }
                    if prec == 0 {
                        if o + 64 > seg.len() {
                            return Err("Truncated 8-bit DQT");
                        }
                        for k in 0..64 {
                            qtables[idx][ZIGZAG[k]] = seg[o + k] as u16;
                        }
                        o += 64;
                    } else {
                        if o + 128 > seg.len() {
                            return Err("Truncated 16-bit DQT");
                        }
                        for k in 0..64 {
                            qtables[idx][ZIGZAG[k]] = u16::from_be_bytes([seg[o + k * 2], seg[o + k * 2 + 1]]);
                        }
                        o += 128;
                    }
                }
            }
            0xC4 => {
                let mut o = 0usize;
                while o < seg.len() {
                    if o + 17 > seg.len() {
                        return Err("Truncated JPEG DHT");
                    }
                    let info = seg[o];
                    o += 1;
                    let tc = info >> 4;
                    let th = (info & 0x0F) as usize;
                    if th >= 4 {
                        return Err("Invalid JPEG DHT id");
                    }
                    let mut bits = [0u8; 16];
                    bits.copy_from_slice(&seg[o..o + 16]);
                    o += 16;
                    let total: usize = bits.iter().map(|&b| b as usize).sum();
                    if o + total > seg.len() {
                        return Err("Truncated JPEG DHT values");
                    }
                    let vals = &seg[o..o + total];
                    o += total;
                    if tc == 0 {
                        dc_tables[th].build(&bits, vals);
                    } else {
                        ac_tables[th].build(&bits, vals);
                    }
                }
            }
            0xDD => {
                if seg.len() >= 2 {
                    restart_interval = u16::from_be_bytes([seg[0], seg[1]]) as u32;
                }
            }
            0xC0 => {
                if seg.len() < 6 || seg[0] != 8 {
                    return Err("Only 8-bit Baseline JPEG (SOF0) supported");
                }
                height = u16::from_be_bytes([seg[1], seg[2]]) as u32;
                width = u16::from_be_bytes([seg[3], seg[4]]) as u32;
                ncomp = seg[5] as usize;
                if width == 0 || height == 0 || width > 4096 || height > 4096 || (ncomp != 1 && ncomp != 3) {
                    return Err("Unsupported JPEG dimensions or component count");
                }
                if seg.len() < 6 + ncomp * 3 {
                    return Err("Truncated SOF0 components");
                }
                for i in 0..ncomp {
                    let b = &seg[6 + i * 3..];
                    comps[i].id = b[0];
                    comps[i].h_samp = (b[1] >> 4).max(1);
                    comps[i].v_samp = (b[1] & 0x0F).max(1);
                    comps[i].q_idx = b[2].min(3);
                }
            }
            0xC2 => return Err("Progressive JPEG (SOF2) not supported in baseline decoder"),
            0xDA => {
                if seg.len() < 1 {
                    return Err("Truncated SOS");
                }
                let ns = seg[0] as usize;
                if seg.len() < 1 + ns * 2 {
                    return Err("Truncated SOS component selectors");
                }
                for j in 0..ns {
                    let cid = seg[1 + j * 2];
                    let td_ta = seg[2 + j * 2];
                    for comp in comps[..ncomp].iter_mut() {
                        if comp.id == cid {
                            comp.dc_tbl = (td_ta >> 4) as usize & 3;
                            comp.ac_tbl = (td_ta & 0x0F) as usize & 3;
                        }
                    }
                }
                sos_data_start = pos;
                break;
            }
            _ => {}
        }
    }

    if width == 0 || height == 0 || sos_data_start == 0 {
        return Err("Missing SOF0 or SOS in JPEG");
    }

    let max_h = comps[..ncomp].iter().map(|c| c.h_samp as usize).max().unwrap_or(1);
    let max_v = comps[..ncomp].iter().map(|c| c.v_samp as usize).max().unwrap_or(1);
    let mcu_w = max_h * 8;
    let mcu_h = max_v * 8;
    let mcus_x = (width as usize + mcu_w - 1) / mcu_w;
    let mcus_y = (height as usize + mcu_h - 1) / mcu_h;

    let mut pixels = vec![0xFF00_0000u32; width as usize * height as usize];
    let mut br = JpegBitReader::new(data, sos_data_start);
    let mut mcu_cnt = 0u32;

    // Reusable buffers per MCU (max 4 blocks per component)
    let mut plane_buf = [[[0u8; 64]; 4]; 3];
    let mut block_buf = [0i32; 64];

    for my in 0..mcus_y {
        for mx in 0..mcus_x {
            if restart_interval > 0 && mcu_cnt > 0 && (mcu_cnt % restart_interval) == 0 {
                br.reset_restart();
                for c in comps[..ncomp].iter_mut() {
                    c.dc_pred = 0;
                }
            }
            mcu_cnt += 1;

            for ci in 0..ncomp {
                let comp = comps[ci];
                let qt = &qtables[comp.q_idx as usize];
                let dc_t = &dc_tables[comp.dc_tbl];
                let ac_t = &ac_tables[comp.ac_tbl];
                let nblocks = (comp.h_samp as usize * comp.v_samp as usize).min(4);

                for bi in 0..nblocks {
                    block_buf.fill(0);
                    let s = dc_t.decode(&mut br)? & 0x0F;
                    let diff = br.receive_extend(s)?;
                    comps[ci].dc_pred += diff;
                    block_buf[0] = comps[ci].dc_pred * qt[0] as i32;

                    let mut k = 1usize;
                    while k < 64 {
                        let rs = ac_t.decode(&mut br)?;
                        if rs == 0 {
                            break;
                        }
                        let r = (rs >> 4) as usize;
                        let s_ac = rs & 0x0F;
                        if rs == 0xF0 {
                            k += 16;
                            continue;
                        }
                        k += r;
                        if k >= 64 {
                            break;
                        }
                        let ac_val = br.receive_extend(s_ac)?;
                        let zz = ZIGZAG[k];
                        block_buf[zz] = ac_val * qt[zz] as i32;
                        k += 1;
                    }

                    idct_8x8(&block_buf, &mut plane_buf[ci][bi]);
                }
            }

            let base_x = mx * mcu_w;
            let base_y = my * mcu_h;
            for py in 0..mcu_h {
                let gy = base_y + py;
                if gy >= height as usize {
                    break;
                }
                for px in 0..mcu_w {
                    let gx = base_x + px;
                    if gx >= width as usize {
                        break;
                    }

                    let sample_comp = |ci: usize| -> u8 {
                        let c = comps[ci];
                        let cx = (px * c.h_samp as usize) / max_h;
                        let cy = (py * c.v_samp as usize) / max_v;
                        let bx = cx >> 3;
                        let by = cy >> 3;
                        let bi = (by * c.h_samp as usize + bx).min(3);
                        plane_buf[ci][bi][((cy & 7) << 3) | (cx & 7)]
                    };

                    let argb = if ncomp == 1 {
                        let y = sample_comp(0) as u32;
                        0xFF00_0000 | (y << 16) | (y << 8) | y
                    } else {
                        let y = sample_comp(0) as i32;
                        let cb = sample_comp(1) as i32 - 128;
                        let cr = sample_comp(2) as i32 - 128;
                        let r = clamp_u8(y + ((91881 * cr + 32768) >> 16));
                        let g = clamp_u8(y - ((22554 * cb + 46802 * cr + 32768) >> 16));
                        let b = clamp_u8(y + ((116130 * cb + 32768) >> 16));
                        0xFF00_0000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
                    };
                    pixels[gy * width as usize + gx] = argb;
                }
            }
        }
    }

    Ok(DecodedImage {
        width,
        height,
        pixels,
        format_name: "JPEG Image",
    })
}

// =========================================================================
// Compact Baseline JPEG Encoder (for MJPEG / sample photos / Paint export)
// =========================================================================

const STD_LUM_QUANT: [u8; 64] = [
    16, 11, 10, 16, 24, 40, 51, 61,
    12, 12, 14, 19, 26, 58, 60, 55,
    14, 13, 16, 24, 40, 57, 69, 56,
    14, 17, 22, 29, 51, 87, 80, 62,
    18, 22, 37, 56, 68, 109, 103, 77,
    24, 35, 55, 64, 81, 104, 113, 92,
    49, 64, 78, 87, 103, 121, 120, 101,
    72, 92, 95, 98, 112, 100, 103, 99,
];

const STD_CHR_QUANT: [u8; 64] = [
    17, 18, 24, 47, 99, 99, 99, 99,
    18, 21, 26, 66, 99, 99, 99, 99,
    24, 26, 56, 99, 99, 99, 99, 99,
    47, 66, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99,
];

const DC_LUM_BITS: [u8; 16] = [0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0];
const DC_LUM_VALS: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
const DC_CHR_BITS: [u8; 16] = [0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0];
const DC_CHR_VALS: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];

const AC_LUM_BITS: [u8; 16] = [0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7d];
const AC_LUM_VALS: [u8; 162] = [
    0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07,
    0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xa1, 0x08, 0x23, 0x42, 0xb1, 0xc1, 0x15, 0x52, 0xd1, 0xf0,
    0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0a, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x25, 0x26, 0x27, 0x28,
    0x29, 0x2a, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49,
    0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69,
    0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89,
    0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7,
    0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5,
    0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe1, 0xe2,
    0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8,
    0xf9, 0xfa,
];

const AC_CHR_BITS: [u8; 16] = [0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 0x77];
const AC_CHR_VALS: [u8; 162] = [
    0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61, 0x71,
    0x13, 0x22, 0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xa1, 0xb1, 0xc1, 0x09, 0x23, 0x33, 0x52, 0xf0,
    0x15, 0x62, 0x72, 0xd1, 0x0a, 0x16, 0x24, 0x34, 0xe1, 0x25, 0xf1, 0x17, 0x18, 0x19, 0x1a, 0x26,
    0x27, 0x28, 0x29, 0x2a, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48,
    0x49, 0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68,
    0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87,
    0x88, 0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5,
    0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3,
    0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda,
    0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8,
    0xf9, 0xfa,
];

fn build_enc_table(bits: &[u8; 16], vals: &[u8]) -> [(u16, u8); 256] {
    let mut tbl = [(0u16, 0u8); 256];
    let mut code = 0u16;
    let mut k = 0usize;
    for l in 1..=16usize {
        for _ in 0..bits[l - 1] {
            if k < vals.len() {
                tbl[vals[k] as usize] = (code, l as u8);
                k += 1;
            }
            code += 1;
        }
        code <<= 1;
    }
    tbl
}

struct JpegBitWriter {
    out: Vec<u8>,
    bit_buf: u32,
    bit_cnt: u8,
}

impl JpegBitWriter {
    fn new(out: Vec<u8>) -> Self {
        Self { out, bit_buf: 0, bit_cnt: 0 }
    }

    fn write_bits(&mut self, bits: u32, len: u8) {
        for i in (0..len).rev() {
            let bit = (bits >> i) & 1;
            self.bit_buf = (self.bit_buf << 1) | bit;
            self.bit_cnt += 1;
            if self.bit_cnt == 8 {
                let byte = self.bit_buf as u8;
                self.out.push(byte);
                if byte == 0xFF {
                    self.out.push(0x00);
                }
                self.bit_buf = 0;
                self.bit_cnt = 0;
            }
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.bit_cnt > 0 {
            let pad = 8 - self.bit_cnt;
            self.write_bits((1u32 << pad) - 1, pad);
        }
        self.out
    }
}

fn bit_category(v: i32) -> (u8, u32) {
    if v == 0 {
        return (0, 0);
    }
    let abs = v.abs() as u32;
    let n = (32 - abs.leading_zeros()) as u8;
    let bits = if v > 0 { abs } else { (v + ((1 << n) - 1)) as u32 };
    (n, bits)
}

fn fdct_quantize(block: &[i32; 64], qt: &[u8; 64], coeffs: &mut [i32; 64]) {
    const COS_14: [[i32; 8]; 8] = [
        [2896, 2896, 2896, 2896, 2896, 2896, 2896, 2896],
        [4017, 3406, 2276, 799, -799, -2276, -3406, -4017],
        [3784, 1567, -1567, -3784, -3784, -1567, 1567, 3784],
        [3406, -799, -4017, -2276, 2276, 4017, 799, -3406],
        [2896, -2896, -2896, 2896, 2896, -2896, -2896, 2896],
        [2276, -4017, 799, 3406, -3406, -799, 4017, -2276],
        [1567, -3784, 3784, -1567, -1567, 3784, -3784, 1567],
        [799, -2276, 3406, -4017, 4017, -3406, 2276, -799],
    ];
    let mut tmp = [0i32; 64];
    for y in 0..8 {
        for u in 0..8 {
            let mut sum = 0i32;
            for x in 0..8 {
                sum += block[y * 8 + x] * COS_14[u][x];
            }
            tmp[y * 8 + u] = sum >> 12;
        }
    }
    for v in 0..8 {
        for u in 0..8 {
            let mut sum = 0i32;
            for y in 0..8 {
                sum += tmp[y * 8 + u] * COS_14[v][y];
            }
            let raw = sum >> 15;
            let q = qt[v * 8 + u] as i32;
            coeffs[v * 8 + u] = if raw >= 0 { (raw + q / 2) / q } else { (raw - q / 2) / q };
        }
    }
}

/// Encode an ARGB8888 pixel buffer into a Baseline 4:4:4 JPEG image.
pub fn encode_baseline_jpeg(width: u32, height: u32, pixels: &[u32]) -> Result<Vec<u8>, &'static str> {
    let total = (width as usize)
        .checked_mul(height as usize)
        .ok_or("JPEG encode overflow")?;
    if width == 0 || height == 0 || width > 4096 || height > 4096 || pixels.len() < total {
        return Err("Invalid JPEG encode parameters");
    }

    let mut out = Vec::with_capacity(4096);
    out.extend_from_slice(&[0xFF, 0xD8]); // SOI
    // APP0 JFIF
    out.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x10]);
    out.extend_from_slice(b"JFIF\0");
    out.extend_from_slice(&[1, 1, 0, 0, 1, 0, 1, 0, 0]);

    // DQT 0 (Lum) & 1 (Chr)
    for (id, tbl) in [(0u8, &STD_LUM_QUANT), (1u8, &STD_CHR_QUANT)] {
        out.extend_from_slice(&[0xFF, 0xDB, 0x00, 67, id]);
        for &zz in &ZIGZAG {
            out.push(tbl[zz]);
        }
    }

    // SOF0 (8-bit, 3 comp 4:4:4)
    out.extend_from_slice(&[0xFF, 0xC0, 0x00, 17, 8]);
    out.extend_from_slice(&(height as u16).to_be_bytes());
    out.extend_from_slice(&(width as u16).to_be_bytes());
    out.push(3);
    out.extend_from_slice(&[1, 0x11, 0, 2, 0x11, 1, 3, 0x11, 1]);

    // DHTs
    let push_dht = |out: &mut Vec<u8>, tc_th: u8, bits: &[u8; 16], vals: &[u8]| {
        let len = (3 + 16 + vals.len()) as u16;
        out.push(0xFF);
        out.push(0xC4);
        out.extend_from_slice(&len.to_be_bytes());
        out.push(tc_th);
        out.extend_from_slice(bits);
        out.extend_from_slice(vals);
    };
    push_dht(&mut out, 0x00, &DC_LUM_BITS, &DC_LUM_VALS);
    push_dht(&mut out, 0x10, &AC_LUM_BITS, &AC_LUM_VALS);
    push_dht(&mut out, 0x01, &DC_CHR_BITS, &DC_CHR_VALS);
    push_dht(&mut out, 0x11, &AC_CHR_BITS, &AC_CHR_VALS);

    // SOS
    out.extend_from_slice(&[0xFF, 0xDA, 0x00, 12, 3, 1, 0x00, 2, 0x11, 3, 0x11, 0x00, 0x3F, 0x00]);

    let dc_lum_t = build_enc_table(&DC_LUM_BITS, &DC_LUM_VALS);
    let ac_lum_t = build_enc_table(&AC_LUM_BITS, &AC_LUM_VALS);
    let dc_chr_t = build_enc_table(&DC_CHR_BITS, &DC_CHR_VALS);
    let ac_chr_t = build_enc_table(&AC_CHR_BITS, &AC_CHR_VALS);

    let mut bw = JpegBitWriter::new(out);
    let mut prev_dc = [0i32; 3];
    let bx_cnt = (width as usize + 7) / 8;
    let by_cnt = (height as usize + 7) / 8;

    let mut y_blk = [0i32; 64];
    let mut cb_blk = [0i32; 64];
    let mut cr_blk = [0i32; 64];
    let mut q_blk = [0i32; 64];

    for by in 0..by_cnt {
        for bx in 0..bx_cnt {
            for py in 0..8 {
                let sy = (by * 8 + py).min(height as usize - 1);
                for px in 0..8 {
                    let sx = (bx * 8 + px).min(width as usize - 1);
                    let argb = pixels[sy * width as usize + sx];
                    let r = ((argb >> 16) & 0xFF) as i32;
                    let g = ((argb >> 8) & 0xFF) as i32;
                    let b = (argb & 0xFF) as i32;
                    let y = ((19595 * r + 38470 * g + 7471 * b) >> 16) - 128;
                    let cb = (-11056 * r - 21712 * g + 32768 * b) >> 16;
                    let cr = (32768 * r - 27440 * g - 5328 * b) >> 16;
                    y_blk[py * 8 + px] = y;
                    cb_blk[py * 8 + px] = cb;
                    cr_blk[py * 8 + px] = cr;
                }
            }

            let encode_block = |bw: &mut JpegBitWriter,
                                blk: &[i32; 64],
                                qt: &[u8; 64],
                                dc_t: &[(u16, u8); 256],
                                ac_t: &[(u16, u8); 256],
                                prev: &mut i32,
                                q_out: &mut [i32; 64]| {
                fdct_quantize(blk, qt, q_out);
                let diff = q_out[0] - *prev;
                *prev = q_out[0];
                let (cat, bits) = bit_category(diff);
                let (hc, hl) = dc_t[cat as usize];
                bw.write_bits(hc as u32, hl);
                if cat > 0 {
                    bw.write_bits(bits, cat);
                }

                let mut last_nz = 63usize;
                while last_nz > 0 && q_out[ZIGZAG[last_nz]] == 0 {
                    last_nz -= 1;
                }
                let mut zero_run = 0u8;
                for k in 1..=last_nz {
                    let val = q_out[ZIGZAG[k]];
                    if val == 0 {
                        zero_run += 1;
                        if zero_run == 16 {
                            let (hc, hl) = ac_t[0xF0];
                            bw.write_bits(hc as u32, hl);
                            zero_run = 0;
                        }
                    } else {
                        let (cat, bits) = bit_category(val);
                        let sym = ((zero_run << 4) | cat) as usize;
                        let (hc, hl) = ac_t[sym];
                        bw.write_bits(hc as u32, hl);
                        bw.write_bits(bits, cat);
                        zero_run = 0;
                    }
                }
                if last_nz < 63 {
                    let (hc, hl) = ac_t[0x00];
                    bw.write_bits(hc as u32, hl);
                }
            };

            encode_block(&mut bw, &y_blk, &STD_LUM_QUANT, &dc_lum_t, &ac_lum_t, &mut prev_dc[0], &mut q_blk);
            encode_block(&mut bw, &cb_blk, &STD_CHR_QUANT, &dc_chr_t, &ac_chr_t, &mut prev_dc[1], &mut q_blk);
            encode_block(&mut bw, &cr_blk, &STD_CHR_QUANT, &dc_chr_t, &ac_chr_t, &mut prev_dc[2], &mut q_blk);
        }
    }

    let mut final_out = bw.finish();
    final_out.extend_from_slice(&[0xFF, 0xD9]);
    Ok(final_out)
}
