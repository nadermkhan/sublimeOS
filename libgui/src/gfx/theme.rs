use core::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use crate::gfx::Color;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThemeKind {
    SublimeClassic = 0,
    SublimeSlate = 1,
}

impl ThemeKind {
    pub fn from_u8(v: u8) -> Self {
        match v % 2 {
            0 => ThemeKind::SublimeClassic,
            _ => ThemeKind::SublimeSlate,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ThemeKind::SublimeClassic => "Sublime Metro Light",
            ThemeKind::SublimeSlate => "Sublime Metro Dark",
        }
    }
}

static ACTIVE_THEME: AtomicU8 = AtomicU8::new(0);
static ACTIVE_SEED_RGB: AtomicU32 = AtomicU32::new(0);

pub fn current_theme_kind() -> ThemeKind {
    ThemeKind::from_u8(ACTIVE_THEME.load(Ordering::Relaxed))
}

pub fn set_theme_kind(kind: ThemeKind) {
    ACTIVE_THEME.store(kind as u8, Ordering::Relaxed);
}

pub fn set_wallpaper_seed(seed: Option<Color>) {
    let raw = match seed {
        Some(c) => 0xFF00_0000 | (c.to_u32() & 0x00FF_FFFF),
        None => 0,
    };
    ACTIVE_SEED_RGB.store(raw, Ordering::Relaxed);
}

pub fn set_wallpaper_seed_raw(raw: u32) {
    ACTIVE_SEED_RGB.store(raw, Ordering::Relaxed);
}

pub fn current_wallpaper_seed_raw() -> u32 {
    ACTIVE_SEED_RGB.load(Ordering::Relaxed)
}

pub fn current_wallpaper_seed() -> Option<Color> {
    let raw = ACTIVE_SEED_RGB.load(Ordering::Relaxed);
    if (raw >> 24) == 0 {
        None
    } else {
        Some(Color(raw | 0xFF00_0000))
    }
}

pub fn current_theme() -> Theme {
    let kind = current_theme_kind();
    let raw = ACTIVE_SEED_RGB.load(Ordering::Relaxed);
    if (raw >> 24) != 0 {
        Theme::from_wallpaper_seed(kind, Color(raw | 0xFF00_0000))
    } else {
        Theme::for_kind(kind)
    }
}

/// Perceived relative luminance (0..=255) for WCAG-style foreground contrast checks.
pub const fn luminance_255(c: Color) -> u8 {
    let r = (c.to_u32() >> 16) & 0xFF;
    let g = (c.to_u32() >> 8) & 0xFF;
    let b = c.to_u32() & 0xFF;
    ((r * 299 + g * 587 + b * 114) / 1000) as u8
}

/// Choose dark or crisp white foreground text depending on background luminance.
pub const fn contrast_text_on(bg: Color) -> Color {
    if luminance_255(bg) >= 165 {
        Color(0xFF141414)
    } else {
        Color(0xFFFFFFFF)
    }
}

/// Convert RGB to integer `(hue_0_360, sat_0_100, light_0_100)` for Material You tonal math.
pub fn rgb_to_hsl(c: Color) -> (i32, i32, i32) {
    let r = c.r() as i32;
    let g = c.g() as i32;
    let b = c.b() as i32;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let l = ((max + min) * 100 + 255) / 510;
    if d == 0 {
        return (210, 0, l);
    }
    let s = if l <= 50 {
        (d * 100) / (max + min).max(1)
    } else {
        (d * 100) / (510 - max - min).max(1)
    };
    let h_raw = if max == r {
        ((g - b) * 60) / d + if g < b { 360 } else { 0 }
    } else if max == g {
        ((b - r) * 60) / d + 120
    } else {
        ((r - g) * 60) / d + 240
    };
    ((h_raw % 360 + 360) % 360, s.clamp(0, 100), l.clamp(0, 100))
}

/// Convert integer `(hue_0_360, sat_0_100, light_0_100)` back into opaque `Color`.
pub fn hsl_to_rgb(h_deg: i32, s_pct: i32, l_pct: i32) -> Color {
    let h = ((h_deg % 360) + 360) % 360;
    let s = s_pct.clamp(0, 100);
    let l = l_pct.clamp(0, 100);
    if s == 0 {
        let v = ((l * 255 + 50) / 100).clamp(0, 255) as u8;
        return Color::rgb(v, v, v);
    }
    let c_255 = ((100 - (2 * l - 100).abs()) * s * 255) / 10000;
    let sector_pos = (h * 100) / 60; // 0..600 scaled by 100
    let mod2 = sector_pos % 200;
    let x_255 = (c_255 * (100 - (mod2 - 100).abs())) / 100;
    let m_255 = ((l * 255) / 100) - (c_255 / 2);

    let (r1, g1, b1) = match h {
        0..=59 => (c_255, x_255, 0),
        60..=119 => (x_255, c_255, 0),
        120..=179 => (0, c_255, x_255),
        180..=239 => (0, x_255, c_255),
        240..=299 => (x_255, 0, c_255),
        _ => (c_255, 0, x_255),
    };
    let r = (r1 + m_255).clamp(0, 255) as u8;
    let g = (g1 + m_255).clamp(0, 255) as u8;
    let b = (b1 + m_255).clamp(0, 255) as u8;
    Color::rgb(r, g, b)
}

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub kind: ThemeKind,
    pub sky_top: Color,
    pub sky_horizon: Color,
    pub hill_far: Color,
    pub hill_light: Color,
    pub hill_mid: Color,
    pub hill_dark: Color,

    pub frame_outer: Color,
    pub frame_active: Color,
    pub frame_inactive: Color,
    pub title_highlight: Color,
    pub title_top: Color,
    pub title_mid: Color,
    pub title_bot: Color,
    pub title_inactive_top: Color,
    pub title_inactive_bot: Color,

    pub face: Color,
    pub face_light: Color,
    pub face_dark: Color,
    pub input_bg: Color,
    pub input_border: Color,
    pub ctrl_border: Color,
    pub border_shadow: Color,

    pub selection: Color,
    pub selection_text: Color,
    pub accent: Color,

    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_link: Color,
    pub good: Color,
    pub warn: Color,
    pub danger: Color,

    pub taskbar_hilite: Color,
    pub taskbar_top: Color,
    pub taskbar_bot: Color,
    pub start_top: Color,
    pub start_mid: Color,
    pub start_bot: Color,
    pub tray_top: Color,
    pub tray_bot: Color,
}

impl Theme {
    pub const fn for_kind(kind: ThemeKind) -> Self {
        match kind {
            ThemeKind::SublimeClassic => Self::sublime_classic(),
            ThemeKind::SublimeSlate => Self::sublime_slate(),
        }
    }

    /// Generate a Windows 8.1 colorized + Material You dynamic tonal palette from a wallpaper seed color.
    pub fn from_wallpaper_seed(kind: ThemeKind, seed: Color) -> Self {
        let mut base = Self::for_kind(kind);
        let (h, s, _l) = rgb_to_hsl(seed);
        let is_dark = matches!(kind, ThemeKind::SublimeSlate);
        let s_prim = if s < 12 { 18 } else { s.clamp(46, 86) };
        let tonal_low = if s < 12 { 6 } else { (s / 5).clamp(6, 16) };
        let tonal_mid = if s < 12 { 10 } else { (s / 3).clamp(12, 26) };

        let win_accent = hsl_to_rgb(h, s_prim, if is_dark { 37 } else { 42 });
        let win_border = hsl_to_rgb(h, s_prim, if is_dark { 31 } else { 36 });
        let sel_col = hsl_to_rgb(h, (s_prim + 4).min(90), if is_dark { 44 } else { 42 });
        let sel_text = contrast_text_on(sel_col);

        base.frame_outer = win_border;
        base.frame_active = win_accent;
        base.title_highlight = hsl_to_rgb(h, s_prim, 52);
        base.title_top = win_accent;
        base.title_mid = win_accent;
        base.title_bot = win_accent;
        base.selection = sel_col;
        base.selection_text = sel_text;
        base.accent = sel_col;

        if is_dark {
            base.face = hsl_to_rgb(h, tonal_low, 13);
            base.face_light = hsl_to_rgb(h, tonal_mid, 18);
            base.face_dark = hsl_to_rgb(h, tonal_low, 9);
            base.input_bg = hsl_to_rgb(h, (tonal_low / 2).max(4), 8);
            base.input_border = hsl_to_rgb(h, tonal_mid, 30);
            base.ctrl_border = hsl_to_rgb(h, tonal_mid, 35);
            base.border_shadow = hsl_to_rgb(h, tonal_low, 8);
            base.title_inactive_top = hsl_to_rgb(h, tonal_low, 20);
            base.title_inactive_bot = hsl_to_rgb(h, tonal_low, 20);
            base.frame_inactive = hsl_to_rgb(h, tonal_low, 26);
            base.text_link = hsl_to_rgb(h, (s_prim + 12).min(92), 68);
        } else {
            base.face = hsl_to_rgb(h, tonal_low, 94);
            base.face_light = hsl_to_rgb(h, (tonal_low / 2).max(2), 99);
            base.face_dark = hsl_to_rgb(h, tonal_mid, 88);
            base.input_bg = Color(0xFFFFFFFF);
            base.input_border = hsl_to_rgb(h, tonal_mid, 68);
            base.ctrl_border = hsl_to_rgb(h, tonal_mid, 66);
            base.border_shadow = hsl_to_rgb(h, tonal_mid, 81);
            base.title_inactive_top = hsl_to_rgb(h, tonal_low, 90);
            base.title_inactive_bot = hsl_to_rgb(h, tonal_low, 90);
            base.frame_inactive = hsl_to_rgb(h, tonal_low, 76);
            base.text_link = hsl_to_rgb(h, (s_prim + 8).min(88), 34);
        }

        // Harmonized Windows 8.1 / Material You Taskbar & Start Menu tonal accent
        base.taskbar_top = hsl_to_rgb(h, (tonal_mid + 10).clamp(12, 46), if is_dark { 9 } else { 13 });
        base.taskbar_bot = hsl_to_rgb(h, (tonal_mid + 10).clamp(12, 46), if is_dark { 7 } else { 10 });
        base.taskbar_hilite = hsl_to_rgb(h, (tonal_mid + 14).clamp(16, 56), if is_dark { 24 } else { 29 });
        base.tray_top = hsl_to_rgb(h, (tonal_mid + 12).clamp(14, 48), if is_dark { 12 } else { 16 });
        base.tray_bot = hsl_to_rgb(h, (tonal_mid + 10).clamp(12, 46), if is_dark { 9 } else { 12 });
        base.start_top = hsl_to_rgb(h, s_prim, 42);
        base.start_mid = hsl_to_rgb(h, s_prim, 35);
        base.start_bot = hsl_to_rgb(h, s_prim, 27);
        base
    }

    pub const fn sublime_classic() -> Self {
        Self {
            kind: ThemeKind::SublimeClassic,
            sky_top: Color(0xFF0A326E),
            sky_horizon: Color(0xFF1474D4),
            hill_far: Color(0xFF1B9AEF),
            hill_light: Color(0xFF39C6FF),
            hill_mid: Color(0xFF0D4BA3),
            hill_dark: Color(0xFF061F4E),

            frame_outer: Color(0xFF0067B8),
            frame_active: Color(0xFF0078D7),
            frame_inactive: Color(0xFFC2C6CC),
            title_highlight: Color(0xFF0078D7),
            title_top: Color(0xFF0078D7),
            title_mid: Color(0xFF0078D7),
            title_bot: Color(0xFF0078D7),
            title_inactive_top: Color(0xFFEBEBEB),
            title_inactive_bot: Color(0xFFEBEBEB),

            face: Color(0xFFF0F0F0),
            face_light: Color(0xFFFFFFFF),
            face_dark: Color(0xFFE1E1E1),
            input_bg: Color(0xFFFFFFFF),
            input_border: Color(0xFFABADB3),
            ctrl_border: Color(0xFFADADAD),
            border_shadow: Color(0xFFD0D0D0),

            selection: Color(0xFF0078D7),
            selection_text: Color(0xFFFFFFFF),
            accent: Color(0xFF0078D7),

            text_primary: Color(0xFF141414),
            text_secondary: Color(0xFF5C5C5C),
            text_link: Color(0xFF0066CC),
            good: Color(0xFF107C10),
            warn: Color(0xFFD83B01),
            danger: Color(0xFFC75050),

            taskbar_hilite: Color(0xFF325072),
            taskbar_top: Color(0xFF101D2E),
            taskbar_bot: Color(0xFF0C1624),
            start_top: Color(0xFF0078D7),
            start_mid: Color(0xFF0067B8),
            start_bot: Color(0xFF005A9E),
            tray_top: Color(0xFF142336),
            tray_bot: Color(0xFF0E1928),
        }
    }

    pub const fn sublime_slate() -> Self {
        Self {
            kind: ThemeKind::SublimeSlate,
            sky_top: Color(0xFF130B2E),
            sky_horizon: Color(0xFF006378),
            hill_far: Color(0xFF008299),
            hill_light: Color(0xFF00B7C3),
            hill_mid: Color(0xFF281856),
            hill_dark: Color(0xFF0B0E18),

            frame_outer: Color(0xFF008299),
            frame_active: Color(0xFF008299),
            frame_inactive: Color(0xFF333337),
            title_highlight: Color(0xFF008299),
            title_top: Color(0xFF008299),
            title_mid: Color(0xFF008299),
            title_bot: Color(0xFF008299),
            title_inactive_top: Color(0xFF2D2D30),
            title_inactive_bot: Color(0xFF2D2D30),

            face: Color(0xFF1F1F23),
            face_light: Color(0xFF2B2B30),
            face_dark: Color(0xFF161619),
            input_bg: Color(0xFF121215),
            input_border: Color(0xFF46464C),
            ctrl_border: Color(0xFF55555C),
            border_shadow: Color(0xFF111114),

            selection: Color(0xFF0099BC),
            selection_text: Color(0xFFFFFFFF),
            accent: Color(0xFF0099BC),

            text_primary: Color(0xFFF3F3F3),
            text_secondary: Color(0xFFA6A6AC),
            text_link: Color(0xFF3CC5E8),
            good: Color(0xFF38C964),
            warn: Color(0xFFFFB900),
            danger: Color(0xFFE04343),

            taskbar_hilite: Color(0xFF36363C),
            taskbar_top: Color(0xFF141418),
            taskbar_bot: Color(0xFF101014),
            start_top: Color(0xFF0099BC),
            start_mid: Color(0xFF008299),
            start_bot: Color(0xFF006B80),
            tray_top: Color(0xFF18181D),
            tray_bot: Color(0xFF121216),
        }
    }
}
