//! Tableau blanc : application de bureau (egui).

mod app;
mod fonts;
mod keymap;
mod render;
mod rich_edit;
mod tools;
mod ui;

use eframe::egui;
use std::path::PathBuf;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut open: Option<PathBuf> = None;
    let mut screenshot: Option<PathBuf> = None;
    let mut bench = false;
    let mut edit_demo = false;
    let mut shortcuts_demo = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--screenshot" => screenshot = it.next().map(PathBuf::from),
            "--bench" => bench = true,
            "--edit-demo" => edit_demo = true,
            "--shortcuts-demo" => shortcuts_demo = true,
            other => open = Some(PathBuf::from(other)),
        }
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1360.0, 860.0]).with_min_inner_size([720.0, 480.0]).with_title("Tableau blanc"),
        ..Default::default()
    };
    eframe::run_native(
        "Tableau blanc",
        options,
        Box::new(move |cc| {
            let mut a = app::App::new(cc, open.clone(), screenshot.clone());
            a.bench = bench.then(app::Bench::default);
            a.edit_demo = edit_demo;
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
