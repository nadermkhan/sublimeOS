use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::event::{EventResult, GuiEvent, KeyCode, MouseButton};
use crate::font::{with_default_font, AntialiasMode, TextStyle};
use crate::gfx::{Color, Rect, Surface, Theme, ThemeKind, UxTheme};
use crate::system::{
    CommandId, DamageTracker, FocusManager, HotkeyRegistry, SystemClipboard, TimerScheduler,
    WidgetId,
};
use crate::widget::{
    Button, ButtonState, ButtonStyle, CheckBox, CheckBoxState, CheckState, ComboBox,
    ComboBoxState, DialogResult, Label, ScrollBar, ScrollBarState, ScrollOrientation, Slider,
    SliderState, TextInput, TextInputState, ToastSeverity,
};

/// Shared runtime services passed into UI lifecycle & callback handlers.
pub struct ComponentContext {
    pub damage: DamageTracker,
    pub focus: FocusManager,
    pub timers: TimerScheduler,
    pub hotkeys: HotkeyRegistry,
    pub commands: Vec<CommandId>,
    pub pending_toast: Option<(String, String, ToastSeverity)>,
    pub clipboard_dirty_sync: Option<String>,
}

impl Default for ComponentContext {
    fn default() -> Self {
        Self::new()
    }
}

impl ComponentContext {
    pub fn new() -> Self {
        Self {
            damage: DamageTracker::new(),
            focus: FocusManager::new(),
            timers: TimerScheduler::new(),
            hotkeys: HotkeyRegistry::new(),
            commands: Vec::new(),
            pending_toast: None,
            clipboard_dirty_sync: None,
        }
    }

    pub fn invalidate(&mut self, rect: Rect) {
        self.damage.invalidate(rect);
    }

    pub fn emit(&mut self, cmd: CommandId) {
        self.commands.push(cmd);
    }

    pub fn notify(&mut self, title: &str, body: &str, severity: ToastSeverity) {
        self.pending_toast = Some((String::from(title), String::from(body), severity));
    }

    pub fn copy_to_clipboard(&mut self, text: &str) {
        SystemClipboard::set_text(text);
        self.clipboard_dirty_sync = Some(String::from(text));
    }

    pub fn paste_from_clipboard(&self) -> String {
        SystemClipboard::get_text()
    }
}

/// Retained UI component contract with explicit mount, unmount, focus, theme, timer & input lifecycle.
pub trait UiComponent<S> {
    fn id(&self) -> WidgetId;
    fn bounds(&self) -> Rect;
    fn set_bounds(&mut self, rect: Rect);
    fn is_focusable(&self) -> bool {
        false
    }

    fn on_mount(&mut self, _state: &mut S, _ctx: &mut ComponentContext) {}
    fn on_unmount(&mut self, _state: &mut S, _ctx: &mut ComponentContext) {}
    fn on_focus(&mut self, _focused: bool, _state: &mut S, _ctx: &mut ComponentContext) {}
    fn on_theme_changed(&mut self, _kind: ThemeKind, _state: &mut S, _ctx: &mut ComponentContext) {}
    fn on_tick(&mut self, _now_ms: u64, _state: &mut S, _ctx: &mut ComponentContext) {}
    fn on_event(&mut self, event: GuiEvent, state: &mut S, ctx: &mut ComponentContext) -> EventResult;
    fn draw(&mut self, surface: &mut Surface, theme: &Theme, focused: bool);
}

/// Callback-bound managed PushButton component (`BS_PUSHBUTTON` / `BS_DEFPUSHBUTTON`).
pub struct ManagedButton<S> {
    pub id: WidgetId,
    pub rect: Rect,
    pub label: String,
    pub style: ButtonStyle,
    pub state: ButtonState,
    on_click: Option<Box<dyn FnMut(&mut S, &mut ComponentContext)>>,
}

impl<S> ManagedButton<S> {
    pub fn new(id: WidgetId, rect: Rect, label: &str, style: ButtonStyle) -> Self {
        Self {
            id,
            rect,
            label: String::from(label),
            style,
            state: ButtonState::default(),
            on_click: None,
        }
    }

    pub fn on_click<F: FnMut(&mut S, &mut ComponentContext) + 'static>(mut self, cb: F) -> Self {
        self.on_click = Some(Box::new(cb));
        self
    }
}

impl<S> UiComponent<S> for ManagedButton<S> {
    fn id(&self) -> WidgetId {
        self.id
    }
    fn bounds(&self) -> Rect {
        self.rect
    }
    fn set_bounds(&mut self, rect: Rect) {
        self.rect = rect;
    }
    fn is_focusable(&self) -> bool {
        !self.state.disabled
    }
    fn on_event(&mut self, event: GuiEvent, state: &mut S, ctx: &mut ComponentContext) -> EventResult {
        if let GuiEvent::KeyDown { key: KeyCode::Enter | KeyCode::Char(' '), .. } = event {
            if ctx.focus.is_focused(self.id) && !self.state.disabled {
                if let Some(ref mut cb) = self.on_click {
                    cb(state, ctx);
                }
                ctx.invalidate(self.rect);
                return EventResult::NeedsRedraw;
            }
        }
        let (res, clicked) = Button::handle_event(&mut self.state, self.rect, event);
        if res.needs_redraw() {
            ctx.invalidate(self.rect);
        }
        if clicked {
            ctx.focus.set_focus(Some(self.id));
            if let Some(ref mut cb) = self.on_click {
                cb(state, ctx);
            }
        }
        res
    }
    fn draw(&mut self, surface: &mut Surface, theme: &Theme, focused: bool) {
        Button::draw(surface, self.rect, &self.label, self.style, self.state, theme);
        if focused {
            UxTheme::draw_focus_rect(surface, self.rect, theme);
        }
    }
}

/// Callback-bound CheckBox component (`BS_AUTOCHECKBOX` / `BS_3STATE`).
pub struct ManagedCheckBox<S> {
    pub id: WidgetId,
    pub rect: Rect,
    pub label: String,
    pub state: CheckBoxState,
    on_change: Option<Box<dyn FnMut(&mut S, CheckState, &mut ComponentContext)>>,
}

impl<S> ManagedCheckBox<S> {
    pub fn new(id: WidgetId, rect: Rect, label: &str, checked: bool) -> Self {
        Self {
            id,
            rect,
            label: String::from(label),
            state: CheckBoxState::new(checked),
            on_change: None,
        }
    }

    pub fn on_change<F: FnMut(&mut S, CheckState, &mut ComponentContext) + 'static>(mut self, cb: F) -> Self {
        self.on_change = Some(Box::new(cb));
        self
    }
}

impl<S> UiComponent<S> for ManagedCheckBox<S> {
    fn id(&self) -> WidgetId {
        self.id
    }
    fn bounds(&self) -> Rect {
        self.rect
    }
    fn set_bounds(&mut self, rect: Rect) {
        self.rect = rect;
    }
    fn is_focusable(&self) -> bool {
        !self.state.disabled
    }
    fn on_focus(&mut self, focused: bool, _state: &mut S, ctx: &mut ComponentContext) {
        self.state.focused = focused;
        ctx.invalidate(self.rect);
    }
    fn on_event(&mut self, event: GuiEvent, state: &mut S, ctx: &mut ComponentContext) -> EventResult {
        let (res, toggled) = CheckBox::handle_event(&mut self.state, self.rect, event);
        if res.needs_redraw() {
            ctx.invalidate(self.rect);
        }
        if toggled {
            ctx.focus.set_focus(Some(self.id));
            if let Some(ref mut cb) = self.on_change {
                cb(state, self.state.check, ctx);
            }
        }
        res
    }
    fn draw(&mut self, surface: &mut Surface, theme: &Theme, focused: bool) {
        let mut st = self.state;
        st.focused = focused;
        CheckBox::draw(surface, self.rect, &self.label, st, theme);
    }
}

/// Callback-bound Slider component (`msctls_trackbar32`).
pub struct ManagedSlider<S> {
    pub id: WidgetId,
    pub rect: Rect,
    pub state: SliderState,
    on_change: Option<Box<dyn FnMut(&mut S, i32, &mut ComponentContext)>>,
}

impl<S> ManagedSlider<S> {
    pub fn new(id: WidgetId, rect: Rect, value: i32, min: i32, max: i32) -> Self {
        Self {
            id,
            rect,
            state: SliderState::new(value, min, max),
            on_change: None,
        }
    }

    pub fn on_change<F: FnMut(&mut S, i32, &mut ComponentContext) + 'static>(mut self, cb: F) -> Self {
        self.on_change = Some(Box::new(cb));
        self
    }
}

impl<S> UiComponent<S> for ManagedSlider<S> {
    fn id(&self) -> WidgetId {
        self.id
    }
    fn bounds(&self) -> Rect {
        self.rect
    }
    fn set_bounds(&mut self, rect: Rect) {
        self.rect = rect;
    }
    fn is_focusable(&self) -> bool {
        true
    }
    fn on_event(&mut self, event: GuiEvent, state: &mut S, ctx: &mut ComponentContext) -> EventResult {
        let (res, changed) = Slider::handle_event(&mut self.state, self.rect, event);
        if res.needs_redraw() {
            ctx.invalidate(self.rect);
        }
        if changed {
            if let Some(ref mut cb) = self.on_change {
                cb(state, self.state.value, ctx);
            }
        }
        res
    }
    fn draw(&mut self, surface: &mut Surface, theme: &Theme, focused: bool) {
        Slider::draw(surface, self.rect, self.state, theme);
        if focused {
            UxTheme::draw_focus_rect(surface, self.rect, theme);
        }
    }
}

/// Callback-bound ComboBox component (`CBS_DROPDOWNLIST`).
pub struct ManagedComboBox<S> {
    pub id: WidgetId,
    pub rect: Rect,
    pub items: Vec<String>,
    pub state: ComboBoxState,
    on_select: Option<Box<dyn FnMut(&mut S, usize, &mut ComponentContext)>>,
}

impl<S> ManagedComboBox<S> {
    pub fn new(id: WidgetId, rect: Rect, items: Vec<String>, selected: usize) -> Self {
        Self {
            id,
            rect,
            items,
            state: ComboBoxState::new(selected),
            on_select: None,
        }
    }

    pub fn on_select<F: FnMut(&mut S, usize, &mut ComponentContext) + 'static>(mut self, cb: F) -> Self {
        self.on_select = Some(Box::new(cb));
        self
    }
}

impl<S> UiComponent<S> for ManagedComboBox<S> {
    fn id(&self) -> WidgetId {
        self.id
    }
    fn bounds(&self) -> Rect {
        self.rect
    }
    fn set_bounds(&mut self, rect: Rect) {
        self.rect = rect;
    }
    fn is_focusable(&self) -> bool {
        !self.state.disabled
    }
    fn on_event(&mut self, event: GuiEvent, state: &mut S, ctx: &mut ComponentContext) -> EventResult {
        let refs: Vec<&str> = self.items.iter().map(|s| s.as_str()).collect();
        let was_open = self.state.open;
        let (res, sel) = ComboBox::handle_event(&mut self.state, self.rect, &refs, event);
        if res.needs_redraw() || was_open != self.state.open {
            ctx.invalidate(self.rect.union(ComboBox::popup_rect(self.rect, refs.len())));
        }
        if let Some(idx) = sel {
            if let Some(ref mut cb) = self.on_select {
                cb(state, idx, ctx);
            }
        }
        res
    }
    fn draw(&mut self, surface: &mut Surface, theme: &Theme, focused: bool) {
        let refs: Vec<&str> = self.items.iter().map(|s| s.as_str()).collect();
        ComboBox::draw(surface, self.rect, &refs, self.state, theme);
        ComboBox::draw_popup(surface, self.rect, &refs, self.state, theme);
        if focused {
            UxTheme::draw_focus_rect(surface, self.rect, theme);
        }
    }
}

/// Callback-bound TextInput component with system clipboard (`Ctrl+C`/`Ctrl+V`) support.
pub struct ManagedTextInput<S> {
    pub id: WidgetId,
    pub rect: Rect,
    pub placeholder: String,
    pub state: TextInputState,
    on_change: Option<Box<dyn FnMut(&mut S, &str, &mut ComponentContext)>>,
    on_submit: Option<Box<dyn FnMut(&mut S, &str, &mut ComponentContext)>>,
}

impl<S> ManagedTextInput<S> {
    pub fn new(id: WidgetId, rect: Rect, initial: &str, placeholder: &str) -> Self {
        Self {
            id,
            rect,
            placeholder: String::from(placeholder),
            state: TextInputState::new(initial),
            on_change: None,
            on_submit: None,
        }
    }

    pub fn on_change<F: FnMut(&mut S, &str, &mut ComponentContext) + 'static>(mut self, cb: F) -> Self {
        self.on_change = Some(Box::new(cb));
        self
    }

    pub fn on_submit<F: FnMut(&mut S, &str, &mut ComponentContext) + 'static>(mut self, cb: F) -> Self {
        self.on_submit = Some(Box::new(cb));
        self
    }
}

impl<S> UiComponent<S> for ManagedTextInput<S> {
    fn id(&self) -> WidgetId {
        self.id
    }
    fn bounds(&self) -> Rect {
        self.rect
    }
    fn set_bounds(&mut self, rect: Rect) {
        self.rect = rect;
    }
    fn is_focusable(&self) -> bool {
        true
    }
    fn on_focus(&mut self, focused: bool, _state: &mut S, ctx: &mut ComponentContext) {
        self.state.focused = focused;
        ctx.invalidate(self.rect);
    }
    fn on_event(&mut self, event: GuiEvent, state: &mut S, ctx: &mut ComponentContext) -> EventResult {
        if self.state.focused {
            if let GuiEvent::KeyDown { key: KeyCode::Char(c), mods, .. } = event {
                if mods.ctrl && (c == 'c' || c == 'C') {
                    ctx.copy_to_clipboard(&self.state.text);
                    return EventResult::Handled;
                }
                if mods.ctrl && (c == 'v' || c == 'V') {
                    let clip = ctx.paste_from_clipboard();
                    if !clip.is_empty() {
                        self.state.text.push_str(&clip);
                        self.state.cursor_bytes = self.state.text.len();
                        if let Some(ref mut cb) = self.on_change {
                            cb(state, &self.state.text, ctx);
                        }
                        ctx.invalidate(self.rect);
                        return EventResult::NeedsRedraw;
                    }
                    return EventResult::Handled;
                }
            }
        }
        let prev_len = self.state.text.len();
        let prev_text_hash = self.state.text.as_bytes().to_vec();
        let (res, submitted) = TextInput::handle_event(&mut self.state, self.rect, event);
        let changed = self.state.text.len() != prev_len || self.state.text.as_bytes() != prev_text_hash.as_slice();
        if self.state.focused {
            ctx.focus.set_focus(Some(self.id));
        }
        if res.needs_redraw() {
            ctx.invalidate(self.rect);
        }
        if changed {
            if let Some(ref mut cb) = self.on_change {
                cb(state, &self.state.text, ctx);
            }
        }
        if submitted {
            if let Some(ref mut cb) = self.on_submit {
                cb(state, &self.state.text, ctx);
            }
        }
        res
    }
    fn draw(&mut self, surface: &mut Surface, theme: &Theme, focused: bool) {
        self.state.focused = focused;
        TextInput::draw(surface, self.rect, &mut self.state, &self.placeholder, true, theme);
    }
}

/// Manages a tree of `UiComponent<S>` children with mount/unmount lifecycle, focus traversal & localized damage.
pub struct ComponentTree<S> {
    children: Vec<Box<dyn UiComponent<S>>>,
    mounted: bool,
    pub ctx: ComponentContext,
}

impl<S> Default for ComponentTree<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> ComponentTree<S> {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            mounted: false,
            ctx: ComponentContext::new(),
        }
    }

    pub fn add(&mut self, comp: Box<dyn UiComponent<S>>) {
        if comp.is_focusable() {
            self.ctx.focus.register(comp.id());
        }
        self.children.push(comp);
    }

    pub fn mount(&mut self, state: &mut S) {
        if self.mounted {
            return;
        }
        self.mounted = true;
        for c in &mut self.children {
            c.on_mount(state, &mut self.ctx);
        }
        self.ctx.damage.invalidate_all();
    }

    pub fn unmount(&mut self, state: &mut S) {
        if !self.mounted {
            return;
        }
        self.mounted = false;
        for c in &mut self.children {
            c.on_unmount(state, &mut self.ctx);
        }
    }

    pub fn notify_theme_changed(&mut self, kind: ThemeKind, state: &mut S) {
        for c in &mut self.children {
            c.on_theme_changed(kind, state, &mut self.ctx);
        }
        self.ctx.damage.invalidate_all();
    }

    pub fn dispatch_event(&mut self, event: GuiEvent, state: &mut S) -> EventResult {
        if let GuiEvent::Tick { now_ms } = event {
            for c in &mut self.children {
                c.on_tick(now_ms, state, &mut self.ctx);
            }
            return if self.ctx.damage.is_dirty() {
                EventResult::NeedsRedraw
            } else {
                EventResult::Ignored
            };
        }

        if let GuiEvent::KeyDown { key: KeyCode::Tab, mods, .. } = event {
            let (prev, next) = if mods.shift {
                self.ctx.focus.focus_prev()
            } else {
                self.ctx.focus.focus_next()
            };
            if prev != next {
                for c in &mut self.children {
                    if Some(c.id()) == prev {
                        c.on_focus(false, state, &mut self.ctx);
                    } else if Some(c.id()) == next {
                        c.on_focus(true, state, &mut self.ctx);
                    }
                }
                return EventResult::NeedsRedraw;
            }
        }

        if let Some(cmd) = self.ctx.hotkeys.match_event(event) {
            self.ctx.emit(cmd);
        }

        let mut res = EventResult::Ignored;
        for c in self.children.iter_mut().rev() {
            let r = c.on_event(event, state, &mut self.ctx);
            res.merge(r);
        }
        res
    }

    pub fn draw_all(&mut self, surface: &mut Surface, theme: &Theme) {
        let focused = self.ctx.focus.focused();
        for c in &mut self.children {
            let is_f = focused == Some(c.id());
            c.draw(surface, theme, is_f);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SplitOrientation {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug)]
pub struct SplitPaneState {
    pub split_px: i32,
    pub min_first: i32,
    pub min_second: i32,
    pub dragging: bool,
    pub hovered: bool,
}

impl SplitPaneState {
    pub const fn new(split_px: i32) -> Self {
        Self {
            split_px,
            min_first: 80,
            min_second: 80,
            dragging: false,
            hovered: false,
        }
    }
}

/// Interactive draggable horizontal/vertical splitter pane container.
pub struct SplitPane;

impl SplitPane {
    const BAR_THICK: i32 = 6;

    pub fn layout(bounds: Rect, ori: SplitOrientation, state: &SplitPaneState) -> (Rect, Rect, Rect) {
        match ori {
            SplitOrientation::Horizontal => {
                let s = state.split_px.clamp(state.min_first, (bounds.w - Self::BAR_THICK - state.min_second).max(state.min_first));
                let a = Rect::new(bounds.x, bounds.y, s, bounds.h);
                let bar = Rect::new(a.right(), bounds.y, Self::BAR_THICK, bounds.h);
                let b = Rect::new(bar.right(), bounds.y, (bounds.right() - bar.right()).max(0), bounds.h);
                (a, bar, b)
            }
            SplitOrientation::Vertical => {
                let s = state.split_px.clamp(state.min_first, (bounds.h - Self::BAR_THICK - state.min_second).max(state.min_first));
                let a = Rect::new(bounds.x, bounds.y, bounds.w, s);
                let bar = Rect::new(bounds.x, a.bottom(), bounds.w, Self::BAR_THICK);
                let b = Rect::new(bounds.x, bar.bottom(), bounds.w, (bounds.bottom() - bar.bottom()).max(0));
                (a, bar, b)
            }
        }
    }

    pub fn handle_event(
        state: &mut SplitPaneState,
        bounds: Rect,
        ori: SplitOrientation,
        event: GuiEvent,
    ) -> EventResult {
        let (_, bar, _) = Self::layout(bounds, ori, state);
        match event {
            GuiEvent::MouseMove { x, y } => {
                if state.dragging {
                    let raw = match ori {
                        SplitOrientation::Horizontal => x - bounds.x - Self::BAR_THICK / 2,
                        SplitOrientation::Vertical => y - bounds.y - Self::BAR_THICK / 2,
                    };
                    let max_s = match ori {
                        SplitOrientation::Horizontal => bounds.w - Self::BAR_THICK - state.min_second,
                        SplitOrientation::Vertical => bounds.h - Self::BAR_THICK - state.min_second,
                    }
                    .max(state.min_first);
                    let next = raw.clamp(state.min_first, max_s);
                    if next != state.split_px {
                        state.split_px = next;
                        return EventResult::NeedsRedraw;
                    }
                    return EventResult::Handled;
                }
                let h = bar.contains(x, y);
                if h != state.hovered {
                    state.hovered = h;
                    return EventResult::NeedsRedraw;
                }
                EventResult::Ignored
            }
            GuiEvent::MouseDown { x, y, button: MouseButton::Left } if bar.contains(x, y) => {
                state.dragging = true;
                EventResult::NeedsRedraw
            }
            GuiEvent::MouseUp { button: MouseButton::Left, .. } if state.dragging => {
                state.dragging = false;
                EventResult::NeedsRedraw
            }
            _ => EventResult::Ignored,
        }
    }

    pub fn draw_bar(surface: &mut Surface, bar: Rect, ori: SplitOrientation, state: SplitPaneState, theme: &Theme) {
        let col = if state.dragging || state.hovered { theme.face_light } else { theme.face };
        surface.fill_rect(bar, col);
        let cx = bar.x + bar.w / 2;
        let cy = bar.y + bar.h / 2;
        for d in [-6, 0, 6] {
            match ori {
                SplitOrientation::Horizontal => surface.fill_rect(Rect::new(cx - 1, cy + d, 2, 2), theme.border_shadow),
                SplitOrientation::Vertical => surface.fill_rect(Rect::new(cx + d, cy - 1, 2, 2), theme.border_shadow),
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ScrollAreaState {
    pub scroll: ScrollBarState,
}

impl ScrollAreaState {
    pub const fn new(content_h: i32, page_h: i32) -> Self {
        let max_s = if content_h > page_h { content_h - page_h } else { 0 };
        let pg = if page_h > 16 { page_h } else { 16 };
        Self {
            scroll: ScrollBarState::new(0, max_s, pg),
        }
    }
}

/// Reusable viewport-clipped vertical ScrollArea with integrated `ScrollBar` & mouse wheel.
pub struct ScrollArea;

impl ScrollArea {
    const SB_W: i32 = 14;

    pub fn viewport_and_bar(bounds: Rect, content_h: i32, state: &mut ScrollAreaState) -> (Rect, Rect) {
        let max_s = (content_h - bounds.h).max(0);
        state.scroll.max = max_s;
        state.scroll.page = bounds.h.max(16);
        state.scroll.value = state.scroll.value.clamp(0, max_s);
        if max_s == 0 {
            return (bounds, Rect::default());
        }
        let vp = Rect::new(bounds.x, bounds.y, (bounds.w - Self::SB_W).max(0), bounds.h);
        let sb = Rect::new(vp.right(), bounds.y, Self::SB_W, bounds.h);
        (vp, sb)
    }

    pub fn handle_event(state: &mut ScrollAreaState, bounds: Rect, content_h: i32, event: GuiEvent) -> EventResult {
        let (_, sb) = Self::viewport_and_bar(bounds, content_h, state);
        if !sb.is_empty() {
            let (sr, _) = ScrollBar::handle_event(&mut state.scroll, sb, ScrollOrientation::Vertical, event);
            if sr.consumed() {
                return sr;
            }
        }
        if let GuiEvent::MouseWheel { x, y, delta_y } = event {
            if bounds.contains(x, y) && state.scroll.max > 0 {
                let prev = state.scroll.value;
                state.scroll.value = (state.scroll.value - delta_y * 24).clamp(0, state.scroll.max);
                return if state.scroll.value != prev { EventResult::NeedsRedraw } else { EventResult::Handled };
            }
        }
        EventResult::Ignored
    }
}

/// Windows `ChooseColor`-style 16-swatch Color Picker common dialog component.
#[derive(Clone, Debug)]
pub struct ColorPickerState {
    pub visible: bool,
    pub selected: Color,
    pub btn_ok: ButtonState,
    pub btn_cancel: ButtonState,
}

impl Default for ColorPickerState {
    fn default() -> Self {
        Self {
            visible: false,
            selected: Color::rgb(49, 106, 197),
            btn_ok: ButtonState::default(),
            btn_cancel: ButtonState::default(),
        }
    }
}

pub struct ColorPickerDialog;

impl ColorPickerDialog {
    pub const SWATCHES: [Color; 16] = [
        Color::rgb(0, 0, 0),
        Color::rgb(128, 128, 128),
        Color::rgb(255, 255, 255),
        Color::rgb(196, 48, 20),
        Color::rgb(230, 139, 44),
        Color::rgb(245, 200, 60),
        Color::rgb(45, 160, 65),
        Color::rgb(20, 150, 165),
        Color::rgb(49, 106, 197),
        Color::rgb(0, 60, 116),
        Color::rgb(108, 70, 185),
        Color::rgb(190, 65, 135),
        Color::rgb(236, 233, 216),
        Color::rgb(30, 35, 43),
        Color::rgb(78, 178, 245),
        Color::rgb(104, 224, 136),
    ];

    fn modal_rect(bounds: Rect) -> Rect {
        let w = 276;
        let h = 166;
        Rect::new(bounds.x + (bounds.w - w) / 2, bounds.y + (bounds.h - h) / 2, w, h)
    }

    pub fn handle_event(state: &mut ColorPickerState, bounds: Rect, event: GuiEvent) -> (EventResult, DialogResult) {
        if !state.visible {
            return (EventResult::Ignored, DialogResult::None);
        }
        let m = Self::modal_rect(bounds);
        let ok_r = Rect::new(m.right() - 160, m.bottom() - 32, 70, 22);
        let can_r = Rect::new(m.right() - 82, m.bottom() - 32, 70, 22);
        let (r1, c1) = Button::handle_event(&mut state.btn_ok, ok_r, event);
        if c1 {
            state.visible = false;
            return (EventResult::NeedsRedraw, DialogResult::Ok);
        }
        let (r2, c2) = Button::handle_event(&mut state.btn_cancel, can_r, event);
        if c2 {
            state.visible = false;
            return (EventResult::NeedsRedraw, DialogResult::Cancel);
        }
        if let GuiEvent::MouseDown { x, y, button: MouseButton::Left } = event {
            let sx0 = m.x + 14;
            let sy0 = m.y + 34;
            for (i, col) in Self::SWATCHES.iter().enumerate() {
                let r = Rect::new(sx0 + (i as i32 % 8) * 30, sy0 + (i as i32 / 8) * 30, 24, 24);
                if r.contains(x, y) {
                    state.selected = *col;
                    return (EventResult::NeedsRedraw, DialogResult::None);
                }
            }
        }
        let mut res = EventResult::Handled;
        res.merge(r1);
        res.merge(r2);
        (res, DialogResult::None)
    }

    pub fn draw(surface: &mut Surface, bounds: Rect, state: &ColorPickerState, theme: &Theme) {
        if !state.visible {
            return;
        }
        surface.fill_rect(bounds, Color::argb(96, 0, 0, 0));
        let m = Self::modal_rect(bounds);
        surface.fill_rect(m, theme.face);
        surface.draw_rect(m, theme.frame_outer);
        let title_r = Rect::new(m.x + 1, m.y + 1, m.w - 2, 24);
        surface.draw_gradient_v(title_r, theme.title_top, theme.title_bot);
        Label::draw(surface, Rect::new(title_r.x + 8, title_r.y, title_r.w - 16, title_r.h), "Choose Color", TextStyle { bold: true, color: Color::WHITE, ..TextStyle::ui(Color::WHITE) });

        for (i, col) in Self::SWATCHES.iter().enumerate() {
            let r = Rect::new(m.x + 14 + (i as i32 % 8) * 30, m.y + 34 + (i as i32 / 8) * 30, 24, 24);
            surface.fill_rect(r, *col);
            surface.draw_rect(r, if *col == state.selected { theme.frame_outer } else { theme.ctrl_border });
        }
        let prev_r = Rect::new(m.x + 14, m.y + 102, 74, 22);
        surface.fill_rect(prev_r, state.selected);
        surface.draw_rect(prev_r, theme.ctrl_border);

        let ok_r = Rect::new(m.right() - 160, m.bottom() - 32, 70, 22);
        let can_r = Rect::new(m.right() - 82, m.bottom() - 32, 70, 22);
        Button::draw(surface, ok_r, "OK", ButtonStyle::Primary, state.btn_ok, theme);
        Button::draw(surface, can_r, "Cancel", ButtonStyle::Normal, state.btn_cancel, theme);
    }
}

/// Windows `ChooseFont`-style Font & ClearType AA Picker dialog component.
#[derive(Clone, Debug)]
pub struct FontPickerState {
    pub visible: bool,
    pub size_px: i32,
    pub bold: bool,
    pub cleartype: bool,
    pub btn_ok: ButtonState,
    pub btn_cancel: ButtonState,
    pub chk_bold: CheckBoxState,
    pub chk_ct: CheckBoxState,
    pub btn_smaller: ButtonState,
    pub btn_larger: ButtonState,
}

impl Default for FontPickerState {
    fn default() -> Self {
        Self {
            visible: false,
            size_px: 14,
            bold: false,
            cleartype: true,
            btn_ok: ButtonState::default(),
            btn_cancel: ButtonState::default(),
            chk_bold: CheckBoxState::new(false),
            chk_ct: CheckBoxState::new(true),
            btn_smaller: ButtonState::default(),
            btn_larger: ButtonState::default(),
        }
    }
}

pub struct FontPickerDialog;

impl FontPickerDialog {
    fn modal_rect(bounds: Rect) -> Rect {
        let w = 310;
        let h = 196;
        Rect::new(bounds.x + (bounds.w - w) / 2, bounds.y + (bounds.h - h) / 2, w, h)
    }

    pub fn handle_event(state: &mut FontPickerState, bounds: Rect, event: GuiEvent) -> (EventResult, DialogResult) {
        if !state.visible {
            return (EventResult::Ignored, DialogResult::None);
        }
        let m = Self::modal_rect(bounds);
        let dec_r = Rect::new(m.x + 14, m.y + 36, 36, 22);
        let inc_r = Rect::new(m.x + 114, m.y + 36, 36, 22);
        let bold_r = Rect::new(m.x + 166, m.y + 36, 128, 22);
        let ct_r = Rect::new(m.x + 14, m.y + 66, 280, 22);
        let ok_r = Rect::new(m.right() - 160, m.bottom() - 32, 70, 22);
        let can_r = Rect::new(m.right() - 82, m.bottom() - 32, 70, 22);

        let mut res = EventResult::Handled;
        let (r1, c1) = Button::handle_event(&mut state.btn_smaller, dec_r, event);
        res.merge(r1);
        if c1 {
            state.size_px = (state.size_px - 1).max(10);
            return (EventResult::NeedsRedraw, DialogResult::None);
        }
        let (r2, c2) = Button::handle_event(&mut state.btn_larger, inc_r, event);
        res.merge(r2);
        if c2 {
            state.size_px = (state.size_px + 1).min(24);
            return (EventResult::NeedsRedraw, DialogResult::None);
        }
        let (r3, t3) = CheckBox::handle_event(&mut state.chk_bold, bold_r, event);
        res.merge(r3);
        if t3 {
            state.bold = state.chk_bold.check.is_checked();
            return (EventResult::NeedsRedraw, DialogResult::None);
        }
        let (r4, t4) = CheckBox::handle_event(&mut state.chk_ct, ct_r, event);
        res.merge(r4);
        if t4 {
            state.cleartype = state.chk_ct.check.is_checked();
            return (EventResult::NeedsRedraw, DialogResult::None);
        }
        let (r5, c5) = Button::handle_event(&mut state.btn_ok, ok_r, event);
        res.merge(r5);
        if c5 {
            state.visible = false;
            return (EventResult::NeedsRedraw, DialogResult::Ok);
        }
        let (r6, c6) = Button::handle_event(&mut state.btn_cancel, can_r, event);
        res.merge(r6);
        if c6 {
            state.visible = false;
            return (EventResult::NeedsRedraw, DialogResult::Cancel);
        }
        (res, DialogResult::None)
    }

    pub fn draw(surface: &mut Surface, bounds: Rect, state: &FontPickerState, theme: &Theme) {
        if !state.visible {
            return;
        }
        surface.fill_rect(bounds, Color::argb(96, 0, 0, 0));
        let m = Self::modal_rect(bounds);
        surface.fill_rect(m, theme.face);
        surface.draw_rect(m, theme.frame_outer);
        let title_r = Rect::new(m.x + 1, m.y + 1, m.w - 2, 24);
        surface.draw_gradient_v(title_r, theme.title_top, theme.title_bot);
        Label::draw(surface, Rect::new(title_r.x + 8, title_r.y, title_r.w - 16, title_r.h), "Font & ClearType Settings", TextStyle { bold: true, color: Color::WHITE, ..TextStyle::ui(Color::WHITE) });

        Button::draw(surface, Rect::new(m.x + 14, m.y + 36, 36, 22), "A-", ButtonStyle::Normal, state.btn_smaller, theme);
        Label::draw(surface, Rect::new(m.x + 56, m.y + 36, 54, 22), &format!("{} px", state.size_px), TextStyle::ui(theme.text_primary));
        Button::draw(surface, Rect::new(m.x + 114, m.y + 36, 36, 22), "A+", ButtonStyle::Normal, state.btn_larger, theme);
        CheckBox::draw(surface, Rect::new(m.x + 166, m.y + 36, 128, 22), "Bold Weight", state.chk_bold, theme);
        CheckBox::draw(surface, Rect::new(m.x + 14, m.y + 66, 280, 22), "SublimeOS ClearType RGB Subpixel AA", state.chk_ct, theme);

        let sample_r = Rect::new(m.x + 14, m.y + 96, m.w - 28, 56);
        surface.fill_rect(sample_r, theme.input_bg);
        surface.draw_rect(sample_r, theme.input_border);
        surface.push_clip(sample_r);
        with_default_font(|font| {
            let prev = font.antialias;
            font.antialias = if state.cleartype { AntialiasMode::ClearTypeRgb } else { AntialiasMode::Grayscale };
            font.draw_text_ellipsis(
                surface,
                sample_r.x + 8,
                sample_r.y + 12,
                sample_r.w - 16,
                "SublimeOS AaBbSs 012345!",
                TextStyle {
                    size_px: state.size_px,
                    bold: state.bold,
                    color: theme.text_primary,
                    ..TextStyle::ui(theme.text_primary)
                },
            );
            font.antialias = prev;
        });
        surface.pop_clip();

        Button::draw(surface, Rect::new(m.right() - 160, m.bottom() - 32, 70, 22), "OK", ButtonStyle::Primary, state.btn_ok, theme);
        Button::draw(surface, Rect::new(m.right() - 82, m.bottom() - 32, 70, 22), "Cancel", ButtonStyle::Normal, state.btn_cancel, theme);
    }
}
