#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use libgui::app::{WindowClient, WindowSpec};
use libgui::codecs::{decode_audio, inspect_audio_metadata, is_audio_path, AudioMetadata, DecodedAudio};
use libgui::component::ComponentContext;
use libgui::event::{EventResult, GuiEvent, KeyCode, MouseButton};
use libgui::font::{with_default_font, TextStyle};
use libgui::gfx::{contrast_text_on, Color, Rect, Surface, Theme};
use libgui::widget::{ScrollBar, ScrollBarState, ScrollOrientation};
use sublime_rt::{
    args, close, getpid, open, println, read, read_bytes, read_dir, write, O_RDONLY, O_WRONLY,
    SublimeWmOs,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum RepeatMode {
    Off,
    All,
    One,
}

impl RepeatMode {
    fn next(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Off => "Rep:Off",
            Self::All => "Rep:All",
            Self::One => "Rep:1",
        }
    }
}

#[derive(Clone)]
struct TrackEntry {
    path: String,
    meta: AudioMetadata,
}

struct AudioCtlState {
    vol: u8,
    queued: usize,
    active: bool,
    muted: bool,
}

struct MusicPlayerApp {
    ctx: ComponentContext,
    tracks: Vec<TrackEntry>,
    selected_idx: usize,
    scroll_idx: usize,
    list_vbar: ScrollBarState,
    current_path: String,
    current_meta: Option<AudioMetadata>,
    decoded: Option<DecodedAudio>,
    playing: bool,
    play_head_frame: usize,
    sent_frame: usize,
    last_tick_ms: u64,
    last_feed_ms: u64,
    last_ctl_poll_ms: u64,
    volume: u8,
    muted: bool,
    shuffle: bool,
    shuffle_rng: u32,
    repeat: RepeatMode,
    bars: [u8; 16],
    dragging_seek: bool,
    dragging_vol: bool,
    status: String,
    dirty_all: bool,
}

impl MusicPlayerApp {
    fn new(initial_path: Option<&str>) -> Self {
        let mut app = Self {
            ctx: ComponentContext::new(),
            tracks: Vec::new(),
            selected_idx: 0,
            scroll_idx: 0,
            list_vbar: ScrollBarState::new(0, 0, 10),
            current_path: String::new(),
            current_meta: None,
            decoded: None,
            playing: false,
            play_head_frame: 0,
            sent_frame: 0,
            last_tick_ms: 0,
            last_feed_ms: 0,
            last_ctl_poll_ms: 0,
            volume: 80,
            muted: false,
            shuffle: false,
            shuffle_rng: 0x5A17_9C3Du32 ^ (getpid() as u32),
            repeat: RepeatMode::All,
            bars: [6; 16],
            dragging_seek: false,
            dragging_vol: false,
            status: String::from("Ready — Select a track to play"),
            dirty_all: true,
        };
        app.sync_ctl_status();
        app.scan_library();
        if let Some(p) = initial_path {
            app.load_track(p, true);
        } else if !app.tracks.is_empty() {
            let first = app.tracks[0].path.clone();
            app.load_track(&first, false);
        }
        app
    }

    fn audioctl(&self, cmd: &str) {
        let fd = open("/dev/audioctl", O_WRONLY, 0);
        if fd >= 0 {
            let _ = write(fd as usize, cmd.as_bytes());
            let _ = close(fd as usize);
        }
    }

    fn read_ctl_status(&self) -> Option<AudioCtlState> {
        let fd = open("/dev/audioctl", O_RDONLY, 0);
        if fd < 0 {
            return None;
        }
        let mut buf = [0u8; 256];
        let n = read(fd as usize, &mut buf);
        let _ = close(fd as usize);
        if n <= 0 {
            return None;
        }
        let text = core::str::from_utf8(&buf[..n as usize]).ok()?;
        let mut st = AudioCtlState {
            vol: self.volume,
            queued: 0,
            active: false,
            muted: self.muted,
        };
        for tok in text.split_whitespace() {
            if let Some(v) = tok.strip_prefix("vol=") {
                st.vol = v.parse::<u8>().unwrap_or(st.vol).clamp(0, 100);
            } else if let Some(q) = tok.strip_prefix("queued=") {
                st.queued = q.parse::<usize>().unwrap_or(0);
            } else if let Some(a) = tok.strip_prefix("active=") {
                st.active = a == "1";
            } else if let Some(m) = tok.strip_prefix("muted=") {
                st.muted = m == "1";
            }
        }
        Some(st)
    }

    fn sync_ctl_status(&mut self) {
        if let Some(st) = self.read_ctl_status() {
            self.volume = st.vol;
            self.muted = st.muted;
        }
    }

    fn set_hw_volume(&mut self, vol: u8) {
        self.volume = vol.clamp(0, 100);
        self.audioctl(&format!("vol {}", self.volume));
    }

    fn toggle_mute(&mut self) {
        self.muted = !self.muted;
        self.audioctl(if self.muted { "mute" } else { "unmute" });
    }

    fn scan_library(&mut self) {
        self.tracks.clear();
        for dir in ["/Users/sublime/Music", "/Users/sublime/Downloads"] {
            if let Ok(entries) = read_dir(dir) {
                for e in entries {
                    if e.is_dir || !is_audio_path(&e.name) {
                        continue;
                    }
                    let path = format!("{}/{}", dir, e.name);
                    if let Ok(bytes) = read_bytes(&path, 8 * 1024 * 1024) {
                        let meta = inspect_audio_metadata(&path, &bytes);
                        self.tracks.push(TrackEntry { path, meta });
                    }
                }
            }
        }
        if self.selected_idx >= self.tracks.len() {
            self.selected_idx = 0;
        }
    }

    fn total_frames(&self) -> usize {
        self.decoded
            .as_ref()
            .map(|d| d.samples_44k_stereo.len() / 2)
            .unwrap_or(0)
    }

    fn feed_chunk(&mut self, max_frames: usize) {
        let Some(ref d) = self.decoded else { return };
        let total = d.samples_44k_stereo.len() / 2;
        if self.sent_frame >= total {
            return;
        }
        if let Some(st) = self.read_ctl_status() {
            if (st.queued / 2) > 22050 {
                return;
            }
        }
        let end_frame = (self.sent_frame + max_frames).min(total);
        let start_i = self.sent_frame * 2;
        let end_i = end_frame * 2;
        if end_i <= start_i {
            return;
        }
        let mut bytes = Vec::with_capacity((end_i - start_i) * 2);
        for &s in &d.samples_44k_stereo[start_i..end_i] {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        let fd = open("/dev/audio", O_WRONLY, 0);
        if fd >= 0 {
            let written = write(fd as usize, &bytes);
            let _ = close(fd as usize);
            if written > 0 {
                self.sent_frame += (written as usize) / 4;
            }
        }
    }

    fn stop_playback(&mut self) {
        self.playing = false;
        self.play_head_frame = 0;
        self.sent_frame = 0;
        self.audioctl("stop");
    }

    fn pause_playback(&mut self) {
        self.playing = false;
        self.audioctl("stop");
    }

    fn start_or_resume(&mut self) {
        let total = self.total_frames();
        if total == 0 {
            return;
        }
        if self.play_head_frame >= total {
            self.play_head_frame = 0;
        }
        self.sent_frame = self.play_head_frame;
        self.audioctl("clear");
        self.playing = true;
        self.last_feed_ms = 0;
        self.feed_chunk(44100 / 4);
        if let Some(ref m) = self.current_meta {
            self.status = format!("Playing: {} — {}", m.title, m.artist);
        }
    }

    fn load_track(&mut self, path: &str, autoplay: bool) {
        if self.playing {
            self.stop_playback();
        } else {
            self.play_head_frame = 0;
            self.sent_frame = 0;
        }
        self.current_path = String::from(path);
        if let Some(pos) = self.tracks.iter().position(|t| t.path == path) {
            self.selected_idx = pos;
        }
        let Ok(bytes) = read_bytes(path, 8 * 1024 * 1024) else {
            self.decoded = None;
            self.current_meta = None;
            self.status = format!("Failed to read {}", path);
            return;
        };
        let meta = inspect_audio_metadata(path, &bytes);
        if self.tracks.iter().all(|t| t.path != path) {
            self.tracks.insert(
                0,
                TrackEntry {
                    path: String::from(path),
                    meta: meta.clone(),
                },
            );
            self.selected_idx = 0;
        }
        match decode_audio(&bytes) {
            Ok(dec) => {
                let mut m = meta;
                if m.duration_ms == 0 {
                    m.duration_ms = dec.duration_ms;
                }
                if let Some(slot) = self.tracks.get_mut(self.selected_idx) {
                    slot.meta = m.clone();
                }
                self.status = format!("Loaded: {} ({})", m.title, m.format_name);
                self.current_meta = Some(m);
                self.decoded = Some(dec);
                self.play_head_frame = 0;
                self.sent_frame = 0;
                if autoplay {
                    self.start_or_resume();
                }
            }
            Err(e) => {
                self.decoded = None;
                self.current_meta = Some(meta);
                self.status = format!("Decode error: {}", e);
            }
        }
    }

    fn next_rng(&mut self) -> u32 {
        let mut x = self.shuffle_rng.max(1);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.shuffle_rng = x;
        x
    }

    fn advance_track(&mut self, forward: bool, autoplay: bool) {
        if self.tracks.is_empty() {
            return;
        }
        let len = self.tracks.len();
        let target_idx = if self.shuffle && len > 1 {
            let pick = (self.next_rng() as usize) % (len - 1);
            if pick >= self.selected_idx {
                pick + 1
            } else {
                pick
            }
        } else if forward {
            (self.selected_idx + 1) % len
        } else if self.selected_idx == 0 {
            len - 1
        } else {
            self.selected_idx - 1
        };
        let path = self.tracks[target_idx].path.clone();
        self.load_track(&path, autoplay);
    }

    fn seek_to_frame(&mut self, frame: usize) {
        let total = self.total_frames();
        if total == 0 {
            return;
        }
        self.play_head_frame = frame.min(total.saturating_sub(1));
        self.sent_frame = self.play_head_frame;
        if self.playing {
            self.audioctl("clear");
            self.feed_chunk(44100 / 4);
        }
    }

    fn update_visualizer(&mut self) {
        let Some(ref d) = self.decoded else {
            for b in self.bars.iter_mut() {
                *b = (*b * 3) / 4;
            }
            return;
        };
        if !self.playing || d.samples_44k_stereo.is_empty() {
            for b in self.bars.iter_mut() {
                *b = ((*b as u32 * 3) / 4).max(4) as u8;
            }
            return;
        }
        let pcm = &d.samples_44k_stereo;
        let anchor = (self.play_head_frame * 2).min(pcm.len().saturating_sub(2));
        for bi in 0..16usize {
            let mut peak: i32 = 0;
            let base = (anchor + bi * 64) % pcm.len();
            for step in 0..16 {
                let s = pcm[(base + step * 2) % pcm.len()] as i32;
                peak = peak.max(s.abs());
            }
            let level = ((peak * 100) / 32768).clamp(4, 100) as u8;
            let prev = self.bars[bi];
            self.bars[bi] = if level > prev {
                level
            } else {
                ((prev as u16 * 3 + level as u16) / 4) as u8
            };
        }
    }

    fn draw_cover_art(&self, surf: &mut Surface, rect: Rect, theme: &Theme) {
        let seed_str = self
            .current_meta
            .as_ref()
            .map(|m| m.title.as_str())
            .unwrap_or("SublimeOS");
        let mut hash = 2166136261u32;
        for b in seed_str.bytes() {
            hash ^= b as u32;
            hash = hash.wrapping_mul(16777619);
        }
        let c1 = Color::rgb(
            (40 + (hash & 0x5F)) as u8,
            (70 + ((hash >> 8) & 0x6F)) as u8,
            (130 + ((hash >> 16) & 0x6F)) as u8,
        );
        let c2 = theme.accent;
        surf.fill_rect(rect, c1);
        surf.fill_rect(
            Rect::new(rect.x + 10, rect.y + 10, rect.w - 20, rect.h - 20),
            Color::rgb(18, 22, 32),
        );
        surf.fill_rect(
            Rect::new(rect.x + rect.w / 2 - 14, rect.y + rect.h / 2 - 14, 28, 28),
            c2,
        );
        surf.fill_rect(
            Rect::new(rect.x + rect.w / 2 - 5, rect.y + rect.h / 2 - 5, 10, 10),
            Color::rgb(245, 248, 255),
        );
        surf.draw_rect(rect, theme.ctrl_border);
    }
}

fn format_mmss(total_sec: u64) -> String {
    format!("{:02}:{:02}", total_sec / 60, total_sec % 60)
}

impl WindowClient for MusicPlayerApp {
    fn spec(&self) -> WindowSpec {
        WindowSpec {
            title: String::from("Music Player"),
            width: 820,
            height: 540,
            min_width: 620,
            min_height: 400,
            resizable: true,
        }
    }

    fn on_theme_changed(&mut self, _theme: libgui::gfx::ThemeKind) {
        self.dirty_all = true;
    }

    fn on_event(&mut self, event: GuiEvent, bounds: Rect) -> EventResult {
        let side_w = 240i32;
        let hdr_h = 36i32;
        let ctrl_h = 84i32;
        let status_h = 22i32;
        let deck_x = bounds.x + side_w;
        let deck_w = (bounds.w - side_w).max(200);
        let ctrl_y = bounds.y + bounds.h - ctrl_h - status_h;
        let seek_rect = Rect::new(deck_x + 16, ctrl_y + 10, (deck_w - 32).max(80), 10);
        let btn_y = ctrl_y + 38;
        let vol_rect = Rect::new(bounds.x + bounds.w - 134, btn_y + 8, 80, 12);

        match event {
            GuiEvent::Tick { now_ms } => {
                let dt_ms = if self.last_tick_ms == 0 {
                    16
                } else {
                    now_ms.saturating_sub(self.last_tick_ms).min(200)
                };
                self.last_tick_ms = now_ms;

                if !self.dragging_vol && now_ms.saturating_sub(self.last_ctl_poll_ms) >= 250 {
                    self.last_ctl_poll_ms = now_ms;
                    self.sync_ctl_status();
                }

                if self.playing {
                    let total = self.total_frames();
                    let step = ((dt_ms * 44100) / 1000) as usize;
                    self.play_head_frame = (self.play_head_frame + step).min(self.sent_frame.max(self.play_head_frame));

                    if now_ms.saturating_sub(self.last_feed_ms) >= 75 {
                        self.last_feed_ms = now_ms;
                        self.feed_chunk(44100 / 5);
                    }

                    if total > 0 && self.play_head_frame >= total && self.sent_frame >= total {
                        let drained = self
                            .read_ctl_status()
                            .map(|s| s.queued == 0 && !s.active)
                            .unwrap_or(true);
                        if drained {
                            match self.repeat {
                                RepeatMode::One => {
                                    self.play_head_frame = 0;
                                    self.sent_frame = 0;
                                    self.start_or_resume();
                                }
                                RepeatMode::All => self.advance_track(true, true),
                                RepeatMode::Off => {
                                    if self.selected_idx + 1 < self.tracks.len() {
                                        self.advance_track(true, true);
                                    } else {
                                        self.stop_playback();
                                        self.status = String::from("Finished playlist");
                                    }
                                }
                            }
                        }
                    }
                    self.update_visualizer();
                    self.ctx.damage.invalidate(bounds);
                    return EventResult::NeedsRedraw;
                }
                EventResult::Ignored
            }
            GuiEvent::MouseWheel { x, y, delta_y } => {
                if x < deck_x && y >= bounds.y + hdr_h {
                    let vis = ((bounds.h - hdr_h - status_h) / 36).max(1) as usize;
                    let max_s = self.tracks.len().saturating_sub(vis);
                    if delta_y < 0 {
                        self.scroll_idx = (self.scroll_idx + 1).min(max_s);
                    } else if delta_y > 0 {
                        self.scroll_idx = self.scroll_idx.saturating_sub(1);
                    }
                    self.ctx.damage.invalidate(bounds);
                    return EventResult::NeedsRedraw;
                }
                EventResult::Ignored
            }
            GuiEvent::MouseMove { x, .. } => {
                if self.dragging_seek && self.total_frames() > 0 {
                    let rel = (x - seek_rect.x).clamp(0, seek_rect.w) as usize;
                    let frame = (rel * self.total_frames()) / seek_rect.w.max(1) as usize;
                    self.seek_to_frame(frame);
                    self.ctx.damage.invalidate(bounds);
                    return EventResult::NeedsRedraw;
                }
                if self.dragging_vol {
                    let rel = (x - vol_rect.x).clamp(0, vol_rect.w);
                    let nv = ((rel * 100) / vol_rect.w.max(1)) as u8;
                    self.set_hw_volume(nv);
                    self.ctx.damage.invalidate(bounds);
                    return EventResult::NeedsRedraw;
                }
                EventResult::Ignored
            }
            GuiEvent::MouseUp { .. } => {
                if self.dragging_seek || self.dragging_vol {
                    self.dragging_seek = false;
                    self.dragging_vol = false;
                    return EventResult::Handled;
                }
                EventResult::Ignored
            }
            GuiEvent::MouseDown { x, y, button: MouseButton::Left } => {
                let ref_rect = Rect::new(bounds.x + bounds.w - 86, bounds.y + 6, 74, 24);
                if ref_rect.contains(x, y) {
                    self.scan_library();
                    self.status = format!("Library refreshed ({} tracks)", self.tracks.len());
                    self.ctx.damage.invalidate(bounds);
                    return EventResult::NeedsRedraw;
                }

                // Track list + ScrollBar
                if x < deck_x && y >= bounds.y + hdr_h && y < bounds.y + bounds.h - status_h {
                    let sb_r = Rect::new(bounds.x + side_w - 14, bounds.y + hdr_h, 14, (bounds.h - hdr_h - status_h).max(16));
                    let vis = (sb_r.h / 36).max(1) as usize;
                    let max_s = self.tracks.len().saturating_sub(vis);
                    self.list_vbar.sync_range(self.scroll_idx.min(max_s) as i32, max_s as i32, vis as i32);
                    if sb_r.contains(x, y) {
                        let (_, ch) = ScrollBar::handle_event(
                            &mut self.list_vbar,
                            sb_r,
                            ScrollOrientation::Vertical,
                            GuiEvent::MouseDown { x, y, button: MouseButton::Left },
                        );
                        if ch {
                            self.scroll_idx = (self.list_vbar.value.max(0) as usize).min(max_s);
                        }
                        self.ctx.damage.invalidate(bounds);
                        return EventResult::NeedsRedraw;
                    }
                    let row = ((y - (bounds.y + hdr_h + 4)) / 36).max(0) as usize + self.scroll_idx;
                    if row < self.tracks.len() {
                        let p = self.tracks[row].path.clone();
                        self.load_track(&p, true);
                        self.ctx.damage.invalidate(bounds);
                        return EventResult::NeedsRedraw;
                    }
                }

                // Seek scrubber
                if seek_rect.contains(x, y) && self.total_frames() > 0 {
                    self.dragging_seek = true;
                    let rel = (x - seek_rect.x).clamp(0, seek_rect.w) as usize;
                    let frame = (rel * self.total_frames()) / seek_rect.w.max(1) as usize;
                    self.seek_to_frame(frame);
                    self.ctx.damage.invalidate(bounds);
                    return EventResult::NeedsRedraw;
                }

                // Transport controls
                if y >= btn_y && y < btn_y + 28 {
                    let bx = deck_x + 16;
                    if x >= bx && x < bx + 46 {
                        self.advance_track(false, true);
                    } else if x >= bx + 50 && x < bx + 118 {
                        if self.playing {
                            self.pause_playback();
                        } else {
                            self.start_or_resume();
                        }
                    } else if x >= bx + 122 && x < bx + 168 {
                        self.stop_playback();
                    } else if x >= bx + 172 && x < bx + 218 {
                        self.advance_track(true, true);
                    } else if x >= bx + 224 && x < bx + 276 {
                        self.shuffle = !self.shuffle;
                    } else if x >= bx + 280 && x < bx + 344 {
                        self.repeat = self.repeat.next();
                    }
                    let mute_r = Rect::new(bounds.x + bounds.w - 184, btn_y, 44, 28);
                    if mute_r.contains(x, y) {
                        self.toggle_mute();
                    }
                    if vol_rect.contains(x, y) {
                        self.dragging_vol = true;
                        let rel = (x - vol_rect.x).clamp(0, vol_rect.w);
                        self.set_hw_volume(((rel * 100) / vol_rect.w.max(1)) as u8);
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
                    }
                    KeyCode::Left => {
                        let back = 44100 * 5;
                        self.seek_to_frame(self.play_head_frame.saturating_sub(back));
                    }
                    KeyCode::Right => {
                        self.seek_to_frame(self.play_head_frame.saturating_add(44100 * 5));
                    }
                    KeyCode::Up => {
                        self.selected_idx = self.selected_idx.saturating_sub(1);
                        if self.selected_idx < self.scroll_idx {
                            self.scroll_idx = self.selected_idx;
                        }
                    }
                    KeyCode::Down => {
                        if self.selected_idx + 1 < self.tracks.len() {
                            self.selected_idx += 1;
                        }
                    }
                    KeyCode::Enter => {
                        if let Some(t) = self.tracks.get(self.selected_idx) {
                            let p = t.path.clone();
                            self.load_track(&p, true);
                        }
                    }
                    KeyCode::Char('n') | KeyCode::Char('N') => self.advance_track(true, true),
                    KeyCode::Char('p') | KeyCode::Char('P') => self.advance_track(false, true),
                    KeyCode::Char('s') | KeyCode::Char('S') => self.shuffle = !self.shuffle,
                    KeyCode::Char('r') | KeyCode::Char('R') => self.repeat = self.repeat.next(),
                    KeyCode::Char('m') | KeyCode::Char('M') => self.toggle_mute(),
                    KeyCode::Char('+') | KeyCode::Char('=') => self.set_hw_volume(self.volume.saturating_add(5)),
                    KeyCode::Char('-') => self.set_hw_volume(self.volume.saturating_sub(5)),
                    _ => return EventResult::Ignored,
                }
                self.ctx.damage.invalidate(bounds);
                EventResult::NeedsRedraw
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
        let side_w = 240i32;
        let hdr_h = 36i32;
        let ctrl_h = 84i32;
        let status_h = 22i32;
        let deck_x = bounds.x + side_w;
        let deck_w = (bounds.w - side_w).max(200);
        let stage_h = (bounds.h - hdr_h - ctrl_h - status_h).max(120);
        let stage_rect = Rect::new(deck_x, bounds.y + hdr_h, deck_w, stage_h);
        let ctrl_y = bounds.y + bounds.h - ctrl_h - status_h;

        // Top header bar & sidebar & deck backdrop
        surf.fill_rect(Rect::new(bounds.x, bounds.y, bounds.w, hdr_h), theme.face_dark);
        surf.fill_rect(
            Rect::new(bounds.x, bounds.y + hdr_h, side_w, bounds.h - hdr_h - status_h),
            theme.face,
        );
        surf.fill_rect(stage_rect, Color::rgb(14, 18, 26));
        surf.fill_rect(Rect::new(deck_x, ctrl_y, deck_w, ctrl_h), theme.face);

        // Cover Art + 16-band spectrum bars
        let art_rect = Rect::new(stage_rect.x + 22, stage_rect.y + 22, 84, 84);
        self.draw_cover_art(surf, art_rect, theme);

        let vis_x = stage_rect.x + 22;
        let vis_w = (stage_rect.w - 44).max(120);
        let vis_base_y = stage_rect.y + stage_rect.h - 24;
        let vis_max_h = (stage_rect.h - 140).clamp(36, 160);
        let bar_slot = (vis_w / 16).max(6);
        for bi in 0..16i32 {
            let pct = self.bars[bi as usize] as i32;
            let bh = ((pct * vis_max_h) / 100).max(4);
            let col = if pct > 75 { Color::rgb(96, 220, 255) } else { theme.accent };
            surf.fill_rect(Rect::new(vis_x + bi * bar_slot + 2, vis_base_y - bh, bar_slot - 4, bh), col);
        }

        // Seek bar
        let seek_rect = Rect::new(deck_x + 16, ctrl_y + 10, (deck_w - 32).max(80), 10);
        surf.fill_rect(seek_rect, theme.face_dark);
        let total = self.total_frames().max(1);
        let fill_w = ((self.play_head_frame.min(total) * seek_rect.w as usize) / total) as i32;
        if fill_w > 0 {
            surf.fill_rect(Rect::new(seek_rect.x, seek_rect.y, fill_w, seek_rect.h), theme.accent);
        }

        // Text + Buttons
        with_default_font(|font| {
            let fg_hdr = contrast_text_on(theme.face_dark);
            font.draw_text(
                surf,
                bounds.x + 12,
                bounds.y + 10,
                &format!("Sublime Music  •  {} Tracks", self.tracks.len()),
                TextStyle::ui_sized(13, fg_hdr),
            );

            let ref_rect = Rect::new(bounds.x + bounds.w - 86, bounds.y + 6, 74, 24);
            surf.fill_rect(ref_rect, theme.face_light);
            surf.draw_rect(ref_rect, theme.ctrl_border);
            font.draw_text(
                surf,
                ref_rect.x + 14,
                ref_rect.y + 5,
                "Refresh",
                TextStyle::ui_sized(11, contrast_text_on(theme.face_light)),
            );

            // Tracklist rows + ScrollBar
            let mut py = bounds.y + hdr_h + 4;
            for (i, tr) in self.tracks.iter().enumerate().skip(self.scroll_idx) {
                if py + 36 > bounds.y + bounds.h - status_h {
                    break;
                }
                let row_r = Rect::new(bounds.x + 4, py, side_w - 20, 34);
                let sel = i == self.selected_idx;
                let bg = if sel { theme.accent } else { theme.face };
                surf.fill_rect(row_r, bg);
                let fg = contrast_text_on(bg);
                let sub = if sel { fg } else { Color::rgb(130, 140, 155) };
                font.draw_text(surf, row_r.x + 6, row_r.y + 3, &tr.meta.title, TextStyle::ui_sized(11, fg));
                let dur = format_mmss((tr.meta.duration_ms as u64) / 1000);
                font.draw_text(
                    surf,
                    row_r.x + 6,
                    row_r.y + 18,
                    &format!("{}  •  {}", tr.meta.artist, dur),
                    TextStyle::ui_sized(10, sub),
                );
                py += 36;
            }
            let sb_r = Rect::new(bounds.x + side_w - 14, bounds.y + hdr_h, 14, (bounds.h - hdr_h - status_h).max(16));
            let vis = (sb_r.h / 36).max(1) as usize;
            let max_s = self.tracks.len().saturating_sub(vis);
            self.list_vbar.sync_range(self.scroll_idx.min(max_s) as i32, max_s as i32, vis as i32);
            ScrollBar::draw(surf, sb_r, ScrollOrientation::Vertical, self.list_vbar, theme);

            // Metadata deck text
            if let Some(ref m) = self.current_meta {
                let tx = art_rect.x + art_rect.w + 16;
                font.draw_text(surf, tx, art_rect.y + 4, &m.title, TextStyle::ui_sized(16, Color::rgb(240, 245, 255)));
                font.draw_text(surf, tx, art_rect.y + 28, &format!("{} — {}", m.artist, m.album), TextStyle::ui_sized(12, Color::rgb(175, 190, 210)));
                font.draw_text(
                    surf,
                    tx,
                    art_rect.y + 50,
                    &format!("{}  |  {} Hz  |  {} ch", m.format_name, m.sample_rate, m.channels),
                    TextStyle::ui_sized(11, Color::rgb(130, 180, 225)),
                );
            } else {
                font.draw_text(surf, art_rect.x + art_rect.w + 16, art_rect.y + 24, "No Track Selected", TextStyle::ui_sized(15, Color::rgb(210, 220, 235)));
            }

            // Elapsed / Total timestamp
            let cur_s = (self.play_head_frame as u64) / 44100;
            let tot_s = (self.total_frames() as u64) / 44100;
            font.draw_text(
                surf,
                seek_rect.x,
                ctrl_y + 22,
                &format!("{} / {}", format_mmss(cur_s), format_mmss(tot_s)),
                TextStyle::ui_sized(10, contrast_text_on(theme.face)),
            );

            // Transport buttons
            let btn_y = ctrl_y + 38;
            let bx = deck_x + 16;
            let btns = [
                (bx, 46, "Prev", false),
                (bx + 50, 68, if self.playing { "Pause" } else { "Play" }, true),
                (bx + 122, 46, "Stop", false),
                (bx + 172, 46, "Next", false),
                (bx + 224, 52, "Shuf", self.shuffle),
                (bx + 280, 64, self.repeat.label(), self.repeat != RepeatMode::Off),
            ];
            for (x, w, label, active) in btns {
                let bg = if active { theme.accent } else { theme.face_light };
                surf.fill_rect(Rect::new(x, btn_y, w, 28), bg);
                surf.draw_rect(Rect::new(x, btn_y, w, 28), theme.ctrl_border);
                font.draw_text(surf, x + 8, btn_y + 7, label, TextStyle::ui_sized(11, contrast_text_on(bg)));
            }

            // Mute + Volume bar
            let mute_r = Rect::new(bounds.x + bounds.w - 184, btn_y, 44, 28);
            let mbg = if self.muted { Color::rgb(190, 60, 60) } else { theme.face_light };
            surf.fill_rect(mute_r, mbg);
            surf.draw_rect(mute_r, theme.ctrl_border);
            font.draw_text(
                surf,
                mute_r.x + 6,
                mute_r.y + 7,
                if self.muted { "Muted" } else { "Mute" },
                TextStyle::ui_sized(10, contrast_text_on(mbg)),
            );

            let vol_rect = Rect::new(bounds.x + bounds.w - 134, btn_y + 8, 80, 12);
            surf.fill_rect(vol_rect, theme.face_dark);
            let vfill = ((self.volume as i32 * vol_rect.w) / 100).clamp(0, vol_rect.w);
            if vfill > 0 {
                surf.fill_rect(Rect::new(vol_rect.x, vol_rect.y, vfill, vol_rect.h), theme.accent);
            }
            font.draw_text(
                surf,
                vol_rect.x + vol_rect.w + 6,
                btn_y + 7,
                &format!("{}%", self.volume),
                TextStyle::ui_sized(11, contrast_text_on(theme.face)),
            );

            // Status bar
            let sy = bounds.y + bounds.h - status_h;
            surf.fill_rect(Rect::new(bounds.x, sy, bounds.w, status_h), theme.face_dark);
            font.draw_text(surf, bounds.x + 8, sy + 4, &self.status, TextStyle::ui_sized(11, fg_hdr));
        });
    }

    fn on_unmount(&mut self) {
        if self.playing {
            self.stop_playback();
        }
    }
}

#[no_mangle]
pub fn main() -> i32 {
    let argv = args();
    let initial = argv.get(1).copied();
    let mut os = SublimeWmOs;
    let mut app = MusicPlayerApp::new(initial);
    match libgui::ipc::run_app(&mut app, &mut os) {
        Ok(()) => 0,
        Err(code) => {
            println!("[music pid={}] WM error: {}", getpid(), code);
            1
        }
    }
}
