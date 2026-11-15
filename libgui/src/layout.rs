use alloc::vec::Vec;
use crate::gfx::{Insets, Rect};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Length {
    Px(i32),
    Flex(u16),
}

/// Vertical layout cursor that allocates stacked slices without overlap.
pub struct VStack {
    cursor_y: i32,
    x: i32,
    w: i32,
    bottom: i32,
    gap: i32,
}

impl VStack {
    pub fn new(bounds: Rect, gap: i32) -> Self {
        Self {
            cursor_y: bounds.y,
            x: bounds.x,
            w: bounds.w.max(0),
            bottom: bounds.bottom(),
            gap: gap.max(0),
        }
    }

    pub fn padded(bounds: Rect, padding: Insets, gap: i32) -> Self {
        Self::new(bounds.inset(padding), gap)
    }

    #[inline]
    pub fn remaining_height(&self) -> i32 {
        (self.bottom - self.cursor_y).max(0)
    }

    #[inline]
    pub fn remaining_rect(&self) -> Rect {
        Rect::new(self.x, self.cursor_y, self.w, self.remaining_height())
    }

    pub fn next(&mut self, height: i32) -> Rect {
        let h = height.clamp(0, self.remaining_height());
        let rect = Rect::new(self.x, self.cursor_y, self.w, h);
        self.cursor_y = (self.cursor_y + h + self.gap).min(self.bottom);
        rect
    }

    pub fn space(&mut self, height: i32) {
        self.cursor_y = (self.cursor_y + height.max(0)).min(self.bottom);
    }

    /// Take a slice anchored to the bottom of the remaining area and shrink `bottom`.
    pub fn take_bottom(&mut self, height: i32) -> Rect {
        let h = height.clamp(0, self.remaining_height());
        let y = self.bottom - h;
        self.bottom = (y - self.gap).max(self.cursor_y);
        Rect::new(self.x, y, self.w, h)
    }

    /// Allocate mixed fixed `Px` and weighted `Flex` rows across the remaining height.
    pub fn allocate(&mut self, specs: &[Length]) -> Vec<Rect> {
        let count = specs.len();
        if count == 0 {
            return Vec::new();
        }
        let total_gaps = self.gap * (count.saturating_sub(1) as i32);
        let avail = (self.remaining_height() - total_gaps).max(0);
        let mut fixed_sum = 0i32;
        let mut flex_total = 0i32;
        for spec in specs {
            match *spec {
                Length::Px(px) => fixed_sum += px.max(0),
                Length::Flex(w) => flex_total += w as i32,
            }
        }
        let flex_rem = (avail - fixed_sum).max(0);
        let mut out = Vec::with_capacity(count);
        let mut flex_acc = 0i32;
        let mut flex_used = 0i32;

        for spec in specs {
            let h = match *spec {
                Length::Px(px) => px.max(0),
                Length::Flex(w) => {
                    if flex_total <= 0 {
                        0
                    } else {
                        flex_acc += w as i32;
                        let target = ((flex_rem as i64 * flex_acc as i64) / flex_total as i64) as i32;
                        let slice = (target - flex_used).max(0);
                        flex_used += slice;
                        slice
                    }
                }
            };
            out.push(self.next(h));
        }
        out
    }
}

/// Horizontal layout cursor allocating left-to-right slices without overflow.
pub struct HStack {
    cursor_x: i32,
    y: i32,
    h: i32,
    right: i32,
    gap: i32,
}

impl HStack {
    pub fn new(bounds: Rect, gap: i32) -> Self {
        Self {
            cursor_x: bounds.x,
            y: bounds.y,
            h: bounds.h.max(0),
            right: bounds.right(),
            gap: gap.max(0),
        }
    }

    pub fn padded(bounds: Rect, padding: Insets, gap: i32) -> Self {
        Self::new(bounds.inset(padding), gap)
    }

    #[inline]
    pub fn remaining_width(&self) -> i32 {
        (self.right - self.cursor_x).max(0)
    }

    #[inline]
    pub fn remaining_rect(&self) -> Rect {
        Rect::new(self.cursor_x, self.y, self.remaining_width(), self.h)
    }

    pub fn next(&mut self, width: i32) -> Rect {
        let w = width.clamp(0, self.remaining_width());
        let rect = Rect::new(self.cursor_x, self.y, w, self.h);
        self.cursor_x = (self.cursor_x + w + self.gap).min(self.right);
        rect
    }

    pub fn space(&mut self, width: i32) {
        self.cursor_x = (self.cursor_x + width.max(0)).min(self.right);
    }

    pub fn take_right(&mut self, width: i32) -> Rect {
        let w = width.clamp(0, self.remaining_width());
        let x = self.right - w;
        self.right = (x - self.gap).max(self.cursor_x);
        Rect::new(x, self.y, w, self.h)
    }

    pub fn allocate(&mut self, specs: &[Length]) -> Vec<Rect> {
        let count = specs.len();
        if count == 0 {
            return Vec::new();
        }
        let total_gaps = self.gap * (count.saturating_sub(1) as i32);
        let avail = (self.remaining_width() - total_gaps).max(0);
        let mut fixed_sum = 0i32;
        let mut flex_total = 0i32;
        for spec in specs {
            match *spec {
                Length::Px(px) => fixed_sum += px.max(0),
                Length::Flex(w) => flex_total += w as i32,
            }
        }
        let flex_rem = (avail - fixed_sum).max(0);
        let mut out = Vec::with_capacity(count);
        let mut flex_acc = 0i32;
        let mut flex_used = 0i32;

        for spec in specs {
            let w = match *spec {
                Length::Px(px) => px.max(0),
                Length::Flex(weight) => {
                    if flex_total <= 0 {
                        0
                    } else {
                        flex_acc += weight as i32;
                        let target = ((flex_rem as i64 * flex_acc as i64) / flex_total as i64) as i32;
                        let slice = (target - flex_used).max(0);
                        flex_used += slice;
                        slice
                    }
                }
            };
            out.push(self.next(w));
        }
        out
    }
}

/// Two-region horizontal or vertical split helper.
pub struct SplitPane;

impl SplitPane {
    pub fn horizontal(bounds: Rect, left_width: i32, gap: i32) -> (Rect, Rect) {
        bounds.split_left(left_width, gap)
    }

    pub fn vertical(bounds: Rect, top_height: i32, gap: i32) -> (Rect, Rect) {
        bounds.split_top(top_height, gap)
    }
}

/// Uniform grid cell calculator (e.g. calculator buttons or icon grids).
pub struct Grid {
    pub bounds: Rect,
    pub cols: usize,
    pub rows: usize,
    pub gap_x: i32,
    pub gap_y: i32,
}

impl Grid {
    pub const fn new(bounds: Rect, cols: usize, rows: usize, gap_x: i32, gap_y: i32) -> Self {
        Self {
            bounds,
            cols: if cols == 0 { 1 } else { cols },
            rows: if rows == 0 { 1 } else { rows },
            gap_x,
            gap_y,
        }
    }

    pub fn cell(&self, col: usize, row: usize) -> Rect {
        self.cell_span(col, row, 1, 1)
    }

    pub fn cell_span(&self, col: usize, row: usize, col_span: usize, row_span: usize) -> Rect {
        let c = col.min(self.cols - 1);
        let r = row.min(self.rows - 1);
        let cs = col_span.max(1).min(self.cols - c);
        let rs = row_span.max(1).min(self.rows - r);

        let total_gx = self.gap_x.max(0) * (self.cols.saturating_sub(1) as i32);
        let total_gy = self.gap_y.max(0) * (self.rows.saturating_sub(1) as i32);
        let cell_area_w = (self.bounds.w - total_gx).max(0);
        let cell_area_h = (self.bounds.h - total_gy).max(0);

        let x0 = self.bounds.x
            + ((cell_area_w as i64 * c as i64) / self.cols as i64) as i32
            + self.gap_x.max(0) * c as i32;
        let x1 = self.bounds.x
            + ((cell_area_w as i64 * (c + cs) as i64) / self.cols as i64) as i32
            + self.gap_x.max(0) * (c + cs - 1) as i32;

        let y0 = self.bounds.y
            + ((cell_area_h as i64 * r as i64) / self.rows as i64) as i32
            + self.gap_y.max(0) * r as i32;
        let y1 = self.bounds.y
            + ((cell_area_h as i64 * (r + rs) as i64) / self.rows as i64) as i32
            + self.gap_y.max(0) * (r + rs - 1) as i32;

        Rect::new(x0, y0, (x1 - x0).max(0), (y1 - y0).max(0))
    }
}
