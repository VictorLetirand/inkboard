//! Édition de texte riche : sélection de mots, gras / italique / souligné / barré, couleur, surlignage.
//!
//! La chaîne brute (`buf`) reste la source du champ de saisie ; `rich` porte les styles et suit chaque frappe
//! (`RichText::apply_edit`). Le champ affiche les styles grâce à un `layouter`.

use crate::app::App;
use crate::fonts::font_id;
use crate::keymap::Action;
use crate::render::{Cam, col, label_screen_rect};
use eframe::egui::text::{CCursor, CCursorRange, LayoutJob, TextFormat};
use eframe::egui::{self, Color32, Stroke, vec2};
use inkboard_core::style::PALETTE;
use inkboard_core::text::Format;
use inkboard_core::*;
use std::sync::Arc;

pub struct Editing {
    pub id: Id,
    /// Texte brut du champ de saisie.
    pub buf: String,
    /// Même contenu, avec les styles.
    pub rich: RichText,
    /// Dernière chaîne à laquelle `rich` correspond.
    pub synced: String,
    pub cell: Option<(usize, usize)>,
    /// Objet créé à l'instant : s'il reste vide, on annule sa création.
    pub fresh: bool,
    pub focused: bool,
    /// Sélection courante, en caractères (début ≤ fin).
    pub sel: (usize, usize),
    /// Sélection à rétablir après un clic sur la barre de mise en forme.
    pub restore_sel: Option<(usize, usize)>,
}

const HIGHLIGHTS: [Color; 8] = [
    Color::rgb(0xff, 0xec, 0x99),
    Color::rgb(0xff, 0xd8, 0xa8),
    Color::rgb(0xff, 0xc9, 0xc9),
    Color::rgb(0xb2, 0xf2, 0xbb),
    Color::rgb(0x99, 0xe9, 0xf2),
    Color::rgb(0xa5, 0xd8, 0xff),
    Color::rgb(0xd0, 0xbf, 0xff),
    Color::rgb(0xfc, 0xc2, 0xd7),
];

fn rich_of(o: &Object, cell: Option<(usize, usize)>) -> Option<RichText> {
    match &o.kind {
        Kind::Box(b) => Some(b.label.text.clone()),
        Kind::Table(t) => {
            let (r, c) = cell.unwrap_or((0, 0));
            Some(t.cell(r, c).cloned().unwrap_or_default())
        }
        Kind::Connector(c) => Some(c.label.as_ref().map(|l| l.text.clone()).unwrap_or_default()),
        _ => None,
    }
}

fn texts_mut(o: &mut Object) -> Vec<&mut RichText> {
    match &mut o.kind {
        Kind::Box(b) => vec![&mut b.label.text],
        Kind::Table(t) => t.cells.iter_mut().collect(),
        Kind::Connector(c) => c.label.iter_mut().map(|l| &mut l.text).collect(),
        _ => Vec::new(),
    }
}

impl App {
    pub fn start_editing(&mut self, id: &Id, cell: Option<(usize, usize)>) {
        if self.edit_formula(id) {
            return;
        }
        let Some(o) = self.ed.page().get(id) else { return };
        let cell = matches!(o.kind, Kind::Table(_)).then(|| cell.unwrap_or((0, 0)));
        let Some(rich) = rich_of(o, cell) else { return };
        let buf = rich.to_plain();
        self.editing = Some(Editing { id: id.clone(), synced: buf.clone(), buf, rich, cell, fresh: false, focused: false, sel: (0, 0), restore_sel: None });
    }

    pub fn commit_editing(&mut self) {
        let Some(mut e) = self.editing.take() else { return };
        if e.synced != e.buf {
            e.rich.apply_edit(&e.synced, &e.buf);
        }
        let empty = e.buf.trim().is_empty();
        let is_text_box = matches!(self.ed.page().get(&e.id).map(|o| &o.kind), Some(Kind::Box(b)) if b.geom == BoxGeom::Text);
        if e.fresh && empty && is_text_box {
            self.ed.undo();
            self.sel.clear();
            return;
        }
        let m = self.measurer.clone();
        let size = self.label.size;
        let rich = e.rich.clone();
        self.ed.update(&e.id, |o| match &mut o.kind {
            Kind::Box(b) => {
                if b.label.text != rich {
                    b.label.text = rich.clone();
                    b.fit(&m);
                }
            }
            Kind::Table(t) => {
                if let Some((r, c)) = e.cell
                    && let Some(cell) = t.cell_mut(r, c)
                {
                    *cell = rich.clone();
                }
            }
            Kind::Connector(c) => {
                let mut l = c.label.take().unwrap_or_else(|| Label::new("", size.min(16.0), Align::Center));
                l.text = rich.clone();
                c.label = (!rich.is_empty()).then_some(l);
            }
            _ => {}
        });
        if is_text_box && empty {
            self.ed.remove(std::slice::from_ref(&e.id));
            self.sel.clear();
        }
    }

    /// Applique une mise en forme : à la sélection du champ de saisie s'il est ouvert
    /// (tout le texte si rien n'est sélectionné), sinon à tout le texte des objets sélectionnés.
    pub fn apply_format(&mut self, f: Format) {
        if let Some(e) = self.editing.as_mut() {
            if e.synced != e.buf {
                e.rich.apply_edit(&e.synced, &e.buf);
                e.synced = e.buf.clone();
            }
            let len = e.rich.len_chars();
            let (a, b) = if e.sel.0 == e.sel.1 { (0, len) } else { e.sel };
            e.rich.format(a, b, f);
            e.restore_sel = Some(e.sel);
            return;
        }
        let ids: Vec<Id> = self.editable_selection().iter().map(|o| o.id.clone()).collect();
        let objs = self.editable_selection();
        let mut all_active = true;
        let mut any_text = false;
        for o in &objs {
            let mut c = o.clone();
            for t in texts_mut(&mut c) {
                let n = t.len_chars();
                if n > 0 {
                    any_text = true;
                    all_active &= f.is_active(t, 0, n);
                }
            }
        }
        if !any_text {
            return;
        }
        let m = self.measurer.clone();
        self.ed.update_many(&ids, |o| {
            for t in texts_mut(o) {
                let n = t.len_chars();
                // si tout est déjà actif on retire ; sinon on s'assure que c'est appliqué partout
                if n > 0 && (all_active || !f.is_active(t, 0, n)) {
                    t.format(0, n, f);
                }
            }
            if let Kind::Box(b) = &mut o.kind {
                b.fit(&m);
            }
        });
    }

    /// Mise en forme du texte sélectionné (ou en cours d'édition) : état « tout actif » pour la barre.
    pub fn format_active(&self, f: Format) -> bool {
        if let Some(e) = &self.editing {
            let len = e.rich.len_chars();
            let (a, b) = if e.sel.0 == e.sel.1 { (0, len) } else { e.sel };
            return f.is_active(&e.rich, a, b);
        }
        let objs = self.editable_selection();
        let mut any = false;
        for o in objs {
            let mut c = o.clone();
            for t in texts_mut(&mut c) {
                let n = t.len_chars();
                if n > 0 {
                    any = true;
                    if !f.is_active(t, 0, n) {
                        return false;
                    }
                }
            }
        }
        any
    }
}

/// Spans → texte mis en forme egui.
#[allow(clippy::too_many_arguments)]
fn rich_job(rich: &RichText, family: FontFamily, size: f32, base: Color32, dark: bool, wrap: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap;
    for sp in &rich.0 {
        let color = sp.style.color.map_or(base, |c| col(c, dark, 1.0));
        job.append(
            &sp.text,
            0.0,
            TextFormat {
                font_id: font_id(family, size, sp.style.bold),
                color,
                background: sp.style.highlight.map_or(Color32::TRANSPARENT, |h| col(h, dark, 0.55)),
                italics: sp.style.italic,
                underline: if sp.style.underline { Stroke::new(1.0_f32, color) } else { Stroke::NONE },
                strikethrough: if sp.style.strike { Stroke::new(1.0_f32, color) } else { Stroke::NONE },
                ..Default::default()
            },
        );
    }
    if rich.0.is_empty() {
        job.append("", 0.0, TextFormat { font_id: font_id(family, size, false), color: base, ..Default::default() });
    }
    job
}

/// Barre flottante de mise en forme ; renvoie l'action choisie.
fn format_bar(ui: &mut egui::Ui, app: &App) -> Option<Format> {
    let mut action = None;
    ui.horizontal(|ui| {
        for (label, f) in [("B", Format::Bold), ("I", Format::Italic), ("U", Format::Underline), ("S", Format::Strike)] {
            let mut t = egui::RichText::new(label);
            t = match f {
                Format::Bold => t.strong(),
                Format::Italic => t.italics(),
                Format::Underline => t.underline(),
                _ => t.strikethrough(),
            };
            if ui
                .selectable_label(app.format_active(f), t)
                .on_hover_text(match f {
                    Format::Bold => "Gras (Ctrl+B)",
                    Format::Italic => "Italique (Ctrl+I)",
                    Format::Underline => "Souligné (Ctrl+U)",
                    _ => "Barré",
                })
                .clicked()
            {
                action = Some(f);
            }
        }
        ui.separator();
        ui.label("A");
        for c in PALETTE.iter().skip(1) {
            let on = app.format_active(Format::Color(Some(*c)));
            if swatch(ui, *c, on, "Couleur du texte").clicked() {
                action = Some(Format::Color(Some(*c)));
            }
        }
        ui.separator();
        ui.label("🖍");
        for c in HIGHLIGHTS {
            let on = app.format_active(Format::Highlight(Some(c)));
            if swatch(ui, c, on, "Surligner").clicked() {
                action = Some(Format::Highlight(Some(c)));
            }
        }
        ui.separator();
        if ui.button("Effacer").on_hover_text("Retirer la mise en forme").clicked() {
            action = Some(Format::Clear);
        }
    });
    action
}

fn swatch(ui: &mut egui::Ui, c: Color, active: bool, tip: &str) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::click());
    let (rr, g, b) = c.resolve(false);
    ui.painter().rect_filled(r, 4.0, Color32::from_rgb(rr, g, b));
    ui.painter().rect_stroke(
        r,
        4.0,
        Stroke::new(if active { 2.5 } else { 1.0_f32 }, if active { Color32::from_rgb(0x4f, 0x6d, 0xf5) } else { Color32::from_black_alpha(70) }),
        egui::StrokeKind::Middle,
    );
    resp.on_hover_text(tip)
}

/// Champ de saisie superposé à l'objet édité, avec sa barre de mise en forme.
pub fn draw_text_editor(app: &mut App, ui: &mut egui::Ui, cam: &Cam) {
    let dark = app.ed.doc.background.preset.is_dark();
    let Some(ed) = app.editing.as_ref() else { return };
    let page = app.ed.page();
    let Some(o) = page.get(&ed.id) else {
        app.editing = None;
        return;
    };
    let (area, family, size, align, color, no_wrap): (egui::Rect, FontFamily, f32, Align, Color32, bool) = match &o.kind {
        Kind::Box(b) => {
            let color = if b.geom == BoxGeom::Sticky { Color32::from_rgb(0x1e, 0x1e, 0x1e) } else { col(o.style.stroke, dark, 1.0) };
            let mut area = label_screen_rect(cam, &b.frame, b.label_rect());
            if b.geom == BoxGeom::Text {
                area = area.expand2(vec2(40.0, 0.0));
            }
            (area, b.label.family, b.label.size as f32 * cam.zoom, b.label.align, color, b.geom == BoxGeom::Text)
        }
        Kind::Table(t) => {
            let (r, c) = ed.cell.unwrap_or((0, 0));
            let cr = t.cell_rect(r, c);
            (
                cam.rect_s(Rect::new(cr.x + 8.0, cr.y + 4.0, cr.w - 16.0, cr.h - 8.0)),
                t.family,
                t.size as f32 * cam.zoom,
                Align::Left,
                col(o.style.stroke, dark, 1.0),
                false,
            )
        }
        Kind::Connector(c) => {
            let Some(path) = page.connector_path(c) else { return };
            let (mid, _) = inkboard_core::connector::path_midpoint(&path);
            (
                egui::Rect::from_center_size(cam.s(mid), vec2(160.0, 24.0)),
                FontFamily::Sans,
                15.0 * cam.zoom,
                Align::Center,
                col(o.style.stroke, dark, 1.0),
                false,
            )
        }
        _ => return,
    };
    let ctx = ui.ctx().clone();
    let field_id = egui::Id::new("wb-text-edit-field");
    let halign = match align {
        Align::Left => egui::Align::LEFT,
        Align::Center => egui::Align::Center,
        Align::Right => egui::Align::RIGHT,
    };
    let size = size.max(8.0);

    // raccourcis de mise en forme (consommés avant que le champ ne les voie)
    let key_fmt = {
        let formats =
            [(Action::Bold, Format::Bold), (Action::Italic, Format::Italic), (Action::Underline, Format::Underline), (Action::Strike, Format::Strike)];
        let bindings: Vec<(Format, Vec<crate::keymap::Shortcut>)> = formats.iter().map(|(a, f)| (*f, app.keymap.bindings(*a).to_vec())).collect();
        ctx.input_mut(|i| bindings.iter().find_map(|(f, scs)| scs.iter().any(|sc| i.consume_key(sc.modifiers(), sc.key)).then_some(*f)))
    };
    if let Some(f) = key_fmt {
        app.apply_format(f);
    }

    // barre de mise en forme
    let bar_pos = {
        let y = if area.top() > 70.0 { area.top() - 44.0 } else { area.bottom() + 8.0 };
        egui::pos2(area.left().max(60.0), y)
    };
    let mut bar_action = None;
    egui::Area::new(egui::Id::new("wb-rich-bar")).order(egui::Order::Foreground).fade_in(false).fixed_pos(bar_pos).show(&ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| bar_action = format_bar(ui, app));
    });
    if let Some(f) = bar_action {
        app.apply_format(f);
    }

    let Some(ed) = app.editing.as_mut() else { return };
    if let Some((a, b)) = ed.restore_sel.take() {
        let mut st = egui::text_edit::TextEditState::load(&ctx, field_id).unwrap_or_default();
        st.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(a), CCursor::new(b))));
        st.store(&ctx, field_id);
        ctx.memory_mut(|m| m.request_focus(field_id));
    }
    let mut done = false;
    let width = area.width().max(60.0);
    egui::Area::new(egui::Id::new("wb-text-edit")).order(egui::Order::Foreground).fade_in(false).fixed_pos(area.min).show(&ctx, |ui| {
        ui.set_width(width);
        let Editing { buf, rich, synced, .. } = &mut *ed;
        let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap: f32| -> Arc<egui::Galley> {
            let t = text.as_str();
            if t != synced.as_str() {
                rich.apply_edit(synced, t);
                *synced = t.to_owned();
            }
            let job = rich_job(rich, family, size, color, dark, if no_wrap { f32::INFINITY } else { wrap });
            ui.ctx().fonts_mut(|f| f.layout_job(job))
        };
        let out = egui::TextEdit::multiline(buf)
            .id(field_id)
            .frame(false)
            .desired_width(width)
            .desired_rows(1)
            .horizontal_align(halign)
            .lock_focus(true)
            .layouter(&mut layouter)
            .show(ui);
        if let Some(r) = out.cursor_range {
            let (p, s) = (r.primary.index, r.secondary.index);
            ed.sel = (p.min(s), p.max(s));
        }
        if !ed.focused {
            out.response.request_focus();
            ed.focused = true;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape) || (i.key_pressed(egui::Key::Enter) && i.modifiers.command)) {
            done = true;
        }
    });
    if ed.synced != ed.buf {
        ed.rich.apply_edit(&ed.synced, &ed.buf);
        ed.synced = ed.buf.clone();
    }
    if done {
        app.commit_editing();
    }
}
