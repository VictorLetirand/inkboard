//! Windows implementation: a window subclass that observes the `WM_POINTER*` messages.

use crate::{PenSample, PointerKind, record, set_eraser};
use std::num::NonZeroIsize;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Input::Pointer::{GetPointerPenInfo, GetPointerType, POINTER_PEN_INFO};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{PT_PEN, PT_TOUCH, WM_POINTERDOWN, WM_POINTERLEAVE, WM_POINTERUP, WM_POINTERUPDATE};

const SUBCLASS_ID: usize = 0x5742_5045; // "WBPE"

/// Pointer id: low word of `wParam` (`GET_POINTERID_WPARAM`).
fn pointer_id(wparam: WPARAM) -> u32 {
    u32::try_from(wparam & 0xffff).expect("masked to 16 bits")
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _id: usize, _data: usize) -> LRESULT {
    if matches!(msg, WM_POINTERDOWN | WM_POINTERUPDATE | WM_POINTERUP | WM_POINTERLEAVE) {
        let id = pointer_id(wparam);
        if msg == WM_POINTERLEAVE {
            set_eraser(false);
        } else {
            let mut kind = 0;
            // SAFETY: `kind` is a valid integer for the whole call; an invalid id simply makes the call fail.
            let ok = unsafe { GetPointerType(id, &raw mut kind) } != 0;
            if ok && kind == PT_PEN {
                let mut info = POINTER_PEN_INFO::default();
                // SAFETY: output structure valid for the duration of the call.
                if unsafe { GetPointerPenInfo(id, &raw mut info) } != 0 {
                    record(PenSample {
                        kind: PointerKind::Pen,
                        pressure: u16::try_from(info.pressure).map_or(1.0, |p| (f32::from(p) / 1024.0).min(1.0)),
                        flags: info.penFlags,
                        tilt: (info.tiltX, info.tiltY),
                        seq: 0,
                    });
                }
            } else if ok {
                // mouse or finger: no more eraser
                record(PenSample { kind: if kind == PT_TOUCH { PointerKind::Touch } else { PointerKind::Mouse }, ..PenSample::NONE });
            }
        }
    }
    // messages are never consumed: winit/egui keep processing them
    // SAFETY: the received arguments are passed unchanged to the next procedure in the chain.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// Installs the observation of pen messages on the window. `hwnd` must be the handle of a window of **this** process and thread
/// (obtained through `raw-window-handle`); returns `false` when the system refuses.
pub fn install(hwnd: NonZeroIsize) -> bool {
    // SAFETY: `proc` has the expected `SUBCLASSPROC` signature; the handle is that of a live window owned by the calling thread.
    unsafe { SetWindowSubclass(hwnd.get() as HWND, Some(proc), SUBCLASS_ID, 0) != 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, SendMessageW, WS_OVERLAPPED};

    fn dummy_window() -> HWND {
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        // SAFETY: "STATIC" window (system class) created without a parent; destroyed at the end of the test.
        unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                WS_OVERLAPPED,
                0,
                0,
                10,
                10,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        }
    }

    #[test]
    fn subclass_installs_and_ignores_unknown_pointers_without_consuming_messages() {
        let _g = crate::TEST_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let hwnd = dummy_window();
        assert!(!hwnd.is_null(), "dummy window created");
        assert!(install(NonZeroIsize::new(hwnd as isize).unwrap()));
        set_eraser(true);
        // nonexistent pointer: the state is unchanged (GetPointerType fails) and the message passes through without error
        // SAFETY: valid window of the current thread.
        unsafe { SendMessageW(hwnd, WM_POINTERUPDATE, 0xffff, 0) };
        assert!(crate::eraser_active(), "an unknown pointer does not change the state");
        // POINTERLEAVE clears the state
        unsafe { SendMessageW(hwnd, WM_POINTERLEAVE, 0xffff, 0) };
        assert!(!crate::eraser_active());
        unsafe { DestroyWindow(hwnd) };
    }
}
