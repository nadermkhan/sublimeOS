use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::fs::vfs::{FileType, VFS};
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::font::{
    draw_ui_string, draw_ui_string_bold, draw_ui_string_ellipsis, measure_ui_string, FONT_HEIGHT,
};
use crate::gui::mouse::CursorType;
use crate::gui::wallpaper::{self, DecodedImage};
use crate::sync::IrqSafeSpinlock;
use libgui::event::{GuiEvent, KeyCode, Modifiers, MouseButton};
use libgui::font::{with_default_font, TextStyle};
use libgui::gfx::{Insets, Rect};
use libgui::layout::Length;
use libgui::widget::{
    Button, ButtonState, ButtonStyle, ContextMenu, ContextMenuState, ListView, ListViewState,
    MenuBar, MenuBarState, MenuItemSpec, MenuSpec, StatusBar, StatusPane, TextInput,
    TextInputState,
};

#[derive(Clone)]
pub struct FileEntryInfo {
    pub name: String,
    pub file_type: FileType,
    pub size: usize,
    pub inode: u64,
    pub is_elf: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SortField {
    Name,
    Type,
    Size,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HoverTarget {
    Sidebar(usize),
    ColHeader(SortField),
    SearchClear,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PreviewAction {
    Restore,
    SetWallpaper,
    Open,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RenameOutcome {
    Committed,
    UnchangedOrEmpty,
    Invalid,
    Failed,
}

#[derive(Clone, Copy)]
struct PlaceSpec {
    id: u16,
    label: &'static str,
    path: &'static str,
    is_favorite: bool,
}

const FM_PLACE_SPECS: &[PlaceSpec] = &[
    // Places (7 items, IDs 50..=56)
    PlaceSpec { id: 50, label: "Applications", path: "/Applications", is_favorite: true },
    PlaceSpec { id: 51, label: "Desktop", path: "/Users/sublime/Desktop", is_favorite: true },
    PlaceSpec { id: 52, label: "Documents", path: "/Users/sublime/Documents", is_favorite: true },
    PlaceSpec { id: 53, label: "Music", path: "/Users/sublime/Music", is_favorite: true },
    PlaceSpec { id: 54, label: "Pictures", path: "/Users/sublime/Pictures", is_favorite: true },
    PlaceSpec { id: 55, label: "Downloads", path: "/Users/sublime/Downloads", is_favorite: true },
    PlaceSpec { id: 56, label: "Trash", path: "/Trash", is_favorite: true },
    // Computer (5 items, IDs 57..=61)
    PlaceSpec { id: 57, label: "Local Disk (/)", path: "/", is_favorite: false },
    PlaceSpec { id: 58, label: "Volumes", path: "/Volumes", is_favorite: false },
    PlaceSpec { id: 59, label: "System", path: "/System", is_favorite: false },
    PlaceSpec { id: 60, label: "Devices (/dev)", path: "/dev", is_favorite: false },
    PlaceSpec { id: 61, label: "Config (/etc)", path: "/etc", is_favorite: false },
];

const SIDEBAR_CARD_HDR_H: i32 = 18;
const SIDEBAR_ROW_H: i32 = 18;
const SIDEBAR_CARD_GAP: i32 = 4;

const FM_FILE_ITEMS: &[MenuItemSpec] = &[
    MenuItemSpec::action(1, "Open Selected", "Enter"),
    MenuItemSpec::action(20, "Set as Wallpaper", ""),
    MenuItemSpec::sep(),
    MenuItemSpec::action(4, "New Folder", "Ctrl+N"),
    MenuItemSpec::action(5, "New Text File", ""),
    MenuItemSpec::action(21, "Rename", "F2"),
    MenuItemSpec::action(6, "Delete / Trash", "Del"),
    MenuItemSpec::action(22, "Empty Trash", ""),
    MenuItemSpec::sep(),
    MenuItemSpec::action(23, "Properties", "Alt+Enter"),
];

const FM_EDIT_ITEMS: &[MenuItemSpec] = &[
    MenuItemSpec::action(30, "Cut", "Ctrl+X"),
    MenuItemSpec::action(31, "Copy", "Ctrl+C"),
    MenuItemSpec::action(32, "Paste", "Ctrl+V"),
    MenuItemSpec::sep(),
    MenuItemSpec::action(7, "Copy Path", ""),
];

const FM_VIEW_ITEMS: &[MenuItemSpec] = &[
    MenuItemSpec::action(40, "Sort by Name", ""),
    MenuItemSpec::action(41, "Sort by Type", ""),
    MenuItemSpec::action(42, "Sort by Size", ""),
    MenuItemSpec::sep(),
    MenuItemSpec::action(43, "Toggle Hidden Files", "Ctrl+H"),
    MenuItemSpec::action(44, "Toggle Preview Pane", ""),
    MenuItemSpec::action(3, "Refresh View", "F5"),
];

const FM_PLACES_ITEMS: &[MenuItemSpec] = &[
    MenuItemSpec::action(FM_PLACE_SPECS[0].id, FM_PLACE_SPECS[0].label, ""),
    MenuItemSpec::action(FM_PLACE_SPECS[1].id, FM_PLACE_SPECS[1].label, ""),
    MenuItemSpec::action(FM_PLACE_SPECS[2].id, FM_PLACE_SPECS[2].label, ""),
    MenuItemSpec::action(FM_PLACE_SPECS[3].id, FM_PLACE_SPECS[3].label, ""),
    MenuItemSpec::action(FM_PLACE_SPECS[4].id, FM_PLACE_SPECS[4].label, ""),
    MenuItemSpec::action(FM_PLACE_SPECS[5].id, FM_PLACE_SPECS[5].label, ""),
    MenuItemSpec::action(FM_PLACE_SPECS[6].id, FM_PLACE_SPECS[6].label, ""),
    MenuItemSpec::sep(),
    MenuItemSpec::action(FM_PLACE_SPECS[7].id, FM_PLACE_SPECS[7].label, ""),
    MenuItemSpec::action(FM_PLACE_SPECS[8].id, FM_PLACE_SPECS[8].label, ""),
    MenuItemSpec::action(FM_PLACE_SPECS[9].id, FM_PLACE_SPECS[9].label, ""),
    MenuItemSpec::action(FM_PLACE_SPECS[10].id, FM_PLACE_SPECS[10].label, ""),
    MenuItemSpec::action(FM_PLACE_SPECS[11].id, FM_PLACE_SPECS[11].label, ""),
];

const FM_MENUS: &[MenuSpec] = &[
    MenuSpec { title: "File", items: FM_FILE_ITEMS },
    MenuSpec { title: "Edit", items: FM_EDIT_ITEMS },
    MenuSpec { title: "View", items: FM_VIEW_ITEMS },
    MenuSpec { title: "Places", items: FM_PLACES_ITEMS },
];

/// Tracks original parent folders for items moved into `/Trash` across File Explorer instances.
static TRASH_ORIGINS: IrqSafeSpinlock<Vec<(String, String)>> = IrqSafeSpinlock::new(Vec::new());

fn record_trash_origin(trash_name: &str, orig_parent: &str) {
    let mut origins = TRASH_ORIGINS.lock();
    origins.retain(|(n, _)| n != trash_name);
    if origins.len() >= 64 {
        origins.remove(0);
    }
    origins.push((String::from(trash_name), String::from(orig_parent)));
}

fn take_trash_origin(trash_name: &str) -> Option<String> {
    let mut origins = TRASH_ORIGINS.lock();
    if let Some(pos) = origins.iter().position(|(n, _)| n == trash_name) {
        Some(origins.remove(pos).1)
    } else {
        None
    }
}

fn rename_trash_origin(old_name: &str, new_name: &str) {
    let mut origins = TRASH_ORIGINS.lock();
    let existing = origins.iter().position(|(n, _)| n == old_name).map(|pos| origins.remove(pos).1);
    if let Some(orig_parent) = existing {
        origins.retain(|(n, _)| n != new_name);
        origins.push((String::from(new_name), orig_parent));
    }
}

fn clear_trash_origins() {
    TRASH_ORIGINS.lock().clear();
}

#[derive(Clone, Copy)]
struct FileManagerLayout {
    bounds: Rect,
    menu_bar: Rect,
    toolbar: Rect,
    btn_back: Rect,
    btn_fwd: Rect,
    btn_up: Rect,
    btn_refresh: Rect,
    addr_rect: Rect,
    search_rect: Rect,
    search_clear_rect: Rect,
    sidebar_rect: Rect,
    header_rect: Rect,
    type_x: i32,
    size_x: i32,
    show_type_col: bool,
    show_size_col: bool,
    list_rect: Rect,
    content_right: i32,
    preview_rect: Option<Rect>,
    preview_btn_rect: Option<Rect>,
    status_rect: Rect,
}

impl FileManagerLayout {
    fn compute(
        ix: i32,
        iy: i32,
        w: i32,
        h: i32,
        show_preview: bool,
        entry_count: usize,
        row_height: i32,
        search_has_text: bool,
    ) -> Self {
        let w = w.max(80);
        let h = h.max(88);
        let bounds = Rect::new(ix, iy, w, h);
        let menu_bar = Rect::new(ix, iy, w, 20);
        let toolbar = Rect::new(ix, iy + 20, w, 28);

        let tb_y = iy + 23;
        let btn_back = Rect::new(ix + 6, tb_y, 24, 22);
        let btn_fwd = Rect::new(ix + 32, tb_y, 24, 22);
        let btn_up = Rect::new(ix + 58, tb_y, 24, 22);
        let btn_refresh = Rect::new(ix + 84, tb_y, 24, 22);

        let search_w = if w >= 460 { 120 } else { 0 };
        let addr_x = ix + 114;
        let addr_w = (w - 120 - search_w - if search_w > 0 { 6 } else { 0 }).max(48);
        let addr_rect = Rect::new(addr_x, tb_y, addr_w, 22);
        let search_rect = if search_w > 0 {
            Rect::new(addr_x + addr_w + 6, tb_y, search_w, 22)
        } else {
            Rect::default()
        };
        let search_clear_rect = if search_w > 0 && search_has_text {
            Rect::new(search_rect.right() - 19, tb_y + 3, 16, 16)
        } else {
            Rect::default()
        };

        let status_rect = Rect::new(ix, iy + h - 20, w, 20);
        let body_top = iy + 48;
        let body_bottom = status_rect.y;
        let body_h = (body_bottom - body_top).max(0);

        let left_w = if w >= 380 { 150 } else { 0 };
        let sidebar_rect = if left_w > 0 {
            Rect::new(ix, body_top, left_w, body_h)
        } else {
            Rect::default()
        };

        let right_x = ix + left_w;
        let right_w = (w - left_w).max(60);
        let header_rect = Rect::new(right_x, body_top, right_w, 22);
        let right_body_top = header_rect.bottom();
        let right_body_h = (body_bottom - right_body_top).max(0);

        let preview_h = if show_preview && right_body_h >= 196 { 74 } else { 0 };
        let list_h = (right_body_h - preview_h).max(0);
        let list_rect = Rect::new(right_x, right_body_top, right_w, list_h);
        let preview_rect = if preview_h > 0 {
            Some(Rect::new(right_x, list_rect.bottom(), right_w, preview_h))
        } else {
            None
        };
        let preview_btn_rect = preview_rect.and_then(|pr| {
            if right_w >= 210 {
                Some(Rect::new(pr.right() - 122, pr.y + 4, 114, 22))
            } else {
                None
            }
        });

        let inner_list = list_rect.inset(Insets::all(1));
        let rh = row_height.max(1);
        let needs_sb = (entry_count as i32).saturating_mul(rh) > inner_list.h;
        let content_right = inner_list.right() - if needs_sb { ListView::SCROLLBAR_W } else { 0 };
        let content_w = (content_right - inner_list.x).max(40);
        let show_size_col = content_w >= 170;
        let show_type_col = content_w >= 260;
        let size_x = content_right - 64;
        let type_x = size_x - 86;

        Self {
            bounds,
            menu_bar,
            toolbar,
            btn_back,
            btn_fwd,
            btn_up,
            btn_refresh,
            addr_rect,
            search_rect,
            search_clear_rect,
            sidebar_rect,
            header_rect,
            type_x,
            size_x,
            show_type_col,
            show_size_col,
            list_rect,
            content_right,
            preview_rect,
            preview_btn_rect,
            status_rect,
        }
    }

    fn popup_bounds(&self) -> Rect {
        Rect::new(
            self.bounds.x,
            self.menu_bar.bottom(),
            self.bounds.w,
            (self.bounds.h - self.menu_bar.h).max(40),
        )
    }

    fn content_rect(&self) -> Rect {
        let inner = self.list_rect.inset(Insets::all(1));
        Rect::new(inner.x, inner.y, (self.content_right - inner.x).max(0), inner.h)
    }

    fn sidebar_item_rect(&self, idx: usize) -> Option<Rect> {
        if self.sidebar_rect.w <= 0 || idx >= FM_PLACE_SPECS.len() {
            return None;
        }
        let card_x = self.sidebar_rect.x + 8;
        let card_w = self.sidebar_rect.w - 16;
        let fav_count = FM_PLACE_SPECS.iter().filter(|p| p.is_favorite).count();
        let card1_top = self.sidebar_rect.y + 6;
        let card1_body_top = card1_top + SIDEBAR_CARD_HDR_H;
        let y = if idx < fav_count {
            card1_body_top + 1 + (idx as i32) * SIDEBAR_ROW_H
        } else {
            let card1_h = SIDEBAR_CARD_HDR_H + (fav_count as i32) * SIDEBAR_ROW_H + 2;
            let card2_top = card1_top + card1_h + SIDEBAR_CARD_GAP;
            let comp_idx = idx - fav_count;
            card2_top + SIDEBAR_CARD_HDR_H + 1 + (comp_idx as i32) * SIDEBAR_ROW_H
        };
        let r = Rect::new(card_x + 2, y, (card_w - 4).max(0), SIDEBAR_ROW_H);
        let vis = r.intersect(self.sidebar_rect);
        if vis.w > 0 && vis.h > 0 { Some(vis) } else { None }
    }

    fn rename_rect(&self, rename_idx: usize, scroll_y: i32, row_height: i32) -> Option<Rect> {
        let inner = self.list_rect.inset(Insets::all(1));
        if inner.h <= 0 || inner.w <= 0 {
            return None;
        }
        let rh = row_height.max(1);
        let row_y = inner.y + (rename_idx as i32) * rh - scroll_y;
        if row_y + rh <= inner.y || row_y >= inner.bottom() {
            return None;
        }
        let name_right = if self.show_type_col {
            self.type_x - 6
        } else if self.show_size_col {
            self.size_x - 6
        } else {
            self.content_right - 4
        };
        let rx = inner.x + 4;
        let max_w = (self.content_right - inner.x - 8).max(24);
        let rw = (name_right - rx).clamp(24, max_w);
        Some(Rect::new(rx, row_y, rw, rh))
    }

    fn header_field_at(&self, x: i32, y: i32) -> Option<SortField> {
        if !self.header_rect.contains(x, y) {
            return None;
        }
        if self.show_size_col && x >= self.size_x {
            Some(SortField::Size)
        } else if self.show_type_col && x >= self.type_x {
            Some(SortField::Type)
        } else {
            Some(SortField::Name)
        }
    }

    fn viewer_rects(&self) -> (Rect, Rect, Rect, Rect, Rect, Rect) {
        let panel = Rect::new(
            self.bounds.x,
            self.menu_bar.bottom(),
            self.bounds.w,
            (self.bounds.h - self.menu_bar.h - self.status_rect.h).max(60),
        );
        let hdr = Rect::new(panel.x, panel.y, panel.w, 28);
        let bottom = Rect::new(panel.x, self.status_rect.y - 32, panel.w, 32);
        let img_area = Rect::new(
            panel.x + 8,
            hdr.bottom() + 4,
            (panel.w - 16).max(32),
            (bottom.y - hdr.bottom() - 8).max(32),
        );
        let btn_wall = Rect::new(panel.x + 10, bottom.y + 4, 116, 24);
        let btn_close = Rect::new(panel.right() - 86, bottom.y + 4, 76, 24);
        (panel, hdr, img_area, bottom, btn_wall, btn_close)
    }

    fn properties_rects(&self) -> (Rect, Rect, Rect) {
        let mw = 316.min((self.bounds.w - 20).max(200));
        let mh = 208.min((self.bounds.h - 24).max(120));
        let mx = self.bounds.x + (self.bounds.w - mw) / 2;
        let my = (self.bounds.y + (self.bounds.h - mh) / 2).max(self.menu_bar.bottom());
        let modal = Rect::new(mx, my, mw, mh);
        let btn_wall = Rect::new(mx + 12, my + mh - 32, 134, 22);
        let btn_close = Rect::new(mx + mw - 82, my + mh - 32, 70, 22);
        (modal, btn_wall, btn_close)
    }
}

pub struct FileManagerApp {
    pub current_path: String,
    pub all_entries: Vec<FileEntryInfo>,
    pub entries: Vec<FileEntryInfo>,
    pub selected_index: usize,
    pub last_click_index: Option<usize>,
    pub last_click_ms: u64,
    pub pending_open_file: Option<String>,
    pub pending_title: Option<String>,
    pub list_state: ListViewState,
    pub menu_state: MenuBarState,
    pub preview_text: String,
    pub status_msg: String,
    pub preview_image: Option<DecodedImage>,
    pub image_viewer: Option<(String, DecodedImage)>,
    pub context_menu: ContextMenuState,
    pub context_items: Vec<MenuItemSpec>,
    pub back_history: Vec<String>,
    pub forward_history: Vec<String>,
    pub addr_input: TextInputState,
    pub addr_focused: bool,
    pub search_input: TextInputState,
    pub search_focused: bool,
    pub rename_index: Option<usize>,
    pub rename_input: TextInputState,
    pub clipboard: Option<(String, bool)>,
    pub sort_field: SortField,
    pub sort_asc: bool,
    pub show_hidden: bool,
    pub show_preview: bool,
    pub show_properties: bool,
    pub properties_target_dir: bool,
    pub last_vfs_gen: u64,
    last_viewport: (i32, i32),
    preview_cache_key: Option<(String, usize, u64, u64)>,
    hover_target: Option<HoverTarget>,
    btn_back: ButtonState,
    btn_fwd: ButtonState,
    btn_up: ButtonState,
    btn_refresh: ButtonState,
    btn_preview_act: ButtonState,
    btn_viewer_wall: ButtonState,
    btn_viewer_close: ButtonState,
    btn_props_wall: ButtonState,
    btn_props_close: ButtonState,
}

impl FileManagerApp {
    pub fn new() -> Self {
        let mut app = Self {
            current_path: String::from("/Applications"),
            all_entries: Vec::new(),
            entries: Vec::new(),
            selected_index: 0,
            last_click_index: None,
            last_click_ms: 0,
            pending_open_file: None,
            pending_title: None,
            list_state: ListViewState::new(22),
            menu_state: MenuBarState::default(),
            preview_text: String::new(),
            status_msg: String::new(),
            preview_image: None,
            image_viewer: None,
            context_menu: ContextMenuState::default(),
            context_items: Vec::new(),
            back_history: Vec::new(),
            forward_history: Vec::new(),
            addr_input: TextInputState::new("/Applications"),
            addr_focused: false,
            search_input: TextInputState::new(""),
            search_focused: false,
            rename_index: None,
            rename_input: TextInputState::new(""),
            clipboard: None,
            sort_field: SortField::Name,
            sort_asc: true,
            show_hidden: false,
            show_preview: true,
            show_properties: false,
            properties_target_dir: false,
            last_vfs_gen: crate::fs::vfs::vfs_generation(),
            last_viewport: (588, 359),
            preview_cache_key: None,
            hover_target: None,
            btn_back: ButtonState::default(),
            btn_fwd: ButtonState::default(),
            btn_up: ButtonState::default(),
            btn_refresh: ButtonState::default(),
            btn_preview_act: ButtonState::default(),
            btn_viewer_wall: ButtonState::default(),
            btn_viewer_close: ButtonState::default(),
            btn_props_wall: ButtonState::default(),
            btn_props_close: ButtonState::default(),
        };
        app.refresh();
        app
    }

    fn layout(&self, ix: i32, iy: i32, w: i32, h: i32) -> FileManagerLayout {
        FileManagerLayout::compute(
            ix,
            iy,
            w,
            h,
            self.show_preview,
            self.entries.len(),
            self.list_state.row_height,
            !self.search_input.text.is_empty(),
        )
    }

    fn reset_button_states(&mut self) {
        self.btn_back = ButtonState::default();
        self.btn_fwd = ButtonState::default();
        self.btn_up = ButtonState::default();
        self.btn_refresh = ButtonState::default();
        self.btn_preview_act = ButtonState::default();
        self.btn_viewer_wall = ButtonState::default();
        self.btn_viewer_close = ButtonState::default();
        self.btn_props_wall = ButtonState::default();
        self.btn_props_close = ButtonState::default();
        self.sync_nav_button_disabled();
    }

    fn sync_nav_button_disabled(&mut self) {
        self.btn_back.disabled = self.back_history.is_empty();
        self.btn_fwd.disabled = self.forward_history.is_empty();
        self.btn_up.disabled = self.current_path == "/";
    }

    fn unfocus_inputs(&mut self) {
        if self.addr_focused {
            self.addr_focused = false;
            self.addr_input.focused = false;
            self.addr_input.set_text(&self.current_path);
        }
        if self.search_focused {
            self.search_focused = false;
            self.search_input.focused = false;
        }
    }

    fn cancel_rename(&mut self) {
        self.rename_index = None;
        self.rename_input.focused = false;
    }

    pub fn dismiss_popups(&mut self) {
        self.context_menu.close();
        self.menu_state = MenuBarState::default();
        self.hover_target = None;
        self.list_state.dragging_thumb = false;
        self.reset_button_states();
    }

    pub fn on_deactivate(&mut self) {
        self.dismiss_popups();
        self.unfocus_inputs();
        self.cancel_rename();
    }

    fn sync_title(&mut self) {
        let name = if self.current_path == "/" {
            "/"
        } else {
            self.current_path.rsplit('/').next().unwrap_or(&self.current_path)
        };
        self.pending_title = Some(format!("Files - {}", name));
    }

    pub fn sync_vfs(&mut self) -> bool {
        let gen = crate::fs::vfs::vfs_generation();
        if gen == self.last_vfs_gen || self.rename_index.is_some() {
            return false;
        }
        let prev_name = self.entries.get(self.selected_index).map(|e| e.name.clone());
        self.refresh_inner(false);
        if let Some(name) = prev_name {
            if let Some(pos) = self.entries.iter().position(|e| e.name == name) {
                self.selected_index = pos;
                self.list_state.selected = Some(pos);
                self.update_preview();
            }
        }
        true
    }

    pub fn refresh(&mut self) {
        self.refresh_inner(false);
    }

    fn refresh_inner(&mut self, reset_scroll: bool) {
        self.last_vfs_gen = crate::fs::vfs::vfs_generation();
        self.all_entries.clear();
        let raw_res = {
            let vfs = VFS.lock();
            vfs.resolve_path(&self.current_path).and_then(|dir| {
                let list = dir.lock().readdir()?;
                let mut detailed = Vec::with_capacity(list.len());
                for de in list {
                    let mut is_elf = false;
                    let lookup_res = dir.lock().lookup(&de.name);
                    let (size, inode) = match lookup_res {
                        Ok(child) => {
                            let st = child.lock().stat();
                            if st.file_type != FileType::Directory && st.size >= 20 {
                                let mut hdr = [0u8; 20];
                                if let Ok(n) = child.lock().read(0, &mut hdr) {
                                    is_elf = crate::user::apps::is_elf64_magic(&hdr[..n]);
                                }
                            }
                            (st.size, st.inode)
                        }
                        Err(_) => (0, de.inode),
                    };
                    detailed.push(FileEntryInfo {
                        name: de.name,
                        file_type: de.file_type,
                        size,
                        inode,
                        is_elf,
                    });
                }
                Ok(detailed)
            })
        };
        if let Ok(list) = raw_res {
            self.all_entries = list;
        }
        if !self.addr_focused {
            self.addr_input.set_text(&self.current_path);
        }
        self.cancel_rename();
        self.sync_nav_button_disabled();
        self.apply_filter_and_sort_keep_scroll(reset_scroll);
    }

    pub fn apply_filter_and_sort(&mut self) {
        self.last_click_index = None;
        self.apply_filter_and_sort_keep_scroll(true);
    }

    fn apply_filter_and_sort_keep_scroll(&mut self, reset_scroll: bool) {
        let query = self.search_input.text.to_ascii_lowercase();
        let mut filtered: Vec<FileEntryInfo> = self
            .all_entries
            .iter()
            .filter(|e| {
                if !self.show_hidden && e.name.starts_with('.') {
                    return false;
                }
                if query.is_empty() {
                    true
                } else {
                    e.name.to_ascii_lowercase().contains(&query)
                }
            })
            .cloned()
            .collect();

        let sort_field = self.sort_field;
        let asc = self.sort_asc;
        filtered.sort_by(|a, b| {
            let a_dir = a.file_type == FileType::Directory;
            let b_dir = b.file_type == FileType::Directory;
            if a_dir != b_dir {
                return if a_dir {
                    core::cmp::Ordering::Less
                } else {
                    core::cmp::Ordering::Greater
                };
            }
            let ord = match sort_field {
                SortField::Name => a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()),
                SortField::Type => entry_type_label(a).cmp(entry_type_label(b)),
                SortField::Size => a.size.cmp(&b.size),
            };
            if asc { ord } else { ord.reverse() }
        });

        self.entries = filtered;
        if self.selected_index >= self.entries.len() {
            self.selected_index = 0;
        }
        let view_h = self
            .layout(0, 0, self.last_viewport.0, self.last_viewport.1)
            .list_rect
            .inset(Insets::all(1))
            .h
            .max(self.list_state.row_height);
        if reset_scroll {
            self.list_state.scroll_y = 0;
        } else {
            self.list_state.clamp_scroll(self.entries.len(), view_h);
        }
        self.list_state.selected = if self.entries.is_empty() {
            None
        } else {
            Some(self.selected_index)
        };
        self.update_preview();
    }

    fn ensure_selected_visible(&mut self) {
        if self.entries.is_empty() || self.selected_index >= self.entries.len() {
            return;
        }
        let rh = self.list_state.row_height.max(1);
        let view_h = self
            .layout(0, 0, self.last_viewport.0, self.last_viewport.1)
            .list_rect
            .inset(Insets::all(1))
            .h
            .max(rh);
        let row_top = (self.selected_index as i32).saturating_mul(rh);
        let row_bottom = row_top.saturating_add(rh);
        if row_top < self.list_state.scroll_y {
            self.list_state.scroll_y = row_top;
        } else if row_bottom > self.list_state.scroll_y + view_h {
            self.list_state.scroll_y = row_bottom - view_h;
        }
        self.list_state.clamp_scroll(self.entries.len(), view_h);
    }

    pub fn navigate_to(&mut self, path: &str) {
        let normalized = normalize_vfs_path(path);
        let ok = {
            let vfs = VFS.lock();
            vfs.resolve_path(&normalized)
                .map(|n| n.lock().stat().file_type == FileType::Directory)
                .unwrap_or(false)
        };
        if !ok {
            self.status_msg = format!("Folder not found: {}", normalized);
            return;
        }
        let changed = normalized != self.current_path;
        if changed {
            self.back_history.push(self.current_path.clone());
            if self.back_history.len() > 32 {
                self.back_history.remove(0);
            }
            self.forward_history.clear();
            self.current_path = normalized;
            self.selected_index = 0;
        }
        self.last_click_index = None;
        self.search_input.set_text("");
        self.addr_focused = false;
        self.addr_input.focused = false;
        self.show_properties = false;
        self.image_viewer = None;
        self.context_menu.close();
        self.menu_state = MenuBarState::default();
        self.sync_title();
        self.refresh_inner(true);
    }

    pub fn navigate_back(&mut self) {
        if let Some(prev) = self.back_history.pop() {
            self.forward_history.push(self.current_path.clone());
            self.current_path = prev;
            self.selected_index = 0;
            self.last_click_index = None;
            self.search_input.set_text("");
            self.addr_focused = false;
            self.addr_input.focused = false;
            self.show_properties = false;
            self.image_viewer = None;
            self.context_menu.close();
            self.sync_title();
            self.refresh_inner(true);
        }
    }

    pub fn navigate_forward(&mut self) {
        if let Some(next) = self.forward_history.pop() {
            self.back_history.push(self.current_path.clone());
            self.current_path = next;
            self.selected_index = 0;
            self.last_click_index = None;
            self.search_input.set_text("");
            self.addr_focused = false;
            self.addr_input.focused = false;
            self.show_properties = false;
            self.image_viewer = None;
            self.context_menu.close();
            self.sync_title();
            self.refresh_inner(true);
        }
    }

    pub fn navigate_parent(&mut self) {
        if self.current_path == "/" {
            return;
        }
        let mut parent = self.current_path.clone();
        if let Some(idx) = parent.rfind('/') {
            if idx == 0 {
                parent = String::from("/");
            } else {
                parent.truncate(idx);
            }
            self.navigate_to(&parent);
        }
    }

    fn selected_full_path(&self) -> Option<String> {
        let entry = self.entries.get(self.selected_index)?;
        Some(join_path(&self.current_path, &entry.name))
    }

    pub fn open_selected(&mut self) {
        let Some(entry) = self.entries.get(self.selected_index).cloned() else {
            return;
        };
        let full_path = join_path(&self.current_path, &entry.name);
        if entry.file_type == FileType::Directory {
            self.navigate_to(&full_path);
        } else if wallpaper::is_supported_image(&full_path) {
            let vfs_gen = crate::fs::vfs::vfs_generation();
            if let Some(cached) = self.preview_image.as_ref().filter(|_| {
                self.preview_cache_key.as_ref().map_or(false, |(p, _, _, g)| {
                    p == &full_path && *g == vfs_gen
                })
            }) {
                let img = cached.clone();
                self.status_msg = format!("Viewing {} ({}x{})", entry.name, img.width, img.height);
                self.image_viewer = Some((full_path, img));
                return;
            }
            match wallpaper::load_vfs_image(&full_path) {
                Ok(img) => {
                    self.status_msg = format!("Viewing {} ({}x{})", entry.name, img.width, img.height);
                    self.image_viewer = Some((full_path, img));
                }
                Err(e) => {
                    self.status_msg = format!("Image open error: {}", e);
                }
            }
        } else if entry.file_type == FileType::Regular {
            self.pending_open_file = Some(full_path);
        } else {
            self.status_msg = format!("Device node {} cannot be opened directly", entry.name);
        }
    }

    pub fn set_selected_as_wallpaper(&mut self) {
        let Some(full_path) = self.selected_full_path() else {
            return;
        };
        if !wallpaper::is_supported_image(&full_path) {
            self.status_msg = String::from("Select a .png, .jpg, .qoi, .bmp, .tga, or .ppm image for wallpaper");
            return;
        }
        match wallpaper::set_wallpaper_from_file(&full_path) {
            Ok((w, h)) => {
                self.status_msg = format!("Wallpaper set: {} ({}x{})", full_path, w, h);
            }
            Err(e) => {
                self.status_msg = format!("Wallpaper failed: {}", e);
            }
        }
    }

    pub fn click_item(&mut self, index: usize) {
        if index >= self.entries.len() {
            return;
        }
        let now_ms = crate::arch::x86_64::pit::get_ticks();
        let is_double = self.last_click_index == Some(index)
            && now_ms.saturating_sub(self.last_click_ms) <= 550;
        self.last_click_index = Some(index);
        self.last_click_ms = now_ms;
        self.selected_index = index;
        self.list_state.selected = Some(index);
        self.update_preview();
        self.ensure_selected_visible();
        if is_double {
            self.last_click_index = None;
            self.open_selected();
        }
    }

    pub fn select_item(&mut self, index: usize) {
        self.selected_index = index;
        self.list_state.selected = if index < self.entries.len() { Some(index) } else { None };
        self.open_selected();
    }

    fn preview_action(&self) -> Option<(PreviewAction, &'static str)> {
        let entry = self.entries.get(self.selected_index)?;
        if self.current_path == "/Trash" {
            Some((PreviewAction::Restore, "Restore"))
        } else if wallpaper::is_supported_image(&entry.name) {
            Some((PreviewAction::SetWallpaper, "Set Wallpaper"))
        } else if libgui::codecs::is_audio_path(&entry.name) {
            Some((PreviewAction::Open, "Play in Music"))
        } else if entry.file_type == FileType::Regular {
            Some((PreviewAction::Open, "Open"))
        } else {
            None
        }
    }

    fn update_preview(&mut self) {
        let Some(entry) = self.entries.get(self.selected_index).cloned() else {
            self.preview_cache_key = None;
            self.preview_image = None;
            self.preview_text = format!("Folder {} (empty)", self.current_path);
            return;
        };
        let full_path = join_path(&self.current_path, &entry.name);
        if entry.file_type == FileType::Directory {
            self.preview_cache_key = None;
            self.preview_image = None;
            let count = {
                let vfs = VFS.lock();
                vfs.resolve_path(&full_path)
                    .and_then(|d| d.lock().readdir())
                    .map(|l| l.len())
                    .unwrap_or(0)
            };
            self.preview_text = format!("Folder: {}\n{} item(s)", entry.name, count);
            return;
        }

        let vfs_gen = crate::fs::vfs::vfs_generation();
        let next_key = (full_path.clone(), entry.size, entry.inode, vfs_gen);
        if self.preview_cache_key.as_ref() == Some(&next_key) {
            return;
        }

        self.preview_text.clear();
        self.preview_image = None;
        self.preview_cache_key = Some(next_key);

        if wallpaper::is_supported_image(&entry.name) {
            if let Ok(img) = wallpaper::load_vfs_image(&full_path) {
                self.preview_text = format!(
                    "{} Image ({}x{} px)\n{} bytes • Double-click to View",
                    img.format_name, img.width, img.height, entry.size
                );
                self.preview_image = Some(img);
                return;
            }
        }

        if entry.name.ends_with(".app") {
            if let Some(m) = crate::user::apps::read_app_manifest(&full_path) {
                let kind = if m.builtin.is_some() {
                    "Built-in Desktop App"
                } else if m.terminal {
                    "CLI User App Bundle"
                } else {
                    "Ring-3 GUI App Bundle"
                };
                self.preview_text = format!(
                    "{} ({}) — {}\nExec: {} • Double-click to Launch",
                    m.name, kind, m.comment, m.exec
                );
                return;
            }
        }

        if libgui::codecs::is_audio_path(&entry.name) {
            let max_cap = entry.size.min(256 * 1024).max(4096);
            let mut audio_buf = Vec::new();
            {
                let vfs = VFS.lock();
                if let Ok(file) = vfs.open(&full_path, 0) {
                    let mut chunk = [0u8; 4096];
                    while audio_buf.len() < max_cap {
                        let want = (max_cap - audio_buf.len()).min(chunk.len());
                        match file.read(&mut chunk[..want]) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => audio_buf.extend_from_slice(&chunk[..n]),
                        }
                    }
                }
            }
            if !audio_buf.is_empty() {
                let meta = libgui::codecs::inspect_audio_metadata(&full_path, &audio_buf);
                self.preview_text = format!(
                    "{}: {} — {}\n{} Hz, {} ch • Double-click to Play in Music Player",
                    meta.format_name, meta.title, meta.artist, meta.sample_rate, meta.channels
                );
                return;
            }
        }

        let header = {
            let vfs = VFS.lock();
            if let Ok(file) = vfs.open(&full_path, 0) {
                let mut buf = [0u8; 4096];
                file.read(&mut buf).ok().map(|n| (buf, n))
            } else {
                None
            }
        };

        if let Some((buf, bytes_read)) = header {
            if crate::user::apps::is_elf64_magic(&buf[..bytes_read]) {
                let is_gui = crate::user::apps::elf_is_gui_client(&full_path);
                self.preview_text = format!(
                    "ELF64 User Executable ({}) — {} bytes\nDouble-click to Run",
                    if is_gui { "libgui / GUI" } else { "Terminal CLI" },
                    entry.size
                );
            } else if let Some((w, h, fps, kind)) = libgui::codecs::inspect_avi(&buf[..bytes_read]) {
                self.preview_text = format!(
                    "{} ({}x{} @ {} fps)\n{} bytes • Double-click to Play in Media Player",
                    kind, w, h, fps, entry.size
                );
            } else if let Some((iw, ih, kind)) = wallpaper::inspect_image_header(&buf[..bytes_read]) {
                self.preview_text = format!("{} ({}x{} px) • {} bytes", kind, iw, ih, entry.size);
            } else if let Ok(s) = core::str::from_utf8(&buf[..bytes_read]) {
                let single_line: String = s
                    .chars()
                    .take(240)
                    .map(|c| if c == '\r' || c == '\t' { ' ' } else { c })
                    .collect();
                self.preview_text = single_line;
            } else {
                self.preview_text = format!("Binary object ({} bytes)", entry.size);
            }
        }
    }

    pub fn new_folder(&mut self) {
        let mut idx = 1usize;
        let name = loop {
            let candidate = if idx == 1 {
                String::from("New Folder")
            } else {
                format!("New Folder {}", idx)
            };
            if !self.all_entries.iter().any(|e| e.name == candidate) {
                break candidate;
            }
            idx += 1;
        };
        let res = {
            let vfs = VFS.lock();
            if let Ok(dir) = vfs.resolve_path(&self.current_path) {
                dir.lock().create(&name, FileType::Directory).map(|_| ())
            } else {
                Err("Cannot resolve directory")
            }
        };
        match res {
            Ok(()) => {
                self.refresh_inner(false);
                if let Some(pos) = self.entries.iter().position(|e| e.name == name) {
                    self.selected_index = pos;
                    self.list_state.selected = Some(pos);
                    self.update_preview();
                    self.ensure_selected_visible();
                }
                self.status_msg = format!("Created folder '{}'", name);
            }
            Err(e) => self.status_msg = format!("New Folder failed: {}", e),
        }
    }

    pub fn new_file(&mut self) {
        let mut idx = 1usize;
        let name = loop {
            let candidate = if idx == 1 {
                String::from("New Document.txt")
            } else {
                format!("New Document {}.txt", idx)
            };
            if !self.all_entries.iter().any(|e| e.name == candidate) {
                break candidate;
            }
            idx += 1;
        };
        let res = {
            let vfs = VFS.lock();
            if let Ok(dir) = vfs.resolve_path(&self.current_path) {
                dir.lock().create(&name, FileType::Regular).map(|_| ())
            } else {
                Err("Cannot resolve directory")
            }
        };
        match res {
            Ok(()) => {
                self.refresh_inner(false);
                if let Some(pos) = self.entries.iter().position(|e| e.name == name) {
                    self.selected_index = pos;
                    self.list_state.selected = Some(pos);
                    self.update_preview();
                    self.ensure_selected_visible();
                }
                self.status_msg = format!("Created file '{}'", name);
            }
            Err(e) => self.status_msg = format!("New File failed: {}", e),
        }
    }

    pub fn begin_rename(&mut self) {
        let Some(entry) = self.entries.get(self.selected_index).cloned() else {
            return;
        };
        let full = join_path(&self.current_path, &entry.name);
        if is_protected_path(&full) {
            self.status_msg = format!("Protected system folder cannot be renamed: {}", full);
            return;
        }
        self.ensure_selected_visible();
        self.rename_index = Some(self.selected_index);
        self.rename_input = TextInputState::new(&entry.name);
        self.rename_input.focused = true;
        self.unfocus_inputs();
        self.context_menu.close();
        self.menu_state = MenuBarState::default();
    }

    fn try_commit_rename(&mut self) -> RenameOutcome {
        let Some(idx) = self.rename_index else {
            return RenameOutcome::UnchangedOrEmpty;
        };
        let Some(entry) = self.entries.get(idx).cloned() else {
            self.cancel_rename();
            return RenameOutcome::UnchangedOrEmpty;
        };
        let new_name = String::from(self.rename_input.text.trim());
        let old_name = entry.name;
        if new_name.is_empty() || new_name == old_name {
            self.cancel_rename();
            return RenameOutcome::UnchangedOrEmpty;
        }
        if new_name == "." || new_name == ".." || new_name.contains('/') || new_name.contains('\0') {
            self.status_msg = String::from("Invalid filename");
            return RenameOutcome::Invalid;
        }
        let old_full = join_path(&self.current_path, &old_name);
        if is_protected_path(&old_full) {
            self.cancel_rename();
            self.status_msg = format!("Protected system folder cannot be renamed: {}", old_full);
            return RenameOutcome::Failed;
        }
        let res = {
            let vfs = VFS.lock();
            if let Ok(dir) = vfs.resolve_path(&self.current_path) {
                dir.lock().rename(&old_name, &new_name)
            } else {
                Err("Cannot resolve directory")
            }
        };
        self.cancel_rename();
        match res {
            Ok(()) => {
                let new_full = join_path(&self.current_path, &new_name);
                if let Some((ref mut clip_path, _)) = self.clipboard {
                    if *clip_path == old_full {
                        *clip_path = new_full;
                    } else if let Some(rest) = clip_path.strip_prefix(&format!("{}/", old_full)) {
                        *clip_path = format!("{}/{}", new_full, rest);
                    }
                }
                if self.current_path == "/Trash" {
                    rename_trash_origin(&old_name, &new_name);
                }
                self.status_msg = format!("Renamed '{}' -> '{}'", old_name, new_name);
                self.refresh_inner(false);
                if let Some(pos) = self.entries.iter().position(|e| e.name == new_name) {
                    self.selected_index = pos;
                    self.list_state.selected = Some(pos);
                    self.update_preview();
                    self.ensure_selected_visible();
                }
                RenameOutcome::Committed
            }
            Err(e) => {
                self.status_msg = format!("Rename failed: {}", e);
                RenameOutcome::Failed
            }
        }
    }

    pub fn commit_rename(&mut self) {
        let outcome = self.try_commit_rename();
        if outcome == RenameOutcome::Invalid {
            self.cancel_rename();
        }
    }

    pub fn copy_selected_file(&mut self) {
        if let Some(full) = self.selected_full_path() {
            self.clipboard = Some((full.clone(), false));
            self.status_msg = format!("Copied: {}", full);
        }
    }

    pub fn cut_selected_file(&mut self) {
        if let Some(full) = self.selected_full_path() {
            if is_protected_path(&full) {
                self.status_msg = format!("Protected system folder cannot be moved: {}", full);
                return;
            }
            self.clipboard = Some((full.clone(), true));
            self.status_msg = format!("Cut: {}", full);
        }
    }

    pub fn paste_clipboard_file(&mut self) {
        let Some((src_path, is_cut)) = self.clipboard.clone() else {
            self.status_msg = String::from("Clipboard is empty");
            return;
        };
        if self.current_path == src_path || self.current_path.starts_with(&format!("{}/", src_path)) {
            self.status_msg = String::from("Cannot paste folder into itself");
            return;
        }
        if is_cut {
            if let Some((src_parent, _)) = split_parent_child(&src_path) {
                if src_parent == self.current_path {
                    self.clipboard = None;
                    self.status_msg = String::from("Item is already in this folder");
                    return;
                }
            }
        }
        let src_name = src_path.rsplit('/').next().unwrap_or("item");
        let dst_name = unique_name_in_dir(&self.current_path, src_name);
        let dst_path = join_path(&self.current_path, &dst_name);
        match copy_vfs_recursive(&src_path, &dst_path) {
            Ok(()) => {
                if is_cut {
                    if let Err(e) = unlink_vfs_path(&src_path) {
                        let _ = unlink_vfs_path(&dst_path);
                        self.status_msg = format!("Move failed: {}", e);
                        return;
                    }
                    self.clipboard = None;
                }
                self.refresh_inner(false);
                if let Some(pos) = self.entries.iter().position(|e| e.name == dst_name) {
                    self.selected_index = pos;
                    self.list_state.selected = Some(pos);
                    self.update_preview();
                    self.ensure_selected_visible();
                }
                self.status_msg = format!("Pasted '{}'", dst_name);
            }
            Err(e) => {
                self.status_msg = format!("Paste failed: {}", e);
            }
        }
    }

    fn clear_clipboard_if_matches(&mut self, target_path: &str) {
        if let Some((ref cp, _)) = self.clipboard {
            if cp == target_path || cp.starts_with(&format!("{}/", target_path)) {
                self.clipboard = None;
            }
        }
    }

    pub fn delete_selected(&mut self) {
        let Some(entry) = self.entries.get(self.selected_index).cloned() else {
            return;
        };
        let full = join_path(&self.current_path, &entry.name);
        if is_protected_path(&full) {
            self.status_msg = format!("Protected system folder cannot be deleted: {}", full);
            return;
        }
        if self.current_path != "/Trash" && VFS.lock().resolve_path("/Trash").is_ok() {
            let trash_name = unique_name_in_dir("/Trash", &entry.name);
            let dst = join_path("/Trash", &trash_name);
            if copy_vfs_recursive(&full, &dst).is_ok() {
                if unlink_vfs_path(&full).is_ok() {
                    record_trash_origin(&trash_name, &self.current_path);
                    self.clear_clipboard_if_matches(&full);
                    self.refresh_inner(false);
                    self.status_msg = format!("Moved '{}' to Trash", entry.name);
                    return;
                } else {
                    let _ = unlink_vfs_path(&dst);
                }
            }
        }
        match unlink_vfs_path(&full) {
            Ok(()) => {
                if self.current_path == "/Trash" {
                    let _ = take_trash_origin(&entry.name);
                }
                self.clear_clipboard_if_matches(&full);
                self.refresh_inner(false);
                self.status_msg = format!("Deleted '{}'", entry.name);
            }
            Err(e) => self.status_msg = format!("Delete failed: {}", e),
        }
    }

    pub fn restore_from_trash(&mut self) {
        if self.current_path != "/Trash" {
            return;
        }
        let Some(entry) = self.entries.get(self.selected_index).cloned() else {
            return;
        };
        let src = join_path("/Trash", &entry.name);
        let orig = take_trash_origin(&entry.name);
        let fallback_dir = "/Users/sublime/Documents";
        let dst_dir = match orig {
            Some(ref dir) => {
                let is_dir = {
                    let vfs = VFS.lock();
                    vfs.resolve_path(dir)
                        .map(|n| n.lock().stat().file_type == FileType::Directory)
                        .unwrap_or(false)
                };
                if is_dir { dir.as_str() } else { fallback_dir }
            }
            None => fallback_dir,
        };
        let dst_name = unique_name_in_dir(dst_dir, &entry.name);
        let dst = join_path(dst_dir, &dst_name);
        match copy_vfs_recursive(&src, &dst) {
            Ok(()) => {
                if let Err(e) = unlink_vfs_path(&src) {
                    let _ = unlink_vfs_path(&dst);
                    record_trash_origin(&entry.name, dst_dir);
                    self.status_msg = format!("Restore failed: {}", e);
                    return;
                }
                self.clear_clipboard_if_matches(&src);
                self.refresh_inner(false);
                self.status_msg = format!("Restored '{}' to {}", dst_name, dst_dir);
            }
            Err(e) => {
                record_trash_origin(&entry.name, dst_dir);
                self.status_msg = format!("Restore failed: {}", e);
            }
        }
    }

    pub fn empty_trash(&mut self) {
        let names: Vec<String> = {
            let vfs = VFS.lock();
            if let Ok(dir) = vfs.resolve_path("/Trash") {
                dir.lock()
                    .readdir()
                    .map(|l| l.into_iter().map(|e| e.name).collect())
                    .unwrap_or_default()
            } else {
                Vec::new()
            }
        };
        let mut count = 0usize;
        for n in names {
            let p = join_path("/Trash", &n);
            if unlink_vfs_path(&p).is_ok() {
                self.clear_clipboard_if_matches(&p);
                count += 1;
            }
        }
        clear_trash_origins();
        if self.current_path == "/Trash" {
            self.refresh_inner(false);
        }
        self.status_msg = format!("Emptied Trash ({} item(s))", count);
    }

    pub fn copy_selected_path(&mut self) {
        let path = self.selected_full_path().unwrap_or_else(|| self.current_path.clone());
        libgui::system::SystemClipboard::set_text(&path);
        self.status_msg = format!("Copied path: {}", path);
    }

    pub fn copy_current_folder_path(&mut self) {
        libgui::system::SystemClipboard::set_text(&self.current_path);
        self.status_msg = format!("Copied folder path: {}", self.current_path);
    }

    fn toggle_sort(&mut self, field: SortField) {
        if self.sort_field == field {
            self.sort_asc = !self.sort_asc;
        } else {
            self.sort_field = field;
            self.sort_asc = true;
        }
        self.last_click_index = None;
        self.apply_filter_and_sort_keep_scroll(false);
    }

    fn apply_menu_cmd(&mut self, cmd: u16) {
        if let Some(place) = FM_PLACE_SPECS.iter().find(|p| p.id == cmd) {
            self.navigate_to(place.path);
            return;
        }
        match cmd {
            1 | 4 | 5 | 6 | 20 | 21 | 22 | 24 | 30 | 31 | 32 => {
                self.show_properties = false;
                self.image_viewer = None;
            }
            23 => {
                self.image_viewer = None;
                self.properties_target_dir = self.entries.is_empty();
                self.show_properties = !self.show_properties;
                return;
            }
            26 => {
                self.image_viewer = None;
                self.properties_target_dir = true;
                self.show_properties = true;
                return;
            }
            _ => {}
        }
        match cmd {
            1 => self.open_selected(),
            3 => self.refresh(),
            4 => self.new_folder(),
            5 => self.new_file(),
            6 => self.delete_selected(),
            7 => self.copy_selected_path(),
            20 => self.set_selected_as_wallpaper(),
            21 => self.begin_rename(),
            22 => self.empty_trash(),
            24 => self.restore_from_trash(),
            25 => self.copy_current_folder_path(),
            30 => self.cut_selected_file(),
            31 => self.copy_selected_file(),
            32 => self.paste_clipboard_file(),
            40 => self.toggle_sort(SortField::Name),
            41 => self.toggle_sort(SortField::Type),
            42 => self.toggle_sort(SortField::Size),
            43 => {
                self.show_hidden = !self.show_hidden;
                self.apply_filter_and_sort_keep_scroll(false);
            }
            44 => {
                self.show_preview = !self.show_preview;
                let view_h = self
                    .layout(0, 0, self.last_viewport.0, self.last_viewport.1)
                    .list_rect
                    .inset(Insets::all(1))
                    .h
                    .max(self.list_state.row_height);
                self.list_state.clamp_scroll(self.entries.len(), view_h);
            }
            _ => {}
        }
    }

    pub fn handle_wheel(
        &mut self,
        px: i32,
        py: i32,
        delta_y: i32,
        inner_x: i32,
        inner_y: i32,
        w: i32,
        h: i32,
    ) -> bool {
        self.last_viewport = (w, h);
        if self.context_menu.open {
            self.context_menu.close();
            return true;
        }
        if self.menu_state.open_menu.is_some() {
            self.menu_state.open_menu = None;
            self.menu_state.hovered_item = None;
            return true;
        }
        if self.show_properties || self.image_viewer.is_some() {
            return false;
        }
        if self.rename_index.is_some() {
            self.cancel_rename();
            return true;
        }
        let layout = self.layout(inner_x, inner_y, w, h);
        let (res, _) = ListView::handle_event(
            &mut self.list_state,
            layout.list_rect,
            self.entries.len(),
            GuiEvent::MouseWheel { x: px, y: py, delta_y },
        );
        res.needs_redraw()
    }

    pub fn handle_mouse_move(
        &mut self,
        x: i32,
        y: i32,
        inner_x: i32,
        inner_y: i32,
        w: i32,
        h: i32,
    ) -> bool {
        self.last_viewport = (w, h);
        let layout = self.layout(inner_x, inner_y, w, h);

        if self.context_menu.open {
            let (cres, _) = ContextMenu::handle_event(
                &mut self.context_menu,
                layout.popup_bounds(),
                &self.context_items,
                GuiEvent::MouseMove { x, y },
            );
            return cres.needs_redraw();
        }

        let (mres, _) = MenuBar::handle_event(
            &mut self.menu_state,
            layout.menu_bar,
            FM_MENUS,
            GuiEvent::MouseMove { x, y },
        );
        if self.menu_state.open_menu.is_some() {
            return mres.needs_redraw();
        }

        if self.show_properties {
            let (_, btn_wall, btn_close) = layout.properties_rects();
            let mut changed = mres.needs_redraw();
            let show_wall = !self.properties_target_dir
                && self
                    .entries
                    .get(self.selected_index)
                    .map_or(false, |e| wallpaper::is_supported_image(&e.name));
            if show_wall {
                let (r, _) = Button::handle_event(&mut self.btn_props_wall, btn_wall, GuiEvent::MouseMove { x, y });
                changed |= r.needs_redraw();
            }
            let (r, _) = Button::handle_event(&mut self.btn_props_close, btn_close, GuiEvent::MouseMove { x, y });
            return changed | r.needs_redraw();
        }

        if self.image_viewer.is_some() {
            let (_, _, _, _, btn_wall, btn_close) = layout.viewer_rects();
            let (r1, _) = Button::handle_event(&mut self.btn_viewer_wall, btn_wall, GuiEvent::MouseMove { x, y });
            let (r2, _) = Button::handle_event(&mut self.btn_viewer_close, btn_close, GuiEvent::MouseMove { x, y });
            return mres.needs_redraw() | r1.needs_redraw() | r2.needs_redraw();
        }

        if self.rename_index.is_some() {
            return mres.needs_redraw();
        }

        let mut changed = mres.needs_redraw();
        self.sync_nav_button_disabled();
        let ev = GuiEvent::MouseMove { x, y };
        changed |= Button::handle_event(&mut self.btn_back, layout.btn_back, ev).0.needs_redraw();
        changed |= Button::handle_event(&mut self.btn_fwd, layout.btn_fwd, ev).0.needs_redraw();
        changed |= Button::handle_event(&mut self.btn_up, layout.btn_up, ev).0.needs_redraw();
        changed |= Button::handle_event(&mut self.btn_refresh, layout.btn_refresh, ev).0.needs_redraw();
        if let Some(pbr) = layout.preview_btn_rect {
            if self.preview_action().is_some() {
                changed |= Button::handle_event(&mut self.btn_preview_act, pbr, ev).0.needs_redraw();
            }
        }

        let next_hover = if layout.search_clear_rect.w > 0 && layout.search_clear_rect.contains(x, y) {
            Some(HoverTarget::SearchClear)
        } else if let Some(field) = layout.header_field_at(x, y) {
            Some(HoverTarget::ColHeader(field))
        } else if layout.sidebar_rect.contains(x, y) {
            (0..FM_PLACE_SPECS.len())
                .find(|&i| layout.sidebar_item_rect(i).map_or(false, |r| r.contains(x, y)))
                .map(HoverTarget::Sidebar)
        } else {
            None
        };
        if next_hover != self.hover_target {
            self.hover_target = next_hover;
            changed = true;
        }

        let (lres, _) = ListView::handle_event(
            &mut self.list_state,
            layout.list_rect,
            self.entries.len(),
            ev,
        );
        changed | lres.needs_redraw()
    }

    pub fn handle_mouse_up(
        &mut self,
        x: i32,
        y: i32,
        inner_x: i32,
        inner_y: i32,
        w: i32,
        h: i32,
    ) -> bool {
        self.last_viewport = (w, h);
        let layout = self.layout(inner_x, inner_y, w, h);
        let ev = GuiEvent::MouseUp { x, y, button: MouseButton::Left };

        let mut changed = false;
        if self.list_state.dragging_thumb && (self.context_menu.open || self.menu_state.open_menu.is_some()) {
            self.list_state.dragging_thumb = false;
            changed = true;
        }

        if self.context_menu.open || self.menu_state.open_menu.is_some() {
            return changed;
        }

        if self.show_properties {
            self.list_state.dragging_thumb = false;
            let (_, btn_wall, btn_close) = layout.properties_rects();
            let show_wall = !self.properties_target_dir
                && self
                    .entries
                    .get(self.selected_index)
                    .map_or(false, |e| wallpaper::is_supported_image(&e.name));
            let (rw, act_wall) = if show_wall {
                Button::handle_event(&mut self.btn_props_wall, btn_wall, ev)
            } else {
                (libgui::EventResult::Ignored, false)
            };
            let (rc, act_close) = Button::handle_event(&mut self.btn_props_close, btn_close, ev);
            changed |= rw.needs_redraw() | rc.needs_redraw();
            if act_wall {
                self.set_selected_as_wallpaper();
                changed = true;
            }
            if act_close {
                self.show_properties = false;
                self.btn_props_close = ButtonState::default();
                self.btn_props_wall = ButtonState::default();
                changed = true;
            }
            return changed;
        }

        if self.image_viewer.is_some() {
            self.list_state.dragging_thumb = false;
            let (_, _, _, _, btn_wall, btn_close) = layout.viewer_rects();
            let (rw, act_wall) = Button::handle_event(&mut self.btn_viewer_wall, btn_wall, ev);
            let (rc, act_close) = Button::handle_event(&mut self.btn_viewer_close, btn_close, ev);
            changed |= rw.needs_redraw() | rc.needs_redraw();
            if act_wall {
                if let Some((ref img_path, _)) = self.image_viewer {
                    match wallpaper::set_wallpaper_from_file(img_path) {
                        Ok((iw, ih)) => self.status_msg = format!("Wallpaper set ({}x{})", iw, ih),
                        Err(e) => self.status_msg = format!("Wallpaper failed: {}", e),
                    }
                    changed = true;
                }
            }
            if act_close {
                self.image_viewer = None;
                self.btn_viewer_close = ButtonState::default();
                self.btn_viewer_wall = ButtonState::default();
                changed = true;
            }
            return changed;
        }

        self.sync_nav_button_disabled();
        let (r1, fire_back) = Button::handle_event(&mut self.btn_back, layout.btn_back, ev);
        let (r2, fire_fwd) = Button::handle_event(&mut self.btn_fwd, layout.btn_fwd, ev);
        let (r3, fire_up) = Button::handle_event(&mut self.btn_up, layout.btn_up, ev);
        let (r4, fire_ref) = Button::handle_event(&mut self.btn_refresh, layout.btn_refresh, ev);
        changed |= r1.needs_redraw() | r2.needs_redraw() | r3.needs_redraw() | r4.needs_redraw();

        let preview_act = self.preview_action();
        let fire_prev = if let (Some(pbr), Some((pa, _))) = (layout.preview_btn_rect, preview_act) {
            let (rp, fired) = Button::handle_event(&mut self.btn_preview_act, pbr, ev);
            changed |= rp.needs_redraw();
            if fired { Some(pa) } else { None }
        } else {
            None
        };

        let (lres, _) = ListView::handle_event(
            &mut self.list_state,
            layout.list_rect,
            self.entries.len(),
            ev,
        );
        changed |= lres.needs_redraw();

        if fire_back {
            self.navigate_back();
            return true;
        }
        if fire_fwd {
            self.navigate_forward();
            return true;
        }
        if fire_up {
            self.navigate_parent();
            return true;
        }
        if fire_ref {
            self.refresh();
            return true;
        }
        if let Some(pa) = fire_prev {
            match pa {
                PreviewAction::Restore => self.restore_from_trash(),
                PreviewAction::SetWallpaper => self.set_selected_as_wallpaper(),
                PreviewAction::Open => self.open_selected(),
            }
            return true;
        }

        changed
    }

    pub fn handle_right_click(&mut self, x: i32, y: i32, inner_x: i32, inner_y: i32, w: i32, h: i32) {
        self.last_viewport = (w, h);
        self.menu_state = MenuBarState::default();
        self.unfocus_inputs();
        self.cancel_rename();

        if self.show_properties || self.image_viewer.is_some() {
            return;
        }

        let layout = self.layout(inner_x, inner_y, w, h);
        if !layout.list_rect.contains(x, y) {
            self.context_menu.close();
            return;
        }
        let content_rect = layout.content_rect();
        if !content_rect.contains(x, y) {
            self.context_menu.close();
            return;
        }

        let rh = self.list_state.row_height.max(1);
        let row_idx = ((y - content_rect.y + self.list_state.scroll_y) / rh) as usize;
        self.context_items.clear();

        if row_idx < self.entries.len() {
            self.last_click_index = None;
            self.selected_index = row_idx;
            self.list_state.selected = Some(row_idx);
            self.update_preview();

            let is_img = wallpaper::is_supported_image(&self.entries[row_idx].name);
            if self.current_path == "/Trash" {
                self.context_items.push(MenuItemSpec::action(24, "Restore", ""));
                self.context_items.push(MenuItemSpec::action(6, "Delete Permanently", "Del"));
                self.context_items.push(MenuItemSpec::sep());
                self.context_items.push(MenuItemSpec::action(23, "Properties", "Alt+Enter"));
                self.context_items.push(MenuItemSpec::sep());
                self.context_items.push(MenuItemSpec::action(22, "Empty Trash", ""));
                self.context_items.push(MenuItemSpec::action(3, "Refresh", "F5"));
            } else {
                self.context_items.push(MenuItemSpec::action(1, "Open", "Enter"));
                if is_img {
                    self.context_items.push(MenuItemSpec::action(20, "Set as Wallpaper", ""));
                }
                self.context_items.push(MenuItemSpec::sep());
                self.context_items.push(MenuItemSpec::action(30, "Cut", "Ctrl+X"));
                self.context_items.push(MenuItemSpec::action(31, "Copy", "Ctrl+C"));
                if self.clipboard.is_some() {
                    self.context_items.push(MenuItemSpec::action(32, "Paste", "Ctrl+V"));
                }
                self.context_items.push(MenuItemSpec::sep());
                self.context_items.push(MenuItemSpec::action(21, "Rename", "F2"));
                self.context_items.push(MenuItemSpec::action(7, "Copy Path", ""));
                self.context_items.push(MenuItemSpec::action(6, "Delete", "Del"));
                self.context_items.push(MenuItemSpec::sep());
                self.context_items.push(MenuItemSpec::action(23, "Properties", "Alt+Enter"));
            }
        } else if self.current_path == "/Trash" {
            self.context_items.push(MenuItemSpec::action(22, "Empty Trash", ""));
            self.context_items.push(MenuItemSpec::action(43, "Toggle Hidden Files", "Ctrl+H"));
            self.context_items.push(MenuItemSpec::action(3, "Refresh", "F5"));
            self.context_items.push(MenuItemSpec::sep());
            self.context_items.push(MenuItemSpec::action(26, "Folder Properties", ""));
        } else {
            if self.clipboard.is_some() {
                self.context_items.push(MenuItemSpec::action(32, "Paste", "Ctrl+V"));
            }
            self.context_items.push(MenuItemSpec::action(4, "New Folder", "Ctrl+N"));
            self.context_items.push(MenuItemSpec::action(5, "New Text File", ""));
            self.context_items.push(MenuItemSpec::sep());
            self.context_items.push(MenuItemSpec::action(25, "Copy Folder Path", ""));
            self.context_items.push(MenuItemSpec::action(43, "Toggle Hidden Files", "Ctrl+H"));
            self.context_items.push(MenuItemSpec::action(3, "Refresh", "F5"));
            self.context_items.push(MenuItemSpec::sep());
            self.context_items.push(MenuItemSpec::action(26, "Folder Properties", ""));
        }

        self.context_menu.show(x, y);
    }

    pub fn handle_click(&mut self, x: i32, y: i32, inner_x: i32, inner_y: i32, w: i32, h: i32) {
        self.last_viewport = (w, h);
        let layout = self.layout(inner_x, inner_y, w, h);
        let down_ev = GuiEvent::MouseDown { x, y, button: MouseButton::Left };

        // 1. Open right-click context menu absorbs all left clicks.
        if self.context_menu.open {
            let (_, cmd_opt) = ContextMenu::handle_event(
                &mut self.context_menu,
                layout.popup_bounds(),
                &self.context_items,
                down_ev,
            );
            if let Some(cmd) = cmd_opt {
                self.apply_menu_cmd(cmd);
            }
            return;
        }

        // 2. MenuBar header & open dropdown popup.
        let (mres, m_act) = MenuBar::handle_event(&mut self.menu_state, layout.menu_bar, FM_MENUS, down_ev);
        if mres.needs_redraw() || m_act.is_some() {
            self.cancel_rename();
            self.unfocus_inputs();
        }
        if let Some(cmd) = m_act {
            self.apply_menu_cmd(cmd);
            return;
        }
        if mres.needs_redraw() {
            return;
        }

        // 3. Properties modal.
        if self.show_properties {
            let (modal_r, btn_wall, btn_close) = layout.properties_rects();
            let show_wall = !self.properties_target_dir
                && self
                    .entries
                    .get(self.selected_index)
                    .map_or(false, |e| wallpaper::is_supported_image(&e.name));
            if show_wall && btn_wall.contains(x, y) {
                let _ = Button::handle_event(&mut self.btn_props_wall, btn_wall, down_ev);
                return;
            }
            if btn_close.contains(x, y) {
                let _ = Button::handle_event(&mut self.btn_props_close, btn_close, down_ev);
                return;
            }
            if !modal_r.contains(x, y) {
                self.show_properties = false;
                self.btn_props_wall = ButtonState::default();
                self.btn_props_close = ButtonState::default();
            }
            return;
        }

        // 4. Image Quick Viewer modal.
        if self.image_viewer.is_some() {
            let (_, _, _, _, btn_wall, btn_close) = layout.viewer_rects();
            if btn_wall.contains(x, y) {
                let _ = Button::handle_event(&mut self.btn_viewer_wall, btn_wall, down_ev);
            } else if btn_close.contains(x, y) {
                let _ = Button::handle_event(&mut self.btn_viewer_close, btn_close, down_ev);
            }
            return;
        }

        // 5. Inline rename active: click inside edits caret, click outside commits or cancels.
        if let Some(idx) = self.rename_index {
            if let Some(rr) = layout.rename_rect(idx, self.list_state.scroll_y, self.list_state.row_height) {
                let vis_rr = rr.intersect(layout.content_rect());
                if vis_rr.contains(x, y) {
                    let _ = TextInput::handle_event(&mut self.rename_input, rr, down_ev);
                    return;
                }
            }
            self.commit_rename();
            return;
        }

        // 6. Toolbar buttons (press state tracked; fires on MouseUp).
        self.sync_nav_button_disabled();
        if layout.btn_back.contains(x, y) {
            self.unfocus_inputs();
            let _ = Button::handle_event(&mut self.btn_back, layout.btn_back, down_ev);
            return;
        }
        if layout.btn_fwd.contains(x, y) {
            self.unfocus_inputs();
            let _ = Button::handle_event(&mut self.btn_fwd, layout.btn_fwd, down_ev);
            return;
        }
        if layout.btn_up.contains(x, y) {
            self.unfocus_inputs();
            let _ = Button::handle_event(&mut self.btn_up, layout.btn_up, down_ev);
            return;
        }
        if layout.btn_refresh.contains(x, y) {
            self.unfocus_inputs();
            let _ = Button::handle_event(&mut self.btn_refresh, layout.btn_refresh, down_ev);
            return;
        }

        // Address bar input.
        if layout.addr_rect.contains(x, y) {
            if self.search_focused {
                self.search_focused = false;
                self.search_input.focused = false;
            }
            self.addr_focused = true;
            self.addr_input.focused = true;
            let _ = TextInput::handle_event(&mut self.addr_input, layout.addr_rect, down_ev);
            return;
        }

        // Search clear [x] button or Search bar input.
        if layout.search_clear_rect.w > 0 && layout.search_clear_rect.contains(x, y) {
            if self.addr_focused {
                self.addr_focused = false;
                self.addr_input.focused = false;
                self.addr_input.set_text(&self.current_path);
            }
            self.search_input.set_text("");
            self.search_focused = true;
            self.search_input.focused = true;
            self.last_click_index = None;
            self.apply_filter_and_sort_keep_scroll(true);
            return;
        }
        if layout.search_rect.w > 0 && layout.search_rect.contains(x, y) {
            if self.addr_focused {
                self.addr_focused = false;
                self.addr_input.focused = false;
                self.addr_input.set_text(&self.current_path);
            }
            self.search_focused = true;
            self.search_input.focused = true;
            let _ = TextInput::handle_event(&mut self.search_input, layout.search_rect, down_ev);
            return;
        }

        // Clicking below toolbar unfocuses toolbar inputs.
        self.unfocus_inputs();

        // Sidebar item clicks (gated by visible sidebar_rect).
        if layout.sidebar_rect.contains(x, y) {
            for (i, place) in FM_PLACE_SPECS.iter().enumerate() {
                if layout.sidebar_item_rect(i).map_or(false, |r| r.contains(x, y)) {
                    self.navigate_to(place.path);
                    return;
                }
            }
        }

        // Column headers.
        if let Some(field) = layout.header_field_at(x, y) {
            self.toggle_sort(field);
            return;
        }

        // Preview action button.
        if let (Some(pbr), Some(_)) = (layout.preview_btn_rect, self.preview_action()) {
            if pbr.contains(x, y) {
                let _ = Button::handle_event(&mut self.btn_preview_act, pbr, down_ev);
                return;
            }
        }

        // ListView rows / scrollbar.
        if layout.list_rect.contains(x, y) {
            let content_rect = layout.content_rect();
            let (_, hit_idx) = ListView::handle_event(
                &mut self.list_state,
                layout.list_rect,
                self.entries.len(),
                down_ev,
            );
            if let Some(idx) = hit_idx {
                self.click_item(idx);
            } else if content_rect.contains(x, y) {
                self.last_click_index = None;
            }
        }
    }

    pub fn render(&mut self, canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32) {
        self.last_viewport = (w, h);
        self.sync_nav_button_disabled();
        let layout = self.layout(x, y, w, h);
        let theme = libgui::gfx::current_theme();
        let text_pri = Color::from_libgui(theme.text_primary);
        let text_sec = Color::from_libgui(theme.text_secondary);

        // Top menu bar & toolbar strip.
        canvas.fill_rect(layout.menu_bar.x, layout.menu_bar.y, layout.menu_bar.w, layout.menu_bar.h + layout.toolbar.h, Color::from_libgui(theme.face));
        canvas.with_surface(|surf| {
            MenuBar::draw_bar(surf, layout.menu_bar, FM_MENUS, self.menu_state, &theme);
            Button::draw(surf, layout.btn_back, "<", ButtonStyle::Normal, self.btn_back, &theme);
            Button::draw(surf, layout.btn_fwd, ">", ButtonStyle::Normal, self.btn_fwd, &theme);
            Button::draw(surf, layout.btn_up, "^", ButtonStyle::Normal, self.btn_up, &theme);
            Button::draw(surf, layout.btn_refresh, "R", ButtonStyle::Normal, self.btn_refresh, &theme);
            let addr_focus = self.addr_focused;
            TextInput::draw(surf, layout.addr_rect, &mut self.addr_input, "", addr_focus, &theme);
            if layout.search_rect.w > 0 {
                let search_focus = self.search_focused;
                if layout.search_clear_rect.w > 0 {
                    surf.push_clip(Rect::new(
                        layout.search_rect.x + 1,
                        layout.search_rect.y,
                        (layout.search_rect.w - 20).max(12),
                        layout.search_rect.h,
                    ));
                    TextInput::draw(surf, layout.search_rect, &mut self.search_input, "Search...", search_focus, &theme);
                    surf.pop_clip();
                    // Re-draw right border/clear chip so TextInput never overlaps the [x] button.
                    let bg_r = Rect::new(layout.search_rect.right() - 20, layout.search_rect.y + 1, 19, layout.search_rect.h - 2);
                    surf.fill_rect(bg_r, theme.input_bg);
                    let border_c = if search_focus { theme.frame_active } else { theme.input_border };
                    surf.draw_rect(layout.search_rect, border_c);
                    let cr = layout.search_clear_rect;
                    let hovered = self.hover_target == Some(HoverTarget::SearchClear);
                    let chip_bg = if hovered { theme.face_dark } else { theme.face };
                    surf.fill_rect(cr, chip_bg);
                    surf.draw_rect(cr, theme.ctrl_border);
                    with_default_font(|font| {
                        font.draw_text(surf, cr.x + 4, cr.y, "x", TextStyle::ui(theme.text_primary));
                    });
                } else {
                    TextInput::draw(surf, layout.search_rect, &mut self.search_input, "Search...", search_focus, &theme);
                }
            }
        });
        canvas.fill_rect(x, layout.toolbar.bottom() - 1, layout.toolbar.w, 1, Color::from_libgui(theme.border_shadow));

        // 1. Left Places & Computer Sidebar (clipped to sidebar_rect).
        if layout.sidebar_rect.w > 0 && layout.sidebar_rect.h > 0 {
            let sr = layout.sidebar_rect;
            canvas.set_clip_rect(sr.x, sr.y, sr.w, sr.h);
            canvas.draw_gradient_v(sr.x, sr.y, sr.w, sr.h, Color::from_libgui(theme.title_top), Color::from_libgui(theme.title_bot));

            let card_x = sr.x + 8;
            let card_w = sr.w - 16;
            let fav_count = FM_PLACE_SPECS.iter().filter(|p| p.is_favorite).count();
            let comp_count = FM_PLACE_SPECS.len() - fav_count;

            let card1_y = sr.y + 6;
            let card1_body_h = (fav_count as i32) * SIDEBAR_ROW_H + 2;
            canvas.fill_rect(card_x, card1_y, card_w, SIDEBAR_CARD_HDR_H, Color::from_libgui(theme.face));
            draw_ui_string_bold(canvas, card_x + 8, card1_y + 1, "Places", Color::from_libgui(theme.text_link));
            canvas.fill_rect(card_x, card1_y + SIDEBAR_CARD_HDR_H, card_w, card1_body_h, Color::from_libgui(theme.face_light));

            let card2_y = card1_y + SIDEBAR_CARD_HDR_H + card1_body_h + SIDEBAR_CARD_GAP;
            let card2_body_h = (comp_count as i32) * SIDEBAR_ROW_H + 2;
            canvas.fill_rect(card_x, card2_y, card_w, SIDEBAR_CARD_HDR_H, Color::from_libgui(theme.face));
            draw_ui_string_bold(canvas, card_x + 8, card2_y + 1, "Computer", Color::from_libgui(theme.text_link));
            canvas.fill_rect(card_x, card2_y + SIDEBAR_CARD_HDR_H, card_w, card2_body_h, Color::from_libgui(theme.face_light));

            for (i, place) in FM_PLACE_SPECS.iter().enumerate() {
                if let Some(rr) = layout.sidebar_item_rect(i) {
                    let is_curr = self.current_path == place.path;
                    let is_hov = self.hover_target == Some(HoverTarget::Sidebar(i));
                    if is_curr {
                        canvas.fill_rect(rr.x, rr.y, rr.w, rr.h, Color::from_libgui(theme.selection));
                    } else if is_hov {
                        canvas.fill_rect(rr.x, rr.y, rr.w, rr.h, Color::from_libgui(theme.face));
                    }
                    let fg = Color::from_libgui(if is_curr { theme.selection_text } else { theme.text_link });
                    draw_ui_string_ellipsis(canvas, rr.x + 6, rr.y + 1, place.label, fg, (rr.w - 10).max(20));
                }
            }
            canvas.clear_clip_rect();
        }

        // 2. Column Headers (`Name | Type | Size`) & File Pane.
        let hr = layout.header_rect;
        canvas.fill_rect(hr.x, hr.y, hr.w, hr.h, Color::from_libgui(theme.face_light));
        if let Some(HoverTarget::ColHeader(hf)) = self.hover_target {
            let (hx, hw) = match hf {
                SortField::Size if layout.show_size_col => (layout.size_x - 4, (layout.header_rect.right() - (layout.size_x - 4)).max(0)),
                SortField::Type if layout.show_type_col => {
                    let end = if layout.show_size_col { layout.size_x - 4 } else { layout.header_rect.right() };
                    (layout.type_x - 4, (end - (layout.type_x - 4)).max(0))
                }
                _ => {
                    let end = if layout.show_type_col {
                        layout.type_x - 4
                    } else if layout.show_size_col {
                        layout.size_x - 4
                    } else {
                        layout.header_rect.right()
                    };
                    (hr.x, (end - hr.x).max(0))
                }
            };
            if hw > 0 {
                canvas.fill_rect(hx, hr.y, hw, hr.h - 1, Color::from_libgui(theme.face));
            }
        }
        canvas.fill_rect(hr.x, hr.y + hr.h - 1, hr.w, 1, Color::from_libgui(theme.border_shadow));

        let arrow = if self.sort_asc { " ^" } else { " v" };
        let name_hdr = if self.sort_field == SortField::Name { format!("Name{}", arrow) } else { String::from("Name") };
        draw_ui_string_bold(canvas, hr.x + 8, hr.y + 2, &name_hdr, text_pri);
        if layout.show_type_col {
            let type_hdr = if self.sort_field == SortField::Type { format!("Type{}", arrow) } else { String::from("Type") };
            draw_ui_string_bold(canvas, layout.type_x, hr.y + 2, &type_hdr, text_pri);
        }
        if layout.show_size_col {
            let size_hdr = if self.sort_field == SortField::Size { format!("Size{}", arrow) } else { String::from("Size") };
            draw_ui_string_bold(canvas, layout.size_x, hr.y + 2, &size_hdr, text_pri);
        }

        // ListView rows.
        self.list_state.selected = if self.entries.is_empty() {
            None
        } else {
            Some(self.selected_index)
        };
        let cur_path = self.current_path.as_str();
        let entries = &self.entries;
        let clipboard = &self.clipboard;
        let rename_idx = self.rename_index;
        let show_type = layout.show_type_col;
        let show_size = layout.show_size_col;
        let type_x = layout.type_x;
        let size_x = layout.size_x;
        let content_right = layout.content_right;

        canvas.with_surface(|surf| {
            ListView::draw(surf, layout.list_rect, entries.len(), &mut self.list_state, &theme, |s, idx, row_r, sel| {
                let entry = &entries[idx];
                let full_p = join_path(cur_path, &entry.name);
                let is_cut = matches!(clipboard, Some((ref cp, true)) if cp == &full_p);
                let muted = libgui::gfx::Color::lerp(theme.text_primary, theme.input_bg, 110);
                let tc = if sel {
                    theme.selection_text
                } else if is_cut {
                    muted
                } else {
                    theme.text_primary
                };
                let tc_sec = if sel {
                    theme.selection_text
                } else if is_cut {
                    muted
                } else {
                    theme.text_secondary
                };
                if rename_idx == Some(idx) {
                    // Inline TextInput overlays the name column after ListView::draw finishes.
                } else {
                    let tag = entry_tag(entry);
                    let display_name = if is_cut {
                        format!("[CUT] {} {}", tag, entry.name)
                    } else {
                        format!("{} {}", tag, entry.name)
                    };
                    let name_limit = if show_type {
                        type_x - 6
                    } else if show_size {
                        size_x - 6
                    } else {
                        content_right - 4
                    };
                    let name_w = (name_limit - (row_r.x + 6)).max(24);
                    with_default_font(|font| {
                        let st_pri = TextStyle::ui(tc);
                        s.push_clip(Rect::new(row_r.x + 4, row_r.y, name_w, row_r.h));
                        font.draw_text(s, row_r.x + 6, row_r.y + 3, &display_name, st_pri);
                        s.pop_clip();
                    });
                }
                with_default_font(|font| {
                    let st_sec = TextStyle::ui(tc_sec);
                    if show_type {
                        s.push_clip(Rect::new(type_x, row_r.y, 80, row_r.h));
                        font.draw_text(s, type_x, row_r.y + 3, entry_type_label(entry), st_sec);
                        s.pop_clip();
                    }
                    if show_size {
                        let sz = format_size(entry);
                        s.push_clip(Rect::new(size_x, row_r.y, 58, row_r.h));
                        font.draw_text(s, size_x, row_r.y + 3, &sz, st_sec);
                        s.pop_clip();
                    }
                });
            });
        });

        // Empty folder / no-match message centered inside list_rect.
        if self.entries.is_empty() && layout.list_rect.h >= 24 {
            let msg = if self.search_input.text.is_empty() {
                String::from("This folder is empty")
            } else {
                format!("No items matching \"{}\"", self.search_input.text)
            };
            let lr = layout.list_rect;
            let mw = measure_ui_string(&msg);
            let tx = lr.x + ((lr.w - mw) / 2).max(8);
            let ty = lr.y + ((lr.h - FONT_HEIGHT) / 2).max(4);
            draw_ui_string_ellipsis(canvas, tx, ty, &msg, text_sec, (lr.w - 16).max(20));
        }

        // Inline TextInput for active rename.
        if let Some(ridx) = self.rename_index {
            if let Some(rr) = layout.rename_rect(ridx, self.list_state.scroll_y, self.list_state.row_height) {
                let clip_r = layout.content_rect();
                canvas.with_surface(|surf| {
                    surf.push_clip(clip_r);
                    TextInput::draw(surf, rr, &mut self.rename_input, "", true, &theme);
                    surf.pop_clip();
                });
            }
        }

        // 3. Bottom Details & Preview Strip.
        if let Some(pr) = layout.preview_rect {
            canvas.fill_rect(pr.x, pr.y, pr.w, pr.h, Color::from_libgui(theme.face_light));
            canvas.fill_rect(pr.x, pr.y, pr.w, 1, Color::from_libgui(theme.border_shadow));
            let title_max_w = match layout.preview_btn_rect {
                Some(pbr) => (pbr.x - pr.x - 16).max(40),
                None => (pr.w - 20).max(40),
            };
            draw_ui_string_ellipsis(
                canvas,
                pr.x + 10,
                pr.y + 4,
                "Details & Preview:",
                Color::from_libgui(theme.text_link),
                title_max_w,
            );

            if let (Some(pbr), Some((pa, lbl))) = (layout.preview_btn_rect, self.preview_action()) {
                let style = if pa == PreviewAction::SetWallpaper || pa == PreviewAction::Restore {
                    ButtonStyle::Primary
                } else {
                    ButtonStyle::Normal
                };
                canvas.with_surface(|surf| {
                    Button::draw(surf, pbr, lbl, style, self.btn_preview_act, &theme);
                });
            }

            if let Some(ref img) = self.preview_image {
                wallpaper::draw_decoded_image_fit(canvas, img, pr.x + 10, pr.y + 26, 62, 40);
                let mut py = pr.y + 28;
                for line in self.preview_text.lines().take(2) {
                    draw_ui_string_ellipsis(canvas, pr.x + 80, py, line, text_sec, (pr.w - 92).max(24));
                    py += FONT_HEIGHT + 2;
                }
            } else {
                let mut py = pr.y + 28;
                for line in self.preview_text.lines().take(2) {
                    draw_ui_string_ellipsis(canvas, pr.x + 10, py, line, text_sec, (pr.w - 20).max(24));
                    py += FONT_HEIGHT + 2;
                }
            }
        }

        // 4. Optional Image Quick Viewer Overlay.
        if let Some((ref img_path, ref img)) = self.image_viewer {
            let (panel, hdr, img_area, bottom, btn_wall, btn_close) = layout.viewer_rects();
            canvas.fill_rect(panel.x, panel.y, panel.w, panel.h, Color::rgb(18, 22, 30));
            canvas.fill_rect(hdr.x, hdr.y, hdr.w, hdr.h, Color::rgb(26, 32, 42));
            let file_name = img_path.rsplit('/').next().unwrap_or(img_path);
            let hdr_lbl = format!("{} ({}x{})", file_name, img.width, img.height);
            draw_ui_string_ellipsis(canvas, hdr.x + 10, hdr.y + 6, &hdr_lbl, Color::WHITE, (hdr.w - 20).max(40));
            wallpaper::draw_decoded_image_fit(canvas, img, img_area.x, img_area.y, img_area.w, img_area.h);
            canvas.fill_rect(bottom.x, bottom.y, bottom.w, bottom.h, Color::rgb(24, 29, 38));
            canvas.with_surface(|surf| {
                Button::draw(surf, btn_wall, "Set Wallpaper", ButtonStyle::Primary, self.btn_viewer_wall, &theme);
                Button::draw(surf, btn_close, "Close", ButtonStyle::Normal, self.btn_viewer_close, &theme);
            });
        }

        // 5. Optional Properties Modal.
        if self.show_properties {
            let (modal, btn_wall, btn_close) = layout.properties_rects();
            canvas.fill_rect(modal.x + 2, modal.y + 2, modal.w, modal.h, Color::argb(55, 0, 0, 0));
            canvas.fill_rect(modal.x, modal.y, modal.w, modal.h, Color::from_libgui(theme.face_light));
            canvas.draw_rect(modal.x, modal.y, modal.w, modal.h, Color::from_libgui(theme.frame_active));
            canvas.fill_rect(modal.x, modal.y, modal.w, 26, Color::from_libgui(theme.title_top));
            let modal_title = if self.properties_target_dir { "Folder Properties" } else { "Item Properties" };
            draw_ui_string_bold(canvas, modal.x + 10, modal.y + 4, modal_title, Color::WHITE);

            let clip_bottom = (modal.bottom() - 36).max(modal.y + 28);
            canvas.set_clip_rect(modal.x + 2, modal.y + 28, modal.w - 4, clip_bottom - (modal.y + 28));
            let mut show_wall_btn = false;
            if self.properties_target_dir {
                let fname = if self.current_path == "/" { "/" } else { self.current_path.rsplit('/').next().unwrap_or(&self.current_path) };
                let contents_s = format!("{} item(s)", self.entries.len());
                let inode_num = {
                    let vfs = VFS.lock();
                    vfs.resolve_path(&self.current_path)
                        .map(|n| n.lock().stat().inode)
                        .unwrap_or(0)
                };
                let inode_s = format!("#{}", inode_num);
                let rows = [
                    ("Folder:", fname),
                    ("Type:", "File Folder"),
                    ("Path:", self.current_path.as_str()),
                    ("Contents:", contents_s.as_str()),
                    ("Size:", "-"),
                    ("Inode:", inode_s.as_str()),
                ];
                let mut py = modal.y + 34;
                for (k, v) in rows {
                    draw_ui_string(canvas, modal.x + 12, py, k, text_sec);
                    draw_ui_string_ellipsis(canvas, modal.x + 80, py, v, text_pri, (modal.w - 92).max(40));
                    py += 20;
                }
            } else if let Some(entry) = self.entries.get(self.selected_index) {
                let full_p = join_path(&self.current_path, &entry.name);
                let size_s = format!("{} bytes ({})", entry.size, format_size(entry));
                let inode_s = format!("#{}", entry.inode);
                let kind_s = if entry.file_type == FileType::Directory {
                    "Directory"
                } else if entry.file_type == FileType::CharDevice || entry.file_type == FileType::BlockDevice {
                    "Device"
                } else if entry.name.ends_with(".app") {
                    "App Bundle"
                } else if entry.is_elf {
                    if crate::user::apps::elf_is_gui_client(&full_p) { "ELF64 GUI" } else { "ELF64 CLI" }
                } else {
                    "Regular File"
                };
                let rows = [
                    ("Name:", entry.name.as_str()),
                    ("Type:", entry_type_label(entry)),
                    ("Path:", full_p.as_str()),
                    ("Size:", size_s.as_str()),
                    ("Inode:", inode_s.as_str()),
                    ("Kind:", kind_s),
                ];
                let mut py = modal.y + 34;
                for (k, v) in rows {
                    draw_ui_string(canvas, modal.x + 12, py, k, text_sec);
                    draw_ui_string_ellipsis(canvas, modal.x + 80, py, v, text_pri, (modal.w - 92).max(40));
                    py += 20;
                }
                show_wall_btn = wallpaper::is_supported_image(&entry.name);
            }
            canvas.clear_clip_rect();

            canvas.with_surface(|surf| {
                if show_wall_btn {
                    Button::draw(surf, btn_wall, "Set as Wallpaper", ButtonStyle::Primary, self.btn_props_wall, &theme);
                }
                Button::draw(surf, btn_close, "Close", ButtonStyle::Normal, self.btn_props_close, &theme);
            });
        }

        // 6. Status Bar.
        let obj_count = format!("{} object(s)", self.entries.len());
        let right_note = if !self.status_msg.is_empty() {
            self.status_msg.clone()
        } else if w < 460 && !self.search_input.text.is_empty() {
            format!("Filter: \"{}\" - {}", self.search_input.text, self.current_path)
        } else {
            self.current_path.clone()
        };
        let stat_panes = [
            StatusPane { text: &obj_count, width: Length::Px(104) },
            StatusPane { text: right_note.as_str(), width: Length::Flex(1) },
        ];
        canvas.with_surface(|surf| {
            StatusBar::draw(surf, layout.status_rect, &stat_panes, &theme);
        });

        // 7. Right-Click Context Menu & MenuBar Popups on Top.
        let popup_bounds = layout.popup_bounds();
        canvas.with_surface(|surf| {
            ContextMenu::draw(surf, popup_bounds, &self.context_items, self.context_menu, &theme);
            MenuBar::draw_popup(surf, layout.menu_bar, FM_MENUS, self.menu_state, &theme);
        });
    }
}

impl FileManagerApp {
    pub fn cursor_at(&self, mx: i32, my: i32, ix: i32, iy: i32, w: i32, h: i32) -> CursorType {
        if self.context_menu.open || self.menu_state.open_menu.is_some() || self.show_properties || self.image_viewer.is_some() {
            return CursorType::Arrow;
        }
        let layout = self.layout(ix, iy, w, h);
        if let Some(idx) = self.rename_index {
            if let Some(rr) = layout.rename_rect(idx, self.list_state.scroll_y, self.list_state.row_height) {
                if rr.intersect(layout.content_rect()).contains(mx, my) {
                    return CursorType::IBeam;
                }
            }
            return CursorType::Arrow;
        }
        if layout.addr_rect.contains(mx, my) {
            return CursorType::IBeam;
        }
        if layout.search_rect.w > 0
            && layout.search_rect.contains(mx, my)
            && !layout.search_clear_rect.contains(mx, my)
        {
            return CursorType::IBeam;
        }
        CursorType::Arrow
    }

    pub fn handle_key_event(&mut self, key: KeyCode, ch: Option<char>, mods: Modifiers) {
        let lower_ch = ch.map(|c| c.to_ascii_lowercase());

        // 1. Open context menu.
        if self.context_menu.open {
            if key == KeyCode::Escape {
                self.context_menu.close();
            }
            return;
        }

        // 2. Open MenuBar dropdown.
        if self.menu_state.open_menu.is_some() {
            if key == KeyCode::Escape {
                self.menu_state.open_menu = None;
                self.menu_state.hovered_item = None;
            }
            return;
        }

        // 3. Properties modal.
        if self.show_properties {
            if key == KeyCode::Escape || (mods.alt && key == KeyCode::Enter) {
                self.show_properties = false;
                self.btn_props_close = ButtonState::default();
                self.btn_props_wall = ButtonState::default();
            }
            return;
        }

        // 4. Image viewer modal.
        if self.image_viewer.is_some() {
            if key == KeyCode::Escape {
                self.image_viewer = None;
                self.btn_viewer_close = ButtonState::default();
                self.btn_viewer_wall = ButtonState::default();
            } else if mods.alt && key == KeyCode::Enter {
                self.image_viewer = None;
                self.properties_target_dir = false;
                self.show_properties = true;
            }
            return;
        }

        // 5. Inline rename TextInput.
        if self.rename_index.is_some() {
            if key == KeyCode::Escape {
                self.cancel_rename();
                return;
            }
            let (_, submitted) = TextInput::handle_event(
                &mut self.rename_input,
                Rect::new(0, 0, 200, 22),
                GuiEvent::KeyDown { key, ch, mods },
            );
            if submitted {
                let _ = self.try_commit_rename();
            }
            return;
        }

        // Global File Explorer navigation/focus shortcuts (checked before focused inputs).
        if mods.ctrl && !mods.alt && lower_ch == Some('f') {
            if self.last_viewport.0 >= 460 {
                self.unfocus_inputs();
                self.search_focused = true;
                self.search_input.focused = true;
            }
            return;
        }
        if (mods.ctrl && !mods.alt && lower_ch == Some('l')) || (mods.alt && !mods.ctrl && lower_ch == Some('d')) {
            if self.search_focused {
                self.search_focused = false;
                self.search_input.focused = false;
            }
            self.addr_focused = true;
            self.addr_input.focused = true;
            return;
        }
        if key == KeyCode::Tab && !mods.ctrl && !mods.alt {
            let has_search = self.last_viewport.0 >= 460;
            if mods.shift {
                if self.search_focused {
                    self.search_focused = false;
                    self.search_input.focused = false;
                    self.addr_focused = true;
                    self.addr_input.focused = true;
                } else if self.addr_focused {
                    self.unfocus_inputs();
                } else if has_search {
                    self.search_focused = true;
                    self.search_input.focused = true;
                } else {
                    self.addr_focused = true;
                    self.addr_input.focused = true;
                }
            } else if self.addr_focused {
                self.unfocus_inputs();
                if has_search {
                    self.search_focused = true;
                    self.search_input.focused = true;
                }
            } else if self.search_focused {
                self.search_focused = false;
                self.search_input.focused = false;
            } else {
                self.addr_focused = true;
                self.addr_input.focused = true;
            }
            return;
        }
        if mods.alt && !mods.ctrl {
            match key {
                KeyCode::Left => {
                    self.navigate_back();
                    return;
                }
                KeyCode::Right => {
                    self.navigate_forward();
                    return;
                }
                KeyCode::Up => {
                    self.navigate_parent();
                    return;
                }
                KeyCode::Enter => {
                    self.properties_target_dir = self.entries.is_empty();
                    self.show_properties = !self.show_properties;
                    return;
                }
                _ => {}
            }
        }
        if mods.ctrl && !mods.alt {
            match lower_ch {
                Some('n') => {
                    self.unfocus_inputs();
                    self.new_folder();
                    return;
                }
                Some('h') => {
                    self.show_hidden = !self.show_hidden;
                    self.apply_filter_and_sort_keep_scroll(false);
                    return;
                }
                _ => {}
            }
        }
        if key == KeyCode::Function(5) && !mods.ctrl && !mods.alt {
            self.refresh();
            return;
        }

        // Focused toolbar inputs.
        if self.addr_focused {
            if key == KeyCode::Escape {
                self.unfocus_inputs();
                return;
            }
            let (_, submitted) = TextInput::handle_event(
                &mut self.addr_input,
                Rect::new(0, 0, 240, 22),
                GuiEvent::KeyDown { key, ch, mods },
            );
            if submitted {
                let dest = self.addr_input.text.clone();
                self.addr_focused = false;
                self.addr_input.focused = false;
                self.navigate_to(&dest);
            }
            return;
        }

        if self.search_focused {
            if key == KeyCode::Escape {
                self.search_focused = false;
                self.search_input.focused = false;
                self.search_input.set_text("");
                self.apply_filter_and_sort();
                return;
            }
            let prev_text = self.search_input.text.clone();
            let (_, _) = TextInput::handle_event(
                &mut self.search_input,
                Rect::new(0, 0, 120, 22),
                GuiEvent::KeyDown { key, ch, mods },
            );
            if prev_text != self.search_input.text {
                self.last_click_index = None;
                self.apply_filter_and_sort_keep_scroll(true);
            }
            return;
        }

        // ListView file clipboard shortcuts.
        if mods.ctrl && !mods.alt {
            match lower_ch {
                Some('c') => {
                    self.copy_selected_file();
                    return;
                }
                Some('x') => {
                    self.cut_selected_file();
                    return;
                }
                Some('v') => {
                    self.paste_clipboard_file();
                    return;
                }
                _ => {}
            }
        }

        self.handle_gui_key(key);
    }

    pub fn handle_gui_key(&mut self, key: KeyCode) {
        if self.context_menu.open {
            if key == KeyCode::Escape {
                self.context_menu.close();
            }
            return;
        }
        if self.menu_state.open_menu.is_some() {
            if key == KeyCode::Escape {
                self.menu_state.open_menu = None;
                self.menu_state.hovered_item = None;
            }
            return;
        }
        if self.show_properties {
            if key == KeyCode::Escape {
                self.show_properties = false;
            }
            return;
        }
        if self.image_viewer.is_some() {
            if key == KeyCode::Escape {
                self.image_viewer = None;
            }
            return;
        }
        if self.rename_index.is_some() {
            if key == KeyCode::Escape {
                self.cancel_rename();
            }
            return;
        }

        match key {
            KeyCode::Escape => {
                self.unfocus_inputs();
            }
            KeyCode::Function(2) => self.begin_rename(),
            KeyCode::Delete => self.delete_selected(),
            KeyCode::Up => {
                if self.selected_index > 0 {
                    self.selected_index -= 1;
                    self.list_state.selected = Some(self.selected_index);
                    self.update_preview();
                    self.ensure_selected_visible();
                }
            }
            KeyCode::Down => {
                if self.selected_index + 1 < self.entries.len() {
                    self.selected_index += 1;
                    self.list_state.selected = Some(self.selected_index);
                    self.update_preview();
                    self.ensure_selected_visible();
                }
            }
            KeyCode::Home => {
                if !self.entries.is_empty() && self.selected_index != 0 {
                    self.selected_index = 0;
                    self.list_state.selected = Some(0);
                    self.update_preview();
                }
                self.ensure_selected_visible();
            }
            KeyCode::End => {
                if let Some(last) = self.entries.len().checked_sub(1) {
                    if self.selected_index != last {
                        self.selected_index = last;
                        self.list_state.selected = Some(last);
                        self.update_preview();
                    }
                    self.ensure_selected_visible();
                }
            }
            KeyCode::PageUp => {
                if !self.entries.is_empty() {
                    let rh = self.list_state.row_height.max(1);
                    let view_h = self
                        .layout(0, 0, self.last_viewport.0, self.last_viewport.1)
                        .list_rect
                        .inset(Insets::all(1))
                        .h
                        .max(rh);
                    let step = (view_h / rh).max(1) as usize;
                    let next = self.selected_index.saturating_sub(step);
                    if next != self.selected_index {
                        self.selected_index = next;
                        self.list_state.selected = Some(next);
                        self.update_preview();
                    }
                    self.ensure_selected_visible();
                }
            }
            KeyCode::PageDown => {
                if let Some(last) = self.entries.len().checked_sub(1) {
                    let rh = self.list_state.row_height.max(1);
                    let view_h = self
                        .layout(0, 0, self.last_viewport.0, self.last_viewport.1)
                        .list_rect
                        .inset(Insets::all(1))
                        .h
                        .max(rh);
                    let step = (view_h / rh).max(1) as usize;
                    let next = (self.selected_index + step).min(last);
                    if next != self.selected_index {
                        self.selected_index = next;
                        self.list_state.selected = Some(next);
                        self.update_preview();
                    }
                    self.ensure_selected_visible();
                }
            }
            KeyCode::Enter => self.open_selected(),
            KeyCode::Backspace => self.navigate_parent(),
            KeyCode::Function(5) => self.refresh(),
            _ => {}
        }
    }

    pub fn handle_char(&mut self, ch: char) {
        let key = match ch {
            '\x08' | '\x7F' => KeyCode::Backspace,
            '\n' | '\r' => KeyCode::Enter,
            '\t' => KeyCode::Tab,
            '\x1B' => KeyCode::Escape,
            _ => KeyCode::from_byte(ch as u8),
        };
        let ch_opt = if ch >= ' ' && ch != '\x7F' { Some(ch) } else { None };
        self.handle_key_event(key, ch_opt, Modifiers::default());
    }
}

fn is_protected_path(path: &str) -> bool {
    matches!(
        path,
        "/"
            | "/Trash"
            | "/dev"
            | "/proc"
            | "/etc"
            | "/bin"
            | "/lib"
            | "/System"
            | "/Library"
            | "/Applications"
            | "/Users"
            | "/Users/sublime"
            | "/Users/cosmos"
            | "/Volumes"
            | "/private"
            | "/tmp"
            | "/var"
    )
}

fn join_path(dir: &str, name: &str) -> String {
    if dir == "/" {
        format!("/{}", name)
    } else {
        format!("{}/{}", dir.trim_end_matches('/'), name)
    }
}

fn normalize_vfs_path(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return String::from("/");
    }
    let mut parts: Vec<&str> = Vec::new();
    for seg in trimmed.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                let _ = parts.pop();
            }
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        String::from("/")
    } else {
        format!("/{}", parts.join("/"))
    }
}

fn unique_name_in_dir(dir_path: &str, base: &str) -> String {
    let existing: Vec<String> = {
        let vfs = VFS.lock();
        vfs.resolve_path(dir_path)
            .and_then(|d| d.lock().readdir())
            .map(|l| l.into_iter().map(|e| e.name).collect())
            .unwrap_or_default()
    };
    if !existing.iter().any(|n| n == base) {
        return String::from(base);
    }
    let (stem, ext) = if let Some(dot) = base.rfind('.') {
        (&base[..dot], &base[dot..])
    } else {
        (base, "")
    };
    for i in 2..200 {
        let cand = if ext.is_empty() {
            format!("{} ({})", stem, i)
        } else {
            format!("{} ({}){}", stem, i, ext)
        };
        if !existing.iter().any(|n| n == &cand) {
            return cand;
        }
    }
    format!("{}_copy", base)
}

fn split_parent_child(full_path: &str) -> Option<(String, String)> {
    let idx = full_path.rfind('/')?;
    let name = &full_path[idx + 1..];
    if name.is_empty() {
        return None;
    }
    let parent = if idx == 0 {
        String::from("/")
    } else {
        String::from(&full_path[..idx])
    };
    Some((parent, String::from(name)))
}

fn unlink_vfs_path(full_path: &str) -> Result<(), &'static str> {
    let (parent, name) = split_parent_child(full_path).ok_or("Invalid path")?;
    let vfs = VFS.lock();
    let dir = vfs.resolve_path(&parent)?;
    let res = dir.lock().unlink(&name);
    res
}

fn copy_vfs_recursive(src_path: &str, dst_path: &str) -> Result<(), &'static str> {
    let (is_dir, dir_children) = {
        let vfs = VFS.lock();
        let node = vfs.resolve_path(src_path)?;
        let st = node.lock().stat();
        if st.file_type == FileType::Directory {
            let children: Vec<String> = node.lock().readdir()?.into_iter().map(|e| e.name).collect();
            (true, children)
        } else {
            (false, Vec::new())
        }
    };

    let (dst_parent, dst_name) = split_parent_child(dst_path).ok_or("Invalid destination path")?;
    if is_dir {
        {
            let vfs = VFS.lock();
            let pnode = vfs.resolve_path(&dst_parent)?;
            let _ = pnode.lock().create(&dst_name, FileType::Directory)?;
        }
        for child in dir_children {
            copy_vfs_recursive(&join_path(src_path, &child), &join_path(dst_path, &child))?;
        }
        Ok(())
    } else {
        let data = {
            let vfs = VFS.lock();
            let file = vfs.open(src_path, 0)?;
            let mut out = Vec::with_capacity(file.stat().size as usize);
            let mut buf = [0u8; 4096];
            loop {
                let n = file.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                out.extend_from_slice(&buf[..n]);
            }
            out
        };
        let dst_node = {
            let vfs = VFS.lock();
            let pnode = vfs.resolve_path(&dst_parent)?;
            let created = pnode.lock().create(&dst_name, FileType::Regular)?;
            created
        };
        if !data.is_empty() {
            dst_node.lock().write(0, &data)?;
        }
        Ok(())
    }
}

fn entry_tag(e: &FileEntryInfo) -> &'static str {
    if e.file_type == FileType::Directory {
        "[DIR]"
    } else if e.file_type == FileType::CharDevice || e.file_type == FileType::BlockDevice {
        "[DEV]"
    } else if e.name.ends_with(".app") {
        "[APP]"
    } else if e.is_elf {
        "[BIN]"
    } else if wallpaper::is_supported_image(&e.name) {
        "[IMG]"
    } else if libgui::codecs::is_audio_path(&e.name) {
        "[AUD]"
    } else if libgui::codecs::is_video_path(&e.name) {
        "[VID]"
    } else if libgui::codecs::is_code_path(&e.name) {
        "[SRC]"
    } else if e.name.ends_with(".txt") || e.name.ends_with(".plist") {
        "[TXT]"
    } else {
        "[FIL]"
    }
}

fn entry_type_label(e: &FileEntryInfo) -> &'static str {
    let lower = e.name.to_ascii_lowercase();
    if e.file_type == FileType::Directory {
        "File Folder"
    } else if e.file_type == FileType::CharDevice || e.file_type == FileType::BlockDevice {
        "Device Node"
    } else if lower.ends_with(".app") {
        "Application"
    } else if e.is_elf {
        "ELF64 App"
    } else if lower.ends_with(".png") {
        "PNG Image"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "JPEG Image"
    } else if lower.ends_with(".qoi") {
        "QOI Image"
    } else if lower.ends_with(".bmp") {
        "BMP Image"
    } else if lower.ends_with(".tga") {
        "TGA Image"
    } else if lower.ends_with(".ppm") {
        "PPM Image"
    } else if lower.ends_with(".wav") {
        "WAV Audio"
    } else if lower.ends_with(".mp3") {
        "MP3 Audio"
    } else if lower.ends_with(".qoa") {
        "QOA Audio"
    } else if lower.ends_with(".avi") {
        "AVI Video"
    } else if lower.ends_with(".rs") {
        "Rust Source"
    } else if lower.ends_with(".c") || lower.ends_with(".h") || lower.ends_with(".cpp") {
        "C/C++ Source"
    } else if lower.ends_with(".py") {
        "Python Script"
    } else if lower.ends_with(".js") || lower.ends_with(".ts") {
        "JS / TS Source"
    } else if lower.ends_with(".json") {
        "JSON Data"
    } else if lower.ends_with(".sh") {
        "Shell Script"
    } else if lower.ends_with(".txt") || lower.ends_with(".md") {
        "Text Document"
    } else if lower.ends_with(".plist") {
        "Property List"
    } else {
        "File"
    }
}

fn format_size(e: &FileEntryInfo) -> String {
    if e.file_type == FileType::Directory {
        String::from("<DIR>")
    } else if e.size < 1024 {
        format!("{} B", e.size)
    } else {
        format!("{} KB", (e.size + 1023) / 1024)
    }
}
