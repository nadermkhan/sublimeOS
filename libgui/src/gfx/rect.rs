#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    #[inline(always)]
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Size {
    pub w: i32,
    pub h: i32,
}

impl Size {
    #[inline(always)]
    pub const fn new(w: i32, h: i32) -> Self {
        Self { w, h }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Insets {
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub left: i32,
}

impl Insets {
    #[inline(always)]
    pub const fn all(v: i32) -> Self {
        Self {
            top: v,
            right: v,
            bottom: v,
            left: v,
        }
    }

    #[inline(always)]
    pub const fn new(top: i32, right: i32, bottom: i32, left: i32) -> Self {
        Self { top, right, bottom, left }
    }

    #[inline(always)]
    pub const fn hv(h: i32, v: i32) -> Self {
        Self {
            top: v,
            right: h,
            bottom: v,
            left: h,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    #[inline(always)]
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    #[inline(always)]
    pub const fn right(&self) -> i32 {
        self.x + self.w
    }

    #[inline(always)]
    pub const fn bottom(&self) -> i32 {
        self.y + self.h
    }

    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    #[inline(always)]
    pub const fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }

    #[inline]
    pub fn intersect(&self, other: Rect) -> Rect {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = self.right().min(other.right());
        let y1 = self.bottom().min(other.bottom());
        Rect::new(x0, y0, (x1 - x0).max(0), (y1 - y0).max(0))
    }

    #[inline]
    pub fn intersects(&self, other: Rect) -> bool {
        !self.intersect(other).is_empty()
    }

    #[inline]
    pub fn union(&self, other: Rect) -> Rect {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return *self;
        }
        let x0 = self.x.min(other.x);
        let y0 = self.y.min(other.y);
        let x1 = self.right().max(other.right());
        let y1 = self.bottom().max(other.bottom());
        Rect::new(x0, y0, (x1 - x0).max(0), (y1 - y0).max(0))
    }

    #[inline]
    pub fn expand(&self, pad: i32) -> Rect {
        Rect::new(self.x - pad, self.y - pad, (self.w + pad * 2).max(0), (self.h + pad * 2).max(0))
    }

    #[inline]
    pub fn inset(&self, ins: Insets) -> Rect {
        Rect::new(
            self.x + ins.left,
            self.y + ins.top,
            (self.w - ins.left - ins.right).max(0),
            (self.h - ins.top - ins.bottom).max(0),
        )
    }

    /// Carve `top_h` pixels from the top, returning `(top_slice, remainder)`.
    #[inline]
    pub fn split_top(&self, top_h: i32, gap: i32) -> (Rect, Rect) {
        let h1 = top_h.clamp(0, self.h);
        let top = Rect::new(self.x, self.y, self.w, h1);
        let shift = (h1 + gap).min(self.h);
        let rem = Rect::new(self.x, self.y + shift, self.w, (self.h - shift).max(0));
        (top, rem)
    }

    /// Carve `bot_h` pixels from the bottom, returning `(remainder, bottom_slice)`.
    #[inline]
    pub fn split_bottom(&self, bot_h: i32, gap: i32) -> (Rect, Rect) {
        let h2 = bot_h.clamp(0, self.h);
        let bot = Rect::new(self.x, self.y + self.h - h2, self.w, h2);
        let used = (h2 + gap).min(self.h);
        let rem = Rect::new(self.x, self.y, self.w, (self.h - used).max(0));
        (rem, bot)
    }

    /// Carve `left_w` pixels from the left, returning `(left_slice, remainder)`.
    #[inline]
    pub fn split_left(&self, left_w: i32, gap: i32) -> (Rect, Rect) {
        let w1 = left_w.clamp(0, self.w);
        let left = Rect::new(self.x, self.y, w1, self.h);
        let shift = (w1 + gap).min(self.w);
        let rem = Rect::new(self.x + shift, self.y, (self.w - shift).max(0), self.h);
        (left, rem)
    }

    /// Carve `right_w` pixels from the right, returning `(remainder, right_slice)`.
    #[inline]
    pub fn split_right(&self, right_w: i32, gap: i32) -> (Rect, Rect) {
        let w2 = right_w.clamp(0, self.w);
        let right = Rect::new(self.x + self.w - w2, self.y, w2, self.h);
        let used = (w2 + gap).min(self.w);
        let rem = Rect::new(self.x, self.y, (self.w - used).max(0), self.h);
        (rem, right)
    }
}
