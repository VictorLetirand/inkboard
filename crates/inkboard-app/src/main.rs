//! Inkboard desktop application (egui).

// release builds on Windows are GUI programs: no console window behind the application
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod bench_icons;
mod chrome;
mod clipboard;
mod fonts;
mod i18n;
mod i18n_fr;
mod icons;
mod insert;
mod keymap;
mod library;
mod live;
mod panel;
mod pen_test;
mod render;
mod rich_edit;
mod theme;
mod tools;
mod ui;
mod watch;

use eframe::egui;
use std::path::PathBuf;

/// Command-line options.
#[derive(Clone, Default)]
struct Launch {
    open: Option<PathBuf>,
    screenshot: Option<PathBuf>,
    bench: bool,
    bench_close: bool,
    edit_demo: bool,
    shortcuts_demo: bool,
    formula_demo: bool,
    pen_test: bool,
    ui_demo: String,
    /// `--bench-icons COUNT[:DISTINCT]`
    bench_icons: Option<(usize, usize)>,
}

fn parse_args(args: &[String]) -> Launch {
    let mut l = Launch::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--screenshot" => l.screenshot = it.next().map(PathBuf::from),
            "--bench" => l.bench = true,
            "--bench-close" => {
                l.bench = true;
                l.bench_close = true;
            }
            "--edit-demo" => l.edit_demo = true,
            "--shortcuts-demo" => l.shortcuts_demo = true,
            "--formula-demo" => l.formula_demo = true,
            "--pen-test" => l.pen_test = true,
            "--bench-icons" => {
                let spec = it.next().cloned().unwrap_or_default();
                let (n, d) = spec.split_once(':').unwrap_or((spec.as_str(), "0"));
                let n = n.parse().unwrap_or(1000);
                l.bench_icons = Some((n, d.parse().ok().filter(|d| *d > 0).unwrap_or(n.min(2000))));
            }
            "--ui-demo" => l.ui_demo = it.next().cloned().unwrap_or_default(),
            other => l.open = Some(PathBuf::from(other)),
        }
    }
    l
}

/// Shows an error to the user (a message box: release builds have no console) and prints it.
fn report(title: &str, message: &str) {
    eprintln!("{title}: {message}");
    rfd::MessageDialog::new().set_level(rfd::MessageLevel::Error).set_title(title).set_description(message).show();
}

/// Writes unexpected failures to `crash.log` in the data folder and tells the user, instead of vanishing silently.
fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default(info);
        let dir = inkboard_mcp::live::data_dir();
        let _ = std::fs::create_dir_all(&dir);
        let log = dir.join("crash.log");
        let text = format!(
            "Inkboard {} crashed:
{info}
",
            env!("CARGO_PKG_VERSION")
        );
        let _ = std::fs::write(&log, &text);
        report(
            "Inkboard stopped unexpectedly",
            &format!(
                "{text}
This report was saved to {}.
Your last saved file is intact.",
                log.display()
            ),
        );
    }));
}

fn main() -> eframe::Result {
    let l = parse_args(&std::env::args().skip(1).collect::<Vec<_>>());
    if l.screenshot.is_none() && !l.bench && l.bench_icons.is_none() {
        install_panic_hook();
    }
    // OpenGL first (light and fast); if the graphics stack has no usable OpenGL (remote desktop, virtual machine, old driver), wgpu
    // (Direct3D 12 / Vulkan, with a software fallback) takes over.
    // `INKBOARD_RENDERER=wgpu` (or `glow`) forces one renderer, for troubleshooting
    let forced = std::env::var("INKBOARD_RENDERER").ok().map(|v| v.to_ascii_lowercase());
    if forced.as_deref() == Some("wgpu") {
        return launch(&l, eframe::Renderer::Wgpu).inspect_err(|e| report("Inkboard cannot open its window", &format!("Direct3D / Vulkan: {e}")));
    }
    match launch(&l, eframe::Renderer::Glow) {
        Ok(()) => Ok(()),
        Err(first) => {
            eprintln!("OpenGL start failed: {first}; trying wgpu");
            launch(&l, eframe::Renderer::Wgpu).inspect_err(|second| {
                report(
                    "Inkboard cannot open its window",
                    &format!(
                        "The graphics system could not be initialized.

OpenGL: {first}
Direct3D / Vulkan: {second}

Updating your graphics driver usually fixes this."
                    ),
                );
            })
        }
    }
}

fn launch(l: &Launch, renderer: eframe::Renderer) -> eframe::Result {
    let options = eframe::NativeOptions {
        renderer,
        viewport: {
            let mut v = egui::ViewportBuilder::default().with_inner_size([1360.0, 860.0]).with_min_inner_size([720.0, 480.0]).with_title("Inkboard");
            if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")) {
                v = v.with_icon(icon);
            }
            v
        },
        ..Default::default()
    };
    let l = l.clone();
    eframe::run_native(
        "Inkboard",
        options,
        Box::new(move |cc| {
            // pen eraser: winit/egui do not expose the eraser end, so it is observed through the Windows Pointer API
            {
                use raw_window_handle::{HasWindowHandle, RawWindowHandle};
                if let Ok(h) = cc.window_handle()
                    && let RawWindowHandle::Win32(w) = h.as_raw()
                {
                    inkboard_pen::install(w.hwnd);
                }
            }
            let mut a = app::App::new(cc, l.open.clone(), l.screenshot.clone());
            if l.screenshot.is_none() && !l.bench && l.bench_icons.is_none() {
                // live mode of inkboard-mcp: an agent can drive the displayed board
                a.live = live::LiveServer::start(cc.egui_ctx.clone(), inkboard_mcp::live::discovery_path()).ok();
            }
            if let Some((n, d)) = l.bench_icons
                && let Ok(lib) = inkboard_icons::Library::open(library::PACK)
            {
                a.ed = inkboard_core::Editor::new(bench_icons::document(&lib, n, d));
                a.icon_bench = Some(bench_icons::IconBench::new(n, d));
            }
            a.bench = l.bench.then(|| app::Bench { close_up: l.bench_close, ..Default::default() });
            a.edit_demo = l.edit_demo;
            a.formula_demo = l.formula_demo;
            a.ui_demo = l.ui_demo.clone();
            a.pen_test = l.pen_test.then(pen_test::PenReport::default);
            if l.shortcuts_demo {
                a.dialog = Some(app::Dialog::Shortcuts { filter: String::new(), recording: Some(keymap::Action::ToolPen) });
                let _ = a.keymap.add(keymap::Action::ToolEllipse, keymap::Shortcut::key(eframe::egui::Key::K));
            }
            Ok(Box::new(a))
        }),
    )
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_ui;
