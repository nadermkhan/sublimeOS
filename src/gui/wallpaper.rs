use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use crate::fs::vfs::VFS;
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::sync::IrqSafeSpinlock;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum BuiltinWallpaper {
    Prism = 0,
    Midnight = 1,
    Emerald = 2,
    Amber = 3,
    Slate = 4,
}

impl BuiltinWallpaper {
    pub fn name(self) -> &'static str {
        match self {
            BuiltinWallpaper::Prism => "Windows 8.1 Prism",
            BuiltinWallpaper::Midnight => "Midnight Nebula",
            BuiltinWallpaper::Emerald => "Emerald Aurora",
            BuiltinWallpaper::Amber => "Amber Horizon",
            BuiltinWallpaper::Slate => "Metro Solid Slate",
        }
    }

    pub fn next(self) -> Self {
        match self {
            BuiltinWallpaper::Prism => BuiltinWallpaper::Midnight,
            BuiltinWallpaper::Midnight => BuiltinWallpaper::Emerald,
            BuiltinWallpaper::Emerald => BuiltinWallpaper::Amber,
            BuiltinWallpaper::Amber => BuiltinWallpaper::Slate,
            BuiltinWallpaper::Slate => BuiltinWallpaper::Prism,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum WallpaperFit {
    Fill = 0,
    Stretch = 1,
    Center = 2,
    Tile = 3,
}

impl WallpaperFit {
    pub fn name(self) -> &'static str {
        match self {
            WallpaperFit::Fill => "Fill",
            WallpaperFit::Stretch => "Stretch",
            WallpaperFit::Center => "Center",
            WallpaperFit::Tile => "Tile",
        }
    }

    pub fn next(self) -> Self {
        match self {
            WallpaperFit::Fill => WallpaperFit::Stretch,
            WallpaperFit::Stretch => WallpaperFit::Center,
            WallpaperFit::Center => WallpaperFit::Tile,
            WallpaperFit::Tile => WallpaperFit::Fill,
        }
    }
}

pub use libgui::codecs::DecodedImage;

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum WallpaperSource {
    Builtin(BuiltinWallpaper),
    ImageFile(String),
}

struct WallpaperState {
    source: WallpaperSource,
    fit: WallpaperFit,
    generation: u64,
    cached_image: Option<Arc<DecodedImage>>,
    dynamic_palette: bool,
    seed_color: libgui::gfx::Color,
}

static WALLPAPER_STATE: IrqSafeSpinlock<WallpaperState> = IrqSafeSpinlock::new(WallpaperState {
    source: WallpaperSource::Builtin(BuiltinWallpaper::Prism),
    fit: WallpaperFit::Fill,
    generation: 1,
    cached_image: None,
    dynamic_palette: true,
    seed_color: libgui::gfx::Color(0xFF0078D7),
});

/// Material You inspired quantized hue-chroma seed extraction from ARGB pixels.
pub fn extract_seed_from_pixels(pixels: &[u32]) -> libgui::gfx::Color {
    if pixels.is_empty() {
        return libgui::gfx::Color(0xFF0078D7);
    }
    const BINS: usize = 36;
    let mut weight_sum = [0u64; BINS];
    let mut r_sum = [0u64; BINS];
    let mut g_sum = [0u64; BINS];
    let mut b_sum = [0u64; BINS];

    let step = (pixels.len() / 2048).max(1);
    let mut idx = 0usize;
    while idx < pixels.len() {
        let px = pixels[idx];
        let r = ((px >> 16) & 0xFF) as u64;
        let g = ((px >> 8) & 0xFF) as u64;
        let b = (px & 0xFF) as u64;
        let (h, s, l) = libgui::gfx::rgb_to_hsl(libgui::gfx::Color(px | 0xFF00_0000));
        let bin = ((h / 10).clamp(0, 35)) as usize;
        let w = if l < 8 || l > 94 {
            1u64
        } else if s >= 22 {
            (s as u64) * 3 + if l >= 25 && l <= 75 { 24 } else { 4 }
        } else {
            (s as u64) + 2
        };
        weight_sum[bin] += w;
        r_sum[bin] += r * w;
        g_sum[bin] += g * w;
        b_sum[bin] += b * w;
        idx += step;
    }

    let mut best_bin = 0usize;
    let mut best_score = 0u64;
    for i in 0..BINS {
        let prev = (i + BINS - 1) % BINS;
        let next = (i + 1) % BINS;
        let score = weight_sum[i] * 2 + (weight_sum[prev] + weight_sum[next]) / 2;
        if score > best_score {
            best_score = score;
            best_bin = i;
        }
    }

    let tw = weight_sum[best_bin];
    if tw == 0 {
        return libgui::gfx::Color(0xFF0078D7);
    }
    let rr = (r_sum[best_bin] / tw).min(255) as u8;
    let gg = (g_sum[best_bin] / tw).min(255) as u8;
    let bb = (b_sum[best_bin] / tw).min(255) as u8;
    libgui::gfx::Color::rgb(rr, gg, bb)
}

fn builtin_seed_color(b: BuiltinWallpaper, theme: &libgui::gfx::Theme) -> libgui::gfx::Color {
    let mut sample = vec![0u32; 48 * 27];
    render_builtin_to_buffer(&mut sample, 48, 0, 0, 48, 27, b, theme);
    extract_seed_from_pixels(&sample)
}

fn sync_state_palette(st: &mut WallpaperState) {
    let base = libgui::gfx::Theme::for_kind(libgui::gfx::current_theme_kind());
    let seed = match (&st.source, &st.cached_image) {
        (WallpaperSource::ImageFile(_), Some(img)) => extract_seed_from_pixels(&img.pixels),
        (WallpaperSource::Builtin(b), _) => builtin_seed_color(*b, &base),
        _ => libgui::gfx::Color(0xFF0078D7),
    };
    st.seed_color = seed;
    if st.dynamic_palette {
        libgui::gfx::set_wallpaper_seed(Some(seed));
    } else {
        libgui::gfx::set_wallpaper_seed(None);
    }
}

pub fn wallpaper_seed_color() -> libgui::gfx::Color {
    WALLPAPER_STATE.lock().seed_color
}

pub fn is_dynamic_palette_enabled() -> bool {
    WALLPAPER_STATE.lock().dynamic_palette
}

pub fn toggle_dynamic_palette() -> bool {
    let mut st = WALLPAPER_STATE.lock();
    st.dynamic_palette = !st.dynamic_palette;
    st.generation = st.generation.wrapping_add(1);
    let enabled = st.dynamic_palette;
    sync_state_palette(&mut st);
    enabled
}

pub fn apply_current_wallpaper_palette() {
    let mut st = WALLPAPER_STATE.lock();
    sync_state_palette(&mut st);
}

pub fn wallpaper_palette_swatches() -> [Color; 5] {
    let theme = libgui::gfx::current_theme();
    let seed = wallpaper_seed_color();
    [
        Color::from_libgui(seed),
        Color::from_libgui(theme.accent),
        Color::from_libgui(theme.title_top),
        Color::from_libgui(theme.face_dark),
        Color::from_libgui(theme.taskbar_top),
    ]
}

pub fn wallpaper_generation() -> u64 {
    WALLPAPER_STATE.lock().generation
}

pub fn wallpaper_fit() -> WallpaperFit {
    WALLPAPER_STATE.lock().fit
}

pub fn wallpaper_label() -> String {
    let st = WALLPAPER_STATE.lock();
    match st.source {
        WallpaperSource::Builtin(b) => String::from(b.name()),
        WallpaperSource::ImageFile(ref p) => {
            let short = p.rsplit('/').next().unwrap_or(p);
            if let Some(ref img) = st.cached_image {
                format!("{} ({}x{})", short, img.width, img.height)
            } else {
                String::from(short)
            }
        }
    }
}

pub fn wallpaper_path_or_name() -> String {
    let st = WALLPAPER_STATE.lock();
    match st.source {
        WallpaperSource::Builtin(b) => String::from(b.name()),
        WallpaperSource::ImageFile(ref p) => p.clone(),
    }
}

pub fn set_builtin_wallpaper(kind: BuiltinWallpaper) {
    let mut st = WALLPAPER_STATE.lock();
    st.source = WallpaperSource::Builtin(kind);
    st.cached_image = None;
    st.generation = st.generation.wrapping_add(1);
    sync_state_palette(&mut st);
}

pub fn cycle_wallpaper() -> String {
    let mut st = WALLPAPER_STATE.lock();
    let next = match st.source {
        WallpaperSource::Builtin(b) => b.next(),
        WallpaperSource::ImageFile(_) => BuiltinWallpaper::Prism,
    };
    st.source = WallpaperSource::Builtin(next);
    st.cached_image = None;
    st.generation = st.generation.wrapping_add(1);
    sync_state_palette(&mut st);
    String::from(next.name())
}

pub fn cycle_wallpaper_fit() -> WallpaperFit {
    let mut st = WALLPAPER_STATE.lock();
    st.fit = st.fit.next();
    st.generation = st.generation.wrapping_add(1);
    st.fit
}

pub fn is_supported_image(path: &str) -> bool {
    libgui::codecs::is_image_path(path)
}

pub fn load_vfs_image(path: &str) -> Result<DecodedImage, &'static str> {
    let file = {
        let vfs = VFS.lock();
        vfs.open(path, 0)?
    };
    let size_hint = (file.stat().size as usize).min(16 * 1024 * 1024);
    let mut data = Vec::with_capacity(size_hint);
    let mut chunk = vec![0u8; 32768];
    loop {
        match file.read(&mut chunk)? {
            0 => break,
            n => {
                if data.len() + n > 16 * 1024 * 1024 {
                    return Err("Image file exceeds 16 MB limit");
                }
                data.extend_from_slice(&chunk[..n]);
            }
        }
    }
    decode_image_bytes(&data)
}

pub fn set_wallpaper_from_file(path: &str) -> Result<(u32, u32), &'static str> {
    let decoded = Arc::new(load_vfs_image(path)?);
    let dims = (decoded.width, decoded.height);
    let mut st = WALLPAPER_STATE.lock();
    st.source = WallpaperSource::ImageFile(String::from(path));
    st.cached_image = Some(decoded);
    st.generation = st.generation.wrapping_add(1);
    sync_state_palette(&mut st);
    Ok(dims)
}

pub fn inspect_image_header(data: &[u8]) -> Option<(u32, u32, &'static str)> {
    libgui::codecs::inspect_image(data)
}

pub fn decode_image_bytes(data: &[u8]) -> Result<DecodedImage, &'static str> {
    libgui::codecs::decode_image(data)
}

fn generate_argb_pattern(width: u32, height: u32, style: u8) -> Vec<u32> {
    let mut pixels = vec![0xFF00_0000u32; width as usize * height as usize];
    let w_safe = width.max(1) as i32;
    let h_safe = height.max(1) as i32;
    for py in 0..height as i32 {
        let ny = (py * 1000) / h_safe;
        for px in 0..width as i32 {
            let nx = (px * 1000) / w_safe;
            let (r, g, b): (u8, u8, u8) = match style {
                0 => {
                    let d = nx * 7 + ny * 10;
                    if d > 8600 && d < 12000 {
                        (38, 178, 245)
                    } else {
                        ((10 + nx / 50) as u8, (64 + ny / 16) as u8, (150 + ny / 14).min(230) as u8)
                    }
                }
                1 => {
                    let ribbon = (nx * 4 + ny * 9) % 1000;
                    if ribbon > 420 && ribbon < 740 {
                        (24, (170 + (nx / 18)).min(240) as u8, (155 + (ny / 18)).min(235) as u8)
                    } else {
                        ((8 + ny / 40) as u8, (54 + nx / 20) as u8, (76 + ny / 25) as u8)
                    }
                }
                _ => {
                    let stripe = ((nx / 80) + (ny / 80)) % 2;
                    let base_r = (140 + (ny * 95) / 1000).min(245) as u8;
                    let base_g = (42 + (nx * 90) / 1000).min(170) as u8;
                    let base_b = (68 + ((1000 - ny) * 80) / 1000).min(180) as u8;
                    if stripe == 0 {
                        (base_r, base_g, base_b)
                    } else {
                        (base_r.saturating_sub(14), base_g.saturating_sub(10), base_b.saturating_add(12))
                    }
                }
            };
            pixels[py as usize * width as usize + px as usize] =
                0xFF00_0000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
        }
    }
    pixels
}

pub fn generate_sample_png(width: u32, height: u32, style: u8) -> Vec<u8> {
    let pixels = generate_argb_pattern(width, height, style);
    libgui::codecs::encode_png_argb(width, height, &pixels).unwrap_or_default()
}

pub fn generate_sample_qoi(width: u32, height: u32, style: u8) -> Vec<u8> {
    let pixels = generate_argb_pattern(width, height, style);
    libgui::codecs::encode_qoi_argb(width, height, &pixels).unwrap_or_default()
}

pub fn generate_sample_jpeg(width: u32, height: u32, style: u8) -> Vec<u8> {
    let pixels = generate_argb_pattern(width, height, style);
    libgui::codecs::encode_baseline_jpeg(width, height, &pixels).unwrap_or_default()
}

pub fn generate_sample_wav() -> Vec<u8> {
    let pcm = crate::drivers::audio::pcm::generate_cosmic_chime(44100);
    let data_bytes = (pcm.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_bytes as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&2u16.to_le_bytes()); // stereo
    out.extend_from_slice(&44100u32.to_le_bytes());
    out.extend_from_slice(&(44100u32 * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_bytes.to_le_bytes());
    for s in pcm {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

pub fn generate_sample_qoa() -> Vec<u8> {
    let tone = crate::drivers::audio::pcm::generate_sine_wave(523, 600, 44100);
    // Extract left channel mono samples for compact QOA file
    let mono: Vec<i16> = tone.chunks_exact(2).map(|c| c[0]).collect();
    libgui::codecs::encode_qoa_mono(44100, &mono)
}

/// Generate a multi-note 22.05 kHz mono QOA synth arpeggio (~6s, ~55 KB).
pub fn generate_sample_qoa_arp() -> Vec<u8> {
    const SR: u32 = 22050;
    let notes = [261u32, 329, 392, 523, 392, 329, 293, 349, 440, 587, 440, 349];
    let mut mono = Vec::with_capacity((SR as usize * notes.len()) / 2);
    for &freq in &notes {
        let step_stereo = crate::drivers::audio::pcm::generate_sine_wave(freq, 500, SR);
        mono.extend(step_stereo.chunks_exact(2).map(|c| c[0]));
    }
    libgui::codecs::encode_qoa_mono(SR, &mono)
}

/// Generate a multi-chord 22.05 kHz mono IMA-ADPCM WAV track with RIFF INFO metadata (~7s, ~78 KB).
pub fn generate_sample_adpcm_track(
    title: &str,
    artist: &str,
    album: &str,
    freqs: &[u32; 4],
) -> Vec<u8> {
    const SR: u32 = 22050;
    let mut mono = Vec::with_capacity(SR as usize * 7);
    for (idx, &freq) in freqs.iter().enumerate() {
        let base = crate::drivers::audio::pcm::generate_sine_wave(freq, 1750, SR);
        let harm = crate::drivers::audio::pcm::generate_sine_wave(freq * 3 / 2, 1750, SR);
        let fade_len = base.len() / 2;
        for f in 0..fade_len {
            let b = base[f * 2] as i32;
            let h = harm[f * 2] as i32;
            let mix = if (idx & 1) == 0 {
                (b * 2 + h) / 3
            } else {
                (b * 3 + h) / 4
            };
            mono.push(mix.clamp(-32767, 32767) as i16);
        }
    }
    libgui::codecs::encode_ima_adpcm_mono(SR, &mono, title, artist, album)
}

pub fn generate_sample_avi() -> Vec<u8> {
    let w = 96u32;
    let h = 64u32;
    let frame_cnt = 12usize;
    let mut frames = Vec::with_capacity(frame_cnt);
    for f in 0..frame_cnt {
        let mut px = vec![0xFF0E_1726u32; (w * h) as usize];
        let cx = 12 + (f as i32 * 6);
        let cy = 16 + ((f as i32 % 4) * 5);
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                let bg_r = (14 + (x * 18) / w as i32) as u32;
                let bg_g = (28 + (y * 32) / h as i32) as u32;
                let bg_b = 58u32;
                let dx = x - cx;
                let dy = y - cy;
                let col = if dx * dx + dy * dy <= 144 {
                    0xFF00_B4FFu32
                } else {
                    0xFF00_0000 | (bg_r << 16) | (bg_g << 8) | bg_b
                };
                px[y as usize * w as usize + x as usize] = col;
            }
        }
        frames.push(px);
    }
    let audio = crate::drivers::audio::pcm::generate_sine_wave(440, 1200, 44100);
    libgui::codecs::build_mjpeg_avi(w, h, 10, &frames, &audio).unwrap_or_default()
}

/// Generate a valid uncompressed 24-bit bottom-up Windows BMP image for built-in sample pictures.
pub fn generate_sample_bmp(width: u32, height: u32, style: u8) -> Vec<u8> {
    let row_stride = ((width as usize * 3) + 3) & !3;
    let img_bytes = row_stride * height as usize;
    let file_size = 54 + img_bytes;
    let mut out = vec![0u8; file_size];

    out[0..2].copy_from_slice(b"BM");
    out[2..6].copy_from_slice(&(file_size as u32).to_le_bytes());
    out[10..14].copy_from_slice(&54u32.to_le_bytes());
    out[14..18].copy_from_slice(&40u32.to_le_bytes());
    out[18..22].copy_from_slice(&(width as i32).to_le_bytes());
    out[22..26].copy_from_slice(&(height as i32).to_le_bytes());
    out[26..28].copy_from_slice(&1u16.to_le_bytes());
    out[28..30].copy_from_slice(&24u16.to_le_bytes());
    out[34..38].copy_from_slice(&(img_bytes as u32).to_le_bytes());

    let w_safe = width.max(1) as i32;
    let h_safe = height.max(1) as i32;
    for py in 0..height as i32 {
        let bmp_row = (height as i32 - 1 - py) as usize;
        let row_off = 54 + bmp_row * row_stride;
        let ny = (py * 1000) / h_safe;
        for px in 0..width as i32 {
            let nx = (px * 1000) / w_safe;
            let (r, g, b): (u8, u8, u8) = match style {
                0 => {
                    // Sublime Prism Blue/Cyan
                    let d = nx * 7 + ny * 10;
                    if d > 8600 && d < 12000 {
                        (38, 178, 245)
                    } else {
                        ((10 + nx / 50) as u8, (64 + ny / 16) as u8, (150 + ny / 14).min(230) as u8)
                    }
                }
                1 => {
                    // Aurora Emerald/Teal Wave
                    let ribbon = (nx * 4 + ny * 9) % 1000;
                    if ribbon > 420 && ribbon < 740 {
                        (24, (170 + (nx / 18)).min(240) as u8, (155 + (ny / 18)).min(235) as u8)
                    } else {
                        ((8 + ny / 40) as u8, (54 + nx / 20) as u8, (76 + ny / 25) as u8)
                    }
                }
                _ => {
                    // Warm Sunset Horizon Grid
                    let stripe = ((nx / 80) + (ny / 80)) % 2;
                    let base_r = (140 + (ny * 95) / 1000).min(245) as u8;
                    let base_g = (42 + (nx * 90) / 1000).min(170) as u8;
                    let base_b = (68 + ((1000 - ny) * 80) / 1000).min(180) as u8;
                    if stripe == 0 {
                        (base_r, base_g, base_b)
                    } else {
                        (base_r.saturating_sub(14), base_g.saturating_sub(10), base_b.saturating_add(12))
                    }
                }
            };
            let off = row_off + px as usize * 3;
            out[off] = b;
            out[off + 1] = g;
            out[off + 2] = r;
        }
    }
    out
}

pub fn render_wallpaper_to_buffer(
    buf: &mut [u32],
    stride: usize,
    x0: i32,
    y0: i32,
    w: i32,
    work_h: i32,
    theme: &libgui::gfx::Theme,
) {
    let (source, fit, img_opt) = {
        let st = WALLPAPER_STATE.lock();
        (st.source.clone(), st.fit, st.cached_image.clone())
    };

    if let (WallpaperSource::ImageFile(_), Some(img)) = (&source, &img_opt) {
        render_image_wallpaper(buf, stride, x0, y0, w, work_h, img, fit);
        return;
    }

    let builtin = match source {
        WallpaperSource::Builtin(b) => b,
        WallpaperSource::ImageFile(_) => BuiltinWallpaper::Prism,
    };
    render_builtin_to_buffer(buf, stride, x0, y0, w, work_h, builtin, theme);
}

fn render_builtin_to_buffer(
    buf: &mut [u32],
    stride: usize,
    x0: i32,
    y0: i32,
    w: i32,
    work_h: i32,
    builtin: BuiltinWallpaper,
    theme: &libgui::gfx::Theme,
) {
    let (sky_top, sky_horizon, facet_a, facet_hi, facet_mid, facet_dark) = match builtin {
        BuiltinWallpaper::Prism => (
            Color::from_libgui(theme.sky_top),
            Color::from_libgui(theme.sky_horizon),
            Color::from_libgui(theme.hill_far),
            Color::from_libgui(theme.hill_light),
            Color::from_libgui(theme.hill_mid),
            Color::from_libgui(theme.hill_dark),
        ),
        BuiltinWallpaper::Midnight => (
            Color::rgb(14, 18, 46),
            Color::rgb(34, 24, 84),
            Color::rgb(78, 56, 158),
            Color::rgb(122, 92, 224),
            Color::rgb(46, 34, 114),
            Color::rgb(10, 12, 30),
        ),
        BuiltinWallpaper::Emerald => (
            Color::rgb(10, 44, 52),
            Color::rgb(16, 86, 88),
            Color::rgb(30, 156, 140),
            Color::rgb(82, 214, 186),
            Color::rgb(18, 108, 98),
            Color::rgb(8, 28, 36),
        ),
        BuiltinWallpaper::Amber => (
            Color::rgb(44, 22, 34),
            Color::rgb(112, 46, 48),
            Color::rgb(208, 106, 52),
            Color::rgb(245, 162, 74),
            Color::rgb(146, 62, 44),
            Color::rgb(26, 14, 22),
        ),
        BuiltinWallpaper::Slate => (
            Color::rgb(28, 36, 48),
            Color::rgb(40, 52, 68),
            Color::rgb(52, 72, 96),
            Color::rgb(68, 96, 126),
            Color::rgb(36, 48, 64),
            Color::rgb(20, 26, 36),
        ),
    };

    let w_safe = w.max(1);
    let h_safe = work_h.max(1);
    for py in 0..work_h {
        let ny = (py * 1000) / h_safe;
        let row_off = (y0 + py) as usize * stride + x0 as usize;
        for px in 0..w {
            let nx = (px * 1000) / w_safe;
            let base = Color::lerp(sky_top, sky_horizon, ((nx * 45 + ny * 55) / 400).min(255) as u32);
            let col = if builtin == BuiltinWallpaper::Slate {
                if nx + ny > 1450 {
                    Color::lerp(base, facet_a, 85)
                } else {
                    base
                }
            } else {
                let d1 = nx * 7 + ny * 10;
                let d2 = nx * 10 - ny * 6;
                if d1 > 9200 && d1 < 12600 && d2 > -1500 && d2 < 5200 {
                    let t = (((d1 - 9200) * 255) / 3400).min(255) as u32;
                    Color::lerp(facet_hi, facet_a, t)
                } else if d1 > 6400 && d1 < 9800 && d2 > 2200 {
                    let t = (((d1 - 6400) * 255) / 3400).min(255) as u32;
                    Color::lerp(facet_a, facet_mid, t)
                } else if nx * 4 + ny * 9 < 5600 {
                    Color::lerp(base, facet_dark, 110)
                } else if nx * 8 - ny * 5 > 4200 && d1 > 12200 {
                    Color::lerp(base, facet_dark, 145)
                } else {
                    base
                }
            };
            buf[row_off + px as usize] = col.to_u32();
        }
    }
}

fn render_image_wallpaper(
    buf: &mut [u32],
    stride: usize,
    x0: i32,
    y0: i32,
    w: i32,
    h: i32,
    img: &DecodedImage,
    fit: WallpaperFit,
) {
    let iw = img.width.max(1) as i64;
    let ih = img.height.max(1) as i64;
    let dw = w.max(1) as i64;
    let dh = h.max(1) as i64;

    match fit {
        WallpaperFit::Stretch => {
            for py in 0..h {
                let sy = ((py as i64 * ih) / dh).clamp(0, ih - 1) as usize;
                let src_row = sy * iw as usize;
                let dst_off = (y0 + py) as usize * stride + x0 as usize;
                for px in 0..w {
                    let sx = ((px as i64 * iw) / dw).clamp(0, iw - 1) as usize;
                    buf[dst_off + px as usize] = img.pixels[src_row + sx];
                }
            }
        }
        WallpaperFit::Fill => {
            // Preserve aspect ratio and cover target rectangle completely
            let (crop_w, crop_h) = if iw * dh >= ih * dw {
                ((ih * dw) / dh.max(1), ih)
            } else {
                (iw, (iw * dh) / dw.max(1))
            };
            let crop_w = crop_w.clamp(1, iw);
            let crop_h = crop_h.clamp(1, ih);
            let ox = (iw - crop_w) / 2;
            let oy = (ih - crop_h) / 2;
            for py in 0..h {
                let sy = (oy + (py as i64 * crop_h) / dh).clamp(0, ih - 1) as usize;
                let src_row = sy * iw as usize;
                let dst_off = (y0 + py) as usize * stride + x0 as usize;
                for px in 0..w {
                    let sx = (ox + (px as i64 * crop_w) / dw).clamp(0, iw - 1) as usize;
                    buf[dst_off + px as usize] = img.pixels[src_row + sx];
                }
            }
        }
        WallpaperFit::Center => {
            let bg = 0xFF10_1824u32;
            let ox = (dw - iw) / 2;
            let oy = (dh - ih) / 2;
            for py in 0..h {
                let sy = py as i64 - oy;
                let dst_off = (y0 + py) as usize * stride + x0 as usize;
                if sy < 0 || sy >= ih {
                    for px in 0..w {
                        buf[dst_off + px as usize] = bg;
                    }
                } else {
                    let src_row = sy as usize * iw as usize;
                    for px in 0..w {
                        let sx = px as i64 - ox;
                        buf[dst_off + px as usize] = if sx >= 0 && sx < iw {
                            img.pixels[src_row + sx as usize]
                        } else {
                            bg
                        };
                    }
                }
            }
        }
        WallpaperFit::Tile => {
            for py in 0..h {
                let sy = (py as i64 % ih) as usize;
                let src_row = sy * iw as usize;
                let dst_off = (y0 + py) as usize * stride + x0 as usize;
                for px in 0..w {
                    let sx = (px as i64 % iw) as usize;
                    buf[dst_off + px as usize] = img.pixels[src_row + sx];
                }
            }
        }
    }
}

pub fn draw_thumbnail(
    canvas: &mut Canvas,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    theme: &libgui::gfx::Theme,
) {
    if w <= 2 || h <= 2 {
        return;
    }
    let (cx0, cy0, cx1, cy1) = canvas.clip_bounds();
    let x0 = x.max(cx0).max(0);
    let y0 = y.max(cy0).max(0);
    let x1 = (x + w).min(cx1).min(canvas.width as i32);
    let y1 = (y + h).min(cy1).min(canvas.height as i32);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let rw = x1 - x0;
    let rh = y1 - y0;
    let stride = canvas.width as usize;
    render_wallpaper_to_buffer(&mut canvas.buffer, stride, x0, y0, rw, rh, theme);
    canvas.draw_rect(x, y, w, h, Color::from_libgui(theme.ctrl_border));
}

pub fn draw_decoded_image_fit(canvas: &mut Canvas, img: &DecodedImage, x: i32, y: i32, w: i32, h: i32) {
    let (cx0, cy0, cx1, cy1) = canvas.clip_bounds();
    let x0 = x.max(cx0).max(0);
    let y0 = y.max(cy0).max(0);
    let x1 = (x + w).min(cx1).min(canvas.width as i32);
    let y1 = (y + h).min(cy1).min(canvas.height as i32);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let rw = x1 - x0;
    let rh = y1 - y0;
    let stride = canvas.width as usize;
    render_image_wallpaper(&mut canvas.buffer, stride, x0, y0, rw, rh, img, WallpaperFit::Fill);
}
