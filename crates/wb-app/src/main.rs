//! Tableau blanc : application de bureau (egui).

mod app;
mod clipboard;
mod fonts;
mod insert;
mod keymap;
mod live;
mod render;
mod rich_edit;
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
            other => open = Some(PathBuf::from(other)),
        }
    }
    let options = eframe::NativeOptions {
        viewport: {
            let mut v = egui::ViewportBuilder::default().with_inner_size([1360.0, 860.0]).with_min_inner_size([720.0, 480.0]).with_title("Tableau blanc");
            if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")) {
                v = v.with_icon(icon);
            }
            v
        },
        ..Default::default()
    };
    eframe::run_native(
        "Tableau blanc",
        options,
        Box::new(move |cc| {
            // gomme du stylet : winit/egui n'exposent pas l'extrémité gomme, on l'observe via l'API Pointer de Windows
            {
                use raw_window_handle::{HasWindowHandle, RawWindowHandle};
                if let Ok(h) = cc.window_handle()
                    && let RawWindowHandle::Win32(w) = h.as_raw()
                {
                    wb_pen::install(w.hwnd);
                }
            }
            let mut a = app::App::new(cc, open.clone(), screenshot.clone());
            if screenshot.is_none() && !bench {
                // mode direct de wb-mcp : un agent peut piloter le tableau affiché
                a.live = live::LiveServer::start(cc.egui_ctx.clone(), wb_mcp::live::discovery_path()).ok();
            }
            a.bench = bench.then(|| app::Bench { close_up: bench_close, ..Default::default() });
            a.edit_demo = edit_demo;
            a.formula_demo = formula_demo;
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
