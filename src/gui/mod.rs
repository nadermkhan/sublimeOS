pub mod apps;
pub mod canvas;
pub mod color;
pub mod desktop;
pub mod font;
pub mod mouse;
pub mod taskbar;
pub mod window;

use alloc::vec::Vec;
use crate::arch::x86_64::pit::sleep_ms;
use crate::arch::x86_64::serial::read_rx_char;
use crate::serial_println;
use crate::drivers::keyboard::read_char;
use crate::drivers::mouse::MOUSE;
use crate::drivers::vbe::{self, VBE};
use crate::gui::apps::{AboutApp, FileManagerApp, SysmonApp, TerminalApp};
use crate::gui::canvas::Canvas;
use crate::gui::desktop::{Desktop, DesktopAppId};
use crate::gui::mouse::draw_cursor;
use crate::gui::taskbar::{Taskbar, TaskbarAction, TaskbarTab};
use crate::gui::window::{Window, WindowApp};
use crate::kinfo;

pub struct DesktopManager {
    pub canvas: Canvas,
    pub desktop: Desktop,
    pub taskbar: Taskbar,
    pub windows: Vec<Window>,
    next_window_id: u32,
    prev_left_down: bool,
    pub mouse_x: i32,
    pub mouse_y: i32,
}

impl DesktopManager {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            canvas: Canvas::new(width, height),
            desktop: Desktop::new(),
            taskbar: Taskbar::new(),
            windows: Vec::new(),
            next_window_id: 1,
            prev_left_down: false,
            mouse_x: (width / 2) as i32,
            mouse_y: (height / 2) as i32,
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
        };

        // Deactivate other windows
        for w in self.windows.iter_mut() {
            w.is_active = false;
        }

        self.windows.push(win);
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
        }
    }

    pub fn close_window(&mut self, window_id: u32) {
        if let Some(idx) = self.windows.iter().position(|w| w.id == window_id) {
            self.windows.remove(idx);
            if let Some(last) = self.windows.last_mut() {
                last.is_active = true;
            }
        }
    }

    pub fn handle_mouse_event(&mut self, x: i32, y: i32, left_down: bool) -> TaskbarAction {
        self.mouse_x = x;
        self.mouse_y = y;
        let result_action = TaskbarAction::None;

        let left_clicked = left_down && !self.prev_left_down;
        let left_released = !left_down && self.prev_left_down;
        self.prev_left_down = left_down;

        // 1. If currently dragging a window, update its position
        if let Some(drag_win) = self.windows.iter_mut().find(|w| w.dragging) {
            if left_down {
                drag_win.x = x - drag_win.drag_offset_x;
                drag_win.y = (y - drag_win.drag_offset_y).clamp(0, 768 - 36 - 28);
            }
        }

        if left_released {
            for w in self.windows.iter_mut() {
                w.dragging = false;
            }
        }

        if left_clicked {
            let sw = self.canvas.width as i32;
            let sh = self.canvas.height as i32;

            // Collect tabs info for taskbar click testing
            let tabs: Vec<TaskbarTab> = self
                .windows
                .iter()
                .map(|w| TaskbarTab {
                    window_id: w.id,
                    title: w.title.clone(),
                    is_active: w.is_active,
                    is_minimized: w.is_minimized,
                })
                .collect();

            // 2. Check taskbar / start menu click
            let action = self.taskbar.handle_click(x, y, &tabs, sw, sh);
            match action {
                TaskbarAction::FocusWindow(win_id) => {
                    // If already active and not minimized, toggle minimize
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
                    return TaskbarAction::None;
                }
                TaskbarAction::LaunchApp(app_id) => {
                    self.open_app(app_id);
                    return TaskbarAction::None;
                }
                TaskbarAction::ToggleStartMenu => return TaskbarAction::None,
                TaskbarAction::ExitToShell | TaskbarAction::HaltSystem => return action,
                TaskbarAction::None => {}
            }

            // If click was in taskbar area, don't check windows
            if y >= sh - 36 {
                return TaskbarAction::None;
            }

            // 3. Check Windows from front to back
            let mut clicked_win_id = None;
            let mut close_id = None;
            let mut max_id = None;
            let mut min_id = None;
            let mut start_drag_id = None;
            let mut inner_click_id = None;

            for w in self.windows.iter().rev() {
                if !w.is_minimized && w.contains(x, y) {
                    clicked_win_id = Some(w.id);

                    if w.contains_close_btn(x, y) {
                        close_id = Some(w.id);
                    } else if w.contains_maximize_btn(x, y) {
                        max_id = Some(w.id);
                    } else if w.contains_minimize_btn(x, y) {
                        min_id = Some(w.id);
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
                if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
                    w.toggle_maximize(sw, sh);
                }
                return TaskbarAction::None;
            }

            if let Some(id) = min_id {
                if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
                    w.is_minimized = true;
                    w.is_active = false;
                }
                return TaskbarAction::None;
            }

            if let Some((id, off_x, off_y)) = start_drag_id {
                self.bring_to_front(id);
                if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
                    w.dragging = true;
                    w.drag_offset_x = off_x;
                    w.drag_offset_y = off_y;
                }
                return TaskbarAction::None;
            }

            if let Some(id) = inner_click_id {
                self.bring_to_front(id);
                if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
                    w.handle_inner_click(x, y);
                }
                return TaskbarAction::None;
            }

            // 4. Clicked on desktop background
            if clicked_win_id.is_none() {
                // Deactivate windows
                for w in self.windows.iter_mut() {
                    w.is_active = false;
                }

                if let Some(app_id) = self.desktop.check_click(x, y) {
                    self.open_app(app_id);
                }
            }
        }

        result_action
    }

    pub fn handle_key_event(&mut self, ch: char) {
        if let Some(active_win) = self.windows.iter_mut().find(|w| w.is_active && !w.is_minimized) {
            active_win.handle_char(ch);
        }
    }

    pub fn render_frame(&mut self) {
        // 1. Wallpaper & Icons
        self.desktop.render(&mut self.canvas, self.mouse_x, self.mouse_y);

        // 2. Windows (back to front)
        for w in self.windows.iter_mut() {
            w.render(&mut self.canvas);
        }

        // 3. Taskbar & Start Menu
        let tabs: Vec<TaskbarTab> = self
            .windows
            .iter()
            .map(|w| TaskbarTab {
                window_id: w.id,
                title: w.title.clone(),
                is_active: w.is_active,
                is_minimized: w.is_minimized,
            })
            .collect();
        self.taskbar.render(&mut self.canvas, &tabs, self.mouse_x, self.mouse_y);

        // 4. Mouse Cursor Pointer
        draw_cursor(&mut self.canvas, self.mouse_x, self.mouse_y);

        // 5. Blit to hardware linear framebuffer (respecting GOP pitch stride & channel shifts)
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
        kinfo!("GUI launch aborted: {}", e);
        return;
    }

    let (w, h) = vbe::dimensions();
    crate::drivers::mouse::set_bounds(w as i32, h as i32);
    let mut desktop_mgr = DesktopManager::new(w, h);

    // Open default initial windows: Terminal & System Monitor
    desktop_mgr.open_app(DesktopAppId::Terminal);
    desktop_mgr.open_app(DesktopAppId::Sysmon);

    serial_println!("[INFO] Sublime Graphical Desktop is now running!");
    serial_println!("[INFO] Type in serial terminal or graphical window to interact with SublimeSH.");
    serial_println!("[INFO] Press F12 or click 'Exit to Shell' in Start Menu to return to text console.");

    let mut running = true;
    crate::drivers::gop_console::set_gui_active(true);

    while running {
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

        // 4. Render frame
        desktop_mgr.render_frame();

        // 5. Short sleep to throttle frame rate (~30 FPS)
        sleep_ms(30);
    }

    crate::drivers::gop_console::set_gui_active(false);
    kinfo!("Exited Sublime Desktop GUI. Returning to SublimeSH text shell...");
}
