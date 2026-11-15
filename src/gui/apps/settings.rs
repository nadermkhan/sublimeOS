use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use libgui::event::{GuiEvent, KeyCode, Modifiers, MouseButton};
use libgui::font::{with_default_font, TextStyle};
use libgui::gfx::{
    current_theme, current_theme_kind, set_theme_kind, Insets, Rect, ThemeKind,
};
use libgui::layout::VStack;
use libgui::widget::{
    Button, ButtonState, ButtonStyle, GroupBox, ListView, ListViewState, Slider, SliderState,
    TabBar, TabBarState, TextInput, TextInputState,
};
use crate::drivers::platform::smbios::get_hardware_telemetry;
use crate::drivers::vbe::VBE;
use crate::fs::vfs::VFS;
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::font::{
    draw_ui_string, draw_ui_string_bold, draw_ui_string_ellipsis, get_terminal_bold,
    get_ui_font_style,
    set_terminal_bold, set_ui_font_style, ui_font_style_name,
};
use crate::mm::pmm::PMM;
use crate::net::device::NET_MANAGER;
use crate::net::wifi::{WifiAccessPoint, WIFI_MANAGER};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    General = 0,
    Display = 1,
    Sound = 2,
    Network = 3,
    Storage = 4,
}

const TAB_LABELS: &[&str] = &["General", "Display", "Sound", "Network", "Storage"];

pub struct SettingsApp {
    pub active_tab: SettingsTab,
    pub tab_state: TabBarState,
    pub audio_volume: u8,
    pub vol_slider: SliderState,
    pub wifi_list: ListViewState,
    pub pass_input: TextInputState,
    pub status_feedback: String,
    pub selected_ap: Option<usize>,
    pub wifi_passphrase: String,
    pub pass_focused: bool,
    pub pending_close: bool,
    last_wall_gen: u64,
    last_theme_raw: u8,
    last_font_style: u8,
    last_wifi_sig: (bool, usize),
}

impl SettingsApp {
    pub fn new() -> Self {
        let vol = crate::drivers::audio::get_volume();
        let wifi_sig = {
            let wifi = WIFI_MANAGER.lock();
            (wifi.is_connected, wifi.scan_results.len())
        };
        Self {
            active_tab: SettingsTab::General,
            tab_state: TabBarState::new(0),
            audio_volume: vol,
            vol_slider: SliderState::new(vol as i32, 0, 100),
            wifi_list: ListViewState::new(21),
            pass_input: TextInputState::new(""),
            status_feedback: String::from("SublimeOS Control Panel Ready"),
            selected_ap: None,
            wifi_passphrase: String::new(),
            pass_focused: false,
            pending_close: false,
            last_wall_gen: crate::gui::wallpaper::wallpaper_generation(),
            last_theme_raw: current_theme_kind() as u8,
            last_font_style: get_ui_font_style(),
            last_wifi_sig: wifi_sig,
        }
    }

    pub fn sync_external_state(&mut self) -> bool {
        let mut dirty = false;
        let sys_vol = crate::drivers::audio::get_volume();
        if !self.vol_slider.dragging && sys_vol != self.audio_volume {
            self.audio_volume = sys_vol;
            self.vol_slider.value = sys_vol as i32;
            dirty = true;
        }
        let wg = crate::gui::wallpaper::wallpaper_generation();
        let tk = current_theme_kind() as u8;
        let fs = get_ui_font_style();
        let wifi_sig = {
            let wifi = WIFI_MANAGER.lock();
            (wifi.is_connected, wifi.scan_results.len())
        };
        if wg != self.last_wall_gen
            || tk != self.last_theme_raw
            || fs != self.last_font_style
            || wifi_sig != self.last_wifi_sig
        {
            self.last_wall_gen = wg;
            self.last_theme_raw = tk;
            self.last_font_style = fs;
            self.last_wifi_sig = wifi_sig;
            dirty = true;
        }
        dirty
    }

    pub fn handle_mouse_up(&mut self) -> bool {
        let mut changed = false;
        if self.vol_slider.dragging {
            self.vol_slider.dragging = false;
            changed = true;
        }
        if self.wifi_list.dragging_thumb {
            self.wifi_list.dragging_thumb = false;
            changed = true;
        }
        changed
    }

    pub fn set_tab(&mut self, tab: SettingsTab) {
        self.active_tab = tab;
        self.tab_state.active = tab as usize;
        self.pass_focused = false;
        self.pass_input.focused = false;
    }

    pub fn handle_char(&mut self, ch: char) {
        let key = match ch {
            '\x08' | '\x7F' => KeyCode::Backspace,
            '\n' | '\r' => KeyCode::Enter,
            _ => KeyCode::from_byte(ch as u8),
        };
        let ch_opt = if ch >= ' ' && ch != '\x7F' { Some(ch) } else { None };
        self.handle_key(key, ch_opt);
    }

    pub fn handle_key(&mut self, key: KeyCode, ch: Option<char>) {
        if self.active_tab != SettingsTab::Network || !self.pass_focused {
            return;
        }
        let (_, submitted) = TextInput::handle_event(
            &mut self.pass_input,
            Rect::new(0, 0, 200, 22),
            GuiEvent::KeyDown {
                key,
                ch,
                mods: Modifiers::default(),
            },
        );
        self.wifi_passphrase = self.pass_input.text.clone();
        if submitted {
            self.connect_selected_wifi();
        }
    }

    pub fn handle_wheel(&mut self, px: i32, py: i32, delta_y: i32, x: i32, y: i32, w: i32, h: i32) -> bool {
        if self.active_tab != SettingsTab::Network {
            return false;
        }
        let page = Rect::new(x + 10, y + 34, (w - 20).max(80), (h - 72).max(120));
        let inner = page.inset(Insets::all(12));
        let ap_rect = Rect::new(inner.x, inner.y + 102, inner.w, (inner.h - 140).clamp(48, 90));
        let count = WIFI_MANAGER.lock().scan_results.len();
        let (res, _) = ListView::handle_event(
            &mut self.wifi_list,
            ap_rect,
            count,
            GuiEvent::MouseWheel { x: px, y: py, delta_y },
        );
        res.needs_redraw()
    }

    fn connect_selected_wifi(&mut self) {
        let mut wifi = WIFI_MANAGER.lock();
        let target_ssid = self
            .selected_ap
            .and_then(|idx| wifi.scan_results.get(idx).map(|ap| ap.ssid.clone()))
            .or_else(|| wifi.scan_results.first().map(|ap| ap.ssid.clone()));

        let Some(ssid) = target_ssid else {
            self.status_feedback = String::from("Click 'Scan Wi-Fi' to discover networks first");
            return;
        };

        let pass = if self.pass_input.text.is_empty() {
            None
        } else {
            Some(self.pass_input.text.as_str())
        };
        match wifi.connect(&ssid, pass) {
            Ok(()) => {
                self.status_feedback = format!("Wi-Fi connected to '{}'", ssid);
            }
            Err(e) => {
                self.status_feedback = format!("Wi-Fi error: {}", e);
            }
        }
    }

    pub fn handle_mouse_move(
        &mut self,
        px: i32,
        py: i32,
        left_down: bool,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    ) -> bool {
        if self.active_tab == SettingsTab::Network {
            let page = Rect::new(x + 10, y + 34, (w - 20).max(80), (h - 72).max(120));
            let inner = page.inset(Insets::all(12));
            let ap_rect = Rect::new(inner.x, inner.y + 102, inner.w, (inner.h - 140).clamp(48, 90));
            let count = WIFI_MANAGER.lock().scan_results.len();
            let ev = if left_down {
                GuiEvent::MouseMove { x: px, y: py }
            } else {
                GuiEvent::MouseUp {
                    x: px,
                    y: py,
                    button: MouseButton::Left,
                }
            };
            let (res, _) = ListView::handle_event(&mut self.wifi_list, ap_rect, count, ev);
            return res.needs_redraw();
        }
        if self.active_tab != SettingsTab::Sound {
            self.vol_slider.dragging = false;
            return false;
        }
        if !left_down {
            let was = self.vol_slider.dragging;
            self.vol_slider.dragging = false;
            return was;
        }
        if !self.vol_slider.dragging {
            return false;
        }
        let page = Rect::new(x + 10, y + 34, (w - 20).max(80), (h - 72).max(120));
        let inner = page.inset(Insets::all(12));
        let slider_rect = Rect::new(inner.x + 110, inner.y + 68, (inner.w - 170).max(100), 20);
        let (_, changed) = Slider::handle_event(
            &mut self.vol_slider,
            slider_rect,
            GuiEvent::MouseMove { x: px, y: py },
        );
        if changed {
            self.audio_volume = self.vol_slider.value.clamp(0, 100) as u8;
            crate::drivers::audio::set_volume(self.audio_volume);
            self.status_feedback = format!("Master volume set to {}%", self.audio_volume);
        }
        changed
    }

    pub fn handle_click(&mut self, px: i32, py: i32, x: i32, y: i32, w: i32, h: i32) {
        let bot_y = y + h - 32;
        let ok_rect = Rect::new(x + w - 84, bot_y, 72, 22);
        if ok_rect.contains(px, py) {
            self.pending_close = true;
            return;
        }

        let tab_bar = Rect::new(x + 10, y + 8, (w - 20).max(80), 26);
        let (_, tab_hit) = TabBar::handle_event(
            &mut self.tab_state,
            tab_bar,
            TAB_LABELS,
            GuiEvent::MouseDown {
                x: px,
                y: py,
                button: MouseButton::Left,
            },
        );
        if let Some(idx) = tab_hit {
            let tab = match idx {
                0 => SettingsTab::General,
                1 => SettingsTab::Display,
                2 => SettingsTab::Sound,
                3 => SettingsTab::Network,
                _ => SettingsTab::Storage,
            };
            self.set_tab(tab);
            return;
        }

        let page = Rect::new(x + 10, y + 34, (w - 20).max(80), (h - 72).max(120));
        let inner = page.inset(Insets::all(12));

        match self.active_tab {
            SettingsTab::Display => {
                let mut vs = VStack::new(inner, 8);
                let _hdr = vs.next(24);
                let _rows = vs.next(104);
                let row1_y = vs.next(26).y;
                if py >= row1_y && py <= row1_y + 26 {
                    if px >= inner.x && px <= inner.x + 116 {
                        let next = (get_ui_font_style() + 1) % 3;
                        set_ui_font_style(next);
                        self.status_feedback = format!("Font: {}", ui_font_style_name(next));
                        return;
                    }
                    if px >= inner.x + 122 && px <= inner.x + 226 {
                        let next_bold = !get_terminal_bold();
                        set_terminal_bold(next_bold);
                        self.status_feedback = format!(
                            "Terminal font: {}",
                            if next_bold { "Bold" } else { "Regular 8x16" }
                        );
                        return;
                    }
                    if px >= inner.x + 232 && px <= inner.x + 342 {
                        let next_theme = match current_theme_kind() {
                            ThemeKind::SublimeClassic => ThemeKind::SublimeSlate,
                            ThemeKind::SublimeSlate => ThemeKind::SublimeClassic,
                        };
                        set_theme_kind(next_theme);
                        self.status_feedback = format!("Theme: {}", next_theme.name());
                        return;
                    }
                }
                let row2_y = vs.next(26).y;
                if py >= row2_y && py <= row2_y + 26 {
                    if px >= inner.x && px <= inner.x + 128 {
                        let name = crate::gui::wallpaper::cycle_wallpaper();
                        self.status_feedback = format!("Wallpaper: {}", name);
                        return;
                    }
                    if px >= inner.x + 134 && px <= inner.x + 232 {
                        let fit = crate::gui::wallpaper::cycle_wallpaper_fit();
                        self.status_feedback = format!("Wallpaper Fit: {}", fit.name());
                        return;
                    }
                    if px >= inner.x + 238 && px <= inner.x + 350 {
                        let imgs: Vec<String> = {
                            let vfs = VFS.lock();
                            if let Ok(dir) = vfs.resolve_path("/Users/sublime/Pictures") {
                                if let Ok(entries) = dir.lock().readdir() {
                                    entries
                                        .into_iter()
                                        .filter(|e| crate::gui::wallpaper::is_supported_image(&e.name))
                                        .map(|e| format!("/Users/sublime/Pictures/{}", e.name))
                                        .collect()
                                } else {
                                    Vec::new()
                                }
                            } else {
                                Vec::new()
                            }
                        };
                        if imgs.is_empty() {
                            self.status_feedback = String::from("No images in /Users/sublime/Pictures");
                        } else {
                            let curr = crate::gui::wallpaper::wallpaper_path_or_name();
                            let pos = imgs.iter().position(|p| p == &curr);
                            let next_path = &imgs[pos.map(|i| (i + 1) % imgs.len()).unwrap_or(0)];
                            match crate::gui::wallpaper::set_wallpaper_from_file(next_path) {
                                Ok((iw, ih)) => {
                                    self.status_feedback = format!("Wallpaper: {} ({}x{})", next_path, iw, ih);
                                }
                                Err(e) => {
                                    self.status_feedback = format!("Wallpaper error: {}", e);
                                }
                            }
                        }
                        return;
                    }
                }
                let row3_y = vs.next(26).y;
                if py >= row3_y && py <= row3_y + 26 && px >= inner.x + 218 && px <= inner.x + 350 {
                    let on = crate::gui::wallpaper::toggle_dynamic_palette();
                    self.status_feedback = format!(
                        "Material You Color: {}",
                        if on { "Active (Wallpaper Seed)" } else { "Default" }
                    );
                }
            }
            SettingsTab::Sound => {
                let slider_rect = Rect::new(inner.x + 110, inner.y + 68, (inner.w - 170).max(100), 20);
                let (_, changed) = Slider::handle_event(
                    &mut self.vol_slider,
                    slider_rect,
                    GuiEvent::MouseDown {
                        x: px,
                        y: py,
                        button: MouseButton::Left,
                    },
                );
                if changed || slider_rect.contains(px, py) {
                    self.audio_volume = self.vol_slider.value.clamp(0, 100) as u8;
                    crate::drivers::audio::set_volume(self.audio_volume);
                    self.status_feedback = format!("Master volume set to {}%", self.audio_volume);
                    return;
                }
                let btn_r = Rect::new(inner.x + 110, inner.y + 106, 128, 26);
                if btn_r.contains(px, py) {
                    crate::drivers::audio::play_sublime_chime();
                    self.status_feedback = format!("Playing SublimeOS Chime via {}", crate::drivers::audio::active_device_name());
                }
            }
            SettingsTab::Network => {
                let btn_y = inner.y + 28;
                if py >= btn_y && py <= btn_y + 24 {
                    if px >= inner.x && px <= inner.x + 96 {
                        let mut wifi = WIFI_MANAGER.lock();
                        let count = wifi.scan().len();
                        if self.selected_ap.is_none() && count > 0 {
                            self.selected_ap = Some(0);
                            self.wifi_list.selected = Some(0);
                        }
                        self.status_feedback = format!("Wi-Fi scan found {} access points", count);
                        return;
                    }
                    if px >= inner.x + 104 && px <= inner.x + 206 {
                        let is_conn = WIFI_MANAGER.lock().is_connected;
                        if is_conn {
                            WIFI_MANAGER.lock().disconnect();
                            self.status_feedback = String::from("Wi-Fi disconnected");
                        } else {
                            self.connect_selected_wifi();
                        }
                        return;
                    }
                    if px >= inner.x + 214 && px <= inner.x + 318 {
                        let target_iface = {
                            let net = NET_MANAGER.lock();
                            net.get_default_device().map(|(d, _)| String::from(d.name()))
                        };
                        if let Some(iface) = target_iface {
                            if let Ok(frame) = crate::net::dhcp::start_dhcp_discovery(&iface) {
                                let dev = NET_MANAGER.lock().get_device(&iface).map(|(d, _)| d);
                                if let Some(d) = dev {
                                    let _ = d.send_packet(&frame);
                                }
                                self.status_feedback = format!("Sent DHCP Discover on {}", iface);
                            } else {
                                self.status_feedback = String::from("No active NIC for DHCP");
                            }
                        } else {
                            self.status_feedback = String::from("No network adapter registered");
                        }
                        return;
                    }
                }

                let pass_rect = Rect::new(inner.x + 86, inner.bottom() - 24, (inner.w - 86).max(110), 22);
                let (_, _) = TextInput::handle_event(
                    &mut self.pass_input,
                    pass_rect,
                    GuiEvent::MouseDown {
                        x: px,
                        y: py,
                        button: MouseButton::Left,
                    },
                );
                self.pass_focused = self.pass_input.focused;
                if self.pass_focused {
                    return;
                }

                let ap_rect = Rect::new(inner.x, inner.y + 102, inner.w, (inner.h - 140).clamp(48, 90));
                let count = WIFI_MANAGER.lock().scan_results.len();
                let (_, hit_idx) = ListView::handle_event(
                    &mut self.wifi_list,
                    ap_rect,
                    count,
                    GuiEvent::MouseDown {
                        x: px,
                        y: py,
                        button: MouseButton::Left,
                    },
                );
                if let Some(idx) = hit_idx {
                    self.selected_ap = Some(idx);
                    if let Some(ap) = WIFI_MANAGER.lock().scan_results.get(idx) {
                        self.status_feedback = format!("Selected Wi-Fi AP: {}", ap.ssid);
                    }
                }
            }
            _ => {}
        }
    }

    pub fn render(&mut self, canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32) {
        let theme = current_theme();
        let face = Color::from_libgui(theme.face);
        let text_pri = Color::from_libgui(theme.text_primary);
        let text_sec = Color::from_libgui(theme.text_secondary);
        let link = Color::from_libgui(theme.text_link);
        let border = Color::from_libgui(theme.ctrl_border);

        canvas.fill_rect(x, y, w, h, face);

        let tab_bar = Rect::new(x + 10, y + 8, (w - 20).max(80), 26);
        let page = Rect::new(x + 10, y + 34, (w - 20).max(80), (h - 72).max(120));
        let inner = page.inset(Insets::all(12));

        canvas.with_surface(|surf| {
            TabBar::draw(surf, tab_bar, TAB_LABELS, self.tab_state, &theme);
            surf.fill_rect(page, theme.face_light);
            surf.draw_rect(page, theme.ctrl_border);
        });

        let mut cy = inner.y;
        match self.active_tab {
            SettingsTab::General => {
                let hw = get_hardware_telemetry();
                let grp = Rect::new(inner.x, inner.y, inner.w, 168);
                canvas.with_surface(|surf| {
                    GroupBox::draw(surf, grp, "SublimeOS System Properties", Some(theme.face_light), &theme);
                });
                cy += 24;

                let rows: [(&str, &str); 6] = [
                    ("System:", "SublimeOS 1.0.0 (KiteKernel x86_64)"),
                    ("Computer:", &hw.model_name),
                    ("Processor:", &hw.chip),
                    ("Cores:", &hw.cores_threads),
                    ("Memory:", &hw.memory_summary),
                    ("Firmware:", &hw.system_firmware),
                ];
                for (label, val) in rows.iter() {
                    draw_ui_string(canvas, inner.x + 12, cy, label, text_sec);
                    draw_ui_string_ellipsis(canvas, inner.x + 114, cy, val, text_pri, (inner.w - 126).max(40));
                    cy += 22;
                }
            }
            SettingsTab::Display => {
                let (gw, gh, gbpp, gpitch) = {
                    let v = VBE.lock();
                    (v.width, v.height, v.bpp, v.pitch)
                };
                let mut vs = VStack::new(inner, 8);
                let hdr = vs.next(24);
                draw_ui_string_bold(canvas, hdr.x, hdr.y, "Display, Wallpaper & Typography", text_pri);
                canvas.fill_rect(hdr.x, hdr.bottom() - 2, hdr.w, 1, Color::from_libgui(theme.border_shadow));

                let list_area = vs.next(104);
                let res_str = format!("{} x {} ({} bpp, Pitch {})", gw, gh, gbpp, gpitch);
                let style_name = ui_font_style_name(get_ui_font_style());
                let term_mode = if get_terminal_bold() { "Bold" } else { "Regular 8x16" };
                let theme_font = format!("{} / {}", theme.kind.name(), style_name);
                let wall_lbl = crate::gui::wallpaper::wallpaper_label();
                let fit_lbl = format!("{} (Term: {})", crate::gui::wallpaper::wallpaper_fit().name(), term_mode);
                let rows: [(&str, &str); 4] = [
                    ("Resolution:", res_str.as_str()),
                    ("Theme & Font:", theme_font.as_str()),
                    ("Wallpaper:", wall_lbl.as_str()),
                    ("Fit Mode:", fit_lbl.as_str()),
                ];
                let val_max_w = if inner.w >= 390 { (inner.w - 230).max(60) } else { (inner.w - 118).max(40) };
                let mut ry = list_area.y;
                for (label, val) in rows.iter() {
                    draw_ui_string(canvas, list_area.x, ry, label, text_sec);
                    draw_ui_string_ellipsis(canvas, list_area.x + 110, ry, val, text_pri, val_max_w);
                    ry += 23;
                }
                if inner.w >= 390 {
                    crate::gui::wallpaper::draw_thumbnail(
                        canvas,
                        inner.right() - 108,
                        list_area.y + 4,
                        104,
                        66,
                        &theme,
                    );
                }

                let row1 = vs.next(26);
                let row2 = vs.next(26);
                let row3 = vs.next(26);
                let fit_btn = format!("Fit: {}", crate::gui::wallpaper::wallpaper_fit().name());
                let dyn_on = crate::gui::wallpaper::is_dynamic_palette_enabled();
                let dyn_btn = if dyn_on { "Material You: ON" } else { "Material You: OFF" };
                let swatches = crate::gui::wallpaper::wallpaper_palette_swatches();
                canvas.with_surface(|surf| {
                    Button::draw(
                        surf,
                        Rect::new(row1.x, row1.y, 116, 24),
                        "Cycle UI Font",
                        ButtonStyle::Normal,
                        ButtonState::default(),
                        &theme,
                    );
                    Button::draw(
                        surf,
                        Rect::new(row1.x + 122, row1.y, 104, 24),
                        "Term Bold",
                        ButtonStyle::Normal,
                        ButtonState::default(),
                        &theme,
                    );
                    Button::draw(
                        surf,
                        Rect::new(row1.x + 232, row1.y, 110, 24),
                        "Light / Dark",
                        ButtonStyle::Normal,
                        ButtonState::default(),
                        &theme,
                    );
                    Button::draw(
                        surf,
                        Rect::new(row2.x, row2.y, 128, 24),
                        "Next Wallpaper",
                        ButtonStyle::Primary,
                        ButtonState::default(),
                        &theme,
                    );
                    Button::draw(
                        surf,
                        Rect::new(row2.x + 134, row2.y, 98, 24),
                        &fit_btn,
                        ButtonStyle::Normal,
                        ButtonState::default(),
                        &theme,
                    );
                    Button::draw(
                        surf,
                        Rect::new(row2.x + 238, row2.y, 112, 24),
                        "Pictures BMP",
                        ButtonStyle::Normal,
                        ButtonState::default(),
                        &theme,
                    );
                    Button::draw(
                        surf,
                        Rect::new(row3.x + 218, row3.y, 132, 24),
                        dyn_btn,
                        if dyn_on { ButtonStyle::Primary } else { ButtonStyle::Normal },
                        ButtonState::default(),
                        &theme,
                    );
                });
                draw_ui_string(canvas, row3.x, row3.y + 3, "Palette:", text_sec);
                for (si, sw_col) in swatches.iter().enumerate() {
                    let sx = row3.x + 56 + (si as i32) * 30;
                    canvas.fill_rect(sx, row3.y + 3, 24, 18, *sw_col);
                    canvas.draw_rect(sx, row3.y + 3, 24, 18, border);
                }
            }
            SettingsTab::Sound => {
                draw_ui_string_bold(canvas, inner.x, cy, "Sounds and Audio Devices", text_pri);
                cy += 22;
                canvas.fill_rect(inner.x, cy, inner.w, 1, Color::from_libgui(theme.border_shadow));
                cy += 12;

                if !self.vol_slider.dragging {
                    let sys_vol = crate::drivers::audio::get_volume();
                    self.audio_volume = sys_vol;
                    self.vol_slider.value = sys_vol as i32;
                }
                draw_ui_string(canvas, inner.x, cy, "Audio Device:", text_sec);
                draw_ui_string_ellipsis(canvas, inner.x + 110, cy, crate::drivers::audio::active_device_name(), link, (inner.w - 110).max(40));
                cy += 34;

                draw_ui_string(canvas, inner.x, cy, "Master Volume:", text_sec);
                self.vol_slider.value = self.audio_volume as i32;
                let slider_rect = Rect::new(inner.x + 110, inner.y + 68, (inner.w - 170).max(100), 20);
                let vol_text = format!("{}%", self.audio_volume);
                let vol_s = self.vol_slider;
                let chime_btn = Rect::new(inner.x + 110, inner.y + 106, 128, 26);
                canvas.with_surface(|surf| {
                    Slider::draw(surf, slider_rect, vol_s, &theme);
                    with_default_font(|font| {
                        font.draw_text(surf, slider_rect.right() + 10, slider_rect.y + 2, &vol_text, TextStyle::ui(theme.text_primary));
                    });
                    Button::draw(surf, chime_btn, "Play Chime", ButtonStyle::Primary, ButtonState::default(), &theme);
                });
            }
            SettingsTab::Network => {
                let (wifi_conn, wifi_ssid, wifi_spd, wifi_aps): (bool, Option<String>, u32, Vec<WifiAccessPoint>) = {
                    let wlock = WIFI_MANAGER.lock();
                    (
                        wlock.is_connected,
                        wlock.current_ssid.clone(),
                        wlock.link_speed_mbps,
                        wlock.scan_results.clone(),
                    )
                };

                let status_line = if wifi_conn {
                    format!(
                        "Wireless: Connected to {} ({} Mbps)",
                        wifi_ssid.as_deref().unwrap_or("WLAN"),
                        wifi_spd
                    )
                } else {
                    String::from("Wireless: Ready (Click Scan Wi-Fi)")
                };
                draw_ui_string_bold(
                    canvas,
                    inner.x,
                    cy,
                    &status_line,
                    if wifi_conn { Color::from_libgui(theme.good) } else { text_pri },
                );

                let btn_y = inner.y + 28;
                let net_summary = {
                    let net = NET_MANAGER.lock();
                    let mut parts = String::new();
                    for (dev, cfg) in &net.devices {
                        if !parts.is_empty() {
                            parts.push_str(" | ");
                        }
                        let st = if dev.is_link_up() { "UP" } else { "DN" };
                        parts.push_str(&format!(
                            "{}({}):{}.{}.{}.{}",
                            dev.name(),
                            st,
                            cfg.ipv4[0],
                            cfg.ipv4[1],
                            cfg.ipv4[2],
                            cfg.ipv4[3]
                        ));
                    }
                    if parts.is_empty() {
                        String::from("No NICs registered")
                    } else {
                        parts
                    }
                };
                draw_ui_string_ellipsis(canvas, inner.x, inner.y + 58, &net_summary, link, inner.w);
                draw_ui_string_bold(canvas, inner.x, inner.y + 82, "Available 802.11 Networks:", text_sec);

                let ap_rect = Rect::new(inner.x, inner.y + 102, inner.w, (inner.h - 140).clamp(48, 90));
                let pass_rect = Rect::new(inner.x + 86, inner.bottom() - 24, (inner.w - 86).max(110), 22);
                self.wifi_list.selected = self.selected_ap;

                draw_ui_string(canvas, inner.x, inner.bottom() - 20, "Passkey:", text_sec);

                let wifi_list = &mut self.wifi_list;
                let pass_input = &mut self.pass_input;
                canvas.with_surface(|surf| {
                    Button::draw(
                        surf,
                        Rect::new(inner.x, btn_y, 96, 22),
                        "Scan Wi-Fi",
                        ButtonStyle::Normal,
                        ButtonState::default(),
                        &theme,
                    );
                    Button::draw(
                        surf,
                        Rect::new(inner.x + 104, btn_y, 102, 22),
                        if wifi_conn { "Disconnect" } else { "Connect" },
                        ButtonStyle::Primary,
                        ButtonState::default(),
                        &theme,
                    );
                    Button::draw(
                        surf,
                        Rect::new(inner.x + 214, btn_y, 104, 22),
                        "DHCP Renew",
                        ButtonStyle::Normal,
                        ButtonState::default(),
                        &theme,
                    );

                    ListView::draw(surf, ap_rect, wifi_aps.len(), wifi_list, &theme, |s, idx, row_r, sel| {
                        let ap = &wifi_aps[idx];
                        let row = format!("{} Ch{} {}dBm {}", ap.ssid, ap.channel, ap.rssi, ap.security.as_str());
                        ListView::draw_simple_row(s, row_r, &row, sel, &theme);
                    });

                    TextInput::draw(
                        surf,
                        pass_rect,
                        pass_input,
                        "Click to type WPA2 key + Enter",
                        true,
                        &theme,
                    );
                });
            }
            SettingsTab::Storage => {
                draw_ui_string_bold(canvas, inner.x, cy, "Mounted Storage & Memory Cache", text_pri);
                cy += 22;
                canvas.fill_rect(inner.x, cy, inner.w, 1, Color::from_libgui(theme.border_shadow));
                cy += 12;

                let (used_mb, total_mb) = {
                    let pmm = PMM.lock();
                    ((pmm.get_used_frames() * 4) / 1024, (pmm.get_total_frames() * 4) / 1024)
                };
                let vols = VFS
                    .lock()
                    .resolve_path("/Volumes")
                    .and_then(|node| node.lock().readdir())
                    .unwrap_or_default();
                let mut rows: Vec<(String, String)> = Vec::new();
                rows.push((String::from("Root VFS:"), String::from("/ (ramfs + devfs + tmpfs)")));
                if vols.is_empty() {
                    rows.push((String::from("Volumes:"), String::from("No external partitions detected")));
                } else {
                    for entry in vols.iter().take(4) {
                        rows.push((String::from("Mounted:"), format!("/Volumes/{}", entry.name)));
                    }
                }
                rows.push((String::from("Physical RAM:"), format!("{} MB / {} MB allocated", used_mb, total_mb)));

                for (label, val) in rows.iter() {
                    draw_ui_string(canvas, inner.x, cy, label, text_sec);
                    draw_ui_string_ellipsis(canvas, inner.x + 114, cy, val, text_pri, (inner.w - 114).max(40));
                    cy += 24;
                }
            }
        }

        let bot_y = y + h - 32;
        draw_ui_string_ellipsis(canvas, x + 12, bot_y + 2, &self.status_feedback, text_sec, w - 104);
        let ok_rect = Rect::new(x + w - 84, bot_y, 72, 22);
        let _ = border;
        canvas.with_surface(|surf| {
            Button::draw(surf, ok_rect, "OK", ButtonStyle::Normal, ButtonState::default(), &theme);
        });
    }
}
