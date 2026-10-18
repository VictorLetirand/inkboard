//! Thème de l'interface : les couleurs, arrondis, ombres et cartes flottantes du prototype web (clair et sombre).

use crate::icons;
use eframe::egui::{self, Align2, Color32, CornerRadius, FontFamily, FontId, Margin, Response, Sense, Shadow, Stroke, TextStyle, Ui, Vec2, vec2};

/// Jeu de couleurs (variables CSS du prototype : `--ui`, `--ui2`, `--bd`, `--tx`, `--mut`, `--ac`, `--acbg`).
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub ui: Color32,
    pub ui2: Color32,
    pub bd: Color32,
    pub tx: Color32,
    pub mute: Color32,
    pub ac: Color32,
    pub acbg: Color32,
    pub dark: bool,
}

pub fn palette(dark: bool) -> Palette {
    let c = |r, g, b| Color32::from_rgb(r, g, b);
    if dark {
        Palette {
            ui: c(0x2a, 0x2d, 0x37),
            ui2: c(0x38, 0x3c, 0x4a),
            bd: c(0x47, 0x4c, 0x5c),
            tx: c(0xec, 0xee, 0xf3),
            mute: c(0x9a, 0xa1, 0xb2),
            ac: c(0x8a, 0x9b, 0xff),
            acbg: c(0x3b, 0x42, 0x72),
            dark,
        }
    } else {
        Palette {
            ui: Color32::WHITE,
            ui2: c(0xf1, 0xf3, 0xf5),
            bd: c(0xdd, 0xe1, 0xe6),
            tx: c(0x1b, 0x1f, 0x23),
            mute: c(0x6b, 0x72, 0x80),
            ac: c(0x4f, 0x6d, 0xf5),
            acbg: c(0xe3, 0xe8, 0xff),
            dark,
        }
    }
}

/// Palette du thème courant du contexte.
pub fn current(ctx: &egui::Context) -> Palette {
    palette(ctx.style().visuals.dark_mode)
}

/// Rayon des cartes et des menus (`--r`).
const RADIUS: u8 = 10;

fn shadow(p: &Palette) -> Shadow {
    Shadow { offset: [0, 2], blur: if p.dark { 12 } else { 10 }, spread: 0, color: Color32::from_black_alpha(if p.dark { 140 } else { 36 }) }
}

/// Applique le thème à tout le contexte (à rappeler quand le fond clair/sombre change).
pub fn apply(ctx: &egui::Context, dark: bool) {
    let p = palette(dark);
    let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.dark_mode = dark;
    v.panel_fill = p.ui;
    v.window_fill = p.ui;
    v.window_stroke = Stroke::new(1.0_f32, p.bd);
    v.window_corner_radius = CornerRadius::same(RADIUS);
    v.menu_corner_radius = CornerRadius::same(RADIUS);
    v.window_shadow = shadow(&p);
    v.popup_shadow = shadow(&p);
    v.extreme_bg_color = p.ui;
    v.faint_bg_color = p.ui2;
    v.code_bg_color = p.ui2;
    v.hyperlink_color = p.ac;
    v.selection.bg_fill = p.acbg;
    v.selection.stroke = Stroke::new(1.0_f32, p.ac);
    let r = CornerRadius::same(8);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.ui;
    w.noninteractive.weak_bg_fill = p.ui;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.bd);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.tx);
    w.noninteractive.corner_radius = r;
    for (wv, fill) in [(&mut w.inactive, p.ui), (&mut w.hovered, p.ui2), (&mut w.active, p.acbg), (&mut w.open, p.ui2)] {
        wv.weak_bg_fill = fill;
        wv.bg_fill = if fill == p.ui { p.ui2 } else { fill };
        wv.bg_stroke = Stroke::new(1.0_f32, p.bd);
        wv.fg_stroke = Stroke::new(1.0_f32, p.tx);
        wv.corner_radius = r;
        wv.expansion = 0.0;
    }
    w.hovered.bg_stroke = Stroke::new(1.0_f32, p.bd);
    w.active.bg_stroke = Stroke::new(1.0_f32, p.ac);
    w.active.fg_stroke = Stroke::new(1.0_f32, p.ac);
    ctx.set_visuals(v);

    ctx.style_mut(|s| {
        s.spacing.item_spacing = vec2(6.0, 6.0);
        s.spacing.button_padding = vec2(12.0, 5.0);
        s.spacing.interact_size = vec2(34.0, 30.0);
        s.spacing.menu_margin = Margin::same(6);
        s.spacing.window_margin = Margin::same(14);
        s.spacing.slider_width = 150.0;
        s.text_styles = [
            (TextStyle::Small, FontId::new(11.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(13.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(13.0, FontFamily::Proportional)),
            (TextStyle::Heading, FontId::new(17.0, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(12.5, FontFamily::Monospace)),
        ]
        .into();
    });
}

/// Carte flottante (`.bar`, `#toolbar`, `#panel`…) : fond, bordure, coins arrondis et ombre.
pub fn card_frame(p: &Palette, margin: i8) -> egui::Frame {
    egui::Frame::new()
        .fill(p.ui)
        .stroke(Stroke::new(1.0_f32, p.bd))
        .corner_radius(CornerRadius::same(RADIUS))
        .shadow(shadow(p))
        .inner_margin(Margin::same(margin))
}

/// Pose une carte flottante ancrée dans la fenêtre, au-dessus du canevas.
pub fn floating<R>(ctx: &egui::Context, id: &str, anchor: Align2, offset: Vec2, margin: i8, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = current(ctx);
    egui::Area::new(egui::Id::new(id))
        .anchor(anchor, offset)
        .order(egui::Order::Middle)
        .fade_in(false)
        .show(ctx, |ui| card_frame(&p, margin).show(ui, add).inner)
        .inner
}

/// Bouton à icône de 34 × 34 (`.btn`) : survol gris, actif en couleur d'accent, grisé si désactivé.
pub fn icon_button(ui: &mut Ui, name: &str, active: bool, enabled: bool) -> Response {
    let p = current(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(34.0, 34.0), if enabled { Sense::click() } else { Sense::hover() });
    if active {
        ui.painter().rect_filled(r, 8.0, p.acbg);
    } else if resp.hovered() && enabled {
        ui.painter().rect_filled(r, 8.0, p.ui2);
    }
    let color = if active { p.ac } else { p.tx };
    icons::paint(ui.painter(), name, r.shrink(7.0), if enabled { color } else { color.gamma_multiply(0.35) });
    resp
}

/// Bouton texte compact de la barre du bas (`.btn.txt`).
pub fn text_button(ui: &mut Ui, text: &str) -> Response {
    let p = current(ui.ctx());
    let galley = ui.painter().layout_no_wrap(text.to_owned(), FontId::new(13.0, FontFamily::Proportional), p.tx);
    let (r, resp) = ui.allocate_exact_size(vec2((galley.size().x + 12.0).max(52.0), 34.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, 8.0, p.ui2);
    }
    ui.painter().galley(r.center() - galley.size() / 2.0, galley, p.tx);
    resp
}

/// Séparateur vertical (`.sep`).
pub fn vsep(ui: &mut Ui) {
    let p = current(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(9.0, 22.0), Sense::hover());
    ui.painter().vline(r.center().x, r.y_range(), Stroke::new(1.0_f32, p.bd));
}

/// Style des menus : lignes contiguës (pas d'espace vertical entre deux entrées).
pub fn menu_style(ui: &mut Ui) {
    ui.spacing_mut().item_spacing.y = 0.0;
}

/// Ligne de menu (`.menu .it`) : icône, libellé, raccourci à droite ou coche si `on`. Renvoie `true` au clic.
pub fn menu_item(ui: &mut Ui, icon: Option<&str>, label: &str, key: &str, on: bool, enabled: bool) -> bool {
    let p = current(ui.ctx());
    let font = FontId::new(13.0, FontFamily::Proportional);
    let small = FontId::new(11.0, FontFamily::Proportional);
    let label_g = ui.painter().layout_no_wrap(label.to_owned(), font, if enabled { p.tx } else { p.tx.gamma_multiply(0.4) });
    let key_g = (!key.is_empty()).then(|| ui.painter().layout_no_wrap(key.to_owned(), small.clone(), p.mute));
    let w = (10.0 + 17.0 + 10.0 + label_g.size().x + 22.0 + key_g.as_ref().map_or(0.0, |g| g.size().x) + 16.0 + 10.0).max(230.0);
    let w = w.max(ui.available_width());
    let (r, resp) = ui.allocate_exact_size(vec2(w, 32.0), if enabled { Sense::click() } else { Sense::hover() });
    if resp.hovered() && enabled {
        ui.painter().rect_filled(r, 6.0, p.ui2);
    }
    let icon_c = if enabled { p.tx } else { p.tx.gamma_multiply(0.4) };
    if let Some(i) = icon {
        icons::paint(ui.painter(), i, egui::Rect::from_center_size(egui::pos2(r.left() + 10.0 + 8.5, r.center().y), vec2(17.0, 17.0)), icon_c);
    }
    ui.painter().galley(egui::pos2(r.left() + 10.0 + 17.0 + 10.0, r.center().y - label_g.size().y / 2.0), label_g, p.tx);
    if on {
        icons::paint(ui.painter(), "check", egui::Rect::from_center_size(egui::pos2(r.right() - 18.0, r.center().y), vec2(15.0, 15.0)), p.ac);
    } else if let Some(g) = key_g {
        let pos = egui::pos2(r.right() - 10.0 - g.size().x, r.center().y - g.size().y / 2.0);
        ui.painter().galley(pos, g, p.mute);
    }
    resp.clicked()
}

/// Titre de section de menu ou de panneau (`.lbl` : petit, grisé, majuscules).
pub fn menu_label(ui: &mut Ui, text: &str) {
    let p = current(ui.ctx());
    ui.add_space(4.0);
    ui.label(egui::RichText::new(text.to_uppercase()).size(11.0).color(p.mute));
}

/// Séparateur de menu (`.menu hr`).
pub fn menu_sep(ui: &mut Ui) {
    let p = current(ui.ctx());
    ui.add_space(3.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0_f32, p.bd));
    ui.add_space(3.0);
}

/// Étiquette de réglage du panneau (`#panel label` : 11 px grisé).
pub fn field_label(ui: &mut Ui, text: &str) {
    let p = current(ui.ctx());
    ui.label(egui::RichText::new(text).size(11.0).color(p.mute));
}

/// Bouton segmenté (`.seg button`) : 34 × 30, bordé, actif en accent. `content` dessine l'intérieur (icône ou texte).
pub fn seg_button(ui: &mut Ui, on: bool, tip: &str, content: impl FnOnce(&egui::Painter, egui::Rect, Color32)) -> Response {
    let p = current(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(34.0, 30.0), Sense::click());
    let fill = if on {
        p.acbg
    } else if resp.hovered() {
        p.ui2
    } else {
        p.ui
    };
    ui.painter().rect(r, 7.0, fill, Stroke::new(1.0_f32, if on { p.ac } else { p.bd }), egui::StrokeKind::Inside);
    content(ui.painter(), r, if on { p.ac } else { p.tx });
    resp.on_hover_text(tip)
}

/// Pastille de couleur (`.sw .c`) de 22 × 22, contour d'accent si sélectionnée.
pub fn swatch_frame(ui: &mut Ui, selected: bool) -> (Response, egui::Rect) {
    let p = current(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
    if selected {
        ui.painter().rect_stroke(r.expand(2.0), 7.0, Stroke::new(2.0_f32, p.ac), egui::StrokeKind::Middle);
    }
    (resp, r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palettes_match_the_prototype_tokens() {
        let l = palette(false);
        assert_eq!((l.ac, l.acbg, l.ui2), (Color32::from_rgb(0x4f, 0x6d, 0xf5), Color32::from_rgb(0xe3, 0xe8, 0xff), Color32::from_rgb(0xf1, 0xf3, 0xf5)));
        let d = palette(true);
        assert_eq!((d.ui, d.ac, d.tx), (Color32::from_rgb(0x2a, 0x2d, 0x37), Color32::from_rgb(0x8a, 0x9b, 0xff), Color32::from_rgb(0xec, 0xee, 0xf3)));
    }

    #[test]
    fn apply_switches_visuals_and_text_sizes() {
        let ctx = egui::Context::default();
        apply(&ctx, true);
        assert!(current(&ctx).dark);
        apply(&ctx, false);
        assert!(!current(&ctx).dark);
        assert_eq!(ctx.style().text_styles[&TextStyle::Body].size, 13.0);
        assert_eq!(ctx.style().visuals.window_corner_radius, CornerRadius::same(10));
    }
}
