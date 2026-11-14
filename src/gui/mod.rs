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
use crate::gui::apps::settings::SettingsTab;
use crate::gui::apps::sysmon::SysmonAction;
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
    cursor_dirty: bool,
    last_sec: u8,
    last_blink: u64,
    cursor_bg: [u32; 32 * 32],
    saved_cursor_box: Option<(i32, i32, i32, i32)>,
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
            cursor_dirty: true,
            last_sec: 255,
            last_blink: 0,
            cursor_bg: [0; 32 * 32],
            saved_cursor_box: None,
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
                260,
                330,
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
        };

        // Deactivate other windows
        for w in self.windows.iter_mut() {
            w.is_active = false;
        }

        self.windows.push(win);
        self.dirty = true;
    }

    pub fn open_settings_tab(&mut self, tab: SettingsTab) {
        self.open_app(DesktopAppId::Settings);
        for w in self.windows.iter_mut() {
            if let WindowApp::Settings(ref mut s) = w.app {
                s.set_tab(tab);
                break;
            }
        }
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
            self.cursor_dirty = true;
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
                        res_win.height = (y - res_win.y).clamp(res_win.min_height, sh - res_win.y - 30);
                    }
                    crate::gui::window::ResizeEdge::Right => {
                        res_win.width = (x - res_win.x).clamp(res_win.min_width, sw - res_win.x);
                    }
                    crate::gui::window::ResizeEdge::Bottom => {
                        res_win.height = (y - res_win.y).clamp(res_win.min_height, sh - res_win.y - 30);
                    }
                    _ => {}
                }
                self.dirty = true;
            }
        }

        // 2. If currently dragging a window, update its position within Windows XP workspace
        if let Some(drag_win) = self.windows.iter_mut().find(|w| w.dragging) {
            if left_down {
                drag_win.x = (x - drag_win.drag_offset_x).clamp(-drag_win.width + 60, sw - 60);
                drag_win.y = (y - drag_win.drag_offset_y).clamp(0, sh - 30 - 28);
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
            // Check Windows XP Run dialog interaction if open
            if self.spotlight.is_open {
                let results = self.spotlight.get_results();
                let modal_w = 460;
                let item_h = 26;
                let list_h = (results.len().min(5) as i32) * item_h;
                let modal_h = 132 + list_h;
                let modal_x = (sw - modal_w) / 2;
                let modal_y = ((sh - 30 - modal_h) / 2).max(40);

                if x >= modal_x && x <= modal_x + modal_w && y >= modal_y && y <= modal_y + modal_h {
                    let close_x = modal_x + modal_w - 24;
                    let close_y = modal_y + 4;
                    let btn_y = modal_y + modal_h - 34;
                    let ok_x = modal_x + modal_w - 172;
                    let cancel_x = modal_x + modal_w - 90;

                    if (x >= close_x && x <= close_x + 20 && y >= close_y && y <= close_y + 19)
                        || (x >= cancel_x && x <= cancel_x + 74 && y >= btn_y && y <= btn_y + 24)
                    {
                        self.spotlight.is_open = false;
                    } else if x >= ok_x && x <= ok_x + 74 && y >= btn_y && y <= btn_y + 24 {
                        if let Some(item) = results.get(self.spotlight.selected_index) {
                            match &item.target {
                                SpotlightTarget::LaunchApp(app_id) => self.open_app(*app_id),
                                SpotlightTarget::Calculation(_) => {}
                            }
                            self.spotlight.is_open = false;
                        }
                    } else {
                        let list_y = modal_y + 27 + 62;
                        let box_x = modal_x + 3 + 62;
                        let box_w = modal_w - 6 - 76;
                        if x >= box_x && x <= box_x + box_w && y >= list_y && y <= list_y + list_h {
                            let idx = ((y - list_y) / item_h) as usize;
                            if let Some(item) = results.get(idx) {
                                match &item.target {
                                    SpotlightTarget::LaunchApp(app_id) => self.open_app(*app_id),
                                    SpotlightTarget::Calculation(_) => {}
                                }
                                self.spotlight.is_open = false;
                            }
                        }
                    }
                    self.dirty = true;
                    return TaskbarAction::None;
                } else {
                    self.spotlight.is_open = false;
                    self.dirty = true;
                }
            }

            // 3. Check Windows XP Taskbar & Start Menu click
            let action = self.taskbar.handle_click(x, y, &self.windows, sw, sh);
            match action {
                TaskbarAction::FocusWindow(win_id) => {
                    let mut minimized_now = false;
                    for w in self.windows.iter_mut() {
                        if w.id == win_id && w.is_active && !w.is_minimized {
                            w.is_minimized = true;
                            w.is_active = false;
                            minimized_now = true;
                            break;
                        }
                    }
                    if !minimized_now {
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
                TaskbarAction::MinimizeAll => {
                    for w in self.windows.iter_mut() {
                        w.is_minimized = true;
                        w.is_active = false;
                    }
                    self.dirty = true;
                    return TaskbarAction::None;
                }
                TaskbarAction::LaunchApp(app_id) => {
                    self.open_app(app_id);
                    self.dirty = true;
                    return TaskbarAction::None;
                }
                TaskbarAction::OpenNetworkSettings => {
                    self.open_settings_tab(SettingsTab::Network);
                    self.dirty = true;
                    return TaskbarAction::None;
                }
                TaskbarAction::OpenDisplaySettings => {
                    self.open_settings_tab(SettingsTab::Display);
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

            // If click was in bottom Windows XP taskbar area, don't check windows
            if y >= sh - 30 {
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
                let mut sys_act = SysmonAction::None;
                if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
                    w.handle_inner_click(x, y);
                    if let WindowApp::Sysmon(ref mut sysmon) = w.app {
                        sys_act = sysmon.take_action();
                    }
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
                    SysmonAction::None => {}
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

    fn determine_cursor_type(&self) -> CursorType {
        let sw = self.canvas.width as i32;
        let sh = self.canvas.height as i32;
        let mut cursor_type = CursorType::Arrow;

        if self.spotlight.is_open {
            let results = self.spotlight.get_results();
            let modal_w = 460;
            let list_h = (results.len().min(5) as i32) * 26;
            let modal_h = 132 + list_h;
            let modal_x = (sw - modal_w) / 2;
            let modal_y = ((sh - 30 - modal_h) / 2).max(40);
            let box_x = modal_x + 65;
            let box_y = modal_y + 59;
            if self.mouse_x >= box_x
                && self.mouse_x <= modal_x + modal_w - 14
                && self.mouse_y >= box_y
                && self.mouse_y <= box_y + 24
            {
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
                            if matches!(&w.app, WindowApp::Terminal(_) | WindowApp::Editor(_))
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
            if self.mouse_y >= sh - 30 && self.mouse_x < 180 {
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
        let ticks = crate::arch::x86_64::pit::get_ticks();
        let now_sec = (ticks / 1000) as u8;
        let blink = ((ticks / 500) % 2) as u64;
        let full_scene = self.dirty || now_sec != self.last_sec || blink != self.last_blink;

        if !full_scene && !self.cursor_dirty {
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

        if !full_scene && self.cursor_dirty && self.saved_cursor_box.is_some() {
            // Ultra-fast hardware-like cursor overlay: restore old 32x32 background & draw new 32x32 cursor
            self.cursor_dirty = false;
            let old_box = self.restore_cursor_bg();
            let (nbx, nby, nbw, nbh) = self.cursor_patch_box(self.mouse_x, self.mouse_y);
            self.capture_cursor_bg(nbx, nby, nbw, nbh);
            let cursor_type = self.determine_cursor_type();
            draw_cursor(&mut self.canvas, self.mouse_x, self.mouse_y, cursor_type);

            if let Some((obx, oby, obw, obh)) = old_box {
                self.canvas
                    .blit_rect_to_framebuffer(fb_ptr, pitch, bpp, rpos, gpos, bpos, obx, oby, obw, obh);
            }
            self.canvas
                .blit_rect_to_framebuffer(fb_ptr, pitch, bpp, rpos, gpos, bpos, nbx, nby, nbw, nbh);
            return;
        }

        self.dirty = false;
        self.cursor_dirty = false;
        self.last_sec = now_sec;
        self.last_blink = blink;
        self.saved_cursor_box = None;

        let sw = self.canvas.width as i32;
        let sh = self.canvas.height as i32;

        // 1. Wallpaper (cached) & Icons
        self.desktop.render(&mut self.canvas, self.mouse_x, self.mouse_y);

        // 2. Sync live window list into Task Manager and render windows (back to front)
        let win_snapshot: Vec<(u32, alloc::string::String, bool)> = self
            .windows
            .iter()
            .map(|w| (w.id, w.title.clone(), w.is_minimized))
            .collect();
        for w in self.windows.iter_mut() {
            if let WindowApp::Sysmon(ref mut sysmon) = w.app {
                sysmon.sync_windows(win_snapshot.clone());
            }
            w.render(&mut self.canvas, self.mouse_x, self.mouse_y);
        }

        // 3. Taskbar, Menus & Dock
        self.taskbar.render(&mut self.canvas, &self.windows, self.mouse_x, self.mouse_y);

        // 3.5. Spotlight Search Modal Overlay
        self.spotlight.render(&mut self.canvas, sw, sh);

        // 4. Capture clean under-cursor patch before drawing Cursor Pointer
        let (nbx, nby, nbw, nbh) = self.cursor_patch_box(self.mouse_x, self.mouse_y);
        self.capture_cursor_bg(nbx, nby, nbw, nbh);
        let cursor_type = self.determine_cursor_type();
        draw_cursor(&mut self.canvas, self.mouse_x, self.mouse_y, cursor_type);

        // 5. Blit full composited frame to hardware linear framebuffer
        self.canvas
            .blit_to_framebuffer(fb_ptr, pitch, bpp, rpos, gpos, bpos);
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

    crate::drivers::mouse::set_bounds(w as i32, h as i32);
    let mut desktop_mgr = alloc::boxed::Box::new(DesktopManager::new(w, h));

    // Open default initial windows: Task Manager behind Terminal so Terminal starts focused
    desktop_mgr.open_app(DesktopAppId::Sysmon);
    desktop_mgr.open_app(DesktopAppId::Terminal);

    kinfo!("SublimeOS Desktop online: {}x{}@{}bpp (Pitch: {} B)", w, h, bpp, pitch);

    // Render initial frame to paint the SublimeOS desktop onto the display before suppressing text console
    desktop_mgr.render_frame();

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

        // 2. Poll keyboard input (PS/2 + USB HID keyboard ring buffer, bounded per frame)
        let mut csi_state = 0u8;
        for _ in 0..64 {
            let Some(byte) = read_char() else {
                break;
            };
            if csi_state == 1 {
                if byte == b'[' || byte == b'O' {
                    csi_state = 2;
                    continue;
                }
                csi_state = 0;
                // Standalone Escape followed by another key: handle ESC action first
                if desktop_mgr.spotlight.is_open {
                    desktop_mgr.spotlight.is_open = false;
                    desktop_mgr.dirty = true;
                } else if desktop_mgr.taskbar.start_menu_open {
                    desktop_mgr.taskbar.start_menu_open = false;
                    desktop_mgr.dirty = true;
                } else {
                    running = false;
                    break;
                }
            } else if csi_state == 2 {
                // Consume CSI parameter/final bytes (0x40..=0x7E finishes sequence)
                if (0x40..=0x7E).contains(&byte) {
                    csi_state = 0;
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
            desktop_mgr.handle_key_event(byte as char);
        }
        if csi_state == 1 && running {
            // Standalone Escape (arrow/CSI keys always queue "\x1B[..." together)
            if desktop_mgr.spotlight.is_open {
                desktop_mgr.spotlight.is_open = false;
                desktop_mgr.dirty = true;
            } else if desktop_mgr.taskbar.start_menu_open {
                desktop_mgr.taskbar.start_menu_open = false;
                desktop_mgr.dirty = true;
            } else {
                running = false;
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
                desktop_mgr.handle_key_event(byte as char);
            }
        }

        let frame_start = crate::arch::x86_64::pit::get_ticks();
        let tsc_deadline = crate::arch::x86_64::pit::read_tsc().saturating_add(40_000_000);

        // 4. Render frame
        desktop_mgr.render_frame();

        // 5. Throttled frame pacing (~60 FPS target): wait up to 16ms with TSC fallback guard
        while crate::arch::x86_64::pit::get_ticks().saturating_sub(frame_start) < 16 {
            if crate::arch::x86_64::pit::read_tsc() >= tsc_deadline {
                break;
            }
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
            core::hint::spin_loop();
        }
    }

    crate::drivers::gop_console::set_gui_active(false);
    kinfo!("Exited Sublime Desktop GUI. Returning to SublimeSH text shell...");
}
