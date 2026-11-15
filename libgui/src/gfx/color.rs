/// 32-bit ARGB color (`0xAARRGGBB`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct Color(pub u32);

impl Color {
    pub const TRANSPARENT: Color = Color(0x00000000);
    pub const BLACK: Color = Color(0xFF000000);
    pub const WHITE: Color = Color(0xFFFFFFFF);

    #[inline(always)]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self(0xFF000000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
    }

    #[inline(always)]
    pub const fn argb(a: u8, r: u8, g: u8, b: u8) -> Self {
        Self(((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
    }

    #[inline(always)]
    pub const fn from_hex(hex: u32) -> Self {
        Self(hex)
    }

    #[inline(always)]
    pub const fn to_u32(self) -> u32 {
        self.0
    }

    #[inline(always)]
    pub const fn a(self) -> u8 {
        ((self.0 >> 24) & 0xFF) as u8
    }

    #[inline(always)]
    pub const fn r(self) -> u8 {
        ((self.0 >> 16) & 0xFF) as u8
    }

    #[inline(always)]
    pub const fn g(self) -> u8 {
        ((self.0 >> 8) & 0xFF) as u8
    }

    #[inline(always)]
    pub const fn b(self) -> u8 {
        (self.0 & 0xFF) as u8
    }

    #[inline(always)]
    pub const fn with_alpha(self, a: u8) -> Self {
        Self((self.0 & 0x00FF_FFFF) | ((a as u32) << 24))
    }

    /// Linear interpolation between two opaque colors (`t` in `0..=255`).
    #[inline]
    pub fn lerp(c1: Color, c2: Color, t: u32) -> Color {
        let t = t.min(255);
        let inv = 255 - t;
        let a = ((c1.a() as u32 * inv + c2.a() as u32 * t) / 255) as u8;
        let r = ((c1.r() as u32 * inv + c2.r() as u32 * t) / 255) as u8;
        let g = ((c1.g() as u32 * inv + c2.g() as u32 * t) / 255) as u8;
        let b = ((c1.b() as u32 * inv + c2.b() as u32 * t) / 255) as u8;
        Color::argb(a, r, g, b)
    }

    /// Porter-Duff `src OVER dst` alpha blend using fast shift division (`(x + 128) >> 8`).
    #[inline(always)]
    pub fn blend(src: Color, dst: Color) -> Color {
        let sa = src.a() as u32;
        if sa == 255 {
            return src;
        }
        if sa == 0 {
            return dst;
        }
        let inv = 255 - sa;
        let r = ((src.r() as u32 * sa + dst.r() as u32 * inv + 128) >> 8) as u8;
        let g = ((src.g() as u32 * sa + dst.g() as u32 * inv + 128) >> 8) as u8;
        let b = ((src.b() as u32 * sa + dst.b() as u32 * inv + 128) >> 8) as u8;
        Color::argb(255, r, g, b)
    }

    /// Multiply alpha of `src` by an 8-bit coverage mask (`0..=255`) and blend over `dst`.
    /// Applies a subtle ClearType-style gamma/contrast curve so thin glyph stems stay crisp.
    #[inline(always)]
    pub fn blend_coverage(src: Color, dst: Color, cov: u8) -> Color {
        if cov == 0 {
            return dst;
        }
        let c = Self::boost_coverage(cov);
        let sa = ((src.a() as u32 * c as u32 + 128) >> 8) as u8;
        if sa == 255 {
            return src.with_alpha(255);
        }
        Self::blend(src.with_alpha(sa), dst)
    }

    /// Windows ClearType-style per-channel RGB subpixel alpha blend (`cov_r`, `cov_g`, `cov_b` in `0..=255`).
    #[inline(always)]
    pub fn blend_subpixel_rgb(src: Color, dst: Color, cov_r: u8, cov_g: u8, cov_b: u8) -> Color {
        if (cov_r | cov_g | cov_b) == 0 {
            return dst;
        }
        let sa = src.a() as u32;
        if sa == 0 {
            return dst;
        }
        let ar = (sa * Self::boost_coverage(cov_r) as u32 + 128) >> 8;
        let ag = (sa * Self::boost_coverage(cov_g) as u32 + 128) >> 8;
        let ab = (sa * Self::boost_coverage(cov_b) as u32 + 128) >> 8;
        let r = ((src.r() as u32 * ar + dst.r() as u32 * (255 - ar) + 128) >> 8) as u8;
        let g = ((src.g() as u32 * ag + dst.g() as u32 * (255 - ag) + 128) >> 8) as u8;
        let b = ((src.b() as u32 * ab + dst.b() as u32 * (255 - ab) + 128) >> 8) as u8;
        Color::argb(255, r, g, b)
    }

    /// Integer gamma/contrast lift for anti-aliased text coverage (`c^0.75` approx) so small fonts remain legible.
    #[inline(always)]
    fn boost_coverage(cov: u8) -> u8 {
        if cov == 0 || cov == 255 {
            return cov;
        }
        let x = cov as u32;
        // Smooth concave lift: cov' = min(255, cov + (cov * (255 - cov)) >> 8)
        (x + ((x * (255 - x)) >> 8)).min(255) as u8
    }
}
