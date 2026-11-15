use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use libgui::event::{GuiEvent, MouseButton};
use libgui::gfx::Rect;
use libgui::widget::{MenuBar, MenuBarState, MenuItemSpec, MenuSpec, ScrollBar, ScrollBarState, ScrollOrientation};
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::font::{
    draw_ui_string, draw_ui_string_bold, draw_ui_string_centered_v, draw_ui_string_ellipsis,
    measure_ui_string,
};
use crate::mm::pmm::PMM;
use crate::net::device::NET_MANAGER;
use crate::net::wifi::WIFI_MANAGER;

const SYSMON_FILE_ITEMS: &[MenuItemSpec] = &[
    MenuItemSpec::action(1, "New Task (Run)...", "Win+R"),
    MenuItemSpec::sep(),
    MenuItemSpec::action(2, "Exit Task Manager", "Alt+F4"),
];

const SYSMON_OPT_ITEMS: &[MenuItemSpec] = &[
    MenuItemSpec::action(10, "Switch View Tab", ""),
    MenuItemSpec::action(11, "Refresh Now", "F5"),
];

const SYSMON_VIEW_ITEMS: &[MenuItemSpec] = &[
    MenuItemSpec::action(20, "Applications", ""),
    MenuItemSpec::action(21, "Processes", ""),
    MenuItemSpec::action(22, "Performance", ""),
    MenuItemSpec::action(23, "Networking", ""),
];

const SYSMON_HELP_ITEMS: &[MenuItemSpec] = &[
    MenuItemSpec::action(30, "About SublimeOS", ""),
];

const SYSMON_MENUS: &[MenuSpec] = &[
    MenuSpec { title: "File", items: SYSMON_FILE_ITEMS },
    MenuSpec { title: "Options", items: SYSMON_OPT_ITEMS },
    MenuSpec { title: "View", items: SYSMON_VIEW_ITEMS },
    MenuSpec { title: "Help", items: SYSMON_HELP_ITEMS },
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SysmonTab {
    Applications,
    Processes,
    Performance,
    Networking,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SysmonAction {
    None,
    SwitchToWindow(u32),
    CloseWindow(u32),
    KillPid(u32),
    CloseSelf,
    OpenSearch,
    OpenAbout,
}

pub struct SysmonApp {
    pub active_tab: SysmonTab,
    pub selected_row: usize,
    pub scroll_row: usize,
    pub vbar: ScrollBarState,
    pub window_list: Vec<(u32, String, bool)>,
    pub pending_action: SysmonAction,
    pub menu_state: MenuBarState,
    pub context_menu: Option<(i32, i32)>,
    pub ctx_hover: Option<usize>,
}

impl SysmonApp {
    pub fn new() -> Self {
        Self {
            active_tab: SysmonTab::Processes,
            selected_row: 0,
            scroll_row: 0,
            vbar: ScrollBarState::new(0, 0, 8),
            window_list: Vec::new(),
            pending_action: SysmonAction::None,
            menu_state: MenuBarState::default(),
            context_menu: None,
            ctx_hover: None,
        }
    }

    const SB_W: i32 = 14;

    fn table_metrics(&self, x: i32, y: i32, w: i32, h: i32) -> Option<(Rect, usize, usize, i32)> {
        let content_w = (w - 20).max(200);
        let body_y = y + 48;
        let body_h = (h - 74).max(120);
        let tbl_x = x + 20;
        let tbl_y = body_y + 10;
        let tbl_w = (content_w - 20).max(160);
        let (tbl_h, total_rows, row_h) = match self.active_tab {
            SysmonTab::Applications => ((body_h - 46).max(60), self.window_list.len(), 20),
            SysmonTab::Processes => ((body_h - 118).max(60), crate::task::scheduler::get_task_list_detailed().len(), 19),
            SysmonTab::Networking => ((body_h - 72).max(60), NET_MANAGER.lock().devices.len(), 19),
            SysmonTab::Performance => return None,
        };
        let list_h = (tbl_h - 24).max(row_h);
        let vis_rows = (list_h / row_h).max(1) as usize;
        let sb_rect = Rect::new(
            tbl_x + tbl_w - Self::SB_W - 1,
            tbl_y + 22,
            Self::SB_W,
            (tbl_h - 23).max(12),
        );
        Some((sb_rect, vis_rows, total_rows, row_h))
    }

    pub fn handle_wheel(&mut self, delta_y: i32) -> bool {
        if delta_y == 0 || self.active_tab == SysmonTab::Performance {
            return false;
        }
        let total = match self.active_tab {
            SysmonTab::Applications => self.window_list.len(),
            SysmonTab::Processes => crate::task::scheduler::get_task_list_detailed().len(),
            SysmonTab::Networking => NET_MANAGER.lock().devices.len(),
            SysmonTab::Performance => 0,
        };
        let max_scroll = total.saturating_sub(3);
        let prev = self.scroll_row;
        if delta_y > 0 {
            self.scroll_row = self.scroll_row.saturating_sub(delta_y as usize * 2);
        } else {
            self.scroll_row = (self.scroll_row + (-delta_y as usize) * 2).min(max_scroll);
        }
        self.vbar.sync_range(self.scroll_row as i32, max_scroll as i32, 6);
        self.scroll_row != prev
    }

    pub fn sync_windows(&mut self, wins: Vec<(u32, String, bool)>) {
        self.window_list = wins;
        if self.active_tab == SysmonTab::Applications && self.selected_row >= self.window_list.len() {
            self.selected_row = self.window_list.len().saturating_sub(1);
        }
    }

    pub fn take_action(&mut self) -> SysmonAction {
        let act = self.pending_action;
        self.pending_action = SysmonAction::None;
        act
    }

    fn apply_menu_cmd(&mut self, cmd: u16) {
        match cmd {
            1 => self.pending_action = SysmonAction::OpenSearch,
            2 => self.pending_action = SysmonAction::CloseSelf,
            10 => {
                self.active_tab = match self.active_tab {
                    SysmonTab::Applications => SysmonTab::Processes,
                    SysmonTab::Processes => SysmonTab::Performance,
                    SysmonTab::Performance => SysmonTab::Networking,
                    SysmonTab::Networking => SysmonTab::Applications,
                };
                self.selected_row = 0;
            }
            11 => {}
            20 => {
                self.active_tab = SysmonTab::Applications;
                self.selected_row = 0;
            }
            21 => {
                self.active_tab = SysmonTab::Processes;
                self.selected_row = 0;
            }
            22 => {
                self.active_tab = SysmonTab::Performance;
                self.selected_row = 0;
            }
            23 => {
                self.active_tab = SysmonTab::Networking;
                self.selected_row = 0;
            }
            30 => self.pending_action = SysmonAction::OpenAbout,
            _ => {}
        }
    }

    fn handle_vbar_event(&mut self, x: i32, y: i32, w: i32, h: i32, event: GuiEvent) -> (bool, bool) {
        let Some((sb_rect, vis_rows, total_rows, _)) = self.table_metrics(x, y, w, h) else {
            return (false, false);
        };
        let max_scroll = total_rows.saturating_sub(vis_rows);
        self.scroll_row = self.scroll_row.min(max_scroll);
        self.vbar.sync_range(self.scroll_row as i32, max_scroll as i32, vis_rows as i32);
        let in_bar = match event {
            GuiEvent::MouseDown { x: px, y: py, .. } => sb_rect.contains(px, py),
            _ => self.vbar.dragging || sb_rect.contains(-1, -1),
        };
        let (res, changed) =
            ScrollBar::handle_event(&mut self.vbar, sb_rect, ScrollOrientation::Vertical, event);
        if changed {
            self.scroll_row = (self.vbar.value.max(0) as usize).min(max_scroll);
        }
        (changed || res.needs_redraw(), in_bar || changed)
    }

    pub fn handle_mouse_move(&mut self, px: i32, py: i32, x: i32, y: i32, w: i32, h: i32) -> bool {
        let mut changed = false;
        if let Some((cx, cy)) = self.context_menu {
            let next = if px >= cx && px < cx + 168 && py >= cy + 4 && py < cy + 4 + 4 * 22 {
                Some(((py - (cy + 4)) / 22) as usize)
            } else {
                None
            };
            if next != self.ctx_hover {
                self.ctx_hover = next;
                changed = true;
            }
        }
        let menu_bar = Rect::new(x, y, w, 20);
        let (res, _) = MenuBar::handle_event(
            &mut self.menu_state,
            menu_bar,
            SYSMON_MENUS,
            GuiEvent::MouseMove { x: px, y: py },
        );
        let (sb_redraw, _) = self.handle_vbar_event(x, y, w, h, GuiEvent::MouseMove { x: px, y: py });
        changed || res.needs_redraw() || sb_redraw
    }

    pub fn handle_mouse_up(&mut self, px: i32, py: i32, x: i32, y: i32, w: i32, h: i32) -> bool {
        let (sb_redraw, _) = self.handle_vbar_event(
            x,
            y,
            w,
            h,
            GuiEvent::MouseUp {
                x: px,
                y: py,
                button: MouseButton::Left,
            },
        );
        sb_redraw
    }

    pub fn handle_right_click(&mut self, px: i32, py: i32, x: i32, y: i32, w: i32, h: i32) -> bool {
        self.menu_state.open_menu = None;
        let mw = 168;
        let mh = 96;
        let cx = px.min((x + w - mw - 4).max(x)).max(x);
        let cy = py.min((y + h - mh - 4).max(y)).max(y);
        self.context_menu = Some((cx, cy));
        self.ctx_hover = None;
        true
    }

    pub fn handle_click(&mut self, px: i32, py: i32, x: i32, y: i32, w: i32, h: i32) {
        if let Some((cx, cy)) = self.context_menu.take() {
            let mw = 168;
            let mh = 96;
            if px >= cx && px <= cx + mw && py >= cy && py <= cy + mh {
                let row = (py - (cy + 4)) / 22;
                match row {
                    0 => {
                        if self.active_tab == SysmonTab::Applications {
                            if let Some((win_id, _, _)) = self.window_list.get(self.selected_row) {
                                self.pending_action = SysmonAction::SwitchToWindow(*win_id);
                            }
                        } else {
                            self.active_tab = SysmonTab::Performance;
                        }
                    }
                    1 => {
                        if self.active_tab == SysmonTab::Applications {
                            if let Some((win_id, _, _)) = self.window_list.get(self.selected_row) {
                                self.pending_action = SysmonAction::CloseWindow(*win_id);
                            }
                        } else {
                            self.pending_action = SysmonAction::OpenSearch;
                        }
                    }
                    2 => {
                        self.active_tab = match self.active_tab {
                            SysmonTab::Applications => SysmonTab::Processes,
                            SysmonTab::Processes => SysmonTab::Performance,
                            SysmonTab::Performance => SysmonTab::Networking,
                            SysmonTab::Networking => SysmonTab::Applications,
                        };
                        self.selected_row = 0;
                    }
                    _ => self.pending_action = SysmonAction::OpenAbout,
                }
                return;
            }
        }

        let menu_bar = Rect::new(x, y, w, 20);
        let (mres, m_act) = MenuBar::handle_event(
            &mut self.menu_state,
            menu_bar,
            SYSMON_MENUS,
            GuiEvent::MouseDown {
                x: px,
                y: py,
                button: MouseButton::Left,
            },
        );
        if let Some(cmd) = m_act {
            self.apply_menu_cmd(cmd);
            return;
        }
        if mres.needs_redraw() {
            return;
        }

        // 1. Tabs at y + 26
        let tab_y = y + 26;
        let tabs = [
            ("Applications", SysmonTab::Applications),
            ("Processes", SysmonTab::Processes),
            ("Performance", SysmonTab::Performance),
            ("Networking", SysmonTab::Networking),
        ];

        let mut tab_x = x + 10;
        for (label, tab) in tabs.iter() {
            let tw = measure_ui_string(label) + 18;
            if px >= tab_x && px <= tab_x + tw && py >= tab_y && py <= tab_y + 22 {
                self.active_tab = *tab;
                self.selected_row = 0;
                self.scroll_row = 0;
                self.vbar.sync_range(0, 0, 8);
                return;
            }
            tab_x += tw + 3;
        }

        let (_, consumed_sb) = self.handle_vbar_event(
            x,
            y,
            w,
            h,
            GuiEvent::MouseDown {
                x: px,
                y: py,
                button: MouseButton::Left,
            },
        );
        if consumed_sb {
            return;
        }

        let content_x = x + 10;
        let content_w = (w - 20).max(200);
        let body_y = tab_y + 22;
        let body_h = (h - 74).max(120);
        let tbl_x = content_x + 10;
        let tbl_y = body_y + 10;
        let tbl_w = (content_w - 20).max(160);
        let row_hit_w = tbl_w - Self::SB_W;

        match self.active_tab {
            SysmonTab::Applications => {
                let tbl_h = (body_h - 46).max(60);
                let mut ry = tbl_y + 26;
                for idx in self.scroll_row..self.window_list.len() {
                    if ry + 18 > tbl_y + tbl_h {
                        break;
                    }
                    if px >= tbl_x && px <= tbl_x + row_hit_w && py >= ry && py < ry + 20 {
                        self.selected_row = idx;
                        return;
                    }
                    ry += 20;
                }
                // Bottom action buttons: [Switch To] [End Task]
                let btn_y = y + h - 54;
                if py >= btn_y && py <= btn_y + 22 {
                    if let Some((win_id, _, _)) = self.window_list.get(self.selected_row) {
                        if px >= tbl_x + tbl_w - 196 && px <= tbl_x + tbl_w - 102 {
                            self.pending_action = SysmonAction::SwitchToWindow(*win_id);
                        } else if px >= tbl_x + tbl_w - 94 && px <= tbl_x + tbl_w {
                            self.pending_action = SysmonAction::CloseWindow(*win_id);
                        }
                    }
                }
            }
            SysmonTab::Processes => {
                let tbl_h = (body_h - 118).max(60);
                let mut ry = tbl_y + 25;
                let mut idx = self.scroll_row;
                while ry + 18 <= tbl_y + tbl_h {
                    if px >= tbl_x && px <= tbl_x + row_hit_w && py >= ry && py < ry + 19 {
                        self.selected_row = idx;
                        return;
                    }
                    ry += 19;
                    idx += 1;
                }
                let btn_y = tbl_y + tbl_h + 4;
                if py >= btn_y && py <= btn_y + 20 && px >= tbl_x + tbl_w - 104 && px <= tbl_x + tbl_w {
                    if let Some(pid) = Self::selected_killable_pid(self.selected_row) {
                        self.pending_action = SysmonAction::KillPid(pid);
                    }
                }
            }
            SysmonTab::Networking => {
                let tbl_h = (body_h - 72).max(60);
                let mut ry = tbl_y + 25;
                let mut idx = self.scroll_row;
                while ry + 18 <= tbl_y + tbl_h {
                    if px >= tbl_x && px <= tbl_x + row_hit_w && py >= ry && py < ry + 19 {
                        self.selected_row = idx;
                        return;
                    }
                    ry += 19;
                    idx += 1;
                }
            }
            SysmonTab::Performance => {}
        }
    }

    fn selected_killable_pid(row: usize) -> Option<u32> {
        let tasks = crate::task::scheduler::get_task_list_detailed();
        let (_, ref name, _, _, _) = tasks.get(row)?;
        let procs = crate::task::process::list_user_processes();
        procs
            .into_iter()
            .find(|(pid, pname, st)| {
                *pid > 1
                    && !matches!(st, crate::task::process::ProcessState::Zombie(_))
                    && (pname == name || name.contains(pname.as_str()))
            })
            .map(|(pid, _, _)| pid as u32)
    }

    pub fn render(&mut self, canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32) {
        let theme = libgui::gfx::current_theme();
        canvas.fill_rect(x, y, w, h, Color::XP_FACE);

        // 0. Live Menu Bar
        let menu_bar = Rect::new(x, y, w, 20);
        canvas.with_surface(|surf| {
            MenuBar::draw_bar(surf, menu_bar, SYSMON_MENUS, self.menu_state, &theme);
        });

        // 1. Property Sheet Tabs
        let tab_y = y + 26;
        let tabs = [
            ("Applications", SysmonTab::Applications),
            ("Processes", SysmonTab::Processes),
            ("Performance", SysmonTab::Performance),
            ("Networking", SysmonTab::Networking),
        ];

        let mut tab_x = x + 10;
        for (label, tab) in tabs.iter() {
            let tw = measure_ui_string(label) + 18;
            if *tab == self.active_tab {
                canvas.fill_rect(tab_x, tab_y - 2, tw, 24, Color::XP_FACE_LIGHT);
                canvas.fill_rect(tab_x, tab_y - 2, tw, 2, Color::XP_ORANGE_GLOW);
                canvas.draw_rect(tab_x, tab_y - 2, tw, 24, Color::XP_SHADOW);
                draw_ui_string_bold(canvas, tab_x + 7, tab_y, label, Color::XP_TEXT_BLACK);
            } else {
                canvas.fill_rect(tab_x, tab_y + 1, tw, 21, Color::XP_FACE_DARK);
                canvas.draw_rect(tab_x, tab_y + 1, tw, 21, Color::XP_SHADOW);
                draw_ui_string(canvas, tab_x + 8, tab_y + 2, label, Color::XP_TEXT_GRAY);
            }
            tab_x += tw + 3;
        }

        // 2. Main Tab Page Container
        let content_x = x + 10;
        let content_w = (w - 20).max(200);
        let body_y = tab_y + 22;
        let body_h = (h - 74).max(120);
        canvas.fill_rect(content_x, body_y, content_w, body_h, Color::XP_FACE_LIGHT);
        canvas.draw_rect(content_x, body_y, content_w, body_h, Color::XP_SHADOW);

        let tbl_x = content_x + 10;
        let tbl_y = body_y + 10;
        let tbl_w = (content_w - 20).max(160);

        // Read live kernel metrics once
        let (used_mb, total_mb) = {
            let pmm = PMM.lock();
            ((pmm.get_used_frames() * 4) / 1024, (pmm.get_total_frames() * 4) / 1024)
        };
        let (h_total, h_used, _) = crate::mm::heap::HEAP_ALLOCATOR.stats();
        let ticks = crate::arch::x86_64::pit::get_ticks();
        let tasks = crate::task::scheduler::get_task_list_detailed();

        match self.active_tab {
            SysmonTab::Applications => {
                let tbl_h = (body_h - 46).max(60);
                canvas.fill_rect(tbl_x, tbl_y, tbl_w, tbl_h, Color::WHITE);
                canvas.draw_rect(tbl_x, tbl_y, tbl_w, tbl_h, Color::XP_INPUT_BORDER);

                canvas.draw_gradient_v(tbl_x + 1, tbl_y + 1, tbl_w - 2, 20, Color::XP_WHITE, Color::XP_FACE_DARK);
                canvas.fill_rect(tbl_x + 1, tbl_y + 21, tbl_w - 2, 1, Color::XP_SHADOW);
                draw_ui_string(canvas, tbl_x + 8, tbl_y + 1, "Task Window", Color::XP_TEXT_BLACK);
                draw_ui_string(canvas, tbl_x + tbl_w - 92, tbl_y + 1, "Status", Color::XP_TEXT_BLACK);

                let data_w = (tbl_w - Self::SB_W - 2).max(80);
                let mut ry = tbl_y + 26;
                for (idx, (_, title, is_min)) in self.window_list.iter().enumerate().skip(self.scroll_row) {
                    if ry + 18 > tbl_y + tbl_h {
                        break;
                    }
                    let is_sel = idx == self.selected_row;
                    if is_sel {
                        canvas.fill_rect(tbl_x + 2, ry - 1, data_w, 19, Color::XP_SELECTION);
                    }
                    let fg = if is_sel { Color::WHITE } else { Color::XP_TEXT_BLACK };
                    let st = if *is_min { "Minimized" } else { "Running" };
                    draw_ui_string_ellipsis(canvas, tbl_x + 8, ry - 1, title, fg, (data_w - 98).max(24));
                    draw_ui_string(canvas, tbl_x + data_w - 80, ry - 1, st, fg);
                    ry += 20;
                }

                if let Some((sb_rect, vis_rows, total_rows, _)) = self.table_metrics(x, y, w, h) {
                    let max_scroll = total_rows.saturating_sub(vis_rows);
                    self.scroll_row = self.scroll_row.min(max_scroll);
                    self.vbar.sync_range(self.scroll_row as i32, max_scroll as i32, vis_rows as i32);
                    canvas.with_surface(|surf| {
                        ScrollBar::draw(surf, sb_rect, ScrollOrientation::Vertical, self.vbar, &theme);
                    });
                }

                let btn_y = y + h - 54;
                draw_sysmon_btn(canvas, tbl_x + tbl_w - 196, btn_y, 94, 22, "Switch To");
                draw_sysmon_btn(canvas, tbl_x + tbl_w - 94, btn_y, 94, 22, "End Task");
            }
            SysmonTab::Processes => {
                let tbl_h = (body_h - 118).max(60);
                canvas.fill_rect(tbl_x, tbl_y, tbl_w, tbl_h, Color::WHITE);
                canvas.draw_rect(tbl_x, tbl_y, tbl_w, tbl_h, Color::XP_INPUT_BORDER);

                canvas.draw_gradient_v(tbl_x + 1, tbl_y + 1, tbl_w - 2, 20, Color::XP_WHITE, Color::XP_FACE_DARK);
                canvas.fill_rect(tbl_x + 1, tbl_y + 21, tbl_w - 2, 1, Color::XP_SHADOW);

                let c_name = tbl_x + 6;
                let c_tid = tbl_x + (tbl_w * 38) / 100;
                let c_cpu = tbl_x + (tbl_w * 48) / 100;
                let c_time = tbl_x + (tbl_w * 62) / 100;
                let c_state = tbl_x + (tbl_w * 79) / 100;

                draw_ui_string(canvas, c_name, tbl_y + 1, "Task Name", Color::XP_TEXT_BLACK);
                draw_ui_string(canvas, c_tid, tbl_y + 1, "TID", Color::XP_TEXT_BLACK);
                draw_ui_string(canvas, c_cpu, tbl_y + 1, "Core", Color::XP_TEXT_BLACK);
                draw_ui_string(canvas, c_time, tbl_y + 1, "CPU Time", Color::XP_TEXT_BLACK);
                draw_ui_string(canvas, c_state, tbl_y + 1, "State", Color::XP_TEXT_BLACK);

                let data_w = (tbl_w - Self::SB_W - 2).max(80);
                let mut ry = tbl_y + 25;
                for (idx, (tid, name, state, cpu_id, cpu_ticks)) in tasks.iter().enumerate().skip(self.scroll_row) {
                    if ry + 18 > tbl_y + tbl_h {
                        break;
                    }
                    let is_sel = idx == self.selected_row;
                    if is_sel {
                        canvas.fill_rect(tbl_x + 2, ry - 1, data_w, 18, Color::XP_SELECTION);
                    }
                    let fg = if is_sel { Color::WHITE } else { Color::XP_TEXT_BLACK };
                    let sub_fg = if is_sel { Color::WHITE } else { Color::XP_TEXT_GRAY };
                    let st_str = format!("{:?}", state);
                    let time_str = if *cpu_ticks < 1000 {
                        format!("{}ms", cpu_ticks)
                    } else {
                        format!("{}.{}s", cpu_ticks / 1000, (cpu_ticks % 1000) / 100)
                    };
                    draw_ui_string_ellipsis(canvas, c_name, ry - 1, name, fg, (c_tid - c_name - 6).max(32));
                    draw_ui_string(canvas, c_tid, ry - 1, &format!("{}", tid), sub_fg);
                    draw_ui_string(canvas, c_cpu, ry - 1, &format!("CPU{}", cpu_id), sub_fg);
                    draw_ui_string(canvas, c_time, ry - 1, &time_str, sub_fg);
                    draw_ui_string_ellipsis(canvas, c_state, ry - 1, &st_str, fg, (tbl_x + data_w - c_state - 4).max(24));
                    ry += 19;
                }

                if let Some((sb_rect, vis_rows, total_rows, _)) = self.table_metrics(x, y, w, h) {
                    let max_scroll = total_rows.saturating_sub(vis_rows);
                    self.scroll_row = self.scroll_row.min(max_scroll);
                    self.vbar.sync_range(self.scroll_row as i32, max_scroll as i32, vis_rows as i32);
                    canvas.with_surface(|surf| {
                        ScrollBar::draw(surf, sb_rect, ScrollOrientation::Vertical, self.vbar, &theme);
                    });
                }

                let btn_y = tbl_y + tbl_h + 4;
                if let Some(pid) = Self::selected_killable_pid(self.selected_row) {
                    draw_ui_string(canvas, tbl_x, btn_y + 2, &format!("Selected Ring-3 PID: {}", pid), Color::XP_TEXT_LINK);
                    draw_sysmon_btn(canvas, tbl_x + tbl_w - 104, btn_y, 104, 20, "End Process");
                } else {
                    draw_ui_string(canvas, tbl_x, btn_y + 2, "Select a Ring-3 process to end", Color::XP_TEXT_GRAY);
                }

                // Bottom live meter box
                let card_y = tbl_y + tbl_h + 28;
                draw_perf_strip(canvas, tbl_x, card_y, tbl_w, used_mb, total_mb, h_used, h_total, ticks);
            }
            SysmonTab::Performance => {
                draw_ui_string_bold(canvas, tbl_x, tbl_y, "Live Kernel & Memory Telemetry", Color::XP_TEXT_BLACK);
                let mem_pct = if total_mb > 0 { ((used_mb * 100) / total_mb).min(100) } else { 0 };
                let heap_pct = if h_total > 0 { ((h_used * 100) / h_total).min(100) } else { 0 };

                let m_y = tbl_y + 28;
                draw_ui_string(canvas, tbl_x, m_y, &format!("Physical Memory: {} / {} MB ({}%)", used_mb, total_mb, mem_pct), Color::XP_TEXT_BLACK);
                canvas.fill_rect(tbl_x, m_y + 20, tbl_w, 18, Color::BLACK);
                let bar_w = ((tbl_w - 4) * mem_pct as i32) / 100;
                canvas.fill_rect(tbl_x + 2, m_y + 22, bar_w.max(2), 14, Color::rgb(0, 210, 64));

                let h_y = m_y + 52;
                draw_ui_string(canvas, tbl_x, h_y, &format!("Kernel Heap Allocator: {} / {} KB ({}%)", h_used / 1024, h_total / 1024, heap_pct), Color::XP_TEXT_BLACK);
                canvas.fill_rect(tbl_x, h_y + 20, tbl_w, 18, Color::BLACK);
                let hbar_w = ((tbl_w - 4) * heap_pct as i32) / 100;
                canvas.fill_rect(tbl_x + 2, h_y + 22, hbar_w.max(2), 14, Color::rgb(64, 175, 255));

                let u_y = h_y + 54;
                let cpu_cnt = crate::arch::x86_64::smp::SMP.lock().online_count();
                draw_ui_string_ellipsis(canvas, tbl_x, u_y, &format!("SMP Cores Online: {}   |   PIT Frequency: 1000 Hz", cpu_cnt), Color::XP_TEXT_GRAY, tbl_w);
                draw_ui_string_ellipsis(canvas, tbl_x, u_y + 22, &format!("System Uptime: {}s ({} ticks)", ticks / 1000, ticks), Color::XP_TEXT_LINK, tbl_w);
            }
            SysmonTab::Networking => {
                let tbl_h = (body_h - 72).max(60);
                canvas.fill_rect(tbl_x, tbl_y, tbl_w, tbl_h, Color::WHITE);
                canvas.draw_rect(tbl_x, tbl_y, tbl_w, tbl_h, Color::XP_INPUT_BORDER);

                canvas.draw_gradient_v(tbl_x + 1, tbl_y + 1, tbl_w - 2, 20, Color::XP_WHITE, Color::XP_FACE_DARK);
                canvas.fill_rect(tbl_x + 1, tbl_y + 21, tbl_w - 2, 1, Color::XP_SHADOW);

                let c_if = tbl_x + 8;
                let c_ip = tbl_x + (tbl_w * 28) / 100;
                let c_tx = tbl_x + (tbl_w * 58) / 100;
                let c_st = tbl_x + (tbl_w * 78) / 100;
                draw_ui_string(canvas, c_if, tbl_y + 1, "Adapter", Color::XP_TEXT_BLACK);
                draw_ui_string(canvas, c_ip, tbl_y + 1, "IPv4 Address", Color::XP_TEXT_BLACK);
                draw_ui_string(canvas, c_tx, tbl_y + 1, "TX / RX", Color::XP_TEXT_BLACK);
                draw_ui_string(canvas, c_st, tbl_y + 1, "Link", Color::XP_TEXT_BLACK);

                let data_w = (tbl_w - Self::SB_W - 2).max(80);
                let net = NET_MANAGER.lock();
                let mut ry = tbl_y + 26;
                for (idx, (dev, cfg)) in net.devices.iter().enumerate().skip(self.scroll_row) {
                    if ry + 18 > tbl_y + tbl_h {
                        break;
                    }
                    let is_sel = idx == self.selected_row;
                    if is_sel {
                        canvas.fill_rect(tbl_x + 2, ry - 1, data_w, 18, Color::XP_SELECTION);
                    }
                    let fg = if is_sel { Color::WHITE } else { Color::XP_TEXT_BLACK };
                    let up_fg = if is_sel { Color::WHITE } else if dev.is_link_up() { Color::rgb(20, 130, 45) } else { Color::XP_TEXT_GRAY };
                    draw_ui_string_ellipsis(canvas, c_if, ry - 1, dev.name(), fg, (c_ip - c_if - 4).max(20));
                    draw_ui_string(canvas, c_ip, ry - 1, &format!("{}.{}.{}.{}", cfg.ipv4[0], cfg.ipv4[1], cfg.ipv4[2], cfg.ipv4[3]), fg);
                    draw_ui_string(canvas, c_tx, ry - 1, &format!("{}/{}", cfg.tx_packets, cfg.rx_packets), fg);
                    draw_ui_string(canvas, c_st, ry - 1, if dev.is_link_up() { "UP" } else { "DOWN" }, up_fg);
                    ry += 19;
                }
                drop(net);

                if let Some((sb_rect, vis_rows, total_rows, _)) = self.table_metrics(x, y, w, h) {
                    let max_scroll = total_rows.saturating_sub(vis_rows);
                    self.scroll_row = self.scroll_row.min(max_scroll);
                    self.vbar.sync_range(self.scroll_row as i32, max_scroll as i32, vis_rows as i32);
                    canvas.with_surface(|surf| {
                        ScrollBar::draw(surf, sb_rect, ScrollOrientation::Vertical, self.vbar, &theme);
                    });
                }

                let wifi = WIFI_MANAGER.lock();
                let w_y = tbl_y + tbl_h + 10;
                let wifi_txt = if wifi.is_connected {
                    format!(
                        "Wi-Fi Active: {} (Ch {}, {} Mbps)",
                        wifi.current_ssid.as_deref().unwrap_or("WLAN"),
                        wifi.current_channel,
                        wifi.link_speed_mbps
                    )
                } else {
                    format!("Wi-Fi Idle: 802.11 BCM4331 ready ({} cached APs)", wifi.scan_results.len())
                };
                draw_ui_string_ellipsis(canvas, tbl_x, w_y, &wifi_txt, Color::XP_TEXT_LINK, tbl_w);
            }
        }

        // 4. Bottom Status Strip
        let stat_y = y + h - 22;
        canvas.fill_rect(x, stat_y, w, 22, Color::XP_FACE);
        canvas.fill_rect(x, stat_y, w, 1, Color::XP_SHADOW);
        let mem_pct = if total_mb > 0 { (used_mb * 100) / total_mb } else { 0 };
        let footer = format!(
            "Tasks: {}   Windows: {}   RAM: {}%   Up: {}s",
            tasks.len(),
            self.window_list.len(),
            mem_pct,
            ticks / 1000
        );
        draw_ui_string_ellipsis(canvas, tbl_x, stat_y + 2, &footer, Color::XP_TEXT_BLACK, w - 24);

        // Draw open dropdown menu / context menu on top
        canvas.with_surface(|surf| {
            MenuBar::draw_popup(surf, menu_bar, SYSMON_MENUS, self.menu_state, &theme);
        });
        if let Some((cx, cy)) = self.context_menu {
            let mw = 168;
            let mh = 96;
            canvas.fill_rect(cx + 2, cy + 2, mw, mh, Color::argb(48, 0, 0, 0));
            canvas.fill_rect(cx, cy, mw, mh, Color::from_libgui(theme.face_light));
            canvas.draw_rect(cx, cy, mw, mh, Color::from_libgui(theme.ctrl_border));
            let items = if self.active_tab == SysmonTab::Applications {
                ["Switch To Window", "End Task", "Next Tab", "About SublimeOS"]
            } else {
                ["Performance View", "New Task (Run)...", "Next Tab", "About SublimeOS"]
            };
            for (idx, lbl) in items.iter().enumerate() {
                let ry = cy + 4 + (idx as i32) * 22;
                let hov = self.ctx_hover == Some(idx);
                if hov {
                    canvas.fill_rect(cx + 2, ry, mw - 4, 22, Color::from_libgui(theme.selection));
                }
                draw_ui_string(
                    canvas,
                    cx + 10,
                    ry + 2,
                    lbl,
                    Color::from_libgui(if hov { theme.selection_text } else { theme.text_primary }),
                );
            }
        }
    }
}

fn draw_perf_strip(
    canvas: &mut Canvas,
    tbl_x: i32,
    card_y: i32,
    tbl_w: i32,
    used_mb: usize,
    total_mb: usize,
    h_used: usize,
    h_total: usize,
    ticks: u64,
) {
    let meter_w = 128;
    let meter_h = 68;
    canvas.fill_rect(tbl_x, card_y, meter_w, meter_h, Color::BLACK);
    canvas.draw_rect(tbl_x, card_y, meter_w, meter_h, Color::XP_SHADOW);
    draw_ui_string(canvas, tbl_x + 8, card_y + 4, "MEM Usage", Color::rgb(0, 220, 60));

    let mem_pct = if total_mb > 0 { ((used_mb * 100) / total_mb).min(100) } else { 0 };
    let bar_w = 108;
    let filled = ((mem_pct as i32 * bar_w) / 100).clamp(2, bar_w);
    canvas.fill_rect(tbl_x + 8, card_y + 28, bar_w, 12, Color::rgb(16, 48, 20));
    canvas.fill_rect(tbl_x + 8, card_y + 28, filled, 12, Color::rgb(0, 210, 64));
    draw_ui_string(canvas, tbl_x + 8, card_y + 44, &format!("{}%", mem_pct), Color::rgb(120, 255, 140));

    let mid_x = tbl_x + meter_w + 14;
    let rem_w = (tbl_w - meter_w - 16).max(80);
    draw_ui_string_ellipsis(canvas, mid_x, card_y + 4, &format!("Physical Memory: {}/{} MB", used_mb, total_mb), Color::XP_TEXT_BLACK, rem_w);
    draw_ui_string_ellipsis(canvas, mid_x, card_y + 24, &format!("Kernel Heap: {}.{} / {}.{} MB", h_used / 1024 / 1024, (h_used / 102400) % 10, h_total / 1024 / 1024, (h_total / 102400) % 10), Color::XP_TEXT_BLACK, rem_w);
    draw_ui_string_ellipsis(canvas, mid_x, card_y + 44, &format!("Uptime: {}s (1000 Hz)", ticks / 1000), Color::XP_TEXT_LINK, rem_w);
}

fn draw_sysmon_btn(canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32, label: &str) {
    let theme = libgui::gfx::current_theme();
    canvas.fill_rect(x, y, w, h, Color::from_libgui(theme.face_dark));
    canvas.draw_rect(x, y, w, h, Color::from_libgui(theme.ctrl_border));
    let tw = measure_ui_string(label).min(w - 8);
    draw_ui_string_centered_v(canvas, x + (w - tw) / 2, y, h, label, Color::from_libgui(theme.text_primary));
}
