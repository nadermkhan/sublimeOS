#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct Color(pub u32);

impl Color {
    pub const TRANSPARENT: Color = Color(0x00000000);
    pub const BLACK: Color = Color(0xFF000000);
    pub const WHITE: Color = Color(0xFFFFFFFF);

    // Cosmic palette
    pub const BG_DEEP_SPACE: Color = Color(0xFF0B0E14);
    pub const BG_MID_SPACE: Color = Color(0xFF161822);
    pub const BG_NEBULA_PURPLE: Color = Color(0xFF231C3D);
    pub const BG_CARD: Color = Color(0xFF1E2030);
    pub const BG_INPUT: Color = Color(0xFF13141C);
    
    // Window styling
    pub const TITLEBAR_ACTIVE_START: Color = Color(0xFF433878);
    pub const TITLEBAR_ACTIVE_END: Color = Color(0xFF2D2354);
    pub const TITLEBAR_INACTIVE: Color = Color(0xFF222436);
    pub const WINDOW_BORDER: Color = Color(0xFF383E5A);
    pub const WINDOW_BORDER_ACTIVE: Color = Color(0xFF6B5B95);
    
    // Window buttons
    pub const BTN_CLOSE: Color = Color(0xFFF7768E);
    pub const BTN_MINIMIZE: Color = Color(0xFFE0AF68);
    pub const BTN_MAXIMIZE: Color = Color(0xFF9ECE6A);
    pub const BTN_HOVER_OVERLAY: Color = Color(0x33FFFFFF);

    // Taskbar styling
    pub const TASKBAR_BG: Color = Color(0xEE111219);
    pub const TASKBAR_BORDER: Color = Color(0xFF2D3149);
    pub const TASKBAR_TAB: Color = Color(0xFF222436);
    pub const TASKBAR_TAB_ACTIVE: Color = Color(0xFF3B4261);
    pub const START_BTN_NORMAL: Color = Color(0xFF5B4B8A);
    pub const START_BTN_ACTIVE: Color = Color(0xFF7A64B8);

    // Accents & text
    pub const TEXT_PRIMARY: Color = Color(0xFFC0CAF5);
    pub const TEXT_SECONDARY: Color = Color(0xFF7982A9);
    pub const TEXT_MUTED: Color = Color(0xFF565F89);
    pub const ACCENT_CYAN: Color = Color(0xFF7DCFFF);
    pub const ACCENT_PURPLE: Color = Color(0xFFBB9AF7);
    pub const ACCENT_BLUE: Color = Color(0xFF7AA2F7);
    pub const ACCENT_GREEN: Color = Color(0xFF73DACA);
    pub const ACCENT_YELLOW: Color = Color(0xFFE0AF68);
    pub const ACCENT_ORANGE: Color = Color(0xFFFF9E64);
    pub const ACCENT_RED: Color = Color(0xFFF7768E);

    #[inline(always)]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Color(0xFF000000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
    }

    #[inline(always)]
    pub const fn argb(a: u8, r: u8, g: u8, b: u8) -> Self {
        Color(((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
    }

    #[inline(always)]
    pub const fn from_hex(hex: u32) -> Self {
        Color(hex)
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
    pub const fn to_u32(self) -> u32 {
        self.0
    }

    /// Linear interpolation between two colors by factor t (0..255).
    pub fn lerp(c1: Color, c2: Color, t: u32) -> Color {
        let t = t.min(255);
        let inv_t = 255 - t;

        let a = ((c1.a() as u32 * inv_t + c2.a() as u32 * t) / 255) as u8;
        let r = ((c1.r() as u32 * inv_t + c2.r() as u32 * t) / 255) as u8;
        let g = ((c1.g() as u32 * inv_t + c2.g() as u32 * t) / 255) as u8;
        let b = ((c1.b() as u32 * inv_t + c2.b() as u32 * t) / 255) as u8;

        Color::argb(a, r, g, b)
    }

    /// Alpha blend source color onto destination color.
    #[inline(always)]
    pub fn blend(src: Color, dst: Color) -> Color {
        let sa = src.a() as u32;
        if sa == 255 {
            return src;
        }
        if sa == 0 {
            return dst;
        }

        let inv_sa = 255 - sa;
        let r = ((src.r() as u32 * sa + dst.r() as u32 * inv_sa) / 255) as u8;
        let g = ((src.g() as u32 * sa + dst.g() as u32 * inv_sa) / 255) as u8;
        let b = ((src.b() as u32 * sa + dst.b() as u32 * inv_sa) / 255) as u8;

        Color::argb(255, r, g, b)
    }
}
