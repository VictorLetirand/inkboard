//! Insertion d'éléments préfabriqués : bloc de code, formule (aperçu Unicode), pastille de vote.

use crate::app::App;
use crate::tools::Tool;
use wb_core::*;

const CODE_SAMPLE: &str = "fn main() {\n    println!(\"Bonjour\");\n}";

impl App {
    fn place_box(&mut self, mut o: Object) {
        let at = self.view_center();
        if let Kind::Box(b) = &mut o.kind {
            b.fit(&self.measurer);
            b.frame.x = at.x - b.frame.w / 2.0;
            b.frame.y = at.y - b.frame.h / 2.0;
        }
        let id = self.ed.insert(o);
        self.sel = vec![id];
        self.tool = Tool::Select;
    }

    /// Bloc de code : rectangle sombre, police à chasse fixe, texte aligné à gauche.
    pub fn insert_code_block(&mut self) {
        let style = Style { stroke: Color::rgb(0xe6, 0xe6, 0xe6), fill: Some(Color::rgb(0x1e, 0x20, 0x2e)), width: 0.0, roundness: 0.1, ..Style::default() };
        let label = Label { text: RichText::plain(CODE_SAMPLE), family: FontFamily::Mono, size: 18.0, align: Align::Left };
        let mut o = Object::new_box(BoxGeom::Rect, Frame::new(0.0, 0.0, 420.0, 120.0), style, label);
        if let Kind::Box(b) = &mut o.kind {
            let lay = b.layout(&self.measurer);
            b.frame.w = (lay.width + 48.0).max(260.0);
            b.frame.h = (lay.height + 40.0).max(90.0);
        }
        self.place_box(o);
        self.toast("Double-clic pour modifier le code");
    }

    /// Formule : le source LaTeX est converti en Unicode (approximation) dans une zone de texte à police serif.
    pub fn insert_formula(&mut self, latex: &str) {
        let text = wb_core::latex::to_unicode(latex);
        if text.trim().is_empty() {
            return;
        }
        let style = Style { width: 0.0, fill: None, ..self.style.clone() };
        let label = Label { text: RichText::plain(text), family: FontFamily::Serif, size: 32.0, align: Align::Center };
        self.place_box(Object::new_box(BoxGeom::Text, Frame::new(0.0, 0.0, 10.0, 10.0), style, label));
    }

    /// Pastille de vote numérotée : petit disque coloré, à poser sur les éléments à départager.
    pub fn insert_vote_dot(&mut self, at: Pt) {
        let style = Style { stroke: Color::rgb(0xff, 0xff, 0xff), fill: Some(Color::rgb(0xe0, 0x31, 0x31)), width: 0.0, ..Style::default() };
        let label = Label { text: RichText::plain("1"), family: FontFamily::Sans, size: 16.0, align: Align::Center };
        let o = Object::new_box(BoxGeom::Ellipse, Frame::new(at.x - 14.0, at.y - 14.0, 28.0, 28.0), style, label);
        let id = self.ed.insert(o);
        self.sel = vec![id];
    }
}
