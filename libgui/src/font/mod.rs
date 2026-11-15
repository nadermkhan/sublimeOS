pub mod atlas;
pub mod builtin_ttf;
pub mod fallback8x16;
pub mod raster;
pub mod ttf;

use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};

pub use atlas::{AntialiasMode, FontAtlas, FontEngine, FontFamily, TextStyle};
pub use builtin_ttf::build_sublime_sans_ttf;
pub use raster::{rasterize_outline, RasterGlyph};
pub use ttf::{FontMetrics, GlyphMetrics, Outline, TtfFont};

pub const FALLBACK_GLYPH_WIDTH: i32 = 8;
pub const FALLBACK_GLYPH_HEIGHT: i32 = 16;

/// Genuine subsetted Google Noto Sans Regular & Bold TTF fonts used system-wide across SublimeOS UI.
pub const NOTO_SANS_REGULAR_TTF: &[u8] = include_bytes!("../../assets/NotoSans-Regular.ttf");
pub const NOTO_SANS_BOLD_TTF: &[u8] = include_bytes!("../../assets/NotoSans-Bold.ttf");

struct SpinMutex<T> {
    locked: AtomicBool,
    inner: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for SpinMutex<T> {}
unsafe impl<T: Send> Send for SpinMutex<T> {}

impl<T> SpinMutex<T> {
    const fn new(val: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            inner: UnsafeCell::new(val),
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
        let res = f(unsafe { &mut *self.inner.get() });
        self.locked.store(false, Ordering::Release);
        res
    }
}

static DEFAULT_FONT_ENGINE: SpinMutex<Option<FontEngine>> = SpinMutex::new(None);

/// Run `f` with the process/kernel default `FontEngine`, initializing built-in Noto Sans TTF on first call.
pub fn with_default_font<R>(f: impl FnOnce(&mut FontEngine) -> R) -> R {
    DEFAULT_FONT_ENGINE.with(|slot| {
        if slot.is_none() {
            *slot = Some(FontEngine::with_static_fonts(
                NOTO_SANS_REGULAR_TTF,
                Some(NOTO_SANS_BOLD_TTF),
            ));
        }
        f(slot.as_mut().unwrap())
    })
}

/// Replace the active default TTF bytes (e.g. from `/usr/share/fonts/ui.ttf`).
pub fn load_default_ttf(bytes: Vec<u8>) -> bool {
    with_default_font(|engine| engine.set_ttf_bytes(bytes))
}
