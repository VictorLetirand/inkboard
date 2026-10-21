//! Clipboard: board objects, text and system images.

use crate::app::App;
use crate::i18n::{t, tr};
use crate::tools::Tool;
use inkboard_core::*;

/// Text placed in the system clipboard to signal "the content to paste is in the internal clipboard".
pub const MARKER: &str = "__inkboard_objects__";

impl App {
    /// Copies the selection (internally) and leaves a marker in the system clipboard.
    pub fn copy_to_system(&mut self, ctx: &eframe::egui::Context) {
        self.copy_selection();
        if !self.clipboard.is_empty() {
            ctx.copy_text(MARKER.to_owned());
        }
    }

    /// Paste: internal objects, otherwise the system image, otherwise text (a new text box).
    pub fn paste_event(&mut self, text: &str) {
        if text == MARKER && !self.clipboard.is_empty() {
            return self.paste();
        }
        if let Some((w, h, rgba)) = (self.system_image)() {
            let at = self.view_center();
            return self.insert_rgba(w, h, &rgba, at);
        }
        if !text.trim().is_empty() {
            return self.paste_text(text);
        }
        self.paste();
    }

    /// The "Paste" of the menus: like Ctrl+V (objects of the board, a system image, then the text of the clipboard).
    pub fn paste_from_menu(&mut self) {
        let text = system_text().unwrap_or_default();
        self.paste_event(&text);
    }

    pub fn paste_text(&mut self, text: &str) {
        let text: String = text.chars().take(5000).collect();
        let at = self.view_center();
        let mut o = Object::new_box(
            BoxGeom::Text,
            Frame::new(at.x, at.y, 10.0, 10.0),
            Style { width: 0.0, fill: None, ..self.style.clone() },
            Label { text: RichText::plain(text), align: Align::Left, ..self.label.clone() },
        );
        if let Kind::Box(b) = &mut o.kind {
            b.fit(&self.measurer);
            b.frame.x = at.x - b.frame.w / 2.0;
            b.frame.y = at.y - b.frame.h / 2.0;
        }
        let id = self.ed.insert(o);
        self.sel = vec![id];
        self.tool = Tool::Select;
    }

    /// Inserts an image given by its RGBA pixels, centered on `at` (shrunk to fit the view).
    pub fn insert_rgba(&mut self, w: usize, h: usize, rgba: &[u8], at: Pt) {
        let Some(img) = image::RgbaImage::from_raw(w as u32, h as u32, rgba.to_vec()) else {
            return self.toast(&t("Clipboard image unreadable"));
        };
        let mut png = Vec::new();
        if img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).is_err() {
            return self.toast(&t("Clipboard image unreadable"));
        }
        let aid = self.ed.doc.add_asset("image/png", png);
        let (fw, fh) = (w as f64, h as f64);
        let k = (480.0 / fw).min(360.0 / fh).min(1.0);
        let frame = Frame::new(at.x - fw * k / 2.0, at.y - fh * k / 2.0, fw * k, fh * k);
        let id = self
            .ed
            .insert(Object::new(Kind::Image(ImageObj { frame, asset: aid, latex: None, mono: false }), Style { width: 0.0, fill: None, ..Style::default() }));
        self.sel = vec![id];
        self.tool = Tool::Select;
    }

    /// Copies the selection (or the whole board) to the clipboard as an image.
    pub fn copy_as_image(&mut self) {
        let page = self.ed.page().clone();
        let ids: Option<Vec<Id>> = (!self.sel.is_empty()).then(|| self.sel.clone());
        let Some(svg) = inkboard_core::svg::export(&self.ed.doc, &page, ids.as_deref(), &self.measurer, &Default::default()) else {
            return self.toast(&t("Nothing to copy"));
        };
        let png = match inkboard_formats::raster::svg_to_png(&svg, 2.0) {
            Ok(p) => p,
            Err(e) => return self.toast(&tr!("Cannot copy: {}", e)),
        };
        let Ok(img) = image::load_from_memory(&png) else { return self.toast(&t("Cannot copy")) };
        let rgba = img.to_rgba8();
        let data = arboard::ImageData { width: rgba.width() as usize, height: rgba.height() as usize, bytes: rgba.into_raw().into() };
        match arboard::Clipboard::new().and_then(|mut c| c.set_image(data)) {
            Ok(()) => self.toast(&t("Image copied")),
            Err(e) => self.toast(&tr!("Clipboard unavailable: {}", e)),
        }
    }
}

/// Text from the system clipboard, if any.
pub fn system_text() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

/// Image from the system clipboard, if any (width, height, RGBA).
pub fn system_image() -> Option<(usize, usize, Vec<u8>)> {
    let img = arboard::Clipboard::new().ok()?.get_image().ok()?;
    Some((img.width, img.height, img.bytes.into_owned()))
}
