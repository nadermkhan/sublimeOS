use alloc::string::String;
use crate::gui::apps::{AboutApp, FileManagerApp, SysmonApp, TerminalApp};
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::desktop::DesktopAppId;
use crate::gui::font::{draw_string, measure_string};

pub const TITLEBAR_HEIGHT: i32 = 28;
pub const BTN_RADIUS: i32 = 5;

pub enum WindowApp {
    Terminal(TerminalApp),
    Sysmon(SysmonApp),
    Files(FileManagerApp),
    About(AboutApp),
}

impl WindowApp {
    pub fn app_id(&self) -> DesktopAppId {
        match self {
            WindowApp::Terminal(_) => DesktopAppId::Terminal,
            WindowApp::Sysmon(_) => DesktopAppId::Sysmon,
            WindowApp::Files(_) => DesktopAppId::FileManager,
            WindowApp::About(_) => DesktopAppId::About,
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
    pub app: WindowApp,
}

impl Window {
    pub fn new(id: u32, title: &str, x: i32, y: i32, width: i32, height: i32, app: WindowApp) -> Self {
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
            app,
        }
    }

    pub fn inner_bounds(&self) -> (i32, i32, i32, i32) {
        (
            self.x + 2,
            self.y + TITLEBAR_HEIGHT,
            self.width - 4,
            self.height - TITLEBAR_HEIGHT - 2,
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
        let btn_x = self.x + 16;
        let btn_y = self.y + 14;
        (px - btn_x) * (px - btn_x) + (py - btn_y) * (py - btn_y) <= (BTN_RADIUS + 3) * (BTN_RADIUS + 3)
    }

    pub fn contains_minimize_btn(&self, px: i32, py: i32) -> bool {
        let btn_x = self.x + 32;
        let btn_y = self.y + 14;
        (px - btn_x) * (px - btn_x) + (py - btn_y) * (py - btn_y) <= (BTN_RADIUS + 3) * (BTN_RADIUS + 3)
    }

    pub fn contains_maximize_btn(&self, px: i32, py: i32) -> bool {
        let btn_x = self.x + 48;
        let btn_y = self.y + 14;
        (px - btn_x) * (px - btn_x) + (py - btn_y) * (py - btn_y) <= (BTN_RADIUS + 3) * (BTN_RADIUS + 3)
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
            self.height = screen_h - 36; // Leave taskbar visible
            self.is_maximized = true;
        }
    }

    pub fn handle_char(&mut self, ch: char) {
        if let WindowApp::Terminal(ref mut term) = self.app {
            term.handle_char(ch);
        }
    }

    pub fn handle_inner_click(&mut self, px: i32, py: i32) {
        let (ix, iy, iw, ih) = self.inner_bounds();
        if let WindowApp::Files(ref mut files) = self.app {
            files.handle_click(px, py, ix, iy, iw, ih);
        }
    }

    pub fn render(&mut self, canvas: &mut Canvas) {
        if self.is_minimized {
            return;
        }

        // 1. Drop shadow (more prominent for active window)
        let shadow_spread = if self.is_active { 8 } else { 4 };
        let shadow_alpha = if self.is_active { 90 } else { 50 };
        canvas.draw_shadow(self.x, self.y, self.width, self.height, shadow_spread, shadow_alpha);

        // 2. Window Frame / Border
        let border_col = if self.is_active {
            Color::WINDOW_BORDER_ACTIVE
        } else {
            Color::WINDOW_BORDER
        };
        canvas.fill_rounded_rect(self.x, self.y, self.width, self.height, 6, Color::BG_CARD);
        canvas.draw_rounded_rect(self.x, self.y, self.width, self.height, 6, border_col);

        // 3. Titlebar
        if self.is_active {
            canvas.draw_gradient_h(
                self.x + 1,
                self.y + 1,
                self.width - 2,
                TITLEBAR_HEIGHT,
                Color::TITLEBAR_ACTIVE_START,
                Color::TITLEBAR_ACTIVE_END,
            );
        } else {
            canvas.fill_rect(
                self.x + 1,
                self.y + 1,
                self.width - 2,
                TITLEBAR_HEIGHT,
                Color::TITLEBAR_INACTIVE,
            );
        }

        // Titlebar bottom separator line
        canvas.fill_rect(
            self.x + 1,
            self.y + TITLEBAR_HEIGHT,
            self.width - 2,
            1,
            Color::WINDOW_BORDER,
        );

        // macOS Window Control Buttons (Traffic Lights on top-LEFT)
        let cy = self.y + 14;
        // Close (Red)
        canvas.fill_circle(self.x + 16, cy, BTN_RADIUS, Color::BTN_CLOSE);
        // Minimize (Yellow)
        canvas.fill_circle(self.x + 32, cy, BTN_RADIUS, Color::BTN_MINIMIZE);
        // Zoom/Maximize (Green)
        canvas.fill_circle(self.x + 48, cy, BTN_RADIUS, Color::BTN_MAXIMIZE);

        // Window title text (CENTERED like macOS)
        let title_w = measure_string(&self.title);
        let title_x = self.x + (self.width - title_w) / 2;
        draw_string(
            canvas,
            title_x,
            self.y + 6,
            &self.title,
            if self.is_active { Color::WHITE } else { Color::TEXT_MUTED },
        );

        // 4. Render App Content inside inner bounds
        let (ix, iy, iw, ih) = self.inner_bounds();
        match self.app {
            WindowApp::Terminal(ref mut term) => term.render(canvas, ix, iy, iw, ih),
            WindowApp::Sysmon(ref sysmon) => sysmon.render(canvas, ix, iy, iw, ih),
            WindowApp::Files(ref files) => files.render(canvas, ix, iy, iw, ih),
            WindowApp::About(ref about) => about.render(canvas, ix, iy, iw, ih),
        }
    }
}
