use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::arch::x86_64::rtc::get_system_datetime;
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::desktop::{draw_sublime_logo, DesktopAppId};
use crate::gui::font::{
    draw_ui_string, draw_ui_string_bold, draw_ui_string_centered_v, draw_ui_string_ellipsis,
    draw_ui_string_sized, measure_ui_string,
};
use crate::gui::window::{app_tile_color, draw_app_badge_icon, draw_mini_window_icon, Window};
use crate::mm::pmm::PMM;
use crate::net::device::NET_MANAGER;
use crate::net::wifi::WIFI_MANAGER;
use libgui::event::{GuiEvent, KeyCode};
use libgui::gfx::{Rect, ThemeKind};
use libgui::widget::{ScrollBar, ScrollBarState, ScrollOrientation};

pub const TASKBAR_HEIGHT: i32 = 36;
const TRAY_W: i32 = 218;
const PEEK_W: i32 = 9;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskbarAction {
    None,
    ToggleStartMenu,
    ToggleSpotlight,
    OpenSpotlight(String),
    ToggleTheme,
    NextWallpaper,
    LaunchApp(DesktopAppId),
    LaunchPath(String),
    OpenFilesAt(&'static str, &'static str),
    OpenNetworkSettings,
    OpenDisplaySettings,
    FocusWindow(u32),
    RestoreWindow(u32),
    MinimizeWindow(u32),
    MaximizeWindow(u32),
    CloseWindow(u32),
    MinimizeAll,
    ExitToShell,
    RebootSystem,
    HaltSystem,
}

#[derive(Clone, Copy)]
enum StartProgramTarget {
    Builtin(DesktopAppId),
    Path(&'static str, DesktopAppId),
}

const START_LEFT_PROGRAMS: [(StartProgramTarget, &str, &str, &str); 12] = [
    (StartProgramTarget::Builtin(DesktopAppId::Terminal), "Terminal", "Command & Shell", ""),
    (StartProgramTarget::Builtin(DesktopAppId::FileManager), "File Explorer", "Files, Drives & Media", ""),
    (StartProgramTarget::Builtin(DesktopAppId::Sysmon), "Task Manager", "Processes & RAM", ""),
    (StartProgramTarget::Builtin(DesktopAppId::TextEdit), "Notepad", "Plain Text Notes", ""),
    (StartProgramTarget::Builtin(DesktopAppId::Calculator), "Calculator", "Standard Metro Calc", ""),
    (StartProgramTarget::Builtin(DesktopAppId::Settings), "Control Panel", "Display, Net & Audio", ""),
    (StartProgramTarget::Builtin(DesktopAppId::Studio), "Widget Studio", "ComCtl32 Ring-3 App", "widget studio"),
    (StartProgramTarget::Path("/Applications/App Center.app", DesktopAppId::RemoteGui), "App Center", "Browse & Launch User Apps", "app center"),
    (StartProgramTarget::Path("/Applications/Sublime Paint.app", DesktopAppId::RemoteGui), "Sublime Paint", "Bitmap Paint Editor", "sublime paint"),
    (StartProgramTarget::Path("/Applications/Music Player.app", DesktopAppId::RemoteGui), "Music Player", "Music Library & Visualizer", "music player"),
    (StartProgramTarget::Path("/Applications/Media Player.app", DesktopAppId::RemoteGui), "Media Player", "Audio, Video & Image Player", "media player"),
    (StartProgramTarget::Path("/Applications/Code Editor.app", DesktopAppId::RemoteGui), "Code Editor", "Syntax-Highlighted IDE", "code editor"),
];

#[derive(Clone, Copy)]
pub enum RightMenuKind {
    Place(&'static str, &'static str),
    Display,
    NextWall,
    Theme,
    About,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlaceToolIcon {
    Computer,
    Documents,
    Music,
    Pictures,
    Downloads,
    Trash,
    Personalize,
    Wallpaper,
    ThemeMode,
    About,
}

struct RightMenuItem {
    label: &'static str,
    kind: RightMenuKind,
    icon: PlaceToolIcon,
}

const START_RIGHT_PLACES: &[RightMenuItem] = &[
    RightMenuItem { label: "My Computer", kind: RightMenuKind::Place("/", "My Computer"), icon: PlaceToolIcon::Computer },
    RightMenuItem { label: "Documents", kind: RightMenuKind::Place("/Users/sublime/Documents", "Documents"), icon: PlaceToolIcon::Documents },
    RightMenuItem { label: "Music", kind: RightMenuKind::Place("/Users/sublime/Music", "Music"), icon: PlaceToolIcon::Music },
    RightMenuItem { label: "Pictures", kind: RightMenuKind::Place("/Users/sublime/Pictures", "Pictures"), icon: PlaceToolIcon::Pictures },
    RightMenuItem { label: "Downloads", kind: RightMenuKind::Place("/Users/sublime/Downloads", "Downloads"), icon: PlaceToolIcon::Downloads },
    RightMenuItem { label: "Trash", kind: RightMenuKind::Place("/Trash", "Trash"), icon: PlaceToolIcon::Trash },
];

const START_RIGHT_TOOLS: &[RightMenuItem] = &[
    RightMenuItem { label: "Personalize...", kind: RightMenuKind::Display, icon: PlaceToolIcon::Personalize },
    RightMenuItem { label: "Next Wallpaper", kind: RightMenuKind::NextWall, icon: PlaceToolIcon::Wallpaper },
    RightMenuItem { label: "Light / Dark Mode", kind: RightMenuKind::Theme, icon: PlaceToolIcon::ThemeMode },
    RightMenuItem { label: "About SublimeOS", kind: RightMenuKind::About, icon: PlaceToolIcon::About },
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PendingPowerAction {
    Reboot,
    Halt,
}

#[derive(Clone, Copy, Debug)]
pub struct StartMenuLayout {
    pub mx: i32,
    pub my: i32,
    pub mw: i32,
    pub mh: i32,
    pub hdr_h: i32,
    pub ftr_h: i32,
    pub body_y: i32,
    pub body_h: i32,
    pub left_w: i32,
    pub right_x: i32,
    pub right_w: i32,
    pub item_h: i32,
    pub row_h: i32,
    pub list_x: i32,
    pub list_y: i32,
    pub list_w: i32,
    pub list_h: i32,
    pub visible_rows: usize,
    pub sb_w: i32,
    pub search_x: i32,
    pub search_y: i32,
    pub search_w: i32,
    pub search_h: i32,
    pub r_row_h: i32,
    pub places_hdr_y: i32,
    pub places_y: i32,
    pub sep_y: i32,
    pub tools_hdr_y: i32,
    pub tools_y: i32,
    pub status_y: i32,
    pub status_h: i32,
}

impl StartMenuLayout {
    pub fn compute(sw: i32, sh: i32) -> Self {
        let mw = (sw - 12).clamp(380, 444);
        let mh = (sh - TASKBAR_HEIGHT - 12).clamp(320, 572);
        let mx = 4;
        let my = (sh - TASKBAR_HEIGHT - mh - 4).max(4);
        let hdr_h = if mh < 450 { 46 } else { 52 };
        let ftr_h = 42;
        let body_y = my + hdr_h;
        let body_h = mh - hdr_h - ftr_h;
        let left_w = (mw * 56) / 100;
        let right_x = mx + left_w;
        let right_w = mw - left_w;
        let item_h = 36;
        let row_h = 34;
        let search_h = 28;
        let search_x = mx + 6;
        let search_w = left_w - 12;
        let search_y = body_y + body_h - search_h - 6;
        let list_x = mx + 6;
        let list_y = body_y + 4;
        let list_w = left_w - 12;
        let list_h = (search_y - 8 - list_y).max(item_h);
        let visible_rows = (list_h / item_h).max(1) as usize;
        let sb_w = 12;
        let r_row_h = if mh < 460 { 22 } else { 25 };
        let places_hdr_y = body_y + 4;
        let places_y = places_hdr_y + 16;
        let sep_y = places_y + (START_RIGHT_PLACES.len() as i32) * r_row_h + 3;
        let tools_hdr_y = sep_y + 4;
        let tools_y = tools_hdr_y + 16;
        let status_y = tools_y + (START_RIGHT_TOOLS.len() as i32) * r_row_h + 6;
        let status_h = (body_y + body_h - 6 - status_y).max(0);
        Self {
            mx, my, mw, mh, hdr_h, ftr_h, body_y, body_h,
            left_w, right_x, right_w, item_h, row_h,
            list_x, list_y, list_w, list_h, visible_rows, sb_w,
            search_x, search_y, search_w, search_h,
            r_row_h, places_hdr_y, places_y, sep_y, tools_hdr_y, tools_y,
            status_y, status_h,
        }
    }

    pub fn menu_rect(&self) -> Rect {
        Rect::new(self.mx, self.my, self.mw, self.mh)
    }

    pub fn sb_rect(&self) -> Rect {
        Rect::new(self.list_x + self.list_w - self.sb_w, self.list_y, self.sb_w, self.list_h)
    }

    pub fn row_w(&self, has_sb: bool) -> i32 {
        if has_sb { self.list_w - self.sb_w - 2 } else { self.list_w }
    }

    pub fn search_rect(&self) -> Rect {
        Rect::new(self.search_x, self.search_y, self.search_w, self.search_h)
    }

    pub fn search_clear_rect(&self) -> Rect {
        Rect::new(self.search_x + self.search_w - 22, self.search_y + 3, 18, self.search_h - 6)
    }

    pub fn place_rect(&self, idx: usize) -> Rect {
        let ry = self.places_y + (idx as i32) * self.r_row_h;
        Rect::new(self.right_x + 6, ry, self.right_w - 12, self.r_row_h - 2)
    }

    pub fn tool_rect(&self, idx: usize) -> Rect {
        let ry = self.tools_y + (idx as i32) * self.r_row_h;
        Rect::new(self.right_x + 6, ry, self.right_w - 12, self.r_row_h - 2)
    }

    pub fn footer_power_rects(&self) -> (Rect, Rect, Rect) {
        let fy = self.my + self.mh - self.ftr_h;
        let by = fy + 8;
        let bh = 26;
        let halt_x = self.mx + self.mw - 10 - 82;
        let reboot_x = halt_x - 6 - 72;
        let shell_x = reboot_x - 6 - 56;
        (
            Rect::new(shell_x, by, 56, bh),
            Rect::new(reboot_x, by, 72, bh),
            Rect::new(halt_x, by, 82, bh),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartEntryAction {
    LaunchApp(DesktopAppId),
    LaunchPath(String),
    OpenFilesAt(&'static str, &'static str),
    OpenDisplaySettings,
    NextWallpaper,
    ToggleTheme,
    OpenSpotlight(String),
}

#[derive(Clone, Debug)]
pub struct StartListEntry {
    pub label: String,
    pub subtitle: &'static str,
    pub app_id: DesktopAppId,
    pub icon_key: &'static str,
    pub place_icon: Option<PlaceToolIcon>,
    pub action: StartEntryAction,
}

pub struct Taskbar {
    pub start_menu_open: bool,
    pub tray_popup_open: bool,
    pub volume_level: u8,
    pub context_menu: Option<(i32, i32, Option<u32>)>,
    pub start_search_query: String,
    pub start_selected_index: usize,
    pub start_scroll: ScrollBarState,
    pub start_pending_power: Option<PendingPowerAction>,
    pub start_cached_apps: Vec<crate::user::apps::AppManifest>,
}

impl Taskbar {
    pub fn new() -> Self {
        Self {
            start_menu_open: false,
            tray_popup_open: false,
            volume_level: crate::drivers::audio::get_volume(),
            context_menu: None,
            start_search_query: String::new(),
            start_selected_index: 0,
            start_scroll: ScrollBarState::new(0, 0, 1),
            start_pending_power: None,
            start_cached_apps: Vec::new(),
        }
    }

    /// Open or close the Start Menu cleanly, resetting search/scroll/power confirmation and caching `.app` bundles once on open.
    pub fn set_start_menu_open(&mut self, open: bool) {
        self.start_menu_open = open;
        self.start_search_query.clear();
        self.start_selected_index = 0;
        self.start_pending_power = None;
        self.start_scroll = ScrollBarState::new(0, 0, 1);
        if open {
            self.tray_popup_open = false;
            self.context_menu = None;
            self.start_cached_apps = crate::user::apps::discover_app_bundles();
        }
    }

    pub fn start_menu_layout(&self, sw: i32, sh: i32) -> StartMenuLayout {
        StartMenuLayout::compute(sw, sh)
    }

    /// Render Windows 8.1 Modern Flat Bottom Taskbar, Start Screen, Tray, and Show-Desktop Peek.
    pub fn render(
        &self,
        canvas: &mut Canvas,
        windows: &[Window],
        mouse_x: i32,
        mouse_y: i32,
    ) {
        let theme = libgui::gfx::current_theme();
        let sw = canvas.width as i32;
        let sh = canvas.height as i32;
        let ty = sh - TASKBAR_HEIGHT;

        // ==========================================
        // 1. Flat Translucent-Dark Metro Taskbar Strip
        // ==========================================
        let tb_top = Color::from_libgui(theme.taskbar_top);
        let tb_hi = Color::from_libgui(theme.taskbar_hilite);
        canvas.fill_rect(0, ty, sw, TASKBAR_HEIGHT, tb_top);
        canvas.fill_rect(0, ty, sw, 1, tb_hi);

        // ==========================================
        // 2. SublimeOS Start Button (Bottom-Left Geometric 'S' Crest)
        // ==========================================
        let start_w = 46;
        let start_hover = mouse_x >= 0 && mouse_x < start_w && mouse_y >= ty;
        if self.start_menu_open {
            canvas.fill_rect(0, ty + 1, start_w, TASKBAR_HEIGHT - 1, Color::from_libgui(theme.accent));
        } else if start_hover {
            canvas.fill_rect(0, ty + 1, start_w, TASKBAR_HEIGHT - 1, Color::argb(40, 255, 255, 255));
        }
        let start_logo_pri = Color::from_libgui(if self.start_menu_open {
            libgui::gfx::contrast_text_on(theme.accent)
        } else {
            libgui::gfx::Color::WHITE
        });
        let start_logo_acc = if start_hover && !self.start_menu_open {
            Color::rgb(96, 205, 255)
        } else if self.start_menu_open {
            Color::lerp(start_logo_pri, Color::from_libgui(theme.accent), 70)
        } else {
            Color::lerp(Color::WHITE, Color::from_libgui(theme.accent), 120)
        };
        draw_sublime_logo(canvas, 13, ty + 8, 20, start_logo_pri, start_logo_acc);

        // ==========================================
        // 3. Quick-Pinned Taskbar Icons (Explorer, Terminal, Studio)
        // ==========================================
        let ql_x = start_w + 6;
        let pin_apps = [
            DesktopAppId::FileManager,
            DesktopAppId::Terminal,
            DesktopAppId::Studio,
        ];
        for (idx, app_id) in pin_apps.iter().enumerate() {
            let px = ql_x + (idx as i32) * 34;
            let ph = mouse_x >= px && mouse_x < px + 32 && mouse_y >= ty + 1 && mouse_y < sh;
            if ph {
                canvas.fill_rect(px, ty + 1, 32, TASKBAR_HEIGHT - 1, Color::argb(38, 255, 255, 255));
            }
            draw_mini_window_icon(canvas, px + 9, ty + 11, *app_id);
        }
        let sep_x = ql_x + 3 * 34 + 4;
        canvas.fill_rect(sep_x, ty + 7, 1, TASKBAR_HEIGHT - 14, tb_hi);

        // ==========================================
        // 4. Open Window Taskbar Buttons (Flat Tiles + Active Underline Bar)
        // ==========================================
        let peek_w = PEEK_W;
        let tray_w = TRAY_W;
        let tray_x = sw - tray_w - peek_w;
        let tabs_start_x = sep_x + 8;
        let tabs_avail_w = (tray_x - tabs_start_x - 8).max(80);
        let accent = Color::from_libgui(theme.accent);

        if !windows.is_empty() {
            let count = windows.len() as i32;
            let tab_w = ((tabs_avail_w / count) - 3).clamp(76, 176);
            let mut tx = tabs_start_x;

            for w in windows.iter() {
                if tx + tab_w > tray_x - 4 {
                    break;
                }
                let is_pressed = w.is_active && !w.is_minimized;
                let is_hover = mouse_x >= tx && mouse_x < tx + tab_w && mouse_y >= ty + 1 && mouse_y < sh;

                if is_pressed {
                    canvas.fill_rect(tx, ty + 1, tab_w, TASKBAR_HEIGHT - 1, Color::argb(65, 255, 255, 255));
                    canvas.fill_rect(tx, sh - 3, tab_w, 3, Color::rgb(96, 205, 255));
                } else if is_hover {
                    canvas.fill_rect(tx, ty + 1, tab_w, TASKBAR_HEIGHT - 1, Color::argb(38, 255, 255, 255));
                    canvas.fill_rect(tx + 2, sh - 2, tab_w - 4, 2, accent);
                } else {
                    canvas.fill_rect(tx, ty + 1, tab_w, TASKBAR_HEIGHT - 1, Color::argb(20, 255, 255, 255));
                    canvas.fill_rect(tx + 4, sh - 2, tab_w - 8, 2, tb_hi);
                }

                draw_mini_window_icon(canvas, tx + 8, ty + 11, w.app.app_id());
                draw_ui_string_ellipsis(canvas, tx + 26, ty + 8, &w.title, Color::WHITE, tab_w - 32);
                tx += tab_w + 3;
            }
        }

        // ==========================================
        // 5. Notification Area (Wi-Fi + Volume + RAM + Clock) & Right-Edge Peek Sliver
        // ==========================================
        let (wifi_conn, wifi_bars): (bool, u8) = {
            let wifi = WIFI_MANAGER.lock();
            if wifi.is_connected {
                let bars = wifi
                    .scan_results
                    .iter()
                    .find(|ap| Some(&ap.ssid) == wifi.current_ssid.as_ref())
                    .map(|ap| {
                        if ap.rssi >= -50 { 4 } else if ap.rssi >= -65 { 3 } else if ap.rssi >= -75 { 2 } else { 1 }
                    })
                    .unwrap_or(3);
                (true, bars)
            } else {
                (false, 0)
            }
        };
        let wx = tray_x + 8;
        for b in 0..4i32 {
            let bh = 3 + b * 3;
            let bar_col = if wifi_conn && (b as u8) < wifi_bars {
                Color::WHITE
            } else if !wifi_conn && b == 0 {
                Color::rgb(255, 200, 90)
            } else {
                Color::argb(90, 255, 255, 255)
            };
            canvas.fill_rect(wx + b * 4, ty + 24 - bh, 3, bh, bar_col);
        }

        // Flat Volume speaker glyph (honors mute state)
        let sys_vol = crate::drivers::audio::get_volume();
        let sys_muted = crate::drivers::audio::is_muted();
        let vx = tray_x + 30;
        canvas.fill_rect(vx, ty + 15, 3, 6, Color::WHITE);
        canvas.fill_rect(vx + 3, ty + 13, 3, 10, Color::WHITE);
        if sys_vol > 0 && !sys_muted {
            canvas.fill_rect(vx + 8, ty + 14, 1, 8, Color::rgb(96, 205, 255));
        } else if sys_muted {
            canvas.draw_line(vx + 8, ty + 15, vx + 12, ty + 19, Color::rgb(235, 95, 95));
            canvas.draw_line(vx + 12, ty + 15, vx + 8, ty + 19, Color::rgb(235, 95, 95));
        }

        // RAM % badge
        let (used_mb, total_mb) = {
            let pmm = PMM.lock();
            let used = (pmm.get_used_frames() * 4) / 1024;
            let total = (pmm.get_total_frames() * 4) / 1024;
            (used, total)
        };
        let ram_pct = if total_mb > 0 { (used_mb * 100) / total_mb } else { 0 };
        let mut ram_str = String::from("M:");
        push_u64(&mut ram_str, ram_pct as u64);
        ram_str.push('%');
        draw_ui_string(canvas, tray_x + 48, ty + 8, &ram_str, Color::rgb(215, 230, 245));

        // RTC Clock (measured dynamically so HH:MM:SS never clips at the Peek sliver)
        let time = get_system_datetime();
        let mut clock_str = String::new();
        push_2digits(&mut clock_str, time.hour);
        clock_str.push(':');
        push_2digits(&mut clock_str, time.minute);
        clock_str.push(':');
        push_2digits(&mut clock_str, time.second);
        let clock_w = measure_ui_string(&clock_str);
        let clock_x = (sw - peek_w - clock_w - 10).max(tray_x + 104);
        draw_ui_string(canvas, clock_x, ty + 8, &clock_str, Color::WHITE);

        // Windows 8.1 Far-Right 9px "Show Desktop / Peek" Sliver
        let peek_x = sw - peek_w;
        let peek_hover = mouse_x >= peek_x && mouse_y >= ty;
        canvas.fill_rect(peek_x, ty + 1, 1, TASKBAR_HEIGHT - 1, tb_hi);
        if peek_hover {
            canvas.fill_rect(peek_x + 1, ty + 1, peek_w - 1, TASKBAR_HEIGHT - 1, Color::argb(60, 255, 255, 255));
        }

        // ==========================================
        // 6. Windows XP Layout + Windows 8.1 Styled Start Menu (if open)
        // ==========================================
        if self.start_menu_open {
            self.render_start_menu(
                canvas, sw, sh, mouse_x, mouse_y, used_mb, total_mb, ram_pct, wifi_conn, sys_vol, sys_muted,
            );
        }

        // ==========================================
        // 7. System Tray Telemetry Popup (if open)
        // ==========================================
        if self.tray_popup_open {
            self.render_tray_popup(canvas, sw, sh, used_mb, total_mb);
        }

        // ==========================================
        // 8. Taskbar / Tab Right-Click Context Menu (if open)
        // ==========================================
        if self.context_menu.is_some() {
            self.render_context_menu(canvas, mouse_x, mouse_y);
        }
    }

    /// Bounding rectangle `(x, y, w, h)` of the responsive Start Menu popup.
    pub fn start_menu_rect(&self, sw: i32, sh: i32) -> (i32, i32, i32, i32) {
        let l = StartMenuLayout::compute(sw, sh);
        (l.mx, l.my, l.mw, l.mh)
    }

    /// Build the filtered Start Menu list cleanly without hitting VFS during render/input.
    pub fn filtered_start_entries(&self) -> Vec<StartListEntry> {
        let q = self.start_search_query.trim().to_ascii_lowercase();
        let mut entries = Vec::new();

        for (target, name, desc, icon_key) in START_LEFT_PROGRAMS.iter() {
            if !q.is_empty() && !matches_query(&q, &[name, desc]) {
                continue;
            }
            let (app_id, action) = match target {
                StartProgramTarget::Builtin(id) => (*id, StartEntryAction::LaunchApp(*id)),
                StartProgramTarget::Path(p, id) => (*id, StartEntryAction::LaunchPath(String::from(*p))),
            };
            entries.push(StartListEntry {
                label: String::from(*name),
                subtitle: desc,
                app_id,
                icon_key,
                place_icon: None,
                action,
            });
        }

        for app in self.start_cached_apps.iter() {
            let already_pinned = START_LEFT_PROGRAMS.iter().any(|(target, _, _, _)| match target {
                StartProgramTarget::Builtin(b) => app.builtin == Some(*b),
                StartProgramTarget::Path(p, _) => app.bundle_path == *p,
            });
            if already_pinned {
                continue;
            }
            if !q.is_empty() && !matches_query(&q, &[&app.name, &app.comment, &app.exec]) {
                continue;
            }
            entries.push(StartListEntry {
                label: app.name.clone(),
                subtitle: "Installed App",
                app_id: app.builtin.unwrap_or(DesktopAppId::RemoteGui),
                icon_key: "",
                place_icon: None,
                action: StartEntryAction::LaunchPath(app.bundle_path.clone()),
            });
        }

        if !q.is_empty() {
            for item in START_RIGHT_PLACES.iter() {
                if matches_query(&q, &[item.label]) {
                    if let RightMenuKind::Place(path, title) = item.kind {
                        entries.push(StartListEntry {
                            label: String::from(item.label),
                            subtitle: "Folder / Place",
                            app_id: DesktopAppId::FileManager,
                            icon_key: "",
                            place_icon: Some(item.icon),
                            action: StartEntryAction::OpenFilesAt(path, title),
                        });
                    }
                }
            }
            for item in START_RIGHT_TOOLS.iter() {
                if matches_query(&q, &[item.label]) {
                    let (app_id, action) = match item.kind {
                        RightMenuKind::Display => (DesktopAppId::Settings, StartEntryAction::OpenDisplaySettings),
                        RightMenuKind::NextWall => (DesktopAppId::Settings, StartEntryAction::NextWallpaper),
                        RightMenuKind::Theme => (DesktopAppId::Settings, StartEntryAction::ToggleTheme),
                        RightMenuKind::About => (DesktopAppId::About, StartEntryAction::LaunchApp(DesktopAppId::About)),
                        RightMenuKind::Place(p, t) => (DesktopAppId::FileManager, StartEntryAction::OpenFilesAt(p, t)),
                    };
                    entries.push(StartListEntry {
                        label: String::from(item.label),
                        subtitle: "System Tool",
                        app_id,
                        icon_key: "",
                        place_icon: Some(item.icon),
                        action,
                    });
                }
            }
            entries.push(StartListEntry {
                label: format!("Search in Spotlight: \"{}\"", self.start_search_query.trim()),
                subtitle: "Apps, CLI tools & calculator",
                app_id: DesktopAppId::Settings,
                icon_key: "",
                place_icon: Some(PlaceToolIcon::About),
                action: StartEntryAction::OpenSpotlight(String::from(self.start_search_query.trim())),
            });
        }

        entries
    }

    /// Keep `start_selected_index` and `start_scroll` bounds valid inside `Taskbar` mutators.
    pub fn sync_start_scroll(&mut self, sw: i32, sh: i32, ensure_selected: bool) {
        let l = StartMenuLayout::compute(sw, sh);
        let count = self.filtered_start_entries().len();
        if count == 0 {
            self.start_selected_index = 0;
            self.start_scroll.sync_range(0, 0, l.visible_rows as i32);
            return;
        }
        if self.start_selected_index >= count {
            self.start_selected_index = count - 1;
        }
        let max_scroll = count.saturating_sub(l.visible_rows) as i32;
        let mut val = self.start_scroll.value.clamp(0, max_scroll);
        if ensure_selected {
            let sel = self.start_selected_index as i32;
            let vis = l.visible_rows as i32;
            if sel < val {
                val = sel;
            } else if sel >= val + vis {
                val = (sel + 1 - vis).clamp(0, max_scroll);
            }
        }
        self.start_scroll.sync_range(val, max_scroll, l.visible_rows as i32);
    }

    #[allow(clippy::too_many_arguments)]
    fn render_start_menu(
        &self,
        canvas: &mut Canvas,
        sw: i32,
        sh: i32,
        mouse_x: i32,
        mouse_y: i32,
        used_mb: usize,
        total_mb: usize,
        ram_pct: usize,
        wifi_conn: bool,
        sys_vol: u8,
        sys_muted: bool,
    ) {
        let theme = libgui::gfx::current_theme();
        let l = StartMenuLayout::compute(sw, sh);
        let entries = self.filtered_start_entries();
        let max_scroll = entries.len().saturating_sub(l.visible_rows) as i32;
        let has_sb = max_scroll > 0;
        let scroll_top = self.start_scroll.value.clamp(0, max_scroll) as usize;
        let row_w = l.row_w(has_sb);

        // Soft drop shadow + crisp outer frame border
        canvas.fill_rect(l.mx + 4, l.my + 4, l.mw, l.mh, Color::argb(96, 0, 0, 0));
        canvas.fill_rect(l.mx - 1, l.my - 1, l.mw + 2, l.mh + 2, Color::from_libgui(theme.frame_outer));

        // 1. Top User Profile Header
        let hdr_top = Color::from_libgui(theme.title_top);
        let hdr_bot = Color::from_libgui(theme.title_bot);
        let hdr_fg = Color::from_libgui(libgui::gfx::contrast_text_on(theme.title_top));
        canvas.draw_gradient_v(l.mx, l.my, l.mw, l.hdr_h, hdr_top, hdr_bot);
        let badge_sz = if l.hdr_h < 50 { 32 } else { 36 };
        let badge_y = l.my + (l.hdr_h - badge_sz) / 2;
        canvas.fill_rect(l.mx + 12, badge_y, badge_sz, badge_sz, Color::from_libgui(theme.accent));
        canvas.draw_rect(l.mx + 12, badge_y, badge_sz, badge_sz, Color::lerp(hdr_fg, hdr_top, 140));
        let crest_fg = Color::from_libgui(libgui::gfx::contrast_text_on(theme.accent));
        let crest_acc = Color::lerp(crest_fg, Color::from_libgui(theme.face_light), 75);
        let logo_pad = (badge_sz - 24) / 2;
        draw_sublime_logo(canvas, l.mx + 12 + logo_pad, badge_y + logo_pad, 24, crest_fg, crest_acc);
        let title_y = l.my + if l.hdr_h < 50 { 5 } else { 9 };
        let sub_y = l.my + if l.hdr_h < 50 { 23 } else { 27 };
        draw_ui_string_bold(canvas, l.mx + 56, title_y, "sublime", hdr_fg);
        draw_ui_string(
            canvas,
            l.mx + 56,
            sub_y,
            "SublimeOS 1.0 | Metro Workspace",
            Color::lerp(hdr_fg, hdr_top, 60),
        );

        // 2. Left Pane — Scrollable Programs / Unified Search List + Inline Search Box
        let left_bg = Color::from_libgui(theme.face_light);
        let left_fg = Color::from_libgui(libgui::gfx::contrast_text_on(theme.face_light));
        let left_sub = Color::lerp(left_fg, left_bg, 95);
        canvas.fill_rect(l.mx, l.body_y, l.left_w, l.body_h, left_bg);

        canvas.set_clip_rect(l.list_x, l.list_y, row_w, l.list_h);
        for rel in 0..=l.visible_rows {
            let abs_idx = scroll_top + rel;
            let Some(entry) = entries.get(abs_idx) else {
                break;
            };
            let iy = l.list_y + (rel as i32) * l.item_h;
            let ix = l.list_x;
            let is_selected = abs_idx == self.start_selected_index;
            let is_mouse_hover = mouse_x >= ix
                && mouse_x < ix + row_w
                && mouse_y >= iy
                && mouse_y < iy + l.row_h
                && mouse_y >= l.list_y
                && mouse_y < l.list_y + l.list_h;

            if is_selected {
                canvas.fill_rect(ix, iy, row_w, l.row_h, Color::from_libgui(theme.selection));
                canvas.fill_rect(ix, iy, 3, l.row_h, Color::from_libgui(theme.accent));
            } else if is_mouse_hover {
                canvas.fill_rect(ix, iy, row_w, l.row_h, Color::argb(28, 128, 128, 128));
            }

            let tile_bg = if entry.place_icon.is_some() {
                Color::from_libgui(theme.face_dark)
            } else {
                app_tile_color(entry.app_id, entry.icon_key)
            };
            canvas.fill_rect(ix + 6, iy + 4, 26, 26, tile_bg);
            if let Some(p_ico) = entry.place_icon {
                draw_place_tool_icon(canvas, ix + 12, iy + 10, p_ico, Color::from_libgui(theme.accent), Color::from_libgui(theme.title_highlight));
            } else {
                draw_app_badge_icon(canvas, ix + 12, iy + 10, entry.app_id, entry.icon_key);
            }

            let t_fg = if is_selected {
                Color::from_libgui(libgui::gfx::contrast_text_on(theme.selection))
            } else {
                left_fg
            };
            let t_sub = if is_selected {
                Color::lerp(t_fg, Color::from_libgui(theme.selection), 55)
            } else {
                left_sub
            };
            draw_ui_string_ellipsis(canvas, ix + 38, iy + 2, &entry.label, t_fg, row_w - 42);
            draw_ui_string_ellipsis(canvas, ix + 38, iy + 17, entry.subtitle, t_sub, row_w - 42);
        }
        canvas.clear_clip_rect();

        if has_sb {
            let sb = l.sb_rect();
            let sb_state = self.start_scroll;
            canvas.with_surface(|surf| {
                ScrollBar::draw(surf, sb, ScrollOrientation::Vertical, sb_state, &theme);
            });
        }

        // Bottom of Left Column: Interactive Inline Search Input Box
        let sr = l.search_rect();
        canvas.fill_rect(l.mx + 8, sr.y - 4, l.left_w - 16, 1, Color::from_libgui(theme.ctrl_border));
        canvas.fill_rect(sr.x, sr.y, sr.w, sr.h, Color::from_libgui(theme.input_bg));
        let sr_border = if !self.start_search_query.is_empty()
            || (mouse_x >= sr.x && mouse_x < sr.right() && mouse_y >= sr.y && mouse_y < sr.bottom())
        {
            theme.accent
        } else {
            theme.input_border
        };
        canvas.draw_rect(sr.x, sr.y, sr.w, sr.h, Color::from_libgui(sr_border));
        canvas.draw_circle(sr.x + 12, sr.y + 13, 4, Color::from_libgui(theme.accent));
        canvas.draw_line(sr.x + 15, sr.y + 16, sr.x + 19, sr.y + 20, Color::from_libgui(theme.accent));

        let clr = l.search_clear_rect();
        let text_x = sr.x + 25;
        let text_max_w = (if self.start_search_query.is_empty() {
            sr.right() - 6
        } else {
            clr.x - 4
        } - text_x)
            .max(16);
        canvas.set_clip_rect(text_x, sr.y + 2, text_max_w, sr.h - 4);
        if self.start_search_query.is_empty() {
            draw_ui_string_centered_v(
                canvas,
                text_x,
                sr.y,
                sr.h,
                "Type to search apps & places...",
                Color::from_libgui(theme.text_secondary),
            );
        } else {
            let shown = format!("{}|", self.start_search_query);
            draw_ui_string_centered_v(canvas, text_x, sr.y, sr.h, &shown, Color::from_libgui(theme.text_primary));
        }
        canvas.clear_clip_rect();

        if !self.start_search_query.is_empty() {
            let clr_hov = mouse_x >= clr.x && mouse_x < clr.right() && mouse_y >= clr.y && mouse_y < clr.bottom();
            canvas.fill_rect(
                clr.x,
                clr.y,
                clr.w,
                clr.h,
                Color::from_libgui(if clr_hov { theme.selection } else { theme.face_dark }),
            );
            let x_col = Color::from_libgui(if clr_hov { theme.selection_text } else { theme.text_primary });
            draw_ui_string_centered_v(canvas, clr.x + 5, clr.y, clr.h, "x", x_col);
        }

        // 3. Right Pane — Places, System Tools & Telemetry Summary
        let right_bg = Color::from_libgui(theme.face);
        let right_fg = Color::from_libgui(libgui::gfx::contrast_text_on(theme.face));
        let right_sub = Color::from_libgui(theme.text_secondary);
        canvas.fill_rect(l.right_x, l.body_y, 1, l.body_h, Color::from_libgui(theme.ctrl_border));
        canvas.fill_rect(l.right_x + 1, l.body_y, l.right_w - 1, l.body_h, right_bg);

        draw_ui_string_sized(canvas, l.right_x + 10, l.places_hdr_y, 11, true, "PLACES", right_sub);
        for (i, item) in START_RIGHT_PLACES.iter().enumerate() {
            let pr = l.place_rect(i);
            let hover = mouse_x >= pr.x && mouse_x < pr.right() && mouse_y >= pr.y && mouse_y < pr.bottom();
            if hover {
                canvas.fill_rect(pr.x, pr.y, pr.w, pr.h, Color::from_libgui(theme.selection));
            }
            let r_fg = if hover {
                Color::from_libgui(libgui::gfx::contrast_text_on(theme.selection))
            } else {
                right_fg
            };
            let ico_y = pr.y + (pr.h - 14) / 2;
            draw_place_tool_icon(canvas, pr.x + 6, ico_y, item.icon, Color::from_libgui(theme.accent), r_fg);
            draw_ui_string_ellipsis(canvas, pr.x + 26, pr.y + (pr.h - 16) / 2, item.label, r_fg, pr.w - 30);
        }

        canvas.fill_rect(l.right_x + 10, l.sep_y, l.right_w - 20, 1, Color::from_libgui(theme.ctrl_border));
        draw_ui_string_sized(canvas, l.right_x + 10, l.tools_hdr_y, 11, true, "SYSTEM", right_sub);

        let is_dark = matches!(theme.kind, ThemeKind::SublimeSlate);
        for (i, item) in START_RIGHT_TOOLS.iter().enumerate() {
            let tr = l.tool_rect(i);
            let hover = mouse_x >= tr.x && mouse_x < tr.right() && mouse_y >= tr.y && mouse_y < tr.bottom();
            if hover {
                canvas.fill_rect(tr.x, tr.y, tr.w, tr.h, Color::from_libgui(theme.selection));
            }
            let r_fg = if hover {
                Color::from_libgui(libgui::gfx::contrast_text_on(theme.selection))
            } else {
                right_fg
            };
            let ico_y = tr.y + (tr.h - 14) / 2;
            draw_place_tool_icon(canvas, tr.x + 6, ico_y, item.icon, Color::from_libgui(theme.accent), r_fg);

            if matches!(item.kind, RightMenuKind::Theme) {
                let badge = if is_dark { "Dark" } else { "Light" };
                let bw = 38;
                let bx = tr.right() - bw - 4;
                canvas.fill_rect(bx, tr.y + 3, bw, tr.h - 6, Color::from_libgui(theme.face_dark));
                canvas.draw_rect(bx, tr.y + 3, bw, tr.h - 6, Color::from_libgui(theme.ctrl_border));
                let tw = measure_ui_string(badge);
                draw_ui_string_centered_v(canvas, bx + (bw - tw) / 2, tr.y + 3, tr.h - 6, badge, right_fg);
                draw_ui_string_ellipsis(canvas, tr.x + 26, tr.y + (tr.h - 16) / 2, item.label, r_fg, (bx - tr.x - 30).max(40));
            } else {
                draw_ui_string_ellipsis(canvas, tr.x + 26, tr.y + (tr.h - 16) / 2, item.label, r_fg, tr.w - 30);
            }
        }

        if l.status_h >= 48 {
            let card_h = l.status_h.min(68);
            let cx = l.right_x + 8;
            let cw = l.right_w - 16;
            let cy = l.body_y + l.body_h - card_h - 6;
            canvas.fill_rect(cx, cy, cw, card_h, Color::from_libgui(theme.face_light));
            canvas.draw_rect(cx, cy, cw, card_h, Color::from_libgui(theme.ctrl_border));
            let ram_line = format!("RAM {}% ({}/{} MB)", ram_pct, used_mb, total_mb);
            draw_ui_string_ellipsis(canvas, cx + 8, cy + 5, &ram_line, right_fg, cw - 16);
            let bar_w = cw - 16;
            let fill_w = ((ram_pct as i32 * bar_w) / 100).clamp(2, bar_w);
            canvas.fill_rect(cx + 8, cy + 24, bar_w, 4, Color::from_libgui(theme.border_shadow));
            canvas.fill_rect(cx + 8, cy + 24, fill_w, 4, Color::from_libgui(theme.accent));
            let net_vol = if sys_muted {
                format!("{} | Audio Muted", if wifi_conn { "Wi-Fi" } else { "Local" })
            } else {
                format!("{} | Vol {}%", if wifi_conn { "Wi-Fi" } else { "Local" }, sys_vol)
            };
            draw_ui_string_ellipsis(canvas, cx + 8, cy + 33, &net_vol, right_sub, cw - 16);
        }

        // 4. Bottom Power & Hint Footer (no telemetry squeeze, 2-step destructive confirm)
        let fy = l.my + l.mh - l.ftr_h;
        let ftr_bg = Color::from_libgui(theme.face_dark);
        let ftr_fg = Color::from_libgui(libgui::gfx::contrast_text_on(theme.face_dark));
        canvas.fill_rect(l.mx, fy, l.mw, 1, Color::from_libgui(theme.ctrl_border));
        canvas.fill_rect(l.mx, fy + 1, l.mw, l.ftr_h - 1, ftr_bg);

        let (shell_r, reboot_r, halt_r) = l.footer_power_rects();
        let hint_max_w = (shell_r.x - (l.mx + 10) - 8).max(40);
        let hint_text = if self.start_pending_power.is_some() {
            "Click Confirm? again or Esc"
        } else {
            "Type to search | Esc"
        };
        draw_ui_string_ellipsis(canvas, l.mx + 10, fy + (l.ftr_h - 16) / 2, hint_text, ftr_fg, hint_max_w);

        let reboot_lbl = if self.start_pending_power == Some(PendingPowerAction::Reboot) { "Confirm?" } else { "Restart" };
        let halt_lbl = if self.start_pending_power == Some(PendingPowerAction::Halt) { "Confirm?" } else { "Shut Down" };
        let reboot_col = if self.start_pending_power == Some(PendingPowerAction::Reboot) {
            Color::from_libgui(theme.warn)
        } else {
            Color::rgb(0, 132, 60)
        };
        let halt_col = if self.start_pending_power == Some(PendingPowerAction::Halt) {
            Color::from_libgui(theme.danger)
        } else {
            Color::rgb(190, 52, 52)
        };
        let p_specs = [
            (shell_r, "Shell", Color::rgb(196, 114, 24)),
            (reboot_r, reboot_lbl, reboot_col),
            (halt_r, halt_lbl, halt_col),
        ];
        for (rect, lbl, col) in p_specs.iter() {
            let hov = mouse_x >= rect.x && mouse_x < rect.right() && mouse_y >= rect.y && mouse_y < rect.bottom();
            let fill = if hov { Color::lerp(*col, Color::WHITE, 45) } else { *col };
            canvas.fill_rect(rect.x, rect.y, rect.w, rect.h, fill);
            if hov {
                canvas.draw_rect(rect.x - 1, rect.y - 1, rect.w + 2, rect.h + 2, Color::WHITE);
            }
            let lw = measure_ui_string(lbl);
            draw_ui_string_centered_v(canvas, rect.x + (rect.w - lw) / 2, rect.y, rect.h, lbl, Color::WHITE);
        }
    }

    fn render_tray_popup(&self, canvas: &mut Canvas, sw: i32, sh: i32, used_mb: usize, total_mb: usize) {
        let theme = libgui::gfx::current_theme();
        let pw = 282;
        let ph = 186;
        let px = sw - pw - 10;
        let py = sh - TASKBAR_HEIGHT - ph - 6;

        canvas.fill_rect(px + 2, py + 2, pw, ph, Color::argb(45, 0, 0, 0));
        canvas.fill_rect(px, py, pw, ph, Color::from_libgui(theme.face_light));
        canvas.draw_rect(px, py, pw, ph, Color::from_libgui(theme.frame_active));
        canvas.fill_rect(px, py, pw, 28, Color::from_libgui(theme.title_top));
        draw_ui_string_bold(canvas, px + 10, py + 4, "SublimeOS Networks & Audio", Color::WHITE);

        let (wifi_line, wifi_col) = {
            let wifi = WIFI_MANAGER.lock();
            if wifi.is_connected {
                (
                    format!(
                        "Wi-Fi: {} ({} Mbps)",
                        wifi.current_ssid.as_deref().unwrap_or("Connected"),
                        wifi.link_speed_mbps
                    ),
                    Color::from_libgui(theme.good),
                )
            } else if !wifi.scan_results.is_empty() {
                (
                    format!("Wi-Fi: Ready ({} APs found)", wifi.scan_results.len()),
                    Color::from_libgui(theme.text_link),
                )
            } else {
                (String::from("Wi-Fi: Disconnected (802.11 ready)"), Color::from_libgui(theme.text_secondary))
            }
        };
        draw_ui_string_ellipsis(canvas, px + 12, py + 34, &wifi_line, wifi_col, pw - 24);

        let nic_line = {
            let net = NET_MANAGER.lock();
            if let Some((dev, cfg)) = net.get_default_device() {
                format!(
                    "NIC: {}  IP {}.{}.{}.{}",
                    dev.name(), cfg.ipv4[0], cfg.ipv4[1], cfg.ipv4[2], cfg.ipv4[3]
                )
            } else {
                String::from("NIC: lo0 127.0.0.1")
            }
        };
        draw_ui_string_ellipsis(canvas, px + 12, py + 54, &nic_line, Color::from_libgui(theme.text_primary), pw - 24);

        let btn_y = py + 80;
        canvas.fill_rect(px + 12, btn_y, 92, 24, Color::from_libgui(theme.face_dark));
        canvas.draw_rect(px + 12, btn_y, 92, 24, Color::from_libgui(theme.ctrl_border));
        draw_ui_string_centered_v(canvas, px + 22, btn_y, 24, "Scan Wi-Fi", Color::from_libgui(theme.text_primary));

        canvas.fill_rect(px + 112, btn_y, 158, 24, Color::from_libgui(theme.accent));
        draw_ui_string_centered_v(canvas, px + 132, btn_y, 24, "Wi-Fi Settings...", Color::WHITE);

        let mut mem_str = String::from("Memory: ");
        push_u64(&mut mem_str, used_mb as u64);
        mem_str.push('/');
        push_u64(&mut mem_str, total_mb as u64);
        mem_str.push_str(" MB");
        draw_ui_string_ellipsis(canvas, px + 12, py + 112, &mem_str, Color::from_libgui(theme.text_primary), pw - 24);
        let vol = crate::drivers::audio::get_volume();
        draw_ui_string(canvas, px + 12, py + 132, &format!("Master Volume: {}%", vol), Color::from_libgui(theme.text_primary));

        let slider_w = pw - 24;
        let filled_w = ((vol as i32 * slider_w) / 100).clamp(4, slider_w);
        canvas.fill_rect(px + 12, py + 158, slider_w, 6, Color::from_libgui(theme.border_shadow));
        canvas.fill_rect(px + 12, py + 158, filled_w, 6, Color::from_libgui(theme.accent));
        canvas.fill_rect(px + 10 + filled_w, py + 152, 8, 18, Color::from_libgui(theme.text_primary));
    }

    pub fn open_context_menu(&mut self, x: i32, y: i32, windows: &[Window], sw: i32, sh: i32) {
        let ty = sh - TASKBAR_HEIGHT;
        if y < ty {
            return;
        }
        let start_w = 46;
        let ql_x = start_w + 6;
        let sep_x = ql_x + 3 * 34 + 4;
        let tabs_start_x = sep_x + 8;
        let tray_x = sw - TRAY_W - PEEK_W;
        let tabs_avail_w = (tray_x - tabs_start_x - 8).max(80);
        let mut target_win = None;
        if !windows.is_empty() && x >= tabs_start_x && x < tray_x {
            let count = windows.len() as i32;
            let tab_w = ((tabs_avail_w / count) - 3).clamp(76, 176);
            let mut tx = tabs_start_x;
            for w in windows.iter() {
                if tx + tab_w > tray_x - 4 {
                    break;
                }
                if x >= tx && x < tx + tab_w {
                    target_win = Some(w.id);
                    break;
                }
                tx += tab_w + 3;
            }
        }
        let mw = 178;
        let mh = 104;
        let mx = x.min((sw - mw - 4).max(0)).max(0);
        let my = (ty - mh - 2).max(0);
        self.tray_popup_open = false;
        self.set_start_menu_open(false);
        self.context_menu = Some((mx, my, target_win));
    }

    pub fn handle_context_click(&mut self, x: i32, y: i32) -> Option<TaskbarAction> {
        let (mx, my, target_win) = self.context_menu?;
        let mw = 178;
        let mh = 104;
        self.context_menu = None;
        if x < mx || x > mx + mw || y < my || y > my + mh {
            return None;
        }
        let row = ((y - (my + 4)) / 24).clamp(0, 3);
        if let Some(wid) = target_win {
            Some(match row {
                0 => TaskbarAction::FocusWindow(wid),
                1 => TaskbarAction::MinimizeWindow(wid),
                2 => TaskbarAction::MaximizeWindow(wid),
                _ => TaskbarAction::CloseWindow(wid),
            })
        } else {
            Some(match row {
                0 => TaskbarAction::LaunchApp(DesktopAppId::Sysmon),
                1 => TaskbarAction::MinimizeAll,
                2 => TaskbarAction::ToggleSpotlight,
                _ => TaskbarAction::ToggleTheme,
            })
        }
    }

    fn render_context_menu(&self, canvas: &mut Canvas, mouse_x: i32, mouse_y: i32) {
        let Some((mx, my, target_win)) = self.context_menu else {
            return;
        };
        let mw = 178;
        let mh = 104;
        let theme = libgui::gfx::current_theme();
        canvas.fill_rect(mx + 2, my + 2, mw, mh, Color::argb(40, 0, 0, 0));
        canvas.fill_rect(mx, my, mw, mh, Color::from_libgui(theme.face_light));
        canvas.draw_rect(mx, my, mw, mh, Color::from_libgui(theme.ctrl_border));

        let labels: [&str; 4] = if target_win.is_some() {
            ["Switch To", "Minimize", "Maximize / Restore", "Close Window"]
        } else {
            ["Task Manager", "Show Desktop", "Search (Win+R)", "Switch Theme"]
        };
        for (i, &lbl) in labels.iter().enumerate() {
            let ry = my + 4 + (i as i32) * 24;
            let hover = mouse_x >= mx + 2 && mouse_x < mx + mw - 2 && mouse_y >= ry && mouse_y < ry + 22;
            if hover {
                canvas.fill_rect(mx + 2, ry, mw - 4, 22, Color::from_libgui(theme.selection));
            }
            draw_ui_string(
                canvas,
                mx + 10,
                ry + 2,
                lbl,
                Color::from_libgui(if hover { theme.selection_text } else { theme.text_primary }),
            );
        }
    }

    /// Forward mouse down/move/up to the Start Menu vertical scrollbar without double-handling.
    pub fn handle_start_scroll_mouse(&mut self, ev: GuiEvent, sw: i32, sh: i32) -> (bool, bool) {
        if !self.start_menu_open {
            return (false, false);
        }
        self.sync_start_scroll(sw, sh, false);
        if self.start_scroll.max <= 0 {
            self.start_scroll.dragging = false;
            return (false, false);
        }
        let l = StartMenuLayout::compute(sw, sh);
        let sb = l.sb_rect();
        let in_sb = match ev {
            GuiEvent::MouseDown { x, y, .. } | GuiEvent::MouseMove { x, y } | GuiEvent::MouseUp { x, y, .. } => {
                x >= sb.x && x < sb.right() && y >= sb.y && y < sb.bottom()
            }
            _ => false,
        };
        if !in_sb && !self.start_scroll.dragging {
            return (false, false);
        }
        let (res, changed) = ScrollBar::handle_event(&mut self.start_scroll, sb, ScrollOrientation::Vertical, ev);
        if res.consumed() && matches!(ev, GuiEvent::MouseDown { .. }) {
            self.start_pending_power = None;
        }
        (res.consumed(), changed)
    }

    /// Handle mouse wheel scrolling inside an open Start Menu.
    pub fn handle_start_wheel(&mut self, x: i32, y: i32, dz: i32, sw: i32, sh: i32) -> bool {
        if !self.start_menu_open || dz == 0 {
            return false;
        }
        let (mx, my, mw, mh) = self.start_menu_rect(sw, sh);
        if x < mx || x >= mx + mw || y < my || y >= my + mh {
            return false;
        }
        self.start_pending_power = None;
        self.sync_start_scroll(sw, sh, false);
        let next = (self.start_scroll.value - dz * 2).clamp(0, self.start_scroll.max);
        self.start_scroll.value = next;
        true
    }

    /// Handle keyboard navigation, inline type-to-search, and two-step Escape in an open Start Menu.
    pub fn handle_start_key(&mut self, key: KeyCode, ch: Option<char>, sw: i32, sh: i32) -> TaskbarAction {
        if !self.start_menu_open {
            return TaskbarAction::None;
        }
        if key != KeyCode::Escape {
            self.start_pending_power = None;
        }

        let l = StartMenuLayout::compute(sw, sh);
        let entries = self.filtered_start_entries();
        let count = entries.len();
        if count == 0 {
            self.start_selected_index = 0;
        } else if self.start_selected_index >= count {
            self.start_selected_index = count - 1;
        }

        match key {
            KeyCode::Up => {
                if count > 0 {
                    self.start_selected_index = if self.start_selected_index == 0 {
                        count - 1
                    } else {
                        self.start_selected_index - 1
                    };
                    self.sync_start_scroll(sw, sh, true);
                }
                TaskbarAction::None
            }
            KeyCode::Down => {
                if count > 0 {
                    self.start_selected_index = (self.start_selected_index + 1) % count;
                    self.sync_start_scroll(sw, sh, true);
                }
                TaskbarAction::None
            }
            KeyCode::PageUp => {
                if count > 0 {
                    self.start_selected_index = self.start_selected_index.saturating_sub(l.visible_rows.max(1));
                    self.sync_start_scroll(sw, sh, true);
                }
                TaskbarAction::None
            }
            KeyCode::PageDown => {
                if count > 0 {
                    self.start_selected_index = (self.start_selected_index + l.visible_rows.max(1)).min(count - 1);
                    self.sync_start_scroll(sw, sh, true);
                }
                TaskbarAction::None
            }
            KeyCode::Home => {
                self.start_selected_index = 0;
                self.sync_start_scroll(sw, sh, true);
                TaskbarAction::None
            }
            KeyCode::End => {
                if count > 0 {
                    self.start_selected_index = count - 1;
                    self.sync_start_scroll(sw, sh, true);
                }
                TaskbarAction::None
            }
            KeyCode::Enter => {
                if let Some(entry) = entries.get(self.start_selected_index) {
                    let act = entry_to_taskbar_action(&entry.action);
                    self.set_start_menu_open(false);
                    act
                } else {
                    TaskbarAction::None
                }
            }
            KeyCode::Backspace => {
                if self.start_search_query.pop().is_some() {
                    self.start_selected_index = 0;
                    self.start_scroll.value = 0;
                    self.sync_start_scroll(sw, sh, true);
                }
                TaskbarAction::None
            }
            KeyCode::Escape => {
                if self.start_pending_power.take().is_some() {
                    return TaskbarAction::None;
                }
                if !self.start_search_query.is_empty() {
                    self.start_search_query.clear();
                    self.start_selected_index = 0;
                    self.start_scroll.value = 0;
                    self.sync_start_scroll(sw, sh, true);
                    return TaskbarAction::None;
                }
                self.set_start_menu_open(false);
                TaskbarAction::ToggleStartMenu
            }
            KeyCode::Tab | KeyCode::Left | KeyCode::Right | KeyCode::Delete | KeyCode::Function(_) => {
                TaskbarAction::None
            }
            _ => {
                let ascii = ch.or(match key {
                    KeyCode::Char(c) => Some(c),
                    _ => None,
                });
                if let Some(c) = ascii {
                    if (' '..='~').contains(&c) {
                        self.start_search_query.push(c);
                        self.start_selected_index = 0;
                        self.start_scroll.value = 0;
                        self.sync_start_scroll(sw, sh, true);
                    }
                }
                TaskbarAction::None
            }
        }
    }

    /// Process mouse clicks on Start Menu, Tray Popup, Quick Launch, Taskbar Tabs, or Start Button.
    pub fn handle_click(
        &mut self,
        x: i32,
        y: i32,
        windows: &[Window],
        sw: i32,
        sh: i32,
    ) -> TaskbarAction {
        let ty = sh - TASKBAR_HEIGHT;

        // 1. Check Responsive Start Menu Popup
        if self.start_menu_open {
            let l = StartMenuLayout::compute(sw, sh);
            if x >= l.mx && x < l.mx + l.mw && y >= l.my && y < l.my + l.mh {
                self.sync_start_scroll(sw, sh, false);
                let entries = self.filtered_start_entries();
                let row_w = l.row_w(self.start_scroll.max > 0);

                // Search clear `[x]` button
                let clr = l.search_clear_rect();
                if !self.start_search_query.is_empty()
                    && x >= clr.x && x < clr.right() && y >= clr.y && y < clr.bottom()
                {
                    self.start_pending_power = None;
                    self.start_search_query.clear();
                    self.start_selected_index = 0;
                    self.start_scroll.value = 0;
                    self.sync_start_scroll(sw, sh, true);
                    return TaskbarAction::None;
                }

                // Scrollable left program / search result rows
                if x >= l.list_x && x < l.list_x + row_w && y >= l.list_y && y < l.list_y + l.list_h {
                    let rel = ((y - l.list_y) / l.item_h) as usize;
                    let idx = (self.start_scroll.value.max(0) as usize) + rel;
                    if let Some(entry) = entries.get(idx) {
                        let act = entry_to_taskbar_action(&entry.action);
                        self.set_start_menu_open(false);
                        return act;
                    }
                    self.start_pending_power = None;
                    return TaskbarAction::None;
                }

                // Inline Search input box focus / click
                let sr = l.search_rect();
                if x >= sr.x && x < sr.right() && y >= sr.y && y < sr.bottom() {
                    self.start_pending_power = None;
                    return TaskbarAction::None;
                }

                // Right column: Places & System Tools
                for (i, item) in START_RIGHT_PLACES.iter().enumerate() {
                    let pr = l.place_rect(i);
                    if x >= pr.x && x < pr.right() && y >= pr.y && y < pr.bottom() {
                        self.set_start_menu_open(false);
                        if let RightMenuKind::Place(path, title) = item.kind {
                            return TaskbarAction::OpenFilesAt(path, title);
                        }
                    }
                }
                for (i, item) in START_RIGHT_TOOLS.iter().enumerate() {
                    let tr = l.tool_rect(i);
                    if x >= tr.x && x < tr.right() && y >= tr.y && y < tr.bottom() {
                        self.set_start_menu_open(false);
                        return match item.kind {
                            RightMenuKind::Display => TaskbarAction::OpenDisplaySettings,
                            RightMenuKind::NextWall => TaskbarAction::NextWallpaper,
                            RightMenuKind::Theme => TaskbarAction::ToggleTheme,
                            RightMenuKind::About => TaskbarAction::LaunchApp(DesktopAppId::About),
                            RightMenuKind::Place(p, t) => TaskbarAction::OpenFilesAt(p, t),
                        };
                    }
                }

                // Bottom footer: Shell / 2-step Restart / 2-step Shut Down
                let (shell_r, reboot_r, halt_r) = l.footer_power_rects();
                if x >= shell_r.x && x < shell_r.right() && y >= shell_r.y && y < shell_r.bottom() {
                    self.set_start_menu_open(false);
                    return TaskbarAction::ExitToShell;
                }
                if x >= reboot_r.x && x < reboot_r.right() && y >= reboot_r.y && y < reboot_r.bottom() {
                    if self.start_pending_power == Some(PendingPowerAction::Reboot) {
                        self.set_start_menu_open(false);
                        return TaskbarAction::RebootSystem;
                    }
                    self.start_pending_power = Some(PendingPowerAction::Reboot);
                    return TaskbarAction::None;
                }
                if x >= halt_r.x && x < halt_r.right() && y >= halt_r.y && y < halt_r.bottom() {
                    if self.start_pending_power == Some(PendingPowerAction::Halt) {
                        self.set_start_menu_open(false);
                        return TaskbarAction::HaltSystem;
                    }
                    self.start_pending_power = Some(PendingPowerAction::Halt);
                    return TaskbarAction::None;
                }

                self.start_pending_power = None;
                return TaskbarAction::None;
            } else if y >= ty && x >= 0 && x < 46 {
                self.set_start_menu_open(false);
                return TaskbarAction::ToggleStartMenu;
            } else {
                self.set_start_menu_open(false);
            }
        }

        // 2. Check Tray Status Popup
        if self.tray_popup_open {
            let pw = 282;
            let ph = 186;
            let px = sw - pw - 10;
            let py = sh - TASKBAR_HEIGHT - ph - 6;

            if x >= px && x <= px + pw && y >= py && y <= py + ph {
                let btn_y = py + 80;
                if y >= btn_y && y <= btn_y + 24 {
                    if x >= px + 12 && x <= px + 104 {
                        let _ = WIFI_MANAGER.lock().scan();
                        return TaskbarAction::None;
                    }
                    if x >= px + 112 && x <= px + 270 {
                        self.tray_popup_open = false;
                        return TaskbarAction::OpenNetworkSettings;
                    }
                }
                if y >= py + 148 && y <= py + 176 && x >= px + 12 && x <= px + pw - 12 {
                    let slider_w = (pw - 24).max(1);
                    let pct = (((x - (px + 12)).clamp(0, slider_w) * 100) / slider_w) as u8;
                    self.volume_level = pct.clamp(0, 100);
                    crate::drivers::audio::set_volume(self.volume_level);
                }
                return TaskbarAction::None;
            } else if !(x >= sw - (TRAY_W + PEEK_W) && y >= ty) {
                self.tray_popup_open = false;
            }
        }

        if y < ty {
            return TaskbarAction::None;
        }

        // 3. Far-Left Start Prism Button
        let start_w = 46;
        if x >= 0 && x < start_w {
            let next = !self.start_menu_open;
            self.set_start_menu_open(next);
            if next {
                self.sync_start_scroll(sw, sh, false);
            }
            return TaskbarAction::ToggleStartMenu;
        }

        // 4. Pinned Quick Icons
        let ql_x = start_w + 6;
        if x >= ql_x && x < ql_x + 32 {
            return TaskbarAction::LaunchApp(DesktopAppId::FileManager);
        }
        if x >= ql_x + 34 && x < ql_x + 66 {
            return TaskbarAction::LaunchApp(DesktopAppId::Terminal);
        }
        if x >= ql_x + 68 && x < ql_x + 100 {
            return TaskbarAction::LaunchApp(DesktopAppId::Studio);
        }

        // 5. Far-Right Peek / Show Desktop Sliver
        let peek_w = PEEK_W;
        if x >= sw - peek_w {
            return TaskbarAction::MinimizeAll;
        }

        // 6. Notification Tray click
        let tray_w = TRAY_W;
        let tray_x = sw - tray_w - peek_w;
        if x >= tray_x {
            self.tray_popup_open = !self.tray_popup_open;
            return TaskbarAction::None;
        }

        // 7. Taskbar Window Tabs
        let sep_x = ql_x + 3 * 34 + 4;
        let tabs_start_x = sep_x + 8;
        let tabs_avail_w = (tray_x - tabs_start_x - 8).max(80);
        if !windows.is_empty() {
            let count = windows.len() as i32;
            let tab_w = ((tabs_avail_w / count) - 3).clamp(76, 176);
            let mut tx = tabs_start_x;

            for w in windows.iter() {
                if tx + tab_w > tray_x - 4 {
                    break;
                }
                if x >= tx && x < tx + tab_w {
                    return TaskbarAction::FocusWindow(w.id);
                }
                tx += tab_w + 3;
            }
        }

        TaskbarAction::None
    }
}



fn push_2digits(s: &mut String, val: u8) {
    s.push(((val / 10) % 10 + b'0') as char);
    s.push((val % 10 + b'0') as char);
}

fn push_u64(s: &mut String, mut val: u64) {
    if val == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut idx = 0;
    while val > 0 {
        buf[idx] = ((val % 10) as u8) + b'0';
        val /= 10;
        idx += 1;
    }
    while idx > 0 {
        idx -= 1;
        s.push(buf[idx] as char);
    }
}

fn matches_query(q_lower: &str, fields: &[&str]) -> bool {
    fields.iter().any(|f| f.to_ascii_lowercase().contains(q_lower))
}

fn entry_to_taskbar_action(act: &StartEntryAction) -> TaskbarAction {
    match act {
        StartEntryAction::LaunchApp(id) => TaskbarAction::LaunchApp(*id),
        StartEntryAction::LaunchPath(p) => TaskbarAction::LaunchPath(p.clone()),
        StartEntryAction::OpenFilesAt(path, title) => TaskbarAction::OpenFilesAt(path, title),
        StartEntryAction::OpenDisplaySettings => TaskbarAction::OpenDisplaySettings,
        StartEntryAction::NextWallpaper => TaskbarAction::NextWallpaper,
        StartEntryAction::ToggleTheme => TaskbarAction::ToggleTheme,
        StartEntryAction::OpenSpotlight(q) => TaskbarAction::OpenSpotlight(q.clone()),
    }
}

fn draw_place_tool_icon(canvas: &mut Canvas, x: i32, y: i32, kind: PlaceToolIcon, accent: Color, fg: Color) {
    match kind {
        PlaceToolIcon::Computer => {
            canvas.fill_rect(x + 1, y + 1, 12, 9, Color::rgb(35, 65, 110));
            canvas.draw_rect(x + 1, y + 1, 12, 9, fg);
            canvas.fill_rect(x + 5, y + 10, 4, 2, fg);
            canvas.fill_rect(x + 3, y + 12, 8, 1, fg);
        }
        PlaceToolIcon::Documents => {
            canvas.fill_rect(x + 1, y + 2, 5, 2, accent);
            canvas.fill_rect(x + 1, y + 4, 12, 8, accent);
            canvas.fill_rect(x + 3, y + 6, 8, 4, Color::WHITE);
        }
        PlaceToolIcon::Music => {
            canvas.fill_rect(x + 4, y + 2, 7, 2, accent);
            canvas.fill_rect(x + 4, y + 2, 2, 8, accent);
            canvas.fill_rect(x + 9, y + 2, 2, 7, accent);
            canvas.fill_rect(x + 2, y + 9, 4, 3, fg);
            canvas.fill_rect(x + 7, y + 8, 4, 3, fg);
        }
        PlaceToolIcon::Pictures => {
            canvas.fill_rect(x + 1, y + 2, 12, 10, Color::rgb(46, 130, 196));
            canvas.draw_rect(x + 1, y + 2, 12, 10, fg);
            canvas.fill_rect(x + 3, y + 8, 8, 3, Color::rgb(96, 200, 110));
            canvas.fill_rect(x + 9, y + 4, 2, 2, Color::rgb(255, 225, 95));
        }
        PlaceToolIcon::Downloads => {
            canvas.fill_rect(x + 6, y + 2, 2, 6, accent);
            canvas.draw_line(x + 4, y + 6, x + 7, y + 9, accent);
            canvas.draw_line(x + 10, y + 6, x + 7, y + 9, accent);
            canvas.fill_rect(x + 2, y + 11, 10, 2, fg);
        }
        PlaceToolIcon::Trash => {
            canvas.fill_rect(x + 4, y + 2, 6, 1, fg);
            canvas.fill_rect(x + 2, y + 3, 10, 2, fg);
            canvas.draw_rect(x + 3, y + 5, 8, 8, fg);
            canvas.fill_rect(x + 5, y + 7, 1, 4, fg);
            canvas.fill_rect(x + 8, y + 7, 1, 4, fg);
        }
        PlaceToolIcon::Personalize => {
            canvas.fill_rect(x + 2, y + 2, 10, 10, Color::rgb(138, 65, 175));
            canvas.draw_rect(x + 2, y + 2, 10, 10, fg);
            canvas.fill_rect(x + 4, y + 4, 3, 3, Color::rgb(255, 200, 70));
            canvas.fill_rect(x + 7, y + 7, 3, 3, Color::rgb(90, 210, 255));
        }
        PlaceToolIcon::Wallpaper => {
            canvas.fill_rect(x + 1, y + 2, 12, 10, Color::rgb(28, 110, 155));
            canvas.draw_rect(x + 1, y + 2, 12, 10, fg);
            canvas.draw_line(x + 5, y + 7, x + 10, y + 7, Color::WHITE);
            canvas.draw_line(x + 8, y + 5, x + 10, y + 7, Color::WHITE);
            canvas.draw_line(x + 8, y + 9, x + 10, y + 7, Color::WHITE);
        }
        PlaceToolIcon::ThemeMode => {
            canvas.draw_circle(x + 7, y + 7, 5, fg);
            canvas.fill_rect(x + 7, y + 3, 4, 8, accent);
        }
        PlaceToolIcon::About => {
            canvas.fill_rect(x + 2, y + 2, 10, 10, accent);
            canvas.fill_rect(x + 6, y + 4, 2, 2, Color::WHITE);
            canvas.fill_rect(x + 6, y + 7, 2, 4, Color::WHITE);
        }
    }
}
