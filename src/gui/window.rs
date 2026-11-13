use alloc::string::String;
use crate::gui::apps::{AboutApp, CalcApp, FileManagerApp, SettingsApp, SysmonApp, TerminalApp, TextEditApp};
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::desktop::DesktopAppId;
use crate::gui::font::{draw_char, draw_string, measure_string};

pub const TITLEBAR_HEIGHT: i32 = 28;
pub const BTN_RADIUS: i32 = 6;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ResizeEdge {
    None,
    Right,
    Bottom,
    BottomRight,
}

pub enum WindowApp {
    Terminal(TerminalApp),
    Sysmon(SysmonApp),
    Files(FileManagerApp),
    About(AboutApp),
    Calc(CalcApp),
    Editor(TextEditApp),
    Settings(SettingsApp),
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
            WindowApp::Calc(_) => (240, 340),
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

        // Bottom-Right corner (14x14 area)
        if px >= rx - 14 && px <= rx + 2 && py >= by - 14 && py <= by + 2 {
            return ResizeEdge::BottomRight;
        }
        // Right border (6px area)
        if px >= rx - 6 && px <= rx + 2 && py >= self.y + TITLEBAR_HEIGHT && py < by - 14 {
            return ResizeEdge::Right;
        }
        // Bottom border (6px area)
        if py >= by - 6 && py <= by + 2 && px >= self.x && px < rx - 14 {
            return ResizeEdge::Bottom;
        }
        ResizeEdge::None
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
        let btn_x = self.x + 34;
        let btn_y = self.y + 14;
        (px - btn_x) * (px - btn_x) + (py - btn_y) * (py - btn_y) <= (BTN_RADIUS + 3) * (BTN_RADIUS + 3)
    }

    pub fn contains_maximize_btn(&self, px: i32, py: i32) -> bool {
        let btn_x = self.x + 52;
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
            self.x = 8;
            self.y = 28; // Below macOS top menu bar
            self.width = screen_w - 16;
            self.height = screen_h - 28 - 72; // Leave Dock visible at bottom
            self.is_maximized = true;
        }
    }

    pub fn handle_char(&mut self, ch: char) {
        match self.app {
            WindowApp::Terminal(ref mut term) => term.handle_char(ch),
            WindowApp::Calc(ref mut calc) => calc.handle_char(ch),
            WindowApp::Editor(ref mut ed) => ed.handle_char(ch),
            _ => {}
        }
    }

    pub fn handle_inner_click(&mut self, px: i32, py: i32) {
        let (ix, iy, iw, ih) = self.inner_bounds();
        match self.app {
            WindowApp::Files(ref mut files) => files.handle_click(px, py, ix, iy, iw, ih),
            WindowApp::Calc(ref mut calc) => calc.handle_click(px, py, ix, iy, iw, ih),
            WindowApp::Editor(ref mut ed) => ed.handle_click(px, py, ix, iy, iw, ih),
            WindowApp::Settings(ref mut s) => s.handle_click(px, py, ix, iy, iw, ih),
            _ => {}
        }
    }

    pub fn render(&mut self, canvas: &mut Canvas, mouse_x: i32, mouse_y: i32) {
        if self.is_minimized {
            return;
        }

        // 1. Drop shadow (more prominent for active window)
        let shadow_spread = if self.is_active { 10 } else { 5 };
        let shadow_alpha = if self.is_active { 95 } else { 50 };
        canvas.draw_shadow(self.x, self.y, self.width, self.height, shadow_spread, shadow_alpha);

        // 2. Window Frame / Border
        let border_col = if self.is_active {
            Color::WINDOW_BORDER_ACTIVE
        } else {
            Color::WINDOW_BORDER
        };
        canvas.fill_rounded_rect(self.x, self.y, self.width, self.height, 8, Color::BG_CARD);
        canvas.draw_rounded_rect(self.x, self.y, self.width, self.height, 8, border_col);

        // 3. Titlebar (macOS dark glass appearance)
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

        // Window Control Buttons (macOS Traffic Lights on top-LEFT)
        let cy = self.y + 14;
        let traffic_hover = mouse_x >= self.x + 8
            && mouse_x <= self.x + 62
            && mouse_y >= self.y + 6
            && mouse_y <= self.y + 22;

        let (red_col, yellow_col, green_col) = if self.is_active || traffic_hover {
            (
                Color::argb(255, 255, 95, 86),
                Color::argb(255, 255, 189, 46),
                Color::argb(255, 39, 201, 63),
            )
        } else {
            // Authentic macOS desaturated grey for inactive windows
            (
                Color::argb(255, 78, 80, 92),
                Color::argb(255, 78, 80, 92),
                Color::argb(255, 78, 80, 92),
            )
        };

        // Close (Red / Inactive Grey)
        canvas.fill_circle(self.x + 16, cy, BTN_RADIUS, red_col);
        canvas.draw_circle(self.x + 16, cy, BTN_RADIUS, Color::argb(60, 0, 0, 0));

        // Minimize (Yellow / Inactive Grey)
        canvas.fill_circle(self.x + 34, cy, BTN_RADIUS, yellow_col);
        canvas.draw_circle(self.x + 34, cy, BTN_RADIUS, Color::argb(60, 0, 0, 0));

        // Zoom/Maximize (Green / Inactive Grey)
        canvas.fill_circle(self.x + 52, cy, BTN_RADIUS, green_col);
        canvas.draw_circle(self.x + 52, cy, BTN_RADIUS, Color::argb(60, 0, 0, 0));

        // Authentic macOS inner symbols when hovering traffic light cluster
        if traffic_hover {
            let symbol_color = Color::argb(200, 70, 20, 20);
            draw_char(canvas, self.x + 13, cy - 6, 'x', symbol_color);
            draw_char(canvas, self.x + 31, cy - 6, '-', Color::argb(200, 100, 60, 10));
            draw_char(canvas, self.x + 49, cy - 6, '+', Color::argb(200, 15, 80, 20));
        }

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
            WindowApp::Calc(ref calc) => calc.render(canvas, ix, iy, iw, ih),
            WindowApp::Editor(ref ed) => ed.render(canvas, ix, iy, iw, ih),
            WindowApp::Settings(ref s) => s.render(canvas, ix, iy, iw, ih),
        }

        // 5. Authentic macOS Resize Grip at bottom-right corner (if not maximized)
        if !self.is_maximized {
            let gx = self.x + self.width - 12;
            let gy = self.y + self.height - 12;
            let grip_col = if self.is_active {
                Color::argb(110, 255, 255, 255)
            } else {
                Color::argb(45, 255, 255, 255)
            };
            for i in 0..3 {
                let off = i * 3;
                canvas.fill_rect(gx + 8 - off, gy + 8, 2, 2, grip_col);
                canvas.fill_rect(gx + 8, gy + 8 - off, 2, 2, grip_col);
            }
        }
    }
}
