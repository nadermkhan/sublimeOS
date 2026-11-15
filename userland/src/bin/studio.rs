#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use libgui::app::{WindowClient, WindowSpec};
use libgui::component::{
    ColorPickerDialog, ColorPickerState, ComponentContext, FontPickerDialog, FontPickerState,
    ManagedTextInput, SplitOrientation, SplitPane, SplitPaneState, UiComponent,
};
use libgui::event::{EventResult, GuiEvent, KeyCode, MouseButton};
use libgui::font::{with_default_font, AntialiasMode, TextStyle};
use libgui::gfx::{Color, Insets, Point, Rect, Surface, Theme, ThemeKind};
use libgui::layout::{HStack, Length, VStack};
use libgui::widget::{
    Accordion, AccordionState, Button, ButtonState, ButtonStyle, CheckBox, CheckBoxState,
    CheckState, ColumnSpec, ComboBox, ComboBoxState, CommandLink, CommandLinkState, ContextMenu,
    ContextMenuState, DatePickerState, DateTimePicker, DialogResult, GroupBox, HotKeyBox,
    HotKeyState, IpAddressInput, IpAddressState, Label, LinkLabel, LinkLabelState, MenuItemSpec,
    MessageBox, MessageBoxButtons, MessageBoxIcon, MessageBoxState, MonthCalendar,
    MonthCalendarState, MultiLineEdit, MultiLineEditState, NotificationToast, ProgressBar,
    ProgressState, RadioGroup, RadioGroupState, ReportListState, ReportListView, ScrollBar,
    ScrollBarState, ScrollOrientation, Slider, SliderState, SortOrder, SpinBox, SpinBoxState,
    SplitAction, SplitButton, SplitButtonState, StatusBar, StatusPane, TabBar, TabBarState,
    ToastSeverity, ToastState, ToggleState, ToggleSwitch, ToolBar, ToolBarItem,
    ToolBarState, ToolTip, TreeNode, TreeView, TreeViewState,
};
use sublime_rt::{getpid, println, SublimeWmOs};

struct StudioApp {
    pid: usize,
    tabs: TabBarState,
    // Tab 0: Selection & Action Controls
    chk_auto: CheckBoxState,
    chk_tri: CheckBoxState,
    radios: RadioGroupState,
    toggle: ToggleState,
    combo: ComboBoxState,
    spin: SpinBoxState,
    slider: SliderState,
    cmd_link: CommandLinkState,
    split_btn: SplitButtonState,
    link_lbl: LinkLabelState,
    // Tab 1: TreeView, ReportListView & ToolBar
    tb_state: ToolBarState,
    tb_items: Vec<ToolBarItem>,
    tree_state: TreeViewState,
    tree_nodes: Vec<TreeNode>,
    report_state: ReportListState,
    report_cols: Vec<ColumnSpec>,
    report_rows: Vec<[String; 3]>,
    // Tab 2: Pickers, Net & Range
    calendar: MonthCalendarState,
    date_picker: DatePickerState,
    ip_addr: IpAddressState,
    hotkey: HotKeyState,
    accordion: AccordionState,
    v_scroll: ScrollBarState,
    // Tab 3: MultiLineEdit, ContextMenu & Dialogs
    editor: MultiLineEditState,
    quick_input: ManagedTextInput<String>,
    ctx_menu: ContextMenuState,
    msgbox: MessageBoxState,
    toast: ToastState,
    color_dlg: ColorPickerState,
    font_dlg: FontPickerState,
    accent_override: Option<Color>,
    split_pane: SplitPaneState,
    btn_msg_info: ButtonState,
    btn_msg_ask: ButtonState,
    btn_toast: ButtonState,
    btn_ctx: ButtonState,
    btn_color: ButtonState,
    btn_font: ButtonState,
    // Tab 4: Typography
    btn_bold: ButtonState,
    btn_choose_font: ButtonState,
    bold_preview: bool,
    cleartype_preview: bool,
    sample_px: i32,
    // Lifecycle, Damage & Shared Status
    ctx: ComponentContext,
    marquee_timer_id: u32,
    caret_timer_id: u32,
    window_focused: bool,
    status_text: String,
    marquee_phase: i32,
    caret_on: bool,
    last_bounds: Rect,
    hover_pos: Point,
}

impl StudioApp {
    fn new() -> Self {
        let mut tree_state = TreeViewState::new();
        tree_state.selected = Some(1);
        Self {
            pid: getpid(),
            tabs: TabBarState::new(0),
            chk_auto: CheckBoxState::new(true),
            chk_tri: CheckBoxState::tristate(CheckState::Indeterminate),
            radios: RadioGroupState::new(0),
            toggle: ToggleState::new(true),
            combo: ComboBoxState::new(0),
            spin: SpinBoxState::new(64, 0, 256, 8),
            slider: SliderState::new(68, 0, 100),
            cmd_link: CommandLinkState::default(),
            split_btn: SplitButtonState::default(),
            link_lbl: LinkLabelState::default(),
            tb_state: ToolBarState::default(),
            tb_items: vec![
                ToolBarItem::button(1, "New"),
                ToolBarItem::button(2, "Open"),
                ToolBarItem::sep(),
                ToolBarItem::toggle(3, "Grid", true),
                ToolBarItem::toggle(4, "Snap", false),
                ToolBarItem::sep(),
                ToolBarItem::button(5, "Inspect"),
            ],
            tree_state,
            tree_nodes: vec![
                TreeNode::new(1, "SublimeOS", 0, None, true, true),
                TreeNode::new(2, "KiteKernel", 1, Some(0), true, true),
                TreeNode::new(3, "libgui::uxtheme", 2, Some(1), false, false),
                TreeNode::new(4, "libgui::widget", 2, Some(1), false, false),
                TreeNode::new(5, "Ring-3 Services", 1, Some(0), true, false),
                TreeNode::new(6, "/run/wm.sock", 2, Some(4), false, false),
                TreeNode::new(7, "/dev/shm", 2, Some(4), false, false),
            ],
            report_state: ReportListState::new(),
            report_cols: vec![
                ColumnSpec::new("Control", 104),
                ColumnSpec::new("Class", 142),
                ColumnSpec::new("State", 92),
            ],
            report_rows: vec![
                [String::from("CheckBox"), String::from("BS_3STATE"), String::from("Ready")],
                [String::from("ComboBox"), String::from("CBS_DROPDOWNLIST"), String::from("Ready")],
                [String::from("TreeView"), String::from("SysTreeView32"), String::from("Expanded")],
                [String::from("HeaderCtrl"), String::from("SysHeader32"), String::from("Sortable")],
                [String::from("DatePicker"), String::from("SysDateTimePick32"), String::from("Interactive")],
                [String::from("IPAddress"), String::from("SysIPAddress32"), String::from("4-Octet")],
                [String::from("SpinBox"), String::from("msctls_updown32"), String::from("Stepped")],
                [String::from("ToolBar"), String::from("ToolbarWindow32"), String::from("UxThemed")],
            ],
            calendar: MonthCalendarState::default(),
            date_picker: DatePickerState::default(),
            ip_addr: IpAddressState::new([192, 168, 1, 42]),
            hotkey: HotKeyState::default(),
            accordion: AccordionState::new(true),
            v_scroll: ScrollBarState::new(25, 100, 25),
            editor: MultiLineEditState::new(
                "SublimeOS MultiLineEdit (ES_MULTILINE)\nSupports Up/Down/Left/Right arrows, Home/End,\nEnter line split, Backspace join, & right-click menu.",
            ),
            quick_input: ManagedTextInput::new(
                401,
                Rect::new(0, 0, 220, 24),
                "Ctrl+C / Ctrl+V Clipboard Ready",
                "Command / Clipboard filter...",
            )
            .on_change(|st, val, ctx| {
                *st = format!("Input ({} chars): {}", val.len(), val);
                ctx.emit(10);
            })
            .on_submit(|st, val, ctx| {
                ctx.copy_to_clipboard(val);
                *st = format!("Copied to SystemClipboard: {}", val);
            }),
            ctx_menu: ContextMenuState::default(),
            msgbox: MessageBoxState::closed(),
            toast: ToastState::default(),
            color_dlg: ColorPickerState::default(),
            font_dlg: FontPickerState::default(),
            accent_override: None,
            split_pane: SplitPaneState::new(205),
            btn_msg_info: ButtonState::default(),
            btn_msg_ask: ButtonState::default(),
            btn_toast: ButtonState::default(),
            btn_ctx: ButtonState::default(),
            btn_color: ButtonState::default(),
            btn_font: ButtonState::default(),
            btn_bold: ButtonState::default(),
            btn_choose_font: ButtonState::default(),
            bold_preview: false,
            cleartype_preview: true,
            sample_px: 14,
            ctx: ComponentContext::new(),
            marquee_timer_id: 0,
            caret_timer_id: 0,
            window_focused: true,
            status_text: String::from("Ready - UxTheme & ComCtl32 v6 active"),
            marquee_phase: 0,
            caret_on: true,
            last_bounds: Rect::new(0, 0, 620, 420),
            hover_pos: Point::new(0, 0),
        }
    }

    fn sort_report_rows(&mut self, col: usize) {
        let asc = self.report_state.sort_order == SortOrder::Ascending;
        self.report_rows.sort_by(|a, b| {
            let ord = a[col].cmp(&b[col]);
            if asc { ord } else { ord.reverse() }
        });
    }

    fn layout_frame(bounds: Rect) -> (Rect, Rect, Rect, Rect) {
        let mut root = VStack::padded(bounds, Insets::all(6), 4);
        let hdr = root.next(28);
        let tabs = root.next(26);
        let status = root.take_bottom(22);
        let body = root.remaining_rect();
        (hdr, tabs, body, status)
    }

    fn combo_items() -> [&'static str; 5] {
        [
            "UxTheme Standard",
            "High Contrast Dark",
            "Sublime Classic Blue",
            "Sublime Slate Dark",
            "Compact Density",
        ]
    }

    fn radio_items() -> [&'static str; 3] {
        ["Normal (PBST_NORMAL)", "Paused (PBST_PAUSED)", "Error (PBST_ERROR)"]
    }

    fn ctx_items() -> [MenuItemSpec; 5] {
        [
            MenuItemSpec::action(1, "Insert Timestamp", "Ctrl+T"),
            MenuItemSpec::action(2, "Clear Text", "Ctrl+L"),
            MenuItemSpec::sep(),
            MenuItemSpec::action(3, "Show TaskDialog...", "F1"),
            MenuItemSpec::action(4, "Pop Balloon Toast", ""),
        ]
    }

    fn tab0_layout(body: Rect) -> (Rect, Rect, Rect, Rect, Rect, Rect, Rect, Rect, Rect, Rect) {
        let mut h = HStack::padded(body, Insets::all(8), 8);
        let cols = h.allocate(&[Length::Flex(1), Length::Flex(1)]);
        let mut lv = VStack::new(cols[0], 6);
        let grp_sel = lv.next(164);
        let grp_act = lv.remaining_rect();

        let rv = VStack::new(cols[1], 6);
        let grp_val = rv.remaining_rect();

        // Inner rects computed using fixed GroupBox inset (x+10, y+20, w-20, h-28)
        let inner_sel = Rect::new(grp_sel.x + 10, grp_sel.y + 20, (grp_sel.w - 20).max(0), (grp_sel.h - 28).max(0));
        let mut sv = VStack::new(inner_sel, 4);
        let chk1 = sv.next(20);
        let chk2 = sv.next(20);
        let tog = sv.next(22);
        let rad = sv.remaining_rect();

        let inner_act = Rect::new(grp_act.x + 10, grp_act.y + 20, (grp_act.w - 20).max(0), (grp_act.h - 28).max(0));
        let mut av = VStack::new(inner_act, 6);
        let cmd = av.next(44);
        let split = av.next(26);
        let link = av.next(20);

        let inner_val = Rect::new(grp_val.x + 10, grp_val.y + 20, (grp_val.w - 20).max(0), (grp_val.h - 28).max(0));
        let mut vv = VStack::new(inner_val, 8);
        let combo = vv.next(24);
        let spin = vv.next(24);
        let slider = vv.next(24);
        let prog = vv.next(20);
        let marq = vv.next(18);
        let _ = (chk1, chk2, tog, rad, cmd, split, link, combo, spin, slider, prog, marq);
        (grp_sel, grp_act, grp_val, chk1, chk2, tog, rad, cmd, split, link)
    }

    fn tab0_right_controls(grp_val: Rect) -> (Rect, Rect, Rect, Rect, Rect) {
        let inner = Rect::new(grp_val.x + 10, grp_val.y + 20, (grp_val.w - 20).max(0), (grp_val.h - 28).max(0));
        let mut vv = VStack::new(inner, 8);
        let combo = vv.next(24);
        let spin = vv.next(24);
        let slider = vv.next(24);
        let prog = vv.next(20);
        let marq = vv.next(18);
        (combo, spin, slider, prog, marq)
    }

    fn tab2_layout(body: Rect) -> (Rect, Rect, Rect, Rect, Rect, Rect) {
        let mut h = HStack::padded(body, Insets::all(8), 8);
        let cols = h.allocate(&[Length::Px(224), Length::Flex(1), Length::Px(18)]);
        let cal_r = cols[0];
        let vbar_r = cols[2];
        let mut rv = VStack::new(cols[1], 8);
        let picker_r = rv.next(24);
        let ip_r = rv.next(24);
        let hk_r = rv.next(24);
        let acc_r = rv.remaining_rect();
        (cal_r, picker_r, ip_r, hk_r, acc_r, vbar_r)
    }
}

impl WindowClient for StudioApp {
    fn spec(&self) -> WindowSpec {
        WindowSpec {
            title: format!("Widget Studio (Ring-3 PID {})", self.pid),
            width: 620,
            height: 420,
            min_width: 480,
            min_height: 320,
            resizable: true,
        }
    }

    fn on_mount(&mut self, bounds: Rect) {
        self.last_bounds = bounds;
        self.marquee_timer_id = self.ctx.timers.set_interval(120, 0);
        self.caret_timer_id = self.ctx.timers.set_interval(500, 0);
        self.ctx.hotkeys.bind(KeyCode::Function(1), false, false, false, 1);
        self.ctx.hotkeys.bind(KeyCode::Char('k'), true, false, false, 2);
        self.ctx.hotkeys.bind(KeyCode::Char('f'), true, false, false, 3);
        self.quick_input.on_mount(&mut self.status_text, &mut self.ctx);
        self.ctx.focus.register(self.quick_input.id());
        self.ctx.damage.invalidate_all();
    }

    fn on_unmount(&mut self) {
        self.quick_input.on_unmount(&mut self.status_text, &mut self.ctx);
        self.ctx.timers.cancel(self.marquee_timer_id);
        self.ctx.timers.cancel(self.caret_timer_id);
    }

    fn on_theme_changed(&mut self, kind: ThemeKind) {
        self.status_text = format!("ThemeChanged -> {:?}", kind);
        self.ctx.damage.invalidate_all();
    }

    fn on_focus(&mut self, focused: bool) {
        self.window_focused = focused;
        let (hdr, _, _, _) = Self::layout_frame(self.last_bounds);
        self.ctx.damage.invalidate(hdr);
    }

    fn on_clipboard(&mut self, text: &str) {
        self.status_text = format!("Clipboard -> {} chars", text.len());
        let (_, _, _, status_r) = Self::layout_frame(self.last_bounds);
        self.ctx.damage.invalidate(status_r);
    }

    fn take_damage(&mut self, bounds: Rect) -> Option<Rect> {
        self.last_bounds = bounds;
        self.ctx.damage.take_bounding(bounds)
    }

    fn draw(&mut self, surface: &mut Surface, bounds: Rect, theme: &Theme) {
        self.last_bounds = bounds;
        surface.fill_rect(bounds, theme.face);
        let (hdr, tabs, body, status_r) = Self::layout_frame(bounds);

        // Header
        surface.fill_rounded_rect(hdr, 4, theme.face_light);
        surface.draw_rounded_rect(hdr, 4, theme.ctrl_border);
        let mut hr = HStack::padded(hdr, Insets::hv(10, 3), 8);
        let badge = hr.take_right(118);
        Label::draw(
            surface,
            hr.remaining_rect(),
            "Sublime Studio  -  ComCtl32 v6 & UxTheme",
            TextStyle { bold: true, size_px: 14, color: theme.text_primary, ..TextStyle::ui(theme.text_primary) },
        );
        let badge_col = self.accent_override.unwrap_or(theme.selection);
        surface.fill_rounded_rect(badge, 3, badge_col);
        Label::draw(
            surface,
            Rect::new(badge.x + 6, badge.y, (badge.w - 12).max(0), badge.h),
            &format!("PID {} | SHM", self.pid),
            TextStyle::ui_sized(12, theme.selection_text),
        );

        let tab_labels = ["Controls", "Data & Tree", "Pickers", "Edit & Dialogs", "Fonts"];
        TabBar::draw(surface, tabs, &tab_labels, self.tabs, theme);
        surface.fill_rect(body, theme.face);
        surface.draw_rect(body, theme.ctrl_border);

        match self.tabs.active {
            0 => {
                let (grp_sel, grp_act, grp_val, chk1, chk2, tog, rad, cmd, split, link) = Self::tab0_layout(body);
                GroupBox::draw(surface, grp_sel, "Selection (Check / Radio)", None, theme);
                GroupBox::draw(surface, grp_act, "Actions (CommandLink / Split)", None, theme);
                GroupBox::draw(surface, grp_val, "Values & Progress", None, theme);

                CheckBox::draw(surface, chk1, "BS_AUTOCHECKBOX enabled", self.chk_auto, theme);
                CheckBox::draw(surface, chk2, "BS_3STATE indeterminate", self.chk_tri, theme);
                ToggleSwitch::draw(surface, tog, "UxTheme Skin Active", self.toggle, theme);
                RadioGroup::draw(surface, rad, &Self::radio_items(), self.radios, theme);

                CommandLink::draw(surface, cmd, "Launch Inspector TaskDialog", "ComCtl32 v6 BS_COMMANDLINK control", self.cmd_link, theme);
                SplitButton::draw(surface, split, "Apply Profile", self.split_btn, theme);
                LinkLabel::draw(surface, link, "SysLink: Fire Balloon Notification", self.link_lbl, theme);

                let (combo_r, spin_r, slider_r, prog_r, marq_r) = Self::tab0_right_controls(grp_val);
                ComboBox::draw(surface, combo_r, &Self::combo_items(), self.combo, theme);
                SpinBox::draw(surface, spin_r, self.spin, theme);
                Slider::draw(surface, slider_r, self.slider, theme);
                let pstate = match self.radios.selected {
                    1 => ProgressState::Paused,
                    2 => ProgressState::Error,
                    _ => ProgressState::Normal,
                };
                ProgressBar::draw_with_state(surface, prog_r, self.slider.value.clamp(0, 100) as u8, pstate, theme);
                ProgressBar::draw_marquee(surface, marq_r, self.marquee_phase, theme);
                // ComboBox dropdown renders after body so it floats cleanly above controls below
                ComboBox::draw_popup(surface, combo_r, &Self::combo_items(), self.combo, theme);
            }
            1 => {
                let mut v = VStack::padded(body, Insets::all(6), 6);
                let tb_r = v.next(28);
                ToolBar::draw(surface, tb_r, &self.tb_items, self.tb_state, theme);
                let (left_r, bar_r, right_r) =
                    SplitPane::layout(v.remaining_rect(), SplitOrientation::Horizontal, &self.split_pane);
                TreeView::draw(surface, left_r, &self.tree_nodes, &self.tree_state, theme);
                SplitPane::draw_bar(surface, bar_r, SplitOrientation::Horizontal, self.split_pane, theme);
                let rows = &self.report_rows;
                ReportListView::draw(surface, right_r, &self.report_cols, rows.len(), &self.report_state, theme, |r, c| {
                    rows[r][c].clone()
                });
            }
            2 => {
                let (cal_r, picker_r, ip_r, hk_r, acc_r, vbar_r) = Self::tab2_layout(body);
                MonthCalendar::draw(surface, cal_r, self.calendar, theme);
                DateTimePicker::draw(surface, picker_r, self.date_picker, theme);
                IpAddressInput::draw(surface, ip_r, self.ip_addr, theme);
                HotKeyBox::draw(surface, hk_r, &self.hotkey, theme);
                if let Some(acc_body) = Accordion::draw(surface, acc_r, "ScrollBar + Selection Summary", self.accordion, theme) {
                    let info = format!(
                        "Selected Date: {}\nIPv4 Endpoint: {}.{}.{}.{}\nHotKey: {} | Scroll: {}%",
                        self.calendar.selected.format_iso(),
                        self.ip_addr.octets[0],
                        self.ip_addr.octets[1],
                        self.ip_addr.octets[2],
                        self.ip_addr.octets[3],
                        self.hotkey.format_combo(),
                        self.v_scroll.value
                    );
                    Label::draw_multiline(surface, acc_body, &info, TextStyle::ui_sized(12, theme.text_primary));
                }
                ScrollBar::draw(surface, vbar_r, ScrollOrientation::Vertical, self.v_scroll, theme);
                DateTimePicker::draw_popup(surface, picker_r, self.date_picker, theme);
            }
            3 => {
                let mut v = VStack::padded(body, Insets::all(8), 6);
                let btn_row = v.next(26);
                let input_r = v.next(24);
                let ed_r = v.remaining_rect();
                let mut bh = HStack::new(btn_row, 5);
                let b1 = bh.next(82);
                let b2 = bh.next(98);
                let b3 = bh.next(78);
                let b4 = bh.next(92);
                let b5 = bh.next(88);
                let b6 = bh.next(88);
                Button::draw(surface, b1, "Info Box", ButtonStyle::Normal, self.btn_msg_info, theme);
                Button::draw(surface, b2, "Yes/No/Cancel", ButtonStyle::Primary, self.btn_msg_ask, theme);
                Button::draw(surface, b3, "Toast", ButtonStyle::Normal, self.btn_toast, theme);
                Button::draw(surface, b4, "Popup Menu", ButtonStyle::Normal, self.btn_ctx, theme);
                Button::draw(surface, b5, "Color... ^K", ButtonStyle::Normal, self.btn_color, theme);
                Button::draw(surface, b6, "Font... ^F", ButtonStyle::Normal, self.btn_font, theme);
                self.quick_input.set_bounds(input_r);
                let input_focused = self.ctx.focus.is_focused(self.quick_input.id());
                self.quick_input.draw(surface, theme, input_focused);
                MultiLineEdit::draw(surface, ed_r, &mut self.editor, self.caret_on, theme);

                if self.btn_msg_ask.hovered {
                    ToolTip::draw(surface, bounds, self.hover_pos, "Opens Win32 TaskDialog with Yes/No/Cancel", theme);
                }
            }
            _ => {
                let mut v = VStack::padded(body, Insets::all(12), 8);
                let bar = v.next(26);
                let mut bh = HStack::new(bar, 8);
                let btn_r = bh.next(118);
                let btn_f = bh.next(118);
                Button::draw(
                    surface,
                    btn_r,
                    if self.bold_preview { "Weight: Bold" } else { "Weight: Regular" },
                    ButtonStyle::Primary,
                    self.btn_bold,
                    theme,
                );
                Button::draw(surface, btn_f, "ChooseFont...", ButtonStyle::Normal, self.btn_choose_font, theme);
                Label::draw(
                    surface,
                    bh.remaining_rect(),
                    if self.cleartype_preview {
                        "ClearType LCD RGB Subpixel AA + Light Stem Hinting"
                    } else {
                        "Grayscale TTF Winding Rasterizer"
                    },
                    TextStyle::ui(theme.text_secondary),
                );
                for &sz in &[13i32, self.sample_px.max(14), 20, 26] {
                    let row = v.next(sz + 10);
                    surface.push_clip(row);
                    with_default_font(|font| {
                        let prev_aa = font.antialias;
                        font.antialias = if self.cleartype_preview {
                            AntialiasMode::ClearTypeRgb
                        } else {
                            AntialiasMode::Grayscale
                        };
                        font.draw_text_ellipsis(
                            surface,
                            row.x,
                            row.y + 2,
                            row.w,
                            &format!("{}px: SublimeOS Ss AaEe012345 ClearType", sz),
                            TextStyle {
                                size_px: sz,
                                bold: self.bold_preview,
                                color: theme.text_primary,
                                ..TextStyle::ui_sized(sz, theme.text_primary)
                            },
                        );
                        font.antialias = prev_aa;
                    });
                    surface.pop_clip();
                }
            }
        }

        // Overlays above all tabs: ContextMenu, NotificationToast, MessageBox, Common Dialogs
        ContextMenu::draw(surface, bounds, &Self::ctx_items(), self.ctx_menu, theme);
        NotificationToast::draw(surface, bounds, &self.toast, theme);
        MessageBox::draw(surface, bounds, &self.msgbox, theme);
        ColorPickerDialog::draw(surface, bounds, &self.color_dlg, theme);
        FontPickerDialog::draw(surface, bounds, &self.font_dlg, theme);

        let date_str = self.calendar.selected.format_iso();
        StatusBar::draw(
            surface,
            status_r,
            &[
                StatusPane { text: &self.status_text, width: Length::Flex(1) },
                StatusPane { text: &date_str, width: Length::Px(92) },
                StatusPane { text: "SHM 32bpp", width: Length::Px(80) },
            ],
            theme,
        );
    }

    fn on_event(&mut self, event: GuiEvent, bounds: Rect) -> EventResult {
        self.last_bounds = bounds;
        if let GuiEvent::MouseMove { x, y } = event {
            self.hover_pos = Point::new(x, y);
        }
        if let GuiEvent::Tick { now_ms } = event {
            let mut dirty = false;
            if self.toast.tick() {
                self.ctx.damage.invalidate(Rect::new(bounds.right() - 300, bounds.y, 300, 140));
                dirty = true;
            }
            for tid in self.ctx.timers.tick(now_ms) {
                if tid == self.marquee_timer_id && self.tabs.active == 0 {
                    self.marquee_phase = self.marquee_phase.wrapping_add(8);
                    let (_, _, body, _) = Self::layout_frame(bounds);
                    let (_, _, grp_val, ..) = Self::tab0_layout(body);
                    let (_, _, _, _, marq_r) = Self::tab0_right_controls(grp_val);
                    self.ctx.damage.invalidate(marq_r);
                    dirty = true;
                } else if tid == self.caret_timer_id && self.tabs.active == 3 {
                    self.caret_on = !self.caret_on;
                    let (_, _, body, _) = Self::layout_frame(bounds);
                    let mut v = VStack::padded(body, Insets::all(8), 6);
                    let _ = v.next(26);
                    let input_r = v.next(24);
                    let ed_r = v.remaining_rect();
                    self.ctx.damage.invalidate(input_r);
                    self.ctx.damage.invalidate(ed_r);
                    dirty = true;
                }
            }
            return if dirty { EventResult::NeedsRedraw } else { EventResult::Ignored };
        }

        // System-wide hotkeys (F1, Ctrl+K, Ctrl+F)
        if let Some(cmd) = self.ctx.hotkeys.match_event(event) {
            match cmd {
                1 => self.msgbox.open(
                    "Hotkey Inspector (F1)",
                    "HotkeyRegistry dispatched CommandId 1.",
                    "Win32 accelerator table & ComponentContext live.",
                    MessageBoxIcon::Info,
                    MessageBoxButtons::Ok,
                ),
                2 => self.color_dlg.visible = true,
                3 => {
                    self.font_dlg.size_px = self.sample_px;
                    self.font_dlg.bold = self.bold_preview;
                    self.font_dlg.cleartype = self.cleartype_preview;
                    self.font_dlg.visible = true;
                }
                _ => {}
            }
            self.ctx.damage.invalidate(bounds);
            return EventResult::NeedsRedraw;
        }

        // Modal ColorPickerDialog & FontPickerDialog capture input when open
        if self.color_dlg.visible {
            let (cr, dres) = ColorPickerDialog::handle_event(&mut self.color_dlg, bounds, event);
            if dres == DialogResult::Ok {
                self.accent_override = Some(self.color_dlg.selected);
                self.status_text = format!("Accent color -> #{:06X}", self.color_dlg.selected.to_u32() & 0xFFFFFF);
            }
            if cr.needs_redraw() {
                self.ctx.damage.invalidate(bounds);
            }
            return cr;
        }
        if self.font_dlg.visible {
            let (fr, dres) = FontPickerDialog::handle_event(&mut self.font_dlg, bounds, event);
            if dres == DialogResult::Ok {
                self.sample_px = self.font_dlg.size_px;
                self.bold_preview = self.font_dlg.bold;
                self.cleartype_preview = self.font_dlg.cleartype;
                self.status_text = format!(
                    "Font -> {}px bold={} {}",
                    self.sample_px,
                    self.bold_preview,
                    if self.cleartype_preview { "ClearType" } else { "Grayscale" }
                );
            }
            if fr.needs_redraw() {
                self.ctx.damage.invalidate(bounds);
            }
            return fr;
        }

        // Modal MessageBox captures all input first when open
        if self.msgbox.visible {
            let (mr, dres) = MessageBox::handle_event(&mut self.msgbox, bounds, event);
            if dres != DialogResult::None {
                self.status_text = format!("DialogResult::{:?}", dres);
            }
            if mr.needs_redraw() {
                self.ctx.damage.invalidate(bounds);
            }
            return mr;
        }

        // ContextMenu popup captures input when open
        if self.ctx_menu.open {
            let (cr, cid) = ContextMenu::handle_event(&mut self.ctx_menu, bounds, &Self::ctx_items(), event);
            if let Some(id) = cid {
                match id {
                    1 => self.editor.lines.push(format!("[{}] Event appended", self.calendar.selected.format_iso())),
                    2 => {
                        self.editor.lines = vec![String::new()];
                        self.editor.cursor_row = 0;
                        self.editor.cursor_col = 0;
                    }
                    3 => self.msgbox.open("Context TaskDialog", "Launched from TrackPopupMenu.", "Win32 context menu command ID 3.", MessageBoxIcon::Info, MessageBoxButtons::Ok),
                    _ => self.toast.show("SublimeOS Notify", "Context menu balloon triggered.", ToastSeverity::Success, 160),
                }
                self.status_text = format!("Context menu item {} invoked", id);
            }
            if cr.needs_redraw() {
                self.ctx.damage.invalidate(bounds);
            }
            return cr;
        }

        if let GuiEvent::MouseDown { x, y, button: MouseButton::Right } = event {
            self.ctx_menu.show(x, y);
            self.ctx.damage.invalidate(bounds);
            return EventResult::NeedsRedraw;
        }

        let (_, tabs, body, _) = Self::layout_frame(bounds);
        let tab_labels = ["Controls", "Data & Tree", "Pickers", "Edit & Dialogs", "Fonts"];
        let (mut res, switched) = TabBar::handle_event(&mut self.tabs, tabs, &tab_labels, event);
        if let Some(t) = switched {
            self.status_text = format!("Switched to tab '{}'", tab_labels[t]);
            self.ctx.damage.invalidate(bounds);
            return EventResult::NeedsRedraw;
        }

        match self.tabs.active {
            0 => {
                let (_, _, grp_val, chk1, chk2, tog, rad, cmd, split, link) = Self::tab0_layout(body);
                let (combo_r, spin_r, slider_r, _, _) = Self::tab0_right_controls(grp_val);

                // Open ComboBox popup gets priority over controls beneath it
                let was_open = self.combo.open;
                let (cr, csel) = ComboBox::handle_event(&mut self.combo, combo_r, &Self::combo_items(), event);
                res.merge(cr);
                if let Some(idx) = csel {
                    self.status_text = format!("ComboBox -> {}", Self::combo_items()[idx]);
                }
                if was_open {
                    if res.needs_redraw() {
                        self.ctx.damage.invalidate(bounds);
                    }
                    return res;
                }

                let (r1, t1) = CheckBox::handle_event(&mut self.chk_auto, chk1, event);
                res.merge(r1);
                if t1 {
                    self.status_text = format!("CheckBox 1 -> {:?}", self.chk_auto.check);
                }
                let (r2, t2) = CheckBox::handle_event(&mut self.chk_tri, chk2, event);
                res.merge(r2);
                if t2 {
                    self.status_text = format!("Tristate CheckBox -> {:?}", self.chk_tri.check);
                }
                let (rt, tt) = ToggleSwitch::handle_event(&mut self.toggle, tog, event);
                res.merge(rt);
                if tt {
                    self.status_text = format!("ToggleSwitch -> {}", if self.toggle.on { "ON" } else { "OFF" });
                }
                let (rr, rsel) = RadioGroup::handle_event(&mut self.radios, rad, &Self::radio_items(), event);
                res.merge(rr);
                if let Some(idx) = rsel {
                    self.status_text = format!("Progress State -> {}", Self::radio_items()[idx]);
                }
                let (rc, cclick) = CommandLink::handle_event(&mut self.cmd_link, cmd, event);
                res.merge(rc);
                if cclick {
                    self.msgbox.open(
                        "UxTheme Inspector",
                        "ComCtl32 CommandLink invoked TaskDialog.",
                        "All controls render in Ring-3 SHM via libgui.",
                        MessageBoxIcon::Info,
                        MessageBoxButtons::OkCancel,
                    );
                }
                let (rsb, sact) = SplitButton::handle_event(&mut self.split_btn, split, event);
                res.merge(rsb);
                match sact {
                    SplitAction::Primary => self.status_text = String::from("SplitButton: Primary Apply"),
                    SplitAction::DropDown => {
                        self.ctx_menu.show(split.x, split.bottom());
                        res = EventResult::NeedsRedraw;
                    }
                    SplitAction::None => {}
                }
                let (rl, lclick) = LinkLabel::handle_event(&mut self.link_lbl, link, event);
                res.merge(rl);
                if lclick {
                    self.toast.show("SysLink Activated", "Win32 Balloon Notification Toast online.", ToastSeverity::Info, 180);
                }
                let (rsp, sp_ch) = SpinBox::handle_event(&mut self.spin, spin_r, event);
                res.merge(rsp);
                if sp_ch {
                    self.status_text = format!("SpinBox value -> {}", self.spin.value);
                }
                let (rsl, sl_ch) = Slider::handle_event(&mut self.slider, slider_r, event);
                res.merge(rsl);
                if sl_ch {
                    self.status_text = format!("Slider -> {}%", self.slider.value);
                }
            }
            1 => {
                let mut v = VStack::padded(body, Insets::all(6), 6);
                let tb_r = v.next(28);
                let rem = v.remaining_rect();
                let spr = SplitPane::handle_event(&mut self.split_pane, rem, SplitOrientation::Horizontal, event);
                res.merge(spr);
                let (left_r, _, right_r) =
                    SplitPane::layout(rem, SplitOrientation::Horizontal, &self.split_pane);

                let (tbr, tbid) = ToolBar::handle_event(&mut self.tb_state, tb_r, &mut self.tb_items, event);
                res.merge(tbr);
                if let Some(id) = tbid {
                    self.status_text = format!("ToolBar button {} pressed", id);
                    if id == 3 {
                        self.tree_state.show_lines = !self.tree_state.show_lines;
                    }
                }
                let (trr, tsel) = TreeView::handle_event(&mut self.tree_state, left_r, &mut self.tree_nodes, event);
                res.merge(trr);
                if let Some(idx) = tsel {
                    self.status_text = format!("TreeView node: {}", self.tree_nodes[idx].label);
                }
                let (rlr, rrow, rsort) = ReportListView::handle_event(
                    &mut self.report_state,
                    right_r,
                    &mut self.report_cols,
                    self.report_rows.len(),
                    event,
                );
                res.merge(rlr);
                if let Some(c) = rsort {
                    self.sort_report_rows(c);
                    self.status_text = format!("Sorted by column {}", self.report_cols[c].title);
                } else if let Some(r) = rrow {
                    self.status_text = format!("Selected row: {}", self.report_rows[r][0]);
                }
            }
            2 => {
                let (cal_r, picker_r, ip_r, hk_r, acc_r, vbar_r) = Self::tab2_layout(body);
                let was_open = self.date_picker.open;
                let (dpr, dpicked) = DateTimePicker::handle_event(&mut self.date_picker, picker_r, event);
                res.merge(dpr);
                if let Some(d) = dpicked {
                    self.calendar.selected = d;
                    self.status_text = format!("Picked Date: {}", d.format_iso());
                }
                if was_open {
                    if res.needs_redraw() {
                        self.ctx.damage.invalidate(bounds);
                    }
                    return res;
                }
                let (cr, cd) = MonthCalendar::handle_event(&mut self.calendar, cal_r, event);
                res.merge(cr);
                if let Some(d) = cd {
                    self.date_picker.calendar.selected = d;
                    self.status_text = format!("Calendar Date: {}", d.format_iso());
                }
                let (ir, ich) = IpAddressInput::handle_event(&mut self.ip_addr, ip_r, event);
                res.merge(ir);
                if ich {
                    self.status_text = format!(
                        "IPv4 -> {}.{}.{}.{}",
                        self.ip_addr.octets[0], self.ip_addr.octets[1], self.ip_addr.octets[2], self.ip_addr.octets[3]
                    );
                }
                let (hkr, hkch) = HotKeyBox::handle_event(&mut self.hotkey, hk_r, event);
                res.merge(hkr);
                if hkch {
                    self.status_text = format!("HotKey -> {}", self.hotkey.format_combo());
                }
                let (ar, _) = Accordion::handle_event(&mut self.accordion, acc_r, event);
                res.merge(ar);
                let (sbr, sbch) = ScrollBar::handle_event(&mut self.v_scroll, vbar_r, ScrollOrientation::Vertical, event);
                res.merge(sbr);
                if sbch {
                    self.status_text = format!("ScrollBar -> {}/{}", self.v_scroll.value, self.v_scroll.max);
                }
            }
            3 => {
                let mut v = VStack::padded(body, Insets::all(8), 6);
                let btn_row = v.next(26);
                let input_r = v.next(24);
                let ed_r = v.remaining_rect();
                let mut bh = HStack::new(btn_row, 5);
                let b1 = bh.next(82);
                let b2 = bh.next(98);
                let b3 = bh.next(78);
                let b4 = bh.next(92);
                let b5 = bh.next(88);
                let b6 = bh.next(88);

                let (r1, c1) = Button::handle_event(&mut self.btn_msg_info, b1, event);
                res.merge(r1);
                if c1 {
                    self.msgbox.open("SublimeOS Info", "Standard Win32 MessageBox overlay.", "Rendered cleanly inside Ring-3 SHM bounds.", MessageBoxIcon::Info, MessageBoxButtons::Ok);
                }
                let (r2, c2) = Button::handle_event(&mut self.btn_msg_ask, b2, event);
                res.merge(r2);
                if c2 {
                    self.msgbox.open("Confirm Changes", "Save current UxTheme layout settings?", "Choose Yes, No, or Cancel to test dialog result.", MessageBoxIcon::Question, MessageBoxButtons::YesNoCancel);
                }
                let (r3, c3) = Button::handle_event(&mut self.btn_toast, b3, event);
                res.merge(r3);
                if c3 {
                    self.toast.show("NotificationToast", "NIIF_INFO balloon fades out automatically.", ToastSeverity::Warning, 180);
                }
                let (r4, c4) = Button::handle_event(&mut self.btn_ctx, b4, event);
                res.merge(r4);
                if c4 {
                    self.ctx_menu.show(b4.x, b4.bottom());
                    res = EventResult::NeedsRedraw;
                }
                let (r5, c5) = Button::handle_event(&mut self.btn_color, b5, event);
                res.merge(r5);
                if c5 {
                    self.color_dlg.visible = true;
                    res = EventResult::NeedsRedraw;
                }
                let (r6, c6) = Button::handle_event(&mut self.btn_font, b6, event);
                res.merge(r6);
                if c6 {
                    self.font_dlg.size_px = self.sample_px;
                    self.font_dlg.bold = self.bold_preview;
                    self.font_dlg.cleartype = self.cleartype_preview;
                    self.font_dlg.visible = true;
                    res = EventResult::NeedsRedraw;
                }

                self.quick_input.set_bounds(input_r);
                let qr = self.quick_input.on_event(event, &mut self.status_text, &mut self.ctx);
                res.merge(qr);
                if let GuiEvent::MouseDown { x, y, .. } = event {
                    if !input_r.contains(x, y) && self.ctx.focus.is_focused(self.quick_input.id()) {
                        self.ctx.focus.set_focus(None);
                    }
                }
                if !self.ctx.focus.is_focused(self.quick_input.id()) {
                    let (er, _) = MultiLineEdit::handle_event(&mut self.editor, ed_r, event);
                    res.merge(er);
                } else if let GuiEvent::MouseDown { x, y, .. } = event {
                    if ed_r.contains(x, y) {
                        let (er, _) = MultiLineEdit::handle_event(&mut self.editor, ed_r, event);
                        res.merge(er);
                    }
                }
            }
            _ => {
                let mut v = VStack::padded(body, Insets::all(12), 8);
                let bar = v.next(26);
                let mut bh = HStack::new(bar, 8);
                let btn_r = bh.next(118);
                let btn_f = bh.next(118);
                let (br, clicked) = Button::handle_event(&mut self.btn_bold, btn_r, event);
                res.merge(br);
                if clicked {
                    self.bold_preview = !self.bold_preview;
                    res = EventResult::NeedsRedraw;
                }
                let (fr, fclicked) = Button::handle_event(&mut self.btn_choose_font, btn_f, event);
                res.merge(fr);
                if fclicked {
                    self.font_dlg.size_px = self.sample_px;
                    self.font_dlg.bold = self.bold_preview;
                    self.font_dlg.cleartype = self.cleartype_preview;
                    self.font_dlg.visible = true;
                    res = EventResult::NeedsRedraw;
                }
            }
        }

        if res.needs_redraw() {
            self.ctx.damage.invalidate(bounds);
        }
        res
    }
}

#[no_mangle]
pub fn main() -> i32 {
    let mut os = SublimeWmOs;
    let mut app = StudioApp::new();
    match libgui::ipc::run_app(&mut app, &mut os) {
        Ok(()) => 0,
        Err(e) => {
            println!("[studio] compositor IPC error: {}", e);
            1
        }
    }
}
