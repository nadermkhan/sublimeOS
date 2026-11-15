use alloc::vec;
use alloc::vec::Vec;
use super::deflate::{crc32, zlib_compress_stored, zlib_decompress};
use super::DecodedImage;

const PNG_SIG: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

#[inline]
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let a = a as i32;
    let b = b as i32;
    let c = c as i32;
    let p = a + b - c;
    let pa = (p - a).abs();
    let pb = (p - b).abs();
    let pc = (p - c).abs();
    if pa <= pb && pa <= pc {
        a as u8
    } else if pb <= pc {
        b as u8
    } else {
        c as u8
    }
}

/// Inspect PNG IHDR dimensions without decompressing IDAT.
pub fn inspect_png(data: &[u8]) -> Option<(u32, u32, &'static str)> {
    if data.len() < 33 || data[0..8] != PNG_SIG || &data[12..16] != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
    let h = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
    if w == 0 || h == 0 || w > 4096 || h > 4096 {
        return None;
    }
    Some((w, h, "PNG Image"))
}

fn unfilter_scanlines(
    raw: &[u8],
    width: usize,
    height: usize,
    bpp_bits: usize,
) -> Result<Vec<u8>, &'static str> {
    let row_bytes = (width * bpp_bits + 7) / 8;
    let filter_bpp = ((bpp_bits + 7) / 8).max(1);
    let stride = 1 + row_bytes;
    if raw.len() < stride * height {
        return Err("Truncated PNG scanlines");
    }
    let mut out = vec![0u8; row_bytes * height];
    for y in 0..height {
        let src_row = &raw[y * stride..y * stride + stride];
        let filter = src_row[0];
        let payload = &src_row[1..];
        let row_off = y * row_bytes;
        for x in 0..row_bytes {
            let cur = payload[x];
            let a = if x >= filter_bpp { out[row_off + x - filter_bpp] } else { 0 };
            let b = if y > 0 { out[(y - 1) * row_bytes + x] } else { 0 };
            let c = if y > 0 && x >= filter_bpp {
                out[(y - 1) * row_bytes + x - filter_bpp]
            } else {
                0
            };
            let val = match filter {
                0 => cur,
                1 => cur.wrapping_add(a),
                2 => cur.wrapping_add(b),
                3 => cur.wrapping_add(((a as u16 + b as u16) >> 1) as u8),
                4 => cur.wrapping_add(paeth(a, b, c)),
                _ => return Err("Unsupported PNG filter type"),
            };
            out[row_off + x] = val;
        }
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn rasterize_pass(
    scanlines: &[u8],
    pass_w: usize,
    pass_h: usize,
    bit_depth: u8,
    color_type: u8,
    palette: &[u8],
    trns: &[u8],
    x0: usize,
    y0: usize,
    dx: usize,
    dy: usize,
    out_w: usize,
    pixels: &mut [u32],
) -> Result<(), &'static str> {
    let samples_per_px: usize = match color_type {
        0 => 1,
        2 => 3,
        3 => 1,
        4 => 2,
        6 => 4,
        _ => return Err("Unsupported PNG color type"),
    };
    let bpp_bits = samples_per_px * bit_depth as usize;
    let row_bytes = (pass_w * bpp_bits + 7) / 8;

    let trns_gray = if color_type == 0 && trns.len() >= 2 {
        Some(u16::from_be_bytes([trns[0], trns[1]]))
    } else {
        None
    };
    let trns_rgb = if color_type == 2 && trns.len() >= 6 {
        Some((
            u16::from_be_bytes([trns[0], trns[1]]),
            u16::from_be_bytes([trns[2], trns[3]]),
            u16::from_be_bytes([trns[4], trns[5]]),
        ))
    } else {
        None
    };

    for py in 0..pass_h {
        let row = &scanlines[py * row_bytes..(py + 1) * row_bytes];
        let dst_y = y0 + py * dy;
        for px in 0..pass_w {
            let dst_x = x0 + px * dx;
            let argb = match (color_type, bit_depth) {
                (6, 8) => {
                    let o = px * 4;
                    ((row[o + 3] as u32) << 24)
                        | ((row[o] as u32) << 16)
                        | ((row[o + 1] as u32) << 8)
                        | (row[o + 2] as u32)
                }
                (2, 8) => {
                    let o = px * 3;
                    let (r, g, b) = (row[o], row[o + 1], row[o + 2]);
                    let a = match trns_rgb {
                        Some((tr, tg, tb)) if (r as u16, g as u16, b as u16) == (tr, tg, tb) => 0,
                        _ => 255,
                    };
                    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
                }
                (0, 8) => {
                    let v = row[px];
                    let a = match trns_gray {
                        Some(tv) if v as u16 == tv => 0,
                        _ => 255,
                    };
                    ((a as u32) << 24) | ((v as u32) << 16) | ((v as u32) << 8) | (v as u32)
                }
                (4, 8) => {
                    let o = px * 2;
                    let v = row[o];
                    let a = row[o + 1];
                    ((a as u32) << 24) | ((v as u32) << 16) | ((v as u32) << 8) | (v as u32)
                }
                (3, bd @ (1 | 2 | 4 | 8)) => {
                    let mask = (1u8 << bd) - 1;
                    let bit_off = px * bd as usize;
                    let byte = row[bit_off >> 3];
                    let shift = 8 - bd as usize - (bit_off & 7);
                    let idx = ((byte >> shift) & mask) as usize;
                    let po = idx * 3;
                    let (r, g, b) = if po + 2 < palette.len() {
                        (palette[po], palette[po + 1], palette[po + 2])
                    } else {
                        (0, 0, 0)
                    };
                    let a = trns.get(idx).copied().unwrap_or(255);
                    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
                }
                (0, bd @ (1 | 2 | 4)) => {
                    let mask = (1u8 << bd) - 1;
                    let bit_off = px * bd as usize;
                    let byte = row[bit_off >> 3];
                    let shift = 8 - bd as usize - (bit_off & 7);
                    let raw_v = ((byte >> shift) & mask) as u16;
                    let max_v = mask as u16;
                    let v = ((raw_v * 255) / max_v) as u8;
                    let a = match trns_gray {
                        Some(tv) if raw_v == tv => 0,
                        _ => 255,
                    };
                    ((a as u32) << 24) | ((v as u32) << 16) | ((v as u32) << 8) | (v as u32)
                }
                (6, 16) => {
                    let o = px * 8;
                    ((row[o + 6] as u32) << 24)
                        | ((row[o] as u32) << 16)
                        | ((row[o + 2] as u32) << 8)
                        | (row[o + 4] as u32)
                }
                (2, 16) => {
                    let o = px * 6;
                    0xFF00_0000
                        | ((row[o] as u32) << 16)
                        | ((row[o + 2] as u32) << 8)
                        | (row[o + 4] as u32)
                }
                _ => return Err("Unsupported PNG bit depth/color combination"),
            };
            pixels[dst_y * out_w + dst_x] = argb;
        }
    }
    Ok(())
}

/// Decode a standard PNG image into ARGB8888 pixels.
pub fn decode_png(data: &[u8]) -> Result<DecodedImage, &'static str> {
    if data.len() < 33 || data[0..8] != PNG_SIG {
        return Err("Invalid PNG signature");
    }

    let mut pos = 8usize;
    let mut width = 0u32;
    let mut height = 0u32;
    let mut bit_depth = 0u8;
    let mut color_type = 0u8;
    let mut interlace = 0u8;
    let mut got_ihdr = false;
    let mut palette: Vec<u8> = Vec::new();
    let mut trns: Vec<u8> = Vec::new();
    let mut idat: Vec<u8> = Vec::new();

    while pos + 12 <= data.len() {
        let len = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let ctype = &data[pos + 4..pos + 8];
        let payload_start = pos + 8;
        let payload_end = payload_start.checked_add(len).ok_or("PNG chunk length overflow")?;
        if payload_end + 4 > data.len() {
            return Err("Truncated PNG chunk");
        }
        let payload = &data[payload_start..payload_end];

        match ctype {
            b"IHDR" => {
                if payload.len() < 13 {
                    return Err("Malformed IHDR");
                }
                width = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
                height = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);
                bit_depth = payload[8];
                color_type = payload[9];
                interlace = payload[12];
                if width == 0 || height == 0 || width > 4096 || height > 4096 {
                    return Err("PNG dimensions out of supported range");
                }
                got_ihdr = true;
            }
            b"PLTE" => {
                palette.extend_from_slice(payload);
            }
            b"tRNS" => {
                trns.extend_from_slice(payload);
            }
            b"IDAT" => {
                idat.extend_from_slice(payload);
            }
            b"IEND" => break,
            _ => {}
        }

        pos = payload_end + 4;
    }

    if !got_ihdr || idat.is_empty() {
        return Err("Missing IHDR or IDAT chunk");
    }

    let raw = zlib_decompress(&idat)?;
    let samples_per_px: usize = match color_type {
        0 => 1,
        2 => 3,
        3 => 1,
        4 => 2,
        6 => 4,
        _ => return Err("Unsupported PNG color type"),
    };
    let bpp_bits = samples_per_px * bit_depth as usize;
    let w_us = width as usize;
    let h_us = height as usize;
    let mut pixels = vec![0u32; w_us * h_us];

    if interlace == 0 {
        let unfiltered = unfilter_scanlines(&raw, w_us, h_us, bpp_bits)?;
        rasterize_pass(
            &unfiltered,
            w_us,
            h_us,
            bit_depth,
            color_type,
            &palette,
            &trns,
            0,
            0,
            1,
            1,
            w_us,
            &mut pixels,
        )?;
    } else if interlace == 1 {
        const PASSES: [(usize, usize, usize, usize); 7] = [
            (0, 0, 8, 8),
            (4, 0, 8, 8),
            (0, 4, 4, 8),
            (2, 0, 4, 4),
            (0, 2, 2, 4),
            (1, 0, 2, 2),
            (0, 1, 1, 2),
        ];
        let mut off = 0usize;
        for &(x0, y0, dx, dy) in &PASSES {
            let pw = if w_us > x0 { (w_us - x0 + dx - 1) / dx } else { 0 };
            let ph = if h_us > y0 { (h_us - y0 + dy - 1) / dy } else { 0 };
            if pw == 0 || ph == 0 {
                continue;
            }
            let row_bytes = (pw * bpp_bits + 7) / 8;
            let pass_bytes = (1 + row_bytes) * ph;
            if off + pass_bytes > raw.len() {
                return Err("Truncated Adam7 PNG pass");
            }
            let unfiltered = unfilter_scanlines(&raw[off..off + pass_bytes], pw, ph, bpp_bits)?;
            rasterize_pass(
                &unfiltered,
                pw,
                ph,
                bit_depth,
                color_type,
                &palette,
                &trns,
                x0,
                y0,
                dx,
                dy,
                w_us,
                &mut pixels,
            )?;
            off += pass_bytes;
        }
    } else {
        return Err("Unsupported PNG interlace method");
    }

    Ok(DecodedImage {
        width,
        height,
        pixels,
        format_name: "PNG Image",
    })
}

fn push_png_chunk(out: &mut Vec<u8>, tag: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let crc_start = out.len();
    out.extend_from_slice(tag);
    out.extend_from_slice(data);
    let c = crc32(&out[crc_start..]);
    out.extend_from_slice(&c.to_be_bytes());
}

/// Encode an ARGB8888 pixel buffer into a standard 32-bit RGBA PNG file.
pub fn encode_png_argb(width: u32, height: u32, pixels: &[u32]) -> Result<Vec<u8>, &'static str> {
    let total = (width as usize)
        .checked_mul(height as usize)
        .ok_or("PNG encode overflow")?;
    if width == 0 || height == 0 || pixels.len() < total {
        return Err("Invalid PNG encode buffer");
    }

    let stride = 1 + width as usize * 4;
    let mut raw = vec![0u8; stride * height as usize];
    for y in 0..height as usize {
        let dst = y * stride;
        raw[dst] = 0; // Filter 0 (None)
        for x in 0..width as usize {
            let px = pixels[y * width as usize + x];
            let o = dst + 1 + x * 4;
            raw[o] = ((px >> 16) & 0xFF) as u8;
            raw[o + 1] = ((px >> 8) & 0xFF) as u8;
            raw[o + 2] = (px & 0xFF) as u8;
            raw[o + 3] = ((px >> 24) & 0xFF) as u8;
        }
    }

    let z = zlib_compress_stored(&raw);
    let mut out = Vec::with_capacity(64 + z.len());
    out.extend_from_slice(&PNG_SIG);

    let mut ihdr = [0u8; 13];
    ihdr[0..4].copy_from_slice(&width.to_be_bytes());
    ihdr[4..8].copy_from_slice(&height.to_be_bytes());
    ihdr[8] = 8; // bit depth
    ihdr[9] = 6; // RGBA
    push_png_chunk(&mut out, b"IHDR", &ihdr);
    push_png_chunk(&mut out, b"IDAT", &z);
    push_png_chunk(&mut out, b"IEND", &[]);
    Ok(out)
}
