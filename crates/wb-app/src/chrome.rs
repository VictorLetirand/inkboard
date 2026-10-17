//! Cadre de l'application : cartes flottantes au-dessus du canevas plein écran, comme le prototype web
//! (barre du titre et menu en haut à gauche, outils en haut au centre, annuler/zoom en bas à gauche, aide en bas à droite).

use crate::app::{App, Dialog};
use crate::keymap::Action;
use crate::panel;
use crate::theme;
use crate::tools::Tool;
use eframe::egui::{self, Align2, Color32, Context, FontId, Sense, Stroke, Ui, vec2};
use std::time::{Duration, Instant};
use wb_core::editor::Order;
use wb_core::*;

/// Largeur de fenêtre sous laquelle la barre d'outils passe sous la barre du titre (`@media (max-width:1180px)`).
const NARROW: f32 = 1180.0;

/// Outils de la barre, par groupes séparés ; `(outil, icône, libellé)`.
const TOOL_GROUPS: &[&[(Tool, &str, &str)]] = &[
    &[(Tool::Select, "select", "Sélection"), (Tool::Hand, "hand", "Main")],
    &[(Tool::Pen, "pen", "Crayon"), (Tool::Highlighter, "highlighter", "Surligneur"), (Tool::Eraser, "eraser", "Gomme")],
    &[
        (Tool::Shape(BoxGeom::Rect), "rect", "Rectangle"),
        (Tool::Shape(BoxGeom::Ellipse), "ellipse", "Ellipse"),
        (Tool::Shape(BoxGeom::Diamond), "diamond", "Losange"),
        (Tool::Shape(BoxGeom::Triangle), "triangle", "Triangle"),
    ],
    &[(Tool::Line, "line", "Ligne — s'accroche aux formes"), (Tool::Arrow, "arrow", "Flèche — s'accroche aux formes")],
    &[(Tool::Text, "text", "Texte"), (Tool::Sticky, "sticky", "Note adhésive"), (Tool::Table, "table", "Tableau"), (Tool::Vote, "vote", "Pastille de vote")],
];

/// Affiche tout le cadre ; à appeler avant le canevas dans chaque image.
pub fn show(app: &mut App, ctx: &Context) {
    let narrow = ctx.content_rect().width() < NARROW;
    title_card(app, ctx);
    tool_card(app, ctx, narrow);
    panel::show(app, ctx, narrow);
    pages_card(app, ctx);
    zoom_card(app, ctx);
    help_card(app, ctx);
}

/* ---------------------------------------------------------------- titre */

fn title_card(app: &mut App, ctx: &Context) {
    theme::floating(ctx, "title", Align2::LEFT_TOP, vec2(12.0, 12.0), 4, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let menu = theme::icon_button(ui, "menu", false, true).on_hover_text("Menu");
            app.menu_popup = Some(egui::Popup::default_response_id(&menu));
            egui::Popup::from_toggle_button_response(&menu)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .width(250.0)
                .show(|ui| main_menu(app, ui));
            board_name(app, ui);
            let (text, p) = (save_state(app), theme::current(ui.ctx()));
            ui.label(egui::RichText::new(text).size(11.0).color(p.mute));
            ui.add_space(6.0);
        });
    });
}

/// Nom du tableau modifiable sur place ; il n'est enregistré (et ne crée une étape d'annulation) qu'à la validation.
fn board_name(app: &mut App, ui: &mut Ui) {
    let p = theme::current(ui.ctx());
    let id = egui::Id::new("board-name");
    let editing = ui.memory(|m| m.has_focus(id));
    let mut buf: String = if editing { ui.data(|d| d.get_temp(id)).unwrap_or_else(|| app.ed.doc.name.clone()) } else { app.ed.doc.name.clone() };
    let hover_id = id.with("hover");
    let hovered: bool = ui.data(|d| d.get_temp(hover_id)).unwrap_or(false);
    let fill = if editing || hovered { p.ui2 } else { Color32::TRANSPARENT };
    let resp = egui::Frame::new().fill(fill).corner_radius(6.0).show(ui, |ui| {
        ui.add(
            egui::TextEdit::singleline(&mut buf)
                .id(id)
                .frame(false)
                .desired_width(150.0)
                .margin(vec2(6.0, 6.0))
                .font(FontId::proportional(14.0))
                .char_limit(80),
        )
    });
    ui.data_mut(|d| d.insert_temp(hover_id, resp.response.hovered()));
    let r = resp.inner;
    if r.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, buf.clone()));
    }
    if r.lost_focus() {
        let name = buf.trim();
        if !name.is_empty() && name != app.ed.doc.name {
            app.ed.set_name(name);
        }
        ui.data_mut(|d| d.remove::<String>(id));
    }
}

fn save_state(app: &App) -> String {
    match (app.dirty(), app.path.is_some()) {
        (true, _) => "Modifications…".to_owned(),
        (false, true) => "Enregistré".to_owned(),
        (false, false) => String::new(),
    }
}

/* -------------------------------------------------------------- outils */

fn tool_card(app: &mut App, ctx: &Context, narrow: bool) {
    let (anchor, offset) = if narrow { (Align2::LEFT_TOP, vec2(12.0, 62.0)) } else { (Align2::CENTER_TOP, vec2(0.0, 12.0)) };
    theme::floating(ctx, "tools", anchor, offset, 4, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            for (gi, group) in TOOL_GROUPS.iter().enumerate() {
                if gi > 0 {
                    theme::vsep(ui);
                }
                for (tool, icon, label) in *group {
                    let key = Action::for_tool(*tool).map(|a| app.keymap.label(a)).unwrap_or_default();
                    let tip = if key.is_empty() { (*label).to_owned() } else { format!("{label} ({key})") };
                    if theme::icon_button(ui, icon, app.tool == *tool, true).on_hover_text(tip).clicked() {
                        app.set_tool(*tool);
                    }
                }
            }
            if theme::icon_button(ui, "image", false, true).on_hover_text("Insérer une image").clicked() {
                app.insert_images_dialog();
            }
            let key = Action::for_tool(Tool::Laser).map(|a| app.keymap.label(a)).unwrap_or_default();
            if theme::icon_button(ui, "laser", app.tool == Tool::Laser, true).on_hover_text(format!("Pointeur laser ({key})")).clicked() {
                app.set_tool(Tool::Laser);
            }
            theme::vsep(ui);
            let tip = format!("Garder l'outil actif ({})", app.keymap.label(Action::ToolLock));
            if theme::icon_button(ui, if app.tool_lock { "lock" } else { "unlock" }, app.tool_lock, true).on_hover_text(tip).clicked() {
                app.tool_lock = !app.tool_lock;
            }
        });
    });
}

/* ---------------------------------------------------------- barre du bas */

fn zoom_card(app: &mut App, ctx: &Context) {
    theme::floating(ctx, "zoom", Align2::LEFT_BOTTOM, vec2(12.0, -12.0), 3, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            if theme::icon_button(ui, "undo", false, app.ed.can_undo()).on_hover_text(format!("Annuler ({})", app.keymap.label(Action::Undo))).clicked() {
                app.ed.undo();
            }
            if theme::icon_button(ui, "redo", false, app.ed.can_redo()).on_hover_text(format!("Rétablir ({})", app.keymap.label(Action::Redo))).clicked() {
                app.ed.redo();
            }
            theme::vsep(ui);
            if theme::icon_button(ui, "minus", false, true).on_hover_text("Zoom arrière").clicked() {
                app.zoom_by(0.8);
            }
            let v = app.ed.page().view;
            if theme::text_button(ui, &format!("{:.0} %", v.zoom * 100.0))
                .on_hover_text(format!("Revenir à 100 % ({})", app.keymap.label(Action::Zoom100)))
                .clicked()
            {
                app.set_view(View { zoom: 1.0, ..v });
            }
            if theme::icon_button(ui, "plus", false, true).on_hover_text("Zoom avant").clicked() {
                app.zoom_by(1.25);
            }
            if theme::icon_button(ui, "fit", false, true).on_hover_text(format!("Tout afficher ({})", app.keymap.label(Action::ZoomFit))).clicked() {
                app.fit_view(false);
            }
            if let Some((text, over)) = crate::ui::timer_text(app) {
                theme::vsep(ui);
                let p = theme::current(ui.ctx());
                ui.label(egui::RichText::new(format!("⏱ {text}")).color(if over { Color32::from_rgb(0xe0, 0x31, 0x31) } else { p.tx }));
                ui.add_space(6.0);
                ui.ctx().request_repaint_after(Duration::from_millis(500));
            }
        });
    });
}

fn help_card(app: &mut App, ctx: &Context) {
    theme::floating(ctx, "help", Align2::RIGHT_BOTTOM, vec2(-12.0, -12.0), 3, |ui| {
        if theme::icon_button(ui, "help", false, true).on_hover_text("Raccourcis clavier").clicked() {
            app.dialog = Some(Dialog::Shortcuts { filter: String::new(), recording: None });
        }
    });
}

fn pages_card(app: &mut App, ctx: &Context) {
    theme::floating(ctx, "pages", Align2::CENTER_BOTTOM, vec2(0.0, -12.0), 3, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let pages: Vec<(Id, String)> = app.ed.doc.pages.iter().map(|p| (p.id.clone(), p.name.clone())).collect();
            for (id, name) in &pages {
                let on = &app.ed.current == id;
                let p = theme::current(ui.ctx());
                let g = ui.painter().layout_no_wrap(name.clone(), FontId::proportional(13.0), if on { p.ac } else { p.tx });
                let (r, resp) = ui.allocate_exact_size(vec2((g.size().x + 20.0).max(48.0), 34.0), Sense::click());
                if on {
                    ui.painter().rect_filled(r, 8.0, p.acbg);
                } else if resp.hovered() {
                    ui.painter().rect_filled(r, 8.0, p.ui2);
                }
                ui.painter().galley(r.center() - g.size() / 2.0, g, p.tx);
                if resp.clicked() {
                    app.ed.current = id.clone();
                    app.sel.clear();
                    app.editing = None;
                }
                resp.context_menu(|ui| {
                    theme::menu_style(ui);
                    if theme::menu_item(ui, Some("edit"), "Renommer…", "", false, true) {
                        app.dialog = Some(Dialog::RenamePage { id: id.clone(), name: name.clone() });
                        ui.close();
                    }
                    if theme::menu_item(ui, Some("trash"), "Supprimer la page", "", false, pages.len() > 1) {
                        app.ed.remove_page(id);
                        app.sel.clear();
                        ui.close();
                    }
                });
            }
            if theme::icon_button(ui, "plus", false, true).on_hover_text("Nouvelle page").clicked() {
                let n = app.ed.doc.pages.len() + 1;
                app.ed.add_page(&format!("Page {n}"));
                app.sel.clear();
                app.fit_pending = true;
            }
        });
    });
}

/* ------------------------------------------------------------ menu principal */

fn mi(ui: &mut Ui, icon: &str, label: &str, key: &str) -> bool {
    let clicked = theme::menu_item(ui, Some(icon), label, key, false, true);
    if clicked {
        ui.close();
    }
    clicked
}

fn key(app: &App, a: Action) -> String {
    app.keymap.label(a)
}

fn main_menu(app: &mut App, ui: &mut Ui) {
    let p = theme::current(ui.ctx());
    let max_h = ui.ctx().content_rect().height() * 0.8;
    ui.set_max_height(max_h);
    egui::ScrollArea::vertical().max_height(max_h).show(ui, |ui| {
        theme::menu_style(ui);
        ui.set_min_width(238.0);
        if mi(ui, "filenew", "Nouveau tableau", &key(app, Action::New)) {
            app.new_document();
        }
        if mi(ui, "folder", "Ouvrir / importer un fichier…", &key(app, Action::Open)) {
            app.open_dialog();
        }
        if mi(ui, "mermaid", "Importer du Mermaid…", "") {
            app.dialog = Some(Dialog::Mermaid {
                text: "flowchart TD\n  A[Idée] --> B{Faisable ?}\n  B -->|Oui| C[Prototype]\n  B -->|Non| D[Abandon]".into(),
                new_board: false,
                error: None,
            });
        }
        if mi(ui, "save", "Enregistrer", &key(app, Action::Save)) {
            app.save();
        }
        if mi(ui, "save", "Enregistrer sous…", &key(app, Action::SaveAs)) {
            app.save_as();
        }
        theme::menu_sep(ui);
        let sel = !app.sel.is_empty();
        if mi(ui, "download", if sel { "Exporter la sélection en PNG" } else { "Exporter en PNG" }, "") {
            app.export("png");
        }
        if mi(ui, "download", "Exporter en SVG", "") {
            app.export("svg");
        }
        if mi(ui, "download", "Exporter en PDF", "") {
            app.export("pdf");
        }
        if mi(ui, "copy", "Copier l'image dans le presse-papiers", "") {
            app.copy_as_image();
        }
        theme::menu_label(ui, "Échange avec d'autres outils");
        if mi(ui, "download", "Exporter en draw.io (.drawio)", "") {
            app.export("drawio");
        }
        if mi(ui, "download", "Exporter en Excalidraw", "") {
            app.export("excalidraw");
        }
        if mi(ui, "download", "Exporter en JSON Canvas (.canvas)", "") {
            app.export("canvas");
        }
        theme::menu_sep(ui);
        theme::menu_label(ui, "Insérer");
        if mi(ui, "image", "Image…", "") {
            app.insert_images_dialog();
        }
        if mi(ui, "code", "Bloc de code", "") {
            app.insert_code_block();
        }
        if mi(ui, "formula", "Formule (LaTeX)…", "") {
            app.dialog =
                Some(Dialog::Formula { src: r"E = mc^2, \quad \sum_{i=1}^{n} x_i \leq \frac{a+b}{2}".into(), editing: None, preview: Default::default() });
        }
        if mi(ui, "vote", "Pastille de vote", "") {
            app.set_tool(Tool::Vote);
        }
        theme::menu_sep(ui);
        theme::menu_label(ui, "Édition");
        if theme::menu_item(ui, Some("undo"), "Annuler", &key(app, Action::Undo), false, app.ed.can_undo()) {
            app.ed.undo();
            ui.close();
        }
        if theme::menu_item(ui, Some("redo"), "Rétablir", &key(app, Action::Redo), false, app.ed.can_redo()) {
            app.ed.redo();
            ui.close();
        }
        if mi(ui, "all", "Tout sélectionner", &key(app, Action::SelectAll)) {
            app.select_all();
        }
        if mi(ui, "layers", "Calques…", "") {
            app.layers_open = !app.layers_open;
        }
        theme::menu_sep(ui);
        theme::menu_label(ui, "Arrière-plan");
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.spacing_mut().item_spacing.x = 6.0;
            for preset in BgPreset::ALL {
                let on = app.ed.doc.background.preset == preset;
                let (r, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
                let (cr, cg, cb) = preset.rgb();
                ui.painter().circle_filled(r.center(), 11.0, Color32::from_rgb(cr, cg, cb));
                ui.painter().circle_stroke(r.center(), 12.0, Stroke::new(2.0_f32, if on { p.ac } else { p.bd }));
                if on {
                    ui.painter().circle_stroke(r.center(), 14.0, Stroke::new(2.0_f32, p.acbg));
                }
                if resp.on_hover_text(preset.label()).clicked() {
                    let mut bg = app.ed.doc.background;
                    bg.preset = preset;
                    app.ed.set_background(bg);
                }
            }
        });
        ui.add_space(4.0);
        for (label, pat) in [("Aucun motif", Pattern::None), ("Points", Pattern::Dots), ("Quadrillage", Pattern::Grid), ("Lignes", Pattern::Lines)] {
            if theme::menu_item(ui, Some("grid"), label, "", app.ed.doc.background.pattern == pat, true) {
                let mut bg = app.ed.doc.background;
                bg.pattern = pat;
                app.ed.set_background(bg);
            }
        }
        if theme::menu_item(ui, Some("grid"), "Magnétisme de la grille", "", app.snap, true) {
            app.snap = !app.snap;
        }
        theme::menu_sep(ui);
        theme::menu_label(ui, "Affichage");
        if mi(ui, "play", "Présenter en plein écran", &key(app, Action::Present)) {
            app.present_req = Some(true);
        }
        if mi(ui, "fit", "Tout afficher", &key(app, Action::ZoomFit)) {
            app.fit_view(false);
        }
        if mi(ui, "full", "Zoom 100 %", &key(app, Action::Zoom100)) {
            let v = app.ed.page().view;
            app.set_view(View { zoom: 1.0, ..v });
        }
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            let p = theme::current(ui.ctx());
            ui.label(egui::RichText::new("Minuteur").size(13.0).color(p.tx));
        });
        ui.horizontal_wrapped(|ui| {
            ui.add_space(6.0);
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
        theme::menu_sep(ui);
        if mi(ui, "keyboard", "Raccourcis clavier…", "") {
            app.dialog = Some(Dialog::Shortcuts { filter: String::new(), recording: None });
        }
        if mi(ui, "trashAll", "Effacer tout le tableau", "") {
            let ids: Vec<Id> = app.ed.page().objects.iter().map(|o| o.id.clone()).collect();
            app.ed.remove(&ids);
            app.sel.clear();
        }
        if mi(ui, "full", "Quitter", "") {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    });
}

/* ----------------------------------------------------- menus contextuels */

/// Menu du clic droit sur le fond du canevas.
pub fn canvas_menu(app: &mut App, ui: &mut Ui) {
    theme::menu_style(ui);
    if mi(ui, "paste", "Coller", "Ctrl+V") {
        app.paste();
    }
    if mi(ui, "all", "Tout sélectionner", &key(app, Action::SelectAll)) {
        app.select_all();
    }
    if mi(ui, "fit", "Tout afficher", &key(app, Action::ZoomFit)) {
        app.fit_view(false);
    }
}

/// Menu du clic droit sur la sélection.
pub fn selection_menu(app: &mut App, ui: &mut Ui) {
    theme::menu_style(ui);
    let sel = app.sel.clone();
    if mi(ui, "cut", "Couper", "Ctrl+X") {
        app.copy_selection();
        app.delete_selection();
    }
    if mi(ui, "copy", "Copier", "Ctrl+C") {
        app.copy_selection();
    }
    if mi(ui, "image", "Copier comme image", "") {
        app.copy_as_image();
    }
    if mi(ui, "paste", "Coller", "Ctrl+V") {
        app.paste();
    }
    if mi(ui, "copy", "Dupliquer", &key(app, Action::Duplicate)) {
        app.sel = app.ed.duplicate(&sel, Pt::new(24.0, 24.0));
    }
    if let [id] = sel.as_slice() {
        let kind = app.ed.page().get(id).map(|o| o.kind.clone());
        if matches!(kind, Some(Kind::Box(_) | Kind::Table(_) | Kind::Connector(_))) && mi(ui, "edit", "Modifier le texte", &key(app, Action::EditText)) {
            let cell = match &kind {
                Some(Kind::Table(t)) => t.cell_at(app.ctx_pos),
                _ => None,
            };
            app.start_editing(id, cell);
        }
        if matches!(&kind, Some(Kind::Image(i)) if i.latex.is_some()) && mi(ui, "formula", "Modifier la formule…", "") {
            app.edit_formula(id);
        }
        if matches!(&kind, Some(Kind::Box(b)) if b.label.family == FontFamily::Mono) && mi(ui, "code", "Colorer le code", "") {
            app.recolor_code(id);
        }
        if matches!(&kind, Some(Kind::Connector(c)) if !c.via.is_empty()) && mi(ui, "line", "Redresser (retirer les points de passage)", "") {
            app.ed.update(id, |o| {
                if let Kind::Connector(c) = &mut o.kind {
                    c.via.clear();
                }
            });
        }
    }
    table_menu(app, ui, &sel);
    theme::menu_sep(ui);
    for (icon, label, o) in [
        ("front", "Premier plan", Order::Front),
        ("forward", "Avancer", Order::Forward),
        ("backward", "Reculer", Order::Backward),
        ("back", "Arrière-plan", Order::Back),
    ] {
        if mi(ui, icon, label, "") {
            app.ed.reorder(&sel, o);
        }
    }
    theme::menu_sep(ui);
    if sel.len() > 1 && mi(ui, "group", "Grouper", &key(app, Action::Group)) {
        app.ed.group(&sel);
    }
    if sel.iter().any(|i| app.ed.page().get(i).is_some_and(|o| o.group.is_some())) && mi(ui, "ungroup", "Dégrouper", &key(app, Action::Ungroup)) {
        app.ed.ungroup(&sel);
    }
    let all_locked = sel.iter().all(|i| app.ed.page().get(i).is_some_and(|o| o.locked));
    if mi(ui, if all_locked { "unlock" } else { "lock" }, if all_locked { "Déverrouiller" } else { "Verrouiller" }, "") {
        app.ed.set_locked(&sel, !all_locked);
    }
    theme::menu_sep(ui);
    if mi(ui, "trash", "Supprimer", &key(app, Action::Delete)) {
        app.delete_selection();
    }
}

/// Opérations sur la ligne / colonne située sous le clic droit, si un tableau est sélectionné.
fn table_menu(app: &mut App, ui: &mut Ui, sel: &[Id]) {
    let [id] = sel else { return };
    let Some((r, c)) = app.ed.page().get(id).and_then(|o| if let Kind::Table(t) = &o.kind { t.cell_at(app.ctx_pos) } else { None }) else { return };
    theme::menu_sep(ui);
    theme::menu_label(ui, &format!("Cellule ligne {} · colonne {}", r + 1, c + 1));
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
        if mi(ui, "table", label, "") {
            app.ed.update(id, |o| {
                if let Kind::Table(t) = &mut o.kind {
                    op(t, r, c);
                }
            });
        }
    }
}
