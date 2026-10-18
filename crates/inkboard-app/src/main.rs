//! Inkboard : application de bureau (egui).

mod app;
mod chrome;
mod clipboard;
mod fonts;
mod i18n;
mod i18n_fr;
mod icons;
mod insert;
mod keymap;
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

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut open: Option<PathBuf> = None;
    let mut screenshot: Option<PathBuf> = None;
    let mut bench = false;
    let mut bench_close = false;
    let mut edit_demo = false;
    let mut shortcuts_demo = false;
    let mut formula_demo = false;
    let mut pen_test = false;
    let mut ui_demo = String::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--screenshot" => screenshot = it.next().map(PathBuf::from),
            "--bench" => bench = true,
            "--bench-close" => {
                bench = true;
                bench_close = true;
            }
            "--edit-demo" => edit_demo = true,
            "--shortcuts-demo" => shortcuts_demo = true,
            "--formula-demo" => formula_demo = true,
            "--pen-test" => pen_test = true,
            "--ui-demo" => ui_demo = it.next().cloned().unwrap_or_default(),
            other => open = Some(PathBuf::from(other)),
        }
    }
    let options = eframe::NativeOptions {
        viewport: {
            let mut v = egui::ViewportBuilder::default().with_inner_size([1360.0, 860.0]).with_min_inner_size([720.0, 480.0]).with_title("Inkboard");
            if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")) {
                v = v.with_icon(icon);
            }
            v
        },
        ..Default::default()
    };
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
            let mut a = app::App::new(cc, open.clone(), screenshot.clone());
            if screenshot.is_none() && !bench {
                // live mode of inkboard-mcp: an agent can drive the displayed board
                a.live = live::LiveServer::start(cc.egui_ctx.clone(), inkboard_mcp::live::discovery_path()).ok();
            }
            a.bench = bench.then(|| app::Bench { close_up: bench_close, ..Default::default() });
            a.edit_demo = edit_demo;
            a.formula_demo = formula_demo;
            a.ui_demo = ui_demo.clone();
            a.pen_test = pen_test.then(pen_test::PenReport::default);
            if shortcuts_demo {
                a.dialog = Some(app::Dialog::Shortcuts { filter: String::new(), recording: Some(keymap::Action::ToolPen) });
                let _ = a.keymap.add(keymap::Action::ToolEllipse, keymap::Shortcut::key(eframe::egui::Key::K));
            }
            Ok(Box::new(a))
        }),
    )
}

#[cfg(test)]
mod tests;
