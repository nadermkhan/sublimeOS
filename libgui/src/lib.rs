#![no_std]

extern crate alloc;

pub mod app;
pub mod code_highlight;
pub mod codecs;
pub mod component;
pub mod event;
pub mod font;
pub mod gfx;
pub mod ipc;
pub mod layout;
pub mod system;
pub mod widget;

pub use app::{WindowClient, WindowSpec};
pub use component::{
    ColorPickerDialog, ComponentContext, ComponentTree, FontPickerDialog, ManagedButton,
    ManagedCheckBox, ManagedComboBox, ManagedSlider, ManagedTextInput, ScrollArea, SplitOrientation,
    SplitPane, UiComponent,
};
pub use event::{EventResult, GuiEvent, KeyCode, Modifiers, MouseButton};
pub use gfx::{ArrowDir, Color, Insets, Point, Rect, Size, Surface, Theme, ThemeKind, ThemePart, ThemeState, UxTheme};
pub use system::{
    CommandId, DamageTracker, FocusManager, HotkeyRegistry, SystemClipboard, TimerScheduler,
    WidgetId,
};
