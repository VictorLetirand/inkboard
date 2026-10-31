//! Interface theme: the colors, rounded corners, shadows and floating cards of the web prototype (light and dark).

use crate::icons;
use eframe::egui::{self, Align2, Color32, CornerRadius, FontFamily, FontId, Margin, Response, Sense, Shadow, Stroke, TextStyle, Ui, Vec2, vec2};

/// Color set (the prototype's CSS variables: `--ui`, `--ui2`, `--bd`, `--tx`, `--mut`, `--ac`, `--acbg`).
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

/// Palette of the context's current theme.
pub fn current(ctx: &egui::Context) -> Palette {
    palette(ctx.style().visuals.dark_mode)
}

/// Corner radius of cards and menus (`--r`).
const RADIUS: u8 = 10;

fn shadow(p: &Palette) -> Shadow {
    Shadow { offset: [0, 2], blur: if p.dark { 12 } else { 10 }, spread: 0, color: Color32::from_black_alpha(if p.dark { 140 } else { 36 }) }
}

/// Applies the theme to the whole context (call again when the light/dark background changes).
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

/// Floating card (`.bar`, `#toolbar`, `#panel`…): background, border, rounded corners and shadow.
pub fn card_frame(p: &Palette, margin: i8) -> egui::Frame {
    egui::Frame::new()
        .fill(p.ui)
        .stroke(Stroke::new(1.0_f32, p.bd))
        .corner_radius(CornerRadius::same(RADIUS))
        .shadow(shadow(p))
        .inner_margin(Margin::same(margin))
}

/// Places a floating card anchored in the window, above the canvas.
pub fn floating<R>(ctx: &egui::Context, id: &str, anchor: Align2, offset: Vec2, margin: i8, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = current(ctx);
    egui::Area::new(egui::Id::new(id))
        .anchor(anchor, offset)
        .order(egui::Order::Middle)
        .fade_in(false)
        .show(ctx, |ui| card_frame(&p, margin).show(ui, add).inner)
        .inner
}

/// 34 × 34 icon button (`.btn`): gray on hover, accent color when active, dimmed when disabled.
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

/// Small padlock badge in the bottom-right corner of an icon button: the tool is locked (stays active after drawing).
pub fn lock_badge(ui: &Ui, button: egui::Rect) {
    let p = current(ui.ctx());
    let c = button.right_bottom() - vec2(8.0, 8.0);
    ui.painter().circle(c, 7.0, p.ac, Stroke::new(1.0_f32, p.ui));
    icons::paint(ui.painter(), "lock", egui::Rect::from_center_size(c, vec2(9.0, 9.0)), p.ui);
}

/// Compact text button of the bottom bar (`.btn.txt`).
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

/// Vertical separator (`.sep`).
pub fn vsep(ui: &mut Ui) {
    let p = current(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(9.0, 22.0), Sense::hover());
    ui.painter().vline(r.center().x, r.y_range(), Stroke::new(1.0_f32, p.bd));
}

/// Menu style: contiguous rows (no vertical gap between two entries).
pub fn menu_style(ui: &mut Ui) {
    ui.spacing_mut().item_spacing.y = 0.0;
}

/// Menu row (`.menu .it`): icon, label, shortcut on the right or a check mark when `on`. Returns `true` on click. The label is translated.
pub fn menu_item(ui: &mut Ui, icon: Option<&str>, label: &str, key: &str, on: bool, enabled: bool) -> bool {
    let label = &crate::i18n::t(label);
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

/// Menu or panel section title (`.lbl`: small, dimmed, uppercase). The text is translated.
pub fn menu_label(ui: &mut Ui, text: &str) {
    menu_label_raw(ui, &crate::i18n::t(text));
}

/// Like [`menu_label`] for a text that is already translated (e.g. built with `tr!`).
pub fn menu_label_raw(ui: &mut Ui, text: &str) {
    let p = current(ui.ctx());
    ui.add_space(4.0);
    ui.label(egui::RichText::new(text.to_uppercase()).size(12.0).color(p.mute));
}

/// Menu separator (`.menu hr`).
pub fn menu_sep(ui: &mut Ui) {
    let p = current(ui.ctx());
    ui.add_space(3.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0_f32, p.bd));
    ui.add_space(3.0);
}

/// Panel setting label (`#panel label`: dimmed, 11 px). The text is translated.
pub fn field_label(ui: &mut Ui, text: &str) {
    field_label_raw(ui, &crate::i18n::t(text));
}

/// Like [`field_label`] for a text that is already translated (e.g. built with `tr!`).
pub fn field_label_raw(ui: &mut Ui, text: &str) {
    let p = current(ui.ctx());
    ui.label(egui::RichText::new(text).size(12.5).color(p.mute));
}

/// Segmented button (`.seg button`): 37 × 33, bordered, accent when active. `content` draws the inside (icon or text). The tooltip is translated.
pub fn seg_button(ui: &mut Ui, on: bool, tip: &str, content: impl FnOnce(&egui::Painter, egui::Rect, Color32)) -> Response {
    seg_button_raw(ui, on, &crate::i18n::t(tip), content)
}

/// Like [`seg_button`] for a tooltip that is already translated (e.g. built with `tr!`).
pub fn seg_button_raw(ui: &mut Ui, on: bool, tip: &str, content: impl FnOnce(&egui::Painter, egui::Rect, Color32)) -> Response {
    let p = current(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(37.0, 33.0), Sense::click());
    let fill = if on {
        p.acbg
    } else if resp.hovered() {
        p.ui2
    } else {
        p.ui
    };
    ui.painter().rect(r, 7.0, fill, Stroke::new(1.0_f32, if on { p.ac } else { p.bd }), egui::StrokeKind::Inside);
    content(ui.painter(), r, if on { p.ac } else { p.tx });
    if tip.is_empty() { resp } else { resp.on_hover_text(tip) }
}

/// 22 × 22 color swatch (`.sw .c`), accent outline when selected.
pub fn swatch_frame(ui: &mut Ui, selected: bool) -> (Response, egui::Rect) {
    let p = current(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::click());
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
