//! Presse-papiers : objets du tableau, texte et images du système.

use crate::app::App;
use crate::tools::Tool;
use wb_core::*;

/// Texte placé dans le presse-papiers système pour signaler « le contenu à coller est dans le presse-papiers interne ».
pub const MARKER: &str = "__tableau_blanc_objets__";

impl App {
    /// Copie la sélection (interne) et laisse un repère dans le presse-papiers système.
    pub fn copy_to_system(&mut self, ctx: &eframe::egui::Context) {
        self.copy_selection();
        if !self.clipboard.is_empty() {
            ctx.copy_text(MARKER.to_owned());
        }
    }

    /// Collage : objets internes, sinon image du système, sinon texte (nouvelle zone de texte).
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

    /// Insère une image décrite par ses pixels RGBA, centrée sur `at` (réduite pour tenir dans la vue).
    pub fn insert_rgba(&mut self, w: usize, h: usize, rgba: &[u8], at: Pt) {
        let Some(img) = image::RgbaImage::from_raw(w as u32, h as u32, rgba.to_vec()) else {
            return self.toast("Image du presse-papiers illisible");
        };
        let mut png = Vec::new();
        if img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).is_err() {
            return self.toast("Image du presse-papiers illisible");
        }
        let aid = self.ed.doc.add_asset("image/png", png);
        let (fw, fh) = (w as f64, h as f64);
        let k = (480.0 / fw).min(360.0 / fh).min(1.0);
        let frame = Frame::new(at.x - fw * k / 2.0, at.y - fh * k / 2.0, fw * k, fh * k);
        let id = self.ed.insert(Object::new(Kind::Image(ImageObj { frame, asset: aid, latex: None }), Style { width: 0.0, fill: None, ..Style::default() }));
        self.sel = vec![id];
        self.tool = Tool::Select;
    }

    /// Copie la sélection (ou tout le tableau) dans le presse-papiers sous forme d'image.
    pub fn copy_as_image(&mut self) {
        let page = self.ed.page().clone();
        let ids: Option<Vec<Id>> = (!self.sel.is_empty()).then(|| self.sel.clone());
        let Some(svg) = wb_core::svg::export(&self.ed.doc, &page, ids.as_deref(), &self.measurer, &Default::default()) else {
            return self.toast("Rien à copier");
        };
        let png = match wb_formats::raster::svg_to_png(&svg, 2.0) {
            Ok(p) => p,
            Err(e) => return self.toast(&format!("Copie impossible : {e}")),
        };
        let Ok(img) = image::load_from_memory(&png) else { return self.toast("Copie impossible") };
        let rgba = img.to_rgba8();
        let data = arboard::ImageData { width: rgba.width() as usize, height: rgba.height() as usize, bytes: rgba.into_raw().into() };
        match arboard::Clipboard::new().and_then(|mut c| c.set_image(data)) {
            Ok(()) => self.toast("Image copiée"),
            Err(e) => self.toast(&format!("Presse-papiers indisponible : {e}")),
        }
    }
}

/// Image du presse-papiers système, si elle existe (largeur, hauteur, RGBA).
pub fn system_image() -> Option<(usize, usize, Vec<u8>)> {
    let img = arboard::Clipboard::new().ok()?.get_image().ok()?;
    Some((img.width, img.height, img.bytes.into_owned()))
}
