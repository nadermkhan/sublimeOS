use alloc::vec;
use alloc::vec::Vec;
use super::DecodedImage;

const QOI_END_MARKER: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 1];

#[inline]
fn hash_rgba(r: u8, g: u8, b: u8, a: u8) -> usize {
    ((r as usize) * 3 + (g as usize) * 5 + (b as usize) * 7 + (a as usize) * 11) & 63
}

/// Inspect QOI header without decoding pixel data.
pub fn inspect_qoi(data: &[u8]) -> Option<(u32, u32, &'static str)> {
    if data.len() < 14 || &data[0..4] != b"qoif" {
        return None;
    }
    let w = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let h = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    if w == 0 || h == 0 || w > 4096 || h > 4096 {
        return None;
    }
    Some((w, h, "QOI Image"))
}

/// Decode a QOI ("Quite OK Image") RGB/RGBA bitstream into ARGB8888 pixels.
pub fn decode_qoi(data: &[u8]) -> Result<DecodedImage, &'static str> {
    if data.len() < 22 || &data[0..4] != b"qoif" {
        return Err("Invalid QOI header");
    }
    let width = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let height = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    let channels = data[12];
    if width == 0 || height == 0 || width > 4096 || height > 4096 || (channels != 3 && channels != 4) {
        return Err("Invalid QOI dimensions or channels");
    }
    let total = (width as usize)
        .checked_mul(height as usize)
        .filter(|&n| n <= 4096 * 4096)
        .ok_or("QOI image dimensions overflow")?;

    let payload_end = if data.len() >= 22 && data[data.len() - 8..] == QOI_END_MARKER {
        data.len() - 8
    } else {
        data.len()
    };

    let mut pixels = vec![0u32; total];
    let mut index = [(0u8, 0u8, 0u8, 0u8); 64];
    let (mut r, mut g, mut b, mut a) = (0u8, 0u8, 0u8, 255u8);
    let mut run = 0usize;
    let mut pos = 14usize;

    for out_px in pixels.iter_mut() {
        if run > 0 {
            run -= 1;
        } else if pos < payload_end {
            let b1 = data[pos];
            pos += 1;
            if b1 == 0xFE {
                if pos + 3 > payload_end {
                    return Err("Truncated QOI RGB op");
                }
                r = data[pos];
                g = data[pos + 1];
                b = data[pos + 2];
                pos += 3;
            } else if b1 == 0xFF {
                if pos + 4 > payload_end {
                    return Err("Truncated QOI RGBA op");
                }
                r = data[pos];
                g = data[pos + 1];
                b = data[pos + 2];
                a = data[pos + 3];
                pos += 4;
            } else {
                match b1 >> 6 {
                    0 => {
                        let px = index[(b1 & 0x3F) as usize];
                        r = px.0;
                        g = px.1;
                        b = px.2;
                        a = px.3;
                    }
                    1 => {
                        let dr = ((b1 >> 4) & 0x03).wrapping_sub(2);
                        let dg = ((b1 >> 2) & 0x03).wrapping_sub(2);
                        let db = (b1 & 0x03).wrapping_sub(2);
                        r = r.wrapping_add(dr);
                        g = g.wrapping_add(dg);
                        b = b.wrapping_add(db);
                    }
                    2 => {
                        if pos >= payload_end {
                            return Err("Truncated QOI LUMA op");
                        }
                        let b2 = data[pos];
                        pos += 1;
                        let vg = (b1 & 0x3F).wrapping_sub(32);
                        let dr_dg = ((b2 >> 4) & 0x0F).wrapping_sub(8);
                        let db_dg = (b2 & 0x0F).wrapping_sub(8);
                        r = r.wrapping_add(vg.wrapping_add(dr_dg));
                        g = g.wrapping_add(vg);
                        b = b.wrapping_add(vg.wrapping_add(db_dg));
                    }
                    _ => {
                        run = (b1 & 0x3F) as usize;
                    }
                }
            }
            index[hash_rgba(r, g, b, a)] = (r, g, b, a);
        }

        *out_px = ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
    }

    Ok(DecodedImage {
        width,
        height,
        pixels,
        format_name: "QOI Image",
    })
}

/// Encode ARGB8888 pixel buffer into a spec-compliant QOI RGBA file.
pub fn encode_qoi_argb(width: u32, height: u32, pixels: &[u32]) -> Result<Vec<u8>, &'static str> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .ok_or("QOI encode dimension overflow")?;
    if width == 0 || height == 0 || pixels.len() < expected {
        return Err("Invalid QOI encode buffer");
    }

    let mut out = Vec::with_capacity(14 + expected * 2 + 8);
    out.extend_from_slice(b"qoif");
    out.extend_from_slice(&width.to_be_bytes());
    out.extend_from_slice(&height.to_be_bytes());
    out.push(4); // RGBA
    out.push(0); // sRGB with linear alpha

    let mut index = [(0u8, 0u8, 0u8, 0u8); 64];
    let (mut prev_r, mut prev_g, mut prev_b, mut prev_a) = (0u8, 0u8, 0u8, 255u8);
    let mut run = 0u8;

    for (i, &px) in pixels[..expected].iter().enumerate() {
        let a = ((px >> 24) & 0xFF) as u8;
        let r = ((px >> 16) & 0xFF) as u8;
        let g = ((px >> 8) & 0xFF) as u8;
        let b = (px & 0xFF) as u8;

        if (r, g, b, a) == (prev_r, prev_g, prev_b, prev_a) {
            run += 1;
            if run == 62 || i + 1 == expected {
                out.push(0xC0 | (run - 1));
                run = 0;
            }
            continue;
        }

        if run > 0 {
            out.push(0xC0 | (run - 1));
            run = 0;
        }

        let h = hash_rgba(r, g, b, a);
        if index[h] == (r, g, b, a) {
            out.push(h as u8);
        } else {
            index[h] = (r, g, b, a);
            if a == prev_a {
                let vr = r.wrapping_sub(prev_r) as i8;
                let vg = g.wrapping_sub(prev_g) as i8;
                let vb = b.wrapping_sub(prev_b) as i8;
                let vg_r = vr.wrapping_sub(vg);
                let vg_b = vb.wrapping_sub(vg);
                if (-2..=1).contains(&vr) && (-2..=1).contains(&vg) && (-2..=1).contains(&vb) {
                    out.push(0x40 | (((vr + 2) as u8) << 4) | (((vg + 2) as u8) << 2) | ((vb + 2) as u8));
                } else if (-8..=7).contains(&vg_r) && (-32..=31).contains(&vg) && (-8..=7).contains(&vg_b) {
                    out.push(0x80 | ((vg + 32) as u8));
                    out.push((((vg_r + 8) as u8) << 4) | ((vg_b + 8) as u8));
                } else {
                    out.push(0xFE);
                    out.extend_from_slice(&[r, g, b]);
                }
            } else {
                out.push(0xFF);
                out.extend_from_slice(&[r, g, b, a]);
            }
        }
        prev_r = r;
        prev_g = g;
        prev_b = b;
        prev_a = a;
    }

    out.extend_from_slice(&QOI_END_MARKER);
    Ok(out)
}
