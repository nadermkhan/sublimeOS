use alloc::borrow::Cow;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use crate::font::fallback8x16::get_glyph_data;
use crate::font::raster::{rasterize_outline, RasterGlyph};
use crate::font::ttf::{FontMetrics, TtfFont};
use crate::gfx::{Color, Surface};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct GlyphKey {
    pub ch: u32,
    pub px_size: u16,
    pub bold: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AntialiasMode {
    ClearTypeRgb,
    Grayscale,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FontFamily {
    UiSans,
    Mono,
}

fn map_ttf_char(ch: char) -> char {
    match ch {
        '\u{2014}' | '\u{2013}' | '\u{2212}' => '-',
        '\u{2018}' | '\u{2019}' => '\'',
        '\u{201C}' | '\u{201D}' => '"',
        '\u{2022}' => '*',
        '\u{2026}' => '.',
        '\u{00A0}' => ' ',
        other => other,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TextStyle {
    pub family: FontFamily,
    pub size_px: i32,
    pub color: Color,
    pub bold: bool,
    pub shadow: Option<Color>,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            family: FontFamily::UiSans,
            size_px: 14,
            color: Color::BLACK,
            bold: false,
            shadow: None,
        }
    }
}

impl TextStyle {
    pub const fn ui(color: Color) -> Self {
        Self {
            family: FontFamily::UiSans,
            size_px: 14,
            color,
            bold: false,
            shadow: None,
        }
    }

    pub const fn ui_sized(size_px: i32, color: Color) -> Self {
        Self {
            family: FontFamily::UiSans,
            size_px,
            color,
            bold: false,
            shadow: None,
        }
    }

    pub const fn mono(color: Color) -> Self {
        Self {
            family: FontFamily::Mono,
            size_px: 16,
            color,
            bold: false,
            shadow: None,
        }
    }
}

/// Bounded cache of anti-aliased rasterized TTF glyphs.
pub struct FontAtlas {
    cache: BTreeMap<GlyphKey, RasterGlyph>,
    max_entries: usize,
}

impl FontAtlas {
    pub fn new(max_entries: usize) -> Self {
        Self {
            cache: BTreeMap::new(),
            max_entries: max_entries.max(64),
        }
    }

    pub fn clear(&mut self) {
        self.cache.clear();
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }

    pub fn get_or_rasterize(&mut self, font: &TtfFont<'_>, ch: char, px_size: i32, synth_bold: bool, key_bold: bool) -> &RasterGlyph {
        let px = px_size.clamp(6, 128) as u16;
        let key = GlyphKey {
            ch: ch as u32,
            px_size: px,
            bold: key_bold,
        };
        if !self.cache.contains_key(&key) {
            if self.cache.len() >= self.max_entries {
                // Evict oldest key in sorted BTreeMap order to bound heap memory
                if let Some(&first_k) = self.cache.keys().next() {
                    self.cache.remove(&first_k);
                }
            }
            let gid = if font.glyph_index(ch) != 0 || ch == ' ' {
                font.glyph_index(ch)
            } else {
                font.glyph_index(map_ttf_char(ch))
            };
            let gm = font.glyph_metrics(gid);
            let metrics = font.metrics();
            let mut raster = if let Some(outline) = font.glyph_outline(gid) {
                rasterize_outline(&outline, &metrics, gm.advance_width, px as i32)
            } else {
                let upem = metrics.units_per_em.max(1) as i64;
                let adv = ((gm.advance_width as i64 * px as i64 + upem / 2) / upem).max(3) as i32;
                RasterGlyph {
                    width: 0,
                    height: 0,
                    bearing_x: 0,
                    bearing_y: 0,
                    advance_x: adv,
                    bitmap: Vec::new(),
                    rgb_bitmap: Vec::new(),
                }
            };
            if synth_bold && !raster.bitmap.is_empty() && raster.width > 0 {
                apply_synthetic_bold(&mut raster);
            }
            self.cache.insert(key, raster);
        }
        self.cache.get(&key).unwrap()
    }
}

fn apply_synthetic_bold(g: &mut RasterGlyph) {
    let w = g.width as usize;
    let h = g.height as usize;
    let nw = w + 1;
    let mut out = alloc::vec![0u8; nw * h];
    let mut out_rgb = if g.rgb_bitmap.len() == w * h * 3 {
        alloc::vec![0u8; nw * h * 3]
    } else {
        Vec::new()
    };
    for r in 0..h {
        let src_row = &g.bitmap[r * w..(r + 1) * w];
        let dst_row = &mut out[r * nw..(r + 1) * nw];
        for c in 0..w {
            let a = src_row[c];
            dst_row[c] = dst_row[c].max(a);
            dst_row[c + 1] = dst_row[c + 1].max(a);
        }
        if !out_rgb.is_empty() {
            let srgb = &g.rgb_bitmap[r * w * 3..(r + 1) * w * 3];
            let drgb = &mut out_rgb[r * nw * 3..(r + 1) * nw * 3];
            for c in 0..w {
                for ch_i in 0..3 {
                    let v = srgb[c * 3 + ch_i];
                    drgb[c * 3 + ch_i] = drgb[c * 3 + ch_i].max(v);
                    drgb[(c + 1) * 3 + ch_i] = drgb[(c + 1) * 3 + ch_i].max(v);
                }
            }
        }
    }
    g.width += 1;
    g.advance_x += 1;
    g.bitmap = out;
    g.rgb_bitmap = out_rgb;
}

/// Complete text layout & rendering engine combining ClearType Noto Sans TTF vector fonts + `8x16` fallback.
pub struct FontEngine {
    ttf_bytes: Cow<'static, [u8]>,
    bold_ttf_bytes: Option<Cow<'static, [u8]>>,
    atlas: FontAtlas,
    pub antialias: AntialiasMode,
}

impl FontEngine {
    pub fn with_ttf_bytes(ttf_bytes: Vec<u8>) -> Self {
        Self {
            ttf_bytes: Cow::Owned(ttf_bytes),
            bold_ttf_bytes: None,
            atlas: FontAtlas::new(512),
            antialias: AntialiasMode::ClearTypeRgb,
        }
    }

    pub fn with_static_fonts(regular: &'static [u8], bold: Option<&'static [u8]>) -> Self {
        Self {
            ttf_bytes: Cow::Borrowed(regular),
            bold_ttf_bytes: bold.map(Cow::Borrowed),
            atlas: FontAtlas::new(512),
            antialias: AntialiasMode::ClearTypeRgb,
        }
    }

    pub fn set_ttf_bytes(&mut self, bytes: Vec<u8>) -> bool {
        if TtfFont::from_bytes(&bytes).is_some() {
            self.ttf_bytes = Cow::Owned(bytes);
            self.bold_ttf_bytes = None;
            self.atlas.clear();
            true
        } else {
            false
        }
    }

    pub fn ttf(&self) -> Option<TtfFont<'_>> {
        TtfFont::from_bytes(&self.ttf_bytes)
    }

    fn resolve_glyph(&mut self, ch: char, px_size: i32, bold: bool) -> Option<&RasterGlyph> {
        let regular = TtfFont::from_bytes(&self.ttf_bytes)?;
        let bold_font = if bold {
            self.bold_ttf_bytes.as_deref().and_then(TtfFont::from_bytes)
        } else {
            None
        };
        let lookup = if regular.glyph_index(ch) != 0 || ch == ' ' {
            ch
        } else {
            let mapped = map_ttf_char(ch);
            if regular.glyph_index(mapped) != 0 || mapped == ' ' {
                mapped
            } else {
                return None;
            }
        };
        if let Some(bf) = bold_font {
            if bf.glyph_index(lookup) != 0 || lookup == ' ' {
                return Some(self.atlas.get_or_rasterize(&bf, lookup, px_size, false, true));
            }
        }
        Some(self.atlas.get_or_rasterize(&regular, lookup, px_size, bold, bold))
    }

    pub fn cached_glyph_count(&self) -> usize {
        self.atlas.len()
    }

    pub fn line_metrics(&self, style: TextStyle) -> (i32, i32) {
        match style.family {
            FontFamily::Mono => (13, 16),
            FontFamily::UiSans => {
                if let Some(font) = self.ttf() {
                    let m: FontMetrics = font.metrics();
                    let upem = m.units_per_em.max(1) as i64;
                    let px = style.size_px.clamp(6, 128) as i64;
                    let asc = ((m.ascent as i64 * px + upem / 2) / upem).max(1) as i32;
                    let desc = ((-m.descent as i64 * px + upem / 2) / upem).max(0) as i32;
                    let gap = ((m.line_gap as i64 * px) / upem).max(0) as i32;
                    (asc, (asc + desc + gap).max(style.size_px + 2))
                } else {
                    (13, 16)
                }
            }
        }
    }

    pub fn measure_text(&mut self, text: &str, style: TextStyle) -> (i32, i32) {
        let (_, line_h) = self.line_metrics(style);
        if text.is_empty() {
            return (0, line_h);
        }
        let mut max_w = 0i32;
        let mut cur_w = 0i32;
        let mut lines = 1i32;
        for ch in text.chars() {
            if ch == '\n' {
                max_w = max_w.max(cur_w);
                cur_w = 0;
                lines += 1;
            } else {
                cur_w += self.char_advance(ch, style);
            }
        }
        (max_w.max(cur_w), lines * line_h)
    }

    pub fn char_advance(&mut self, ch: char, style: TextStyle) -> i32 {
        if style.family == FontFamily::Mono {
            return if style.size_px <= 16 { 8 } else { (8 * style.size_px + 8) / 16 };
        }
        if let Some(g) = self.resolve_glyph(ch, style.size_px, style.bold) {
            return g.advance_x;
        }
        // Fallback scaled bitmap glyph width
        ((8 * style.size_px.clamp(8, 64)) + 8) / 16
    }

    pub fn draw_text(&mut self, surface: &mut Surface, x: i32, y: i32, text: &str, style: TextStyle) -> i32 {
        if let Some(sh) = style.shadow {
            let mut sh_style = style;
            sh_style.color = sh;
            sh_style.shadow = None;
            self.draw_text_internal(surface, x + 1, y + 1, text, sh_style);
        }
        self.draw_text_internal(surface, x, y, text, style)
    }

    fn draw_text_internal(&mut self, surface: &mut Surface, x: i32, y: i32, text: &str, style: TextStyle) -> i32 {
        let (ascent, line_h) = self.line_metrics(style);
        let mut cx = x;
        let mut cy = y;
        let use_cleartype = self.antialias == AntialiasMode::ClearTypeRgb;

        for ch in text.chars() {
            if ch == '\n' {
                cx = x;
                cy += line_h;
                continue;
            }
            if style.family == FontFamily::Mono {
                draw_bitmap_char(surface, cx, cy, ch, style.color, style.bold);
                cx += 8;
                continue;
            }

            let rendered = if let Some(g) = self.resolve_glyph(ch, style.size_px, style.bold) {
                let baseline_y = cy + ascent;
                let gx = cx + g.bearing_x;
                let gy = baseline_y - g.bearing_y;
                let clip = surface.clip_rect();
                if !clip.is_empty()
                    && gx + g.width > clip.x
                    && gx < clip.right()
                    && gy + g.height > clip.y
                    && gy < clip.bottom()
                {
                    let has_rgb = use_cleartype && g.rgb_bitmap.len() == (g.width * g.height * 3) as usize;
                    for row in 0..g.height {
                        for col in 0..g.width {
                            if has_rgb {
                                let base = ((row * g.width + col) * 3) as usize;
                                let cr = g.rgb_bitmap[base];
                                let cg = g.rgb_bitmap[base + 1];
                                let cb = g.rgb_bitmap[base + 2];
                                if (cr | cg | cb) > 0 {
                                    surface.blend_subpixel_rgb(gx + col, gy + row, style.color, cr, cg, cb);
                                }
                            } else {
                                let cov = g.bitmap[(row * g.width + col) as usize];
                                if cov > 0 {
                                    surface.blend_coverage(gx + col, gy + row, style.color, cov);
                                }
                            }
                        }
                    }
                }
                cx += g.advance_x;
                true
            } else {
                false
            };

            if !rendered {
                let y_off = (line_h - 16) / 2;
                draw_bitmap_char(surface, cx, cy + y_off, ch, style.color, style.bold);
                cx += 8;
            }
        }
        cx - x
    }

    pub fn draw_text_ellipsis(
        &mut self,
        surface: &mut Surface,
        x: i32,
        y: i32,
        max_w: i32,
        text: &str,
        style: TextStyle,
    ) {
        if max_w <= 0 {
            return;
        }
        let (full_w, _) = self.measure_text(text, style);
        if full_w <= max_w {
            self.draw_text(surface, x, y, text, style);
            return;
        }
        let dots = "...";
        let (dots_w, _) = self.measure_text(dots, style);
        let budget = (max_w - dots_w).max(0);
        let mut used = 0i32;
        let mut byte_end = 0usize;
        for (idx, ch) in text.char_indices() {
            if ch == '\n' {
                break;
            }
            let adv = self.char_advance(ch, style);
            if used + adv > budget {
                break;
            }
            used += adv;
            byte_end = idx + ch.len_utf8();
        }
        if byte_end > 0 {
            let drawn = self.draw_text(surface, x, y, &text[..byte_end], style);
            self.draw_text(surface, x + drawn, y, dots, style);
        } else if dots_w <= max_w {
            self.draw_text(surface, x, y, dots, style);
        }
    }

    pub fn draw_text_wrapped(
        &mut self,
        surface: &mut Surface,
        x: i32,
        y: i32,
        max_w: i32,
        text: &str,
        style: TextStyle,
    ) -> i32 {
        if max_w <= 16 {
            return 0;
        }
        let (_, line_h) = self.line_metrics(style);
        let mut cy = y;
        let space_w = self.char_advance(' ', style);

        for raw_line in text.split('\n') {
            if raw_line.is_empty() {
                cy += line_h;
                continue;
            }
            let mut cx = x;
            for word in raw_line.split_whitespace() {
                let (ww, _) = self.measure_text(word, style);
                if cx > x && (cx - x) + space_w + ww > max_w {
                    cx = x;
                    cy += line_h;
                }
                if cx > x {
                    cx += space_w;
                }
                self.draw_text(surface, cx, cy, word, style);
                cx += ww;
            }
            cy += line_h;
        }
        cy - y
    }
}

fn draw_bitmap_char(surface: &mut Surface, x: i32, y: i32, ch: char, fg: Color, bold: bool) {
    let glyph = get_glyph_data(ch);
    for row in 0..16i32 {
        let bits = glyph[row as usize];
        if bits == 0 {
            continue;
        }
        let draw_bits = if bold { bits | (bits >> 1) } else { bits };
        for col in 0..8i32 {
            if (draw_bits & (0x80 >> col)) != 0 {
                surface.blend_pixel(x + col, y + row, fg);
            }
        }
    }
}
