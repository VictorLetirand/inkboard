//! Pen without hardware: a virtual pen (`CreateSyntheticPointerDevice`) is injected into a small test window of this process
//! on which the `WM_POINTER*` subclass of `inkboard-pen` is installed, exactly as the application does.
//!
//! These tests act on the REAL desktop session (a small topmost window appears, the pointer pipeline is used), so they are
//! `#[ignore]`d. Run them by hand, without touching the machine meanwhile:
//!
//! ```text
//! cargo test -p inkboard-pen --test synthetic -- --ignored --test-threads=1
//! ```
//!
//! Limit: this validates the `WM_POINTER` path of the application, not real drivers (Wacom / N-trig pressure curves…).

#![cfg(windows)]

use std::num::NonZeroIsize;
use std::ptr::{null, null_mut};
use std::sync::{Mutex, MutexGuard, Once, PoisonError};
use std::time::{Duration, Instant};

use inkboard_pen::{PEN_FLAG_ERASER, PEN_FLAG_INVERTED, PenSample, PointerKind};
use windows_sys::Win32::Foundation::{GetLastError, HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::COLOR_WINDOW;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{
    CreateSyntheticPointerDevice, DestroySyntheticPointerDevice, HSYNTHETICPOINTERDEVICE, POINTER_FEEDBACK_NONE, POINTER_TYPE_INFO,
};
use windows_sys::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext};
use windows_sys::Win32::UI::Input::Pointer::{
    InjectSyntheticPointerInput, POINTER_FLAG_DOWN, POINTER_FLAG_FIRSTBUTTON, POINTER_FLAG_INCONTACT, POINTER_FLAG_INRANGE, POINTER_FLAG_NEW, POINTER_FLAG_UP,
    POINTER_FLAG_UPDATE, POINTER_PEN_INFO,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetWindowRect, MSG, PEN_MASK_PRESSURE, PM_REMOVE, PT_PEN, PeekMessageW, RegisterClassExW,
    SetForegroundWindow, TranslateMessage, WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE, WindowFromPoint,
};

const CLASS_NAME: &str = "InkboardPenSyntheticTest\0";
/// Windows reports pen pressure from 0 to 1024.
const MAX_PRESSURE: f32 = 1024.0;

/// The tests share the global state of `inkboard-pen` and the desktop: one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

/// Virtual pen device, destroyed on drop (also when a test fails).
struct Device(HSYNTHETICPOINTERDEVICE);

impl Drop for Device {
    fn drop(&mut self) {
        // SAFETY: handle returned by `CreateSyntheticPointerDevice`, destroyed once.
        unsafe { DestroySyntheticPointerDevice(self.0) };
    }
}

/// Small topmost test window, destroyed on drop.
struct TestWindow(HWND);

impl Drop for TestWindow {
    fn drop(&mut self) {
        // SAFETY: window created by this thread.
        unsafe { DestroyWindow(self.0) };
    }
}

/// Everything a test needs. Field order = drop order: device, then window, then the lock.
struct Session {
    device: Device,
    window: TestWindow,
    _serial: MutexGuard<'static, ()>,
}

/// What the application sees after one injected frame.
struct Frame {
    sample: PenSample,
    eraser_active: bool,
}

fn pump_messages(duration: Duration) {
    let end = Instant::now() + duration;
    while Instant::now() < end {
        let mut msg = MSG::default();
        // SAFETY: `msg` is valid for the call; messages of this thread only.
        while unsafe { PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) } != 0 {
            // SAFETY: message just returned by `PeekMessageW`.
            unsafe {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn create_window() -> HWND {
    static CLASS: Once = Once::new();
    let class: Vec<u16> = CLASS_NAME.encode_utf16().collect();
    // SAFETY: null = module of the current process.
    let hinstance = unsafe { GetModuleHandleW(null()) };
    CLASS.call_once(|| {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(DefWindowProcW),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: null_mut(),
            hCursor: null_mut(),
            hbrBackground: (COLOR_WINDOW as usize + 1) as _,
            lpszMenuName: null(),
            lpszClassName: class.as_ptr(),
            hIconSm: null_mut(),
        };
        // SAFETY: `wc` and the class name are valid for the call.
        assert!(unsafe { RegisterClassExW(&wc) } != 0, "RegisterClassExW failed (error {})", unsafe { GetLastError() });
    });
    let title: Vec<u16> = "Inkboard pen test\0".encode_utf16().collect();
    // SAFETY: registered class, no parent; destroyed by `TestWindow::drop`.
    unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class.as_ptr(),
            title.as_ptr(),
            WS_POPUP | WS_VISIBLE,
            100,
            100,
            320,
            200,
            null_mut(),
            null_mut(),
            hinstance,
            null(),
        )
    }
}

impl Session {
    fn new() -> Session {
        let serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
        // Pixel coordinates injected below must be physical ones, whatever the display scaling (error ignored: may be set already).
        // SAFETY: plain call.
        unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };

        // The window is created before the device so that a failure leaves nothing behind (both are RAII guards).
        let window = TestWindow(create_window());
        assert!(!window.0.is_null(), "test window not created (error {})", unsafe { GetLastError() });
        let hwnd = NonZeroIsize::new(window.0 as isize).expect("non-null window");
        assert!(inkboard_pen::install(hwnd), "the WM_POINTER subclass was refused");
        // SAFETY: window of this thread. Foreground is best effort (the system may refuse), the topmost style does the rest.
        unsafe { SetForegroundWindow(window.0) };
        pump_messages(Duration::from_millis(150));

        // SAFETY: plain call; the returned handle is owned by `Device`.
        let handle = unsafe { CreateSyntheticPointerDevice(PT_PEN, 1, POINTER_FEEDBACK_NONE) };
        assert!(!handle.is_null(), "CreateSyntheticPointerDevice(PT_PEN) failed (error {}); needs Windows 10 1809+", unsafe { GetLastError() });
        let device = Device(handle);

        inkboard_pen::set_eraser(false);
        Session { device, window, _serial: serial }
    }

    /// Points of a horizontal line through the middle of the window.
    fn line(&self) -> Vec<POINT> {
        let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        // SAFETY: `r` valid for the call, window of this thread.
        assert!(unsafe { GetWindowRect(self.window.0, &mut r) } != 0, "GetWindowRect failed");
        let y = (r.top + r.bottom) / 2;
        let pts: Vec<POINT> = (0..8).map(|i| POINT { x: r.left + 60 + i * 25, y }).collect();
        // The injected coordinates must land in OUR window, otherwise another window of the user would receive the pen.
        for p in &pts {
            assert!(
                p.x > r.left && p.x < r.right && p.y > r.top && p.y < r.bottom,
                "point ({}, {}) outside the test window ({}, {}, {}, {})",
                p.x,
                p.y,
                r.left,
                r.top,
                r.right,
                r.bottom
            );
            // SAFETY: plain call.
            let hit = unsafe { WindowFromPoint(*p) };
            assert_eq!(hit, self.window.0, "another window covers ({}, {}): refusing to inject the pen", p.x, p.y);
        }
        pts
    }

    /// Injects one frame and returns what `inkboard-pen` saw (a new sample is required unless `expect_sample` is false).
    fn inject(&self, p: POINT, pointer_flags: u32, pen_flags: u32, pressure: f32, expect_sample: bool) -> Option<Frame> {
        let mut info = POINTER_TYPE_INFO { r#type: PT_PEN, ..Default::default() };
        let mut pen = POINTER_PEN_INFO::default();
        pen.pointerInfo.pointerType = PT_PEN;
        pen.pointerInfo.pointerId = 0;
        pen.pointerInfo.pointerFlags = pointer_flags;
        pen.pointerInfo.ptPixelLocation = p;
        pen.penFlags = pen_flags;
        pen.penMask = PEN_MASK_PRESSURE;
        pen.pressure = (pressure * MAX_PRESSURE).round() as u32;
        info.Anonymous.penInfo = pen;

        let before = inkboard_pen::last_sample().seq;
        // SAFETY: valid device and one fully initialized `POINTER_TYPE_INFO`.
        let ok = unsafe { InjectSyntheticPointerInput(self.device.0, &info, 1) } != 0;
        assert!(ok, "InjectSyntheticPointerInput failed (error {})", unsafe { GetLastError() });

        // The subclass runs while this thread dispatches messages.
        let end = Instant::now() + Duration::from_millis(750);
        while Instant::now() < end && inkboard_pen::last_sample().seq == before {
            pump_messages(Duration::from_millis(10));
        }
        pump_messages(Duration::from_millis(20)); // let the other messages of the frame arrive
        let sample = inkboard_pen::last_sample();
        if sample.seq == before {
            assert!(!expect_sample, "no WM_POINTER message reached the test window: locked session, no foreground, or injection filtered by the system");
            return None;
        }
        Some(Frame { sample, eraser_active: inkboard_pen::eraser_active() })
    }

    /// One stroke (down, moves, up, leave range). Returns the frames seen while in contact.
    fn stroke(&self, pressure: f32, pen_flags: u32) -> Vec<Frame> {
        let pts = self.line();
        let contact = POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT | POINTER_FLAG_FIRSTBUTTON;
        let mut frames = Vec::new();
        frames.extend(self.inject(pts[0], contact | POINTER_FLAG_NEW | POINTER_FLAG_DOWN, pen_flags, pressure, true));
        for p in &pts[1..] {
            frames.extend(self.inject(*p, contact | POINTER_FLAG_UPDATE, pen_flags, pressure, true));
        }
        let last = *pts.last().expect("points");
        self.inject(last, POINTER_FLAG_INRANGE | POINTER_FLAG_UP, pen_flags, 0.0, true);
        self.inject(last, POINTER_FLAG_UPDATE, 0, 0.0, false); // out of range: the next stroke starts with NEW
        frames
    }
}

fn assert_pen_pressure(frames: &[Frame], expected: f32) {
    assert!(!frames.is_empty(), "no sample received for the stroke");
    for f in frames {
        assert_eq!(f.sample.kind, PointerKind::Pen, "the injected pointer must be seen as a pen: {:?}", f.sample);
        assert!((f.sample.pressure - expected).abs() < 0.02, "pressure {} instead of {expected}: {:?}", f.sample.pressure, f.sample);
    }
}

#[test]
#[ignore = "needs an interactive desktop session; run with --ignored"]
fn pen_pressure_follows_the_injected_value() {
    let s = Session::new();
    let light = s.stroke(0.2, 0);
    let firm = s.stroke(1.0, 0);
    assert_pen_pressure(&light, 0.2);
    assert_pen_pressure(&firm, 1.0);
    let light_max = light.iter().map(|f| f.sample.pressure).fold(0.0, f32::max);
    let firm_min = firm.iter().map(|f| f.sample.pressure).fold(1.0, f32::min);
    assert!(light_max < firm_min, "the firm stroke ({firm_min}) must be above the light one ({light_max})");
    assert!(light.iter().chain(&firm).all(|f| !f.sample.eraser() && !f.eraser_active), "the writing end is not the eraser");
}

#[test]
#[ignore = "needs an interactive desktop session; run with --ignored"]
fn pen_eraser_end_is_detected() {
    let s = Session::new();
    let writing = s.stroke(0.5, 0);
    assert!(writing.iter().all(|f| !f.eraser_active), "writing end: eraser state off");
    let erasing = s.stroke(0.5, PEN_FLAG_INVERTED | PEN_FLAG_ERASER);
    assert_pen_pressure(&erasing, 0.5);
    assert!(erasing.iter().all(|f| f.sample.eraser() && f.eraser_active), "eraser end: every sample flagged and eraser_active() true");
    // back to the writing end
    let again = s.stroke(0.5, 0);
    assert!(again.iter().all(|f| !f.sample.eraser() && !f.eraser_active), "eraser state released with the writing end");
}
