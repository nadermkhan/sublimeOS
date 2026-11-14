use alloc::string::String;
use crate::gui::apps::{AboutApp, CalcApp, FileManagerApp, SettingsApp, SysmonApp, TerminalApp, TextEditApp};
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::desktop::DesktopAppId;
use crate::gui::font::{draw_string_ellipsis, draw_string_shadow};

pub const TITLEBAR_HEIGHT: i32 = 28;
pub const FRAME_BORDER: i32 = 3;

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
            WindowApp::Calc(_) => (256, 320),
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
        let bx = self.x + self.width - 25;
        let by = self.y + 4;
        px >= bx && px <= bx + 20 && py >= by && py <= by + 20
    }

    pub fn contains_maximize_btn(&self, px: i32, py: i32) -> bool {
        let bx = self.x + self.width - 48;
        let by = self.y + 4;
        px >= bx && px <= bx + 20 && py >= by && py <= by + 20
    }

    pub fn contains_minimize_btn(&self, px: i32, py: i32) -> bool {
        let bx = self.x + self.width - 71;
        let by = self.y + 4;
        px >= bx && px <= bx + 20 && py >= by && py <= by + 20
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
            self.height = (screen_h - 30).max(200); // Fill workspace above XP Taskbar
            self.is_maximized = true;
        }
    }

    pub fn handle_char(&mut self, ch: char) {
        match self.app {
            WindowApp::Terminal(ref mut term) => term.handle_char(ch),
            WindowApp::Calc(ref mut calc) => calc.handle_char(ch),
            WindowApp::Editor(ref mut ed) => ed.handle_char(ch),
            WindowApp::Settings(ref mut s) => s.handle_char(ch),
            WindowApp::Files(ref mut f) => f.handle_char(ch),
            _ => {}
        }
    }

    pub fn handle_inner_click(&mut self, px: i32, py: i32) {
        let (ix, iy, iw, ih) = self.inner_bounds();
        match self.app {
            WindowApp::Files(ref mut files) => files.handle_click(px, py, ix, iy, iw, ih),
            WindowApp::Sysmon(ref mut sysmon) => sysmon.handle_click(px, py, ix, iy, iw, ih),
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

        // 1. Subtle 2-step XP drop shadow (fast perimeter outline, avoids heavy blur/overdraw)
        canvas.fill_rect(self.x + 3, self.y + 3, self.width, self.height, Color::argb(45, 0, 0, 0));

        // 2. Classic Windows XP Luna Outer Frame (3px blue frame around #ECE9D8 body)
        let frame_col = if self.is_active {
            Color::XP_FRAME_ACTIVE
        } else {
            Color::XP_FRAME_INACTIVE
        };
        let outer_col = if self.is_active {
            Color::XP_FRAME_OUTER
        } else {
            Color::rgb(102, 125, 178)
        };

        canvas.fill_rect(self.x, self.y + 4, self.width, self.height - 4, frame_col);
        canvas.draw_rect(self.x, self.y, self.width, self.height, outer_col);

        // 3. Luna Blue Glossy Titlebar
        if self.is_active {
            // Top glossy sheen + vibrant Luna gradient
            canvas.draw_gradient_v(
                self.x + 1,
                self.y + 1,
                self.width - 2,
                6,
                Color::XP_TITLE_HIGHLIGHT,
                Color::XP_TITLE_TOP,
            );
            canvas.draw_gradient_v(
                self.x + 1,
                self.y + 7,
                self.width - 2,
                TITLEBAR_HEIGHT - 7,
                Color::XP_TITLE_TOP,
                Color::XP_TITLE_BOTTOM,
            );
            // Bottom titlebar highlight line
            canvas.fill_rect(self.x + 1, self.y + TITLEBAR_HEIGHT - 1, self.width - 2, 1, Color::XP_FRAME_OUTER);
        } else {
            canvas.draw_gradient_v(
                self.x + 1,
                self.y + 1,
                self.width - 2,
                TITLEBAR_HEIGHT - 1,
                Color::XP_TITLE_INACTIVE_TOP,
                Color::XP_TITLE_INACTIVE_BOT,
            );
        }

        // Crisp Luna top corner rounding illusion (1px outer pixels)
        canvas.set_pixel(self.x, self.y, outer_col);
        canvas.set_pixel(self.x + self.width - 1, self.y, outer_col);

        // 4. Left Mini 14x14 App Icon + Left-Aligned Title (Authentic Windows XP UX)
        draw_mini_window_icon(canvas, self.x + 7, self.y + 7, self.app.app_id());
        let title_max_w = (self.width - 108).max(24);
        canvas.set_clip_rect(self.x + 26, self.y + 4, title_max_w, TITLEBAR_HEIGHT - 6);
        if self.is_active {
            draw_string_shadow(
                canvas,
                self.x + 26,
                self.y + 6,
                &self.title,
                Color::WHITE,
                Color::rgb(0, 32, 110),
            );
        } else {
            draw_string_ellipsis(
                canvas,
                self.x + 26,
                self.y + 6,
                &self.title,
                Color::rgb(225, 234, 250),
                title_max_w,
            );
        }
        canvas.clear_clip_rect();

        // 5. Top-Right SublimeOS Caption Buttons: [_] [□] [X]
        let btn_y = self.y + 4;
        let min_x = self.x + self.width - 71;
        let max_x = self.x + self.width - 48;
        let cls_x = self.x + self.width - 25;

        let min_hover = mouse_x >= min_x && mouse_x <= min_x + 20 && mouse_y >= btn_y && mouse_y <= btn_y + 20;
        let max_hover = mouse_x >= max_x && mouse_x <= max_x + 20 && mouse_y >= btn_y && mouse_y <= btn_y + 20;
        let cls_hover = mouse_x >= cls_x && mouse_x <= cls_x + 20 && mouse_y >= btn_y && mouse_y <= btn_y + 20;

        // Minimize Button (_)
        draw_xp_caption_btn(
            canvas,
            min_x,
            btn_y,
            if min_hover { Color::XP_CAP_BLUE_HOVER_TOP } else if self.is_active { Color::XP_CAP_BLUE_TOP } else { Color::XP_TITLE_INACTIVE_TOP },
            if min_hover { Color::XP_CAP_BLUE_HOVER_BOT } else if self.is_active { Color::XP_CAP_BLUE_BOT } else { Color::XP_TITLE_INACTIVE_BOT },
        );
        canvas.fill_rect(min_x + 5, btn_y + 13, 8, 2, Color::WHITE);

        // Maximize / Restore Button (□)
        draw_xp_caption_btn(
            canvas,
            max_x,
            btn_y,
            if max_hover { Color::XP_CAP_BLUE_HOVER_TOP } else if self.is_active { Color::XP_CAP_BLUE_TOP } else { Color::XP_TITLE_INACTIVE_TOP },
            if max_hover { Color::XP_CAP_BLUE_HOVER_BOT } else if self.is_active { Color::XP_CAP_BLUE_BOT } else { Color::XP_TITLE_INACTIVE_BOT },
        );
        canvas.draw_rect(max_x + 5, btn_y + 5, 10, 10, Color::WHITE);
        canvas.fill_rect(max_x + 5, btn_y + 5, 10, 2, Color::WHITE);

        // Close Button (Red X)
        draw_xp_caption_btn(
            canvas,
            cls_x,
            btn_y,
            if cls_hover { Color::XP_CAP_RED_HOVER_TOP } else if self.is_active { Color::XP_CAP_RED_TOP } else { Color::rgb(212, 146, 134) },
            if cls_hover { Color::XP_CAP_RED_HOVER_BOT } else if self.is_active { Color::XP_CAP_RED_BOT } else { Color::rgb(176, 102, 92) },
        );
        // Crisp 2px thick white X
        for i in 0..9 {
            canvas.fill_rect(cls_x + 5 + i, btn_y + 5 + i, 2, 2, Color::WHITE);
            canvas.fill_rect(cls_x + 13 - i, btn_y + 5 + i, 2, 2, Color::WHITE);
        }

        // 6. Render App Content strictly clipped inside inner client bounds
        let (ix, iy, iw, ih) = self.inner_bounds();
        canvas.set_clip_rect(ix, iy, iw, ih);
        canvas.fill_rect(ix, iy, iw, ih, Color::XP_FACE);
        match self.app {
            WindowApp::Terminal(ref mut term) => term.render(canvas, ix, iy, iw, ih),
            WindowApp::Sysmon(ref sysmon) => sysmon.render(canvas, ix, iy, iw, ih),
            WindowApp::Files(ref files) => files.render(canvas, ix, iy, iw, ih),
            WindowApp::About(ref about) => about.render(canvas, ix, iy, iw, ih),
            WindowApp::Calc(ref calc) => calc.render(canvas, ix, iy, iw, ih),
            WindowApp::Editor(ref ed) => ed.render(canvas, ix, iy, iw, ih),
            WindowApp::Settings(ref settings) => settings.render(canvas, ix, iy, iw, ih),
        }
        canvas.clear_clip_rect();

        // 7. Classic Resize Grip at bottom-right corner (if not maximized)
        if !self.is_maximized {
            let rx = self.x + self.width - 14;
            let by = self.y + self.height - 14;
            for step in 0..3 {
                let d = step * 4;
                canvas.draw_line(rx + 2 + d, by + 10, rx + 10, by + 2 + d, Color::XP_SHADOW);
                canvas.draw_line(rx + 3 + d, by + 10, rx + 10, by + 3 + d, Color::WHITE);
            }
        }
    }
}

fn draw_xp_caption_btn(canvas: &mut Canvas, x: i32, y: i32, top: Color, bot: Color) {
    canvas.draw_gradient_v(x + 1, y + 1, 18, 18, top, bot);
    canvas.draw_rounded_rect(x, y, 20, 20, 3, Color::WHITE);
}

pub fn draw_mini_window_icon(canvas: &mut Canvas, x: i32, y: i32, app_id: DesktopAppId) {
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
            canvas.fill_rect(x + 1, y + 1, 12, 12, Color::XP_FACE);
            canvas.fill_rect(x + 3, y + 3, 4, 4, Color::rgb(230, 75, 45));
            canvas.fill_rect(x + 7, y + 3, 4, 4, Color::rgb(95, 185, 55));
            canvas.fill_rect(x + 3, y + 7, 4, 4, Color::rgb(45, 130, 235));
            canvas.fill_rect(x + 7, y + 7, 4, 4, Color::rgb(245, 195, 45));
        }
    }
}
