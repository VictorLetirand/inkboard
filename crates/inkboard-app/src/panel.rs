//! Panneau de style flottant (à gauche), comme `#panel` du prototype web : pastilles de couleur, boutons segmentés, curseur d'opacité,
//! actions sur la sélection. Il n'apparaît que lorsqu'il y a quelque chose à régler (outil de dessin ou objets sélectionnés).

use crate::app::App;
use crate::fonts;
use crate::icons;
use crate::theme::{self, Palette};
use crate::tools::Tool;
use eframe::egui::{self, Align2, Color32, Painter, Rect as ERect, Response, Stroke, Ui, vec2};
use inkboard_core::editor::{AlignMode, Axis, Order};
use inkboard_core::layout::{Direction, auto_layout};
use inkboard_core::style::{FILLS, PALETTE};
use inkboard_core::text::Format;
use inkboard_core::*;

/// Largeur du panneau (`#panel{width:226px}`).
const WIDTH: f32 = 226.0;

/// Tracé d'une ligne d'épaisseur `w` (aperçu des boutons d'épaisseur et de style).
fn line_preview(p: &Painter, r: ERect, w: f32, dash: Dash, color: Color32) {
    let (a, b) = (r.center() - vec2(8.0, 0.0), r.center() + vec2(8.0, 0.0));
    let stroke = Stroke::new(w.clamp(0.8, 9.0), color);
    match dash {
        Dash::Solid => {
            p.line_segment([a, b], stroke);
        }
        Dash::Dashed => {
            p.extend(egui::Shape::dashed_line(&[a, b], stroke, 4.0, 3.0));
        }
        Dash::Dotted => {
            for i in 0..5 {
                p.circle_filled(a.lerp(b, i as f32 / 4.0), (w * 0.45).clamp(1.0, 3.0), color);
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

/// Pastille de couleur du prototype : coins de 6 px, bordure, cas spéciaux « aucune » (barre rouge) et « encre » (diagonale sombre / claire).
fn swatch(ui: &mut Ui, c: Option<Color>, selected: bool, dark: bool) -> Response {
    let p = theme::current(ui.ctx());
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
            pa.rect_filled(r, 6.0, p.ui);
            pa.line_segment([r.left_bottom() - vec2(0.0, 3.0), r.right_top() + vec2(0.0, 3.0)], Stroke::new(1.6_f32, Color32::from_rgb(0xe0, 0x31, 0x31)));
        }
    }
    pa.rect_stroke(r, 6.0, Stroke::new(1.0_f32, p.bd), egui::StrokeKind::Inside);
    resp
}

/// Pastille « couleur personnalisée » (arc-en-ciel) ouvrant un sélecteur de couleur ; renvoie la couleur choisie à chaque modification.
fn custom_swatch(ui: &mut Ui, current: [u8; 3]) -> Option<[u8; 3]> {
    let p = theme::current(ui.ctx());
    let (resp, r) = theme::swatch_frame(ui, false);
    // disque arc-en-ciel : 6 secteurs
    let c = r.center();
    let radius = 10.0;
    let cols = [(255, 0, 0), (255, 255, 0), (0, 255, 0), (0, 255, 255), (0, 0, 255), (255, 0, 255)];
    for i in 0..6 {
        let (a0, a1) = (i as f32 / 6.0 * std::f32::consts::TAU, (i + 1) as f32 / 6.0 * std::f32::consts::TAU);
        let (c0, c1) = (cols[i], cols[(i + 1) % 6]);
        let mid = Color32::from_rgb(((c0.0 + c1.0) / 2) as u8, ((c0.1 + c1.1) / 2) as u8, ((c0.2 + c1.2) / 2) as u8);
        let pts = vec![
            c,
            c + vec2(a0.cos(), a0.sin()) * radius,
            c + vec2(((a0 + a1) / 2.0).cos(), ((a0 + a1) / 2.0).sin()) * radius,
            c + vec2(a1.cos(), a1.sin()) * radius,
        ];
        ui.painter().add(egui::Shape::convex_polygon(pts, mid, Stroke::NONE));
    }
    ui.painter().circle_stroke(c, radius, Stroke::new(1.0_f32, p.bd));
    let mut out = None;
    egui::Popup::from_toggle_button_response(&resp.clone().on_hover_text("Couleur personnalisée"))
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

/// Ligne de réglage : étiquette grisée au-dessus du contenu.
fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    theme::field_label(ui, label);
    ui.add_space(-2.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
        add(ui);
    });
    ui.add_space(6.0);
}

/// Réglages d'une rangée de pastilles de couleur.
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
        if none && swatch(ui, None, current.is_none(), dark).on_hover_text("Aucune").clicked() {
            pick(None);
        }
        for c in list {
            if swatch(ui, Some(*c), current == Some(*c), dark).on_hover_text(if *c == Color::Ink { "Encre (suit le thème)" } else { "" }).clicked() {
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

/// Le panneau a-t-il quelque chose à montrer ?
pub fn visible(app: &App) -> bool {
    app.layers_open || !app.sel.is_empty() || drawing_tool(app.tool) || app.editing.is_some()
}

fn drawing_tool(t: Tool) -> bool {
    matches!(t, Tool::Pen | Tool::Highlighter | Tool::Shape(_) | Tool::Line | Tool::Arrow | Tool::Text | Tool::Sticky | Tool::Table)
}

/// Affiche le panneau flottant s'il y a quelque chose à régler.
pub fn show(app: &mut App, ctx: &egui::Context, narrow: bool) {
    if !visible(app) {
        return;
    }
    let top = if narrow { 112.0 } else { 64.0 };
    let max_h = (ctx.content_rect().height() - top - 78.0).max(160.0);
    theme::floating(ctx, "props", Align2::LEFT_TOP, vec2(12.0, top), 10, |ui| {
        ui.set_width(WIDTH - 24.0);
        egui::ScrollArea::vertical().max_width(WIDTH - 24.0).max_height(max_h - 20.0).auto_shrink([false, true]).show(ui, |ui| {
            ui.set_width(WIDTH - 24.0);
            props(app, ui);
        });
    });
    if app.prop_edit.is_some() && !ctx.input(|i| i.pointer.any_down()) {
        app.live_end();
    }
}

fn props(app: &mut App, ui: &mut Ui) {
    let pal = theme::current(ui.ctx());
    let dark = app.ed.doc.background.preset.is_dark();
    let objs = app.editable_selection();
    let has_sel = !objs.is_empty();
    let sample: Option<Object> = objs.first().cloned();
    let (style, label) = match &sample {
        Some(o) => {
            let l = match &o.kind {
                Kind::Box(b) => b.label.clone(),
                _ => app.label.clone(),
            };
            (o.style.clone(), l)
        }
        None => (app.style.clone(), app.label.clone()),
    };
    let sel_ids: Vec<Id> = objs.iter().map(|o| o.id.clone()).collect();
    let shapes_selected = objs.iter().any(|o| matches!(o.kind, Kind::Box(_)));
    let connectors = objs.iter().any(|o| matches!(o.kind, Kind::Connector(_)));
    let only_text = has_sel && objs.iter().all(|o| matches!(&o.kind, Kind::Box(b) if b.geom == BoxGeom::Text));
    let only_pen = has_sel && objs.iter().all(|o| matches!(o.kind, Kind::Pen(_)));
    let highlighter = if has_sel { objs.iter().all(|o| matches!(&o.kind, Kind::Pen(p) if p.highlighter)) } else { app.tool == Tool::Highlighter };
    let drawing = drawing_tool(app.tool);

    if has_sel || drawing {
        // ---- couleurs
        let stroke_label = if only_text || (!has_sel && app.tool == Tool::Text) {
            "Couleur du texte"
        } else if only_pen || matches!(app.tool, Tool::Pen | Tool::Highlighter) {
            "Couleur"
        } else {
            "Contour"
        };
        let mut set_stroke: Option<Color> = None;
        swatches(
            ui,
            Swatches { label: stroke_label, none: false, list: &PALETTE, current: Some(style.stroke), dark },
            |c| set_stroke = c,
            |v| {
                let c = Color::Rgb(v[0], v[1], v[2]);
                app.style.stroke = c;
                app.live_apply(&move |o| o.style.stroke = c);
            },
        );
        if let Some(c) = set_stroke {
            app.style.stroke = c;
            app.ed.update_many(&sel_ids, |o| o.style.stroke = c);
        }
        let fill_label = if matches!(app.tool, Tool::Sticky) || (has_sel && objs.iter().all(|o| matches!(&o.kind, Kind::Box(b) if b.geom == BoxGeom::Sticky))) {
            "Couleur de la note"
        } else {
            "Remplissage"
        };
        if !only_pen && !highlighter && !matches!(app.tool, Tool::Pen | Tool::Highlighter | Tool::Line | Tool::Arrow) && !connectors_only(&objs) {
            let mut set_fill: Option<Option<Color>> = None;
            swatches(
                ui,
                Swatches { label: fill_label, none: true, list: &FILLS, current: style.fill, dark },
                |c| set_fill = Some(c),
                |v| {
                    let c = Color::Rgb(v[0], v[1], v[2]);
                    app.style.fill = Some(c);
                    app.live_apply(&move |o| o.style.fill = Some(c));
                },
            );
            if let Some(c) = set_fill {
                app.style.fill = c;
                app.ed.update_many(&sel_ids, |o| o.style.fill = c);
            }
        }

        // ---- épaisseur et style de trait
        if !only_text {
            let widths: &[f32] = if highlighter {
                &[10.0, 18.0, 28.0, 40.0]
            } else if only_pen || matches!(app.tool, Tool::Pen) {
                &[1.5, 3.0, 6.0, 10.0]
            } else {
                &[0.0, 1.5, 3.0, 6.0, 10.0]
            };
            theme::field_label(ui, "Épaisseur");
            ui.add_space(-2.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
                for &w in widths {
                    let on = (style.width as f32 - w).abs() < 0.26;
                    let tip = if w == 0.0 { "Sans contour".to_owned() } else { format!("Épaisseur {w}") };
                    let resp = theme::seg_button(ui, on, &tip, |p, r, c| {
                        if w == 0.0 {
                            p.circle_stroke(r.center(), 6.0, Stroke::new(1.3_f32, c));
                            p.line_segment([r.center() + vec2(-5.0, 5.0), r.center() + vec2(5.0, -5.0)], Stroke::new(1.3_f32, c));
                        } else {
                            line_preview(p, r, if highlighter { w / 3.0 } else { w * 0.9 }, Dash::Solid, c);
                        }
                    });
                    if resp.clicked() {
                        app.style.width = f64::from(w);
                        app.ed.update_many(&sel_ids, |o| o.style.width = f64::from(w));
                    }
                }
            });
            ui.add_space(6.0);
            if !highlighter && !matches!(app.tool, Tool::Pen) && !only_pen {
                row(ui, "Style de trait", |ui| {
                    for (d, tip) in [(Dash::Solid, "Continu"), (Dash::Dashed, "Tirets"), (Dash::Dotted, "Pointillés")] {
                        if theme::seg_button(ui, style.dash == d, tip, |p, r, c| line_preview(p, r, 2.2, d, c)).clicked() {
                            app.style.dash = d;
                            app.ed.update_many(&sel_ids, |o| o.style.dash = d);
                        }
                    }
                });
            }
        }
        // ---- coins
        if shapes_selected || matches!(app.tool, Tool::Shape(BoxGeom::Rect) | Tool::Sticky) {
            row(ui, "Coins", |ui| {
                for (rd, tip, rx) in [(0.0f32, "Angles vifs", 0.0f32), (0.25, "Arrondis", 4.0)] {
                    let on = if rd == 0.0 { style.roundness < 0.05 } else { style.roundness >= 0.05 };
                    if theme::seg_button(ui, on, tip, |p, r, c| {
                        p.rect_stroke(ERect::from_center_size(r.center(), vec2(16.0, 12.0)), rx, Stroke::new(1.8_f32, c), egui::StrokeKind::Middle);
                    })
                    .clicked()
                    {
                        app.style.roundness = rd;
                        app.ed.update_many(&sel_ids, |o| o.style.roundness = rd);
                    }
                }
            });
        }
    }

    // ---- connecteurs
    if connectors || matches!(app.tool, Tool::Line | Tool::Arrow) {
        let cur = objs.iter().find_map(|o| if let Kind::Connector(c) = &o.kind { Some(c.clone()) } else { None });
        row(ui, "Tracé", |ui| {
            for (r, name) in [(Route::Straight, "Droit"), (Route::Elbow, "Coudé"), (Route::Curve, "Courbe")] {
                let on = cur.as_ref().map_or(app.route == r, |c| c.route == r);
                if ui.selectable_label(on, name).clicked() {
                    app.route = r;
                    app.ed.update_many(&sel_ids, |o| {
                        if let Kind::Connector(c) = &mut o.kind {
                            c.route = r
                        }
                    });
                }
            }
        });
        if let Some(c) = cur {
            for (title, is_start) in [("Pointe au début", true), ("Pointe à la fin", false)] {
                let now = if is_start { c.head_start } else { c.head_end };
                row(ui, title, |ui| {
                    for (h, name) in [(Arrowhead::None, "—"), (Arrowhead::Arrow, "→"), (Arrowhead::Triangle, "▶"), (Arrowhead::Dot, "●"), (Arrowhead::Bar, "|")]
                    {
                        if theme::seg_button(ui, now == h, "", |p, r, col| text_in(p, name, r, col, egui::FontId::proportional(14.0))).clicked() {
                            app.ed.update_many(&sel_ids, |o| {
                                if let Kind::Connector(c) = &mut o.kind {
                                    if is_start { c.head_start = h } else { c.head_end = h }
                                }
                            });
                        }
                    }
                });
            }
        }
    }

    // ---- texte
    if shapes_selected || matches!(app.tool, Tool::Text | Tool::Sticky | Tool::Shape(_)) {
        row(ui, "Police", |ui| {
            for (f, tip) in
                [(FontFamily::Sans, "Sans empattement"), (FontFamily::Hand, "Manuscrit"), (FontFamily::Serif, "Serif"), (FontFamily::Mono, "Chasse fixe")]
            {
                if theme::seg_button(ui, label.family == f, tip, |p, r, c| text_in(p, "Aa", r, c, fonts::font_id(f, 15.0, false))).clicked() {
                    app.label.family = f;
                    let m = app.measurer.clone();
                    app.ed.update_many(&sel_ids, |o| {
                        if let Kind::Box(b) = &mut o.kind {
                            b.label.family = f;
                            b.fit(&m);
                        }
                    });
                }
            }
        });
        row(ui, "Taille du texte", |ui| {
            for (s, name) in [(16.0, "S"), (22.0, "M"), (32.0, "L"), (48.0, "XL")] {
                if theme::seg_button(ui, (label.size - s).abs() < 0.5, "", |p, r, c| text_in(p, name, r, c, egui::FontId::proportional(13.0))).clicked() {
                    app.label.size = s;
                    let m = app.measurer.clone();
                    app.ed.update_many(&sel_ids, |o| {
                        if let Kind::Box(b) = &mut o.kind {
                            b.label.size = s;
                            b.fit(&m);
                        }
                    });
                }
            }
        });
        row(ui, "Alignement", |ui| {
            for (a, icon) in [(Align::Left, "alignL"), (Align::Center, "alignC"), (Align::Right, "alignR")] {
                if theme::seg_button(ui, label.align == a, "", |p, r, c| icon_in(p, icon, r, c)).clicked() {
                    app.label.align = a;
                    app.ed.update_many(&sel_ids, |o| {
                        if let Kind::Box(b) = &mut o.kind {
                            b.label.align = a
                        }
                    });
                }
            }
        });
    }
    if has_sel || app.editing.is_some() {
        rich_section(app, ui, dark);
    }

    // ---- opacité
    if has_sel || drawing {
        let mut op = style.opacity;
        theme::field_label(ui, &format!("Opacité {} %", (op * 100.0).round()));
        if ui.add(egui::Slider::new(&mut op, 0.05..=1.0).show_value(false)).changed() {
            app.style.opacity = op;
            app.live_apply(&move |o| o.style.opacity = op);
        }
        ui.add_space(6.0);
    }

    if has_sel {
        organisation(app, ui, &objs, &sel_ids, &pal);
    }
    let locked: Vec<Id> = app.selected_locked();
    if !locked.is_empty() && ui.button("Déverrouiller la sélection").clicked() {
        app.ed.set_locked(&locked, false);
    }
    if app.layers_open {
        layers(app, ui, has_sel, &sel_ids);
    }
}

fn connectors_only(objs: &[Object]) -> bool {
    !objs.is_empty() && objs.iter().all(|o| matches!(o.kind, Kind::Connector(_)))
}

/// B / I / U / S, couleur des mots et surlignage (texte riche).
fn rich_section(app: &mut App, ui: &mut Ui, dark: bool) {
    row(ui, if app.editing.is_some() { "Mise en forme de la sélection" } else { "Mise en forme de tout le texte" }, |ui| {
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
        if ui.small_button("Effacer").clicked() {
            app.apply_format(Format::Clear);
        }
    });
    theme::field_label(ui, "Couleur des mots");
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(5.0, 5.0);
        for c in PALETTE.iter().skip(1) {
            if swatch(ui, Some(*c), app.format_active(Format::Color(Some(*c))), dark).clicked() {
                app.apply_format(Format::Color(Some(*c)));
            }
        }
    });
    ui.add_space(6.0);
    theme::field_label(ui, "Surlignage");
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

/// Ordre, alignement, actions, mise en page automatique et opérations sur un tableau.
fn organisation(app: &mut App, ui: &mut Ui, objs: &[Object], sel_ids: &[Id], pal: &Palette) {
    row(ui, "Ordre", |ui| {
        for (icon, tip, o) in [
            ("front", "Premier plan", Order::Front),
            ("forward", "Avancer", Order::Forward),
            ("backward", "Reculer", Order::Backward),
            ("back", "Arrière-plan", Order::Back),
        ] {
            if theme::seg_button(ui, false, tip, |p, r, c| icon_in(p, icon, r, c)).clicked() {
                app.ed.reorder(sel_ids, o);
            }
        }
    });
    if sel_ids.len() > 1 {
        row(ui, "Alignement", |ui| {
            for (icon, tip, m) in [
                ("al", "Aligner à gauche", AlignMode::Left),
                ("ac", "Centrer horizontalement", AlignMode::CenterX),
                ("ar", "Aligner à droite", AlignMode::Right),
                ("at", "Aligner en haut", AlignMode::Top),
                ("am", "Centrer verticalement", AlignMode::CenterY),
                ("ab", "Aligner en bas", AlignMode::Bottom),
            ] {
                if theme::seg_button(ui, false, tip, |p, r, c| icon_in(p, icon, r, c)).clicked() {
                    app.ed.align(sel_ids, m);
                }
            }
            if sel_ids.len() > 2 {
                if theme::seg_button(ui, false, "Répartir horizontalement", |p, r, c| icon_in(p, "dh", r, c)).clicked() {
                    app.ed.distribute(sel_ids, Axis::Horizontal);
                }
                if theme::seg_button(ui, false, "Répartir verticalement", |p, r, c| icon_in(p, "dv", r, c)).clicked() {
                    app.ed.distribute(sel_ids, Axis::Vertical);
                }
            }
        });
        row(ui, "Mise en page automatique", |ui| {
            for (glyph, tip, d) in [
                ("↓", "De haut en bas", Direction::TopDown),
                ("→", "De gauche à droite", Direction::LeftRight),
                ("↑", "De bas en haut", Direction::BottomUp),
                ("←", "De droite à gauche", Direction::RightLeft),
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
        if theme::seg_button(ui, false, "Dupliquer", |p, r, c| icon_in(p, "copy", r, c)).clicked() {
            app.sel = app.ed.duplicate(sel_ids, Pt::new(24.0, 24.0));
        }
        if (sel_ids.len() > 1 || grouped)
            && theme::seg_button(ui, false, if grouped { "Dégrouper" } else { "Grouper" }, |p, r, c| {
                icon_in(p, if grouped { "ungroup" } else { "group" }, r, c)
            })
            .clicked()
        {
            if grouped {
                app.ed.ungroup(sel_ids);
            } else {
                app.ed.group(sel_ids);
            }
        }
        if theme::seg_button(ui, false, if all_locked { "Déverrouiller" } else { "Verrouiller" }, |p, r, c| {
            icon_in(p, if all_locked { "unlock" } else { "lock" }, r, c)
        })
        .clicked()
        {
            app.ed.set_locked(sel_ids, !all_locked);
        }
        let danger = Color32::from_rgb(0xe0, 0x31, 0x31);
        if theme::seg_button(ui, false, "Supprimer", |p, r, _| icon_in(p, "trash", r, danger)).clicked() {
            app.delete_selection();
        }
    });
    if let [o] = objs
        && matches!(o.kind, Kind::Table(_))
    {
        row(ui, "Tableau", |ui| {
            for (name, f) in [("+ ligne", 0), ("+ colonne", 1), ("− ligne", 2), ("− colonne", 3)] {
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

/// Liste des calques de la page (visibilité, verrou, nom, déplacement de la sélection).
fn layers(app: &mut App, ui: &mut Ui, has_sel: bool, sel_ids: &[Id]) {
    theme::menu_sep(ui);
    theme::field_label(ui, "Calques");
    let mut layers = app.ed.page().layers.clone();
    let mut changed = false;
    for l in &mut layers {
        ui.horizontal(|ui| {
            changed |= ui.checkbox(&mut l.visible, "").on_hover_text("Visible").changed();
            let mut locked = l.locked;
            if ui.toggle_value(&mut locked, "Verr.").on_hover_text("Verrouillé").changed() {
                l.locked = locked;
                changed = true;
            }
            let r = ui.add(egui::TextEdit::singleline(&mut l.name).desired_width(80.0));
            changed |= r.lost_focus() && r.changed();
            if has_sel && ui.small_button("→").on_hover_text("Y déplacer la sélection").clicked() {
                let id = l.id.clone();
                app.ed.update_many(sel_ids, |o| o.layer = id.clone());
            }
        });
    }
    if ui.small_button("+ calque").clicked() {
        layers.push(Layer { id: Id::new(), name: format!("Calque {}", layers.len() + 1), visible: true, locked: false });
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
