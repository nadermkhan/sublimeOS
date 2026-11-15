use alloc::string::String;
use alloc::vec::Vec;
use crate::event::{GuiEvent, KeyCode, Modifiers, MouseButton};

pub const WM_SOCKET_PATH: &str = "/run/wm.sock";

const MAX_FRAME_LEN: usize = 65536;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WmClientMsg {
    CreateWindow {
        title: String,
        width: i32,
        height: i32,
        min_width: i32,
        min_height: i32,
        resizable: bool,
    },
    AttachShm {
        window_id: u32,
        width: i32,
        height: i32,
        stride_bytes: u32,
        shm_name: String,
    },
    CommitDamage {
        window_id: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    },
    SetTitle {
        window_id: u32,
        title: String,
    },
    CloseWindow {
        window_id: u32,
    },
    SetClipboard {
        text: String,
    },
    Hello {
        pid: u32,
        app_name: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WmServerMsg {
    Welcome {
        client_id: u32,
        theme_kind: u8,
    },
    Configure {
        window_id: u32,
        width: i32,
        height: i32,
        theme_kind: u8,
    },
    Input {
        window_id: u32,
        event: GuiEvent,
    },
    CloseRequested {
        window_id: u32,
    },
    ThemeChanged {
        theme_kind: u8,
    },
    ClipboardChanged {
        text: String,
    },
    PaletteChanged {
        theme_kind: u8,
        seed_rgb: u32,
    },
}

fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn push_i32(out: &mut Vec<u8>, v: i32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn push_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn push_str(out: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let len = bytes.len().min(4096) as u16;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&bytes[..len as usize]);
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn u8(&mut self) -> Option<u8> {
        let b = *self.data.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }

    fn u16(&mut self) -> Option<u16> {
        let s = self.data.get(self.pos..self.pos + 2)?;
        self.pos += 2;
        Some(u16::from_le_bytes([s[0], s[1]]))
    }

    fn u32(&mut self) -> Option<u32> {
        let s = self.data.get(self.pos..self.pos + 4)?;
        self.pos += 4;
        Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }

    fn i32(&mut self) -> Option<i32> {
        self.u32().map(|v| v as i32)
    }

    fn u64(&mut self) -> Option<u64> {
        let s = self.data.get(self.pos..self.pos + 8)?;
        self.pos += 8;
        Some(u64::from_le_bytes([
            s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7],
        ]))
    }

    fn str(&mut self) -> Option<String> {
        let len = self.u16()? as usize;
        let s = self.data.get(self.pos..self.pos + len)?;
        self.pos += len;
        core::str::from_utf8(s).ok().map(String::from)
    }
}

fn wrap_frame(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

/// Extract the next complete frame payload `[opcode, ...]` from a stream buffer.
pub fn take_next_frame(rx_buf: &mut Vec<u8>) -> Option<Vec<u8>> {
    if rx_buf.len() < 4 {
        return None;
    }
    let len = u32::from_le_bytes([rx_buf[0], rx_buf[1], rx_buf[2], rx_buf[3]]) as usize;
    if len == 0 || len > MAX_FRAME_LEN {
        rx_buf.clear();
        return None;
    }
    if rx_buf.len() < 4 + len {
        return None;
    }
    let payload = rx_buf[4..4 + len].to_vec();
    rx_buf.drain(..4 + len);
    Some(payload)
}

pub fn encode_client_msg(msg: &WmClientMsg) -> Vec<u8> {
    let mut p = Vec::with_capacity(64);
    match msg {
        WmClientMsg::CreateWindow {
            title,
            width,
            height,
            min_width,
            min_height,
            resizable,
        } => {
            p.push(1);
            push_i32(&mut p, *width);
            push_i32(&mut p, *height);
            push_i32(&mut p, *min_width);
            push_i32(&mut p, *min_height);
            p.push(if *resizable { 1 } else { 0 });
            push_str(&mut p, title);
        }
        WmClientMsg::AttachShm {
            window_id,
            width,
            height,
            stride_bytes,
            shm_name,
        } => {
            p.push(2);
            push_u32(&mut p, *window_id);
            push_i32(&mut p, *width);
            push_i32(&mut p, *height);
            push_u32(&mut p, *stride_bytes);
            push_str(&mut p, shm_name);
        }
        WmClientMsg::CommitDamage {
            window_id,
            x,
            y,
            width,
            height,
        } => {
            p.push(3);
            push_u32(&mut p, *window_id);
            push_i32(&mut p, *x);
            push_i32(&mut p, *y);
            push_i32(&mut p, *width);
            push_i32(&mut p, *height);
        }
        WmClientMsg::SetTitle { window_id, title } => {
            p.push(4);
            push_u32(&mut p, *window_id);
            push_str(&mut p, title);
        }
        WmClientMsg::CloseWindow { window_id } => {
            p.push(5);
            push_u32(&mut p, *window_id);
        }
        WmClientMsg::SetClipboard { text } => {
            p.push(6);
            push_str(&mut p, text);
        }
        WmClientMsg::Hello { pid, app_name } => {
            p.push(7);
            push_u32(&mut p, *pid);
            push_str(&mut p, app_name);
        }
    }
    wrap_frame(&p)
}

pub fn decode_client_msg(payload: &[u8]) -> Option<WmClientMsg> {
    let mut r = Reader::new(payload);
    match r.u8()? {
        1 => {
            let width = r.i32()?;
            let height = r.i32()?;
            let min_width = r.i32()?;
            let min_height = r.i32()?;
            let resizable = r.u8()? != 0;
            let title = r.str()?;
            Some(WmClientMsg::CreateWindow {
                title,
                width,
                height,
                min_width,
                min_height,
                resizable,
            })
        }
        2 => {
            let window_id = r.u32()?;
            let width = r.i32()?;
            let height = r.i32()?;
            let stride_bytes = r.u32()?;
            let shm_name = r.str()?;
            Some(WmClientMsg::AttachShm {
                window_id,
                width,
                height,
                stride_bytes,
                shm_name,
            })
        }
        3 => Some(WmClientMsg::CommitDamage {
            window_id: r.u32()?,
            x: r.i32()?,
            y: r.i32()?,
            width: r.i32()?,
            height: r.i32()?,
        }),
        4 => Some(WmClientMsg::SetTitle {
            window_id: r.u32()?,
            title: r.str()?,
        }),
        5 => Some(WmClientMsg::CloseWindow {
            window_id: r.u32()?,
        }),
        6 => Some(WmClientMsg::SetClipboard { text: r.str()? }),
        7 => Some(WmClientMsg::Hello {
            pid: r.u32()?,
            app_name: r.str()?,
        }),
        _ => None,
    }
}

fn encode_mods(mods: Modifiers) -> u8 {
    (if mods.shift { 1 } else { 0 })
        | (if mods.ctrl { 2 } else { 0 })
        | (if mods.alt { 4 } else { 0 })
}

fn decode_mods(bits: u8) -> Modifiers {
    Modifiers {
        shift: (bits & 1) != 0,
        ctrl: (bits & 2) != 0,
        alt: (bits & 4) != 0,
    }
}

fn encode_keycode(out: &mut Vec<u8>, key: KeyCode) {
    match key {
        KeyCode::Char(c) => {
            out.push(1);
            push_u32(out, c as u32);
        }
        KeyCode::Enter => out.push(2),
        KeyCode::Backspace => out.push(3),
        KeyCode::Delete => out.push(4),
        KeyCode::Tab => out.push(5),
        KeyCode::Escape => out.push(6),
        KeyCode::Up => out.push(7),
        KeyCode::Down => out.push(8),
        KeyCode::Left => out.push(9),
        KeyCode::Right => out.push(10),
        KeyCode::Home => out.push(11),
        KeyCode::End => out.push(12),
        KeyCode::PageUp => out.push(13),
        KeyCode::PageDown => out.push(14),
        KeyCode::Function(n) => {
            out.push(15);
            out.push(n);
        }
        KeyCode::Unknown(code) => {
            out.push(16);
            out.extend_from_slice(&code.to_le_bytes());
        }
    }
}

fn decode_keycode(r: &mut Reader<'_>) -> Option<KeyCode> {
    match r.u8()? {
        1 => char::from_u32(r.u32()?).map(KeyCode::Char),
        2 => Some(KeyCode::Enter),
        3 => Some(KeyCode::Backspace),
        4 => Some(KeyCode::Delete),
        5 => Some(KeyCode::Tab),
        6 => Some(KeyCode::Escape),
        7 => Some(KeyCode::Up),
        8 => Some(KeyCode::Down),
        9 => Some(KeyCode::Left),
        10 => Some(KeyCode::Right),
        11 => Some(KeyCode::Home),
        12 => Some(KeyCode::End),
        13 => Some(KeyCode::PageUp),
        14 => Some(KeyCode::PageDown),
        15 => Some(KeyCode::Function(r.u8()?)),
        16 => Some(KeyCode::Unknown(r.u16()?)),
        _ => None,
    }
}

fn encode_event(out: &mut Vec<u8>, ev: GuiEvent) {
    match ev {
        GuiEvent::MouseMove { x, y } => {
            out.push(1);
            push_i32(out, x);
            push_i32(out, y);
        }
        GuiEvent::MouseDown { x, y, button } => {
            out.push(2);
            push_i32(out, x);
            push_i32(out, y);
            out.push(button as u8);
        }
        GuiEvent::MouseUp { x, y, button } => {
            out.push(3);
            push_i32(out, x);
            push_i32(out, y);
            out.push(button as u8);
        }
        GuiEvent::MouseWheel { x, y, delta_y } => {
            out.push(4);
            push_i32(out, x);
            push_i32(out, y);
            push_i32(out, delta_y);
        }
        GuiEvent::KeyDown { key, ch, mods } => {
            out.push(5);
            encode_keycode(out, key);
            push_u32(out, ch.map(|c| c as u32).unwrap_or(0));
            out.push(encode_mods(mods));
        }
        GuiEvent::KeyUp { key, mods } => {
            out.push(6);
            encode_keycode(out, key);
            out.push(encode_mods(mods));
        }
        GuiEvent::FocusChanged { focused } => {
            out.push(7);
            out.push(if focused { 1 } else { 0 });
        }
        GuiEvent::Resize { width, height } => {
            out.push(8);
            push_i32(out, width);
            push_i32(out, height);
        }
        GuiEvent::Tick { now_ms } => {
            out.push(9);
            push_u64(out, now_ms);
        }
    }
}

fn decode_button(b: u8) -> MouseButton {
    match b {
        2 => MouseButton::Right,
        3 => MouseButton::Middle,
        _ => MouseButton::Left,
    }
}

fn decode_event(r: &mut Reader<'_>) -> Option<GuiEvent> {
    match r.u8()? {
        1 => Some(GuiEvent::MouseMove {
            x: r.i32()?,
            y: r.i32()?,
        }),
        2 => Some(GuiEvent::MouseDown {
            x: r.i32()?,
            y: r.i32()?,
            button: decode_button(r.u8()?),
        }),
        3 => Some(GuiEvent::MouseUp {
            x: r.i32()?,
            y: r.i32()?,
            button: decode_button(r.u8()?),
        }),
        4 => Some(GuiEvent::MouseWheel {
            x: r.i32()?,
            y: r.i32()?,
            delta_y: r.i32()?,
        }),
        5 => {
            let key = decode_keycode(r)?;
            let ch_raw = r.u32()?;
            let ch = if ch_raw == 0 {
                None
            } else {
                char::from_u32(ch_raw)
            };
            let mods = decode_mods(r.u8()?);
            Some(GuiEvent::KeyDown { key, ch, mods })
        }
        6 => {
            let key = decode_keycode(r)?;
            let mods = decode_mods(r.u8()?);
            Some(GuiEvent::KeyUp { key, mods })
        }
        7 => Some(GuiEvent::FocusChanged {
            focused: r.u8()? != 0,
        }),
        8 => Some(GuiEvent::Resize {
            width: r.i32()?,
            height: r.i32()?,
        }),
        9 => Some(GuiEvent::Tick { now_ms: r.u64()? }),
        _ => None,
    }
}

pub fn encode_server_msg(msg: &WmServerMsg) -> Vec<u8> {
    let mut p = Vec::with_capacity(48);
    match msg {
        WmServerMsg::Welcome {
            client_id,
            theme_kind,
        } => {
            p.push(101);
            push_u32(&mut p, *client_id);
            p.push(*theme_kind);
        }
        WmServerMsg::Configure {
            window_id,
            width,
            height,
            theme_kind,
        } => {
            p.push(102);
            push_u32(&mut p, *window_id);
            push_i32(&mut p, *width);
            push_i32(&mut p, *height);
            p.push(*theme_kind);
        }
        WmServerMsg::Input { window_id, event } => {
            p.push(103);
            push_u32(&mut p, *window_id);
            encode_event(&mut p, *event);
        }
        WmServerMsg::CloseRequested { window_id } => {
            p.push(104);
            push_u32(&mut p, *window_id);
        }
        WmServerMsg::ThemeChanged { theme_kind } => {
            p.push(105);
            p.push(*theme_kind);
        }
        WmServerMsg::ClipboardChanged { text } => {
            p.push(106);
            push_str(&mut p, text);
        }
        WmServerMsg::PaletteChanged {
            theme_kind,
            seed_rgb,
        } => {
            p.push(107);
            p.push(*theme_kind);
            push_u32(&mut p, *seed_rgb);
        }
    }
    wrap_frame(&p)
}

pub fn decode_server_msg(payload: &[u8]) -> Option<WmServerMsg> {
    let mut r = Reader::new(payload);
    match r.u8()? {
        101 => Some(WmServerMsg::Welcome {
            client_id: r.u32()?,
            theme_kind: r.u8()?,
        }),
        102 => Some(WmServerMsg::Configure {
            window_id: r.u32()?,
            width: r.i32()?,
            height: r.i32()?,
            theme_kind: r.u8()?,
        }),
        103 => Some(WmServerMsg::Input {
            window_id: r.u32()?,
            event: decode_event(&mut r)?,
        }),
        104 => Some(WmServerMsg::CloseRequested {
            window_id: r.u32()?,
        }),
        105 => Some(WmServerMsg::ThemeChanged {
            theme_kind: r.u8()?,
        }),
        106 => Some(WmServerMsg::ClipboardChanged { text: r.str()? }),
        107 => Some(WmServerMsg::PaletteChanged {
            theme_kind: r.u8()?,
            seed_rgb: r.u32()?,
        }),
        _ => None,
    }
}
