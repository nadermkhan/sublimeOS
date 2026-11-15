use alloc::string::String;
use crate::gui::apps::{AboutApp, CalcApp, FileManagerApp, SettingsApp, SysmonApp, TerminalApp, TextEditApp};
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::desktop::DesktopAppId;
use crate::gui::font::{draw_ui_string_ellipsis, measure_ui_string};

pub const TITLEBAR_HEIGHT: i32 = 30;
pub const FRAME_BORDER: i32 = 1;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ResizeEdge {
    None,
    Right,
    Bottom,
    BottomRight,
}

/// Shared-memory Ring-3 application surface attached over `/run/wm.sock` and `/dev/shm/*`.
pub struct RemoteSurface {
    pub client_sock_id: usize,
    pub remote_win_id: u32,
    pub owner_pid: Option<u32>,
    pub shm_name: String,
    pub shm_width: i32,
    pub shm_height: i32,
    pub last_sent_w: i32,
    pub last_sent_h: i32,
}

impl RemoteSurface {
    pub fn send_server_msg(&self, msg: &libgui::ipc::WmServerMsg) {
        let bytes = libgui::ipc::encode_server_msg(msg);
        let _ = crate::net::unix::unix_socket_send(self.client_sock_id, &bytes);
    }

    pub fn send_input(&self, event: libgui::event::GuiEvent) {
        self.send_server_msg(&libgui::ipc::WmServerMsg::Input {
            window_id: self.remote_win_id,
            event,
        });
    }

    pub fn sync_configure(&mut self, w: i32, h: i32, theme_kind: u8) {
        if w != self.last_sent_w || h != self.last_sent_h {
            self.last_sent_w = w;
            self.last_sent_h = h;
            self.send_server_msg(&libgui::ipc::WmServerMsg::Configure {
                window_id: self.remote_win_id,
                width: w,
                height: h,
                theme_kind,
            });
        }
    }

    pub fn render(&self, canvas: &mut Canvas, ix: i32, iy: i32, _iw: i32, _ih: i32) {
        if self.shm_name.is_empty() || self.shm_width <= 0 || self.shm_height <= 0 {
            return;
        }
        let shm_opt = crate::mm::shm::SHM_REGISTRY.lock().get(&self.shm_name);
        if let Some(shm_arc) = shm_opt {
            let shm = shm_arc.lock();
            let (cx0, cy0, cx1, cy1) = canvas.clip_bounds();
            let cw = canvas.width as i32;
            let ch = canvas.height as i32;
            shm.blit_argb_to_canvas(
                self.shm_width,
                self.shm_height,
                canvas.buffer,
                cw,
                ch,
                ix,
                iy,
                (cx0, cy0, (cx1 - cx0).max(0), (cy1 - cy0).max(0)),
            );
        }
    }
}

pub enum WindowApp {
    Terminal(TerminalApp),
    Sysmon(SysmonApp),
    Files(FileManagerApp),
    About(AboutApp),
    Calc(CalcApp),
    Editor(TextEditApp),
    Settings(SettingsApp),
    Remote(RemoteSurface),
}

impl WindowApp {
    pub fn app_id(&self) -> DesktopAppId {
        match self {
            WindowApp::Terminal(_) => DesktopAppId::Terminal,
            WindowApp::Sysmon(_) => DesktopAppId::Sysmon,
            WindowApp::Files(_) => DesktopAppId::FileManager,
            WindowApp::About(_) => DesktopAppId::About,
            WindowApp::Calc(_) => DesktopAppId::Calculator,
            WindowApp::Editor(_) => DesktopAppId::TextEdit,
            WindowApp::Settings(_) => DesktopAppId::Settings,
            WindowApp::Remote(_) => DesktopAppId::RemoteGui,
        }
    }
}

pub struct Window {
    pub id: u32,
    pub title: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub is_active: bool,
    pub is_minimized: bool,
    pub is_maximized: bool,
    pub pre_max_rect: (i32, i32, i32, i32),
    pub dragging: bool,
    pub drag_offset_x: i32,
    pub drag_offset_y: i32,
    pub resizing: bool,
    pub resize_edge: ResizeEdge,
    pub min_width: i32,
    pub min_height: i32,
    pub app: WindowApp,
}

impl Window {
    pub fn new(id: u32, title: &str, x: i32, y: i32, width: i32, height: i32, app: WindowApp) -> Self {
        let (min_w, min_h) = match app {
            WindowApp::Calc(_) => (280, 340),
            WindowApp::About(_) => (380, 260),
            _ => (280, 180),
        };

        Self {
            id,
            title: String::from(title),
            x,
            y,
            width,
            height,
            is_active: true,
            is_minimized: false,
            is_maximized: false,
            pre_max_rect: (x, y, width, height),
            dragging: false,
            drag_offset_x: 0,
            drag_offset_y: 0,
            resizing: false,
            resize_edge: ResizeEdge::None,
            min_width: min_w,
            min_height: min_h,
            app,
        }
    }

    pub fn contains_resize_handle(&self, px: i32, py: i32) -> ResizeEdge {
        if self.is_minimized || self.is_maximized {
            return ResizeEdge::None;
        }
        let rx = self.x + self.width;
        let by = self.y + self.height;

        if px >= rx - 14 && px <= rx + 2 && py >= by - 14 && py <= by + 2 {
            return ResizeEdge::BottomRight;
        }
        if px >= rx - 6 && px <= rx + 2 && py >= self.y + TITLEBAR_HEIGHT && py < by - 14 {
            return ResizeEdge::Right;
        }
        if py >= by - 6 && py <= by + 2 && px >= self.x && px < rx - 14 {
            return ResizeEdge::Bottom;
        }
        ResizeEdge::None
    }

    pub fn inner_bounds(&self) -> (i32, i32, i32, i32) {
        (
            self.x + FRAME_BORDER,
            self.y + TITLEBAR_HEIGHT,
            self.width - FRAME_BORDER * 2,
            self.height - TITLEBAR_HEIGHT - FRAME_BORDER,
        )
    }

    pub fn contains(&self, px: i32, py: i32) -> bool {
        if self.is_minimized {
            return false;
        }
        px >= self.x && px < self.x + self.width && py >= self.y && py < self.y + self.height
    }

    pub fn contains_titlebar(&self, px: i32, py: i32) -> bool {
        if self.is_minimized {
            return false;
        }
        px >= self.x && px < self.x + self.width && py >= self.y && py < self.y + TITLEBAR_HEIGHT
    }

    pub fn contains_close_btn(&self, px: i32, py: i32) -> bool {
        let bx = self.x + self.width - 43;
        let by = self.y + 1;
        px >= bx && px <= bx + 42 && py >= by && py <= by + 23
    }

    pub fn contains_maximize_btn(&self, px: i32, py: i32) -> bool {
        let bx = self.x + self.width - 75;
        let by = self.y + 1;
        px >= bx && px < bx + 32 && py >= by && py <= by + 23
    }

    pub fn contains_minimize_btn(&self, px: i32, py: i32) -> bool {
        let bx = self.x + self.width - 107;
        let by = self.y + 1;
        px >= bx && px < bx + 32 && py >= by && py <= by + 23
    }

    pub fn toggle_maximize(&mut self, screen_w: i32, screen_h: i32) {
        if self.is_maximized {
            self.x = self.pre_max_rect.0;
            self.y = self.pre_max_rect.1;
            self.width = self.pre_max_rect.2;
            self.height = self.pre_max_rect.3;
            self.is_maximized = false;
        } else {
            self.pre_max_rect = (self.x, self.y, self.width, self.height);
            self.x = 0;
            self.y = 0;
            self.width = screen_w;
            self.height = (screen_h - 36).max(200); // Fill workspace above Metro Taskbar
            self.is_maximized = true;
        }
    }

    pub fn dismiss_popups(&mut self) {
        if let WindowApp::Files(ref mut files) = self.app {
            files.dismiss_popups();
        }
    }

    pub fn deactivate(&mut self) {
        self.is_active = false;
        match self.app {
            WindowApp::Files(ref mut files) => files.on_deactivate(),
            WindowApp::Remote(ref rem) => {
                rem.send_input(libgui::event::GuiEvent::FocusChanged { focused: false });
            }
            _ => {}
        }
    }

    pub fn handle_gui_key(
        &mut self,
        key: libgui::event::KeyCode,
        ch: Option<char>,
        mods: libgui::event::Modifiers,
    ) {
        let block_non_files = (mods.ctrl || mods.alt)
            && !matches!(self.app, WindowApp::Files(_) | WindowApp::Remote(_));
        if block_non_files {
            match self.app {
                WindowApp::Terminal(ref mut term) => {
                    if mods.ctrl && !mods.alt {
                        if let Some(c) = ch {
                            if c.eq_ignore_ascii_case(&'h') {
                                term.handle_char('\x08');
                                return;
                            }
                            if c.eq_ignore_ascii_case(&'c') || c.eq_ignore_ascii_case(&'v') {
                                term.handle_key(key, ch, mods);
                            }
                        }
                    }
                    return;
                }
                WindowApp::Calc(ref mut calc) => {
                    if mods.ctrl && !mods.alt {
                        if let Some(c) = ch {
                            if c.eq_ignore_ascii_case(&'c') || c.eq_ignore_ascii_case(&'v') {
                                calc.handle_key(key, ch, mods);
                            }
                        }
                    }
                    return;
                }
                _ => return,
            }
        }
        match self.app {
            WindowApp::Terminal(ref mut term) => term.handle_key(key, ch, mods),
            WindowApp::Calc(ref mut calc) => calc.handle_key(key, ch, mods),
            WindowApp::Editor(ref mut ed) => ed.handle_key(key, ch),
            WindowApp::Settings(ref mut s) => s.handle_key(key, ch),
            WindowApp::Files(ref mut f) => f.handle_key_event(key, ch, mods),
            WindowApp::Remote(ref rem) => {
                rem.send_input(libgui::event::GuiEvent::KeyDown { key, ch, mods });
            }
            _ => {}
        }
    }

    pub fn handle_gui_key_up(
        &mut self,
        key: libgui::event::KeyCode,
        mods: libgui::event::Modifiers,
    ) {
        if let WindowApp::Remote(ref rem) = self.app {
            rem.send_input(libgui::event::GuiEvent::KeyUp { key, mods });
        }
    }

    pub fn handle_char(&mut self, ch: char) {
        let b = if ch.is_ascii() { ch as u8 } else { b'?' };
        let (key, ch_opt, mods) = if (1..=26).contains(&b) && b != b'\t' && b != b'\n' && b != b'\r' && b != 0x08 {
            let letter = (b'a' + (b - 1)) as char;
            (
                libgui::event::KeyCode::Char(letter),
                Some(letter),
                libgui::event::Modifiers {
                    shift: false,
                    ctrl: true,
                    alt: false,
                },
            )
        } else {
            let printable = if (0x20..0x7F).contains(&(b as u32)) {
                Some(ch)
            } else {
                None
            };
            (
                libgui::event::KeyCode::from_byte(b),
                printable,
                libgui::event::Modifiers::default(),
            )
        };
        if !matches!(self.app, WindowApp::Remote(_)) && (0x01..=0x7F).contains(&(b as u32)) {
            match self.app {
                WindowApp::Terminal(ref mut term) => term.handle_char(ch),
                WindowApp::Calc(ref mut calc) => calc.handle_char(ch),
                WindowApp::Editor(ref mut ed) => ed.handle_char(ch),
                WindowApp::Settings(ref mut s) => s.handle_char(ch),
                WindowApp::Files(ref mut f) => {
                    if mods.ctrl || b == b'\t' || b == 0x08 || b == 0x7F || b == b'\n' || b == b'\r' {
                        let mapped_key = match b {
                            0x08 | 0x7F => libgui::event::KeyCode::Backspace,
                            b'\n' | b'\r' => libgui::event::KeyCode::Enter,
                            b'\t' => libgui::event::KeyCode::Tab,
                            _ => key,
                        };
                        f.handle_key_event(mapped_key, ch_opt, mods);
                    } else {
                        f.handle_char(ch);
                    }
                }
                _ => {}
            }
        } else {
            self.handle_gui_key(key, ch_opt, mods);
        }
    }

    pub fn handle_inner_click(&mut self, px: i32, py: i32) {
        let (ix, iy, iw, ih) = self.inner_bounds();
        match self.app {
            WindowApp::Terminal(ref mut term) => term.handle_click(px, py, ix, iy, iw, ih),
            WindowApp::Files(ref mut files) => files.handle_click(px, py, ix, iy, iw, ih),
            WindowApp::Sysmon(ref mut sysmon) => sysmon.handle_click(px, py, ix, iy, iw, ih),
            WindowApp::Calc(ref mut calc) => calc.handle_click(px, py, ix, iy, iw, ih),
            WindowApp::Editor(ref mut ed) => ed.handle_click(px, py, ix, iy, iw, ih),
            WindowApp::Settings(ref mut s) => s.handle_click(px, py, ix, iy, iw, ih),
            WindowApp::Remote(ref rem) => {
                rem.send_input(libgui::event::GuiEvent::MouseDown {
                    x: px - ix,
                    y: py - iy,
                    button: libgui::event::MouseButton::Left,
                });
            }
            _ => {}
        }
    }

    pub fn handle_inner_mouse_move(&mut self, px: i32, py: i32, left_down: bool) -> bool {
        let (ix, iy, iw, ih) = self.inner_bounds();
        match self.app {
            WindowApp::Files(ref mut files) => files.handle_mouse_move(px, py, ix, iy, iw, ih),
            WindowApp::Sysmon(ref mut sysmon) => sysmon.handle_mouse_move(px, py, ix, iy, iw, ih),
            WindowApp::Calc(ref mut calc) => calc.handle_mouse_move(px, py, ix, iy, iw, ih),
            WindowApp::Editor(ref mut ed) => ed.handle_mouse_move(px, py, ix, iy, iw, ih),
            WindowApp::Settings(ref mut s) => s.handle_mouse_move(px, py, left_down, ix, iy, iw, ih),
            WindowApp::Terminal(ref mut term) => term.handle_mouse_move(px, py, ix, iy, iw, ih),
            _ => false,
        }
    }

    pub fn handle_inner_mouse_up(&mut self, px: i32, py: i32) -> bool {
        let (ix, iy, iw, ih) = self.inner_bounds();
        match self.app {
            WindowApp::Terminal(ref mut term) => term.handle_mouse_up(px, py, ix, iy, iw, ih),
            WindowApp::Editor(ref mut ed) => ed.handle_mouse_up(px, py, ix, iy, iw, ih),
            WindowApp::Files(ref mut files) => files.handle_mouse_up(px, py, ix, iy, iw, ih),
            WindowApp::Sysmon(ref mut sysmon) => sysmon.handle_mouse_up(px, py, ix, iy, iw, ih),
            WindowApp::Settings(ref mut s) => s.handle_mouse_up(),
            WindowApp::Calc(ref mut calc) => {
                if calc.active_btn_idx.take().is_some() {
                    return true;
                }
                false
            }
            _ => false,
        }
    }

    pub fn handle_inner_right_click(&mut self, px: i32, py: i32) {
        let (ix, iy, iw, ih) = self.inner_bounds();
        match self.app {
            WindowApp::Files(ref mut files) => files.handle_right_click(px, py, ix, iy, iw, ih),
            WindowApp::Sysmon(ref mut sysmon) => {
                let _ = sysmon.handle_right_click(px, py, ix, iy, iw, ih);
            }
            WindowApp::Calc(ref mut calc) => calc.handle_right_click(px, py, ix, iy, iw, ih),
            WindowApp::Editor(ref mut ed) => ed.handle_right_click(px, py, ix, iy, iw, ih),
            WindowApp::Terminal(ref mut term) => term.handle_right_click(px, py, ix, iy, iw, ih),
            WindowApp::Remote(ref rem) => {
                rem.send_input(libgui::event::GuiEvent::MouseDown {
                    x: px - ix,
                    y: py - iy,
                    button: libgui::event::MouseButton::Right,
                });
            }
            _ => {}
        }
    }

    pub fn handle_wheel(&mut self, px: i32, py: i32, delta_y: i32) -> bool {
        if delta_y == 0 || self.is_minimized {
            return false;
        }
        let (ix, iy, iw, ih) = self.inner_bounds();
        match self.app {
            WindowApp::Files(ref mut files) => files.handle_wheel(px, py, delta_y, ix, iy, iw, ih),
            WindowApp::Settings(ref mut s) => s.handle_wheel(px, py, delta_y, ix, iy, iw, ih),
            WindowApp::Sysmon(ref mut sysmon) => sysmon.handle_wheel(delta_y),
            WindowApp::Editor(ref mut ed) => ed.handle_wheel(delta_y),
            WindowApp::Terminal(ref mut term) => term.handle_wheel(delta_y),
            WindowApp::Remote(ref rem) => {
                rem.send_input(libgui::event::GuiEvent::MouseWheel {
                    x: px - ix,
                    y: py - iy,
                    delta_y,
                });
                true
            }
            _ => false,
        }
    }

    pub fn render(&mut self, canvas: &mut Canvas, mouse_x: i32, mouse_y: i32) {
        if self.is_minimized {
            return;
        }
        let theme = libgui::gfx::current_theme();

        // 1. Crisp minimal drop shadow (Windows 8.1 thin flat shadow)
        canvas.fill_rect(self.x + 2, self.y + 2, self.width, self.height, Color::argb(28, 0, 0, 0));

        // 2. Sharp 1px Flat Frame (no thick bevels or rounded edges)
        let border_col = if self.is_active {
            Color::from_libgui(theme.frame_active)
        } else {
            Color::from_libgui(theme.frame_inactive)
        };
        let title_bg = if self.is_active {
            Color::from_libgui(theme.title_top)
        } else {
            Color::from_libgui(theme.title_inactive_top)
        };

        canvas.fill_rect(self.x, self.y, self.width, TITLEBAR_HEIGHT, title_bg);
        canvas.draw_rect(self.x, self.y, self.width, self.height, border_col);

        // 3. Mini 14x14 App Icon (Left) & Windows 8.1 Centered Titlebar Text
        draw_mini_window_icon(canvas, self.x + 8, self.y + 8, self.app.app_id());
        let title_area_x = self.x + 28;
        let title_max_w = (self.width - 142).max(24);
        let tw = measure_ui_string(&self.title);
        let centered_x = self.x + (self.width - tw.min(title_max_w)) / 2;
        let tx = centered_x.max(title_area_x);
        let active_title_fg = Color::from_libgui(libgui::gfx::contrast_text_on(theme.title_top));
        let title_fg = if self.is_active {
            active_title_fg
        } else {
            Color::from_libgui(theme.text_secondary)
        };
        canvas.set_clip_rect(title_area_x, self.y + 2, title_max_w, TITLEBAR_HEIGHT - 4);
        draw_ui_string_ellipsis(canvas, tx, self.y + 6, &self.title, title_fg, title_max_w);
        canvas.clear_clip_rect();

        // 4. Flush Top-Right Rectangular Windows 8.1 Caption Buttons ([ _ ] [ □ ] [  X  ])
        let btn_y = self.y + 1;
        let btn_h = 23;
        let min_x = self.x + self.width - 107;
        let max_x = self.x + self.width - 75;
        let cls_x = self.x + self.width - 43;

        let min_hover = mouse_x >= min_x && mouse_x < max_x && mouse_y >= btn_y && mouse_y <= btn_y + btn_h;
        let max_hover = mouse_x >= max_x && mouse_x < cls_x && mouse_y >= btn_y && mouse_y <= btn_y + btn_h;
        let cls_hover = mouse_x >= cls_x && mouse_x <= cls_x + 42 && mouse_y >= btn_y && mouse_y <= btn_y + btn_h;

        let cap_hover_bg = if self.is_active {
            Color::argb(48, 255, 255, 255)
        } else {
            Color::argb(35, 0, 0, 0)
        };
        let glyph_col = if self.is_active { active_title_fg } else { Color::from_libgui(theme.text_secondary) };

        // Minimize (_)
        if min_hover {
            canvas.fill_rect(min_x, btn_y, 32, btn_h, cap_hover_bg);
        }
        canvas.fill_rect(min_x + 11, btn_y + 15, 10, 2, glyph_col);

        // Maximize / Restore (□)
        if max_hover {
            canvas.fill_rect(max_x, btn_y, 32, btn_h, cap_hover_bg);
        }
        canvas.draw_rect(max_x + 11, btn_y + 7, 10, 10, glyph_col);
        canvas.fill_rect(max_x + 11, btn_y + 7, 10, 2, glyph_col);

        // Signature Wide Flat Close Block (X)
        let cls_bg = if cls_hover {
            Color::XP_CAP_RED_HOVER_TOP
        } else if self.is_active {
            Color::XP_CAP_RED_TOP
        } else {
            Color::rgb(184, 184, 184)
        };
        canvas.fill_rect(cls_x, btn_y, 42, btn_h, cls_bg);
        let cx0 = cls_x + 16;
        let cy0 = btn_y + 7;
        for i in 0..9 {
            canvas.fill_rect(cx0 + i, cy0 + i, 2, 2, Color::WHITE);
            canvas.fill_rect(cx0 + 8 - i, cy0 + i, 2, 2, Color::WHITE);
        }

        // 5. Render App Content strictly clipped inside inner client bounds
        let (ix, iy, iw, ih) = self.inner_bounds();
        canvas.set_clip_rect(ix, iy, iw, ih);
        canvas.fill_rect(ix, iy, iw, ih, Color::from_libgui(theme.face));
        match self.app {
            WindowApp::Terminal(ref mut term) => term.render(canvas, ix, iy, iw, ih),
            WindowApp::Sysmon(ref mut sysmon) => sysmon.render(canvas, ix, iy, iw, ih),
            WindowApp::Files(ref mut files) => files.render(canvas, ix, iy, iw, ih),
            WindowApp::About(ref about) => about.render(canvas, ix, iy, iw, ih),
            WindowApp::Calc(ref calc) => calc.render(canvas, ix, iy, iw, ih),
            WindowApp::Editor(ref mut ed) => ed.render(canvas, ix, iy, iw, ih),
            WindowApp::Settings(ref mut settings) => settings.render(canvas, ix, iy, iw, ih),
            WindowApp::Remote(ref rem) => rem.render(canvas, ix, iy, iw, ih),
        }
        canvas.clear_clip_rect();
    }
}

/// Map an application ID + optional title/name key to a distinct Metro badge color.
pub fn app_tile_color(app_id: DesktopAppId, key: &str) -> Color {
    match key {
        "app center" => Color::rgb(0, 137, 123),
        "sublime paint" => Color::rgb(216, 79, 52),
        "music player" => Color::rgb(174, 41, 118),
        "media player" => Color::rgb(198, 40, 40),
        "code editor" => Color::rgb(57, 73, 171),
        "widget studio" => Color::rgb(0, 120, 215),
        _ => match app_id {
            DesktopAppId::Terminal => Color::rgb(30, 34, 43),
            DesktopAppId::FileManager => Color::rgb(214, 156, 30),
            DesktopAppId::Sysmon => Color::rgb(16, 124, 16),
            DesktopAppId::TextEdit => Color::rgb(45, 137, 239),
            DesktopAppId::Calculator => Color::rgb(126, 56, 120),
            DesktopAppId::Settings | DesktopAppId::About => {
                Color::from_libgui(libgui::gfx::current_theme().accent)
            }
            DesktopAppId::Studio => Color::rgb(0, 120, 215),
            DesktopAppId::RemoteGui => Color::rgb(81, 51, 171),
        },
    }
}

/// Draw a 14x14 mini app badge icon using `app_id` plus an optional lowercase app key.
pub fn draw_app_badge_icon(canvas: &mut Canvas, x: i32, y: i32, app_id: DesktopAppId, key: &str) {
    if key == "app center" {
        canvas.draw_rect(x + 1, y + 3, 12, 10, Color::WHITE);
        canvas.draw_rect(x + 4, y + 1, 6, 3, Color::rgb(190, 245, 235));
        canvas.fill_rect(x + 3, y + 5, 3, 3, Color::rgb(160, 255, 220));
        canvas.fill_rect(x + 8, y + 5, 3, 3, Color::WHITE);
        canvas.fill_rect(x + 3, y + 9, 3, 2, Color::WHITE);
        canvas.fill_rect(x + 8, y + 9, 3, 2, Color::rgb(160, 255, 220));
        return;
    }
    if key == "sublime paint" {
        canvas.fill_rect(x + 1, y + 2, 12, 10, Color::rgb(255, 248, 240));
        canvas.draw_rect(x + 1, y + 2, 12, 10, Color::WHITE);
        canvas.fill_rect(x + 3, y + 4, 3, 3, Color::rgb(255, 200, 50));
        canvas.fill_rect(x + 8, y + 4, 3, 3, Color::rgb(65, 170, 255));
        canvas.fill_rect(x + 3, y + 8, 3, 2, Color::rgb(70, 210, 120));
        canvas.draw_line(x + 7, y + 10, x + 12, y + 5, Color::rgb(85, 45, 25));
        return;
    }
    if key == "music player" {
        canvas.fill_rect(x + 5, y + 2, 7, 2, Color::WHITE);
        canvas.fill_rect(x + 5, y + 2, 2, 8, Color::WHITE);
        canvas.fill_rect(x + 10, y + 2, 2, 7, Color::WHITE);
        canvas.fill_rect(x + 2, y + 9, 5, 3, Color::rgb(255, 220, 245));
        canvas.fill_rect(x + 8, y + 8, 4, 3, Color::rgb(255, 220, 245));
        return;
    }
    if key == "media player" {
        canvas.fill_rect(x + 1, y + 2, 12, 10, Color::rgb(38, 16, 20));
        canvas.draw_rect(x + 1, y + 2, 12, 10, Color::WHITE);
        for row in 0..7i32 {
            let half = 3 - (row - 3).abs();
            canvas.fill_rect(x + 5, y + 4 + row, (half + 1).max(1), 1, Color::WHITE);
        }
        return;
    }
    if key == "code editor" {
        canvas.fill_rect(x + 1, y + 2, 12, 10, Color::rgb(26, 30, 56));
        canvas.draw_rect(x + 1, y + 2, 12, 10, Color::rgb(190, 205, 255));
        canvas.draw_line(x + 5, y + 4, x + 3, y + 7, Color::rgb(120, 225, 255));
        canvas.draw_line(x + 3, y + 7, x + 5, y + 10, Color::rgb(120, 225, 255));
        canvas.draw_line(x + 9, y + 4, x + 11, y + 7, Color::rgb(255, 205, 110));
        canvas.draw_line(x + 11, y + 7, x + 9, y + 10, Color::rgb(255, 205, 110));
        return;
    }

    match app_id {
        DesktopAppId::Terminal => {
            canvas.fill_rect(x, y + 1, 14, 12, Color::BLACK);
            canvas.fill_rect(x, y + 1, 14, 3, Color::rgb(120, 175, 255));
            canvas.draw_rect(x, y + 1, 14, 12, Color::WHITE);
            canvas.fill_rect(x + 2, y + 6, 4, 2, Color::rgb(220, 220, 220));
            canvas.fill_rect(x + 7, y + 9, 4, 1, Color::rgb(220, 220, 220));
        }
        DesktopAppId::FileManager => {
            canvas.fill_rect(x + 1, y + 2, 6, 3, Color::rgb(245, 200, 60));
            canvas.fill_rect(x, y + 4, 14, 9, Color::rgb(252, 212, 82));
            canvas.draw_rect(x, y + 4, 14, 9, Color::rgb(160, 115, 20));
        }
        DesktopAppId::TextEdit => {
            canvas.fill_rect(x + 2, y + 1, 10, 12, Color::WHITE);
            canvas.fill_rect(x + 2, y + 1, 10, 3, Color::rgb(52, 130, 232));
            canvas.draw_rect(x + 2, y + 1, 10, 12, Color::rgb(70, 95, 135));
        }
        DesktopAppId::Calculator => {
            canvas.fill_rect(x + 2, y + 1, 10, 12, Color::rgb(156, 192, 236));
            canvas.fill_rect(x + 4, y + 3, 6, 3, Color::WHITE);
            canvas.draw_rect(x + 2, y + 1, 10, 12, Color::rgb(45, 80, 130));
        }
        DesktopAppId::Sysmon => {
            canvas.fill_rect(x + 1, y + 1, 12, 12, Color::BLACK);
            canvas.draw_rect(x + 1, y + 1, 12, 12, Color::rgb(190, 210, 235));
            canvas.draw_line(x + 2, y + 9, x + 5, y + 5, Color::rgb(0, 255, 64));
            canvas.draw_line(x + 5, y + 5, x + 8, y + 8, Color::rgb(0, 255, 64));
            canvas.draw_line(x + 8, y + 8, x + 11, y + 3, Color::rgb(0, 255, 64));
        }
        DesktopAppId::Settings | DesktopAppId::About => {
            let th = libgui::gfx::current_theme();
            canvas.fill_rect(x + 1, y + 1, 12, 12, Color::from_libgui(th.face_dark));
            crate::gui::desktop::draw_sublime_logo(
                canvas,
                x + 1,
                y + 1,
                12,
                Color::from_libgui(th.accent),
                Color::rgb(96, 205, 255),
            );
        }
        DesktopAppId::Studio | DesktopAppId::RemoteGui => {
            canvas.fill_rect(x + 1, y + 1, 12, 12, Color::rgb(24, 118, 184));
            canvas.fill_rect(x + 2, y + 2, 10, 3, Color::rgb(78, 178, 245));
            canvas.fill_rect(x + 3, y + 6, 8, 5, Color::WHITE);
            canvas.fill_rect(x + 4, y + 7, 3, 3, Color::rgb(235, 135, 40));
        }
    }
}

pub fn draw_mini_window_icon(canvas: &mut Canvas, x: i32, y: i32, app_id: DesktopAppId) {
    draw_app_badge_icon(canvas, x, y, app_id, "");
}
