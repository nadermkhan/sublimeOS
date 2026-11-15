use alloc::format;
use alloc::vec::Vec;
use crate::app::WindowClient;
use crate::event::{EventResult, GuiEvent};
use crate::gfx::{Rect, Surface, ThemeKind};
use crate::ipc::protocol::{
    decode_server_msg, encode_client_msg, take_next_frame, WmClientMsg, WmServerMsg, WM_SOCKET_PATH,
};
use crate::system::SystemClipboard;

/// Platform syscall adapter implemented by Ring-3 `sublime_rt` (or a test mock).
pub trait WmOs {
    fn connect_unix(&mut self, path: &str) -> Result<usize, i32>;
    fn send_all(&mut self, fd: usize, data: &[u8]) -> Result<usize, i32>;
    /// Non-blocking socket read: `Ok(0)` on peer EOF, `Err(-11)` on EAGAIN.
    fn recv_nonblocking(&mut self, fd: usize, buf: &mut [u8]) -> Result<usize, i32>;
    /// Open/create `/dev/shm/<name>` returning an open FD.
    fn open_shm_rw_create(&mut self, name: &str) -> Result<usize, i32>;
    fn ftruncate(&mut self, fd: usize, len: usize) -> Result<(), i32>;
    fn mmap_shared(&mut self, fd: usize, len: usize) -> Result<*mut u8, i32>;
    fn munmap(&mut self, ptr: *mut u8, len: usize) -> Result<(), i32>;
    fn close(&mut self, fd: usize);
    fn getpid(&mut self) -> u32;
    fn sleep_ms(&mut self, ms: u64);
}

struct ShmBuffer {
    fd: usize,
    ptr: *mut u8,
    mapped_bytes: usize,
    width: i32,
    height: i32,
}

impl ShmBuffer {
    fn allocate<O: WmOs>(os: &mut O, name: &str, width: i32, height: i32) -> Result<Self, i32> {
        let w = width.max(1);
        let h = height.max(1);
        let raw_bytes = (w as usize) * (h as usize) * 4;
        let mapped_bytes = (raw_bytes + 4095) & !4095;
        let fd = os.open_shm_rw_create(name)?;
        if let Err(e) = os.ftruncate(fd, mapped_bytes) {
            os.close(fd);
            return Err(e);
        }
        let ptr = match os.mmap_shared(fd, mapped_bytes) {
            Ok(p) => p,
            Err(e) => {
                os.close(fd);
                return Err(e);
            }
        };
        Ok(Self {
            fd,
            ptr,
            mapped_bytes,
            width: w,
            height: h,
        })
    }

    fn pixels_mut(&mut self) -> &mut [u32] {
        let count = (self.width as usize) * (self.height as usize);
        unsafe { core::slice::from_raw_parts_mut(self.ptr as *mut u32, count) }
    }

    fn release<O: WmOs>(self, os: &mut O) {
        let _ = os.munmap(self.ptr, self.mapped_bytes);
        os.close(self.fd);
    }
}

/// Runs a `WindowClient` over `/run/wm.sock` and `/dev/shm/*` until closed.
pub fn run_app<O: WmOs>(client: &mut dyn WindowClient, os: &mut O) -> Result<(), i32> {
    let mut sock_fd = Err(-111);
    for _ in 0..40 {
        sock_fd = os.connect_unix(WM_SOCKET_PATH);
        if sock_fd.is_ok() {
            break;
        }
        os.sleep_ms(25);
    }
    let sock_fd = sock_fd?;
    let pid = os.getpid();
    let spec = client.spec();

    let hello = encode_client_msg(&WmClientMsg::Hello {
        pid,
        app_name: spec.title.clone(),
    });
    os.send_all(sock_fd, &hello)?;

    let create = encode_client_msg(&WmClientMsg::CreateWindow {
        title: spec.title,
        width: spec.width,
        height: spec.height,
        min_width: spec.min_width,
        min_height: spec.min_height,
        resizable: spec.resizable,
    });
    os.send_all(sock_fd, &create)?;

    let mut rx_buf: Vec<u8> = Vec::with_capacity(2048);
    let mut tmp = [0u8; 1024];
    let mut window_id: Option<u32> = None;
    let mut cur_w = spec.width.max(1);
    let mut cur_h = spec.height.max(1);
    let mut theme_kind = ThemeKind::SublimeClassic;
    let mut shm_seq: u32 = 1;
    let mut dirty = true;
    let mut tick_ms: u64 = 0;

    // Wait briefly for Welcome + initial Configure so we allocate exact client bounds immediately
    for _ in 0..100 {
        match os.recv_nonblocking(sock_fd, &mut tmp) {
            Ok(0) => {
                os.close(sock_fd);
                return Ok(());
            }
            Ok(n) => rx_buf.extend_from_slice(&tmp[..n]),
            Err(_) => {}
        }
        while let Some(frame) = take_next_frame(&mut rx_buf) {
            if let Some(msg) = decode_server_msg(&frame) {
                match msg {
                    WmServerMsg::Welcome { theme_kind: t, .. } => {
                        theme_kind = ThemeKind::from_u8(t);
                        crate::gfx::set_theme_kind(theme_kind);
                    }
                    WmServerMsg::PaletteChanged {
                        theme_kind: t,
                        seed_rgb,
                    } => {
                        theme_kind = ThemeKind::from_u8(t);
                        crate::gfx::set_theme_kind(theme_kind);
                        crate::gfx::set_wallpaper_seed_raw(seed_rgb);
                    }
                    WmServerMsg::Configure {
                        window_id: wid,
                        width,
                        height,
                        theme_kind: t,
                    } => {
                        window_id = Some(wid);
                        cur_w = width.max(64);
                        cur_h = height.max(64);
                        theme_kind = ThemeKind::from_u8(t);
                        crate::gfx::set_theme_kind(theme_kind);
                    }
                    _ => {}
                }
            }
        }
        if window_id.is_some() {
            break;
        }
        os.sleep_ms(10);
    }

    let wid = match window_id {
        Some(id) => id,
        None => {
            os.close(sock_fd);
            return Err(-110); // -ETIMEDOUT
        }
    };

    let init_shm_name = format!("wm_{}_{}", pid, shm_seq);
    shm_seq += 1;
    let buf = ShmBuffer::allocate(os, &init_shm_name, cur_w, cur_h)?;
    let attach = encode_client_msg(&WmClientMsg::AttachShm {
        window_id: wid,
        width: cur_w,
        height: cur_h,
        stride_bytes: (cur_w as u32) * 4,
        shm_name: init_shm_name,
    });
    os.send_all(sock_fd, &attach)?;
    let mut shm = Some(buf);
    // Double-buffer in client RAM so the compositor never samples a half-erased SHM frame mid-draw.
    let mut back_pixels: Vec<u32> = alloc::vec![0u32; (cur_w as usize) * (cur_h as usize)];
    let mut force_full_shm_sync = true;

    let init_bounds = Rect::new(0, 0, cur_w, cur_h);
    client.on_mount(init_bounds);
    client.on_theme_changed(theme_kind);
    let mut last_clip = SystemClipboard::get_text();

    let mut running = true;
    while running {
        // 1. Drain socket messages
        loop {
            match os.recv_nonblocking(sock_fd, &mut tmp) {
                Ok(0) => {
                    running = false;
                    break;
                }
                Ok(n) => {
                    rx_buf.extend_from_slice(&tmp[..n]);
                    if n < tmp.len() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }

        while let Some(frame) = take_next_frame(&mut rx_buf) {
            if let Some(msg) = decode_server_msg(&frame) {
                match msg {
                    WmServerMsg::Welcome { theme_kind: t, .. } => {
                        let new_theme = ThemeKind::from_u8(t);
                        if new_theme != theme_kind {
                            theme_kind = new_theme;
                            crate::gfx::set_theme_kind(theme_kind);
                            client.on_theme_changed(theme_kind);
                        }
                        dirty = true;
                        force_full_shm_sync = true;
                    }
                    WmServerMsg::ThemeChanged { theme_kind: t } => {
                        theme_kind = ThemeKind::from_u8(t);
                        crate::gfx::set_theme_kind(theme_kind);
                        client.on_theme_changed(theme_kind);
                        dirty = true;
                        force_full_shm_sync = true;
                    }
                    WmServerMsg::PaletteChanged {
                        theme_kind: t,
                        seed_rgb,
                    } => {
                        theme_kind = ThemeKind::from_u8(t);
                        crate::gfx::set_theme_kind(theme_kind);
                        crate::gfx::set_wallpaper_seed_raw(seed_rgb);
                        client.on_theme_changed(theme_kind);
                        dirty = true;
                        force_full_shm_sync = true;
                    }
                    WmServerMsg::ClipboardChanged { text } => {
                        if text != last_clip {
                            SystemClipboard::set_text(&text);
                            last_clip = text;
                            client.on_clipboard(&last_clip);
                        }
                    }
                    WmServerMsg::Configure {
                        window_id: msg_wid,
                        width,
                        height,
                        theme_kind: t,
                    } if msg_wid == wid => {
                        let new_theme = ThemeKind::from_u8(t);
                        if new_theme != theme_kind {
                            theme_kind = new_theme;
                            crate::gfx::set_theme_kind(theme_kind);
                            client.on_theme_changed(theme_kind);
                            dirty = true;
                            force_full_shm_sync = true;
                        }
                        let nw = width.max(64);
                        let nh = height.max(64);
                        if nw != cur_w || nh != cur_h {
                            cur_w = nw;
                            cur_h = nh;
                            if let Some(old) = shm.take() {
                                old.release(os);
                            }
                            let name = format!("wm_{}_{}", pid, shm_seq);
                            shm_seq += 1;
                            let next_buf = ShmBuffer::allocate(os, &name, cur_w, cur_h)?;
                            back_pixels.resize((cur_w as usize) * (cur_h as usize), 0);
                            let msg = encode_client_msg(&WmClientMsg::AttachShm {
                                window_id: wid,
                                width: cur_w,
                                height: cur_h,
                                stride_bytes: (cur_w as u32) * 4,
                                shm_name: name,
                            });
                            let _ = os.send_all(sock_fd, &msg);
                            shm = Some(next_buf);
                            let bounds = Rect::new(0, 0, cur_w, cur_h);
                            let res = client.on_event(
                                GuiEvent::Resize {
                                    width: cur_w,
                                    height: cur_h,
                                },
                                bounds,
                            );
                            if matches!(res, EventResult::CloseRequested) {
                                running = false;
                            }
                            dirty = true;
                            force_full_shm_sync = true;
                        }
                    }
                    WmServerMsg::Input {
                        window_id: msg_wid,
                        event,
                    } if msg_wid == wid => {
                        if let GuiEvent::FocusChanged { focused } = event {
                            client.on_focus(focused);
                        }
                        let bounds = Rect::new(0, 0, cur_w, cur_h);
                        match client.on_event(event, bounds) {
                            EventResult::NeedsRedraw => dirty = true,
                            EventResult::CloseRequested => running = false,
                            _ => {}
                        }
                    }
                    WmServerMsg::CloseRequested { window_id: msg_wid } if msg_wid == wid => {
                        running = false;
                    }
                    _ => {}
                }
            }
        }

        if !running {
            break;
        }

        let bounds = Rect::new(0, 0, cur_w, cur_h);
        match client.on_event(GuiEvent::Tick { now_ms: tick_ms }, bounds) {
            EventResult::NeedsRedraw => dirty = true,
            EventResult::CloseRequested => break,
            _ => {}
        }

        let cur_clip = SystemClipboard::get_text();
        if cur_clip != last_clip {
            last_clip = cur_clip.clone();
            let clip_msg = encode_client_msg(&WmClientMsg::SetClipboard { text: cur_clip });
            let _ = os.send_all(sock_fd, &clip_msg);
        }

        if dirty {
            if let Some(ref mut buf) = shm {
                let taken = client.take_damage(bounds);
                let dmg = if force_full_shm_sync {
                    force_full_shm_sync = false;
                    bounds
                } else {
                    taken.map(|r| r.intersect(bounds)).unwrap_or(bounds)
                };
                if !dmg.is_empty() {
                    let theme = crate::gfx::current_theme();
                    let w = buf.width;
                    let h = buf.height;
                    if back_pixels.len() != (w as usize) * (h as usize) {
                        back_pixels.resize((w as usize) * (h as usize), 0);
                    }
                    {
                        let mut surface = Surface::from_slice(w, h, &mut back_pixels);
                        surface.push_clip(dmg);
                        client.draw(&mut surface, bounds, &theme);
                        surface.pop_clip();
                    }
                    let shm_px = buf.pixels_mut();
                    let stride = w as usize;
                    let x0 = dmg.x.max(0) as usize;
                    let x1 = dmg.right().min(w).max(0) as usize;
                    let y0 = dmg.y.max(0) as usize;
                    let y1 = dmg.bottom().min(h).max(0) as usize;
                    if x1 > x0 && y1 > y0 {
                        for row in y0..y1 {
                            let off = row * stride;
                            shm_px[off + x0..off + x1]
                                .copy_from_slice(&back_pixels[off + x0..off + x1]);
                        }
                    }
                    core::sync::atomic::fence(core::sync::atomic::Ordering::Release);
                    let commit = encode_client_msg(&WmClientMsg::CommitDamage {
                        window_id: wid,
                        x: dmg.x,
                        y: dmg.y,
                        width: dmg.w,
                        height: dmg.h,
                    });
                    if os.send_all(sock_fd, &commit).is_err() {
                        break;
                    }
                }
            }
            dirty = false;
        }

        os.sleep_ms(16);
        tick_ms = tick_ms.saturating_add(16);
    }

    client.on_unmount();
    let close_msg = encode_client_msg(&WmClientMsg::CloseWindow { window_id: wid });
    let _ = os.send_all(sock_fd, &close_msg);
    if let Some(old) = shm.take() {
        old.release(os);
    }
    os.close(sock_fd);
    Ok(())
}
