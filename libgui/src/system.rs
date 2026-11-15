use alloc::string::String;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};
use crate::event::{GuiEvent, KeyCode};
use crate::gfx::Rect;

pub type WidgetId = u32;
pub type CommandId = u32;

struct SpinSlot<T> {
    locked: AtomicBool,
    val: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for SpinSlot<T> {}
unsafe impl<T: Send> Send for SpinSlot<T> {}

impl<T> SpinSlot<T> {
    const fn new(v: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            val: UnsafeCell::new(v),
        }
    }

    fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        let r = f(unsafe { &mut *self.val.get() });
        self.locked.store(false, Ordering::Release);
        r
    }
}

static CLIPBOARD: SpinSlot<Option<String>> = SpinSlot::new(None);

/// System-wide clipboard service shared across UI controls and synced over compositor IPC.
pub struct SystemClipboard;

impl SystemClipboard {
    pub fn set_text(text: &str) {
        let bounded = if text.len() > 8192 { &text[..8192] } else { text };
        CLIPBOARD.with(|slot| {
            *slot = Some(String::from(bounded));
        });
    }

    pub fn get_text() -> String {
        CLIPBOARD.with(|slot| slot.as_ref().cloned().unwrap_or_default())
    }

    pub fn clear() {
        CLIPBOARD.with(|slot| {
            *slot = Some(String::new());
        });
    }
}

/// Manages ordered keyboard focus ring (`Tab` / `Shift+Tab`) and focus transitions.
#[derive(Clone, Debug, Default)]
pub struct FocusManager {
    order: Vec<WidgetId>,
    focused: Option<WidgetId>,
}

impl FocusManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, id: WidgetId) {
        if !self.order.contains(&id) {
            self.order.push(id);
            if self.focused.is_none() {
                self.focused = Some(id);
            }
        }
    }

    pub fn unregister(&mut self, id: WidgetId) {
        self.order.retain(|&x| x != id);
        if self.focused == Some(id) {
            self.focused = self.order.first().copied();
        }
    }

    pub fn clear(&mut self) {
        self.order.clear();
        self.focused = None;
    }

    pub fn focused(&self) -> Option<WidgetId> {
        self.focused
    }

    pub fn is_focused(&self, id: WidgetId) -> bool {
        self.focused == Some(id)
    }

    /// Sets focus to `id`, returning `(previous_focused, current_focused)`.
    pub fn set_focus(&mut self, id: Option<WidgetId>) -> (Option<WidgetId>, Option<WidgetId>) {
        let prev = self.focused;
        if let Some(target) = id {
            if !self.order.contains(&target) {
                self.order.push(target);
            }
        }
        self.focused = id;
        (prev, self.focused)
    }

    pub fn focus_next(&mut self) -> (Option<WidgetId>, Option<WidgetId>) {
        if self.order.is_empty() {
            return (self.focused, self.focused);
        }
        let next = match self.focused.and_then(|f| self.order.iter().position(|&x| x == f)) {
            Some(idx) => self.order[(idx + 1) % self.order.len()],
            None => self.order[0],
        };
        self.set_focus(Some(next))
    }

    pub fn focus_prev(&mut self) -> (Option<WidgetId>, Option<WidgetId>) {
        if self.order.is_empty() {
            return (self.focused, self.focused);
        }
        let prev_idx = match self.focused.and_then(|f| self.order.iter().position(|&x| x == f)) {
            Some(0) | None => self.order.len() - 1,
            Some(idx) => idx - 1,
        };
        self.set_focus(Some(self.order[prev_idx]))
    }
}

#[derive(Clone, Copy, Debug)]
struct TimerEntry {
    id: u32,
    interval_ms: u64,
    next_fire_ms: u64,
    repeating: bool,
}

/// Lightweight UI timer wheel (`WM_TIMER`-style one-shot & interval timers).
#[derive(Clone, Debug, Default)]
pub struct TimerScheduler {
    next_id: u32,
    entries: Vec<TimerEntry>,
}

impl TimerScheduler {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            entries: Vec::new(),
        }
    }

    pub fn set_interval(&mut self, interval_ms: u64, now_ms: u64) -> u32 {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        let step = interval_ms.max(16);
        self.entries.push(TimerEntry {
            id,
            interval_ms: step,
            next_fire_ms: now_ms.saturating_add(step),
            repeating: true,
        });
        id
    }

    pub fn set_timeout(&mut self, delay_ms: u64, now_ms: u64) -> u32 {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        let step = delay_ms.max(1);
        self.entries.push(TimerEntry {
            id,
            interval_ms: step,
            next_fire_ms: now_ms.saturating_add(step),
            repeating: false,
        });
        id
    }

    pub fn cancel(&mut self, timer_id: u32) {
        self.entries.retain(|e| e.id != timer_id);
    }

    pub fn tick(&mut self, now_ms: u64) -> Vec<u32> {
        let mut fired = Vec::new();
        let mut i = 0;
        while i < self.entries.len() {
            if now_ms >= self.entries[i].next_fire_ms {
                fired.push(self.entries[i].id);
                if self.entries[i].repeating {
                    let step = self.entries[i].interval_ms;
                    self.entries[i].next_fire_ms = now_ms.saturating_add(step);
                    i += 1;
                } else {
                    self.entries.remove(i);
                }
            } else {
                i += 1;
            }
        }
        fired
    }
}

#[derive(Clone, Copy, Debug)]
struct HotkeyBinding {
    key: KeyCode,
    ctrl: bool,
    shift: bool,
    alt: bool,
    cmd: CommandId,
}

/// System-wide accelerator table mapping `(KeyCode, Modifiers)` to typed `CommandId`s.
#[derive(Clone, Debug, Default)]
pub struct HotkeyRegistry {
    bindings: Vec<HotkeyBinding>,
}

impl HotkeyRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn bind(&mut self, key: KeyCode, ctrl: bool, shift: bool, alt: bool, cmd: CommandId) {
        self.bindings.push(HotkeyBinding {
            key,
            ctrl,
            shift,
            alt,
            cmd,
        });
    }

    pub fn match_event(&self, event: GuiEvent) -> Option<CommandId> {
        if let GuiEvent::KeyDown { key, mods, .. } = event {
            for b in &self.bindings {
                if b.key == key && b.ctrl == mods.ctrl && b.shift == mods.shift && b.alt == mods.alt {
                    return Some(b.cmd);
                }
            }
        }
        None
    }
}

/// Coalescing dirty-rectangle accumulator preventing unnecessary full-frame redraws and MMIO tearing.
#[derive(Clone, Debug, Default)]
pub struct DamageTracker {
    rects: Vec<Rect>,
    full: bool,
}

impl DamageTracker {
    pub const MAX_RECTS: usize = 8;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn invalidate(&mut self, rect: Rect) {
        if self.full || rect.is_empty() {
            return;
        }
        for existing in &mut self.rects {
            if existing.expand(2).intersects(rect) {
                *existing = existing.union(rect);
                return;
            }
        }
        if self.rects.len() >= Self::MAX_RECTS {
            let mut merged = rect;
            for r in self.rects.drain(..) {
                merged = merged.union(r);
            }
            self.rects.push(merged);
        } else {
            self.rects.push(rect);
        }
    }

    pub fn invalidate_all(&mut self) {
        self.full = true;
        self.rects.clear();
    }

    pub fn is_dirty(&self) -> bool {
        self.full || !self.rects.is_empty()
    }

    /// Take damage clipped to `bounds`, returning normalized non-overlapping dirty rects (or `[bounds]` if full).
    pub fn take_clipped(&mut self, bounds: Rect) -> Vec<Rect> {
        if self.full {
            self.full = false;
            self.rects.clear();
            return if bounds.is_empty() { Vec::new() } else { alloc::vec![bounds] };
        }
        let mut out = Vec::with_capacity(self.rects.len());
        for r in self.rects.drain(..) {
            let c = r.intersect(bounds);
            if !c.is_empty() {
                out.push(c);
            }
        }
        out
    }

    /// Take a single bounding rect covering all queued damage, or `None` if clean.
    pub fn take_bounding(&mut self, bounds: Rect) -> Option<Rect> {
        let rects = self.take_clipped(bounds);
        let mut acc = Rect::default();
        for r in rects {
            acc = acc.union(r);
        }
        if acc.is_empty() { None } else { Some(acc) }
    }

    pub fn clear(&mut self) {
        self.full = false;
        self.rects.clear();
    }
}
