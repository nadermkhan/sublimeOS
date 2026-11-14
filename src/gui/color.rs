#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct Color(pub u32);

impl Color {
    pub const TRANSPARENT: Color = Color(0x00000000);
    pub const BLACK: Color = Color(0xFF000000);
    pub const WHITE: Color = Color(0xFFFFFFFF);

    // Rolling meadow wallpaper palette
    pub const XP_SKY_TOP: Color = Color(0xFF2B6CC4);
    pub const XP_SKY_MID: Color = Color(0xFF5FA5EC);
    pub const XP_SKY_HORIZON: Color = Color(0xFF9ED0FA);
    pub const XP_HILL_FAR: Color = Color(0xFF6BB246);
    pub const XP_HILL_LIGHT: Color = Color(0xFF72C033);
    pub const XP_HILL_MID: Color = Color(0xFF4CA02B);
    pub const XP_HILL_DARK: Color = Color(0xFF347A1C);

    // Classic Blue desktop window chrome
    pub const XP_TITLE_HIGHLIGHT: Color = Color(0xFF3E94FF);
    pub const XP_TITLE_TOP: Color = Color(0xFF0058EE);
    pub const XP_TITLE_MID: Color = Color(0xFF004AD6);
    pub const XP_TITLE_BOTTOM: Color = Color(0xFF003EB5);
    pub const XP_TITLE_INACTIVE_TOP: Color = Color(0xFF9DB8EC);
    pub const XP_TITLE_INACTIVE_MID: Color = Color(0xFF7B98DF);
    pub const XP_TITLE_INACTIVE_BOT: Color = Color(0xFF6B86CC);
    pub const XP_FRAME_OUTER: Color = Color(0xFF001E73);
    pub const XP_FRAME_ACTIVE: Color = Color(0xFF0048CC);
    pub const XP_FRAME_INACTIVE: Color = Color(0xFF7A96DF);

    // Window caption buttons
    pub const XP_CAP_BLUE_TOP: Color = Color(0xFF63AAFF);
    pub const XP_CAP_BLUE_BOT: Color = Color(0xFF1F63E0);
    pub const XP_CAP_BLUE_HOVER_TOP: Color = Color(0xFF8CC2FF);
    pub const XP_CAP_BLUE_HOVER_BOT: Color = Color(0xFF3A7EF5);
    pub const XP_CAP_RED_TOP: Color = Color(0xFFEF7A5E);
    pub const XP_CAP_RED_BOT: Color = Color(0xFFC43014);
    pub const XP_CAP_RED_HOVER_TOP: Color = Color(0xFFFF987E);
    pub const XP_CAP_RED_HOVER_BOT: Color = Color(0xFFDE4223);

    // Classic dialog & control surfaces
    pub const XP_FACE: Color = Color(0xFFECE9D8);
    pub const XP_FACE_LIGHT: Color = Color(0xFFF6F4EC);
    pub const XP_FACE_DARK: Color = Color(0xFFD6D2C2);
    pub const XP_WHITE: Color = Color(0xFFFFFFFF);
    pub const XP_SHADOW: Color = Color(0xFFACA899);
    pub const XP_DARK_SHADOW: Color = Color(0xFF716F64);
    pub const XP_CTRL_BORDER: Color = Color(0xFF003C74);
    pub const XP_INPUT_BORDER: Color = Color(0xFF7F9DB9);
    pub const XP_SELECTION: Color = Color(0xFF316AC5);
    pub const XP_TEXT_BLACK: Color = Color(0xFF000000);
    pub const XP_TEXT_GRAY: Color = Color(0xFF5A5A5A);
    pub const XP_TEXT_LINK: Color = Color(0xFF215DC6);
    pub const XP_ORANGE_GLOW: Color = Color(0xFFE68B2C);

    // Taskbar, Start button, and Start Menu
    pub const XP_TASKBAR_HILITE: Color = Color(0xFF4C97F0);
    pub const XP_TASKBAR_TOP: Color = Color(0xFF245EDC);
    pub const XP_TASKBAR_MID: Color = Color(0xFF1E52C6);
    pub const XP_TASKBAR_BOT: Color = Color(0xFF18409D);
    pub const XP_TAB_ACTIVE: Color = Color(0xFF17419E);
    pub const XP_TAB_TOP: Color = Color(0xFF3C81F3);
    pub const XP_TAB_BOT: Color = Color(0xFF2764D8);
    pub const XP_START_TOP: Color = Color(0xFF72C964);
    pub const XP_START_MID: Color = Color(0xFF3D9C30);
    pub const XP_START_BOT: Color = Color(0xFF28751E);
    pub const XP_START_HOVER_TOP: Color = Color(0xFF83DA74);
    pub const XP_START_HOVER_MID: Color = Color(0xFF49B23A);
    pub const XP_TRAY_TOP: Color = Color(0xFF1491EB);
    pub const XP_TRAY_BOT: Color = Color(0xFF0C6EC2);
    pub const XP_TRAY_BORDER: Color = Color(0xFF35B4FF);
    pub const XP_START_PLACES_BG: Color = Color(0xFFD3E5FA);
    pub const XP_START_PLACES_DIV: Color = Color(0xFF95BDEE);

    // Semantic theme aliases
    pub const BG_DEEP_SPACE: Color = Self::XP_SKY_TOP;
    pub const BG_MID_SPACE: Color = Self::XP_FACE;
    pub const BG_NEBULA_PURPLE: Color = Self::XP_SKY_HORIZON;
    pub const BG_CARD: Color = Self::XP_FACE;
    pub const BG_INPUT: Color = Self::XP_WHITE;

    // Window styling
    pub const TITLEBAR_ACTIVE_START: Color = Self::XP_TITLE_TOP;
    pub const TITLEBAR_ACTIVE_END: Color = Self::XP_TITLE_BOTTOM;
    pub const TITLEBAR_INACTIVE: Color = Self::XP_TITLE_INACTIVE_MID;
    pub const WINDOW_BORDER: Color = Self::XP_SHADOW;
    pub const WINDOW_BORDER_ACTIVE: Color = Self::XP_FRAME_ACTIVE;

    // Window buttons
    pub const BTN_CLOSE: Color = Self::XP_CAP_RED_TOP;
    pub const BTN_MINIMIZE: Color = Self::XP_CAP_BLUE_TOP;
    pub const BTN_MAXIMIZE: Color = Self::XP_CAP_BLUE_TOP;
    pub const BTN_HOVER_OVERLAY: Color = Color(0x33FFFFFF);

    // Taskbar styling
    pub const TASKBAR_BG: Color = Self::XP_TASKBAR_MID;
    pub const TASKBAR_BORDER: Color = Self::XP_TASKBAR_HILITE;
    pub const TASKBAR_TAB: Color = Self::XP_TAB_TOP;
    pub const TASKBAR_TAB_ACTIVE: Color = Self::XP_TAB_ACTIVE;
    pub const START_BTN_NORMAL: Color = Self::XP_START_MID;
    pub const START_BTN_ACTIVE: Color = Self::XP_START_HOVER_MID;

    // Accents & text
    pub const TEXT_PRIMARY: Color = Self::XP_TEXT_BLACK;
    pub const TEXT_SECONDARY: Color = Self::XP_TEXT_GRAY;
    pub const TEXT_MUTED: Color = Self::XP_TEXT_GRAY;
    pub const ACCENT_CYAN: Color = Color(0xFF0078D7);
    pub const ACCENT_PURPLE: Color = Color(0xFF6A48B8);
    pub const ACCENT_BLUE: Color = Color(0xFF316AC5);
    pub const ACCENT_GREEN: Color = Color(0xFF3C9C30);
    pub const ACCENT_YELLOW: Color = Color(0xFFE68B2C);
    pub const ACCENT_ORANGE: Color = Color(0xFFE86C24);
    pub const ACCENT_RED: Color = Color(0xFFD63218);

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

    /// Linear interpolation between two colors by factor t (0..255) using fast bit shifts.
    #[inline(always)]
    pub fn lerp(c1: Color, c2: Color, t: u32) -> Color {
        let t = t.min(255);
        let inv_t = 256 - t;
        let t_plus = t + 1;

        let a = ((c1.a() as u32 * inv_t + c2.a() as u32 * t_plus) >> 8) as u8;
        let r = ((c1.r() as u32 * inv_t + c2.r() as u32 * t_plus) >> 8) as u8;
        let g = ((c1.g() as u32 * inv_t + c2.g() as u32 * t_plus) >> 8) as u8;
        let b = ((c1.b() as u32 * inv_t + c2.b() as u32 * t_plus) >> 8) as u8;

        Color::argb(a, r, g, b)
    }

    /// Fast alpha blend source color onto destination color using bit shifts instead of integer division.
    #[inline(always)]
    pub fn blend(src: Color, dst: Color) -> Color {
        let sa = src.a() as u32;
        if sa == 255 {
            return src;
        }
        if sa == 0 {
            return dst;
        }

        let inv_sa = 256 - sa;
        let sa_plus = sa + 1;
        let r = ((src.r() as u32 * sa_plus + dst.r() as u32 * inv_sa) >> 8) as u8;
        let g = ((src.g() as u32 * sa_plus + dst.g() as u32 * inv_sa) >> 8) as u8;
        let b = ((src.b() as u32 * sa_plus + dst.b() as u32 * inv_sa) >> 8) as u8;

        Color::argb(255, r, g, b)
    }
}
