//! Pen eraser.
//!
//! winit and egui do not pass on the state of a pen's buttons / "eraser" end. On Windows, this information is in the
//! `WM_POINTER*` messages (`PEN_FLAG_ERASER` / `PEN_FLAG_INVERTED` flags of `POINTER_PEN_INFO`). This crate installs a window *subclass*
//! that reads these messages without consuming them and remembers the state; the application queries it with [`eraser_active`].
//!
//! On other platforms, [`install`] has no effect and [`eraser_active`] always returns `false`.
//!
//! **Not validated on real hardware**: the flag-interpretation logic is tested, and installing the *subclass* is tested on a
//! dummy window, but no physical pen was available.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

static ERASER: AtomicBool = AtomicBool::new(false);
static LAST: Mutex<PenSample> = Mutex::new(PenSample::NONE);

/// Type of the pointer seen last.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PointerKind {
    #[default]
    None,
    Mouse,
    Touch,
    Pen,
}

/// Last pointer sample, as seen by Windows (for pen diagnostics).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PenSample {
    pub kind: PointerKind,
    /// Pressure from 0.0 to 1.0 (pen only).
    pub pressure: f32,
    /// Indicateurs `PEN_FLAG_*` (stylet uniquement).
    pub flags: u32,
    /// Tilt in degrees (pen only).
    pub tilt: (i32, i32),
    /// Total number of samples received (lets you detect that a new one has arrived).
    pub seq: u64,
}

impl PenSample {
    pub const NONE: PenSample = PenSample { kind: PointerKind::None, pressure: 0.0, flags: 0, tilt: (0, 0), seq: 0 };
    #[must_use]
    pub fn barrel(&self) -> bool {
        self.flags & PEN_FLAG_BARREL != 0
    }
    #[must_use]
    pub fn eraser(&self) -> bool {
        eraser_from_flags(self.flags)
    }
}

/// Last sample received (`PenSample::NONE` until a message has arrived).
pub fn last_sample() -> PenSample {
    *LAST.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Records a sample: updates the eraser state and increments `seq` (internal use and tests).
pub fn record(mut s: PenSample) {
    let mut g = LAST.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    s.seq = g.seq + 1;
    *g = s;
    ERASER.store(s.kind == PointerKind::Pen && s.eraser(), Ordering::Relaxed);
}

/// Pen flags (values of the Windows API `PEN_FLAG_*`).
pub const PEN_FLAG_BARREL: u32 = 0x0000_0001;
pub const PEN_FLAG_INVERTED: u32 = 0x0000_0002;
pub const PEN_FLAG_ERASER: u32 = 0x0000_0004;

/// Is the eraser end in use according to these flags? (flipped end, or the "eraser" button pressed)
#[must_use]
pub fn eraser_from_flags(flags: u32) -> bool {
    flags & (PEN_FLAG_INVERTED | PEN_FLAG_ERASER) != 0
}

/// True while the approached or touching pen is used on its eraser side.
pub fn eraser_active() -> bool {
    ERASER.load(Ordering::Relaxed)
}

/// Sets the state (internal use and tests).
pub fn set_eraser(active: bool) {
    ERASER.store(active, Ordering::Relaxed);
}

#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use win::install;

/// Installs the detection on window `hwnd`. No effect outside Windows.
#[cfg(not(windows))]
pub fn install(_hwnd: std::num::NonZeroIsize) -> bool {
    false
}

/// Tests share the global state (last sample, eraser): they serialize with this lock.
#[cfg(test)]
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorded_samples_drive_the_eraser_state_and_sequence() {
        let _g = TEST_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        record(PenSample { kind: PointerKind::Pen, pressure: 0.5, flags: PEN_FLAG_INVERTED, tilt: (10, -5), seq: 0 });
        let a = last_sample();
        assert!(eraser_active() && a.eraser() && !a.barrel());
        assert_eq!((a.pressure, a.tilt), (0.5, (10, -5)));
        record(PenSample { kind: PointerKind::Mouse, ..PenSample::NONE });
        let b = last_sample();
        assert!(!eraser_active(), "the mouse is never the eraser");
        assert_eq!(b.seq, a.seq + 1);
    }

    #[test]
    fn eraser_is_detected_from_pen_flags() {
        assert!(!eraser_from_flags(0));
        assert!(!eraser_from_flags(PEN_FLAG_BARREL), "the barrel button is not the eraser");
        assert!(eraser_from_flags(PEN_FLAG_INVERTED));
        assert!(eraser_from_flags(PEN_FLAG_ERASER));
        assert!(eraser_from_flags(PEN_FLAG_BARREL | PEN_FLAG_INVERTED));
    }
}
