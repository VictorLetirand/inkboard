//! Floating style panel (on the left), like `#panel` of the web prototype: color swatches, segmented buttons, opacity slider,
//! actions on the selection. It only appears when there is something to set (a drawing tool or selected objects).

use crate::app::App;
use crate::fonts;
use crate::i18n::{t, tr};
use crate::icons;
use crate::theme::{self, Palette};
use crate::tools::Tool;
use eframe::egui::{self, Align2, Color32, Painter, Rect as ERect, Response, Stroke, Ui, vec2};
use inkboard_core::editor::{AlignMode, Axis, Order};
use inkboard_core::highlight::Lang;
use inkboard_core::layout::{Direction, auto_layout};
use inkboard_core::num;
use inkboard_core::style::{FILLS, PALETTE};
use inkboard_core::text::Format;
use inkboard_core::{Align, Arrowhead, BoxGeom, Color, Dash, FontFamily, HeadExt, Id, Kind, Label, Layer, Object, Pt, Route, Style, head_glyph};

/// Panel width (the web prototype used 226 px; a little wider here for comfort).
const WIDTH: f32 = 264.0;

/// Draws a line of thickness `w` (preview for the thickness and style buttons).
fn line_preview(painter: &Painter, rect: ERect, w: f32, dash: Dash, color: Color32) {
    let (a, b) = (rect.center() - vec2(8.0, 0.0), rect.center() + vec2(8.0, 0.0));
    let stroke = Stroke::new(w.clamp(0.8, 9.0), color);
    match dash {
        Dash::Solid => {
            painter.line_segment([a, b], stroke);
        }
        Dash::Dashed => {
            painter.extend(egui::Shape::dashed_line(&[a, b], stroke, 4.0, 3.0));
        }
        Dash::Dotted => {
            for i in 0..5 {
                painter.circle_filled(a.lerp(b, num::f32(f64::from(i)) / 4.0), (w * 0.45).clamp(1.0, 3.0), color);
            }
        }
    }
}

fn icon_in(p: &Painter, name: &str, r: ERect, c: Color32) {
    icons::paint(p, name, ERect::from_center_size(r.center(), vec2(17.0, 17.0)), c);
}

fn text_in(p: &Painter, text: &str, r: ERect, c: Color32, font: egui::FontId) {
    p.text(r.center(), Align2::CENTER_CENTER, text, font, c);
}

/// The prototype's color swatch: 6 px corners, border, special cases "none" (red bar) and "ink" (dark / light diagonal).
fn swatch(ui: &mut Ui, c: Option<Color>, selected: bool, dark: bool) -> Response {
    let palette = theme::current(ui.ctx());
    let (resp, r) = theme::swatch_frame(ui, selected);
    let pa = ui.painter();
    match c {
        Some(Color::Ink) => {
            pa.rect_filled(r, 6.0, Color32::from_rgb(0x1e, 0x1e, 0x1e));
            pa.add(egui::Shape::convex_polygon(vec![r.right_top(), r.right_bottom(), r.left_bottom()], Color32::from_rgb(0xf1, 0xf3, 0xf5), Stroke::NONE));
        }
        Some(c) => {
            let (rr, g, b) = c.resolve(dark);
            pa.rect_filled(r, 6.0, Color32::from_rgb(rr, g, b));
        }
        None => {
            pa.rect_filled(r, 6.0, palette.ui);
            pa.line_segment([r.left_bottom() - vec2(0.0, 3.0), r.right_top() + vec2(0.0, 3.0)], Stroke::new(1.6_f32, Color32::from_rgb(0xe0, 0x31, 0x31)));
        }
    }
    pa.rect_stroke(r, 6.0, Stroke::new(1.0_f32, palette.bd), egui::StrokeKind::Inside);
    resp
}

/// "Custom color" swatch (rainbow) opening a color picker; returns the chosen color on every change.
fn custom_swatch(ui: &mut Ui, current: [u8; 3]) -> Option<[u8; 3]> {
    let p = theme::current(ui.ctx());
    let (resp, r) = theme::swatch_frame(ui, false);
    // rainbow disc: 6 sectors
    let c = r.center();
    let radius = 12.0;
    let cols: [(u8, u8, u8); 6] = [(255, 0, 0), (255, 255, 0), (0, 255, 0), (0, 255, 255), (0, 0, 255), (255, 0, 255)];
    for i in 0..6 {
        let (a0, a1) = (num::f32_of(i) / 6.0 * std::f32::consts::TAU, num::f32_of(i + 1) / 6.0 * std::f32::consts::TAU);
        let (c0, c1) = (cols[i], cols[(i + 1) % 6]);
        let mid = Color32::from_rgb(u8::midpoint(c0.0, c1.0), u8::midpoint(c0.1, c1.1), u8::midpoint(c0.2, c1.2));
        let pts = vec![
            c,
            c + vec2(a0.cos(), a0.sin()) * radius,
            c + vec2(f32::midpoint(a0, a1).cos(), f32::midpoint(a0, a1).sin()) * radius,
            c + vec2(a1.cos(), a1.sin()) * radius,
        ];
        ui.painter().add(egui::Shape::convex_polygon(pts, mid, Stroke::NONE));
    }
    ui.painter().circle_stroke(c, radius, Stroke::new(1.0_f32, p.bd));
    let mut out = None;
    egui::Popup::from_toggle_button_response(&resp.clone().on_hover_text(t("Custom color")))
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            let mut rgb = current;
            if egui::color_picker::color_edit_button_srgb(ui, &mut rgb).changed() {
                out = Some(rgb);
            }
            let mut col = Color32::from_rgb(current[0], current[1], current[2]);
            if egui::color_picker::color_picker_color32(ui, &mut col, egui::color_picker::Alpha::Opaque) {
                out = Some([col.r(), col.g(), col.b()]);
            }
        });
    out
}

/// Setting row: dimmed label above the content.
fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    theme::field_label(ui, label);
    ui.add_space(-2.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
        add(ui);
    });
    ui.add_space(6.0);
}

/// Settings of a row of color swatches.
#[derive(Clone, Copy)]
struct Swatches<'a> {
    label: &'a str,
    none: bool,
    list: &'a [Color],
    current: Option<Color>,
    dark: bool,
}

fn swatches(ui: &mut Ui, set: Swatches, mut pick: impl FnMut(Option<Color>), mut live: impl FnMut([u8; 3])) {
    let Swatches { label, none, list, current, dark } = set;
    theme::field_label(ui, label);
    ui.add_space(-2.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(5.0, 5.0);
        if none && swatch(ui, None, current.is_none(), dark).on_hover_text(t("None")).clicked() {
            pick(None);
        }
        for c in list {
            if swatch(ui, Some(*c), current == Some(*c), dark)
                .on_hover_text(if *c == Color::Ink { t("Ink (follows the theme)") } else { String::new() })
                .clicked()
            {
                pick(Some(*c));
            }
        }
        let rgb = current.map_or([0x4f, 0x6d, 0xf5], |c| {
            let (r, g, b) = c.resolve(dark);
            [r, g, b]
        });
        if let Some(v) = custom_swatch(ui, rgb) {
            live(v);
        }
    });
    ui.add_space(6.0);
}

/// Does the panel have anything to show?
pub fn visible(app: &App) -> bool {
    app.layers_open || !app.sel.is_empty() || drawing_tool(app.tool) || app.editing.is_some()
}

fn drawing_tool(t: Tool) -> bool {
    matches!(t, Tool::Pen | Tool::Highlighter | Tool::Shape(_) | Tool::Line | Tool::Arrow | Tool::Text | Tool::Sticky | Tool::Table)
}

/// Shows the floating panel when there is something to set.
pub fn show(app: &mut App, ctx: &egui::Context, narrow: bool) {
    if !visible(app) {
        return;
    }
    let top = if narrow { 112.0 } else { 64.0 };
    let max_h = (ctx.content_rect().height() - top - 78.0).max(160.0);
    theme::floating(ctx, "props", Align2::LEFT_TOP, vec2(12.0, top), 10, |ui| {
        ui.set_width(WIDTH - 24.0);
        egui::ScrollArea::vertical().max_width(WIDTH - 24.0).max_height(max_h - 20.0).min_scrolled_height(max_h - 20.0).auto_shrink([false, true]).show(
            ui,
            |ui| {
                ui.set_width(WIDTH - 24.0);
                props(app, ui);
            },
        );
    });
    if app.prop_edit.is_some() && !ctx.input(|i| i.pointer.any_down()) {
        app.live_end();
    }
}

/// What the style panel edits: the editable selection, or the defaults of the current tool when nothing is selected.
struct Target {
    objs: Vec<Object>,
    ids: Vec<Id>,
    style: Style,
    label: Label,
}

impl Target {
    fn of(app: &App) -> Self {
        let objs = app.editable_selection();
        let (style, label) = match objs.first() {
            Some(o) => {
                let l = match &o.kind {
                    Kind::Box(b) => b.label.clone(),
                    _ => app.label.clone(),
                };
                (o.style.clone(), l)
            }
            None => (app.style.clone(), app.label.clone()),
        };
        let ids = objs.iter().map(|o| o.id.clone()).collect();
        Target { objs, ids, style, label }
    }

    fn has_sel(&self) -> bool {
        !self.objs.is_empty()
    }

    fn shapes(&self) -> bool {
        self.objs.iter().any(|o| matches!(o.kind, Kind::Box(_)))
    }

    fn connectors(&self) -> bool {
        self.objs.iter().any(|o| matches!(o.kind, Kind::Connector(_)))
    }

    fn only_text(&self) -> bool {
        self.has_sel() && self.objs.iter().all(|o| matches!(&o.kind, Kind::Box(b) if b.geom == BoxGeom::Text))
    }

    fn only_pen(&self) -> bool {
        self.has_sel() && self.objs.iter().all(|o| matches!(o.kind, Kind::Pen(_)))
    }

    /// Highlighter strokes selected (or the highlighter tool active, without selection).
    fn highlighter(&self, tool: Tool) -> bool {
        if self.has_sel() { self.objs.iter().all(|o| matches!(&o.kind, Kind::Pen(p) if p.highlighter)) } else { tool == Tool::Highlighter }
    }
}

/// Language of the selected code blocks.
fn code_lang_section(app: &mut App, ui: &mut Ui, target: &Target) {
    let code_ids: Vec<Id> = target.objs.iter().filter(|o| matches!(&o.kind, Kind::Box(b) if b.lang.is_some())).map(|o| o.id.clone()).collect();
    if !code_ids.is_empty() {
        let cur = target.objs.iter().find_map(|o| if let Kind::Box(b) = &o.kind { b.lang } else { None }).unwrap_or_default();
        let name = |l: Lang| if l == Lang::Auto { t("Auto") } else { l.label().to_owned() };
        theme::field_label(ui, "Code language");
        let mut chosen = None;
        egui::ComboBox::from_id_salt("code-lang").selected_text(name(cur)).width(WIDTH - 24.0 - 8.0).show_ui(ui, |ui| {
            for l in Lang::ALL {
                if ui.selectable_label(l == cur, name(l)).clicked() {
                    chosen = Some(l);
                }
            }
        });
        if let Some(l) = chosen {
            app.set_code_lang(&code_ids, l);
        }
        ui.add_space(8.0);
    }
}

/// Outline (or text) color and fill.
fn color_section(app: &mut App, ui: &mut Ui, target: &Target, dark: bool) {
    let highlighter = target.highlighter(app.tool);
    let stroke_label = if target.only_text() || (!target.has_sel() && app.tool == Tool::Text) {
        "Text color"
    } else if target.only_pen() || matches!(app.tool, Tool::Pen | Tool::Highlighter) {
        "Color"
    } else {
        "Outline"
    };
    let mut set_stroke: Option<Color> = None;
    swatches(
        ui,
        Swatches { label: stroke_label, none: false, list: &PALETTE, current: Some(target.style.stroke), dark },
        |c| set_stroke = c,
        |v| {
            let c = Color::Rgb(v[0], v[1], v[2]);
            app.style.stroke = c;
            app.live_apply(&move |o| o.style.stroke = c);
        },
    );
    if let Some(c) = set_stroke {
        app.style.stroke = c;
        app.ed.update_many(&target.ids, |o| o.style.stroke = c);
    }
    let fill_label =
        if matches!(app.tool, Tool::Sticky) || (target.has_sel() && target.objs.iter().all(|o| matches!(&o.kind, Kind::Box(b) if b.geom == BoxGeom::Sticky))) {
            "Note color"
        } else {
            "Fill"
        };
    if !target.only_pen() && !highlighter && !matches!(app.tool, Tool::Pen | Tool::Highlighter | Tool::Line | Tool::Arrow) && !connectors_only(&target.objs) {
        let mut set_fill: Option<Option<Color>> = None;
        swatches(
            ui,
            Swatches { label: fill_label, none: true, list: &FILLS, current: target.style.fill, dark },
            |c| set_fill = Some(c),
            |v| {
                let c = Color::Rgb(v[0], v[1], v[2]);
                app.style.fill = Some(c);
                app.live_apply(&move |o| o.style.fill = Some(c));
            },
        );
        if let Some(c) = set_fill {
            app.style.fill = c;
            app.ed.update_many(&target.ids, |o| o.style.fill = c);
        }
    }
}

/// Color swatches shown at most for the colors of an imported SVG (very colorful ones are cut).
const RECOLOR_MAX: usize = 24;

/// Popup of a recolor swatch: palette and picker. Returns the chosen color and whether it is a live change (picker drag).
fn recolor_popup(resp: &Response, current: [u8; 3], dark: bool) -> Option<(Color, bool)> {
    let mut out = None;
    egui::Popup::from_toggle_button_response(resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        ui.set_max_width(WIDTH - 40.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(5.0, 5.0);
            for c in PALETTE {
                if swatch(ui, Some(c), false, dark).clicked() {
                    out = Some((c, false));
                }
            }
        });
        let mut col = Color32::from_rgb(current[0], current[1], current[2]);
        if egui::color_picker::color_picker_color32(ui, &mut col, egui::color_picker::Alpha::Opaque) {
            out = Some((Color::Rgb(col.r(), col.g(), col.b()), true));
        }
    });
    out
}

/// New `recolor` list once the original color `from` becomes `to` (the pair is dropped when `to` is the original again).
fn recolor_with(list: &[(Color, Color)], from: Color, to: Color) -> Vec<(Color, Color)> {
    let mut out: Vec<(Color, Color)> = list.iter().copied().filter(|(f, _)| *f != from).collect();
    if to != from {
        out.push((from, to));
    }
    out
}

/// Colors of the selected SVG image: one swatch per color of the file, to replace each one.
fn recolor_section(app: &mut App, ui: &mut Ui, target: &Target, dark: bool) {
    let [obj] = target.objs.as_slice() else { return };
    let Kind::Image(img) = &obj.kind else { return };
    if img.is_tinted() {
        return;
    }
    let Some(asset) = app.ed.doc.assets.get(&img.asset).filter(|a| a.mime == "image/svg+xml") else { return };
    // the panel is drawn every frame: scan the file only when the selected asset changes
    thread_local!(static FOUND: std::cell::RefCell<Option<(inkboard_core::AssetId, Vec<Color>)>> = const { std::cell::RefCell::new(None) });
    let found = FOUND.with_borrow_mut(|memo| match memo {
        Some((a, found)) if *a == img.asset => found.clone(),
        _ => {
            let found: Vec<Color> = inkboard_core::svgcolor::svg_colors(&asset.data).into_iter().map(|(r, g, b)| Color::Rgb(r, g, b)).collect();
            *memo = Some((img.asset.clone(), found.clone()));
            found
        }
    });
    if found.is_empty() {
        return;
    }
    let (id, list) = (obj.id.clone(), img.recolor.clone());
    let current = |c: Color| list.iter().find(|(f, _)| *f == c).map_or(c, |(_, t)| *t);
    let set = |app: &mut App, new: Vec<(Color, Color)>, live: bool| {
        if live {
            app.live_apply(&move |o| {
                if let Kind::Image(i) = &mut o.kind {
                    i.recolor.clone_from(&new);
                }
            });
        } else {
            app.ed.update_many(std::slice::from_ref(&id), |o| {
                if let Kind::Image(i) = &mut o.kind {
                    i.recolor.clone_from(&new);
                }
            });
        }
    };
    let mut change: Option<(Vec<(Color, Color)>, bool)> = None;
    theme::field_label(ui, "Image colors");
    ui.add_space(-2.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(5.0, 5.0);
        for &from in found.iter().take(RECOLOR_MAX) {
            let now = current(from);
            let resp = swatch(ui, Some(now), false, dark).on_hover_text(tr!("Original color: {}", from.to_hex()));
            let (r, g, b) = now.resolve(dark);
            if let Some((to, live)) = recolor_popup(&resp, [r, g, b], dark) {
                change = Some((recolor_with(&list, from, to), live));
            }
        }
    });
    if found.len() > RECOLOR_MAX {
        ui.weak(tr!("+ {} more colors", found.len() - RECOLOR_MAX));
    }
    ui.horizontal_wrapped(|ui| {
        let all = ui.button(t("All one color"));
        if let Some((to, live)) = recolor_popup(&all, [0x4f, 0x6d, 0xf5], dark) {
            change = Some((found.iter().map(|&f| (f, to)).collect(), live));
        }
        if !list.is_empty() && ui.button(t("Original colors")).clicked() {
            change = Some((Vec::new(), false));
        }
    });
    if let Some((new, live)) = change {
        set(app, new, live);
    }
    ui.add_space(6.0);
}

/// Thickness and stroke style.
fn stroke_section(app: &mut App, ui: &mut Ui, target: &Target) {
    let highlighter = target.highlighter(app.tool);
    let widths: &[f32] = if highlighter {
        &[10.0, 18.0, 28.0, 40.0]
    } else if target.only_pen() || matches!(app.tool, Tool::Pen) {
        &[1.5, 3.0, 6.0, 10.0]
    } else {
        &[0.0, 1.5, 3.0, 6.0, 10.0]
    };
    theme::field_label(ui, "Thickness");
    ui.add_space(-2.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
        for &w in widths {
            let on = (num::f32(target.style.width) - w).abs() < 0.26;
            let tip = if w == 0.0 { t("No outline") } else { tr!("Thickness {}", w) };
            let resp = theme::seg_button_raw(ui, on, &tip, |p, r, c| {
                if w == 0.0 {
                    p.circle_stroke(r.center(), 6.0, Stroke::new(1.3_f32, c));
                    p.line_segment([r.center() + vec2(-5.0, 5.0), r.center() + vec2(5.0, -5.0)], Stroke::new(1.3_f32, c));
                } else {
                    line_preview(p, r, if highlighter { w / 3.0 } else { w * 0.9 }, Dash::Solid, c);
                }
            });
            if resp.clicked() {
                app.style.width = f64::from(w);
                app.ed.update_many(&target.ids, |o| o.style.width = f64::from(w));
            }
        }
    });
    ui.add_space(6.0);
    if !highlighter && !matches!(app.tool, Tool::Pen) && !target.only_pen() {
        row(ui, "Stroke style", |ui| {
            for (d, tip) in [(Dash::Solid, "Solid"), (Dash::Dashed, "Dashed"), (Dash::Dotted, "Dotted")] {
                if theme::seg_button(ui, target.style.dash == d, tip, |p, r, c| line_preview(p, r, 2.2, d, c)).clicked() {
                    app.style.dash = d;
                    app.ed.update_many(&target.ids, |o| o.style.dash = d);
                }
            }
        });
    }
}

/// Sharp or rounded corners.
fn corners_section(app: &mut App, ui: &mut Ui, target: &Target) {
    row(ui, "Corners", |ui| {
        for (rd, tip, rx) in [(0.0f32, "Sharp corners", 0.0f32), (0.25, "Rounded corners", 4.0)] {
            let on = if rd == 0.0 { target.style.roundness < 0.05 } else { target.style.roundness >= 0.05 };
            if theme::seg_button(ui, on, tip, |p, r, c| {
                p.rect_stroke(ERect::from_center_size(r.center(), vec2(16.0, 12.0)), rx, Stroke::new(1.8_f32, c), egui::StrokeKind::Middle);
            })
            .clicked()
            {
                app.style.roundness = rd;
                app.ed.update_many(&target.ids, |o| o.style.roundness = rd);
            }
        }
    });
}

/// Drop shadow on or off.
fn shadow_section(app: &mut App, ui: &mut Ui, target: &Target) {
    row(ui, "Shadow", |ui| {
        let on = target.style.shadow;
        if theme::seg_button(ui, on, "Drop shadow", |p, r, c| {
            let body = ERect::from_center_size(r.center() + vec2(-2.0, -2.0), vec2(14.0, 11.0));
            p.rect_filled(body.translate(vec2(3.0, 3.0)), 2.0, c.gamma_multiply(0.35));
            p.rect_stroke(body, 2.0, Stroke::new(1.8_f32, c), egui::StrokeKind::Middle);
        })
        .clicked()
        {
            set_shadow(app, &target.ids, !on);
        }
    });
}

/// Turns the drop shadow of the objects among `ids` on or off (and of the next ones drawn), as one undo step.
pub(crate) fn set_shadow(app: &mut App, ids: &[Id], on: bool) {
    app.style.shadow = on;
    app.ed.update_many(ids, |o| o.style.shadow = on);
}

/// Path and arrowheads of the connectors.
fn connector_section(app: &mut App, ui: &mut Ui, target: &Target) {
    let cur = target.objs.iter().find_map(|o| if let Kind::Connector(c) = &o.kind { Some(c.clone()) } else { None });
    row(ui, "Path", |ui| {
        for (r, name) in [(Route::Straight, "Straight"), (Route::Elbow, "Elbow"), (Route::Curve, "Curve")] {
            let on = cur.as_ref().map_or(app.route == r, |c| c.route == r);
            if ui.selectable_label(on, name).clicked() {
                app.route = r;
                app.ed.update_many(&target.ids, |o| {
                    if let Kind::Connector(c) = &mut o.kind {
                        c.route = r;
                    }
                });
            }
        }
    });
    if let Some(c) = cur {
        for (title, is_start) in [("Arrowhead at the start", true), ("Arrowhead at the end", false)] {
            let (now, now_ext) = if is_start { (c.head_start, c.head_start_ext) } else { (c.head_end, c.head_end_ext) };
            row(ui, title, |ui| {
                for (h, name) in [(Arrowhead::None, "—"), (Arrowhead::Arrow, "→"), (Arrowhead::Triangle, "▶"), (Arrowhead::Dot, "●"), (Arrowhead::Bar, "|")]
                {
                    if theme::seg_button(ui, now_ext.is_none() && now == h, "", |p, r, col| text_in(p, name, r, col, egui::FontId::proportional(14.0)))
                        .clicked()
                    {
                        app.ed.update_many(&target.ids, |o| {
                            if let Kind::Connector(c) = &mut o.kind {
                                if is_start { c.set_head_start(h) } else { c.set_head_end(h) }
                            }
                        });
                    }
                }
                for e in HeadExt::ALL {
                    if theme::seg_button(ui, now_ext == Some(e), head_ext_tip(e), |p, r, col| head_preview(p, r, col, e)).clicked() {
                        app.ed.update_many(&target.ids, |o| {
                            if let Kind::Connector(c) = &mut o.kind {
                                if is_start { c.set_ext_start(e) } else { c.set_ext_end(e) }
                            }
                        });
                    }
                }
            });
        }
        row(ui, "Arrowhead size", |ui| {
            for (scale, name, tip) in HEAD_SIZES {
                if theme::seg_button(ui, (c.head_scale - scale).abs() < 0.05, tip, |p, r, col| text_in(p, name, r, col, egui::FontId::proportional(13.0)))
                    .clicked()
                {
                    set_head_scale(app, &target.ids, scale);
                }
            }
        });
    }
}

/// Sets the arrowhead size of the connectors among `ids`, as one undo step.
pub(crate) fn set_head_scale(app: &mut App, ids: &[Id], scale: f32) {
    app.ed.update_many(ids, |o| {
        if let Kind::Connector(c) = &mut o.kind {
            c.set_head_scale(scale);
        }
    });
}

/// Steps of the arrowhead size: size factor, label and tooltip.
pub(crate) const HEAD_SIZES: [(f32, &str, &str); 4] =
    [(0.6, "S", "Small arrowheads"), (1.0, "M", "Medium arrowheads"), (1.6, "L", "Large arrowheads"), (2.4, "XL", "Extra large arrowheads")];

/// Tooltip of the button of an extended arrowhead (translated by the button).
fn head_ext_tip(e: HeadExt) -> &'static str {
    match e {
        HeadExt::Diamond => "Diamond (composition)",
        HeadExt::DiamondOpen => "Hollow diamond (aggregation)",
        HeadExt::TriangleOpen => "Hollow triangle (inheritance)",
        HeadExt::CrowFoot => "Crow's foot (many)",
        HeadExt::CrowFootOne => "One or many",
        HeadExt::CrowFootZero => "Zero or many",
        HeadExt::OneOnly => "Exactly one",
        HeadExt::ZeroOrOne => "Zero or one",
    }
}

/// Small drawing of an extended arrowhead at the end of a line, inside a button.
fn head_preview(p: &egui::Painter, r: egui::Rect, col: egui::Color32, e: HeadExt) {
    let tip = Pt::new(f64::from(r.right() - 6.0), f64::from(r.center().y));
    let from = Pt::new(f64::from(r.left() + 5.0), f64::from(r.center().y));
    let line = [egui::pos2(r.left() + 5.0, r.center().y), egui::pos2(r.right() - 6.0, r.center().y)];
    p.line_segment(line, egui::Stroke::new(1.5_f32, col));
    let hollow = theme::current(p.ctx()).ui;
    crate::render::paint_head(p, &head_glyph(tip, from, 9.0, Arrowhead::None, Some(e)), &|q| egui::pos2(num::f32(q.x), num::f32(q.y)), 1.0, 1.5, col, hollow);
}

/// Font, size and alignment of the text.
fn text_section(app: &mut App, ui: &mut Ui, target: &Target) {
    row(ui, "Font", |ui| {
        for (f, tip) in [(FontFamily::Sans, "Sans serif"), (FontFamily::Hand, "Handwriting"), (FontFamily::Serif, "Serif"), (FontFamily::Mono, "Monospace")] {
            if theme::seg_button(ui, target.label.family == f, tip, |p, r, c| text_in(p, "Aa", r, c, fonts::font_id(f, 15.0, false))).clicked() {
                app.label.family = f;
                let m = app.measurer.clone();
                app.ed.update_many(&target.ids, |o| {
                    if let Kind::Box(b) = &mut o.kind {
                        b.label.family = f;
                        b.fit(&m);
                    }
                });
            }
        }
    });
    row(ui, "Text size", |ui| {
        for (s, name) in [(16.0, "S"), (22.0, "M"), (32.0, "L"), (48.0, "XL")] {
            if theme::seg_button(ui, (target.label.size - s).abs() < 0.5, "", |p, r, c| text_in(p, name, r, c, egui::FontId::proportional(13.0))).clicked() {
                app.label.size = s;
                let m = app.measurer.clone();
                app.ed.update_many(&target.ids, |o| {
                    if let Kind::Box(b) = &mut o.kind {
                        b.label.size = s;
                        b.fit(&m);
                    }
                });
            }
        }
    });
    row(ui, "Alignment", |ui| {
        for (a, icon) in [(Align::Left, "alignL"), (Align::Center, "alignC"), (Align::Right, "alignR")] {
            if theme::seg_button(ui, target.label.align == a, "", |p, r, c| icon_in(p, icon, r, c)).clicked() {
                app.label.align = a;
                app.ed.update_many(&target.ids, |o| {
                    if let Kind::Box(b) = &mut o.kind {
                        b.label.align = a;
                    }
                });
            }
        }
    });
}

fn props(app: &mut App, ui: &mut Ui) {
    let pal = theme::current(ui.ctx());
    let dark = app.ed.doc.background.preset.is_dark();
    let target = Target::of(app);
    let has_sel = target.has_sel();
    let drawing = drawing_tool(app.tool);
    if has_sel || drawing {
        code_lang_section(app, ui, &target);
        color_section(app, ui, &target, dark);
        recolor_section(app, ui, &target, dark);
        if !target.only_text() {
            stroke_section(app, ui, &target);
        }
        if target.shapes() || matches!(app.tool, Tool::Shape(BoxGeom::Rect) | Tool::Sticky) {
            corners_section(app, ui, &target);
        }
        if !target.only_pen() && !target.highlighter(app.tool) && !matches!(app.tool, Tool::Pen) {
            shadow_section(app, ui, &target);
        }
    }
    if target.connectors() || matches!(app.tool, Tool::Line | Tool::Arrow) {
        connector_section(app, ui, &target);
    }
    if target.shapes() || matches!(app.tool, Tool::Text | Tool::Sticky | Tool::Shape(_)) {
        text_section(app, ui, &target);
    }
    if has_sel || app.editing.is_some() {
        rich_section(app, ui, dark);
    }

    // ---- opacity
    if has_sel || drawing {
        let mut op = target.style.opacity;
        theme::field_label_raw(ui, &tr!("Opacity {} %", (op * 100.0).round()));
        if ui.add(egui::Slider::new(&mut op, 0.05..=1.0).show_value(false)).changed() {
            app.style.opacity = op;
            app.live_apply(&move |o| o.style.opacity = op);
        }
        ui.add_space(6.0);
    }

    if has_sel {
        organisation(app, ui, &target.objs, &target.ids, &pal);
    }
    let locked: Vec<Id> = app.selected_locked();
    if !locked.is_empty() && ui.button(t("Unlock the selection")).clicked() {
        app.ed.set_locked(&locked, false);
    }
    if app.layers_open {
        layers(app, ui, has_sel, &target.ids);
    }
}

fn connectors_only(objs: &[Object]) -> bool {
    !objs.is_empty() && objs.iter().all(|o| matches!(o.kind, Kind::Connector(_)))
}

/// B / I / U / S, word color and highlight (rich text).
fn rich_section(app: &mut App, ui: &mut Ui, dark: bool) {
    row(ui, if app.editing.is_some() { "Format of the selection" } else { "Format of all the text" }, |ui| {
        for (name, f) in [("B", Format::Bold), ("I", Format::Italic), ("U", Format::Underline), ("S", Format::Strike)] {
            let on = app.format_active(f);
            let clicked = theme::seg_button(ui, on, "", |p, r, c| {
                let font = egui::FontId::proportional(14.0);
                let g = p.layout_no_wrap(name.to_owned(), font, c);
                let pos = r.center() - g.size() / 2.0;
                p.galley(pos, g.clone(), c);
                match f {
                    Format::Bold => {
                        p.galley(pos + vec2(0.6, 0.0), g, c);
                    }
                    Format::Underline => {
                        p.line_segment([pos + vec2(0.0, g.size().y), pos + vec2(g.size().x, g.size().y)], Stroke::new(1.2_f32, c));
                    }
                    Format::Strike => {
                        p.line_segment([pos + vec2(-1.0, g.size().y / 2.0), pos + vec2(g.size().x + 1.0, g.size().y / 2.0)], Stroke::new(1.2_f32, c));
                    }
                    _ => {}
                }
            })
            .clicked();
            if clicked {
                app.apply_format(f);
            }
        }
        if ui.small_button(t("Clear")).clicked() {
            app.apply_format(Format::Clear);
        }
    });
    theme::field_label(ui, "Word color");
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(5.0, 5.0);
        for c in PALETTE.iter().skip(1) {
            if swatch(ui, Some(*c), app.format_active(Format::Color(Some(*c))), dark).clicked() {
                app.apply_format(Format::Color(Some(*c)));
            }
        }
    });
    ui.add_space(6.0);
    theme::field_label(ui, "Highlight");
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(5.0, 5.0);
        for c in FILLS.iter().take(8) {
            if swatch(ui, Some(*c), app.format_active(Format::Highlight(Some(*c))), dark).clicked() {
                app.apply_format(Format::Highlight(Some(*c)));
            }
        }
    });
    ui.add_space(6.0);
}

/// Order, alignment, actions, automatic layout and table operations.
fn organisation(app: &mut App, ui: &mut Ui, objs: &[Object], sel_ids: &[Id], pal: &Palette) {
    row(ui, "Order", |ui| {
        for (icon, tip, o) in [
            ("front", "Bring to front", Order::Front),
            ("forward", "Bring forward", Order::Forward),
            ("backward", "Send backward", Order::Backward),
            ("back", "Send to back", Order::Back),
        ] {
            if theme::seg_button(ui, false, tip, |p, r, c| icon_in(p, icon, r, c)).clicked() {
                app.ed.reorder(sel_ids, o);
            }
        }
    });
    if sel_ids.len() > 1 {
        row(ui, "Alignment", |ui| {
            for (icon, tip, m) in [
                ("al", "Align left", AlignMode::Left),
                ("ac", "Center horizontally", AlignMode::CenterX),
                ("ar", "Align right", AlignMode::Right),
                ("at", "Align top", AlignMode::Top),
                ("am", "Center vertically", AlignMode::CenterY),
                ("ab", "Align bottom", AlignMode::Bottom),
            ] {
                if theme::seg_button(ui, false, tip, |p, r, c| icon_in(p, icon, r, c)).clicked() {
                    app.ed.align(sel_ids, m);
                }
            }
            if sel_ids.len() > 2 {
                if theme::seg_button(ui, false, "Distribute horizontally", |p, r, c| icon_in(p, "dh", r, c)).clicked() {
                    app.ed.distribute(sel_ids, Axis::Horizontal);
                }
                if theme::seg_button(ui, false, "Distribute vertically", |p, r, c| icon_in(p, "dv", r, c)).clicked() {
                    app.ed.distribute(sel_ids, Axis::Vertical);
                }
            }
        });
        row(ui, "Automatic layout", |ui| {
            for (glyph, tip, d) in [
                ("↓", "Top to bottom", Direction::TopDown),
                ("→", "Left to right", Direction::LeftRight),
                ("↑", "Bottom to top", Direction::BottomUp),
                ("←", "Right to left", Direction::RightLeft),
            ] {
                if theme::seg_button(ui, false, tip, |p, r, c| text_in(p, glyph, r, c, egui::FontId::proportional(15.0))).clicked() {
                    auto_layout(&mut app.ed, sel_ids, d);
                }
            }
        });
    }
    let all_locked = sel_ids.iter().all(|i| app.ed.page().get(i).is_some_and(|o| o.locked));
    let grouped = objs.iter().any(|o| o.group.is_some());
    row(ui, "Actions", |ui| {
        if theme::seg_button(ui, false, "Duplicate", |p, r, c| icon_in(p, "copy", r, c)).clicked() {
            app.sel = app.ed.duplicate(sel_ids, Pt::new(24.0, 24.0));
        }
        if (sel_ids.len() > 1 || grouped)
            && theme::seg_button(ui, false, if grouped { "Ungroup" } else { "Group" }, |p, r, c| icon_in(p, if grouped { "ungroup" } else { "group" }, r, c))
                .clicked()
        {
            if grouped {
                app.ed.ungroup(sel_ids);
            } else {
                app.ed.group(sel_ids);
            }
        }
        if theme::seg_button(ui, false, if all_locked { "Unlock" } else { "Lock" }, |p, r, c| icon_in(p, if all_locked { "unlock" } else { "lock" }, r, c))
            .clicked()
        {
            app.ed.set_locked(sel_ids, !all_locked);
        }
        let danger = Color32::from_rgb(0xe0, 0x31, 0x31);
        if theme::seg_button(ui, false, "Delete", |p, r, _| icon_in(p, "trash", r, danger)).clicked() {
            app.delete_selection();
        }
    });
    if let [o] = objs
        && matches!(o.kind, Kind::Table(_))
    {
        row(ui, "Table", |ui| {
            for (name, f) in [("+ row", 0), ("+ column", 1), ("− row", 2), ("− column", 3)] {
                if ui.small_button(name).clicked() {
                    app.ed.update(&o.id, |o| {
                        if let Kind::Table(t) = &mut o.kind {
                            match f {
                                0 => t.insert_row(t.rows()),
                                1 => t.insert_col(t.cols()),
                                2 => t.remove_row(t.rows().saturating_sub(1)),
                                _ => t.remove_col(t.cols().saturating_sub(1)),
                            }
                        }
                    });
                }
            }
        });
    }
    let _ = pal;
}

/// The page's layer list (visibility, lock, name, moving the selection).
fn layers(app: &mut App, ui: &mut Ui, has_sel: bool, sel_ids: &[Id]) {
    theme::menu_sep(ui);
    theme::field_label(ui, "Layers");
    let mut layers = app.ed.page().layers.clone();
    let mut changed = false;
    for l in &mut layers {
        ui.horizontal(|ui| {
            changed |= ui.checkbox(&mut l.visible, "").on_hover_text(t("Visible")).changed();
            let mut locked = l.locked;
            if ui.toggle_value(&mut locked, t("Lock")).on_hover_text(t("Locked")).changed() {
                l.locked = locked;
                changed = true;
            }
            let r = ui.add(egui::TextEdit::singleline(&mut l.name).desired_width(80.0));
            changed |= r.lost_focus() && r.changed();
            if has_sel && ui.small_button("→").on_hover_text(t("Move the selection here")).clicked() {
                let id = l.id.clone();
                app.ed.update_many(sel_ids, |o| o.layer = id.clone());
            }
        });
    }
    if ui.small_button(t("+ layer")).clicked() {
        layers.push(Layer { id: Id::new(), name: tr!("Layer {}", layers.len() + 1), visible: true, locked: false });
        changed = true;
    }
    if changed {
        app.ed.set_layers(layers);
    }
}

impl App {
    pub fn selected_locked(&self) -> Vec<Id> {
        let p = self.ed.page();
        p.objects.iter().filter(|o| self.sel.contains(&o.id) && o.locked).map(|o| o.id.clone()).collect()
    }
}
