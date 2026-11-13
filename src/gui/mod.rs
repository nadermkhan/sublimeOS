pub mod apps;
pub mod canvas;
pub mod color;
pub mod desktop;
pub mod font;
pub mod mouse;
pub mod spotlight;
pub mod taskbar;
pub mod window;

use alloc::vec::Vec;
use crate::arch::x86_64::serial::read_rx_char;
use crate::drivers::keyboard::read_char;
use crate::drivers::mouse::MOUSE;
use crate::drivers::vbe::{self, VBE};
use crate::gui::apps::{AboutApp, CalcApp, FileManagerApp, SettingsApp, SysmonApp, TerminalApp, TextEditApp};
use crate::gui::canvas::Canvas;
use crate::gui::desktop::{Desktop, DesktopAppId};
use crate::gui::mouse::{draw_cursor, CursorType};
use crate::gui::spotlight::{Spotlight, SpotlightTarget};
use crate::gui::taskbar::{Taskbar, TaskbarAction};
use crate::gui::window::{ResizeEdge, Window, WindowApp};
use crate::kinfo;

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
    pub dirty: bool,
    last_sec: u8,
    last_blink: u64,
}

impl DesktopManager {
    pub fn new(width: u32, height: u32) -> Self {
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
            dirty: true,
            last_sec: 255,
            last_blink: 0,
        }
    }

    pub fn open_app(&mut self, app_id: DesktopAppId) {
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
                "Terminal — sublime — 80x24",
                90,
                45,
                580,
                380,
                WindowApp::Terminal(TerminalApp::new()),
            ),
            DesktopAppId::Sysmon => Window::new(
                id,
                "Activity Monitor (CPU & Unified Memory)",
                360,
                100,
                540,
                380,
                WindowApp::Sysmon(SysmonApp::new()),
            ),
            DesktopAppId::FileManager => Window::new(
                id,
                "Finder",
                140,
                130,
                580,
                380,
                WindowApp::Files(FileManagerApp::new()),
            ),
            DesktopAppId::About => Window::new(
                id,
                "About SublimeOS",
                260,
                90,
                480,
                330,
                WindowApp::About(AboutApp::new()),
            ),
            DesktopAppId::Calculator => Window::new(
                id,
                "Calculator",
                440,
                80,
                240,
                340,
                WindowApp::Calc(CalcApp::new()),
            ),
            DesktopAppId::TextEdit => Window::new(
                id,
                "TextEdit — untitled.txt",
                180,
                70,
                520,
                360,
                WindowApp::Editor(TextEditApp::new()),
            ),
            DesktopAppId::Settings => Window::new(
                id,
                "System Settings",
                200,
                80,
                560,
                380,
                WindowApp::Settings(SettingsApp::new()),
            ),
        };

        // Deactivate other windows
        for w in self.windows.iter_mut() {
            w.is_active = false;
        }

        self.windows.push(win);
        self.dirty = true;
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
            for w in self.windows.iter_mut() {
                w.is_active = false;
            }
            win.is_active = true;
            win.is_minimized = false;
            self.windows.push(win);
            self.dirty = true;
        }
    }

    pub fn close_window(&mut self, window_id: u32) {
        if let Some(idx) = self.windows.iter().position(|w| w.id == window_id) {
            self.windows.remove(idx);
            if let Some(last) = self.windows.last_mut() {
                last.is_active = true;
            }
            self.dirty = true;
        }
    }

    pub fn handle_mouse_event(&mut self, x: i32, y: i32, left_down: bool) -> TaskbarAction {
        if x != self.mouse_x || y != self.mouse_y {
            self.mouse_x = x;
            self.mouse_y = y;
            self.dirty = true;
        }

        let sw = self.canvas.width as i32;
        let sh = self.canvas.height as i32;
        let result_action = TaskbarAction::None;

        let left_clicked = left_down && !self.prev_left_down;
        let left_released = !left_down && self.prev_left_down;
        self.prev_left_down = left_down;

        if left_clicked || left_released {
            self.dirty = true;
        }

        // 1. If currently resizing a window, update its size
        if let Some(res_win) = self.windows.iter_mut().find(|w| w.resizing) {
            if left_down {
                match res_win.resize_edge {
                    crate::gui::window::ResizeEdge::BottomRight => {
                        res_win.width = (x - res_win.x).clamp(res_win.min_width, sw - res_win.x);
                        res_win.height = (y - res_win.y).clamp(res_win.min_height, sh - res_win.y - 64);
                    }
                    crate::gui::window::ResizeEdge::Right => {
                        res_win.width = (x - res_win.x).clamp(res_win.min_width, sw - res_win.x);
                    }
                    crate::gui::window::ResizeEdge::Bottom => {
                        res_win.height = (y - res_win.y).clamp(res_win.min_height, sh - res_win.y - 64);
                    }
                    _ => {}
                }
                self.dirty = true;
            }
        }

        // 2. If currently dragging a window, update its position with macOS boundary limits
        if let Some(drag_win) = self.windows.iter_mut().find(|w| w.dragging) {
            if left_down {
                drag_win.x = (x - drag_win.drag_offset_x).clamp(-drag_win.width + 60, sw - 60);
                drag_win.y = (y - drag_win.drag_offset_y).clamp(26, sh - 40);
                self.dirty = true;
            }
        }

        if left_released {
            for w in self.windows.iter_mut() {
                w.dragging = false;
                w.resizing = false;
                w.resize_edge = crate::gui::window::ResizeEdge::None;
            }
            self.dirty = true;
        }

        if left_clicked {
            // Check Spotlight interaction if open
            if self.spotlight.is_open {
                let modal_w = 540;
                let results = self.spotlight.get_results();
                let item_h = 32;
                let modal_h = 56 + (results.len().min(5) as i32 * item_h);
                let modal_x = (sw - modal_w) / 2;
                let modal_y = 100;

                if x >= modal_x && x <= modal_x + modal_w && y >= modal_y && y <= modal_y + modal_h {
                    if y >= modal_y + 48 {
                        let idx = ((y - (modal_y + 54)) / item_h) as usize;
                        if let Some(item) = results.get(idx) {
                            match &item.target {
                                SpotlightTarget::LaunchApp(app_id) => self.open_app(*app_id),
                                SpotlightTarget::Calculation(_) => {}
                            }
                            self.spotlight.is_open = false;
                        }
                    }
                    self.dirty = true;
                    return TaskbarAction::None;
                } else {
                    self.spotlight.is_open = false;
                    self.dirty = true;
                }
            }

            // 3. Check taskbar / start menu click
            let action = self.taskbar.handle_click(x, y, &self.windows, sw, sh);
            match action {
                TaskbarAction::FocusWindow(win_id) => {
                    let mut is_active_now = false;
                    for w in self.windows.iter_mut() {
                        if w.id == win_id && w.is_active && !w.is_minimized {
                            w.is_minimized = true;
                            is_active_now = true;
                            break;
                        }
                    }
                    if !is_active_now {
                        self.bring_to_front(win_id);
                    }
                    self.dirty = true;
                    return TaskbarAction::None;
                }
                TaskbarAction::RestoreWindow(win_id) => {
                    self.bring_to_front(win_id);
                    self.dirty = true;
                    return TaskbarAction::None;
                }
                TaskbarAction::LaunchApp(app_id) => {
                    self.open_app(app_id);
                    self.dirty = true;
                    return TaskbarAction::None;
                }
                TaskbarAction::ToggleStartMenu => {
                    self.dirty = true;
                    return TaskbarAction::None;
                }
                TaskbarAction::ToggleSpotlight => {
                    self.spotlight.toggle();
                    self.dirty = true;
                    return TaskbarAction::None;
                }
                TaskbarAction::ExitToShell | TaskbarAction::RebootSystem | TaskbarAction::HaltSystem => return action,
                TaskbarAction::None => {}
            }

            // If click was in taskbar area, don't check windows
            if y >= sh - 36 {
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
                self.dirty = true;
                return TaskbarAction::None;
            }

            if let Some(id) = max_id {
                if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
                    w.toggle_maximize(sw, sh);
                }
                self.dirty = true;
                return TaskbarAction::None;
            }

            if let Some(id) = min_id {
                if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
                    w.is_minimized = true;
                    w.is_active = false;
                }
                self.dirty = true;
                return TaskbarAction::None;
            }

            if let Some((id, edge)) = start_resize_id {
                self.bring_to_front(id);
                if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
                    w.resizing = true;
                    w.resize_edge = edge;
                }
                self.dirty = true;
                return TaskbarAction::None;
            }

            if let Some((id, off_x, off_y)) = start_drag_id {
                self.bring_to_front(id);
                if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
                    w.dragging = true;
                    w.drag_offset_x = off_x;
                    w.drag_offset_y = off_y;
                }
                self.dirty = true;
                return TaskbarAction::None;
            }

            if let Some(id) = inner_click_id {
                self.bring_to_front(id);
                if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
                    w.handle_inner_click(x, y);
                }
                self.dirty = true;
                return TaskbarAction::None;
            }

            // 5. Clicked on desktop background
            if clicked_win_id.is_none() {
                // Deactivate windows
                for w in self.windows.iter_mut() {
                    w.is_active = false;
                }

                if let Some(app_id) = self.desktop.check_click(x, y) {
                    self.open_app(app_id);
                }
                self.dirty = true;
            }
        }

        result_action
    }

    pub fn handle_key_event(&mut self, ch: char) {
        // Ctrl+Space ('\0') or backtick ('`') toggles Spotlight
        if ch == '\0' || ch == '`' {
            self.spotlight.toggle();
            self.dirty = true;
            return;
        }

        if self.spotlight.is_open {
            if let Some(target) = self.spotlight.handle_char(ch) {
                match target {
                    SpotlightTarget::LaunchApp(app_id) => self.open_app(app_id),
                    SpotlightTarget::Calculation(_) => {}
                }
            }
            self.dirty = true;
            return;
        }

        if let Some(active_win) = self.windows.iter_mut().find(|w| w.is_active && !w.is_minimized) {
            active_win.handle_char(ch);
            self.dirty = true;
        }
    }

    pub fn render_frame(&mut self) {
        let ticks = crate::arch::x86_64::pit::get_ticks();
        let now_sec = (ticks / 1000) as u8;
        let blink = ((ticks / 500) % 2) as u64;
        let needs_render = self.dirty || now_sec != self.last_sec || blink != self.last_blink;
        if !needs_render {
            return;
        }
        self.dirty = false;
        self.last_sec = now_sec;
        self.last_blink = blink;

        let sw = self.canvas.width as i32;
        let sh = self.canvas.height as i32;

        // 1. Wallpaper & Icons
        self.desktop.render(&mut self.canvas, self.mouse_x, self.mouse_y);

        // 2. Windows (back to front)
        for w in self.windows.iter_mut() {
            w.render(&mut self.canvas, self.mouse_x, self.mouse_y);
        }

        // 3. Taskbar, Menus & Dock
        self.taskbar.render(&mut self.canvas, &self.windows, self.mouse_x, self.mouse_y);

        // 3.5. Spotlight Search Modal Overlay
        self.spotlight.render(&mut self.canvas, sw, sh);

        // 4. Determine Dynamic Cursor Type
        let mut cursor_type = CursorType::Arrow;
        if self.spotlight.is_open {
            let modal_w = 540;
            let modal_x = (sw - modal_w) / 2;
            let modal_y = 100;
            if self.mouse_x >= modal_x + 30 && self.mouse_x <= modal_x + modal_w - 30 && self.mouse_y >= modal_y + 10 && self.mouse_y <= modal_y + 40 {
                cursor_type = CursorType::IBeam;
            }
        } else {
            for w in self.windows.iter().rev() {
                if !w.is_minimized && w.contains(self.mouse_x, self.mouse_y) {
                    let edge = w.contains_resize_handle(self.mouse_x, self.mouse_y);
                    match edge {
                        ResizeEdge::BottomRight => cursor_type = CursorType::ResizeDiagonal,
                        ResizeEdge::Right => cursor_type = CursorType::ResizeHorizontal,
                        ResizeEdge::Bottom => cursor_type = CursorType::ResizeVertical,
                        ResizeEdge::None => {
                            match &w.app {
                                WindowApp::Terminal(_) | WindowApp::Editor(_) => {
                                    if self.mouse_y > w.y + 28 && self.mouse_y < w.y + w.height - 4 {
                                        cursor_type = CursorType::IBeam;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    break;
                }
            }

            // Dock hover -> Hand
            let dock_h = 58;
            let dock_y = sh - dock_h - 10;
            if self.mouse_y >= dock_y && self.mouse_y <= dock_y + dock_h {
                cursor_type = CursorType::Hand;
            }

            // Top menu bar icons hover -> Hand
            if self.mouse_y <= 24 {
                cursor_type = CursorType::Hand;
            }
        }

        // 5. Mouse Cursor Pointer
        draw_cursor(&mut self.canvas, self.mouse_x, self.mouse_y, cursor_type);

        // 6. Blit to hardware linear framebuffer (respecting GOP pitch stride & channel shifts)
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
        self.canvas
            .blit_to_framebuffer(fb_ptr, pitch, bpp, rpos, gpos, bpos);
    }
}

/// Launch the Sublime Desktop GUI environment.
pub fn launch_desktop() {
    kinfo!("Initializing UEFI GOP / Multiboot Linear Framebuffer for Aqua Desktop...");

    if let Err(e) = vbe::init(1024, 768) {
        crate::kerror!("GUI launch aborted: {}", e);
        return;
    }

    if vbe::is_text_mode_fallback() {
        crate::kerror!("Cannot launch Aqua Desktop: Hardware is running in VGA Text Mode (no UEFI GOP / Multiboot LFB active).");
        return;
    }

    // Ensure CPU interrupts are active (syscalls via FMASK / int 0x80 clear IF)
    crate::arch::x86_64::registers::enable_interrupts();

    // Copy needed display metadata out of VBE lock guard immediately
    let (w, h, pitch, bpp) = {
        let vbe = crate::drivers::vbe::VBE.lock();
        (vbe.width, vbe.height, vbe.pitch, vbe.bpp)
    };

    crate::drivers::mouse::set_bounds(w as i32, h as i32);
    let mut desktop_mgr = DesktopManager::new(w, h);

    // Open default initial windows: Terminal & System Monitor
    desktop_mgr.open_app(DesktopAppId::Terminal);
    desktop_mgr.open_app(DesktopAppId::Sysmon);

    kinfo!("Sublime Aqua Desktop online: {}x{}@{}bpp (Pitch: {} B)", w, h, bpp, pitch);

    let mut running = true;
    crate::drivers::gop_console::set_gui_active(true);

    while running {
        // Poll input hardware directly per frame
        crate::drivers::keyboard::poll_ps2_controller();
        crate::drivers::usb::poll_hid_nonblocking();

        // 1. Poll mouse input
        let (mx, my, left_btn) = {
            let m = MOUSE.lock();
            (m.x, m.y, m.left_button)
        };

        let action = desktop_mgr.handle_mouse_event(mx, my, left_btn);
        match action {
            TaskbarAction::ExitToShell => {
                running = false;
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
        }

        // 2. Poll keyboard input (PS/2 keyboard)
        while let Some(byte) = read_char() {
            if byte == b'\x1B' {
                // Escape key / F12 exits GUI to text shell
                running = false;
                break;
            }
            desktop_mgr.handle_key_event(byte as char);
        }

        // 3. Poll serial input (COM1) so typing in terminal window also feeds GUI terminal!
        while let Some(byte) = read_rx_char() {
            if byte == b'\x1B' {
                running = false;
                break;
            }
            desktop_mgr.handle_key_event(byte as char);
        }

        let frame_start = crate::arch::x86_64::pit::get_ticks();

        // 4. Render frame
        desktop_mgr.render_frame();

        // 5. Throttled frame pacing (~60 FPS target): wait up to 16ms, waking immediately on input/timer interrupts
        while crate::arch::x86_64::pit::get_ticks().saturating_sub(frame_start) < 16 {
            if crate::drivers::keyboard::poll_ps2_controller() {
                break;
            }
            crate::drivers::usb::poll_hid_nonblocking();
            let (mx, my, lbtn) = {
                let m = MOUSE.lock();
                (m.x, m.y, m.left_button)
            };
            if mx != desktop_mgr.mouse_x || my != desktop_mgr.mouse_y || lbtn != desktop_mgr.prev_left_down {
                break;
            }
            crate::arch::x86_64::registers::enable_interrupts_and_hlt();
        }
    }

    crate::drivers::gop_console::set_gui_active(false);
    kinfo!("Exited Sublime Desktop GUI. Returning to SublimeSH text shell...");
}
