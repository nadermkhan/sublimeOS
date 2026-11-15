#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use libgui::app::{WindowClient, WindowSpec};
use libgui::codecs::{
    decode_audio, decode_avi, decode_image, is_audio_path, is_image_path, is_video_path, AviMovie,
    DecodedAudio, DecodedImage,
};
use libgui::component::ComponentContext;
use libgui::event::{EventResult, GuiEvent, KeyCode, MouseButton};
use libgui::font::{with_default_font, TextStyle};
use libgui::gfx::{contrast_text_on, Color, Rect, Surface, Theme, ThemeKind};
use libgui::widget::{ScrollBar, ScrollBarState, ScrollOrientation};
use sublime_rt::{
    args, close, getpid, open, println, read, read_bytes, read_dir, write, write_file, O_RDONLY,
    O_WRONLY, SublimeWmOs,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum MediaKind {
    None,
    Audio,
    Video,
    Image,
}

#[derive(Clone)]
struct PlaylistEntry {
    name: String,
    path: String,
    tag: &'static str,
}

struct MediaPlayerApp {
    ctx: ComponentContext,
    playlist: Vec<PlaylistEntry>,
    selected_idx: usize,
    scroll_idx: usize,
    playlist_vbar: ScrollBarState,
    current_path: String,
    kind: MediaKind,
    audio: Option<DecodedAudio>,
    video: Option<AviMovie>,
    video_frame_idx: usize,
    cached_video_frame: Option<(usize, DecodedImage)>,
    still_image: Option<DecodedImage>,
    playing: bool,
    play_head_sample: usize, // stereo sample frame index (0 .. total_frames)
    audio_sent_sample: usize,
    last_tick_ms: u64,
    last_audio_feed_ms: u64,
    volume: u8,
    status: String,
    dirty_all: bool,
}

impl MediaPlayerApp {
    fn new(initial_path: Option<&str>) -> Self {
        let mut app = Self {
            ctx: ComponentContext::new(),
            playlist: Vec::new(),
            selected_idx: 0,
            scroll_idx: 0,
            playlist_vbar: ScrollBarState::new(0, 0, 12),
            current_path: String::new(),
            kind: MediaKind::None,
            audio: None,
            video: None,
            video_frame_idx: 0,
            cached_video_frame: None,
            still_image: None,
            playing: false,
            play_head_sample: 0,
            audio_sent_sample: 0,
            last_tick_ms: 0,
            last_audio_feed_ms: 0,
            volume: 85,
            status: String::from("Ready — Select a track, video, or image from the playlist"),
            dirty_all: true,
        };
        app.scan_media_library();
        app.set_hw_volume(app.volume);
        if let Some(p) = initial_path {
            app.load_media(p, true);
        } else if !app.playlist.is_empty() {
            let first = app.playlist[0].path.clone();
            app.load_media(&first, false);
        }
        app
    }

    fn scan_media_library(&mut self) {
        self.playlist.clear();
        let dirs = [
            "/Users/sublime/Videos",
            "/Users/sublime/Music",
            "/Users/sublime/Pictures",
        ];
        for dir in dirs {
            if let Ok(entries) = read_dir(dir) {
                for e in entries {
                    if e.is_dir {
                        continue;
                    }
                    let path = format!("{}/{}", dir, e.name);
                    let tag = if is_video_path(&e.name) {
                        "[VID]"
                    } else if is_audio_path(&e.name) {
                        "[AUD]"
                    } else if is_image_path(&e.name) {
                        "[IMG]"
                    } else {
                        continue;
                    };
                    self.playlist.push(PlaylistEntry {
                        name: e.name,
                        path,
                        tag,
                    });
                }
            }
        }
    }

    fn audioctl(&self, cmd: &str) {
        let fd = open("/dev/audioctl", O_WRONLY, 0);
        if fd >= 0 {
            let _ = write(fd as usize, cmd.as_bytes());
            let _ = close(fd as usize);
        }
    }

    fn set_hw_volume(&mut self, vol: u8) {
        self.volume = vol.clamp(0, 100);
        self.audioctl(&format!("vol {}", self.volume));
    }

    fn stop_playback(&mut self) {
        self.playing = false;
        self.play_head_sample = 0;
        self.audio_sent_sample = 0;
        self.video_frame_idx = 0;
        self.cached_video_frame = None;
        self.audioctl("stop");
    }

    fn pause_playback(&mut self) {
        self.playing = false;
        self.audioctl("stop");
    }

    fn start_or_resume(&mut self) {
        if self.kind == MediaKind::None || self.kind == MediaKind::Image {
            return;
        }
        let total = self.total_stereo_frames();
        if total > 0 && self.play_head_sample >= total {
            self.play_head_sample = 0;
            self.audio_sent_sample = 0;
            self.video_frame_idx = 0;
        } else {
            self.audio_sent_sample = self.play_head_sample;
        }
        self.audioctl("clear");
        self.playing = true;
        self.last_audio_feed_ms = 0;
        self.feed_audio_chunk(44100 / 4); // Prime 250ms immediately
    }

    fn total_stereo_frames(&self) -> usize {
        if let Some(ref a) = self.audio {
            a.samples_44k_stereo.len() / 2
        } else if let Some(ref v) = self.video {
            let from_audio = v.audio_samples_44k_stereo.len() / 2;
            let from_vid = ((v.duration_ms as u64) * 44100 / 1000) as usize;
            from_audio.max(from_vid).max(1)
        } else {
            0
        }
    }

    fn active_pcm_slice(&self) -> &[i16] {
        if let Some(ref a) = self.audio {
            &a.samples_44k_stereo
        } else if let Some(ref v) = self.video {
            &v.audio_samples_44k_stereo
        } else {
            &[]
        }
    }

    fn query_dev_audio_queued(&self) -> usize {
        let fd = open("/dev/audio", O_RDONLY, 0);
        if fd < 0 {
            return 0;
        }
        let mut buf = [0u8; 256];
        let n = read(fd as usize, &mut buf);
        let _ = close(fd as usize);
        if n <= 0 {
            return 0;
        }
        if let Ok(s) = core::str::from_utf8(&buf[..n as usize]) {
            for line in s.lines() {
                if let Some(val) = line.strip_prefix("queued_samples=") {
                    if let Ok(num) = val.trim().parse::<usize>() {
                        return num;
                    }
                }
            }
        }
        0
    }

    fn feed_audio_chunk(&mut self, max_frames: usize) {
        let pcm = self.active_pcm_slice();
        if pcm.is_empty() {
            return;
        }
        let total_frames = pcm.len() / 2;
        if self.audio_sent_sample >= total_frames {
            return;
        }
        // Avoid over-buffering past 0.5s inside kernel queue
        let hw_queued_frames = self.query_dev_audio_queued() / 2;
        if hw_queued_frames > 22050 {
            return;
        }
        let end_frame = (self.audio_sent_sample + max_frames).min(total_frames);
        let start_i = self.audio_sent_sample * 2;
        let end_i = end_frame * 2;
        let mut bytes = Vec::with_capacity((end_i - start_i) * 2);
        for &s in &pcm[start_i..end_i] {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        let fd = open("/dev/audio", O_WRONLY, 0);
        if fd >= 0 {
            let written = write(fd as usize, &bytes);
            let _ = close(fd as usize);
            if written > 0 {
                let frames_written = (written as usize) / 4;
                self.audio_sent_sample += frames_written;
            }
        }
    }

    fn load_media(&mut self, path: &str, autoplay: bool) {
        self.stop_playback();
        self.current_path = String::from(path);
        if let Some(idx) = self.playlist.iter().position(|p| p.path == path) {
            self.selected_idx = idx;
        } else {
            let name = String::from(path.rsplit('/').next().unwrap_or(path));
            let tag = if is_video_path(path) {
                "[VID]"
            } else if is_audio_path(path) {
                "[AUD]"
            } else {
                "[IMG]"
            };
            self.playlist.insert(0, PlaylistEntry { name, path: String::from(path), tag });
            self.selected_idx = 0;
        }

        self.audio = None;
        self.video = None;
        self.still_image = None;
        self.kind = MediaKind::None;

        let Ok(bytes) = read_bytes(path, 8 * 1024 * 1024) else {
            self.status = format!("Failed to read {}", path);
            self.dirty_all = true;
            return;
        };

        if is_video_path(path) {
            match decode_avi(&bytes) {
                Ok(avi) => {
                    let secs = avi.duration_ms / 1000;
                    let tenths = (avi.duration_ms % 1000) / 100;
                    self.status = format!(
                        "AVI Video {}x{} @ {} fps • {} frames • {}.{}s",
                        avi.width,
                        avi.height,
                        avi.fps,
                        avi.video_frames_raw.len(),
                        secs,
                        tenths
                    );
                    if let Ok(first) = avi.decode_frame(0) {
                        self.cached_video_frame = Some((0, first));
                    }
                    self.video = Some(avi);
                    self.kind = MediaKind::Video;
                    if autoplay {
                        self.start_or_resume();
                    }
                }
                Err(e) => {
                    self.status = format!("AVI decode error: {}", e);
                }
            }
        } else if is_audio_path(path) {
            match decode_audio(&bytes) {
                Ok(aud) => {
                    let secs = aud.duration_ms / 1000;
                    let tenths = (aud.duration_ms % 1000) / 100;
                    self.status = format!(
                        "{} • {} Hz • {} ch • {}.{}s",
                        aud.format_name, aud.sample_rate, aud.channels, secs, tenths
                    );
                    self.audio = Some(aud);
                    self.kind = MediaKind::Audio;
                    if autoplay {
                        self.start_or_resume();
                    }
                }
                Err(e) => {
                    self.status = format!("Audio decode error: {}", e);
                }
            }
        } else {
            match decode_image(&bytes) {
                Ok(img) => {
                    self.status = format!(
                        "{} • {}x{} px • {} KB",
                        img.format_name,
                        img.width,
                        img.height,
                        bytes.len() / 1024
                    );
                    self.still_image = Some(img);
                    self.kind = MediaKind::Image;
                }
                Err(e) => {
                    self.status = format!("Image decode error: {}", e);
                }
            }
        }
        self.dirty_all = true;
    }

    fn export_snapshot(&mut self) {
        if let Some((_, ref frame)) = self.cached_video_frame {
            if let Ok(png) = libgui::codecs::encode_png_argb(frame.width, frame.height, &frame.pixels) {
                let _ = write_file("/Users/sublime/Pictures/VideoFrame.png", &png);
                self.status = String::from("Saved frame -> /Users/sublime/Pictures/VideoFrame.png");
                self.scan_media_library();
            }
        }
    }

    fn blit_scaled_image(&self, surf: &mut Surface, area: Rect, img: &DecodedImage) {
        if img.width == 0 || img.height == 0 || area.w <= 4 || area.h <= 4 {
            return;
        }
        let max_w = (area.w - 16) as u32;
        let max_h = (area.h - 16) as u32;
        let (dst_w, dst_h) = if img.width * max_h > img.height * max_w {
            (max_w, ((img.height * max_w) / img.width).max(1))
        } else {
            (((img.width * max_h) / img.height).max(1), max_h)
        };
        let x0 = area.x + (area.w - dst_w as i32) / 2;
        let y0 = area.y + (area.h - dst_h as i32) / 2;
        for dy in 0..dst_h as i32 {
            let sy = ((dy as u64 * img.height as u64) / dst_h as u64) as usize;
            let row_off = sy * img.width as usize;
            for dx in 0..dst_w as i32 {
                let sx = ((dx as u64 * img.width as u64) / dst_w as u64) as usize;
                let argb = img.pixels[row_off + sx];
                let a = ((argb >> 24) & 0xFF) as u8;
                let r = ((argb >> 16) & 0xFF) as u8;
                let g = ((argb >> 8) & 0xFF) as u8;
                let b = (argb & 0xFF) as u8;
                if a == 255 {
                    surf.set_pixel(x0 + dx, y0 + dy, Color::rgb(r, g, b));
                } else if a > 0 {
                    surf.blend_pixel(x0 + dx, y0 + dy, Color::argb(a, r, g, b));
                }
            }
        }
    }
}

impl WindowClient for MediaPlayerApp {
    fn spec(&self) -> WindowSpec {
        WindowSpec {
            title: String::from("Sublime Media Player (Ring-3 App)"),
            width: 680,
            height: 440,
            min_width: 480,
            min_height: 320,
            resizable: true,
        }
    }

    fn on_mount(&mut self, _bounds: Rect) {
        self.dirty_all = true;
    }

    fn on_theme_changed(&mut self, _theme: ThemeKind) {
        self.dirty_all = true;
    }

    fn on_unmount(&mut self) {
        self.stop_playback();
    }

    fn on_event(&mut self, event: GuiEvent, bounds: Rect) -> EventResult {
        let side_w = 186i32;
        let ctrl_h = 62i32;
        let status_h = 20i32;
        let stage_rect = Rect::new(
            bounds.x + side_w,
            bounds.y + 34,
            (bounds.w - side_w).max(160),
            (bounds.h - 34 - ctrl_h - status_h).max(100),
        );
        let ctrl_y = bounds.y + bounds.h - ctrl_h - status_h;
        let seek_rect = Rect::new(bounds.x + side_w + 14, ctrl_y + 8, (stage_rect.w - 28).max(80), 12);

        match event {
            GuiEvent::Tick { now_ms } => {
                if self.last_tick_ms == 0 {
                    self.last_tick_ms = now_ms;
                }
                let dt_ms = now_ms.saturating_sub(self.last_tick_ms).min(200);
                self.last_tick_ms = now_ms;

                if self.playing {
                    let advance_samples = ((dt_ms as u64) * 44100 / 1000) as usize;
                    let total = self.total_stereo_frames();
                    self.play_head_sample = (self.play_head_sample + advance_samples).min(total);

                    if now_ms.saturating_sub(self.last_audio_feed_ms) >= 80 {
                        self.last_audio_feed_ms = now_ms;
                        self.feed_audio_chunk(44100 / 6);
                    }

                    if let Some(ref avi) = self.video {
                        let nframes = avi.video_frames_raw.len().max(1);
                        let ms_elapsed = (self.play_head_sample as u64 * 1000 / 44100) as u32;
                        let target_frame = ((ms_elapsed * avi.fps) / 1000) as usize % nframes;
                        if target_frame != self.video_frame_idx {
                            self.video_frame_idx = target_frame;
                            if let Ok(decoded) = avi.decode_frame(target_frame) {
                                self.cached_video_frame = Some((target_frame, decoded));
                            }
                        }
                    }

                    if total > 0 && self.play_head_sample >= total {
                        self.playing = false;
                        self.audioctl("stop");
                    }
                    self.ctx.damage.invalidate(bounds);
                    return EventResult::NeedsRedraw;
                }
                EventResult::Ignored
            }
            GuiEvent::KeyDown { key, .. } => {
                match key {
                    KeyCode::Char(' ') => {
                        if self.playing {
                            self.pause_playback();
                        } else {
                            self.start_or_resume();
                        }
                        self.ctx.damage.invalidate(bounds);
                        return EventResult::NeedsRedraw;
                    }
                    KeyCode::Char('+') | KeyCode::Char('=') => {
                        let nv = self.volume.saturating_add(10);
                        self.set_hw_volume(nv);
                        self.ctx.damage.invalidate(bounds);
                        return EventResult::NeedsRedraw;
                    }
                    KeyCode::Char('-') => {
                        let nv = self.volume.saturating_sub(10);
                        self.set_hw_volume(nv);
                        self.ctx.damage.invalidate(bounds);
                        return EventResult::NeedsRedraw;
                    }
                    _ => {}
                }
                EventResult::Ignored
            }
            GuiEvent::MouseWheel { x, delta_y, .. } => {
                if x < bounds.x + side_w {
                    let list_h = (bounds.h - 56).max(24);
                    let vis = (list_h / 24).max(1) as usize;
                    let max_s = self.playlist.len().saturating_sub(vis);
                    if delta_y > 0 {
                        self.scroll_idx = self.scroll_idx.saturating_sub(delta_y as usize * 2);
                    } else if delta_y < 0 {
                        self.scroll_idx = (self.scroll_idx + (-delta_y as usize) * 2).min(max_s);
                    }
                    self.playlist_vbar.sync_range(self.scroll_idx as i32, max_s as i32, vis as i32);
                    self.ctx.damage.invalidate(bounds);
                    return EventResult::NeedsRedraw;
                }
                EventResult::Ignored
            }
            GuiEvent::MouseMove { x, y } => {
                let sb_r = Rect::new(bounds.x + side_w - 14, bounds.y + 36, 14, (bounds.h - 56).max(16));
                let vis = (sb_r.h / 24).max(1) as usize;
                let max_s = self.playlist.len().saturating_sub(vis);
                self.playlist_vbar.sync_range(self.scroll_idx.min(max_s) as i32, max_s as i32, vis as i32);
                let (res, ch) = ScrollBar::handle_event(&mut self.playlist_vbar, sb_r, ScrollOrientation::Vertical, GuiEvent::MouseMove { x, y });
                if ch {
                    self.scroll_idx = (self.playlist_vbar.value.max(0) as usize).min(max_s);
                }
                if ch || res.needs_redraw() {
                    self.ctx.damage.invalidate(bounds);
                    EventResult::NeedsRedraw
                } else {
                    EventResult::Ignored
                }
            }
            GuiEvent::MouseUp { x, y, button: MouseButton::Left } => {
                let sb_r = Rect::new(bounds.x + side_w - 14, bounds.y + 36, 14, (bounds.h - 56).max(16));
                let (res, _) = ScrollBar::handle_event(&mut self.playlist_vbar, sb_r, ScrollOrientation::Vertical, GuiEvent::MouseUp { x, y, button: MouseButton::Left });
                if res.needs_redraw() {
                    self.ctx.damage.invalidate(bounds);
                    EventResult::NeedsRedraw
                } else {
                    EventResult::Ignored
                }
            }
            GuiEvent::MouseDown {
                x,
                y,
                button: MouseButton::Left,
            } => {
                // Top bar buttons (Refresh, Snapshot)
                if y >= bounds.y + 4 && y < bounds.y + 30 {
                    let ref_x = bounds.x + bounds.w - 154;
                    let snap_x = bounds.x + bounds.w - 80;
                    if x >= ref_x && x < ref_x + 68 {
                        self.scan_media_library();
                        self.status = format!("Scanned {} media item(s)", self.playlist.len());
                        self.ctx.damage.invalidate(bounds);
                        return EventResult::NeedsRedraw;
                    }
                    if x >= snap_x && x < snap_x + 72 {
                        self.export_snapshot();
                        self.ctx.damage.invalidate(bounds);
                        return EventResult::NeedsRedraw;
                    }
                }

                // Playlist scrollbar + click
                if x < bounds.x + side_w && y >= bounds.y + 36 && y < bounds.y + bounds.h - status_h {
                    let sb_r = Rect::new(bounds.x + side_w - 14, bounds.y + 36, 14, (bounds.h - 56).max(16));
                    let vis = (sb_r.h / 24).max(1) as usize;
                    let max_s = self.playlist.len().saturating_sub(vis);
                    self.playlist_vbar.sync_range(self.scroll_idx.min(max_s) as i32, max_s as i32, vis as i32);
                    if sb_r.contains(x, y) {
                        let (_, ch) = ScrollBar::handle_event(&mut self.playlist_vbar, sb_r, ScrollOrientation::Vertical, GuiEvent::MouseDown { x, y, button: MouseButton::Left });
                        if ch {
                            self.scroll_idx = (self.playlist_vbar.value.max(0) as usize).min(max_s);
                        }
                        self.ctx.damage.invalidate(bounds);
                        return EventResult::NeedsRedraw;
                    }
                    let row = ((y - (bounds.y + 38)) / 24) as usize + self.scroll_idx;
                    if row < self.playlist.len() {
                        let p = self.playlist[row].path.clone();
                        self.load_media(&p, true);
                        self.ctx.damage.invalidate(bounds);
                        return EventResult::NeedsRedraw;
                    }
                }

                // Seek bar click
                if seek_rect.contains(x, y) && self.total_stereo_frames() > 0 {
                    let rel = (x - seek_rect.x).clamp(0, seek_rect.w) as usize;
                    let total = self.total_stereo_frames();
                    self.play_head_sample = (rel * total) / seek_rect.w.max(1) as usize;
                    self.audio_sent_sample = self.play_head_sample;
                    if self.playing {
                        self.audioctl("clear");
                        self.feed_audio_chunk(44100 / 5);
                    }
                    self.ctx.damage.invalidate(bounds);
                    return EventResult::NeedsRedraw;
                }

                // Transport buttons
                let btn_y = ctrl_y + 26;
                if y >= btn_y && y < btn_y + 26 {
                    let bx = bounds.x + side_w + 14;
                    if x >= bx && x < bx + 54 {
                        // Prev
                        if !self.playlist.is_empty() {
                            let next = if self.selected_idx == 0 {
                                self.playlist.len() - 1
                            } else {
                                self.selected_idx - 1
                            };
                            let p = self.playlist[next].path.clone();
                            self.load_media(&p, true);
                        }
                        self.ctx.damage.invalidate(bounds);
                        return EventResult::NeedsRedraw;
                    }
                    if x >= bx + 58 && x < bx + 132 {
                        if self.playing {
                            self.pause_playback();
                        } else {
                            self.start_or_resume();
                        }
                        self.ctx.damage.invalidate(bounds);
                        return EventResult::NeedsRedraw;
                    }
                    if x >= bx + 136 && x < bx + 190 {
                        self.stop_playback();
                        self.ctx.damage.invalidate(bounds);
                        return EventResult::NeedsRedraw;
                    }
                    if x >= bx + 194 && x < bx + 248 {
                        if !self.playlist.is_empty() {
                            let next = (self.selected_idx + 1) % self.playlist.len();
                            let p = self.playlist[next].path.clone();
                            self.load_media(&p, true);
                        }
                        self.ctx.damage.invalidate(bounds);
                        return EventResult::NeedsRedraw;
                    }
                    // Volume - / +
                    let vx = bounds.x + bounds.w - 110;
                    if x >= vx && x < vx + 24 {
                        let nv = self.volume.saturating_sub(10);
                        self.set_hw_volume(nv);
                        self.ctx.damage.invalidate(bounds);
                        return EventResult::NeedsRedraw;
                    }
                    if x >= vx + 74 && x < vx + 98 {
                        let nv = self.volume.saturating_add(10);
                        self.set_hw_volume(nv);
                        self.ctx.damage.invalidate(bounds);
                        return EventResult::NeedsRedraw;
                    }
                }
                EventResult::Ignored
            }
            GuiEvent::Resize { .. } => {
                self.dirty_all = true;
                self.ctx.damage.invalidate(bounds);
                EventResult::NeedsRedraw
            }
            _ => EventResult::Ignored,
        }
    }

    fn take_damage(&mut self, bounds: Rect) -> Option<Rect> {
        if self.dirty_all {
            self.dirty_all = false;
            return Some(bounds);
        }
        self.ctx.damage.take_bounding(bounds)
    }

    fn draw(&mut self, surf: &mut Surface, bounds: Rect, theme: &Theme) {
        let side_w = 186i32;
        let ctrl_h = 62i32;
        let status_h = 20i32;

        // Top header bar
        surf.fill_rect(Rect::new(bounds.x, bounds.y, bounds.w, 34), theme.face_dark);
        // Playlist sidebar
        surf.fill_rect(
            Rect::new(bounds.x, bounds.y + 34, side_w, bounds.h - 34 - status_h),
            theme.face,
        );
        // Stage backdrop (deep studio obsidian)
        let stage_rect = Rect::new(
            bounds.x + side_w,
            bounds.y + 34,
            (bounds.w - side_w).max(160),
            (bounds.h - 34 - ctrl_h - status_h).max(100),
        );
        surf.fill_rect(stage_rect, Color::rgb(16, 20, 28));

        // Blit still image or video frame
        if let Some(ref img) = self.still_image {
            self.blit_scaled_image(surf, stage_rect, img);
        } else if let Some((_, ref frame)) = self.cached_video_frame {
            self.blit_scaled_image(surf, stage_rect, frame);
        } else if self.kind == MediaKind::Audio {
            // Animated audio frequency/amplitude bars
            let pcm = self.active_pcm_slice();
            let bar_cnt = 28i32;
            let bar_w = ((stage_rect.w - 48) / bar_cnt).clamp(6, 18);
            let total_bw = bar_cnt * bar_w;
            let bx0 = stage_rect.x + (stage_rect.w - total_bw) / 2;
            let base_y = stage_rect.y + stage_rect.h - 34;
            let max_bh = (stage_rect.h - 70).max(24);
            let anchor = (self.play_head_sample * 2).min(pcm.len().saturating_sub(2));
            for bi in 0..bar_cnt {
                let idx = (anchor + (bi as usize) * 48) % pcm.len().max(1);
                let amp = if pcm.is_empty() {
                    0
                } else {
                    (pcm[idx] as i32).abs()
                };
                let bh = if self.playing {
                    ((amp * max_bh) / 32768).clamp(8, max_bh)
                } else {
                    10 + ((bi * 7) % 18)
                };
                surf.fill_rect(
                    Rect::new(bx0 + bi * bar_w + 1, base_y - bh, bar_w - 3, bh),
                    theme.accent,
                );
            }
        }

        // Control deck
        let ctrl_y = bounds.y + bounds.h - ctrl_h - status_h;
        surf.fill_rect(
            Rect::new(bounds.x + side_w, ctrl_y, stage_rect.w, ctrl_h),
            theme.face,
        );
        // Seek track
        let seek_rect = Rect::new(bounds.x + side_w + 14, ctrl_y + 8, (stage_rect.w - 28).max(80), 8);
        surf.fill_rect(seek_rect, theme.face_dark);
        let total = self.total_stereo_frames().max(1);
        let fill_w = ((self.play_head_sample.min(total) * seek_rect.w as usize) / total) as i32;
        if fill_w > 0 {
            surf.fill_rect(Rect::new(seek_rect.x, seek_rect.y, fill_w, 8), theme.accent);
        }

        // Buttons & text via Noto Sans
        with_default_font(|font| {
            let fg_hdr = contrast_text_on(theme.face_dark);
            let title = self.current_path.rsplit('/').next().unwrap_or("Media Player");
            font.draw_text(
                surf,
                bounds.x + 12,
                bounds.y + 9,
                &format!("Sublime Media Player  |  {}", title),
                TextStyle::ui_sized(12, fg_hdr),
            );

            let ref_x = bounds.x + bounds.w - 154;
            let snap_x = bounds.x + bounds.w - 80;
            surf.fill_rect(Rect::new(ref_x, bounds.y + 5, 68, 24), theme.face_light);
            surf.draw_rect(Rect::new(ref_x, bounds.y + 5, 68, 24), theme.ctrl_border);
            font.draw_text(
                surf,
                ref_x + 12,
                bounds.y + 10,
                "Refresh",
                TextStyle::ui_sized(11, contrast_text_on(theme.face_light)),
            );

            surf.fill_rect(Rect::new(snap_x, bounds.y + 5, 72, 24), theme.accent);
            font.draw_text(
                surf,
                snap_x + 8,
                bounds.y + 10,
                "Snap PNG",
                TextStyle::ui_sized(11, contrast_text_on(theme.accent)),
            );

            // Playlist rows
            let mut py = bounds.y + 38;
            for (i, item) in self.playlist.iter().enumerate().skip(self.scroll_idx) {
                if py + 24 > bounds.y + bounds.h - status_h {
                    break;
                }
                let row_r = Rect::new(bounds.x + 4, py, side_w - 20, 22);
                let sel = i == self.selected_idx;
                let bg = if sel { theme.accent } else { theme.face };
                surf.fill_rect(row_r, bg);
                let fg = contrast_text_on(bg);
                font.draw_text(
                    surf,
                    row_r.x + 6,
                    row_r.y + 4,
                    &format!("{} {}", item.tag, item.name),
                    TextStyle::ui_sized(11, fg),
                );
                py += 24;
            }
            let sb_r = Rect::new(bounds.x + side_w - 14, bounds.y + 36, 14, (bounds.h - 56).max(16));
            let vis = (sb_r.h / 24).max(1) as usize;
            let max_s = self.playlist.len().saturating_sub(vis);
            self.playlist_vbar.sync_range(self.scroll_idx.min(max_s) as i32, max_s as i32, vis as i32);
            ScrollBar::draw(surf, sb_r, ScrollOrientation::Vertical, self.playlist_vbar, theme);

            // Stage subtitle
            if self.kind == MediaKind::Audio {
                font.draw_text(
                    surf,
                    stage_rect.x + 20,
                    stage_rect.y + 18,
                    &format!("Now Playing: {}", title),
                    TextStyle::ui_sized(14, Color::rgb(235, 242, 252)),
                );
            }

            // Transport controls
            let btn_y = ctrl_y + 26;
            let bx = bounds.x + side_w + 14;
            let btns = [
                (bx, 54, "Prev", false),
                (bx + 58, 74, if self.playing { "Pause" } else { "Play" }, true),
                (bx + 136, 54, "Stop", false),
                (bx + 194, 54, "Next", false),
            ];
            for (x, w, label, primary) in btns {
                let bg = if primary { theme.accent } else { theme.face_light };
                surf.fill_rect(Rect::new(x, btn_y, w, 26), bg);
                surf.draw_rect(Rect::new(x, btn_y, w, 26), theme.ctrl_border);
                font.draw_text(
                    surf,
                    x + 12,
                    btn_y + 6,
                    label,
                    TextStyle::ui_sized(11, contrast_text_on(bg)),
                );
            }

            // Time + Volume
            let cur_s = (self.play_head_sample as u64) / 44100;
            let tot_s = (self.total_stereo_frames() as u64) / 44100;
            font.draw_text(
                surf,
                bx + 258,
                btn_y + 6,
                &format!("{:02}:{:02} / {:02}:{:02}", cur_s / 60, cur_s % 60, tot_s / 60, tot_s % 60),
                TextStyle::ui_sized(11, contrast_text_on(theme.face)),
            );

            let vx = bounds.x + bounds.w - 110;
            surf.fill_rect(Rect::new(vx, btn_y, 24, 26), theme.face_light);
            surf.draw_rect(Rect::new(vx, btn_y, 24, 26), theme.ctrl_border);
            font.draw_text(surf, vx + 9, btn_y + 6, "-", TextStyle::ui_sized(12, contrast_text_on(theme.face_light)));

            font.draw_text(
                surf,
                vx + 28,
                btn_y + 6,
                &format!("{}%", self.volume),
                TextStyle::ui_sized(11, contrast_text_on(theme.face)),
            );

            surf.fill_rect(Rect::new(vx + 74, btn_y, 24, 26), theme.face_light);
            surf.draw_rect(Rect::new(vx + 74, btn_y, 24, 26), theme.ctrl_border);
            font.draw_text(surf, vx + 82, btn_y + 6, "+", TextStyle::ui_sized(12, contrast_text_on(theme.face_light)));

            // Status bar
            let sy = bounds.y + bounds.h - status_h;
            surf.fill_rect(Rect::new(bounds.x, sy, bounds.w, status_h), theme.face_dark);
            font.draw_text(
                surf,
                bounds.x + 8,
                sy + 3,
                &self.status,
                TextStyle::ui_sized(11, contrast_text_on(theme.face_dark)),
            );
        });
    }
}

#[no_mangle]
pub fn main() -> i32 {
    let argv = args();
    let initial = argv.get(1).copied();
    let mut os = SublimeWmOs;
    let mut app = MediaPlayerApp::new(initial);
    match libgui::ipc::run_app(&mut app, &mut os) {
        Ok(()) => 0,
        Err(code) => {
            println!("[player pid={}] WM error: {}", getpid(), code);
            1
        }
    }
}
