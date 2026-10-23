//! Rich-text editing: word selection, bold / italic / underline / strikethrough, color, highlight.
//!
//! The raw string (`buf`) remains the source of the input field; `rich` carries the styles and follows every keystroke
//! (`RichText::apply_edit`). The field displays the styles through a `layouter`.

use crate::app::App;
use crate::fonts::font_id;
use crate::i18n::t;
use crate::keymap::Action;
use crate::render::{Cam, col, label_screen_rect};
use eframe::egui::text::{CCursor, CCursorRange, LayoutJob, TextFormat};
use eframe::egui::{self, Color32, Stroke, vec2};
use inkboard_core::style::PALETTE;
use inkboard_core::text::Format;
use inkboard_core::*;
use std::sync::Arc;

/// Where the editor put the lines of text on the last frame (screen coordinates), for tests.
#[derive(Clone, Debug, Default)]
#[cfg_attr(not(test), allow(dead_code))]
pub struct EditorProbe {
    /// Left and top of every non-empty row.
    pub rows: Vec<(f32, f32)>,
    /// Screen position (x, top) of the caret.
    pub caret: Option<(f32, f32)>,
}

pub struct Editing {
    pub id: Id,
    /// Raw text of the input field.
    pub buf: String,
    /// Same content, with the styles.
    pub rich: RichText,
    /// Last string `rich` corresponds to.
    pub synced: String,
    pub cell: Option<(usize, usize)>,
    /// Object just created: if it stays empty, its creation is undone.
    pub fresh: bool,
    pub focused: bool,
    /// Current selection, in characters (start ≤ end).
    pub sel: (usize, usize),
    /// Selection to restore after a click on the formatting bar.
    pub restore_sel: Option<(usize, usize)>,
    /// The object as it was when editing started: the frame follows the text while typing (not recorded in the history), and the
    /// history gets a single before / after when editing ends.
    pub orig: Object,
    /// Plain text when editing started.
    pub start_text: String,
    /// Text alignment chosen from the formatting bar during this editing.
    pub align: Option<Align>,
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

/// Puts `rich` in the text of an object (a box, a table cell or a connector label), fitting the shape to it; the same code serves the
/// validation of an editing and the safety copy of the document made while editing.
fn apply_text(o: &mut Object, cell: Option<(usize, usize)>, rich: &RichText, default_size: f64, align: Option<Align>, m: &dyn Measurer) {
    match &mut o.kind {
        Kind::Box(b) => {
            let new_align = align.unwrap_or(b.label.align);
            if b.label.text != *rich || b.label.align != new_align || b.geom == BoxGeom::Text {
                b.label.text = rich.clone();
                b.label.align = new_align;
                if let Some(lang) = b.lang {
                    b.label.text = inkboard_core::highlight::highlight_lang(&rich.to_plain(), lang);
                }
                b.fit(m);
            }
        }
        Kind::Table(t) => {
            if let Some((r, c)) = cell
                && let Some(cell) = t.cell_mut(r, c)
            {
                *cell = rich.clone();
            }
        }
        Kind::Connector(c) => {
            let mut l = c.label.take().unwrap_or_else(|| Label::new("", default_size.min(16.0), Align::Center));
            l.text = rich.clone();
            c.label = (!rich.is_empty()).then_some(l);
        }
        _ => {}
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
        let orig = o.clone();
        self.editing = Some(Editing {
            id: id.clone(),
            synced: buf.clone(),
            start_text: buf.clone(),
            buf,
            rich,
            cell,
            fresh: false,
            focused: false,
            sel: (0, 0),
            restore_sel: None,
            orig,
            align: None,
        });
    }

    /// Is some typed text or alignment change not yet part of the document?
    pub fn editing_pending(&self) -> bool {
        self.editing.as_ref().is_some_and(|e| e.buf != e.start_text || e.align.is_some())
    }

    /// The document as it will be once the current editing is validated (without touching the history), to save it safely; `None` when
    /// nothing is pending.
    pub fn pending_document(&self) -> Option<Document> {
        let e = self.editing.as_ref().filter(|_| self.editing_pending())?;
        let mut rich = e.rich.clone();
        if e.synced != e.buf {
            rich.apply_edit(&e.synced, &e.buf);
        }
        let mut doc = self.ed.doc.clone();
        let page = doc.pages.iter_mut().find(|p| p.index_of(&e.id).is_some())?;
        let i = page.index_of(&e.id)?;
        page.objects[i] = e.orig.clone();
        apply_text(&mut page.objects[i], e.cell, &rich, self.label.size, e.align, &self.measurer);
        Some(doc)
    }

    /// Keeps the frame of the edited shape in step with the text being typed (a free text grows with its text, a shape grows when the text
    /// overflows), without recording it: the final result is exactly what the user sees while typing.
    fn live_fit_edited(&mut self) {
        let Some(e) = self.editing.as_mut() else { return };
        if e.synced != e.buf {
            e.rich.apply_edit(&e.synced, &e.buf);
            e.synced = e.buf.clone();
        }
        let (id, rich, align) = (e.id.clone(), e.rich.clone(), e.align);
        // always fitted from the object as it was before editing (as the validation does), so that fitting twice cannot make it grow twice
        let Kind::Box(orig) = &e.orig.kind else { return };
        let mut fitted = orig.clone();
        let Some(Kind::Box(b)) = self.ed.page().get(&id).map(|o| &o.kind) else { return };
        let new_align = align.unwrap_or(fitted.label.align);
        // nothing changed: nothing to fit (the validation does the same), except a free text whose frame is always its text's
        if fitted.label.text != rich || fitted.label.align != new_align || fitted.geom == BoxGeom::Text {
            fitted.label.text = rich;
            fitted.label.align = new_align;
            if let Some(lang) = fitted.lang {
                fitted.label.text = inkboard_core::highlight::highlight_lang(&fitted.label.text.to_plain(), lang);
            }
            fitted.fit(&self.measurer);
        }
        if fitted.frame != b.frame || fitted.label.align != b.label.align {
            let mut o = self.ed.page().get(&id).cloned().expect("the object was just read");
            o.kind = Kind::Box(BoxObj { label: Label { text: b.label.text.clone(), ..fitted.label.clone() }, ..fitted });
            self.set_live(&[o]);
        }
    }

    pub fn commit_editing(&mut self) {
        let Some(mut e) = self.editing.take() else { return };
        if e.synced != e.buf {
            e.rich.apply_edit(&e.synced, &e.buf);
        }
        let empty = e.buf.trim().is_empty();
        let is_text_box = matches!(self.ed.page().get(&e.id).map(|o| &o.kind), Some(Kind::Box(b)) if b.geom == BoxGeom::Text);
        // back to the object as it was when editing started: the history then records one before / after
        if self.ed.page().get(&e.id).is_some() {
            self.set_live(std::slice::from_ref(&e.orig));
        }
        if e.fresh && empty && is_text_box {
            self.ed.undo();
            self.sel.clear();
            return;
        }
        let m = self.measurer.clone();
        let size = self.label.size;
        let rich = e.rich.clone();
        self.ed.update(&e.id, |o| apply_text(o, e.cell, &rich, size, e.align, &m));
        if is_text_box && empty {
            self.ed.remove(std::slice::from_ref(&e.id));
            self.sel.clear();
        }
    }

    /// Applies a format: to the selection of the input field when it is open
    /// (all the text when nothing is selected), otherwise to all the text of the selected objects.
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
                // if everything is already active remove it; otherwise make sure it is applied everywhere
                if n > 0 && (all_active || !f.is_active(t, 0, n)) {
                    t.format(0, n, f);
                }
            }
            if let Kind::Box(b) = &mut o.kind {
                b.fit(&m);
            }
        });
    }

    /// Format of the selected (or edited) text: "all active" state for the bar.
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

/// Spans → egui formatted text.
#[allow(clippy::too_many_arguments)]
fn rich_job(rich: &RichText, family: FontFamily, size: f32, line_height: f32, base: Color32, dark: bool, wrap: f32, halign: egui::Align) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap;
    job.halign = halign;
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
                line_height: Some(line_height),
                underline: if sp.style.underline { Stroke::new(1.0_f32, color) } else { Stroke::NONE },
                ..Default::default()
            },
        );
    }
    if rich.0.is_empty() {
        job.append("", 0.0, TextFormat { font_id: font_id(family, size, false), color: base, line_height: Some(line_height), ..Default::default() });
    }
    job
}

/// Choice made in the floating formatting bar.
enum BarAction {
    Format(Format),
    Align(Align),
}

/// Text-style button (B / I / U / S) drawn by hand so that the underline and strikethrough sit on the letter itself
/// (egui draws them relative to the whole text row, which looks off-center on a single capital).
fn style_button(ui: &mut egui::Ui, f: Format, active: bool, tip: &str) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(30.0, 26.0), egui::Sense::click());
    let st = ui.style().interact_selectable(&resp, active);
    ui.painter().rect(r, st.corner_radius, st.bg_fill, st.bg_stroke, egui::StrokeKind::Inside);
    let (label, font) = match f {
        Format::Bold => ("B", font_id(FontFamily::Sans, 15.0, true)),
        Format::Italic => ("I", font_id(FontFamily::Sans, 15.0, false)),
        Format::Underline => ("U", font_id(FontFamily::Sans, 15.0, false)),
        _ => ("S", font_id(FontFamily::Sans, 15.0, false)),
    };
    let color = st.fg_stroke.color;
    let job = LayoutJob::single_section(label.to_owned(), TextFormat { font_id: font, color, italics: f == Format::Italic, ..Default::default() });
    let galley = ui.painter().layout_job(job);
    let pos = r.center() - galley.size() / 2.0;
    let glyph = egui::Rect::from_min_size(pos, galley.size());
    ui.painter().galley(pos, galley, color);
    // capital letters occupy roughly the middle 70 % of the row, slightly above its center
    let cap_center = glyph.center().y - glyph.height() * 0.04;
    let cap_bottom = cap_center + glyph.height() * 0.30;
    let (x0, x1) = (glyph.center().x - 6.0, glyph.center().x + 6.0);
    match f {
        Format::Underline => {
            ui.painter().hline(x0..=x1, cap_bottom + 1.5, Stroke::new(1.3_f32, color));
        }
        Format::Strike => {
            ui.painter().hline(x0..=x1, glyph.center().y + glyph.height() * 0.05, Stroke::new(1.3_f32, color));
        }
        _ => {}
    }
    resp.on_hover_text(t(tip))
}

/// Floating formatting bar; returns the chosen action. `align`: current text alignment of the edited box (`None`: not applicable).
fn format_bar(ui: &mut egui::Ui, app: &App, align: Option<Align>) -> Option<BarAction> {
    let mut action = None;
    ui.horizontal(|ui| {
        for (f, tip) in
            [(Format::Bold, "Bold (Ctrl+B)"), (Format::Italic, "Italic (Ctrl+I)"), (Format::Underline, "Underline (Ctrl+U)"), (Format::Strike, "Strikethrough")]
        {
            if style_button(ui, f, app.format_active(f), tip).clicked() {
                action = Some(BarAction::Format(f));
            }
        }
        if let Some(cur) = align {
            ui.separator();
            for (a, icon, tip) in
                [(Align::Left, "alignL", "Align text left"), (Align::Center, "alignC", "Center text"), (Align::Right, "alignR", "Align text right")]
            {
                let (r, resp) = ui.allocate_exact_size(vec2(30.0, 26.0), egui::Sense::click());
                let st = ui.style().interact_selectable(&resp, cur == a);
                ui.painter().rect(r, st.corner_radius, st.bg_fill, st.bg_stroke, egui::StrokeKind::Inside);
                crate::icons::paint(ui.painter(), icon, egui::Rect::from_center_size(r.center(), vec2(16.0, 16.0)), st.fg_stroke.color);
                if resp.on_hover_text(t(tip)).clicked() {
                    action = Some(BarAction::Align(a));
                }
            }
        }
        ui.separator();
        ui.label("A");
        for c in PALETTE.iter().skip(1) {
            let on = app.format_active(Format::Color(Some(*c)));
            if swatch(ui, *c, on, "Text color").clicked() {
                action = Some(BarAction::Format(Format::Color(Some(*c))));
            }
        }
        ui.separator();
        ui.label("🖍");
        for c in HIGHLIGHTS {
            let on = app.format_active(Format::Highlight(Some(c)));
            if swatch(ui, c, on, "Highlight").clicked() {
                action = Some(BarAction::Format(Format::Highlight(Some(c))));
            }
        }
        ui.separator();
        if ui.button(t("Clear")).on_hover_text(t("Remove the formatting")).clicked() {
            action = Some(BarAction::Format(Format::Clear));
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
    resp.on_hover_text(t(tip))
}

/// Input field overlaid on the edited object, with its formatting bar.
pub fn draw_text_editor(app: &mut App, ui: &mut egui::Ui, cam: &Cam) {
    app.live_fit_edited();
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
            let area = label_screen_rect(cam, &b.frame, b.label_rect());
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
    // the formatting bar is attached to the whole edited object
    let anchor = match &o.kind {
        Kind::Box(b) => cam.rect_s(b.frame.aabb()),
        Kind::Table(t) => cam.rect_s(t.rect()),
        _ => area,
    };
    // same layout as the final rendering: wrap width (model units) and text alignment of the box
    let bar_align = matches!(o.kind, Kind::Box(_)).then_some(align);
    let lay_wrap = match &o.kind {
        Kind::Box(b) if b.geom != BoxGeom::Text => Some(f64::from(area.width() / cam.zoom)),
        Kind::Table(_) => Some(f64::from(area.width() / cam.zoom)),
        _ => None,
    };
    let model_size = f64::from(size / cam.zoom);
    let zoom = cam.zoom;
    let measurer = app.measurer.clone();
    let ctx = ui.ctx().clone();
    let field_id = egui::Id::new("wb-text-edit-field");
    let halign = match align {
        Align::Left => egui::Align::LEFT,
        Align::Center => egui::Align::Center,
        Align::Right => egui::Align::RIGHT,
    };
    let size = size.max(8.0);

    // formatting shortcuts (consumed before the field sees them)
    let key_fmt = {
        let formats =
            [(Action::Bold, Format::Bold), (Action::Italic, Format::Italic), (Action::Underline, Format::Underline), (Action::Strike, Format::Strike)];
        let bindings: Vec<(Format, Vec<crate::keymap::Shortcut>)> = formats.iter().map(|(a, f)| (*f, app.keymap.bindings(*a).to_vec())).collect();
        ctx.input_mut(|i| bindings.iter().find_map(|(f, scs)| scs.iter().any(|sc| i.consume_key(sc.modifiers(), sc.key)).then_some(*f)))
    };
    if let Some(f) = key_fmt {
        app.apply_format(f);
    }

    // formatting bar
    // just above the object (below it when there is no room above), its left edge aligned with the object's
    let above = anchor.top() > 90.0;
    let (pivot, bar_pos) = if above {
        (egui::Align2::LEFT_BOTTOM, egui::pos2(anchor.left().max(12.0), anchor.top() - 12.0))
    } else {
        (egui::Align2::LEFT_TOP, egui::pos2(anchor.left().max(12.0), anchor.bottom() + 12.0))
    };
    let mut bar_action = None;
    egui::Area::new(egui::Id::new("wb-rich-bar")).order(egui::Order::Foreground).fade_in(false).pivot(pivot).fixed_pos(bar_pos).show(&ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| bar_action = format_bar(ui, app, bar_align));
    });
    match bar_action {
        Some(BarAction::Format(f)) => app.apply_format(f),
        Some(BarAction::Align(a)) => {
            app.label.align = a;
            if let Some(e) = app.editing.as_mut() {
                // applied to the object live (see `live_fit_edited`) and recorded with the text when editing ends
                e.align = Some(a);
            }
        }
        None => {}
    }

    let Some(ed) = app.editing.as_mut() else { return };
    if let Some((a, b)) = ed.restore_sel.take() {
        let mut st = egui::text_edit::TextEditState::load(&ctx, field_id).unwrap_or_default();
        st.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(a), CCursor::new(b))));
        st.store(&ctx, field_id);
        ctx.memory_mut(|m| m.request_focus(field_id));
    }
    let mut done = false;
    let mut probe_rows: Vec<(f32, f32)> = Vec::new();
    let mut probe_caret = None;
    let width = area.width().max(if no_wrap { 6.0 } else { 60.0 });
    // vertical centering of the text block in its area, like the final rendering
    if ed.synced != ed.buf {
        ed.rich.apply_edit(&ed.synced, &ed.buf);
        ed.synced = ed.buf.clone();
    }
    let lay = inkboard_core::text::layout_rich(&ed.rich, lay_wrap, &measurer, family, model_size);
    let y_off = ((area.height() - lay.height as f32 * zoom) / 2.0).max(0.0);
    // same line height as the final rendering (egui's own row height is a little larger)
    let line_px = lay.line_h as f32 * zoom;
    let job_align = halign;
    // egui counts the space that follows a word when deciding whether it still fits on the row, the final layout does not: allow for it so
    // that both break the lines at the same words
    let space_px = measurer.width(" ", family, model_size, false) as f32 * zoom;
    let wrap_px = if no_wrap { f32::INFINITY } else { width + space_px };
    // egui's `TextEdit` places a galley by its left edge, but a galley laid out with a center / right alignment is anchored at the
    // middle / right of its text. So for those alignments the field is made twice as wide and shifted so that the anchor lands where
    // the final rendering puts it (using the text width computed with the same measurer).
    // (egui's own width of the widest row: it differs from the measurer's by a pixel or so, which would shift the text)
    let text_w = ctx.fonts_mut(|f| f.layout_job(rich_job(&ed.rich, family, size, line_px, color, dark, wrap_px, job_align))).rect.width();
    let (field_x, field_w, field_align) = match halign {
        egui::Align::LEFT => (area.min.x, width, egui::Align::LEFT),
        egui::Align::Center => (area.min.x + width / 2.0 + text_w / 2.0 - width, 2.0 * width, egui::Align::Center),
        egui::Align::RIGHT => (area.min.x + text_w - width, 2.0 * width, egui::Align::RIGHT),
    };
    egui::Area::new(egui::Id::new("wb-text-edit"))
        .constrain(false)
        .order(egui::Order::Foreground)
        .fade_in(false)
        .fixed_pos(egui::pos2(field_x, area.min.y + y_off))
        .show(&ctx, |ui| {
            ui.set_width(field_w);
            let Editing { buf, rich, synced, .. } = &mut *ed;
            let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, _wrap: f32| -> Arc<egui::Galley> {
                let t = text.as_str();
                if t != synced.as_str() {
                    rich.apply_edit(synced, t);
                    *synced = t.to_owned();
                }
                let job = rich_job(rich, family, size, line_px, color, dark, wrap_px, job_align);
                ui.ctx().fonts_mut(|f| f.layout_job(job))
            };
            let caret_stroke = ui.visuals().text_cursor.stroke;
            ui.visuals_mut().text_cursor.stroke = Stroke::NONE;
            let out = egui::TextEdit::multiline(buf)
                .id(field_id)
                .frame(false)
                // egui keeps a 4 × 2 margin even without a frame: it would shift the text off the box it is typed in
                .margin(egui::Margin::ZERO)
                .desired_width(field_w)
                .desired_rows(1)
                .horizontal_align(field_align)
                .lock_focus(true)
                .layouter(&mut layouter)
                .show(ui);
            probe_rows = out
                .galley
                .rows
                .iter()
                .filter_map(|r| r.row.glyphs.first().map(|g| (out.galley_pos.x + r.pos.x + g.pos.x, out.galley_pos.y + r.pos.y)))
                .collect();
            // strikethrough drawn by hand (see `strike_segments`)
            let strike: Vec<bool> = rich.0.iter().flat_map(|sp| sp.text.chars().filter(|c| *c != '\n').map(|_| sp.style.strike)).collect();
            if strike.iter().any(|s| *s) {
                let c = rich.0.iter().find(|sp| sp.style.strike).and_then(|sp| sp.style.color.map(|c| col(c, dark, 1.0))).unwrap_or(color);
                for (x0, x1, y) in crate::render::strike_segments(&out.galley, &strike, size) {
                    let (p0, p1) = (out.galley_pos + vec2(x0, y), out.galley_pos + vec2(x1, y));
                    ui.painter().line_segment([p0, p1], Stroke::new((size * 0.07).max(1.0), c));
                }
            }
            if let Some(r) = out.cursor_range {
                // egui's own caret ignores the shift of the rows of a centered / right-aligned galley, so it is drawn here
                let rc = out.galley.pos_from_cursor(r.primary);
                let row_x = out.galley.rows.iter().find(|row| (row.min_y() - rc.min.y).abs() < 0.5).map_or(0.0, |row| row.pos.x);
                let c = out.galley_pos + vec2(rc.min.x + row_x, rc.min.y);
                probe_caret = Some((c.x, c.y));
                if r.primary == r.secondary && out.response.has_focus() && ((ui.input(|i| i.time) * 2.0) as u64 & 1) == 0 {
                    ui.painter().line_segment([c, c + vec2(0.0, rc.height())], caret_stroke);
                }
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
    app.editor_probe = Some(EditorProbe { rows: probe_rows, caret: probe_caret });
    if done {
        app.commit_editing();
    }
}
