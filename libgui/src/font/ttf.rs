use alloc::vec;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug)]
pub struct FontMetrics {
    pub units_per_em: u16,
    pub ascent: i16,
    pub descent: i16,
    pub line_gap: i16,
    pub x_min: i16,
    pub y_min: i16,
    pub x_max: i16,
    pub y_max: i16,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GlyphMetrics {
    pub advance_width: u16,
    pub lsb: i16,
}

#[derive(Clone, Copy, Debug)]
pub struct OutlinePoint {
    pub x: i32,
    pub y: i32,
    pub on_curve: bool,
}

#[derive(Clone, Debug)]
pub struct Outline {
    pub contours: Vec<Vec<OutlinePoint>>,
    pub x_min: i16,
    pub y_min: i16,
    pub x_max: i16,
    pub y_max: i16,
}

#[derive(Clone, Copy, Debug)]
enum CmapKind {
    Format4 { offset: usize, seg_count_x2: usize },
    Format12 { offset: usize, num_groups: usize },
    Format0 { offset: usize },
}

/// Zero-copy `#![no_std]` SFNT/TrueType parser supporting `cmap` 0/4/12, `loca`, `glyf`, `hmtx`.
#[derive(Clone, Copy)]
pub struct TtfFont<'a> {
    data: &'a [u8],
    metrics: FontMetrics,
    index_to_loc_format: i16,
    num_glyphs: u16,
    num_hmetrics: u16,
    hmtx_offset: usize,
    loca_offset: usize,
    loca_len: usize,
    glyf_offset: usize,
    glyf_len: usize,
    cmap: CmapKind,
}

#[inline]
fn u16_be(data: &[u8], off: usize) -> Option<u16> {
    let b = data.get(off..off.checked_add(2)?)?;
    Some(u16::from_be_bytes([b[0], b[1]]))
}

#[inline]
fn i16_be(data: &[u8], off: usize) -> Option<i16> {
    u16_be(data, off).map(|v| v as i16)
}

#[inline]
fn u32_be(data: &[u8], off: usize) -> Option<u32> {
    let b = data.get(off..off.checked_add(4)?)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

impl<'a> TtfFont<'a> {
    pub fn from_bytes(data: &'a [u8]) -> Option<Self> {
        if data.len() < 12 {
            return None;
        }
        let sfnt = u32_be(data, 0)?;
        // Accept TrueType 1.0 (0x00010000) or 'true' (0x74727565) or 'typ1'
        if sfnt != 0x0001_0000 && sfnt != 0x7472_7565 {
            return None;
        }
        let num_tables = u16_be(data, 4)? as usize;
        if data.len() < 12 + num_tables * 16 {
            return None;
        }

        let mut head_off = None;
        let mut maxp_off = None;
        let mut hhea_off = None;
        let mut hmtx_off = None;
        let mut loca_rec = None;
        let mut glyf_rec = None;
        let mut cmap_off = None;

        for i in 0..num_tables {
            let rec = 12 + i * 16;
            let tag = data.get(rec..rec + 4)?;
            let off = u32_be(data, rec + 8)? as usize;
            let len = u32_be(data, rec + 12)? as usize;
            if off.checked_add(len)? > data.len() {
                continue;
            }
            match tag {
                b"head" => head_off = Some(off),
                b"maxp" => maxp_off = Some(off),
                b"hhea" => hhea_off = Some(off),
                b"hmtx" => hmtx_off = Some(off),
                b"loca" => loca_rec = Some((off, len)),
                b"glyf" => glyf_rec = Some((off, len)),
                b"cmap" => cmap_off = Some(off),
                _ => {}
            }
        }

        let head = head_off?;
        let maxp = maxp_off?;
        let hhea = hhea_off?;
        let hmtx_offset = hmtx_off?;
        let (loca_offset, loca_len) = loca_rec?;
        let (glyf_offset, glyf_len) = glyf_rec?;
        let cmap_offset = cmap_off?;

        let units_per_em = u16_be(data, head + 18)?;
        if units_per_em == 0 {
            return None;
        }
        let x_min = i16_be(data, head + 36)?;
        let y_min = i16_be(data, head + 38)?;
        let x_max = i16_be(data, head + 40)?;
        let y_max = i16_be(data, head + 42)?;
        let index_to_loc_format = i16_be(data, head + 50)?;

        let num_glyphs = u16_be(data, maxp + 4)?;
        let ascent = i16_be(data, hhea + 4)?;
        let descent = i16_be(data, hhea + 6)?;
        let line_gap = i16_be(data, hhea + 8)?;
        let num_hmetrics = u16_be(data, hhea + 34)?;

        let cmap = Self::select_cmap(data, cmap_offset)?;

        Some(Self {
            data,
            metrics: FontMetrics {
                units_per_em,
                ascent,
                descent,
                line_gap,
                x_min,
                y_min,
                x_max,
                y_max,
            },
            index_to_loc_format,
            num_glyphs,
            num_hmetrics,
            hmtx_offset,
            loca_offset,
            loca_len,
            glyf_offset,
            glyf_len,
            cmap,
        })
    }

    fn select_cmap(data: &[u8], cmap_offset: usize) -> Option<CmapKind> {
        let num_subtables = u16_be(data, cmap_offset + 2)? as usize;
        let mut best: Option<(u8, CmapKind)> = None;

        for i in 0..num_subtables {
            let rec = cmap_offset + 4 + i * 8;
            let platform = u16_be(data, rec)?;
            let encoding = u16_be(data, rec + 2)?;
            let sub_rel = u32_be(data, rec + 4)? as usize;
            let sub_off = cmap_offset.checked_add(sub_rel)?;
            let format = u16_be(data, sub_off)?;

            let priority = match (platform, encoding, format) {
                (3, 10, 12) | (0, 4, 12) => 5,
                (0, 3, 4) | (3, 1, 4) => 4,
                (0, _, 4) => 3,
                (_, _, 12) => 2,
                (_, _, 0) => 1,
                _ => 0,
            };
            if priority == 0 {
                continue;
            }
            let kind = match format {
                12 => {
                    let num_groups = u32_be(data, sub_off + 12)? as usize;
                    CmapKind::Format12 {
                        offset: sub_off,
                        num_groups,
                    }
                }
                4 => {
                    let seg_count_x2 = u16_be(data, sub_off + 6)? as usize;
                    if seg_count_x2 < 2 || (seg_count_x2 & 1) != 0 {
                        continue;
                    }
                    CmapKind::Format4 {
                        offset: sub_off,
                        seg_count_x2,
                    }
                }
                0 => CmapKind::Format0 { offset: sub_off },
                _ => continue,
            };

            if best.map_or(true, |(p, _)| priority > p) {
                best = Some((priority, kind));
            }
        }
        best.map(|(_, k)| k)
    }

    #[inline]
    pub fn metrics(&self) -> FontMetrics {
        self.metrics
    }

    #[inline]
    pub fn num_glyphs(&self) -> u16 {
        self.num_glyphs
    }

    /// Map Unicode scalar to glyph ID (`0` = `.notdef` missing glyph).
    pub fn glyph_index(&self, ch: char) -> u16 {
        let cp = ch as u32;
        match self.cmap {
            CmapKind::Format12 { offset, num_groups } => {
                let mut lo = 0usize;
                let mut hi = num_groups;
                while lo < hi {
                    let mid = (lo + hi) / 2;
                    let rec = offset + 16 + mid * 12;
                    let start = u32_be(self.data, rec).unwrap_or(0);
                    let end = u32_be(self.data, rec + 4).unwrap_or(0);
                    if cp < start {
                        hi = mid;
                    } else if cp > end {
                        lo = mid + 1;
                    } else {
                        let start_gid = u32_be(self.data, rec + 8).unwrap_or(0);
                        return (start_gid + (cp - start)) as u16;
                    }
                }
                0
            }
            CmapKind::Format4 { offset, seg_count_x2 } => {
                if cp > 0xFFFF {
                    return 0;
                }
                let c16 = cp as u16;
                let end_arr = offset + 14;
                let start_arr = end_arr + seg_count_x2 + 2;
                let delta_arr = start_arr + seg_count_x2;
                let range_arr = delta_arr + seg_count_x2;

                let segs = seg_count_x2 / 2;
                let mut lo = 0usize;
                let mut hi = segs;
                while lo < hi {
                    let mid = (lo + hi) / 2;
                    let end_code = u16_be(self.data, end_arr + mid * 2).unwrap_or(0xFFFF);
                    if end_code < c16 {
                        lo = mid + 1;
                    } else {
                        hi = mid;
                    }
                }
                if lo >= segs {
                    return 0;
                }
                let idx_off = lo * 2;
                let end_code = u16_be(self.data, end_arr + idx_off).unwrap_or(0);
                let start_code = u16_be(self.data, start_arr + idx_off).unwrap_or(0xFFFF);
                if c16 < start_code || c16 > end_code {
                    return 0;
                }
                let delta = i16_be(self.data, delta_arr + idx_off).unwrap_or(0);
                let range_offset = u16_be(self.data, range_arr + idx_off).unwrap_or(0);
                if range_offset == 0 {
                    (c16 as i32 + delta as i32) as u16
                } else {
                    let ro_pos = range_arr + idx_off;
                    let glyph_addr = ro_pos + range_offset as usize + ((c16 - start_code) as usize) * 2;
                    let gid = u16_be(self.data, glyph_addr).unwrap_or(0);
                    if gid == 0 {
                        0
                    } else {
                        (gid as i32 + delta as i32) as u16
                    }
                }
            }
            CmapKind::Format0 { offset } => {
                if cp < 256 {
                    self.data.get(offset + 6 + cp as usize).copied().unwrap_or(0) as u16
                } else {
                    0
                }
            }
        }
    }

    pub fn glyph_metrics(&self, gid: u16) -> GlyphMetrics {
        if self.num_hmetrics == 0 {
            return GlyphMetrics::default();
        }
        if gid < self.num_hmetrics {
            let off = self.hmtx_offset + (gid as usize) * 4;
            GlyphMetrics {
                advance_width: u16_be(self.data, off).unwrap_or(0),
                lsb: i16_be(self.data, off + 2).unwrap_or(0),
            }
        } else {
            let last_off = self.hmtx_offset + ((self.num_hmetrics as usize) - 1) * 4;
            let advance_width = u16_be(self.data, last_off).unwrap_or(0);
            let lsb_idx = (gid - self.num_hmetrics) as usize;
            let lsb_off = self.hmtx_offset + (self.num_hmetrics as usize) * 4 + lsb_idx * 2;
            GlyphMetrics {
                advance_width,
                lsb: i16_be(self.data, lsb_off).unwrap_or(0),
            }
        }
    }

    fn glyph_byte_range(&self, gid: u16) -> Option<(usize, usize)> {
        if gid >= self.num_glyphs {
            return None;
        }
        let g = gid as usize;
        let (s, e) = if self.index_to_loc_format == 0 {
            if (g + 2) * 2 > self.loca_len {
                return None;
            }
            let a = (u16_be(self.data, self.loca_offset + g * 2)? as usize) * 2;
            let b = (u16_be(self.data, self.loca_offset + (g + 1) * 2)? as usize) * 2;
            (a, b)
        } else {
            if (g + 2) * 4 > self.loca_len {
                return None;
            }
            let a = u32_be(self.data, self.loca_offset + g * 4)? as usize;
            let b = u32_be(self.data, self.loca_offset + (g + 1) * 4)? as usize;
            (a, b)
        };
        if s >= e || e > self.glyf_len {
            return None;
        }
        Some((self.glyf_offset + s, self.glyf_offset + e))
    }

    pub fn glyph_outline(&self, gid: u16) -> Option<Outline> {
        self.glyph_outline_depth(gid, 0)
    }

    fn glyph_outline_depth(&self, gid: u16, depth: u8) -> Option<Outline> {
        if depth > 8 {
            return None;
        }
        let (start, end) = self.glyph_byte_range(gid)?;
        if end - start < 10 {
            return None;
        }
        let num_contours = i16_be(self.data, start)?;
        let x_min = i16_be(self.data, start + 2)?;
        let y_min = i16_be(self.data, start + 4)?;
        let x_max = i16_be(self.data, start + 6)?;
        let y_max = i16_be(self.data, start + 8)?;

        if num_contours == 0 {
            return None;
        }
        if num_contours > 0 {
            return self.parse_simple_glyph(start + 10, end, num_contours as usize, x_min, y_min, x_max, y_max);
        }
        self.parse_compound_glyph(start + 10, end, depth, x_min, y_min, x_max, y_max)
    }

    fn parse_simple_glyph(
        &self,
        mut cur: usize,
        end: usize,
        num_contours: usize,
        x_min: i16,
        y_min: i16,
        x_max: i16,
        y_max: i16,
    ) -> Option<Outline> {
        if cur + num_contours * 2 + 2 > end {
            return None;
        }
        let mut end_pts = Vec::with_capacity(num_contours);
        let mut last_pt: usize = 0;
        for _ in 0..num_contours {
            let ep = u16_be(self.data, cur)? as usize;
            if ep < last_pt {
                return None;
            }
            last_pt = ep;
            end_pts.push(ep);
            cur += 2;
        }
        let total_pts = last_pt + 1;
        if total_pts > 4096 {
            return None;
        }
        let inst_len = u16_be(self.data, cur)? as usize;
        cur += 2;
        cur = cur.checked_add(inst_len)?;
        if cur > end {
            return None;
        }

        let mut flags = Vec::with_capacity(total_pts);
        while flags.len() < total_pts {
            let f = *self.data.get(cur)?;
            cur += 1;
            flags.push(f);
            if (f & 0x08) != 0 {
                let rep = *self.data.get(cur)? as usize;
                cur += 1;
                for _ in 0..rep {
                    if flags.len() >= total_pts {
                        break;
                    }
                    flags.push(f);
                }
            }
        }

        let mut xs = vec![0i32; total_pts];
        let mut x = 0i32;
        for i in 0..total_pts {
            let f = flags[i];
            let dx = if (f & 0x02) != 0 {
                let v = *self.data.get(cur)? as i32;
                cur += 1;
                if (f & 0x10) != 0 { v } else { -v }
            } else if (f & 0x10) != 0 {
                0
            } else {
                let v = i16_be(self.data, cur)? as i32;
                cur += 2;
                v
            };
            x += dx;
            xs[i] = x;
        }

        let mut ys = vec![0i32; total_pts];
        let mut y = 0i32;
        for i in 0..total_pts {
            let f = flags[i];
            let dy = if (f & 0x04) != 0 {
                let v = *self.data.get(cur)? as i32;
                cur += 1;
                if (f & 0x20) != 0 { v } else { -v }
            } else if (f & 0x20) != 0 {
                0
            } else {
                let v = i16_be(self.data, cur)? as i32;
                cur += 2;
                v
            };
            y += dy;
            ys[i] = y;
        }

        let mut contours = Vec::with_capacity(num_contours);
        let mut start_idx = 0usize;
        for ep in end_pts {
            if ep >= total_pts {
                return None;
            }
            let mut pts = Vec::with_capacity(ep - start_idx + 1);
            for i in start_idx..=ep {
                pts.push(OutlinePoint {
                    x: xs[i],
                    y: ys[i],
                    on_curve: (flags[i] & 0x01) != 0,
                });
            }
            contours.push(pts);
            start_idx = ep + 1;
        }

        Some(Outline {
            contours,
            x_min,
            y_min,
            x_max,
            y_max,
        })
    }

    fn parse_compound_glyph(
        &self,
        mut cur: usize,
        end: usize,
        depth: u8,
        x_min: i16,
        y_min: i16,
        x_max: i16,
        y_max: i16,
    ) -> Option<Outline> {
        const ARG_1_AND_2_ARE_WORDS: u16 = 0x0001;
        const ARGS_ARE_XY_VALUES: u16 = 0x0002;
        const WE_HAVE_A_SCALE: u16 = 0x0008;
        const MORE_COMPONENTS: u16 = 0x0020;
        const WE_HAVE_AN_X_AND_Y_SCALE: u16 = 0x0040;
        const WE_HAVE_A_TWO_BY_TWO: u16 = 0x0080;

        let mut contours = Vec::new();
        loop {
            if cur + 4 > end {
                break;
            }
            let flags = u16_be(self.data, cur)?;
            let comp_gid = u16_be(self.data, cur + 2)?;
            cur += 4;

            let (arg1, arg2) = if (flags & ARG_1_AND_2_ARE_WORDS) != 0 {
                let a = i16_be(self.data, cur)? as i32;
                let b = i16_be(self.data, cur + 2)? as i32;
                cur += 4;
                (a, b)
            } else {
                let a = (*self.data.get(cur)? as i8) as i32;
                let b = (*self.data.get(cur + 1)? as i8) as i32;
                cur += 2;
                (a, b)
            };

            // 2.14 F2Dot14 transform matrix: [a b; c d], identity = 16384
            let (mut a, mut b, mut c, mut d) = (16384i32, 0i32, 0i32, 16384i32);
            if (flags & WE_HAVE_A_SCALE) != 0 {
                let s = i16_be(self.data, cur)? as i32;
                cur += 2;
                a = s;
                d = s;
            } else if (flags & WE_HAVE_AN_X_AND_Y_SCALE) != 0 {
                a = i16_be(self.data, cur)? as i32;
                d = i16_be(self.data, cur + 2)? as i32;
                cur += 4;
            } else if (flags & WE_HAVE_A_TWO_BY_TWO) != 0 {
                a = i16_be(self.data, cur)? as i32;
                b = i16_be(self.data, cur + 2)? as i32;
                c = i16_be(self.data, cur + 4)? as i32;
                d = i16_be(self.data, cur + 6)? as i32;
                cur += 8;
            }

            let (tx, ty) = if (flags & ARGS_ARE_XY_VALUES) != 0 {
                (arg1, arg2)
            } else {
                (0, 0)
            };

            if let Some(sub) = self.glyph_outline_depth(comp_gid, depth + 1) {
                for contour in sub.contours {
                    let mut xformed = Vec::with_capacity(contour.len());
                    for p in contour {
                        let nx = ((a * p.x + c * p.y) >> 14) + tx;
                        let ny = ((b * p.x + d * p.y) >> 14) + ty;
                        xformed.push(OutlinePoint {
                            x: nx,
                            y: ny,
                            on_curve: p.on_curve,
                        });
                    }
                    contours.push(xformed);
                }
            }

            if (flags & MORE_COMPONENTS) == 0 {
                break;
            }
        }

        Some(Outline {
            contours,
            x_min,
            y_min,
            x_max,
            y_max,
        })
    }
}
