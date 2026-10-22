//! Web entry point: mounts the application on the `<canvas id="inkboard_canvas">` of `web/index.html`.
//!
//! The page owns the "loading" indicator (`#loading`) and the error messages (`#error`, shown with `data-case="webgl"` or
//! `data-case="other"`): this file only switches them.

use crate::{app, platform};
use wasm_bindgen::JsCast as _;

#[cfg(not(any(feature = "web-glow", feature = "web-wgpu")))]
compile_error!("the web build needs a renderer: `--features web-glow` (WebGL, light) or `--features web-wgpu` (WebGPU with WebGL fallback)");

/// Value of the `log` query parameter (`?log=debug` shows more in the console; errors only by default).
fn log_level(query: &str) -> log::LevelFilter {
    let value = query.trim_start_matches('?').split('&').find_map(|kv| kv.strip_prefix("log="));
    match value {
        Some("debug") => log::LevelFilter::Debug,
        Some("info") => log::LevelFilter::Info,
        Some("warn") => log::LevelFilter::Warn,
        _ => log::LevelFilter::Error,
    }
}

/// Can this browser give a WebGL 2 context? (The `glow` painter cannot work without; `wgpu` has WebGPU as well.)
fn webgl_available(document: &web_sys::Document) -> bool {
    let probe = document.create_element("canvas").ok().and_then(|c| c.dyn_into::<web_sys::HtmlCanvasElement>().ok());
    probe.is_some_and(|c| matches!(c.get_context("webgl2"), Ok(Some(_))))
}

/// Hides the loading indicator.
fn hide_loading(document: &web_sys::Document) {
    if let Some(e) = document.get_element_by_id("loading") {
        e.remove();
    }
}

/// Replaces the loading indicator by the message `case` of the page (with the technical detail, when there is one).
fn show_error(document: &web_sys::Document, case: &str, detail: &str) {
    hide_loading(document);
    if let Some(e) = document.get_element_by_id("error") {
        let _ = e.set_attribute("data-case", case);
        let _ = e.remove_attribute("hidden");
    }
    if let Some(e) = document.get_element_by_id("error-detail") {
        e.set_text_content(Some(detail));
    }
}

pub fn start() {
    let Some(window) = web_sys::window() else { return };
    let Some(document) = window.document() else { return };
    eframe::WebLogger::init(log_level(&window.location().search().unwrap_or_default())).ok();
    // the ids come from a clock natively; a browser has none, and two browsers must never draw the same ids
    inkboard_core::model::seed_ids(platform::random_seed());
    let canvas = document.get_element_by_id("inkboard_canvas").and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok());
    let Some(canvas) = canvas else {
        show_error(&document, "other", "canvas#inkboard_canvas is missing from the page");
        return;
    };
    if cfg!(not(feature = "web-wgpu")) && !webgl_available(&document) {
        show_error(&document, "webgl", "WebGL 2 is not available");
        return;
    }
    wasm_bindgen_futures::spawn_local(async move {
        let result = eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|cc| {
                    platform::init(&cc.egui_ctx);
                    Ok(Box::new(app::App::new(cc, None, None)))
                }),
            )
            .await;
        match result {
            Ok(()) => hide_loading(&document),
            Err(e) => {
                web_sys::console::error_1(&e);
                show_error(&document, "other", &format!("{e:?}"));
            }
        }
    });
}
