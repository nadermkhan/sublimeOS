pub mod apps;
pub mod boot_splash;
pub mod canvas;
pub mod color;
pub mod desktop;
pub mod font;
pub mod mouse;
pub mod spotlight;
pub mod taskbar;
pub mod wallpaper;
pub mod window;

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use crate::sync::IrqSafeSpinlock;
use crate::arch::x86_64::serial::read_rx_char;
use crate::drivers::keyboard::read_char;
use crate::drivers::mouse::MOUSE;
use crate::drivers::vbe::{self, VBE};
use crate::gui::apps::settings::SettingsTab;
use crate::gui::apps::sysmon::SysmonAction;
use crate::gui::apps::{AboutApp, CalcApp, FileManagerApp, SettingsApp, SysmonApp, TerminalApp, TextEditApp};
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::desktop::{Desktop, DesktopAppId, DesktopMenuAction};
use crate::gui::mouse::{draw_cursor, CursorType};
use crate::gui::spotlight::{Spotlight, SpotlightTarget};
use crate::gui::taskbar::{Taskbar, TaskbarAction};
use crate::gui::window::{RemoteSurface, ResizeEdge, Window, WindowApp, FRAME_BORDER, TITLEBAR_HEIGHT};
use crate::kinfo;

static DESKTOP_ACTIVE: AtomicBool = AtomicBool::new(false);
static PENDING_OPEN_PATHS: IrqSafeSpinlock<Vec<String>> = IrqSafeSpinlock::new(Vec::new());

/// Return `true` while the graphical Desktop compositor event loop is active.
pub fn is_desktop_running() -> bool {
    DESKTOP_ACTIVE.load(Ordering::Acquire)
}

/// Queue a `.app` bundle or file path to be opened on the next compositor frame.
pub fn queue_open_path(path: &str) {
    let trimmed = path.trim();
    if !trimmed.is_empty() {
        PENDING_OPEN_PATHS.lock().push(String::from(trimmed));
    }
}

fn drain_pending_open_paths() -> Vec<String> {
    let mut q = PENDING_OPEN_PATHS.lock();
    core::mem::take(&mut *q)
}

/// Launch a Ring-3 ELF binary from the VFS in the background so it can run alongside the compositor.
pub fn spawn_user_binary(raw_path: &str, args: &[&str]) -> bool {
    let resolved = crate::user::apps::resolve_executable_path(raw_path, "/")
        .unwrap_or_else(|| String::from(raw_path));
    let Some(buf) = crate::user::apps::read_vfs_binary(&resolved) else {
        return false;
    };
    crate::task::user::spawn_user_process(&buf, args, false).is_ok()
}

struct RemoteClient {
    sock_id: usize,
    rx_buf: Vec<u8>,
    owner_pid: Option<u32>,
}

pub struct DesktopManager {
    pub canvas: Canvas,
    pub desktop: Desktop,
    pub taskbar: Taskbar,
    pub spotlight: Spotlight,
    pub windows: Vec<Window>,
    next_window_id: u32,
    pub prev_left_down: bool,
    pub mouse_x: i32,
    pub mouse_y: i32,
    pub damage: libgui::system::DamageTracker,
    cursor_dirty: bool,
    last_sec: u8,
    last_blink: u64,
    cursor_bg: [u32; 32 * 32],
    saved_cursor_box: Option<(i32, i32, i32, i32)>,
    wm_listen_sock_id: Option<usize>,
    remote_clients: Vec<RemoteClient>,
    last_sent_theme: u8,
    last_sent_seed: u32,
    last_hover_win_id: Option<u32>,
    last_wall_gen: u64,
    last_clipboard: alloc::string::String,
    last_titlebar_click: Option<(u32, u64)>,
    pub window_menu: Option<(i32, i32, u32)>,
}

impl DesktopManager {
    pub fn new(width: u32, height: u32) -> Self {
        crate::gui::wallpaper::apply_current_wallpaper_palette();
        Self {
            canvas: Canvas::new(width, height),
            desktop: Desktop::new(),
            taskbar: Taskbar::new(),
            spotlight: Spotlight::new(),
            windows: Vec::new(),
            next_window_id: 1,
            prev_left_down: false,
            mouse_x: (width / 2) as i32,
            mouse_y: (height / 2) as i32,
            damage: {
                let mut d = libgui::system::DamageTracker::new();
                d.invalidate_all();
                d
            },
            cursor_dirty: true,
            last_sec: 255,
            last_blink: 0,
            cursor_bg: [0; 32 * 32],
            saved_cursor_box: None,
            wm_listen_sock_id: None,
            remote_clients: Vec::new(),
            last_sent_theme: libgui::gfx::current_theme_kind() as u8,
            last_sent_seed: libgui::gfx::current_wallpaper_seed_raw(),
            last_hover_win_id: None,
            last_wall_gen: crate::gui::wallpaper::wallpaper_generation(),
            last_clipboard: alloc::string::String::new(),
            last_titlebar_click: None,
            window_menu: None,
        }
    }

    #[inline]
    fn screen_rect(&self) -> libgui::gfx::Rect {
        libgui::gfx::Rect::new(0, 0, self.canvas.width as i32, self.canvas.height as i32)
    }

    #[inline]
    fn taskbar_rect(&self) -> libgui::gfx::Rect {
        let sw = self.canvas.width as i32;
        let sh = self.canvas.height as i32;
        let th = crate::gui::taskbar::TASKBAR_HEIGHT;
        libgui::gfx::Rect::new(0, (sh - th).max(0), sw, th)
    }

    #[inline]
    fn win_outer_rect(w: &Window) -> libgui::gfx::Rect {
        libgui::gfx::Rect::new(w.x - 4, w.y - 4, w.width + 8, w.height + 8)
    }

    #[inline]
    fn win_inner_rect(w: &Window) -> libgui::gfx::Rect {
        let (ix, iy, iw, ih) = w.inner_bounds();
        libgui::gfx::Rect::new(ix, iy, iw, ih)
    }

    #[inline]
    pub fn invalidate_all(&mut self) {
        self.damage.invalidate_all();
    }

    #[inline]
    pub fn invalidate_rect(&mut self, r: libgui::gfx::Rect) {
        self.damage.invalidate(r);
    }

    pub fn open_app(&mut self, app_id: DesktopAppId) {
        if app_id == DesktopAppId::Studio {
            for i in 0..self.windows.len() {
                if self.windows[i].app.app_id() == DesktopAppId::RemoteGui
                    && self.windows[i].title.contains("Studio")
                {
                    let win_id = self.windows[i].id;
                    self.windows[i].is_minimized = false;
                    self.bring_to_front(win_id);
                    return;
                }
            }
            if spawn_user_binary("/bin/studio", &["studio"]) {
                kinfo!("Compositor launched Ring-3 client: /bin/studio");
            }
            return;
        }
        if app_id == DesktopAppId::RemoteGui {
            return;
        }

        // If window already open, unminimize it, bring to front, and activate
        for i in 0..self.windows.len() {
            if self.windows[i].app.app_id() == app_id {
                let win_id = self.windows[i].id;
                self.windows[i].is_minimized = false;
                self.bring_to_front(win_id);
                return;
            }
        }

        // Otherwise open a new window
        let id = self.next_window_id;
        self.next_window_id += 1;

        let win = match app_id {
            DesktopAppId::Terminal => Window::new(
                id,
                "Terminal - SublimeOS",
                155,
                40,
                580,
                380,
                WindowApp::Terminal(TerminalApp::new()),
            ),
            DesktopAppId::Sysmon => Window::new(
                id,
                "Task Manager",
                420,
                110,
                550,
                380,
                WindowApp::Sysmon(SysmonApp::new()),
            ),
            DesktopAppId::FileManager => Window::new(
                id,
                "My Computer",
                150,
                85,
                590,
                390,
                WindowApp::Files(FileManagerApp::new()),
            ),
            DesktopAppId::About => Window::new(
                id,
                "About SublimeOS",
                260,
                90,
                480,
                320,
                WindowApp::About(AboutApp::new()),
            ),
            DesktopAppId::Calculator => Window::new(
                id,
                "Calculator",
                440,
                80,
                300,
                350,
                WindowApp::Calc(CalcApp::new()),
            ),
            DesktopAppId::TextEdit => Window::new(
                id,
                "untitled.txt - Notepad",
                180,
                70,
                530,
                360,
                WindowApp::Editor(TextEditApp::new()),
            ),
            DesktopAppId::Settings => Window::new(
                id,
                "Control Panel",
                200,
                80,
                560,
                380,
                WindowApp::Settings(SettingsApp::new()),
            ),
            DesktopAppId::Studio | DesktopAppId::RemoteGui => return,
        };

        // Deactivate other windows and invalidate changed windows + new window rect
        let new_rect = Self::win_outer_rect(&win);
        for w in self.windows.iter_mut() {
            if w.is_active {
                w.deactivate();
                self.damage.invalidate(Self::win_outer_rect(w));
            }
        }

        self.windows.push(win);
        self.damage.invalidate(new_rect);
        let tb = self.taskbar_rect();
        self.damage.invalidate(tb);
    }

    pub fn open_settings_tab(&mut self, tab: SettingsTab) {
        self.open_app(DesktopAppId::Settings);
        let mut rect = None;
        for w in self.windows.iter_mut() {
            if let WindowApp::Settings(ref mut s) = w.app {
                s.set_tab(tab);
                rect = Some(Self::win_outer_rect(w));
                break;
            }
        }
        if let Some(r) = rect {
            self.damage.invalidate(r);
        }
    }

    pub fn open_files_at(&mut self, path: &str, title: &str) {
        self.open_app(DesktopAppId::FileManager);
        let mut rect = None;
        for w in self.windows.iter_mut() {
            if let WindowApp::Files(ref mut files) = w.app {
                files.navigate_to(path);
                w.title = alloc::string::String::from(title);
                files.pending_title = None;
                rect = Some(Self::win_outer_rect(w));
                break;
            }
        }
        if let Some(r) = rect {
            self.damage.invalidate(r);
        }
        let tb = self.taskbar_rect();
        self.damage.invalidate(tb);
    }

    fn drain_files_effects(&mut self, win_id: u32) {
        let mut open_path: Option<alloc::string::String> = None;
        let mut title_dmg: Option<libgui::gfx::Rect> = None;
        if let Some(w) = self.windows.iter_mut().find(|w| w.id == win_id) {
            if let WindowApp::Files(ref mut files) = w.app {
                open_path = files.pending_open_file.take();
                if let Some(new_title) = files.pending_title.take() {
                    if w.title != new_title {
                        w.title = new_title;
                        title_dmg = Some(libgui::gfx::Rect::new(w.x, w.y, w.width, TITLEBAR_HEIGHT + 2));
                    }
                }
            }
        }
        if let Some(r) = title_dmg {
            self.damage.invalidate(r);
            let tb = self.taskbar_rect();
            self.damage.invalidate(tb);
        }
        if let Some(path) = open_path {
            self.open_editor_file(&path);
        }
    }

    fn dismiss_all_window_popups(&mut self) {
        for i in 0..self.windows.len() {
            self.windows[i].dismiss_popups();
            let r = Self::win_outer_rect(&self.windows[i]);
            self.damage.invalidate(r);
        }
    }

    pub fn open_terminal_with_command(&mut self, cmd_line: &str) {
        self.open_app(DesktopAppId::Terminal);
        let mut rect = None;
        for w in self.windows.iter_mut() {
            if let WindowApp::Terminal(ref mut term) = w.app {
                term.lines.push((alloc::format!("{}{}", term.prompt_prefix(), cmd_line), Color::rgb(120, 220, 255)));
                term.execute_command(cmd_line);
                term.scroll_offset = 0;
                rect = Some(Self::win_outer_rect(w));
                break;
            }
        }
        if let Some(r) = rect {
            self.damage.invalidate(r);
        }
        let tb = self.taskbar_rect();
        self.damage.invalidate(tb);
    }

    fn dispatch_spotlight_target(&mut self, target: SpotlightTarget) {
        match target {
            SpotlightTarget::LaunchApp(app_id) => self.open_app(app_id),
            SpotlightTarget::LaunchPath(path) => self.open_editor_file(&path),
            SpotlightTarget::Calculation(_) => {}
        }
    }

    pub fn open_editor_file(&mut self, path: &str) {
        if path.ends_with(".app") {
            if let Some(manifest) = crate::user::apps::read_app_manifest(path) {
                if let Some(app_id) = manifest.builtin {
                    self.open_app(app_id);
                    return;
                }
                if !manifest.exec.is_empty() {
                    let is_gui = !manifest.terminal && crate::user::apps::elf_is_gui_client(&manifest.exec);
                    if is_gui {
                        let mut arg_refs: Vec<&str> = Vec::with_capacity(1 + manifest.args.len());
                        arg_refs.push(&manifest.exec);
                        for a in &manifest.args {
                            arg_refs.push(a.as_str());
                        }
                        if spawn_user_binary(&manifest.exec, &arg_refs) {
                            kinfo!("Compositor launched .app Ring-3 GUI binary: {}", manifest.exec);
                            return;
                        }
                    } else {
                        let mut cmd_line = manifest.exec.clone();
                        for a in &manifest.args {
                            cmd_line.push(' ');
                            cmd_line.push_str(a);
                        }
                        self.open_terminal_with_command(&cmd_line);
                        return;
                    }
                }
            }
        }

        if crate::user::apps::is_elf64_file(path) {
            if crate::user::apps::elf_is_gui_client(path) {
                if spawn_user_binary(path, &[path]) {
                    kinfo!("Compositor launched Ring-3 ELF GUI client: {}", path);
                }
            } else {
                self.open_terminal_with_command(path);
            }
            return;
        }

        if libgui::codecs::is_audio_path(path) {
            if spawn_user_binary("/bin/music", &["/bin/music", path]) {
                kinfo!("Compositor opened audio in Music Player: {}", path);
                return;
            }
            if spawn_user_binary("/bin/player", &["/bin/player", path]) {
                kinfo!("Compositor opened audio in Media Player: {}", path);
                return;
            }
        }

        if libgui::codecs::is_video_path(path) {
            if spawn_user_binary("/bin/player", &["/bin/player", path]) {
                kinfo!("Compositor opened video in Media Player: {}", path);
                return;
            }
        }

        if libgui::codecs::is_code_path(path) {
            if spawn_user_binary("/bin/code", &["/bin/code", path]) {
                kinfo!("Compositor opened source file in Code Editor: {}", path);
                return;
            }
        }

        self.open_app(DesktopAppId::TextEdit);
        let mut rect = None;
        let short_name = path.rsplit('/').next().unwrap_or(path);
        for w in self.windows.iter_mut() {
            if let WindowApp::Editor(ref mut ed) = w.app {
                ed.open_file(path);
                w.title = alloc::format!("{} - Notepad", short_name);
                rect = Some(Self::win_outer_rect(w));
                break;
            }
        }
        if let Some(r) = rect {
            self.damage.invalidate(r);
        }
        let tb = self.taskbar_rect();
        self.damage.invalidate(tb);
    }

    fn toggle_theme(&mut self) {
        let next = match libgui::gfx::current_theme_kind() {
            libgui::gfx::ThemeKind::SublimeClassic => libgui::gfx::ThemeKind::SublimeSlate,
            libgui::gfx::ThemeKind::SublimeSlate => libgui::gfx::ThemeKind::SublimeClassic,
        };
        libgui::gfx::set_theme_kind(next);
        self.invalidate_all();
    }

    fn apply_taskbar_action(&mut self, action: TaskbarAction, sw: i32, sh: i32) -> TaskbarAction {
        match action {
            TaskbarAction::FocusWindow(win_id) => {
                let mut minimized_r = None;
                for w in self.windows.iter_mut() {
                    if w.id == win_id && w.is_active && !w.is_minimized {
                        w.is_minimized = true;
                        w.deactivate();
                        minimized_r = Some(Self::win_outer_rect(w));
                        break;
                    }
                }
                if let Some(r) = minimized_r {
                    self.damage.invalidate(r);
                    let tb = self.taskbar_rect();
                    self.damage.invalidate(tb);
                } else {
                    self.bring_to_front(win_id);
                }
                TaskbarAction::None
            }
            TaskbarAction::RestoreWindow(win_id) => {
                self.bring_to_front(win_id);
                TaskbarAction::None
            }
            TaskbarAction::MinimizeWindow(win_id) => {
                let mut dmg = None;
                if let Some(w) = self.windows.iter_mut().find(|w| w.id == win_id) {
                    dmg = Some(Self::win_outer_rect(w));
                    w.is_minimized = true;
                    w.deactivate();
                }
                if let Some(r) = dmg {
                    self.damage.invalidate(r);
                    let tb = self.taskbar_rect();
                    self.damage.invalidate(tb);
                }
                TaskbarAction::None
            }
            TaskbarAction::MaximizeWindow(win_id) => {
                self.bring_to_front(win_id);
                let mut dmg = None;
                if let Some(w) = self.windows.iter_mut().find(|w| w.id == win_id) {
                    let old_r = Self::win_outer_rect(w);
                    w.dismiss_popups();
                    w.toggle_maximize(sw, sh);
                    let new_r = Self::win_outer_rect(w);
                    dmg = Some(old_r.union(new_r));
                }
                if let Some(r) = dmg {
                    self.damage.invalidate(r);
                }
                TaskbarAction::None
            }
            TaskbarAction::CloseWindow(win_id) => {
                self.close_window(win_id);
                TaskbarAction::None
            }
            TaskbarAction::MinimizeAll => {
                for w in self.windows.iter_mut() {
                    if !w.is_minimized {
                        self.damage.invalidate(Self::win_outer_rect(w));
                    }
                    w.is_minimized = true;
                    w.deactivate();
                }
                let tb = self.taskbar_rect();
                self.damage.invalidate(tb);
                TaskbarAction::None
            }
            TaskbarAction::LaunchApp(app_id) => {
                self.open_app(app_id);
                TaskbarAction::None
            }
            TaskbarAction::LaunchPath(path) => {
                self.open_editor_file(&path);
                TaskbarAction::None
            }
            TaskbarAction::OpenFilesAt(path, title) => {
                self.open_files_at(path, title);
                TaskbarAction::None
            }
            TaskbarAction::NextWallpaper => {
                let _ = crate::gui::wallpaper::cycle_wallpaper();
                self.invalidate_all();
                TaskbarAction::None
            }
            TaskbarAction::OpenNetworkSettings => {
                self.open_settings_tab(SettingsTab::Network);
                TaskbarAction::None
            }
            TaskbarAction::OpenDisplaySettings => {
                self.open_settings_tab(SettingsTab::Display);
                TaskbarAction::None
            }
            TaskbarAction::ToggleStartMenu => {
                if self.taskbar.start_menu_open {
                    self.taskbar.sync_start_scroll(sw, sh, false);
                }
                self.dismiss_all_window_popups();
                self.invalidate_all();
                TaskbarAction::None
            }
            TaskbarAction::ToggleSpotlight => {
                self.dismiss_all_window_popups();
                self.spotlight.toggle();
                self.invalidate_all();
                TaskbarAction::None
            }
            TaskbarAction::OpenSpotlight(query) => {
                self.dismiss_all_window_popups();
                self.spotlight.is_open = true;
                self.spotlight.query = query;
                self.spotlight.selected_index = 0;
                self.invalidate_all();
                TaskbarAction::None
            }
            TaskbarAction::ToggleTheme => {
                self.toggle_theme();
                TaskbarAction::None
            }
            TaskbarAction::ExitToShell | TaskbarAction::RebootSystem | TaskbarAction::HaltSystem => action,
            TaskbarAction::None => TaskbarAction::None,
        }
    }

    fn draw_window_menu(canvas: &mut Canvas, menu: Option<(i32, i32, u32)>, mouse_x: i32, mouse_y: i32) {
        let Some((mx, my, _wid)) = menu else {
            return;
        };
        let mw = 144;
        let mh = 100;
        let theme = libgui::gfx::current_theme();
        canvas.fill_rect(mx + 2, my + 2, mw, mh, crate::gui::color::Color::argb(40, 0, 0, 0));
        canvas.fill_rect(mx, my, mw, mh, crate::gui::color::Color::from_libgui(theme.face_light));
        canvas.draw_rect(mx, my, mw, mh, crate::gui::color::Color::from_libgui(theme.ctrl_border));
        let labels = ["Restore", "Minimize", "Maximize", "Close"];
        for (i, lbl) in labels.iter().enumerate() {
            let ry = my + 2 + i as i32 * 24;
            let hover = mouse_x >= mx + 2 && mouse_x < mx + mw - 2 && mouse_y >= ry && mouse_y < ry + 22;
            if hover {
                canvas.fill_rect(mx + 2, ry, mw - 4, 22, crate::gui::color::Color::from_libgui(theme.selection));
            }
            let col = if hover {
                crate::gui::color::Color::from_libgui(theme.selection_text)
            } else {
                crate::gui::color::Color::from_libgui(theme.text_primary)
            };
            crate::gui::font::draw_ui_string(canvas, mx + 10, ry + 3, lbl, col);
        }
    }

    pub fn cycle_active_window(&mut self) {
        if self.windows.is_empty() {
            return;
        }
        if self.windows.len() == 1 {
            let id = self.windows[0].id;
            self.windows[0].is_minimized = false;
            self.bring_to_front(id);
            return;
        }
        let next_id = self.windows[0].id;
        self.windows[0].is_minimized = false;
        self.bring_to_front(next_id);
    }

    pub fn bring_to_front(&mut self, window_id: u32) {
        let mut target_idx = None;
        for (idx, w) in self.windows.iter().enumerate() {
            if w.id == window_id {
                target_idx = Some(idx);
                break;
            }
        }

        if let Some(idx) = target_idx {
            let mut win = self.windows.remove(idx);
            let was_active = win.is_active;
            for w in self.windows.iter_mut() {
                if w.is_active {
                    w.deactivate();
                    self.damage.invalidate(Self::win_outer_rect(w));
                }
            }
            win.is_active = true;
            win.is_minimized = false;
            if !was_active {
                if let WindowApp::Remote(ref rem) = win.app {
                    rem.send_input(libgui::event::GuiEvent::FocusChanged { focused: true });
                }
            }
            let win_r = Self::win_outer_rect(&win);
            self.windows.push(win);
            self.damage.invalidate(win_r);
            let tb = self.taskbar_rect();
            self.damage.invalidate(tb);
        }
    }

    pub fn close_window(&mut self, window_id: u32) {
        if let Some(idx) = self.windows.iter().position(|w| w.id == window_id) {
            let closed_r = Self::win_outer_rect(&self.windows[idx]);
            if let WindowApp::Remote(ref rem) = self.windows[idx].app {
                rem.send_server_msg(&libgui::ipc::WmServerMsg::CloseRequested {
                    window_id: rem.remote_win_id,
                });
            }
            self.windows.remove(idx);
            self.damage.invalidate(closed_r);
            if let Some(last) = self.windows.last_mut() {
                last.is_active = true;
                if let WindowApp::Remote(ref rem) = last.app {
                    rem.send_input(libgui::event::GuiEvent::FocusChanged { focused: true });
                }
                let lr = libgui::gfx::Rect::new(last.x, last.y, last.width, TITLEBAR_HEIGHT + 2);
                self.damage.invalidate(lr);
            }
            let tb = self.taskbar_rect();
            self.damage.invalidate(tb);
        }
    }

    pub fn poll_remote_clients(&mut self) {
        // 1. Accept any waiting AF_UNIX /run/wm.sock connections
        if let Some(listen_id) = self.wm_listen_sock_id {
            let theme_now = libgui::gfx::current_theme_kind() as u8;
            let seed_now = libgui::gfx::current_wallpaper_seed_raw();
            for _ in 0..8 {
                match crate::net::unix::unix_socket_accept(listen_id) {
                    Ok(sock_id) => {
                        let welcome = libgui::ipc::encode_server_msg(&libgui::ipc::WmServerMsg::Welcome {
                            client_id: sock_id as u32,
                            theme_kind: theme_now,
                        });
                        let _ = crate::net::unix::unix_socket_send(sock_id, &welcome);
                        let palette = libgui::ipc::encode_server_msg(&libgui::ipc::WmServerMsg::PaletteChanged {
                            theme_kind: theme_now,
                            seed_rgb: seed_now,
                        });
                        let _ = crate::net::unix::unix_socket_send(sock_id, &palette);
                        self.remote_clients.push(RemoteClient {
                            sock_id,
                            rx_buf: Vec::with_capacity(1024),
                            owner_pid: None,
                        });
                    }
                    Err(_) => break,
                }
            }
        }

        // 2. Broadcast live palette/theme changes & kernel-originated clipboard updates to Ring-3 clients
        let cur_theme = libgui::gfx::current_theme_kind() as u8;
        let cur_seed = libgui::gfx::current_wallpaper_seed_raw();
        if cur_theme != self.last_sent_theme || cur_seed != self.last_sent_seed {
            self.last_sent_theme = cur_theme;
            self.last_sent_seed = cur_seed;
            self.invalidate_all();
            let bytes = libgui::ipc::encode_server_msg(&libgui::ipc::WmServerMsg::PaletteChanged {
                theme_kind: cur_theme,
                seed_rgb: cur_seed,
            });
            for c in &self.remote_clients {
                let _ = crate::net::unix::unix_socket_send(c.sock_id, &bytes);
            }
        }
        let cur_clip = libgui::system::SystemClipboard::get_text();
        if cur_clip != self.last_clipboard {
            self.last_clipboard = cur_clip.clone();
            let bytes = libgui::ipc::encode_server_msg(&libgui::ipc::WmServerMsg::ClipboardChanged {
                text: cur_clip,
            });
            for c in &self.remote_clients {
                let _ = crate::net::unix::unix_socket_send(c.sock_id, &bytes);
            }
        }

        // 3. Push Configure notifications when any remote window changes size
        for w in self.windows.iter_mut() {
            let (_, _, iw, ih) = w.inner_bounds();
            if let WindowApp::Remote(ref mut rem) = w.app {
                rem.sync_configure(iw, ih, cur_theme);
            }
        }

        // 4. Receive and dispatch frames from connected clients
        let mut disconnected: Vec<usize> = Vec::new();
        let mut pending_msgs: Vec<(usize, libgui::ipc::WmClientMsg)> = Vec::new();
        let mut tmp = [0u8; 1024];

        for c in self.remote_clients.iter_mut() {
            loop {
                match crate::net::unix::unix_socket_recv(c.sock_id, &mut tmp) {
                    Ok(0) => {
                        disconnected.push(c.sock_id);
                        break;
                    }
                    Ok(n) => {
                        c.rx_buf.extend_from_slice(&tmp[..n]);
                        if n < tmp.len() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            while let Some(frame) = libgui::ipc::take_next_frame(&mut c.rx_buf) {
                if let Some(msg) = libgui::ipc::decode_client_msg(&frame) {
                    pending_msgs.push((c.sock_id, msg));
                }
            }
        }

        for (sock_id, msg) in pending_msgs {
            match msg {
                libgui::ipc::WmClientMsg::Hello { pid, app_name: _ } => {
                    for c in self.remote_clients.iter_mut() {
                        if c.sock_id == sock_id {
                            c.owner_pid = Some(pid);
                        }
                    }
                    for w in self.windows.iter_mut() {
                        if let WindowApp::Remote(ref mut rem) = w.app {
                            if rem.client_sock_id == sock_id {
                                rem.owner_pid = Some(pid);
                            }
                        }
                    }
                }
                libgui::ipc::WmClientMsg::CreateWindow {
                    title,
                    width,
                    height,
                    min_width,
                    min_height,
                    resizable: _,
                } => {
                    let id = self.next_window_id;
                    self.next_window_id += 1;
                    let sw = (self.canvas.width as i32).max(640);
                    let sh = (self.canvas.height as i32).max(480);
                    let client_w = width.clamp(160, sw - 40);
                    let client_h = height.clamp(120, sh - 80);
                    let outer_w = client_w + FRAME_BORDER * 2;
                    let outer_h = client_h + TITLEBAR_HEIGHT + FRAME_BORDER;
                    let stagger = ((id as i32) % 5) * 26;
                    let wx = (140 + stagger).min(sw - outer_w - 8).max(8);
                    let wy = (64 + stagger).min(sh - outer_h - 36).max(8);
                    let owner_pid = self
                        .remote_clients
                        .iter()
                        .find(|c| c.sock_id == sock_id)
                        .and_then(|c| c.owner_pid);

                    let rem = RemoteSurface {
                        client_sock_id: sock_id,
                        remote_win_id: id,
                        owner_pid,
                        shm_name: alloc::string::String::new(),
                        shm_width: 0,
                        shm_height: 0,
                        last_sent_w: client_w,
                        last_sent_h: client_h,
                    };
                    let mut win = Window::new(id, &title, wx, wy, outer_w, outer_h, WindowApp::Remote(rem));
                    win.min_width = min_width.max(160) + FRAME_BORDER * 2;
                    win.min_height = min_height.max(120) + TITLEBAR_HEIGHT + FRAME_BORDER;
                    for w in self.windows.iter_mut() {
                        if w.is_active {
                            w.deactivate();
                            self.damage.invalidate(Self::win_outer_rect(w));
                        }
                    }
                    let cfg = libgui::ipc::encode_server_msg(&libgui::ipc::WmServerMsg::Configure {
                        window_id: id,
                        width: client_w,
                        height: client_h,
                        theme_kind: cur_theme,
                    });
                    let _ = crate::net::unix::unix_socket_send(sock_id, &cfg);
                    let new_r = Self::win_outer_rect(&win);
                    self.windows.push(win);
                    self.damage.invalidate(new_r);
                    let tb = self.taskbar_rect();
                    self.damage.invalidate(tb);
                }
                libgui::ipc::WmClientMsg::AttachShm {
                    window_id,
                    width,
                    height,
                    stride_bytes: _,
                    shm_name,
                } => {
                    let mut dmg_r = None;
                    if let Some(w) = self.windows.iter_mut().find(|w| w.id == window_id) {
                        if let WindowApp::Remote(ref mut rem) = w.app {
                            if rem.client_sock_id == sock_id {
                                rem.shm_name = shm_name;
                                rem.shm_width = width;
                                rem.shm_height = height;
                                dmg_r = Some(Self::win_inner_rect(w));
                            }
                        }
                    }
                    if let Some(r) = dmg_r {
                        self.damage.invalidate(r);
                    }
                }
                libgui::ipc::WmClientMsg::CommitDamage {
                    window_id,
                    x,
                    y,
                    width,
                    height,
                } => {
                    if let Some(w) = self.windows.iter().find(|w| w.id == window_id && !w.is_minimized) {
                        let inner = Self::win_inner_rect(w);
                        let clipped = if width > 0 && height > 0 {
                            libgui::gfx::Rect::new(inner.x + x, inner.y + y, width, height).intersect(inner)
                        } else {
                            inner
                        };
                        self.damage.invalidate(clipped);
                    }
                }
                libgui::ipc::WmClientMsg::SetTitle { window_id, title } => {
                    let mut title_r = None;
                    if let Some(w) = self.windows.iter_mut().find(|w| w.id == window_id) {
                        if let WindowApp::Remote(ref rem) = w.app {
                            if rem.client_sock_id == sock_id {
                                w.title = title;
                                title_r = Some(libgui::gfx::Rect::new(w.x, w.y, w.width, TITLEBAR_HEIGHT + 2));
                            }
                        }
                    }
                    if let Some(r) = title_r {
                        self.damage.invalidate(r);
                        let tb = self.taskbar_rect();
                        self.damage.invalidate(tb);
                    }
                }
                libgui::ipc::WmClientMsg::CloseWindow { window_id } => {
                    let owned = self.windows.iter().any(|w| {
                        w.id == window_id
                            && matches!(&w.app, WindowApp::Remote(r) if r.client_sock_id == sock_id)
                    });
                    if owned {
                        self.close_window(window_id);
                    }
                }
                libgui::ipc::WmClientMsg::SetClipboard { text } => {
                    if text != self.last_clipboard {
                        libgui::system::SystemClipboard::set_text(&text);
                        self.last_clipboard = text.clone();
                        let bytes = libgui::ipc::encode_server_msg(
                            &libgui::ipc::WmServerMsg::ClipboardChanged { text },
                        );
                        for c in &self.remote_clients {
                            if c.sock_id != sock_id {
                                let _ = crate::net::unix::unix_socket_send(c.sock_id, &bytes);
                            }
                        }
                    }
                }
            }
        }

        if !disconnected.is_empty() {
            for dead_sock in &disconnected {
                for w in &self.windows {
                    if matches!(&w.app, WindowApp::Remote(r) if r.client_sock_id == *dead_sock) {
                        self.damage.invalidate(Self::win_outer_rect(w));
                    }
                }
                self.windows.retain(|w| {
                    !matches!(&w.app, WindowApp::Remote(r) if r.client_sock_id == *dead_sock)
                });
                let _ = crate::net::unix::unix_socket_close(*dead_sock);
            }
            self.remote_clients.retain(|c| !disconnected.contains(&c.sock_id));
            if let Some(last) = self.windows.last_mut() {
                last.is_active = true;
                if let WindowApp::Remote(ref rem) = last.app {
                    rem.send_input(libgui::event::GuiEvent::FocusChanged { focused: true });
                }
                let lr = libgui::gfx::Rect::new(last.x, last.y, last.width, TITLEBAR_HEIGHT + 2);
                self.damage.invalidate(lr);
            }
            let tb = self.taskbar_rect();
            self.damage.invalidate(tb);
        }
    }

    pub fn handle_mouse_event(&mut self, x: i32, y: i32, left_down: bool) -> TaskbarAction {
        let prev_x = self.mouse_x;
        let prev_y = self.mouse_y;
        let moved = x != prev_x || y != prev_y;
        if moved {
            self.mouse_x = x;
            self.mouse_y = y;
            self.cursor_dirty = true;
        }

        let sw = self.canvas.width as i32;
        let sh = self.canvas.height as i32;
        let result_action = TaskbarAction::None;

        let left_clicked = left_down && !self.prev_left_down;
        let left_released = !left_down && self.prev_left_down;
        self.prev_left_down = left_down;

        if moved && !self.spotlight.is_open {
            if self.desktop.context_menu.is_some() {
                if let Some((mx, my)) = self.desktop.context_menu {
                    self.damage.invalidate(libgui::gfx::Rect::new(mx - 2, my - 2, 198, 182));
                }
            }
            if self.taskbar.start_menu_open {
                let (smx, smy, smw, smh) = self.taskbar.start_menu_rect(sw, sh);
                self.damage.invalidate(libgui::gfx::Rect::new(
                    (smx - 4).max(0),
                    (smy - 4).max(0),
                    smw + 10,
                    smh + 10,
                ));
            } else if self.taskbar.tray_popup_open {
                self.damage.invalidate(libgui::gfx::Rect::new((sw - 310).max(0), (sh - 240).max(0), 310, 240));
            }
            let th = crate::gui::taskbar::TASKBAR_HEIGHT;
            if y >= sh - th || prev_y >= sh - th {
                let tb = self.taskbar_rect();
                self.damage.invalidate(tb);
            }
            for w in self.windows.iter() {
                if !w.is_minimized
                    && (w.contains_titlebar(x, y) || w.contains_titlebar(prev_x, prev_y))
                {
                    self.damage.invalidate(libgui::gfx::Rect::new(
                        w.x,
                        w.y,
                        w.width,
                        TITLEBAR_HEIGHT + 4,
                    ));
                }
            }
            let over_window = self.windows.iter().rev().any(|w| !w.is_minimized && w.contains(x, y));
            if !over_window && !self.taskbar.start_menu_open {
                let (old_ico, new_ico) = self.desktop.update_hover(x, y);
                if let Some(r) = old_ico {
                    self.damage.invalidate(r);
                }
                if let Some(r) = new_ico {
                    self.damage.invalidate(r);
                }
            } else {
                let (old_ico, _) = self.desktop.update_hover(-1000, -1000);
                if let Some(r) = old_ico {
                    self.damage.invalidate(r);
                }
            }
        }

        if moved && !self.spotlight.is_open && !self.taskbar.start_menu_open {
            if let Some((mx, my)) = self.desktop.context_menu {
                self.damage.invalidate(libgui::gfx::Rect::new(mx - 2, my - 2, 198, 182));
            }
            if let Some((mx, my, _)) = self.taskbar.context_menu {
                self.damage.invalidate(libgui::gfx::Rect::new(mx - 2, my - 2, 184, 110));
            }
            if let Some((mx, my, _)) = self.window_menu {
                self.damage.invalidate(libgui::gfx::Rect::new(mx - 2, my - 2, 150, 106));
            }
            let busy = self.windows.iter().any(|w| w.dragging || w.resizing);
            if !busy {
                let mut inner_hover_dmg: Option<libgui::gfx::Rect> = None;
                let mut cur_hover_win: Option<u32> = None;
                for w in self.windows.iter_mut().rev() {
                    if !w.is_minimized && w.contains(x, y) {
                        let (ix, iy, iw, ih) = w.inner_bounds();
                        if x >= ix && x < ix + iw && y >= iy && y < iy + ih {
                            cur_hover_win = Some(w.id);
                            if let WindowApp::Remote(ref rem) = w.app {
                                rem.send_input(libgui::event::GuiEvent::MouseMove {
                                    x: x - ix,
                                    y: y - iy,
                                });
                            } else if w.handle_inner_mouse_move(x, y, left_down) {
                                inner_hover_dmg = Some(Self::win_inner_rect(w));
                            }
                        }
                        break;
                    }
                }
                if self.last_hover_win_id != cur_hover_win {
                    if let Some(old_id) = self.last_hover_win_id {
                        if let Some(old_w) = self.windows.iter_mut().find(|w| w.id == old_id && !w.is_minimized) {
                            if let WindowApp::Remote(ref rem) = old_w.app {
                                rem.send_input(libgui::event::GuiEvent::MouseMove { x: -1, y: -1 });
                            } else if old_w.handle_inner_mouse_move(-1, -1, false) {
                                self.damage.invalidate(Self::win_inner_rect(old_w));
                            }
                        }
                    }
                    self.last_hover_win_id = cur_hover_win;
                }
                if inner_hover_dmg.is_none() && left_down {
                    for w in self.windows.iter_mut().rev() {
                        if !w.is_minimized && w.is_active {
                            let (ix, iy, _, _) = w.inner_bounds();
                            if let WindowApp::Remote(ref rem) = w.app {
                                rem.send_input(libgui::event::GuiEvent::MouseMove {
                                    x: x - ix,
                                    y: y - iy,
                                });
                            } else if w.handle_inner_mouse_move(x, y, true) {
                                inner_hover_dmg = Some(Self::win_inner_rect(w));
                            }
                            break;
                        }
                    }
                }
                if let Some(r) = inner_hover_dmg {
                    self.damage.invalidate(r);
                }
            }
        }

        // 1. If currently resizing a window, update its size with localized union damage
        let mut resize_dmg: Option<libgui::gfx::Rect> = None;
        let th = crate::gui::taskbar::TASKBAR_HEIGHT;
        if let Some(res_win) = self.windows.iter_mut().find(|w| w.resizing) {
            if left_down {
                let old_r = Self::win_outer_rect(res_win);
                match res_win.resize_edge {
                    crate::gui::window::ResizeEdge::BottomRight => {
                        res_win.width = (x - res_win.x).clamp(res_win.min_width, sw - res_win.x);
                        res_win.height = (y - res_win.y).clamp(res_win.min_height, sh - res_win.y - th);
                    }
                    crate::gui::window::ResizeEdge::Right => {
                        res_win.width = (x - res_win.x).clamp(res_win.min_width, sw - res_win.x);
                    }
                    crate::gui::window::ResizeEdge::Bottom => {
                        res_win.height = (y - res_win.y).clamp(res_win.min_height, sh - res_win.y - th);
                    }
                    _ => {}
                }
                let new_r = Self::win_outer_rect(res_win);
                resize_dmg = Some(old_r.union(new_r));
            }
        }
        if let Some(r) = resize_dmg {
            self.damage.invalidate(r);
        }

        // 2. If currently dragging a window, update its position with localized union damage
        let mut drag_dmg: Option<libgui::gfx::Rect> = None;
        if let Some(drag_win) = self.windows.iter_mut().find(|w| w.dragging) {
            if left_down {
                let old_r = Self::win_outer_rect(drag_win);
                let nx = (x - drag_win.drag_offset_x).clamp(-drag_win.width + 60, sw - 60);
                let ny = (y - drag_win.drag_offset_y).clamp(0, sh - th - 28);
                if nx != drag_win.x || ny != drag_win.y {
                    drag_win.x = nx;
                    drag_win.y = ny;
                    let new_r = Self::win_outer_rect(drag_win);
                    drag_dmg = Some(old_r.union(new_r));
                }
            }
        }
        if let Some(r) = drag_dmg {
            self.damage.invalidate(r);
        }

        // Check Start Menu scrollbar move / drag / release (even outside bounds while dragging)
        if self.taskbar.start_menu_open {
            if moved {
                let (_, changed) = self.taskbar.handle_start_scroll_mouse(
                    libgui::event::GuiEvent::MouseMove { x, y },
                    sw,
                    sh,
                );
                if changed || self.taskbar.start_scroll.dragging {
                    let (smx, smy, smw, smh) = self.taskbar.start_menu_rect(sw, sh);
                    self.damage.invalidate(libgui::gfx::Rect::new(
                        (smx - 4).max(0),
                        (smy - 4).max(0),
                        smw + 10,
                        smh + 10,
                    ));
                }
            }
            if left_released {
                let (consumed, _) = self.taskbar.handle_start_scroll_mouse(
                    libgui::event::GuiEvent::MouseUp {
                        x,
                        y,
                        button: libgui::event::MouseButton::Left,
                    },
                    sw,
                    sh,
                );
                if consumed {
                    let (smx, smy, smw, smh) = self.taskbar.start_menu_rect(sw, sh);
                    self.damage.invalidate(libgui::gfx::Rect::new(
                        (smx - 4).max(0),
                        (smy - 4).max(0),
                        smw + 10,
                        smh + 10,
                    ));
                }
            }
        }

        if left_released {
            let mut up_dmg: Option<libgui::gfx::Rect> = None;
            let mut released_files_id: Option<u32> = None;
            for w in self.windows.iter_mut() {
                if !w.dragging && !w.resizing && !w.is_minimized && w.is_active {
                    let (ix, iy, _, _) = w.inner_bounds();
                    if let WindowApp::Remote(ref rem) = w.app {
                        rem.send_input(libgui::event::GuiEvent::MouseUp {
                            x: x - ix,
                            y: y - iy,
                            button: libgui::event::MouseButton::Left,
                        });
                    } else if w.handle_inner_mouse_up(x, y) {
                        up_dmg = Some(Self::win_inner_rect(w));
                        if matches!(&w.app, WindowApp::Files(_)) {
                            released_files_id = Some(w.id);
                        }
                    }
                }
                w.dragging = false;
                w.resizing = false;
                w.resize_edge = crate::gui::window::ResizeEdge::None;
            }
            if let Some(r) = up_dmg {
                self.damage.invalidate(r);
            }
            if let Some(fid) = released_files_id {
                self.drain_files_effects(fid);
            }
        }

        if left_clicked {
            if let Some((mx, my, wid)) = self.window_menu.take() {
                self.damage.invalidate(libgui::gfx::Rect::new(mx - 2, my - 2, 150, 106));
                if x >= mx && x <= mx + 144 && y >= my && y <= my + 100 {
                    let row = ((y - (my + 2)) / 24).clamp(0, 3);
                    match row {
                        0 => {
                            if let Some(w) = self.windows.iter_mut().find(|w| w.id == wid) {
                                if w.is_maximized {
                                    w.toggle_maximize(sw, sh);
                                }
                                w.is_minimized = false;
                            }
                            self.bring_to_front(wid);
                            self.invalidate_all();
                        }
                        1 => {
                            let _ = self.apply_taskbar_action(TaskbarAction::MinimizeWindow(wid), sw, sh);
                        }
                        2 => {
                            let _ = self.apply_taskbar_action(TaskbarAction::MaximizeWindow(wid), sw, sh);
                        }
                        _ => self.close_window(wid),
                    }
                    return TaskbarAction::None;
                }
            }

            if let Some((mx, my, _)) = self.taskbar.context_menu {
                let menu_hit = x >= mx && x <= mx + 178 && y >= my && y <= my + 104;
                let action = self.taskbar.handle_context_click(x, y);
                self.damage.invalidate(libgui::gfx::Rect::new(mx - 2, my - 2, 184, 110));
                if let Some(act) = action {
                    return self.apply_taskbar_action(act, sw, sh);
                }
                if menu_hit {
                    return TaskbarAction::None;
                }
            }

            if let Some((mx, my)) = self.desktop.context_menu {
                let menu_hit = x >= mx && x <= mx + 192 && y >= my && y <= my + 176;
                let action = self.desktop.handle_context_click(x, y);
                self.damage.invalidate(libgui::gfx::Rect::new(mx - 2, my - 2, 198, 182));
                if let Some(act) = action {
                    match act {
                        DesktopMenuAction::OpenIcon(idx) => {
                            let icon_id = self.desktop.icons[idx].id;
                            let icon_title = self.desktop.icons[idx].title;
                            if let Some(path) = self.desktop.icons[idx].target_path {
                                self.open_files_at(path, icon_title);
                            } else {
                                self.open_app(icon_id);
                            }
                        }
                        DesktopMenuAction::OpenTerminal => self.open_app(DesktopAppId::Terminal),
                        DesktopMenuAction::OpenFiles => self.open_app(DesktopAppId::FileManager),
                        DesktopMenuAction::OpenStudio => self.open_app(DesktopAppId::Studio),
                        DesktopMenuAction::Refresh => self.invalidate_all(),
                        DesktopMenuAction::ToggleTheme => self.toggle_theme(),
                        DesktopMenuAction::NextWallpaper => {
                            let _ = crate::gui::wallpaper::cycle_wallpaper();
                            self.invalidate_all();
                        }
                        DesktopMenuAction::OpenDisplaySettings => {
                            self.open_settings_tab(SettingsTab::Display)
                        }
                        DesktopMenuAction::None => {}
                    }
                    return TaskbarAction::None;
                }
                if menu_hit {
                    return TaskbarAction::None;
                }
            }

            // Check SublimeOS Windows 8.1 Search Charm interaction if open
            if self.spotlight.is_open {
                let (consumed, target_opt) = self.spotlight.handle_click(x, y, sw, sh);
                if let Some(target) = target_opt {
                    self.dispatch_spotlight_target(target);
                }
                self.invalidate_all();
                if consumed {
                    return TaskbarAction::None;
                }
            }

            // 3. Check Start Menu scrollbar MouseDown before click (prevents double-scroll)
            if self.taskbar.start_menu_open {
                let (consumed, _) = self.taskbar.handle_start_scroll_mouse(
                    libgui::event::GuiEvent::MouseDown {
                        x,
                        y,
                        button: libgui::event::MouseButton::Left,
                    },
                    sw,
                    sh,
                );
                if consumed {
                    let (smx, smy, smw, smh) = self.taskbar.start_menu_rect(sw, sh);
                    self.damage.invalidate(libgui::gfx::Rect::new(
                        (smx - 4).max(0),
                        (smy - 4).max(0),
                        smw + 10,
                        smh + 10,
                    ));
                    return TaskbarAction::None;
                }
            }

            // 4. Check Taskbar & Start Menu click
            let had_menu = self.taskbar.start_menu_open || self.taskbar.tray_popup_open;
            let action = self.taskbar.handle_click(x, y, &self.windows, sw, sh);
            if had_menu && !self.taskbar.start_menu_open && !self.taskbar.tray_popup_open {
                self.invalidate_all();
            } else if self.taskbar.start_menu_open {
                let (smx, smy, smw, smh) = self.taskbar.start_menu_rect(sw, sh);
                self.damage.invalidate(libgui::gfx::Rect::new(
                    (smx - 4).max(0),
                    (smy - 4).max(0),
                    smw + 10,
                    smh + 10,
                ));
            } else if self.taskbar.tray_popup_open {
                self.damage.invalidate(libgui::gfx::Rect::new((sw - 300).max(0), (sh - 230).max(0), 300, 230));
            }
            if !matches!(action, TaskbarAction::None) {
                return self.apply_taskbar_action(action, sw, sh);
            }

            // If Start Screen is open or click was in bottom taskbar strip, don't fall through to windows
            if self.taskbar.start_menu_open || y >= sh - crate::gui::taskbar::TASKBAR_HEIGHT {
                return TaskbarAction::None;
            }

            // 4. Check Windows from front to back
            let mut clicked_win_id = None;
            let mut close_id = None;
            let mut max_id = None;
            let mut min_id = None;
            let mut start_resize_id = None;
            let mut start_drag_id = None;
            let mut inner_click_id = None;

            for w in self.windows.iter().rev() {
                if !w.is_minimized && w.contains(x, y) {
                    clicked_win_id = Some(w.id);

                    let edge = w.contains_resize_handle(x, y);
                    if w.contains_close_btn(x, y) {
                        close_id = Some(w.id);
                    } else if w.contains_maximize_btn(x, y) {
                        max_id = Some(w.id);
                    } else if w.contains_minimize_btn(x, y) {
                        min_id = Some(w.id);
                    } else if edge != crate::gui::window::ResizeEdge::None {
                        start_resize_id = Some((w.id, edge));
                    } else if w.contains_titlebar(x, y) {
                        start_drag_id = Some((w.id, x - w.x, y - w.y));
                    } else {
                        inner_click_id = Some(w.id);
                    }
                    break;
                }
            }

            if let Some(id) = close_id {
                self.close_window(id);
                return TaskbarAction::None;
            }

            if let Some(id) = max_id {
                let mut dmg = None;
                if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
                    let old_r = Self::win_outer_rect(w);
                    w.dismiss_popups();
                    w.toggle_maximize(sw, sh);
                    let new_r = Self::win_outer_rect(w);
                    dmg = Some(old_r.union(new_r));
                }
                if let Some(r) = dmg {
                    self.damage.invalidate(r);
                }
                return TaskbarAction::None;
            }

            if let Some(id) = min_id {
                let mut dmg = None;
                if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
                    dmg = Some(Self::win_outer_rect(w));
                    w.is_minimized = true;
                    w.deactivate();
                }
                if let Some(r) = dmg {
                    self.damage.invalidate(r);
                    let tb = self.taskbar_rect();
                    self.damage.invalidate(tb);
                }
                return TaskbarAction::None;
            }

            if let Some((id, edge)) = start_resize_id {
                self.bring_to_front(id);
                let mut dmg = None;
                if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
                    w.dismiss_popups();
                    w.resizing = true;
                    w.resize_edge = edge;
                    dmg = Some(Self::win_outer_rect(w));
                }
                if let Some(r) = dmg {
                    self.damage.invalidate(r);
                }
                return TaskbarAction::None;
            }

            if let Some((id, off_x, off_y)) = start_drag_id {
                if off_x <= 26 {
                    self.bring_to_front(id);
                    let mut dmg = None;
                    if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
                        w.dismiss_popups();
                        dmg = Some(Self::win_outer_rect(w));
                        let mx = (w.x + 4).clamp(0, sw - 148);
                        let my = (w.y + TITLEBAR_HEIGHT).clamp(0, sh - crate::gui::taskbar::TASKBAR_HEIGHT - 104);
                        self.window_menu = Some((mx, my, id));
                        self.damage.invalidate(libgui::gfx::Rect::new(mx - 2, my - 2, 150, 106));
                    }
                    if let Some(r) = dmg {
                        self.damage.invalidate(r);
                    }
                    return TaskbarAction::None;
                }
                let now_ms = crate::arch::x86_64::pit::get_ticks();
                let is_double = self
                    .last_titlebar_click
                    .map(|(pid, pt)| pid == id && now_ms.saturating_sub(pt) <= 500)
                    .unwrap_or(false);
                if is_double {
                    self.last_titlebar_click = None;
                    self.bring_to_front(id);
                    let mut dmg = None;
                    if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
                        let old_r = Self::win_outer_rect(w);
                        w.dismiss_popups();
                        w.toggle_maximize(sw, sh);
                        let new_r = Self::win_outer_rect(w);
                        dmg = Some(old_r.union(new_r));
                    }
                    if let Some(r) = dmg {
                        self.damage.invalidate(r);
                    }
                    return TaskbarAction::None;
                }
                self.last_titlebar_click = Some((id, now_ms));
                self.bring_to_front(id);
                let mut dmg = None;
                if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
                    w.dismiss_popups();
                    w.dragging = true;
                    w.drag_offset_x = off_x;
                    w.drag_offset_y = off_y;
                    dmg = Some(Self::win_outer_rect(w));
                }
                if let Some(r) = dmg {
                    self.damage.invalidate(r);
                }
                return TaskbarAction::None;
            }

            if let Some(id) = inner_click_id {
                self.bring_to_front(id);
                let mut sys_act = SysmonAction::None;
                let mut close_self = false;
                let mut calc_toggle_theme = false;
                let mut calc_open_about = false;
                let mut inner_dmg = None;
                if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
                    w.handle_inner_click(x, y);
                    if !matches!(&w.app, WindowApp::Remote(_)) {
                        inner_dmg = Some(Self::win_inner_rect(w));
                    }
                    match w.app {
                        WindowApp::Sysmon(ref mut sysmon) => sys_act = sysmon.take_action(),
                        WindowApp::Settings(ref mut s) => {
                            if s.pending_close {
                                s.pending_close = false;
                                close_self = true;
                            }
                        }
                        WindowApp::Calc(ref mut calc) => {
                            if calc.pending_toggle_theme {
                                calc.pending_toggle_theme = false;
                                calc_toggle_theme = true;
                            }
                            if calc.pending_about {
                                calc.pending_about = false;
                                calc_open_about = true;
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(r) = inner_dmg {
                    self.damage.invalidate(r);
                }
                self.drain_files_effects(id);
                if close_self {
                    self.close_window(id);
                }
                if calc_toggle_theme {
                    self.toggle_theme();
                }
                if calc_open_about {
                    self.open_app(DesktopAppId::About);
                }
                match sys_act {
                    SysmonAction::SwitchToWindow(target_id) => {
                        for w in self.windows.iter_mut() {
                            if w.id == target_id {
                                w.is_minimized = false;
                            }
                        }
                        self.bring_to_front(target_id);
                    }
                    SysmonAction::CloseWindow(target_id) => {
                        self.close_window(target_id);
                    }
                    SysmonAction::KillPid(pid) => {
                        let _ = crate::task::process::sys_kill(pid as i32, 9);
                        let win_ids: Vec<u32> = self
                            .windows
                            .iter()
                            .filter(|w| matches!(&w.app, WindowApp::Remote(r) if r.owner_pid == Some(pid)))
                            .map(|w| w.id)
                            .collect();
                        for wid in win_ids {
                            self.close_window(wid);
                        }
                        crate::task::process::reap_detached_zombies();
                        self.invalidate_all();
                    }
                    SysmonAction::CloseSelf => {
                        self.close_window(id);
                    }
                    SysmonAction::OpenSearch => {
                        self.spotlight.toggle();
                        self.invalidate_all();
                    }
                    SysmonAction::OpenAbout => {
                        self.open_app(DesktopAppId::About);
                    }
                    SysmonAction::None => {}
                }
                return TaskbarAction::None;
            }

            // 5. Clicked on desktop background
            if clicked_win_id.is_none() {
                // Deactivate windows and invalidate outer rects
                for w in self.windows.iter_mut() {
                    if w.is_active {
                        w.deactivate();
                        self.damage.invalidate(Self::win_outer_rect(w));
                    }
                }

                if let Some(app_id) = self.desktop.check_click(x, y) {
                    if let Some(target_path) = self.desktop.last_launched_target.take() {
                        let title = if target_path == "/" {
                            "My Computer"
                        } else if target_path == "/Trash" {
                            "Trash"
                        } else {
                            "Documents"
                        };
                        self.open_files_at(target_path, title);
                    } else {
                        self.open_app(app_id);
                    }
                } else {
                    self.damage.invalidate(libgui::gfx::Rect::new(0, 0, 220.min(sw), (sh - crate::gui::taskbar::TASKBAR_HEIGHT).max(0)));
                }
            }
        }

        result_action
    }

    pub fn handle_key_event(&mut self, ch: char) -> TaskbarAction {
        let sw = self.canvas.width as i32;
        let sh = self.canvas.height as i32;

        // Ctrl+Space ('\0') toggles Spotlight everywhere; '`' only toggles when Start Menu is closed
        if ch == '\0' || (ch == '`' && !self.taskbar.start_menu_open) {
            self.taskbar.set_start_menu_open(false);
            self.dismiss_all_window_popups();
            self.spotlight.toggle();
            self.invalidate_all();
            return TaskbarAction::None;
        }

        if self.taskbar.start_menu_open {
            let key_opt = if ch == '\r' || ch == '\n' {
                Some(libgui::event::KeyCode::Enter)
            } else if ch == '\t' {
                Some(libgui::event::KeyCode::Tab)
            } else if ch == '\x08' || ch == '\x7F' {
                Some(libgui::event::KeyCode::Backspace)
            } else if ch == '\x1B' {
                Some(libgui::event::KeyCode::Escape)
            } else if (' '..='~').contains(&ch) {
                Some(libgui::event::KeyCode::Char(ch))
            } else {
                None
            };
            if let Some(k) = key_opt {
                let act = self.taskbar.handle_start_key(k, Some(ch), sw, sh);
                let out = self.apply_taskbar_action(act, sw, sh);
                self.invalidate_all();
                return out;
            }
            return TaskbarAction::None;
        }

        if self.spotlight.is_open {
            if let Some(target) = self.spotlight.handle_char(ch) {
                self.dispatch_spotlight_target(target);
            }
            self.invalidate_all();
            return TaskbarAction::None;
        }

        let mut dmg = None;
        let mut active_files_id: Option<u32> = None;
        if let Some(active_win) = self.windows.iter_mut().find(|w| w.is_active && !w.is_minimized) {
            active_win.handle_char(ch);
            if !matches!(&active_win.app, WindowApp::Remote(_)) {
                dmg = Some(Self::win_inner_rect(active_win));
            }
            if matches!(&active_win.app, WindowApp::Files(_)) {
                active_files_id = Some(active_win.id);
            }
        }
        if let Some(r) = dmg {
            self.damage.invalidate(r);
        }
        if let Some(fid) = active_files_id {
            self.drain_files_effects(fid);
        }
        TaskbarAction::None
    }

    pub fn handle_gui_key_event(
        &mut self,
        key: libgui::event::KeyCode,
        ch: Option<char>,
        mods: libgui::event::Modifiers,
    ) -> TaskbarAction {
        let sw = self.canvas.width as i32;
        let sh = self.canvas.height as i32;

        if self.desktop.context_menu.is_some()
            || self.window_menu.is_some()
            || self.taskbar.context_menu.is_some()
            || self.taskbar.tray_popup_open
        {
            if key == libgui::event::KeyCode::Escape {
                self.desktop.context_menu = None;
                self.window_menu = None;
                self.taskbar.context_menu = None;
                self.taskbar.tray_popup_open = false;
                self.invalidate_all();
            }
            return TaskbarAction::None;
        }

        if self.taskbar.start_menu_open {
            if (mods.ctrl || mods.alt) && key != libgui::event::KeyCode::Escape {
                return TaskbarAction::None;
            }
            let act = self.taskbar.handle_start_key(key, ch, sw, sh);
            let out = self.apply_taskbar_action(act, sw, sh);
            self.invalidate_all();
            return out;
        }

        if self.spotlight.is_open {
            if key == libgui::event::KeyCode::Escape {
                self.spotlight.is_open = false;
                self.invalidate_all();
                return TaskbarAction::None;
            }
            if mods.ctrl || mods.alt {
                return TaskbarAction::None;
            }
            if matches!(
                key,
                libgui::event::KeyCode::Up
                    | libgui::event::KeyCode::Down
                    | libgui::event::KeyCode::Enter
            ) {
                if let Some(target) = self.spotlight.handle_key(key) {
                    self.dispatch_spotlight_target(target);
                }
                self.invalidate_all();
                return TaskbarAction::None;
            }
            if key == libgui::event::KeyCode::Backspace {
                let _ = self.handle_key_event('\x08');
            } else if let Some(c) = ch {
                let _ = self.handle_key_event(c);
            }
            return TaskbarAction::None;
        }
        let mut dmg = None;
        let mut active_files_id: Option<u32> = None;
        if let Some(active_win) = self.windows.iter_mut().find(|w| w.is_active && !w.is_minimized) {
            active_win.handle_gui_key(key, ch, mods);
            if !matches!(&active_win.app, WindowApp::Remote(_)) {
                dmg = Some(Self::win_inner_rect(active_win));
            }
            if matches!(&active_win.app, WindowApp::Files(_)) {
                active_files_id = Some(active_win.id);
            }
        }
        if let Some(r) = dmg {
            self.damage.invalidate(r);
        }
        if let Some(fid) = active_files_id {
            self.drain_files_effects(fid);
        }
        TaskbarAction::None
    }

    fn determine_cursor_type(&self) -> CursorType {
        let sw = self.canvas.width as i32;
        let sh = self.canvas.height as i32;
        let mut cursor_type = CursorType::Arrow;

        if self.spotlight.is_open {
            let box_x = sw - 320;
            let box_y = 58;
            if self.mouse_x >= box_x
                && self.mouse_x <= sw - 20
                && self.mouse_y >= box_y
                && self.mouse_y <= box_y + 34
            {
                cursor_type = CursorType::IBeam;
            }
        } else if self.taskbar.start_menu_open {
            let l = self.taskbar.start_menu_layout(sw, sh);
            let mx = self.mouse_x;
            let my = self.mouse_y;
            let contains = |r: libgui::gfx::Rect| mx >= r.x && mx < r.right() && my >= r.y && my < r.bottom();
            if mx >= l.mx && mx < l.mx + l.mw && my >= l.my && my < l.my + l.mh {
                let sr = l.search_rect();
                let clr = l.search_clear_rect();
                let (shell_r, reboot_r, halt_r) = l.footer_power_rects();
                let count = self.taskbar.filtered_start_entries().len();
                let has_sb = count > l.visible_rows;
                let row_w = l.row_w(has_sb);
                if !self.taskbar.start_search_query.is_empty() && contains(clr) {
                    cursor_type = CursorType::Hand;
                } else if contains(sr) {
                    cursor_type = CursorType::IBeam;
                } else if (has_sb && contains(l.sb_rect()))
                    || contains(shell_r)
                    || contains(reboot_r)
                    || contains(halt_r)
                    || (mx >= l.list_x && mx < l.list_x + row_w && my >= l.list_y && my < l.list_y + l.list_h && ((my - l.list_y) / l.item_h) as usize + (self.taskbar.start_scroll.value.max(0) as usize) < count)
                    || (0..6).any(|i| contains(l.place_rect(i)))
                    || (0..4).any(|i| contains(l.tool_rect(i)))
                {
                    cursor_type = CursorType::Hand;
                }
                return cursor_type;
            }
            if my >= sh - crate::gui::taskbar::TASKBAR_HEIGHT && mx < 180 {
                cursor_type = CursorType::Hand;
            }
        } else if self.window_menu.is_some()
            || self.taskbar.context_menu.is_some()
            || self.taskbar.tray_popup_open
            || self.desktop.context_menu.is_some()
        {
            return CursorType::Arrow;
        } else {
            for w in self.windows.iter().rev() {
                if !w.is_minimized && w.contains(self.mouse_x, self.mouse_y) {
                    let edge = w.contains_resize_handle(self.mouse_x, self.mouse_y);
                    match edge {
                        ResizeEdge::BottomRight => cursor_type = CursorType::ResizeDiagonal,
                        ResizeEdge::Right => cursor_type = CursorType::ResizeHorizontal,
                        ResizeEdge::Bottom => cursor_type = CursorType::ResizeVertical,
                        ResizeEdge::None => {
                            if let WindowApp::Files(ref f) = w.app {
                                let (ix, iy, iw, ih) = w.inner_bounds();
                                cursor_type = f.cursor_at(self.mouse_x, self.mouse_y, ix, iy, iw, ih);
                            } else if matches!(&w.app, WindowApp::Terminal(_) | WindowApp::Editor(_))
                                && self.mouse_y > w.y + 28
                                && self.mouse_y < w.y + w.height - 4
                            {
                                cursor_type = CursorType::IBeam;
                            }
                        }
                    }
                    break;
                }
            }

            // Start button & Quick Launch hover -> Hand
            if self.mouse_y >= sh - crate::gui::taskbar::TASKBAR_HEIGHT && self.mouse_x < 180 {
                cursor_type = CursorType::Hand;
            }
        }

        cursor_type
    }

    #[inline]
    fn cursor_patch_box(&self, mx: i32, my: i32) -> (i32, i32, i32, i32) {
        let sw = self.canvas.width as i32;
        let sh = self.canvas.height as i32;
        let x0 = (mx - 10).max(0);
        let y0 = (my - 10).max(0);
        let x1 = (mx + 22).min(sw);
        let y1 = (my + 22).min(sh);
        (x0, y0, (x1 - x0).clamp(0, 32), (y1 - y0).clamp(0, 32))
    }

    fn capture_cursor_bg(&mut self, bx: i32, by: i32, bw: i32, bh: i32) {
        let stride = self.canvas.width as usize;
        for row in 0..bh {
            let sy = (by + row) as usize * stride;
            let dst_off = row as usize * 32;
            for col in 0..bw {
                self.cursor_bg[dst_off + col as usize] = self.canvas.buffer[sy + (bx + col) as usize];
            }
        }
        self.saved_cursor_box = Some((bx, by, bw, bh));
    }

    fn restore_cursor_bg(&mut self) -> Option<(i32, i32, i32, i32)> {
        let (bx, by, bw, bh) = self.saved_cursor_box.take()?;
        let stride = self.canvas.width as usize;
        for row in 0..bh {
            let dy = (by + row) as usize * stride;
            let src_off = row as usize * 32;
            for col in 0..bw {
                self.canvas.buffer[dy + (bx + col) as usize] = self.cursor_bg[src_off + col as usize];
            }
        }
        Some((bx, by, bw, bh))
    }

    pub fn render_frame(&mut self) {
        crate::drivers::audio::poll_audio_stream();
        let ticks = crate::arch::x86_64::pit::get_ticks();
        let now_sec = (ticks / 1000) as u8;
        let blink = ((ticks / 500) % 2) as u64;
        let sw = self.canvas.width as i32;
        let sh = self.canvas.height as i32;
        let screen = self.screen_rect();

        if now_sec != self.last_sec {
            self.last_sec = now_sec;
            crate::task::process::reap_detached_zombies();
            let th = crate::gui::taskbar::TASKBAR_HEIGHT;
            // Only the right notification tray & RTC clock strip needs updating each second
            self.damage.invalidate(libgui::gfx::Rect::new(
                (sw - 220).max(0),
                (sh - th).max(0),
                220,
                th,
            ));
            if self.taskbar.tray_popup_open {
                self.damage.invalidate(libgui::gfx::Rect::new(
                    (sw - 300).max(0),
                    (sh - th - 198).max(0),
                    296,
                    196,
                ));
            }
            for w in self.windows.iter() {
                if !w.is_minimized && matches!(&w.app, WindowApp::Sysmon(_)) {
                    self.damage.invalidate(Self::win_inner_rect(w));
                }
            }
        }
        // State synchronization for visible VFS-backed File Explorer and external Settings state
        let mut sync_rects = [libgui::gfx::Rect::new(0, 0, 0, 0); 8];
        let mut sync_count = 0usize;
        for w in self.windows.iter_mut() {
            if w.is_minimized || sync_count >= sync_rects.len() {
                continue;
            }
            let r = libgui::gfx::Rect::new(
                w.x + 1,
                w.y + TITLEBAR_HEIGHT + 1,
                (w.width - 2).max(0),
                (w.height - TITLEBAR_HEIGHT - 2).max(0),
            );
            let dirty = match &mut w.app {
                WindowApp::Files(ref mut f) => f.sync_vfs(),
                WindowApp::Settings(ref mut s) => s.sync_external_state(),
                _ => false,
            };
            if dirty {
                sync_rects[sync_count] = r;
                sync_count += 1;
            }
        }
        for i in 0..sync_count {
            self.damage.invalidate(sync_rects[i]);
        }
        if blink != self.last_blink {
            self.last_blink = blink;
            // Only active Terminal/Editor windows render a blinking caret
            if let Some(active) = self.windows.iter().rev().find(|w| w.is_active && !w.is_minimized) {
                if matches!(&active.app, WindowApp::Terminal(_) | WindowApp::Editor(_)) {
                    self.damage.invalidate(Self::win_inner_rect(active));
                }
            }
        }

        let wall_gen = crate::gui::wallpaper::wallpaper_generation();
        if wall_gen != self.last_wall_gen {
            self.last_wall_gen = wall_gen;
            self.invalidate_all();
        }

        if !self.damage.is_dirty() && !self.cursor_dirty {
            return;
        }

        let (fb_ptr, pitch, bpp, rpos, gpos, bpos) = {
            let vbe = VBE.lock();
            (
                vbe.framebuffer,
                vbe.pitch,
                vbe.bpp,
                vbe.red_pos,
                vbe.green_pos,
                vbe.blue_pos,
            )
        };

        if !self.damage.is_dirty() && self.cursor_dirty && self.saved_cursor_box.is_some() {
            // Cursor-only motion: restore old background in RAM, draw new cursor, coalesce into non-overlapping MMIO blit
            self.cursor_dirty = false;
            let old_box = self.restore_cursor_bg();
            let (nbx, nby, nbw, nbh) = self.cursor_patch_box(self.mouse_x, self.mouse_y);
            self.capture_cursor_bg(nbx, nby, nbw, nbh);
            let cursor_type = self.determine_cursor_type();
            draw_cursor(&mut self.canvas, self.mouse_x, self.mouse_y, cursor_type);

            let mut blit_dmg = libgui::system::DamageTracker::new();
            if let Some((obx, oby, obw, obh)) = old_box {
                blit_dmg.invalidate(libgui::gfx::Rect::new(obx, oby, obw, obh));
            }
            blit_dmg.invalidate(libgui::gfx::Rect::new(nbx, nby, nbw, nbh));
            for r in blit_dmg.take_clipped(screen) {
                self.canvas.blit_rect_to_framebuffer(
                    fb_ptr, pitch, bpp, rpos, gpos, bpos, r.x, r.y, r.w, r.h,
                );
            }
            return;
        }

        self.cursor_dirty = false;
        let old_cursor = self.restore_cursor_bg();
        let dirty_rects = self.damage.take_clipped(screen);

        let needs_sysmon = self.windows.iter().any(|w| {
            !w.is_minimized
                && matches!(&w.app, WindowApp::Sysmon(_))
                && dirty_rects.iter().any(|d| Self::win_outer_rect(w).intersects(*d))
        });
        let win_snapshot: Option<Vec<(u32, alloc::string::String, bool)>> = if needs_sysmon {
            Some(
                self.windows
                    .iter()
                    .map(|w| (w.id, w.title.clone(), w.is_minimized))
                    .collect(),
            )
        } else {
            None
        };
        if let Some(ref snap) = win_snapshot {
            for w in self.windows.iter_mut() {
                if let WindowApp::Sysmon(ref mut sysmon) = w.app {
                    sysmon.sync_windows(snap.clone());
                }
            }
        }

        for d in &dirty_rects {
            self.canvas.set_base_clip(d.x, d.y, d.w, d.h);

            // 1. Clipped wallpaper scanline copy & desktop icons
            self.desktop.render(&mut self.canvas, self.mouse_x, self.mouse_y);

            // 2. Intersecting windows (back to front)
            for w in self.windows.iter_mut() {
                if !w.is_minimized && Self::win_outer_rect(w).intersects(*d) {
                    w.render(&mut self.canvas, self.mouse_x, self.mouse_y);
                }
            }

            // 3. Taskbar, Start Screen & Tray Popup (only if dirty rect touches taskbar or popups are active)
            if d.bottom() > sh - crate::gui::taskbar::TASKBAR_HEIGHT
                || self.taskbar.start_menu_open
                || self.taskbar.tray_popup_open
                || self.taskbar.context_menu.is_some()
            {
                self.taskbar
                    .render(&mut self.canvas, &self.windows, self.mouse_x, self.mouse_y);
            }

            // 3.5. Spotlight Search Modal Overlay, Desktop Context Menu & Window System Menu
            if self.spotlight.is_open {
                self.spotlight.render(&mut self.canvas, sw, sh);
            }
            self.desktop.render_context_menu(&mut self.canvas, self.mouse_x, self.mouse_y);
            Self::draw_window_menu(&mut self.canvas, self.window_menu, self.mouse_x, self.mouse_y);

            self.canvas.clear_base_clip();
        }

        // 4. Capture clean under-cursor patch before drawing cursor sprite
        let (nbx, nby, nbw, nbh) = self.cursor_patch_box(self.mouse_x, self.mouse_y);
        self.capture_cursor_bg(nbx, nby, nbw, nbh);
        let cursor_type = self.determine_cursor_type();
        draw_cursor(&mut self.canvas, self.mouse_x, self.mouse_y, cursor_type);

        // 5. Blit strictly the coalesced damaged rectangles to hardware linear framebuffer
        let mut blit_dmg = libgui::system::DamageTracker::new();
        if let Some((obx, oby, obw, obh)) = old_cursor {
            blit_dmg.invalidate(libgui::gfx::Rect::new(obx, oby, obw, obh));
        }
        for d in dirty_rects {
            blit_dmg.invalidate(d);
        }
        blit_dmg.invalidate(libgui::gfx::Rect::new(nbx, nby, nbw, nbh));
        for r in blit_dmg.take_clipped(screen) {
            self.canvas.blit_rect_to_framebuffer(
                fb_ptr, pitch, bpp, rpos, gpos, bpos, r.x, r.y, r.w, r.h,
            );
        }
    }
}

/// Launch the Sublime Desktop GUI environment.
pub fn launch_desktop() {
    kinfo!("Initializing UEFI GOP / Multiboot Linear Framebuffer for SublimeOS Desktop...");

    if let Err(e) = vbe::init(1024, 768) {
        crate::kerror!("GUI launch aborted: {}", e);
        return;
    }

    if vbe::is_text_mode_fallback() {
        crate::kerror!("Cannot launch SublimeOS Desktop: Hardware is running in VGA Text Mode (no UEFI GOP / Multiboot LFB active).");
        return;
    }

    // Ensure CPU interrupts are active (syscalls via FMASK / int 0x80 clear IF)
    crate::arch::x86_64::registers::enable_interrupts();

    // Copy needed display metadata out of VBE lock guard immediately
    let (w, h, pitch, bpp) = {
        let vbe = crate::drivers::vbe::VBE.lock();
        (vbe.width, vbe.height, vbe.pitch, vbe.bpp)
    };

    // Load optional TrueType UI font from VFS (/usr/share/fonts/ui.ttf) if available
    if let Ok(file) = crate::fs::vfs::VFS.lock().open("/usr/share/fonts/ui.ttf", 0) {
        let mut buf = alloc::vec![0u8; 512 * 1024];
        if let Ok(n) = file.read(&mut buf) {
            if n > 12 {
                buf.truncate(n);
                if libgui::font::load_default_ttf(buf) {
                    kinfo!("Loaded custom system TTF font from /usr/share/fonts/ui.ttf ({} bytes)", n);
                }
            }
        }
    }

    crate::drivers::mouse::set_bounds(w as i32, h as i32);
    let mut desktop_mgr = alloc::boxed::Box::new(DesktopManager::new(w, h));

    // Bind /run/wm.sock so Ring-3 libgui apps can connect as real IPC compositor clients
    let wm_sock = crate::net::unix::unix_socket_open(crate::net::unix::SOCK_STREAM);
    if crate::net::unix::unix_socket_bind(wm_sock, libgui::ipc::WM_SOCKET_PATH).is_ok()
        && crate::net::unix::unix_socket_listen(wm_sock, 8).is_ok()
    {
        desktop_mgr.wm_listen_sock_id = Some(wm_sock);
        kinfo!("Compositor IPC listening on {}", libgui::ipc::WM_SOCKET_PATH);
    } else {
        let _ = crate::net::unix::unix_socket_close(wm_sock);
    }

    // Open default initial windows: Task Manager behind Terminal so Terminal starts focused
    desktop_mgr.open_app(DesktopAppId::Sysmon);
    desktop_mgr.open_app(DesktopAppId::Terminal);

    kinfo!("SublimeOS Desktop online: {}x{}@{}bpp (Pitch: {} B)", w, h, bpp, pitch);

    // Render initial frame to paint the SublimeOS desktop onto the display before suppressing text console
    desktop_mgr.render_frame();

    let mut running = true;
    let mut prev_right_down = false;
    DESKTOP_ACTIVE.store(true, Ordering::Release);
    crate::gui::boot_splash::stop();
    crate::drivers::gop_console::set_gui_active(true);

    while running {
        // 0. Service Ring-3 compositor IPC clients on /run/wm.sock and queued LaunchServices opens
        for open_path in drain_pending_open_paths() {
            desktop_mgr.open_editor_file(&open_path);
        }
        desktop_mgr.poll_remote_clients();

        // Poll input hardware directly per frame
        crate::drivers::keyboard::poll_ps2_controller();
        crate::drivers::usb::poll_hid_nonblocking();

        // 1. Poll mouse input
        let (mx, my, left_btn, right_btn) = {
            let m = MOUSE.lock();
            (m.x, m.y, m.left_button, m.right_button)
        };
        if right_btn != prev_right_down {
            let pressed_now = right_btn;
            prev_right_down = right_btn;
            if desktop_mgr.taskbar.start_menu_open {
                if pressed_now {
                    let (smx, smy, smw, smh) = desktop_mgr.taskbar.start_menu_rect(w as i32, h as i32);
                    let in_menu = mx >= smx && mx < smx + smw && my >= smy && my < smy + smh;
                    if !in_menu {
                        if my >= (h as i32 - crate::gui::taskbar::TASKBAR_HEIGHT) {
                            desktop_mgr.taskbar.open_context_menu(mx, my, &desktop_mgr.windows, w as i32, h as i32);
                        } else {
                            desktop_mgr.taskbar.set_start_menu_open(false);
                        }
                        desktop_mgr.invalidate_all();
                    }
                }
            } else {
            let mut hit_window = false;
            let mut bring_front_id = None;
            let mut title_menu_id = None;
            let mut local_rclick_rect = None;
            for win in desktop_mgr.windows.iter_mut().rev() {
                if !win.is_minimized && win.contains(mx, my) {
                    hit_window = true;
                    bring_front_id = Some(win.id);
                    let (ix, iy, iw, ih) = win.inner_bounds();
                    if mx >= ix && mx < ix + iw && my >= iy && my < iy + ih {
                        if let WindowApp::Remote(ref rem) = win.app {
                            let ev = if pressed_now {
                                libgui::event::GuiEvent::MouseDown {
                                    x: mx - ix,
                                    y: my - iy,
                                    button: libgui::event::MouseButton::Right,
                                }
                            } else {
                                libgui::event::GuiEvent::MouseUp {
                                    x: mx - ix,
                                    y: my - iy,
                                    button: libgui::event::MouseButton::Right,
                                }
                            };
                            rem.send_input(ev);
                        } else if pressed_now {
                            win.handle_inner_right_click(mx, my);
                            local_rclick_rect = Some(DesktopManager::win_inner_rect(win));
                        }
                    } else if pressed_now && win.contains_titlebar(mx, my) {
                        title_menu_id = Some(win.id);
                    }
                    break;
                }
            }
            if pressed_now {
                if let Some(wid) = bring_front_id {
                    desktop_mgr.bring_to_front(wid);
                }
                if let Some(r) = local_rclick_rect {
                    desktop_mgr.invalidate_rect(r);
                }
                if let Some(wid) = title_menu_id {
                    let sw_i = w as i32;
                    let sh_i = h as i32;
                    let menu_x = mx.min((sw_i - 148).max(0)).max(0);
                    let menu_y = my.min((sh_i - crate::gui::taskbar::TASKBAR_HEIGHT - 104).max(0)).max(0);
                    desktop_mgr.dismiss_all_window_popups();
                    desktop_mgr.window_menu = Some((menu_x, menu_y, wid));
                    desktop_mgr.desktop.context_menu = None;
                    desktop_mgr.taskbar.context_menu = None;
                    desktop_mgr.invalidate_all();
                } else if !hit_window && my >= (h as i32 - crate::gui::taskbar::TASKBAR_HEIGHT) {
                    desktop_mgr.dismiss_all_window_popups();
                    desktop_mgr.desktop.context_menu = None;
                    desktop_mgr.window_menu = None;
                    desktop_mgr.taskbar.open_context_menu(mx, my, &desktop_mgr.windows, w as i32, h as i32);
                    desktop_mgr.invalidate_all();
                } else if !hit_window
                    && !desktop_mgr.taskbar.start_menu_open
                    && my < (h as i32 - crate::gui::taskbar::TASKBAR_HEIGHT)
                {
                    desktop_mgr.dismiss_all_window_popups();
                    desktop_mgr.taskbar.set_start_menu_open(false);
                    desktop_mgr.taskbar.tray_popup_open = false;
                    desktop_mgr.taskbar.context_menu = None;
                    desktop_mgr.window_menu = None;
                    desktop_mgr.spotlight.is_open = false;
                    desktop_mgr.desktop.open_context_menu(mx, my, w as i32, h as i32);
                    desktop_mgr.invalidate_all();
                }
            }
            }
        }
        let wheel_dz = crate::drivers::mouse::take_scroll_delta();
        if wheel_dz != 0 {
            if desktop_mgr.taskbar.start_menu_open {
                if desktop_mgr.taskbar.handle_start_wheel(mx, my, wheel_dz, w as i32, h as i32) {
                    let (smx, smy, smw, smh) = desktop_mgr.taskbar.start_menu_rect(w as i32, h as i32);
                    desktop_mgr.invalidate_rect(libgui::gfx::Rect::new(
                        (smx - 4).max(0),
                        (smy - 4).max(0),
                        smw + 10,
                        smh + 10,
                    ));
                }
            } else {
                let mut wheel_dmg = None;
                for win in desktop_mgr.windows.iter_mut().rev() {
                    if win.contains(mx, my) {
                        if win.handle_wheel(mx, my, wheel_dz) && !matches!(&win.app, WindowApp::Remote(_)) {
                            wheel_dmg = Some(DesktopManager::win_inner_rect(win));
                        }
                        break;
                    }
                }
                if let Some(r) = wheel_dmg {
                    desktop_mgr.invalidate_rect(r);
                }
            }
        }

        let handle_session_action = |act: TaskbarAction, running_flag: &mut bool| match act {
            TaskbarAction::ExitToShell => {
                *running_flag = false;
            }
            TaskbarAction::RebootSystem => {
                kinfo!("System reboot requested via GUI.");
                unsafe {
                    crate::arch::x86_64::io::outb(0x64, 0xFE);
                }
            }
            TaskbarAction::HaltSystem => {
                kinfo!("System halted by user via GUI.");
                crate::arch::x86_64::acpi::poweroff();
            }
            _ => {}
        };

        let action = desktop_mgr.handle_mouse_event(mx, my, left_btn);
        handle_session_action(action, &mut running);

        // 2. Poll GUI shortcut / KeyUp / Function-key events first
        for _ in 0..32 {
            let Some(gev) = crate::drivers::keyboard::pop_gui_event() else {
                break;
            };
            match gev {
                crate::drivers::keyboard::GuiInputEvent::Shortcut(sc) => {
                    use crate::drivers::keyboard::SystemShortcut;
                    match sc {
                        SystemShortcut::ToggleStartMenu => {
                            let next = !desktop_mgr.taskbar.start_menu_open;
                            desktop_mgr.dismiss_all_window_popups();
                            desktop_mgr.taskbar.set_start_menu_open(next);
                            if next {
                                desktop_mgr.taskbar.sync_start_scroll(w as i32, h as i32, false);
                            }
                            desktop_mgr.taskbar.tray_popup_open = false;
                            desktop_mgr.spotlight.is_open = false;
                            desktop_mgr.desktop.context_menu = None;
                            desktop_mgr.invalidate_all();
                        }
                        SystemShortcut::AltTab => {
                            desktop_mgr.taskbar.set_start_menu_open(false);
                            desktop_mgr.cycle_active_window();
                        }
                        SystemShortcut::AltF4 => {
                            let active_id = desktop_mgr
                                .windows
                                .iter()
                                .rev()
                                .find(|w| w.is_active && !w.is_minimized)
                                .map(|w| w.id);
                            if let Some(id) = active_id {
                                desktop_mgr.close_window(id);
                            }
                        }
                        SystemShortcut::ToggleSpotlight => {
                            desktop_mgr.dismiss_all_window_popups();
                            desktop_mgr.taskbar.set_start_menu_open(false);
                            desktop_mgr.desktop.context_menu = None;
                            desktop_mgr.spotlight.toggle();
                            desktop_mgr.invalidate_all();
                        }
                        SystemShortcut::ShowDesktop => {
                            desktop_mgr.taskbar.set_start_menu_open(false);
                            let any_visible = desktop_mgr.windows.iter().any(|w| !w.is_minimized);
                            for w in desktop_mgr.windows.iter_mut() {
                                w.is_minimized = any_visible;
                                if any_visible {
                                    w.deactivate();
                                }
                            }
                            if !any_visible {
                                if let Some(last) = desktop_mgr.windows.last_mut() {
                                    last.is_active = true;
                                }
                            }
                            desktop_mgr.invalidate_all();
                        }
                    }
                }
                crate::drivers::keyboard::GuiInputEvent::FunctionKey { num, mods } => {
                    let act = desktop_mgr.handle_gui_key_event(
                        libgui::event::KeyCode::Function(num),
                        None,
                        mods,
                    );
                    handle_session_action(act, &mut running);
                }
                crate::drivers::keyboard::GuiInputEvent::KeyDown { key, ch, mods } => {
                    let act = desktop_mgr.handle_gui_key_event(key, ch, mods);
                    handle_session_action(act, &mut running);
                }
                crate::drivers::keyboard::GuiInputEvent::KeyUp { key, mods } => {
                    if !desktop_mgr.taskbar.start_menu_open {
                        if let Some(active_win) = desktop_mgr
                            .windows
                            .iter_mut()
                            .rev()
                            .find(|w| w.is_active && !w.is_minimized)
                        {
                            active_win.handle_gui_key_up(key, mods);
                        }
                    }
                }
            }
        }

        // 2b. Poll keyboard input (PS/2 + USB HID keyboard ring buffer, bounded per frame)
        let mut csi_state = 0u8;
        let mut csi_param = 0u8;
        for _ in 0..64 {
            let Some(byte) = read_char() else {
                break;
            };
            if csi_state == 1 {
                if byte == b'[' || byte == b'O' {
                    csi_state = 2;
                    csi_param = 0;
                    continue;
                }
                csi_state = 0;
                // Standalone Escape followed by another key: close active popup or forward Escape
                if desktop_mgr.desktop.context_menu.take().is_some() {
                    desktop_mgr.invalidate_all();
                } else if desktop_mgr.spotlight.is_open {
                    desktop_mgr.spotlight.is_open = false;
                    desktop_mgr.invalidate_all();
                } else if desktop_mgr.taskbar.start_menu_open {
                    let act = desktop_mgr.handle_gui_key_event(
                        libgui::event::KeyCode::Escape,
                        None,
                        libgui::event::Modifiers::default(),
                    );
                    handle_session_action(act, &mut running);
                } else if desktop_mgr.taskbar.tray_popup_open {
                    desktop_mgr.taskbar.tray_popup_open = false;
                    desktop_mgr.invalidate_all();
                } else {
                    let act = desktop_mgr.handle_gui_key_event(
                        libgui::event::KeyCode::Escape,
                        None,
                        libgui::event::Modifiers::default(),
                    );
                    handle_session_action(act, &mut running);
                }
            } else if csi_state == 2 {
                if (b'0'..=b'9').contains(&byte) {
                    csi_param = csi_param.saturating_mul(10).saturating_add(byte - b'0');
                    continue;
                }
                if byte == b';' {
                    continue;
                }
                if (0x40..=0x7E).contains(&byte) {
                    csi_state = 0;
                    let key_opt = match byte {
                        b'A' => Some(libgui::event::KeyCode::Up),
                        b'B' => Some(libgui::event::KeyCode::Down),
                        b'C' => Some(libgui::event::KeyCode::Right),
                        b'D' => Some(libgui::event::KeyCode::Left),
                        b'H' => Some(libgui::event::KeyCode::Home),
                        b'F' => Some(libgui::event::KeyCode::End),
                        b'~' => match csi_param {
                            3 => Some(libgui::event::KeyCode::Delete),
                            5 => Some(libgui::event::KeyCode::PageUp),
                            6 => Some(libgui::event::KeyCode::PageDown),
                            _ => None,
                        },
                        _ => None,
                    };
                    if let Some(k) = key_opt {
                        let act = desktop_mgr.handle_gui_key_event(k, None, libgui::event::Modifiers::default());
                        handle_session_action(act, &mut running);
                    }
                }
                continue;
            }

            if byte == 0xFF {
                continue;
            }
            if byte == b'\x1B' {
                csi_state = 1;
                continue;
            }
            let act = desktop_mgr.handle_key_event(byte as char);
            handle_session_action(act, &mut running);
        }
        if csi_state == 1 && running {
            // Standalone Escape (arrow/CSI keys always queue "\x1B[..." together)
            if desktop_mgr.desktop.context_menu.take().is_some() {
                desktop_mgr.invalidate_all();
            } else if desktop_mgr.spotlight.is_open {
                desktop_mgr.spotlight.is_open = false;
                desktop_mgr.invalidate_all();
            } else if desktop_mgr.taskbar.start_menu_open {
                let act = desktop_mgr.handle_gui_key_event(
                    libgui::event::KeyCode::Escape,
                    None,
                    libgui::event::Modifiers::default(),
                );
                handle_session_action(act, &mut running);
            } else if desktop_mgr.taskbar.tray_popup_open {
                desktop_mgr.taskbar.tray_popup_open = false;
                desktop_mgr.invalidate_all();
            } else {
                let act = desktop_mgr.handle_gui_key_event(
                    libgui::event::KeyCode::Escape,
                    None,
                    libgui::event::Modifiers::default(),
                );
                handle_session_action(act, &mut running);
            }
        }

        // 3. Poll serial input (COM1, bounded & 0xFF-safe so absent UARTs never hang)
        if running {
            for _ in 0..64 {
                let Some(byte) = read_rx_char() else {
                    break;
                };
                if byte == 0xFF {
                    break;
                }
                if byte == b'\x1B' {
                    running = false;
                    break;
                }
                let act = desktop_mgr.handle_key_event(byte as char);
                handle_session_action(act, &mut running);
            }
        }

        let frame_start = crate::arch::x86_64::pit::get_ticks();
        let tsc_deadline = crate::arch::x86_64::pit::read_tsc().saturating_add(40_000_000);

        // 4. Render frame
        desktop_mgr.render_frame();

        if !crate::arch::x86_64::pit::IN_KERNEL_TESTS.load(core::sync::atomic::Ordering::Relaxed) {
            crate::net::poll_periodic();
        }

        // 5. Throttled frame pacing (~60 FPS target): wait up to 16ms with immediate input wakeup
        while crate::arch::x86_64::pit::get_ticks().saturating_sub(frame_start) < 16 {
            if crate::arch::x86_64::pit::read_tsc() >= tsc_deadline {
                break;
            }
            if crate::drivers::keyboard::poll_ps2_controller()
                || crate::drivers::usb::poll_hid_nonblocking()
                || crate::drivers::keyboard::poll_typematic()
                || crate::drivers::keyboard::has_pending_input()
            {
                break;
            }
            let (mx, my, lbtn) = {
                let m = MOUSE.lock();
                (m.x, m.y, m.left_button)
            };
            if mx != desktop_mgr.mouse_x || my != desktop_mgr.mouse_y || lbtn != desktop_mgr.prev_left_down {
                break;
            }
            crate::task::scheduler::yield_now();
            core::hint::spin_loop();
        }
    }

    for client in desktop_mgr.remote_clients.drain(..) {
        let _ = crate::net::unix::unix_socket_close(client.sock_id);
    }
    if let Some(lid) = desktop_mgr.wm_listen_sock_id.take() {
        let _ = crate::net::unix::unix_socket_close(lid);
    }

    crate::drivers::gop_console::set_gui_active(false);
    DESKTOP_ACTIVE.store(false, Ordering::Release);
    kinfo!("Exited Sublime Desktop GUI. Returning to SublimeSH text shell...");
}
