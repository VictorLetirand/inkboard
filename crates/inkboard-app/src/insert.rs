//! Insertion d'éléments préfabriqués : bloc de code, formule (aperçu Unicode), pastille de vote.

use crate::app::{App, Dialog, FormulaPreview};
use crate::tools::Tool;
use inkboard_core::*;

/// Corps du texte d'une formule à l'insertion (unités du tableau).
const FORMULA_SIZE: f64 = 36.0;
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
        let label = Label { text: inkboard_core::highlight::highlight(CODE_SAMPLE), family: FontFamily::Mono, size: 18.0, align: Align::Left };
        let mut o = Object::new_box(BoxGeom::Rect, Frame::new(0.0, 0.0, 420.0, 120.0), style, label);
        if let Kind::Box(b) = &mut o.kind {
            let lay = b.layout(&self.measurer);
            b.frame.w = (lay.width + 48.0).max(260.0);
            b.frame.h = (lay.height + 40.0).max(90.0);
        }
        self.place_box(o);
        self.toast("Double-clic pour modifier le code");
    }

    /// Recolore le texte d'une forme à chasse fixe (la mise en forme existante est remplacée par la coloration syntaxique).
    pub fn recolor_code(&mut self, id: &Id) {
        self.ed.update(id, |o| {
            if let Kind::Box(b) = &mut o.kind {
                b.label.text = inkboard_core::highlight::highlight(&b.label.text.to_plain());
            }
        });
    }

    /// Insère une formule composée (SVG blanc teinté par la couleur de trait, source conservée) ou, avec `editing`, remplace celle d'un objet existant
    /// en gardant son centre, son angle et son échelle.
    pub fn insert_formula(&mut self, latex: &str, editing: Option<&Id>) -> Result<(), String> {
        let f = inkboard_formats::math::render(latex, FORMULA_SIZE).map_err(|e| e.to_string())?;
        let aid = self.ed.doc.add_asset("image/svg+xml", f.svg.into_bytes());
        let src = latex.to_owned();
        if let Some(id) = editing {
            let old_h = match self.ed.page().get(id).map(|o| &o.kind) {
                Some(Kind::Image(i)) => i.latex.as_deref().and_then(|l| inkboard_formats::math::render(l, FORMULA_SIZE).ok()).map(|f| f.h / i.frame.h.max(1.0)),
                _ => None,
            };
            let k = old_h.map_or(1.0, |r| 1.0 / r);
            self.ed.update(id, |o| {
                if let Kind::Image(i) = &mut o.kind {
                    let c = i.frame.center();
                    i.frame = Frame { x: c.x - f.w * k / 2.0, y: c.y - f.h * k / 2.0, w: f.w * k, h: f.h * k, angle: i.frame.angle };
                    i.asset = aid;
                    i.latex = Some(src);
                }
            });
            return Ok(());
        }
        let at = self.view_center();
        let frame = Frame::new(at.x - f.w / 2.0, at.y - f.h / 2.0, f.w, f.h);
        let style = Style { stroke: self.style.stroke, width: 0.0, fill: None, ..Style::default() };
        let id = self.ed.insert(Object::new(Kind::Image(ImageObj { frame, asset: aid, latex: Some(src) }), style));
        self.sel = vec![id];
        self.tool = Tool::Select;
        Ok(())
    }

    /// Ouvre la fenêtre de modification de la formule `id` (sans effet si ce n'est pas une formule).
    pub fn edit_formula(&mut self, id: &Id) -> bool {
        let Some(Kind::Image(ImageObj { latex: Some(src), .. })) = self.ed.page().get(id).map(|o| &o.kind) else { return false };
        self.dialog = Some(Dialog::Formula { src: src.clone(), editing: Some(id.clone()), preview: FormulaPreview::default() });
        true
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
