use alloc::string::String;
use crate::event::{EventResult, GuiEvent};
use crate::gfx::{Rect, Surface, Theme};

/// Creation descriptor for both in-kernel and Ring-3 remote windows.
#[derive(Clone, Debug)]
pub struct WindowSpec {
    pub title: String,
    pub width: i32,
    pub height: i32,
    pub min_width: i32,
    pub min_height: i32,
    pub resizable: bool,
}

impl WindowSpec {
    pub fn new(title: &str, width: i32, height: i32) -> Self {
        Self {
            title: String::from(title),
            width,
            height,
            min_width: 240,
            min_height: 160,
            resizable: true,
        }
    }
}

/// Shared UI application interface implemented by both kernel apps and Ring-3 GUI clients.
pub trait WindowClient {
    fn spec(&self) -> WindowSpec;
    fn draw(&mut self, surface: &mut Surface, bounds: Rect, theme: &Theme);
    fn on_event(&mut self, event: GuiEvent, bounds: Rect) -> EventResult;

    /// Called once when the window client is attached to a live surface/session.
    fn on_mount(&mut self, _bounds: Rect) {}
    /// Called before teardown when the window is closing.
    fn on_unmount(&mut self) {}
    /// Called when the compositor switches system themes.
    fn on_theme_changed(&mut self, _theme: crate::gfx::ThemeKind) {}
    /// Called when window keyboard focus changes.
    fn on_focus(&mut self, _focused: bool) {}
    /// Called when the compositor broadcasts a system clipboard update.
    fn on_clipboard(&mut self, _text: &str) {}
    /// Returns the damaged sub-rectangle to repaint and submit via CommitDamage.
    fn take_damage(&mut self, bounds: Rect) -> Option<Rect> {
        Some(bounds)
    }
}
