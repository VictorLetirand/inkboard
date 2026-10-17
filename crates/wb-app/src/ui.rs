//! Barres, panneaux, menus et dialogues.

use crate::app::{App, Dialog};
use crate::keymap::{Action, Shortcut};
use crate::tools::Tool;
use eframe::egui::{self, Color32, Painter, Rect as ERect, RichText as ERich, Stroke, pos2, vec2};
use std::time::{Duration, Instant};
use wb_core::editor::{AlignMode, Axis, Order};
use wb_core::layout::{Direction, auto_layout};
use wb_core::style::{FILLS, PALETTE};
use wb_core::text::Format;
use wb_core::*;

fn accent() -> Color32 {
    Color32::from_rgb(0x4f, 0x6d, 0xf5)
}

/// Entrée de menu affichant le raccourci courant de l'action.
fn item(ui: &mut egui::Ui, app: &App, label: &str, a: Action) -> bool {
    ui.add(egui::Button::new(label).shortcut_text(app.keymap.label(a))).clicked()
}

/* ------------------------------------------------------------- Icônes */

fn tool_icon(p: &Painter, r: ERect, tool: Tool, c: Color32) {
    let s = Stroke::new(1.7, c);
    let u = r.width() / 24.0;
    let pt = |x: f32, y: f32| r.min + vec2(x, y) * u;
    match tool {
        Tool::Select => {
            p.add(egui::Shape::closed_line(vec![pt(6.0, 3.0), pt(19.0, 11.0), pt(13.0, 13.0), pt(10.0, 19.5)], s));
        }
        Tool::Hand => {
            for x in [8.0, 11.0, 14.0, 17.0] {
                p.line_segment([pt(x, 12.0), pt(x, if x == 11.0 || x == 14.0 { 4.0 } else { 6.0 })], s);
            }
            p.add(egui::Shape::line(vec![pt(8.0, 12.0), pt(7.0, 15.0), pt(11.0, 20.0), pt(16.0, 20.0), pt(18.0, 15.0), pt(17.0, 12.0)], s));
        }
        Tool::Pen => {
            p.line_segment([pt(5.0, 19.0), pt(16.0, 5.0)], Stroke::new(3.0, c));
            p.line_segment([pt(16.0, 5.0), pt(19.0, 8.0)], s);
            p.line_segment([pt(5.0, 19.0), pt(4.0, 21.0)], s);
        }
        Tool::Highlighter => {
            p.line_segment([pt(6.0, 17.0), pt(17.0, 6.0)], Stroke::new(5.0, c.gamma_multiply(0.45)));
            p.line_segment([pt(4.0, 21.0), pt(20.0, 21.0)], s);
        }
        Tool::Eraser => {
            p.add(egui::Shape::closed_line(vec![pt(5.0, 14.0), pt(13.0, 5.0), pt(20.0, 12.0), pt(12.0, 19.0), pt(8.0, 19.0)], s));
            p.line_segment([pt(9.0, 9.0), pt(16.0, 16.0)], s);
        }
        Tool::Shape(BoxGeom::Rect) | Tool::Shape(BoxGeom::Sticky) | Tool::Shape(BoxGeom::Text) => {
            p.rect_stroke(ERect::from_min_max(pt(4.0, 6.0), pt(20.0, 18.0)), 2.0, s, egui::StrokeKind::Middle);
        }
        Tool::Shape(BoxGeom::Ellipse) => {
            p.add(egui::Shape::ellipse_stroke(pt(12.0, 12.0), vec2(9.0, 7.0) * u, s));
        }
        Tool::Shape(BoxGeom::Diamond) => {
            p.add(egui::Shape::closed_line(vec![pt(12.0, 3.0), pt(21.0, 12.0), pt(12.0, 21.0), pt(3.0, 12.0)], s));
        }
        Tool::Shape(BoxGeom::Triangle) => {
            p.add(egui::Shape::closed_line(vec![pt(12.0, 4.0), pt(21.0, 20.0), pt(3.0, 20.0)], s));
        }
        Tool::Line => {
            p.line_segment([pt(5.0, 19.0), pt(19.0, 5.0)], s);
        }
        Tool::Arrow => {
            p.line_segment([pt(5.0, 19.0), pt(19.0, 5.0)], s);
            p.add(egui::Shape::line(vec![pt(10.0, 5.0), pt(19.0, 5.0), pt(19.0, 14.0)], s));
        }
        Tool::Text => {
            p.line_segment([pt(5.0, 5.0), pt(19.0, 5.0)], s);
            p.line_segment([pt(12.0, 5.0), pt(12.0, 20.0)], s);
            p.line_segment([pt(9.0, 20.0), pt(15.0, 20.0)], s);
        }
        Tool::Sticky => {
            p.add(egui::Shape::closed_line(vec![pt(4.0, 4.0), pt(20.0, 4.0), pt(20.0, 14.0), pt(14.0, 20.0), pt(4.0, 20.0)], s));
            p.add(egui::Shape::line(vec![pt(14.0, 20.0), pt(14.0, 14.0), pt(20.0, 14.0)], s));
        }
        Tool::Vote => {
            p.circle_filled(pt(12.0, 12.0), 7.0 * u, c.gamma_multiply(0.85));
            p.circle_stroke(pt(12.0, 12.0), 9.5 * u, s);
        }
        Tool::Table => {
            p.rect_stroke(ERect::from_min_max(pt(4.0, 5.0), pt(20.0, 19.0)), 1.0, s, egui::StrokeKind::Middle);
            p.line_segment([pt(4.0, 10.0), pt(20.0, 10.0)], s);
            p.line_segment([pt(10.0, 5.0), pt(10.0, 19.0)], s);
            p.line_segment([pt(15.0, 5.0), pt(15.0, 19.0)], s);
        }
    }
}

fn image_icon(p: &Painter, r: ERect, c: Color32) {
    let u = r.width() / 24.0;
    let pt = |x: f32, y: f32| r.min + vec2(x, y) * u;
    let s = Stroke::new(1.7, c);
    p.rect_stroke(ERect::from_min_max(pt(3.0, 4.0), pt(21.0, 20.0)), 2.0, s, egui::StrokeKind::Middle);
    p.circle_stroke(pt(9.0, 10.0), 1.6 * u, s);
    p.add(egui::Shape::line(vec![pt(21.0, 16.0), pt(16.0, 11.0), pt(7.0, 20.0)], s));
}

const TOOLS: [(Tool, &str); 15] = [
    (Tool::Select, "Sélection (V)"),
    (Tool::Hand, "Main (H)"),
    (Tool::Pen, "Crayon (P)"),
    (Tool::Highlighter, "Surligneur (M)"),
    (Tool::Eraser, "Gomme (E)"),
    (Tool::Shape(BoxGeom::Rect), "Rectangle (R)"),
    (Tool::Shape(BoxGeom::Ellipse), "Ellipse (O)"),
    (Tool::Shape(BoxGeom::Diamond), "Losange (D)"),
    (Tool::Shape(BoxGeom::Triangle), "Triangle (Y)"),
    (Tool::Line, "Ligne (L) — s'accroche aux formes"),
    (Tool::Arrow, "Flèche (A) — s'accroche aux formes"),
    (Tool::Text, "Texte (T)"),
    (Tool::Sticky, "Note adhésive (N)"),
    (Tool::Table, "Tableau (B)"),
    (Tool::Vote, "Pastille de vote — un clic en pose une"),
];

pub fn tool_bar(app: &mut App, ctx: &egui::Context) {
    egui::SidePanel::left("tools").resizable(false).exact_width(48.0).show(ctx, |ui| {
        ui.add_space(4.0);
        ui.vertical_centered(|ui| {
            for (t, tip) in TOOLS {
                if matches!(t, Tool::Pen | Tool::Shape(BoxGeom::Rect) | Tool::Line | Tool::Text) {
                    ui.add_space(6.0);
                }
                let (r, resp) = ui.allocate_exact_size(vec2(36.0, 36.0), egui::Sense::click());
                let active = app.tool == t;
                if active {
                    ui.painter().rect_filled(r, 8.0, accent().gamma_multiply(0.18));
                } else if resp.hovered() {
                    ui.painter().rect_filled(r, 8.0, Color32::from_black_alpha(14));
                }
                tool_icon(ui.painter(), r.shrink(7.0), t, if active { accent() } else { ui.visuals().text_color() });
                let key = Action::for_tool(t).map(|a| app.keymap.label(a)).unwrap_or_default();
                let tip = if key.is_empty() { tip.to_owned() } else { format!("{tip} ({key})") };
                if resp.on_hover_text(tip).clicked() {
                    app.set_tool(t);
                }
            }
            let (r, resp) = ui.allocate_exact_size(vec2(36.0, 36.0), egui::Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(r, 8.0, Color32::from_black_alpha(14));
            }
            image_icon(ui.painter(), r.shrink(7.0), ui.visuals().text_color());
            if resp.on_hover_text("Insérer une image").clicked() {
                app.insert_images_dialog();
            }
            ui.add_space(6.0);
            let lock = ui.selectable_label(app.tool_lock, "Fixe").on_hover_text("Garder l'outil actif (Q)");
            if lock.clicked() {
                app.tool_lock = !app.tool_lock;
            }
        });
    });
}

/* -------------------------------------------------------------- Menus */

pub fn top_bar(app: &mut App, ctx: &egui::Context) {
    egui::TopBottomPanel::top("menu").show(ctx, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("Fichier", |ui| {
                if item(ui, app, "Nouveau", Action::New) {
                    app.new_document();
                    ui.close();
                }
                if item(ui, app, "Ouvrir / importer…", Action::Open) {
                    ui.close();
                    app.open_dialog();
                }
                if ui.button("Importer du Mermaid…").clicked() {
                    app.dialog = Some(Dialog::Mermaid {
                        text: "flowchart TD\n  A[Idée] --> B{Faisable ?}\n  B -->|Oui| C[Prototype]\n  B -->|Non| D[Abandon]".into(),
                        new_board: false,
                        error: None,
                    });
                    ui.close();
                }
                ui.separator();
                if item(ui, app, "Enregistrer", Action::Save) {
                    ui.close();
                    app.save();
                }
                if item(ui, app, "Enregistrer sous…", Action::SaveAs) {
                    ui.close();
                    app.save_as();
                }
                ui.menu_button("Exporter", |ui| {
                    for (label, ext) in [
                        ("draw.io (.drawio)", "drawio"),
                        ("Excalidraw", "excalidraw"),
                        ("JSON Canvas (.canvas)", "canvas"),
                        ("Image vectorielle (.svg)", "svg"),
                        ("Image (.png)", "png"),
                        ("Document PDF (.pdf)", "pdf"),
                    ] {
                        if ui.button(label).clicked() {
                            ui.close();
                            app.export(ext);
                        }
                    }
                });
                ui.separator();
                if ui.button("Quitter").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Édition", |ui| {
                ui.add_enabled_ui(app.ed.can_undo(), |ui| {
                    if item(ui, app, "Annuler", Action::Undo) {
                        app.ed.undo();
                        ui.close();
                    }
                });
                ui.add_enabled_ui(app.ed.can_redo(), |ui| {
                    if item(ui, app, "Rétablir", Action::Redo) {
                        app.ed.redo();
                        ui.close();
                    }
                });
                ui.separator();
                if item(ui, app, "Tout sélectionner", Action::SelectAll) {
                    app.select_all();
                    ui.close();
                }
                if !app.sel.is_empty() {
                    selection_menu(app, ui);
                }
            });
            ui.menu_button("Insérer", |ui| {
                if ui.button("Bloc de code").clicked() {
                    app.insert_code_block();
                    ui.close();
                }
                if ui.button("Formule (LaTeX)…").clicked() {
                    app.dialog = Some(Dialog::Formula { src: r"E = mc^2, \quad \sum_{i=1}^{n} x_i \leq \frac{a+b}{2}".into() });
                    ui.close();
                }
                if ui.button("Pastille de vote").clicked() {
                    app.tool = Tool::Vote;
                    ui.close();
                }
            });
            ui.menu_button("Affichage", |ui| {
                ui.label("Arrière-plan");
                for p in BgPreset::ALL {
                    if ui.radio(app.ed.doc.background.preset == p, p.label()).clicked() {
                        let mut bg = app.ed.doc.background;
                        bg.preset = p;
                        app.ed.set_background(bg);
                    }
                }
                ui.separator();
                for (label, pat) in [("Aucun motif", Pattern::None), ("Points", Pattern::Dots), ("Quadrillage", Pattern::Grid), ("Lignes", Pattern::Lines)] {
                    if ui.radio(app.ed.doc.background.pattern == pat, label).clicked() {
                        let mut bg = app.ed.doc.background;
                        bg.pattern = pat;
                        app.ed.set_background(bg);
                    }
                }
                ui.separator();
                ui.checkbox(&mut app.snap, "Magnétisme de la grille");
                if ui.button("Raccourcis clavier…").clicked() {
                    app.dialog = Some(Dialog::Shortcuts { filter: String::new(), recording: None });
                    ui.close();
                }
                if item(ui, app, "Zoom 100 %", Action::Zoom100) {
                    let v = app.ed.page().view;
                    app.set_view(View { zoom: 1.0, ..v });
                    ui.close();
                }
                if item(ui, app, "Tout afficher", Action::ZoomFit) {
                    app.fit_view(false);
                    ui.close();
                }
                ui.separator();
                if item(ui, app, "Présenter en plein écran", Action::Present) {
                    app.present_req = Some(true);
                    ui.close();
                }
                ui.menu_button("Minuteur", |ui| {
                    for min in [1u64, 5, 10, 15, 30] {
                        if ui.button(format!("{min} min")).clicked() {
                            let d = Duration::from_secs(min * 60);
                            app.timer = Some((Instant::now() + d, d));
                            ui.close();
                        }
                    }
                    if app.timer.is_some() && ui.button("Arrêter").clicked() {
                        app.timer = None;
                        ui.close();
                    }
                });
            });
        });
    });
}

/// Actions applicables à la sélection (menu contextuel et menu Édition).
pub fn selection_menu(app: &mut App, ui: &mut egui::Ui) {
    let sel = app.sel.clone();
    if ui.button("Couper").clicked() {
        app.copy_selection();
        app.delete_selection();
        ui.close();
    }
    if ui.button("Copier").clicked() {
        app.copy_selection();
        ui.close();
    }
    if ui.button("Copier comme image").clicked() {
        app.copy_as_image();
        ui.close();
    }
    if ui.button("Coller").clicked() {
        app.paste();
        ui.close();
    }
    if item(ui, app, "Dupliquer", Action::Duplicate) {
        app.sel = app.ed.duplicate(&sel, Pt::new(24.0, 24.0));
        ui.close();
    }
    if let [id] = sel.as_slice()
        && app.ed.page().get(id).is_some_and(|o| matches!(o.kind, Kind::Box(_) | Kind::Table(_) | Kind::Connector(_)))
        && ui.button("Modifier le texte").clicked()
    {
        let cell = match &app.ed.page().get(id).map(|o| &o.kind) {
            Some(Kind::Table(t)) => t.cell_at(app.ctx_pos),
            _ => None,
        };
        app.start_editing(id, cell);
        ui.close();
    }
    if let [id] = sel.as_slice()
        && app.ed.page().get(id).is_some_and(|o| matches!(&o.kind, Kind::Connector(c) if !c.via.is_empty()))
        && ui.button("Redresser (retirer les points de passage)").clicked()
    {
        app.ed.update(id, |o| {
            if let Kind::Connector(c) = &mut o.kind {
                c.via.clear();
            }
        });
        ui.close();
    }
    table_menu(app, ui, &sel);
    ui.separator();
    for (label, o) in [("Premier plan", Order::Front), ("Avancer", Order::Forward), ("Reculer", Order::Backward), ("Arrière-plan", Order::Back)] {
        if ui.button(label).clicked() {
            app.ed.reorder(&sel, o);
            ui.close();
        }
    }
    ui.separator();
    if sel.len() > 1 && item(ui, app, "Grouper", Action::Group) {
        app.ed.group(&sel);
        ui.close();
    }
    if ui.button("Dégrouper").clicked() {
        app.ed.ungroup(&sel);
        ui.close();
    }
    let all_locked = sel.iter().all(|i| app.ed.page().get(i).is_some_and(|o| o.locked));
    if ui.button(if all_locked { "Déverrouiller" } else { "Verrouiller" }).clicked() {
        app.ed.set_locked(&sel, !all_locked);
        ui.close();
    }
    if sel.len() > 1 {
        ui.menu_button("Mise en page automatique", |ui| {
            for (label, d) in [
                ("De haut en bas", Direction::TopDown),
                ("De gauche à droite", Direction::LeftRight),
                ("De bas en haut", Direction::BottomUp),
                ("De droite à gauche", Direction::RightLeft),
            ] {
                if ui.button(label).clicked() {
                    auto_layout(&mut app.ed, &sel, d);
                    ui.close();
                }
            }
        });
    }
    ui.separator();
    if item(ui, app, "Supprimer", Action::Delete) {
        app.delete_selection();
        ui.close();
    }
}

/// Opérations sur la ligne / colonne située sous le clic droit, si un tableau est sélectionné.
fn table_menu(app: &mut App, ui: &mut egui::Ui, sel: &[Id]) {
    let [id] = sel else { return };
    let Some((r, c)) = app.ed.page().get(id).and_then(|o| if let Kind::Table(t) = &o.kind { t.cell_at(app.ctx_pos) } else { None }) else { return };
    ui.separator();
    ui.label(ERich::new(format!("Cellule ligne {} · colonne {}", r + 1, c + 1)).small().weak());
    type Op = fn(&mut wb_core::Table, usize, usize);
    let ops: [(&str, Op); 6] = [
        ("Insérer une ligne au-dessus", |t, r, _| t.insert_row(r)),
        ("Insérer une ligne en dessous", |t, r, _| t.insert_row(r + 1)),
        ("Insérer une colonne à gauche", |t, _, c| t.insert_col(c)),
        ("Insérer une colonne à droite", |t, _, c| t.insert_col(c + 1)),
        ("Supprimer la ligne", |t, r, _| t.remove_row(r)),
        ("Supprimer la colonne", |t, _, c| t.remove_col(c)),
    ];
    for (label, op) in ops {
        if ui.button(label).clicked() {
            app.ed.update(id, |o| {
                if let Kind::Table(t) = &mut o.kind {
                    op(t, r, c);
                }
            });
            ui.close();
        }
    }
}

/* ------------------------------------------------------- Barre du bas */

pub fn bottom_bar(app: &mut App, ctx: &egui::Context) {
    egui::TopBottomPanel::bottom("bottom").show(ctx, |ui| {
        ui.horizontal(|ui| {
            let pages: Vec<(Id, String)> = app.ed.doc.pages.iter().map(|p| (p.id.clone(), p.name.clone())).collect();
            for (id, name) in &pages {
                let resp = ui.selectable_label(&app.ed.current == id, name);
                if resp.clicked() {
                    app.ed.current = id.clone();
                    app.sel.clear();
                    app.editing = None;
                }
                resp.context_menu(|ui| {
                    if ui.button("Renommer…").clicked() {
                        app.dialog = Some(Dialog::RenamePage { id: id.clone(), name: name.clone() });
                        ui.close();
                    }
                    if ui.add_enabled(pages.len() > 1, egui::Button::new("Supprimer la page")).clicked() {
                        app.ed.remove_page(id);
                        app.sel.clear();
                        ui.close();
                    }
                });
            }
            if ui.button("+").on_hover_text("Nouvelle page").clicked() {
                let n = app.ed.doc.pages.len() + 1;
                app.ed.add_page(&format!("Page {n}"));
                app.sel.clear();
                app.fit_pending = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some((text, over)) = timer_text(app) {
                    let c = if over { Color32::from_rgb(0xe0, 0x31, 0x31) } else { ui.visuals().text_color() };
                    ui.label(ERich::new(format!("⏱ {text}")).color(c));
                    ui.ctx().request_repaint_after(Duration::from_millis(500));
                }
                let v = app.ed.page().view;
                if ui.button("Tout afficher").on_hover_text(app.keymap.label(Action::ZoomFit)).clicked() {
                    app.fit_view(false);
                }
                if ui.button("+").clicked() {
                    app.zoom_by(1.25);
                }
                if ui.button(format!("{:.0} %", v.zoom * 100.0)).on_hover_text("Revenir à 100 %").clicked() {
                    app.set_view(View { zoom: 1.0, ..v });
                }
                if ui.button("−").clicked() {
                    app.zoom_by(0.8);
                }
                ui.separator();
                if ui
                    .add_enabled(app.ed.can_redo(), egui::Button::new("Rétablir"))
                    .on_hover_text(format!("Rétablir ({})", app.keymap.label(Action::Redo)))
                    .clicked()
                {
                    app.ed.redo();
                }
                if ui
                    .add_enabled(app.ed.can_undo(), egui::Button::new("Annuler"))
                    .on_hover_text(format!("Annuler ({})", app.keymap.label(Action::Undo)))
                    .clicked()
                {
                    app.ed.undo();
                }
            });
        });
    });
}

/* ---------------------------------------------------- Panneau de droite */

fn swatch(ui: &mut egui::Ui, c: Option<Color>, selected: bool, dark: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), egui::Sense::click());
    let p = ui.painter();
    match c {
        Some(Color::Ink) => {
            p.rect_filled(r, 5.0, Color32::from_rgb(0x1e, 0x1e, 0x1e));
            p.add(egui::Shape::convex_polygon(vec![r.right_top(), r.right_bottom(), r.left_bottom()], Color32::from_rgb(0xf1, 0xf3, 0xf5), Stroke::NONE));
        }
        Some(c) => {
            let (rr, g, b) = c.resolve(dark);
            p.rect_filled(r, 5.0, Color32::from_rgb(rr, g, b));
        }
        None => {
            p.rect_filled(r, 5.0, Color32::WHITE);
            p.line_segment([r.left_bottom(), r.right_top()], Stroke::new(1.5, Color32::from_rgb(0xe0, 0x31, 0x31)));
        }
    }
    p.rect_stroke(
        r,
        5.0,
        Stroke::new(if selected { 2.5 } else { 1.0 }, if selected { accent() } else { Color32::from_black_alpha(60) }),
        egui::StrokeKind::Middle,
    );
    resp
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(8.0);
    ui.label(ERich::new(title).small().weak());
}

pub fn side_panel(app: &mut App, ctx: &egui::Context) {
    egui::SidePanel::right("props").default_width(244.0).min_width(220.0).show(ctx, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            props(app, ui);
        });
    });
    if app.prop_edit.is_some() && !ctx.input(|i| i.pointer.any_down()) {
        app.live_end();
    }
}

fn props(app: &mut App, ui: &mut egui::Ui) {
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
    ui.heading(if has_sel { format!("{} sélectionné(s)", objs.len()) } else { "Style".to_owned() });
    let shapes_selected = objs.iter().any(|o| matches!(o.kind, Kind::Box(_)));
    let connectors = objs.iter().any(|o| matches!(o.kind, Kind::Connector(_)));
    let drawing_tool = matches!(app.tool, Tool::Pen | Tool::Highlighter | Tool::Shape(_) | Tool::Line | Tool::Arrow | Tool::Text | Tool::Sticky | Tool::Table);
    if !has_sel && !drawing_tool {
        ui.label(ERich::new("Sélectionnez un objet ou choisissez un outil pour régler son style.").weak());
    }

    // ---- couleurs
    if has_sel || drawing_tool {
        section(ui, "Contour / texte");
        ui.horizontal_wrapped(|ui| {
            for c in PALETTE {
                if swatch(ui, Some(c), style.stroke == c, dark).clicked() {
                    app.style.stroke = c;
                    app.ed.update_many(&sel_ids, |o| o.style.stroke = c);
                }
            }
            let mut rgb = {
                let (r, g, b) = style.stroke.resolve(dark);
                [r, g, b]
            };
            if ui.color_edit_button_srgb(&mut rgb).changed() {
                let c = Color::Rgb(rgb[0], rgb[1], rgb[2]);
                app.style.stroke = c;
                app.live_apply(&move |o| o.style.stroke = c);
            }
        });
        section(ui, "Remplissage");
        ui.horizontal_wrapped(|ui| {
            if swatch(ui, None, style.fill.is_none(), dark).clicked() {
                app.style.fill = None;
                app.ed.update_many(&sel_ids, |o| o.style.fill = None);
            }
            for c in FILLS {
                if swatch(ui, Some(c), style.fill == Some(c), dark).clicked() {
                    app.style.fill = Some(c);
                    app.ed.update_many(&sel_ids, |o| o.style.fill = Some(c));
                }
            }
        });

        // ---- trait
        section(ui, "Épaisseur");
        let mut w = style.width as f32;
        let r = ui.add(egui::Slider::new(&mut w, 0.0..=24.0).fixed_decimals(1));
        if r.changed() {
            app.style.width = f64::from(w);
            app.live_apply(&move |o| o.style.width = f64::from(w));
        }
        ui.horizontal(|ui| {
            for (d, label) in [(Dash::Solid, "Continu"), (Dash::Dashed, "Tirets"), (Dash::Dotted, "Points")] {
                if ui.selectable_label(style.dash == d, label).clicked() {
                    app.style.dash = d;
                    app.ed.update_many(&sel_ids, |o| o.style.dash = d);
                }
            }
        });
        section(ui, "Opacité");
        let mut op = style.opacity;
        if ui.add(egui::Slider::new(&mut op, 0.05..=1.0)).changed() {
            app.style.opacity = op;
            app.live_apply(&move |o| o.style.opacity = op);
        }
        if shapes_selected || matches!(app.tool, Tool::Shape(BoxGeom::Rect) | Tool::Sticky) {
            section(ui, "Arrondi des coins");
            let mut rd = style.roundness;
            if ui.add(egui::Slider::new(&mut rd, 0.0..=1.0)).changed() {
                app.style.roundness = rd;
                app.live_apply(&move |o| o.style.roundness = rd);
            }
        }
    }

    // ---- texte
    if shapes_selected || matches!(app.tool, Tool::Text | Tool::Sticky | Tool::Shape(_)) {
        section(ui, "Police");
        ui.horizontal(|ui| {
            for (f, name) in [(FontFamily::Sans, "Sans"), (FontFamily::Hand, "Manuscrit"), (FontFamily::Serif, "Serif"), (FontFamily::Mono, "Mono")] {
                if ui.selectable_label(label.family == f, name).clicked() {
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
        ui.horizontal(|ui| {
            for (s, name) in [(16.0, "S"), (22.0, "M"), (32.0, "L"), (48.0, "XL")] {
                if ui.selectable_label((label.size - s).abs() < 0.5, name).clicked() {
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
        ui.horizontal(|ui| {
            for (a, name) in [(Align::Left, "⬅"), (Align::Center, "↔"), (Align::Right, "➡")] {
                if ui.selectable_label(label.align == a, name).clicked() {
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
        {
            section(ui, if app.editing.is_some() { "Mise en forme de la sélection" } else { "Mise en forme de tout le texte" });
            ui.horizontal(|ui| {
                for (name, f) in [("B", Format::Bold), ("I", Format::Italic), ("U", Format::Underline), ("S", Format::Strike)] {
                    let t = egui::RichText::new(name);
                    let t = match f {
                        Format::Bold => t.strong(),
                        Format::Italic => t.italics(),
                        Format::Underline => t.underline(),
                        _ => t.strikethrough(),
                    };
                    if ui.selectable_label(app.format_active(f), t).clicked() {
                        app.apply_format(f);
                    }
                }
                if ui.small_button("Effacer").clicked() {
                    app.apply_format(Format::Clear);
                }
            });
            ui.label(ERich::new("Couleur des mots").small().weak());
            ui.horizontal_wrapped(|ui| {
                for c in PALETTE.iter().skip(1) {
                    if swatch(ui, Some(*c), app.format_active(Format::Color(Some(*c))), dark).clicked() {
                        app.apply_format(Format::Color(Some(*c)));
                    }
                }
            });
            ui.label(ERich::new("Surlignage").small().weak());
            ui.horizontal_wrapped(|ui| {
                for c in FILLS.iter().take(8) {
                    if swatch(ui, Some(*c), app.format_active(Format::Highlight(Some(*c))), dark).clicked() {
                        app.apply_format(Format::Highlight(Some(*c)));
                    }
                }
            });
        }
    }

    // ---- connecteurs
    if connectors || matches!(app.tool, Tool::Line | Tool::Arrow) {
        section(ui, "Tracé du connecteur");
        let cur = objs.iter().find_map(|o| if let Kind::Connector(c) = &o.kind { Some(c.clone()) } else { None });
        ui.horizontal(|ui| {
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
            for (title, is_start) in [("Début", true), ("Fin", false)] {
                ui.horizontal(|ui| {
                    ui.label(title);
                    let now = if is_start { c.head_start } else { c.head_end };
                    for (h, name) in [(Arrowhead::None, "—"), (Arrowhead::Arrow, "→"), (Arrowhead::Triangle, "▶"), (Arrowhead::Dot, "●"), (Arrowhead::Bar, "|")]
                    {
                        if ui.selectable_label(now == h, name).clicked() {
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

    // ---- organisation
    if has_sel {
        section(ui, "Organisation");
        ui.horizontal_wrapped(|ui| {
            for (name, o) in [("Premier plan", Order::Front), ("Avancer", Order::Forward), ("Reculer", Order::Backward), ("Arrière", Order::Back)] {
                if ui.small_button(name).clicked() {
                    app.ed.reorder(&sel_ids, o);
                }
            }
        });
        if sel_ids.len() > 1 {
            ui.horizontal_wrapped(|ui| {
                for (name, m) in [
                    ("⇤", AlignMode::Left),
                    ("↔", AlignMode::CenterX),
                    ("⇥", AlignMode::Right),
                    ("⤒", AlignMode::Top),
                    ("↕", AlignMode::CenterY),
                    ("⤓", AlignMode::Bottom),
                ] {
                    if ui.small_button(name).on_hover_text("Aligner").clicked() {
                        app.ed.align(&sel_ids, m);
                    }
                }
                if sel_ids.len() > 2 {
                    if ui.small_button("⇔").on_hover_text("Répartir horizontalement").clicked() {
                        app.ed.distribute(&sel_ids, Axis::Horizontal);
                    }
                    if ui.small_button("⇕").on_hover_text("Répartir verticalement").clicked() {
                        app.ed.distribute(&sel_ids, Axis::Vertical);
                    }
                }
            });
        }
        ui.horizontal_wrapped(|ui| {
            if sel_ids.len() > 1 && ui.small_button("Grouper").clicked() {
                app.ed.group(&sel_ids);
            }
            if ui.small_button("Dégrouper").clicked() {
                app.ed.ungroup(&sel_ids);
            }
            if ui.small_button("Verrouiller").clicked() {
                app.ed.set_locked(&sel_ids, true);
            }
        });
        if sel_ids.len() > 1 {
            ui.menu_button("Mise en page automatique", |ui| {
                for (label, d) in [("De haut en bas", Direction::TopDown), ("De gauche à droite", Direction::LeftRight)] {
                    if ui.button(label).clicked() {
                        auto_layout(&mut app.ed, &sel_ids, d);
                        ui.close();
                    }
                }
            });
        }
        if let [o] = objs.as_slice()
            && matches!(o.kind, Kind::Table(_))
        {
            section(ui, "Tableau");
            ui.horizontal_wrapped(|ui| {
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
    }
    let locked: Vec<Id> = app.selected_locked();
    if !locked.is_empty() && ui.button("Déverrouiller la sélection").clicked() {
        app.ed.set_locked(&locked, false);
    }

    // ---- calques
    section(ui, "Calques");
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
            let r = ui.add(egui::TextEdit::singleline(&mut l.name).desired_width(110.0));
            changed |= r.lost_focus() && r.changed();
            if has_sel && ui.small_button("→").on_hover_text("Y déplacer la sélection").clicked() {
                let id = l.id.clone();
                app.ed.update_many(&sel_ids, |o| o.layer = id.clone());
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
    let _ = pos2(0.0, 0.0);
}

impl App {
    fn selected_locked(&self) -> Vec<Id> {
        let p = self.ed.page();
        p.objects.iter().filter(|o| self.sel.contains(&o.id) && o.locked).map(|o| o.id.clone()).collect()
    }
}

/* ------------------------------------------------------- Présentation */

/// Minuteur restant sous la forme « m:ss » ; `None` si aucun minuteur n'est lancé.
pub fn timer_text(app: &App) -> Option<(String, bool)> {
    let (end, _) = app.timer?;
    let left = end.saturating_duration_since(Instant::now()).as_secs();
    Some((format!("{}:{:02}", left / 60, left % 60), left == 0))
}

/// Surimpression discrète du mode présentation : numéro de page et minuteur.
pub fn present_overlay(app: &mut App, ctx: &egui::Context) {
    let n = app.ed.doc.pages.len();
    let i = app.ed.doc.pages.iter().position(|p| p.id == app.ed.current).unwrap_or(0) + 1;
    if n > 1 {
        egui::Area::new(egui::Id::new("present_page")).anchor(egui::Align2::CENTER_BOTTOM, vec2(0.0, -12.0)).order(egui::Order::Tooltip).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).fill(Color32::from_black_alpha(150)).show(ui, |ui| {
                ui.label(ERich::new(format!("{i} / {n}")).color(Color32::WHITE));
            });
        });
    }
    if let Some((text, over)) = timer_text(app) {
        egui::Area::new(egui::Id::new("present_timer")).anchor(egui::Align2::RIGHT_TOP, vec2(-14.0, 12.0)).order(egui::Order::Tooltip).show(ctx, |ui| {
            let fill = if over { Color32::from_rgb(0xc9, 0x2a, 0x2a) } else { Color32::from_black_alpha(150) };
            egui::Frame::popup(ui.style()).fill(fill).show(ui, |ui| {
                ui.label(ERich::new(text).size(22.0).color(Color32::WHITE));
            });
        });
        ctx.request_repaint_after(Duration::from_millis(250));
    }
}

/* ------------------------------------------------------------ Dialogues */

pub fn dialogs(app: &mut App, ctx: &egui::Context) {
    let Some(dialog) = app.dialog.take() else { return };
    let mut keep: Option<Dialog> = None;
    match dialog {
        Dialog::Mermaid { mut text, mut new_board, mut error } => {
            let mut open = true;
            let mut generate = false;
            egui::Window::new("Importer du Mermaid").fade_in(false).open(&mut open).collapsible(false).default_width(520.0).show(ctx, |ui| {
                ui.label("flowchart / graph, stateDiagram ou sequenceDiagram. La mise en page est automatique.");
                ui.add(egui::TextEdit::multiline(&mut text).code_editor().desired_rows(14).desired_width(f32::INFINITY));
                ui.radio_value(&mut new_board, false, "Ajouter à ce tableau");
                ui.radio_value(&mut new_board, true, "Nouveau tableau");
                if let Some(e) = &error {
                    ui.colored_label(Color32::from_rgb(0xe0, 0x31, 0x31), e);
                }
                if ui.button("Générer").clicked() {
                    generate = true;
                }
            });
            if generate {
                match app.import_mermaid(&text, new_board) {
                    Ok(()) => open = false,
                    Err(e) => error = Some(e),
                }
            }
            if open {
                keep = Some(Dialog::Mermaid { text, new_board, error });
            }
        }
        Dialog::Formula { mut src } => {
            let mut open = true;
            let mut insert = false;
            egui::Window::new("Insérer une formule").fade_in(false).open(&mut open).collapsible(false).default_width(420.0).show(ctx, |ui| {
                ui.label("Source LaTeX. L'aperçu est une approximation en Unicode (lettres grecques, exposants, fractions simples).");
                ui.add(egui::TextEdit::multiline(&mut src).code_editor().desired_rows(3).desired_width(f32::INFINITY));
                ui.separator();
                ui.label(egui::RichText::new(wb_core::latex::to_unicode(&src)).size(24.0));
                if ui.button("Insérer").clicked() {
                    insert = true;
                }
            });
            if insert {
                app.insert_formula(&src);
                open = false;
            }
            if open {
                keep = Some(Dialog::Formula { src });
            }
        }
        Dialog::Shortcuts { filter, recording } => {
            keep = shortcuts_dialog(app, ctx, filter, recording);
        }
        Dialog::RenamePage { id, mut name } => {
            let mut open = true;
            let mut accepted = false;
            egui::Window::new("Renommer la page").fade_in(false).open(&mut open).collapsible(false).show(ctx, |ui| {
                let r = ui.text_edit_singleline(&mut name);
                r.request_focus();
                if ui.button("OK").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                    accepted = true;
                }
            });
            if accepted {
                app.ed.rename_page(&id, &name);
                open = false;
            }
            if open {
                keep = Some(Dialog::RenamePage { id, name });
            }
        }
    }
    if app.dialog.is_none() {
        app.dialog = keep;
    }
}

/// Fenêtre de réglage des raccourcis : liste par groupe, ajout (enregistrement de la prochaine touche), retrait, réinitialisation.
fn shortcuts_dialog(app: &mut App, ctx: &egui::Context, mut filter: String, mut recording: Option<Action>) -> Option<Dialog> {
    let mut open = true;
    // enregistrement : la prochaine combinaison pressée est affectée à l'action
    if let Some(a) = recording {
        let pressed = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, repeat: false, .. } => Some((*key, *modifiers)),
                _ => None,
            })
        });
        if let Some((key, m)) = pressed {
            if key == egui::Key::Escape {
                recording = None;
            } else {
                let sc = Shortcut::new(key, m.command, m.shift, m.alt);
                match app.keymap.add(a, sc) {
                    Ok(Some(other)) => {
                        app.toast(&format!("{} retiré de « {} »", sc.label(), other.label()));
                        app.save_keymap();
                    }
                    Ok(None) => app.save_keymap(),
                    Err(e) => app.toast(&e),
                }
                recording = None;
            }
        }
    }
    egui::Window::new("Raccourcis clavier").fade_in(false).open(&mut open).collapsible(false).default_width(520.0).default_height(560.0).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Filtrer :");
            ui.text_edit_singleline(&mut filter);
            if ui.button("Tout réinitialiser").clicked() {
                app.keymap.reset_all();
                app.save_keymap();
                recording = None;
            }
        });
        ui.label(
            ERich::new("Cliquez sur « + » puis appuyez sur la combinaison voulue (Échap pour annuler). Cliquer sur un raccourci le retire.").small().weak(),
        );
        for (sc, x, y) in app.keymap.conflicts() {
            ui.colored_label(Color32::from_rgb(0xe0, 0x31, 0x31), format!("Conflit : {} est utilisé par « {} » et « {} »", sc.label(), x.label(), y.label()));
        }
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            let mut group = "";
            let f = filter.to_lowercase();
            for a in Action::ALL {
                let shown =
                    f.is_empty() || a.label().to_lowercase().contains(&f) || app.keymap.bindings(a).iter().any(|s| s.label().to_lowercase().contains(&f));
                if !shown {
                    continue;
                }
                if a.group() != group {
                    group = a.group();
                    ui.add_space(6.0);
                    ui.label(ERich::new(group).strong());
                }
                ui.horizontal(|ui| {
                    let (cell, _) = ui.allocate_exact_size(vec2(190.0, 22.0), egui::Sense::hover());
                    ui.painter().text(
                        cell.left_center(),
                        egui::Align2::LEFT_CENTER,
                        a.label(),
                        egui::TextStyle::Body.resolve(ui.style()),
                        ui.visuals().text_color(),
                    );
                    for sc in app.keymap.bindings(a).to_vec() {
                        if ui.button(sc.label()).on_hover_text("Retirer ce raccourci").clicked() {
                            app.keymap.remove(a, &sc);
                            app.save_keymap();
                        }
                    }
                    if recording == Some(a) {
                        ui.label(ERich::new("Appuyez sur une touche…").color(accent()));
                    } else if ui.small_button("+").on_hover_text("Ajouter un raccourci").clicked() {
                        recording = Some(a);
                    }
                    if !app.keymap.is_default(a) && ui.small_button("↺").on_hover_text("Rétablir la valeur par défaut").clicked() {
                        app.keymap.reset(a);
                        app.save_keymap();
                    }
                });
            }
        });
    });
    open.then_some(Dialog::Shortcuts { filter, recording })
}

pub fn toast(app: &mut App, ctx: &egui::Context) {
    let Some((msg, t)) = app.toast.clone() else { return };
    if t.elapsed() > Duration::from_secs(4) {
        app.toast = None;
        return;
    }
    egui::Area::new(egui::Id::new("toast")).anchor(egui::Align2::CENTER_BOTTOM, vec2(0.0, -48.0)).order(egui::Order::Tooltip).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).fill(Color32::from_rgb(0x22, 0x22, 0x22)).show(ui, |ui| {
            ui.label(ERich::new(msg).color(Color32::WHITE));
        });
    });
    ctx.request_repaint_after(Duration::from_millis(300));
}
