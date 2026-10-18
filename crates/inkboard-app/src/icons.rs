//! Interface icons: the web prototype's SVG icon set (1.8 px stroke, rounded caps), rasterized once into a white texture
//! then tinted when drawn with the wanted color (text, accent, dimmed…).

use eframe::egui::{self, Color32, Rect, TextureHandle, TextureOptions, pos2};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// SVG content (24×24 viewBox) of each icon.
const ICONS: &[(&str, &str)] = &[
    ("menu", r#"<path d="M4 7h16M4 12h16M4 17h16"/>"#),
    ("select", r#"<path d="M5 3l14 8-6 2-3 6z"/>"#),
    (
        "hand",
        r#"<path d="M8 13V6.5a1.5 1.5 0 013 0V11M11 11V4.5a1.5 1.5 0 013 0V11M14 11V6.5a1.5 1.5 0 013 0V14c0 4-2.5 7-6.5 7-3 0-4.5-1.5-6.5-4.5L3.5 13a1.5 1.5 0 012.4-1.8L8 14"/>"#,
    ),
    ("pen", r#"<path d="M4 20l1-4L16.5 4.5a2 2 0 013 3L8 19z"/><path d="M14 7l3 3"/>"#),
    ("highlighter", r#"<path d="M9 11l4 4M5 19h4l10-10a2 2 0 000-3l-1-1a2 2 0 00-3 0L5 15z"/><path d="M4 21h16"/>"#),
    ("eraser", r#"<path d="M20 20H9L4 15a2 2 0 010-3l8-8a2 2 0 013 0l5 5a2 2 0 010 3l-7 8"/><path d="M8 8l8 8"/>"#),
    ("rect", r#"<rect x="4" y="6" width="16" height="12" rx="2"/>"#),
    ("ellipse", r#"<ellipse cx="12" cy="12" rx="9" ry="7"/>"#),
    ("diamond", r#"<path d="M12 3l9 9-9 9-9-9z"/>"#),
    ("triangle", r#"<path d="M12 4l9 16H3z"/>"#),
    ("line", r#"<path d="M5 19L19 5"/>"#),
    ("arrow", r#"<path d="M5 19L19 5M9 5h10v10"/>"#),
    ("text", r#"<path d="M5 7V4h14v3M12 4v16M9 20h6"/>"#),
    ("sticky", r#"<path d="M4 4h16v10l-6 6H4z"/><path d="M14 20v-6h6"/>"#),
    ("image", r#"<rect x="3" y="4" width="18" height="16" rx="2"/><circle cx="9" cy="10" r="1.6"/><path d="M21 16l-5-5L7 20"/>"#),
    ("table", r#"<rect x="4" y="5" width="16" height="14" rx="2"/><path d="M4 10h16M10 5v14M15 5v14"/>"#),
    ("vote", r#"<circle cx="12" cy="12" r="8"/><circle cx="12" cy="12" r="3.2" fill="currentColor"/>"#),
    (
        "laser",
        r#"<circle cx="12" cy="12" r="3"/><path d="M12 3v3M12 18v3M3 12h3M18 12h3M5.6 5.6l2.1 2.1M16.3 16.3l2.1 2.1M18.4 5.6l-2.1 2.1M7.7 16.3l-2.1 2.1"/>"#,
    ),
    ("undo", r#"<path d="M9 14L4 9l5-5"/><path d="M4 9h10a6 6 0 010 12h-3"/>"#),
    ("redo", r#"<path d="M15 14l5-5-5-5"/><path d="M20 9H10a6 6 0 000 12h3"/>"#),
    ("plus", r#"<path d="M12 5v14M5 12h14"/>"#),
    ("minus", r#"<path d="M5 12h14"/>"#),
    ("fit", r#"<path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"/>"#),
    ("help", r#"<circle cx="12" cy="12" r="9"/><path d="M9.5 9.5a2.5 2.5 0 115 .5c0 1.5-2.5 2-2.5 3.5M12 17h.01"/>"#),
    ("lock", r#"<rect x="5" y="11" width="14" height="9" rx="2"/><path d="M8 11V8a4 4 0 018 0v3"/>"#),
    ("unlock", r#"<rect x="5" y="11" width="14" height="9" rx="2"/><path d="M8 11V8a4 4 0 017.5-2"/>"#),
    ("trash", r#"<path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3"/>"#),
    ("copy", r#"<rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V6a2 2 0 00-2-2H6a2 2 0 00-2 2v8a2 2 0 002 2h2"/>"#),
    ("cut", r#"<circle cx="6" cy="18" r="3"/><circle cx="18" cy="18" r="3"/><path d="M8 16L18 4M16 16L6 4"/>"#),
    ("paste", r#"<rect x="6" y="5" width="12" height="16" rx="2"/><path d="M9 5V4h6v1"/>"#),
    ("group", r#"<rect x="3" y="3" width="8" height="8" rx="1"/><rect x="13" y="13" width="8" height="8" rx="1"/><path d="M11 7h4a2 2 0 012 2v4"/>"#),
    ("ungroup", r#"<rect x="3" y="3" width="8" height="8" rx="1"/><rect x="13" y="13" width="8" height="8" rx="1"/>"#),
    ("front", r#"<rect x="3" y="3" width="11" height="11" rx="1" fill="currentColor" fill-opacity=".25"/><rect x="10" y="10" width="11" height="11" rx="1"/>"#),
    ("forward", r#"<path d="M12 4v12M7 9l5-5 5 5M5 20h14"/>"#),
    ("backward", r#"<path d="M12 20V8M7 15l5 5 5-5M5 4h14"/>"#),
    ("back", r#"<rect x="10" y="10" width="11" height="11" rx="1" fill="currentColor" fill-opacity=".25"/><rect x="3" y="3" width="11" height="11" rx="1"/>"#),
    ("al", r#"<path d="M4 3v18M8 7h12M8 14h8"/>"#),
    ("ac", r#"<path d="M12 3v18M5 7h14M8 14h8"/>"#),
    ("ar", r#"<path d="M20 3v18M4 7h12M8 14h8"/>"#),
    ("at", r#"<path d="M3 4h18M7 8v12M14 8v8"/>"#),
    ("am", r#"<path d="M3 12h18M7 5v14M14 8v8"/>"#),
    ("ab", r#"<path d="M3 20h18M7 4v12M14 8v8"/>"#),
    ("dh", r#"<path d="M4 4v16M20 4v16M10 8h4v8h-4z"/>"#),
    ("dv", r#"<path d="M4 4h16M4 20h16M8 10v4h8v-4z"/>"#),
    ("folder", r#"<path d="M3 7a2 2 0 012-2h4l2 2h8a2 2 0 012 2v8a2 2 0 01-2 2H5a2 2 0 01-2-2z"/>"#),
    ("filenew", r#"<path d="M6 3h8l5 5v13H6z"/><path d="M14 3v5h5M12 12v6M9 15h6"/>"#),
    ("save", r#"<path d="M5 3h12l3 3v15H5z"/><path d="M8 3v6h8V3M8 21v-7h8v7"/>"#),
    ("download", r#"<path d="M12 4v11M7 11l5 5 5-5M5 20h14"/>"#),
    ("grid", r#"<path d="M4 4h16v16H4zM4 12h16M12 4v16"/>"#),
    ("full", r#"<path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"/>"#),
    ("play", r#"<path d="M7 4l13 8-13 8z"/>"#),
    ("timer", r#"<circle cx="12" cy="13" r="7.5"/><path d="M12 9v4l3 2M9.5 3h5"/>"#),
    ("code", r#"<path d="M9 8l-5 4 5 4M15 8l5 4-5 4"/>"#),
    ("formula", r#"<path d="M18 5H7l6 7-6 7h11"/>"#),
    ("mermaid", r#"<path d="M4 5h6v5H4zM14 14h6v5h-6zM10 7.5h4a2 2 0 012 2V14"/>"#),
    ("keyboard", r#"<rect x="3" y="6" width="18" height="12" rx="2"/><path d="M7 10h.01M11 10h.01M15 10h.01M7 14h10"/>"#),
    ("edit", r#"<path d="M4 20l1-4L16.5 4.5a2 2 0 013 3L8 19z"/>"#),
    ("all", r#"<path d="M4 4h16v16H4z" stroke-dasharray="3 3"/>"#),
    ("alignL", r#"<path d="M4 6h16M4 12h10M4 18h14"/>"#),
    ("alignC", r#"<path d="M4 6h16M7 12h10M5 18h14"/>"#),
    ("alignR", r#"<path d="M4 6h16M10 12h10M6 18h14"/>"#),
    ("trashAll", r#"<path d="M4 7h16M6 7l1 13h10l1-13M9 7V4h6v3"/>"#),
    ("page", r#"<path d="M6 3h8l5 5v13H6z"/><path d="M14 3v5h5"/>"#),
    ("layers", r#"<path d="M12 3l9 5-9 5-9-5zM3 13l9 5 9-5"/>"#),
    ("check", r#"<path d="M5 12.5l4.5 4.5L19 7.5"/>"#),
];

/// Rasterization resolution: 3 pixels per viewBox unit (sharp up to a 72 px icon).
const SCALE: f32 = 3.0;

fn svg_of(inner: &str) -> String {
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" color="#ffffff" fill="none" stroke="#ffffff" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">{inner}</svg>"##
    )
}

/// Textures already created (one per egui context: a test context shares nothing with the real window).
#[derive(Clone, Default)]
struct Cache(Arc<Mutex<HashMap<&'static str, TextureHandle>>>);

/// White texture of icon `name` (`None` when the name is unknown).
pub fn texture(ctx: &egui::Context, name: &str) -> Option<TextureHandle> {
    let (key, inner) = ICONS.iter().find(|(n, _)| *n == name)?;
    let cache: Cache = ctx.data_mut(|d| d.get_temp_mut_or_default::<Cache>(egui::Id::new("wb-icons")).clone());
    let mut map = cache.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(t) = map.get(key) {
        return Some(t.clone());
    }
    let r = inkboard_formats::raster::svg_to_raster(&svg_of(inner), SCALE, false).ok()?;
    let img = egui::ColorImage::from_rgba_premultiplied([r.width as usize, r.height as usize], &r.rgba_premultiplied);
    let t = ctx.load_texture(format!("icon-{key}"), img, TextureOptions::LINEAR);
    map.insert(key, t.clone());
    Some(t)
}

/// Draws icon `name` in `rect`, tinted with `color`.
pub fn paint(painter: &egui::Painter, name: &str, rect: Rect, color: Color32) {
    if let Some(t) = texture(painter.ctx(), name) {
        painter.image(t.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), color);
    }
}

/// Names of the available icons (for tests).
#[cfg(test)]
pub fn names() -> impl Iterator<Item = &'static str> {
    ICONS.iter().map(|(n, _)| *n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_rasterizes_to_visible_pixels() {
        for (name, inner) in ICONS {
            let r = inkboard_formats::raster::svg_to_raster(&svg_of(inner), SCALE, false).unwrap_or_else(|e| panic!("{name} : {e}"));
            assert_eq!((r.width, r.height), (72, 72), "{name}");
            let opaque = r.rgba_premultiplied.chunks(4).filter(|p| p[3] > 0).count();
            assert!(opaque > 60, "{name}: empty icon ({opaque} pixels)");
        }
    }

    #[test]
    fn icon_names_are_unique() {
        let mut v: Vec<_> = names().collect();
        v.sort_unstable();
        let n = v.len();
        v.dedup();
        assert_eq!(v.len(), n);
    }
}
