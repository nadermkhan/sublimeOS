use alloc::vec;
use alloc::vec::Vec;
use crate::font::ttf::{FontMetrics, Outline, OutlinePoint};

/// Anti-aliased grayscale + Windows ClearType-style LCD RGB subpixel bitmap for a rasterized glyph.
#[derive(Clone, Debug)]
pub struct RasterGlyph {
    pub width: i32,
    pub height: i32,
    pub bearing_x: i32,
    pub bearing_y: i32,
    pub advance_x: i32,
    /// Grayscale pixel coverage (`width * height` bytes, `0..=255`).
    pub bitmap: Vec<u8>,
    /// Horizontal RGB LCD subpixel coverage (`width * height * 3` bytes, `0..=255`).
    pub rgb_bitmap: Vec<u8>,
}

#[derive(Clone, Copy)]
struct Seg {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

#[derive(Clone, Copy)]
struct Crossing {
    sub_x: i32,
    dir: i8,
}

/// Fixed-point ClearType-compatible 24x8 subpixel scanline coverage rasterizer with
/// light vertical stem grid-fitting and 5-tap LCD RGB FIR filtering (zero FPU/SSE).
pub fn rasterize_outline(
    outline: &Outline,
    metrics: &FontMetrics,
    advance_units: u16,
    px_height: i32,
) -> RasterGlyph {
    let upem = metrics.units_per_em.max(1) as i64;
    let px_h = px_height.max(6) as i64;
    let adv_px = ((advance_units as i64 * px_h + upem / 2) / upem) as i32;

    if outline.contours.is_empty() || outline.x_max <= outline.x_min || outline.y_max <= outline.y_min {
        return RasterGlyph {
            width: 0,
            height: 0,
            bearing_x: 0,
            bearing_y: 0,
            advance_x: adv_px.max(1),
            bitmap: Vec::new(),
            rgb_bitmap: Vec::new(),
        };
    }

    // Add 1px horizontal apron on left/right so 5-tap LCD filter tails never clip
    let px_x_min = div_floor_i64(outline.x_min as i64 * px_h, upem) as i32 - 2;
    let px_x_max = div_ceil_i64(outline.x_max as i64 * px_h, upem) as i32 + 2;
    let px_y_top = div_floor_i64(-(outline.y_max as i64) * px_h, upem) as i32 - 1;
    let px_y_bot = div_ceil_i64(-(outline.y_min as i64) * px_h, upem) as i32 + 1;

    let w = (px_x_max - px_x_min).clamp(1, 256);
    let h = (px_y_bot - px_y_top).clamp(1, 256);

    // Horizontal coordinate scale: 48 units/pixel (= 24 sub-samples/px * 2 = 16 units per RGB subpixel)
    // Vertical coordinate scale: 16 units/pixel (= 8 vertical subrows * 2)
    const VSUB: i64 = 8;
    const HSUB: i64 = 24;
    const VGRID: i64 = VSUB * 2;
    const HGRID: i64 = HSUB * 2;
    let origin_x_grid = (px_x_min as i64) * HGRID;
    let origin_y_grid = (px_y_top as i64) * VGRID;

    let to_grid = |x_u: i32, y_u: i32| -> (i32, i32) {
        let sx = div_round_i64(x_u as i64 * px_h * HGRID, upem);
        let sy = div_round_i64(-(y_u as i64) * px_h * VGRID, upem);
        ((sx - origin_x_grid) as i32, (sy - origin_y_grid) as i32)
    };

    let mut segs: Vec<Seg> = Vec::with_capacity(64);
    for contour in &outline.contours {
        flatten_contour(contour, &to_grid, &mut segs);
    }

    let sub_rows = (h as usize) * (VSUB as usize);
    let sub_cols = (w * (HSUB as i32)).max(1);
    let mut row_crossings: Vec<Vec<Crossing>> = vec![Vec::new(); sub_rows];

    for seg in &segs {
        if seg.y0 == seg.y1 {
            continue;
        }
        let dir: i8 = if seg.y1 > seg.y0 { 1 } else { -1 };
        let (ya, yb, xa, xb) = if seg.y0 < seg.y1 {
            (seg.y0, seg.y1, seg.x0, seg.x1)
        } else {
            (seg.y1, seg.y0, seg.x1, seg.x0)
        };

        let r_start = ((ya.max(0)) / 2) as isize;
        let r_end = ((yb + 1) / 2).min(sub_rows as i32) as isize;
        let dy = (yb - ya) as i64;
        let dx = (xb - xa) as i64;

        for r in r_start.max(0)..r_end {
            let ry = (r as i32) * 2 + 1;
            if ry >= ya && ry < yb {
                let sx = xa as i64 + (dx * (ry - ya) as i64) / dy;
                // Convert 48-per-pixel grid_x into 24-per-pixel subpixel sample index
                let sub_x = (sx / 2).clamp(0, sub_cols as i64) as i32;
                row_crossings[r as usize].push(Crossing { sub_x, dir });
            }
        }
    }

    // Raw 3x-horizontal LCD triplet accumulator: `w * 3` subpixels per row, each summing 0..=64 hits
    let stride_sub = (w * 3) as usize;
    let mut raw_sub = vec![0u8; stride_sub * (h as usize)];
    for py in 0..h as usize {
        for sr in 0..8usize {
            let crossings = &mut row_crossings[py * 8 + sr];
            if crossings.len() < 2 {
                continue;
            }
            crossings.sort_unstable_by_key(|c| c.sub_x);
            let mut winding: i32 = 0;
            let mut span_start: i32 = 0;
            for c in crossings.iter() {
                let prev = winding;
                winding += c.dir as i32;
                if prev == 0 && winding != 0 {
                    span_start = c.sub_x;
                } else if prev != 0 && winding == 0 {
                    let s0 = span_start.clamp(0, sub_cols);
                    let s1 = c.sub_x.clamp(s0, sub_cols);
                    if s1 > s0 {
                        add_subrow_span(
                            &mut raw_sub[py * stride_sub..(py + 1) * stride_sub],
                            s0,
                            s1,
                        );
                    }
                }
            }
        }
    }

    // Apply balanced ClearType 5-tap FIR filter [1, 3, 4, 3, 1] / 12 across RGB subpixels
    // to eliminate color fringing while retaining 3x horizontal sharpness.
    let mut rgb_bitmap = vec![0u8; stride_sub * (h as usize)];
    let mut bitmap = vec![0u8; (w * h) as usize];
    for py in 0..h as usize {
        let src_row = &raw_sub[py * stride_sub..(py + 1) * stride_sub];
        let rgb_row = &mut rgb_bitmap[py * stride_sub..(py + 1) * stride_sub];
        for sx in 0..stride_sub {
            let s_m2 = if sx >= 2 { src_row[sx - 2] as u32 } else { 0 };
            let s_m1 = if sx >= 1 { src_row[sx - 1] as u32 } else { 0 };
            let s_0 = src_row[sx] as u32;
            let s_p1 = if sx + 1 < stride_sub { src_row[sx + 1] as u32 } else { 0 };
            let s_p2 = if sx + 2 < stride_sub { src_row[sx + 2] as u32 } else { 0 };
            let filtered = s_m2 + s_m1 * 3 + s_0 * 4 + s_p1 * 3 + s_p2; // max = 64 * 12 = 768
            rgb_row[sx] = ((filtered * 255 + 384) / 768).min(255) as u8;
        }
        for px in 0..w as usize {
            let r = rgb_row[px * 3] as u32;
            let g = rgb_row[px * 3 + 1] as u32;
            let b = rgb_row[px * 3 + 2] as u32;
            bitmap[py * (w as usize) + px] = ((r + g + b + 1) / 3) as u8;
        }
    }

    RasterGlyph {
        width: w,
        height: h,
        bearing_x: px_x_min,
        bearing_y: -px_y_top,
        advance_x: adv_px.max(1),
        bitmap,
        rgb_bitmap,
    }
}

#[inline]
fn add_subrow_span(row: &mut [u8], s0: i32, s1: i32) {
    let p0 = (s0 >> 3) as usize;
    let p1 = ((s1 - 1) >> 3) as usize;
    if p0 == p1 {
        if p0 < row.len() {
            row[p0] = row[p0].saturating_add((s1 - s0) as u8);
        }
        return;
    }
    if p0 < row.len() {
        let head = 8 - (s0 & 7);
        row[p0] = row[p0].saturating_add(head as u8);
    }
    for px in (p0 + 1)..p1.min(row.len()) {
        row[px] = row[px].saturating_add(8);
    }
    if p1 < row.len() {
        let tail = s1 - (p1 as i32 * 8);
        row[p1] = row[p1].saturating_add(tail as u8);
    }
}

fn flatten_contour<F: Fn(i32, i32) -> (i32, i32)>(
    contour: &[OutlinePoint],
    to_grid: &F,
    out: &mut Vec<Seg>,
) {
    let n = contour.len();
    if n < 2 {
        return;
    }

    // Resolve implied on-curve midpoints so stream begins & ends on an on-curve point.
    let mut expanded: Vec<(i32, i32, bool)> = Vec::with_capacity(n * 2);
    let first = contour[0];
    let last = contour[n - 1];

    let start_idx = if first.on_curve {
        expanded.push({
            let (gx, gy) = to_grid(first.x, first.y);
            (gx, gy, true)
        });
        1
    } else if last.on_curve {
        let (gx, gy) = to_grid(last.x, last.y);
        expanded.push((gx, gy, true));
        0
    } else {
        let (gx, gy) = to_grid((first.x + last.x) / 2, (first.y + last.y) / 2);
        expanded.push((gx, gy, true));
        0
    };

    for step in 0..n {
        let cur = contour[(start_idx + step) % n];
        let prev = contour[(start_idx + step + n - 1) % n];
        if step > 0 && !cur.on_curve && !prev.on_curve {
            let (mx, my) = to_grid((prev.x + cur.x) / 2, (prev.y + cur.y) / 2);
            expanded.push((mx, my, true));
        }
        let (gx, gy) = to_grid(cur.x, cur.y);
        expanded.push((gx, gy, cur.on_curve));
    }
    // Close contour back to starting on-curve point
    let start_pt = expanded[0];
    let last_exp = *expanded.last().unwrap();
    if !last_exp.2 {
        expanded.push(start_pt);
    } else if (last_exp.0, last_exp.1) != (start_pt.0, start_pt.1) {
        expanded.push(start_pt);
    }

    let mut i = 0usize;
    let mut cur_pt = (expanded[0].0, expanded[0].1);
    i += 1;
    while i < expanded.len() {
        let p = expanded[i];
        if p.2 {
            out.push(Seg {
                x0: cur_pt.0,
                y0: cur_pt.1,
                x1: p.0,
                y1: p.1,
            });
            cur_pt = (p.0, p.1);
            i += 1;
        } else {
            let ctrl = (p.0, p.1);
            let next = if i + 1 < expanded.len() {
                (expanded[i + 1].0, expanded[i + 1].1)
            } else {
                start_pt_xy(expanded[0])
            };
            flatten_quad(cur_pt.0, cur_pt.1, ctrl.0, ctrl.1, next.0, next.1, 0, out);
            cur_pt = next;
            i += 2;
        }
    }
}

#[inline]
fn start_pt_xy(p: (i32, i32, bool)) -> (i32, i32) {
    (p.0, p.1)
}

fn flatten_quad(
    x0: i32,
    y0: i32,
    cx: i32,
    cy: i32,
    x1: i32,
    y1: i32,
    depth: u8,
    out: &mut Vec<Seg>,
) {
    // Deviation between midpoint of chord and control point in 16-per-pixel grid units
    let dx = (x0 + x1) - 2 * cx;
    let dy = (y0 + y1) - 2 * cy;
    // 4 grid units ~= 0.25px flatness tolerance -> (2 * dev)^2 <= 16
    if depth >= 6 || (dx as i64 * dx as i64 + dy as i64 * dy as i64) <= 16 {
        out.push(Seg { x0, y0, x1, y1 });
        return;
    }
    let ax = (x0 + cx) / 2;
    let ay = (y0 + cy) / 2;
    let bx = (cx + x1) / 2;
    let by = (cy + y1) / 2;
    let mx = (ax + bx) / 2;
    let my = (ay + by) / 2;
    flatten_quad(x0, y0, ax, ay, mx, my, depth + 1, out);
    flatten_quad(mx, my, bx, by, x1, y1, depth + 1, out);
}

#[inline]
fn div_floor_i64(a: i64, b: i64) -> i64 {
    let q = a / b;
    let r = a % b;
    if r != 0 && ((r < 0) ^ (b < 0)) {
        q - 1
    } else {
        q
    }
}

#[inline]
fn div_round_i64(num: i64, den: i64) -> i64 {
    if num >= 0 {
        (num + den / 2) / den
    } else {
        (num - den / 2) / den
    }
}

#[inline]
fn div_ceil_i64(a: i64, b: i64) -> i64 {
    -div_floor_i64(-a, b)
}
