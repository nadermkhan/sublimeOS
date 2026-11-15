use alloc::vec;
use super::DecodedImage;

struct PpmTokens<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> PpmTokens<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn skip_ws_and_comments(&mut self) {
        while self.pos < self.data.len() {
            let b = self.data[self.pos];
            if b == b'#' {
                while self.pos < self.data.len() && self.data[self.pos] != b'\n' {
                    self.pos += 1;
                }
            } else if b.is_ascii_whitespace() {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn next_u32(&mut self) -> Option<u32> {
        self.skip_ws_and_comments();
        let start = self.pos;
        let mut val = 0u32;
        while self.pos < self.data.len() && self.data[self.pos].is_ascii_digit() {
            val = val.saturating_mul(10).saturating_add((self.data[self.pos] - b'0') as u32);
            self.pos += 1;
        }
        if self.pos == start {
            None
        } else {
            Some(val)
        }
    }
}

pub fn inspect_raster(data: &[u8]) -> Option<(u32, u32, &'static str)> {
    if data.len() >= 26 && &data[0..2] == b"BM" {
        let dib_size = u32::from_le_bytes([data[14], data[15], data[16], data[17]]);
        if dib_size >= 40 && data.len() >= 34 {
            let w = i32::from_le_bytes([data[18], data[19], data[20], data[21]]);
            let h = i32::from_le_bytes([data[22], data[23], data[24], data[25]]).abs();
            if w > 0 && h > 0 && w <= 4096 && h <= 4096 {
                return Some((w as u32, h as u32, "BMP Image"));
            }
        }
    }
    if data.len() >= 7 && (data.starts_with(b"P6") || data.starts_with(b"P3")) {
        let mut tokens = PpmTokens::new(&data[2..]);
        if let (Some(w), Some(h), Some(maxv)) =
            (tokens.next_u32(), tokens.next_u32(), tokens.next_u32())
        {
            if w > 0 && h > 0 && maxv > 0 && w <= 4096 && h <= 4096 {
                return Some((w, h, "PPM Image"));
            }
        }
    }
    if data.len() >= 18 {
        let img_type = data[2];
        let bpp = data[16];
        let w = u16::from_le_bytes([data[12], data[13]]) as u32;
        let h = u16::from_le_bytes([data[14], data[15]]) as u32;
        if (img_type == 2 || img_type == 10)
            && (bpp == 24 || bpp == 32)
            && w > 0
            && h > 0
            && w <= 4096
            && h <= 4096
        {
            return Some((w, h, "TGA Image"));
        }
    }
    None
}

pub fn decode_bmp(data: &[u8]) -> Result<DecodedImage, &'static str> {
    if data.len() < 54 || &data[0..2] != b"BM" {
        return Err("Invalid BMP header");
    }
    let pixel_offset = u32::from_le_bytes([data[10], data[11], data[12], data[13]]) as usize;
    let dib_size = u32::from_le_bytes([data[14], data[15], data[16], data[17]]);
    if dib_size < 40 {
        return Err("Unsupported BMP DIB header");
    }
    let width_i = i32::from_le_bytes([data[18], data[19], data[20], data[21]]);
    let height_i = i32::from_le_bytes([data[22], data[23], data[24], data[25]]);
    let planes = u16::from_le_bytes([data[26], data[27]]);
    let bpp = u16::from_le_bytes([data[28], data[29]]);
    let compression = u32::from_le_bytes([data[30], data[31], data[32], data[33]]);

    if planes != 1 || width_i <= 0 || height_i == 0 {
        return Err("Invalid BMP dimensions");
    }
    let width = width_i as u32;
    let height = height_i.unsigned_abs();
    if width > 4096 || height > 4096 || (width as usize * height as usize) > 4096 * 2160 {
        return Err("BMP dimensions too large");
    }
    if !(bpp == 24 || bpp == 32) || (compression != 0 && !(bpp == 32 && compression == 3)) {
        return Err("Only uncompressed 24-bit or 32-bit BMP is supported");
    }

    let top_down = height_i < 0;
    let bytes_per_pixel = (bpp / 8) as usize;
    let row_stride = ((width as usize * bytes_per_pixel) + 3) & !3;
    let needed = pixel_offset.saturating_add(row_stride.saturating_mul(height as usize));
    if data.len() < needed {
        return Err("Truncated BMP pixel data");
    }

    let mut pixels = vec![0u32; width as usize * height as usize];
    for row in 0..height as usize {
        let dst_row = if top_down { row } else { height as usize - 1 - row };
        let src_base = pixel_offset + row * row_stride;
        let dst_base = dst_row * width as usize;
        for col in 0..width as usize {
            let p = src_base + col * bytes_per_pixel;
            let b = data[p] as u32;
            let g = data[p + 1] as u32;
            let r = data[p + 2] as u32;
            pixels[dst_base + col] = 0xFF00_0000 | (r << 16) | (g << 8) | b;
        }
    }

    Ok(DecodedImage {
        width,
        height,
        pixels,
        format_name: "BMP",
    })
}

pub fn decode_tga(data: &[u8]) -> Result<DecodedImage, &'static str> {
    if data.len() < 18 {
        return Err("Invalid TGA header");
    }
    let id_len = data[0] as usize;
    let color_map = data[1];
    let img_type = data[2];
    let width = u16::from_le_bytes([data[12], data[13]]) as u32;
    let height = u16::from_le_bytes([data[14], data[15]]) as u32;
    let bpp = data[16];
    let desc = data[17];

    if color_map != 0 || (img_type != 2 && img_type != 10) || (bpp != 24 && bpp != 32) {
        return Err("Unsupported TGA variant");
    }
    if width == 0 || height == 0 || width > 4096 || height > 4096 || (width as usize * height as usize) > 4096 * 2160 {
        return Err("Invalid TGA dimensions");
    }
    let step = (bpp / 8) as usize;
    let total = width as usize * height as usize;
    let mut pos = 18 + id_len;
    if pos >= data.len() {
        return Err("Truncated TGA");
    }

    let mut raw_pixels = vec![0u32; total];
    if img_type == 2 {
        if pos + total * step > data.len() {
            return Err("Truncated uncompressed TGA");
        }
        for i in 0..total {
            let p = pos + i * step;
            let b = data[p] as u32;
            let g = data[p + 1] as u32;
            let r = data[p + 2] as u32;
            raw_pixels[i] = 0xFF00_0000 | (r << 16) | (g << 8) | b;
        }
    } else {
        let mut out_idx = 0usize;
        while out_idx < total {
            if pos >= data.len() {
                return Err("Truncated RLE TGA");
            }
            let hdr = data[pos];
            pos += 1;
            let count = ((hdr & 0x7F) as usize) + 1;
            if (hdr & 0x80) != 0 {
                if pos + step > data.len() {
                    return Err("Truncated RLE packet");
                }
                let col = 0xFF00_0000
                    | ((data[pos + 2] as u32) << 16)
                    | ((data[pos + 1] as u32) << 8)
                    | (data[pos] as u32);
                pos += step;
                for _ in 0..count {
                    if out_idx < total {
                        raw_pixels[out_idx] = col;
                        out_idx += 1;
                    }
                }
            } else {
                for _ in 0..count {
                    if pos + step > data.len() {
                        return Err("Truncated raw TGA packet");
                    }
                    let col = 0xFF00_0000
                        | ((data[pos + 2] as u32) << 16)
                        | ((data[pos + 1] as u32) << 8)
                        | (data[pos] as u32);
                    pos += step;
                    if out_idx < total {
                        raw_pixels[out_idx] = col;
                        out_idx += 1;
                    }
                }
            }
        }
    }

    let top_origin = (desc & 0x20) != 0;
    if top_origin {
        return Ok(DecodedImage {
            width,
            height,
            pixels: raw_pixels,
            format_name: "TGA",
        });
    }
    let mut pixels = vec![0u32; total];
    for row in 0..height as usize {
        let src_row = height as usize - 1 - row;
        pixels[row * width as usize..(row + 1) * width as usize]
            .copy_from_slice(&raw_pixels[src_row * width as usize..(src_row + 1) * width as usize]);
    }
    Ok(DecodedImage {
        width,
        height,
        pixels,
        format_name: "TGA",
    })
}

pub fn decode_ppm(data: &[u8]) -> Result<DecodedImage, &'static str> {
    let is_binary = data.starts_with(b"P6");
    if !is_binary && !data.starts_with(b"P3") {
        return Err("Not a PPM image");
    }
    let mut tok = PpmTokens::new(&data[2..]);
    let w = tok.next_u32().ok_or("Invalid PPM width")?;
    let h = tok.next_u32().ok_or("Invalid PPM height")?;
    let maxval = tok.next_u32().ok_or("Invalid PPM maxval")?;
    if w == 0 || h == 0 || w > 4096 || h > 4096 || maxval == 0 || maxval > 255 {
        return Err("Unsupported PPM parameters");
    }
    let total = w as usize * h as usize;
    let mut pixels = vec![0u32; total];
    if is_binary {
        let start = 2 + tok.pos + 1;
        if start + total * 3 > data.len() {
            return Err("Truncated P6 PPM");
        }
        for i in 0..total {
            let p = start + i * 3;
            let r = (data[p] as u32 * 255) / maxval;
            let g = (data[p + 1] as u32 * 255) / maxval;
            let b = (data[p + 2] as u32 * 255) / maxval;
            pixels[i] = 0xFF00_0000 | (r << 16) | (g << 8) | b;
        }
    } else {
        for px in pixels.iter_mut() {
            let r = (tok.next_u32().ok_or("Truncated P3")? * 255) / maxval;
            let g = (tok.next_u32().ok_or("Truncated P3")? * 255) / maxval;
            let b = (tok.next_u32().ok_or("Truncated P3")? * 255) / maxval;
            *px = 0xFF00_0000 | (r << 16) | (g << 8) | b;
        }
    }
    Ok(DecodedImage {
        width: w,
        height: h,
        pixels,
        format_name: "PPM",
    })
}
