//! Dessin d'une page avec egui : même logique que l'export SVG du moteur.

use crate::fonts::{EguiMeasurer, font_id};
use eframe::egui::epaint::{Mesh, TextShape, Vertex};
use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{self, Color32, Painter, Pos2, Shape, Stroke, TextureHandle, pos2, vec2};
use std::collections::HashMap;
use wb_core::connector::{head_points, path_midpoint};
use wb_core::geom::{Frame, Pt, Rect};
use wb_core::model::Outline;
use wb_core::text::{TextLayout, layout_rich};
use wb_core::*;

pub const GRID: f32 = 24.0;

/// Caméra : position écran de l'origine du monde et zoom.
#[derive(Clone, Copy, Debug)]
pub struct Cam {
    pub origin: Pos2,
    pub zoom: f32,
}

impl Cam {
    pub fn s(&self, p: Pt) -> Pos2 {
        pos2(self.origin.x + p.x as f32 * self.zoom, self.origin.y + p.y as f32 * self.zoom)
    }
    pub fn w(&self, p: Pos2) -> Pt {
        Pt::new(f64::from((p.x - self.origin.x) / self.zoom), f64::from((p.y - self.origin.y) / self.zoom))
    }
    pub fn rect_s(&self, r: Rect) -> egui::Rect {
        egui::Rect::from_min_max(self.s(Pt::new(r.x, r.y)), self.s(Pt::new(r.right(), r.bottom())))
    }
}

pub fn col(c: Color, dark: bool, opacity: f32) -> Color32 {
    let (r, g, b) = c.resolve(dark);
    Color32::from_rgba_unmultiplied(r, g, b, (opacity.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// Textures des images du document, décodées à la demande.
#[derive(Default)]
pub struct TextureCache {
    map: HashMap<Id, Option<TextureHandle>>,
}

impl TextureCache {
    pub fn get(&mut self, ctx: &egui::Context, id: &Id, asset: &Asset) -> Option<&TextureHandle> {
        self.map
            .entry(id.clone())
            .or_insert_with(|| {
                let img = image::load_from_memory(&asset.data).ok()?.to_rgba8();
                let size = [img.width() as usize, img.height() as usize];
                Some(ctx.load_texture(id.as_str(), egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()), egui::TextureOptions::LINEAR))
            })
            .as_ref()
    }
    pub fn clear(&mut self) {
        self.map.clear();
    }
}

pub struct Renderer<'a> {
    pub painter: &'a Painter,
    pub cam: Cam,
    pub dark: bool,
    pub doc: &'a Document,
    pub page: &'a Page,
    pub measurer: &'a EguiMeasurer,
    pub textures: &'a mut TextureCache,
    pub bg: Color32,
    /// Table identifiant → position (en cache dans l'application) ; calculée à la volée si absente.
    pub ids: Option<&'a HashMap<Id, usize>>,
    /// Index spatial à jour (absent pendant les gestes qui déplacent des objets).
    pub index: Option<&'a wb_core::spatial::SpatialIndex>,
}

impl Renderer<'_> {
    pub fn background(&self, rect: egui::Rect) {
        self.painter.rect_filled(rect, 0.0, self.bg);
        let pattern = self.doc.background.pattern;
        if pattern == Pattern::None {
            return;
        }
        let mut step = GRID * self.cam.zoom;
        while step < 14.0 {
            step *= 2.0;
        }
        let ox = (self.cam.origin.x - rect.min.x).rem_euclid(step);
        let oy = (self.cam.origin.y - rect.min.y).rem_euclid(step);
        let base = if self.dark { 255 } else { 0 };
        match pattern {
            Pattern::Dots => {
                let c = Color32::from_rgba_unmultiplied(base, base, base, 50);
                let mut x = rect.min.x + ox;
                while x < rect.max.x {
                    let mut y = rect.min.y + oy;
                    while y < rect.max.y {
                        self.painter.rect_filled(egui::Rect::from_center_size(pos2(x, y), vec2(2.0, 2.0)), 0.0, c);
                        y += step;
                    }
                    x += step;
                }
            }
            Pattern::Grid | Pattern::Lines => {
                let st = Stroke::new(1.0, Color32::from_rgba_unmultiplied(base, base, base, 24));
                if pattern == Pattern::Grid {
                    let mut x = rect.min.x + ox;
                    while x < rect.max.x {
                        self.painter.line_segment([pos2(x.round(), rect.min.y), pos2(x.round(), rect.max.y)], st);
                        x += step;
                    }
                }
                let mut y = rect.min.y + oy;
                while y < rect.max.y {
                    self.painter.line_segment([pos2(rect.min.x, y.round()), pos2(rect.max.x, y.round())], st);
                    y += step;
                }
            }
            Pattern::None => {}
        }
    }

    /// Dessine les objets visibles ; `skip` : identifiants à ne pas dessiner (texte en cours d'édition…).
    pub fn page(&mut self, clip: egui::Rect, skip_text: Option<&Id>, ghost: &[Id]) {
        let clip = clip.expand(60.0);
        let page: &Page = self.page;
        let owned;
        let ids: &HashMap<Id, usize> = match self.ids {
            Some(i) => i,
            None => {
                owned = wb_core::spatial::id_index(page);
                &owned
            }
        };
        let get = |id: &Id| ids.get(id).map(|&i| &page.objects[i]);
        let (a, z) = (self.cam.w(clip.min), self.cam.w(clip.max));
        let visible: Option<Vec<usize>> = self.index.map(|ix| ix.query(Rect::from_corners(a, z)));
        let order: Box<dyn Iterator<Item = usize>> = match &visible {
            Some(v) => Box::new(v.iter().copied()),
            None => Box::new(0..page.objects.len()),
        };
        for i in order {
            let o = &page.objects[i];
            if !page.is_visible(o) {
                continue;
            }
            let wb = self.index.map_or_else(|| page.bounds_with(o, &get), |ix| ix.bounds(i));
            let b = self.cam.rect_s(wb);
            if !b.intersects(clip) {
                continue;
            }
            if b.width() < 3.0 && b.height() < 3.0 {
                self.speck(o, b);
                continue;
            }
            let opacity_mul = if ghost.contains(&o.id) { 0.3 } else { 1.0 };
            self.object(o, skip_text == Some(&o.id), opacity_mul, &get);
        }
    }

    /// Niveau de détail : un objet de moins de 3 px est réduit à un point de couleur.
    fn speck(&self, o: &Object, b: egui::Rect) {
        let color = match &o.kind {
            Kind::Connector(_) => return,
            Kind::Image(_) => Color32::from_gray(150),
            _ => o.style.fill.map_or_else(|| col(o.style.stroke, self.dark, o.style.opacity), |f| col(f, self.dark, o.style.opacity)),
        };
        self.painter.rect_filled(b.expand(0.6), 0.0, color);
    }

    /// Dessine un seul objet (brouillon, aperçu).
    pub fn draw_one(&mut self, o: &Object) {
        let page: &Page = self.page;
        self.object(o, false, 1.0, &|id| page.get(id));
    }

    fn stroke_path(&self, pts: &[Pos2], closed: bool, width: f64, color: Color32, dash: Dash, round_caps: bool) {
        let w = (width as f32 * self.cam.zoom).max(0.0);
        if w <= 0.0 || color.a() == 0 || pts.len() < 2 {
            return;
        }
        let mut v: Vec<Pos2> = pts.to_vec();
        if closed {
            v.push(pts[0]);
        }
        let z = self.cam.zoom;
        match dash {
            Dash::Solid => {
                self.painter.add(Shape::line(v.clone(), Stroke::new(w.max(0.5), color)));
                if round_caps && w > 2.5 {
                    let n = if v.len() <= 80 { v.len() } else { 0 };
                    for p in v.iter().take(n) {
                        self.painter.circle_filled(*p, w / 2.0, color);
                    }
                    if n == 0 {
                        self.painter.circle_filled(v[0], w / 2.0, color);
                        self.painter.circle_filled(v[v.len() - 1], w / 2.0, color);
                    }
                }
            }
            Dash::Dashed => {
                self.painter.extend(Shape::dashed_line(&v, Stroke::new(w.max(0.5), color), ((width * 3.2 + 3.0) as f32) * z, ((width * 2.4 + 3.0) as f32) * z));
            }
            Dash::Dotted => {
                self.painter.extend(Shape::dotted_line(&v, color, ((width * 2.2 + 2.0) as f32) * z, (w / 2.0).max(0.5)));
            }
        }
    }

    fn object<'x>(&mut self, o: &Object, skip_text: bool, mul: f32, get: &dyn Fn(&Id) -> Option<&'x Object>) {
        let op = o.style.opacity * mul;
        match &o.kind {
            Kind::Box(b) => self.boxed(o, b, skip_text, op),
            Kind::Pen(p) => self.pen(o, p, op),
            Kind::Connector(c) => self.connector(o, c, op, get),
            Kind::Image(i) => self.image(i, op),
            Kind::Table(t) => self.table(o, t, op),
        }
    }

    fn outline(&self, b: &BoxObj, roundness: f32) -> Vec<Pt> {
        let (w, h) = (b.frame.w, b.frame.h);
        let (hw, hh) = (w / 2.0, h / 2.0);
        match b.geom {
            BoxGeom::Ellipse => {
                let n = ((w.max(h) as f32 * self.cam.zoom) / 3.0).clamp(10.0, 160.0) as usize;
                (0..n)
                    .map(|i| {
                        let a = i as f64 / n as f64 * std::f64::consts::TAU;
                        Pt::new(hw * a.cos(), hh * a.sin())
                    })
                    .collect()
            }
            BoxGeom::Diamond => Outline::Diamond.polygon(w, h),
            BoxGeom::Triangle => Outline::Triangle.polygon(w, h),
            BoxGeom::Rect if roundness > 0.0 => {
                let r = (f64::from(roundness) * w.min(h) / 2.0).min(hw.min(hh));
                let mut pts = Vec::new();
                for (cx, cy, a0) in [(hw - r, -hh + r, -90.0f64), (hw - r, hh - r, 0.0), (-hw + r, hh - r, 90.0), (-hw + r, -hh + r, 180.0)] {
                    for k in 0..=8 {
                        let a = (a0 + 90.0 * f64::from(k) / 8.0).to_radians();
                        pts.push(Pt::new(cx + r * a.cos(), cy + r * a.sin()));
                    }
                }
                pts
            }
            _ => Outline::Rect.polygon(w, h),
        }
    }

    fn boxed(&mut self, o: &Object, b: &BoxObj, skip_text: bool, op: f32) {
        let st = &o.style;
        let f = b.frame;
        let screen = |pts: &[Pt]| -> Vec<Pos2> { pts.iter().map(|p| self.cam.s(f.to_world(*p))).collect() };
        let mut label_color = col(st.stroke, self.dark, op);
        match b.geom {
            BoxGeom::Text => {}
            BoxGeom::Sticky => {
                let pts = screen(&self.outline(b, 0.0));
                let off = |dx: f32, dy: f32| pts.iter().map(|p| *p + vec2(dx, dy) * self.cam.zoom).collect::<Vec<_>>();
                self.painter.add(Shape::convex_polygon(off(4.0, 8.0), Color32::from_black_alpha((26.0 * op) as u8), Stroke::NONE));
                self.painter.add(Shape::convex_polygon(off(2.0, 4.0), Color32::from_black_alpha((20.0 * op) as u8), Stroke::NONE));
                self.painter.add(Shape::convex_polygon(pts, col(st.fill.unwrap_or(style::NOTE_YELLOW), self.dark, op), Stroke::NONE));
                label_color = Color32::from_rgba_unmultiplied(0x1e, 0x1e, 0x1e, (op * 255.0) as u8);
            }
            _ => {
                let pts = screen(&self.outline(b, st.roundness));
                if let Some(fill) = st.fill {
                    self.painter.add(Shape::convex_polygon(pts.clone(), col(fill, self.dark, op), Stroke::NONE));
                }
                if st.width > 0.0 {
                    self.stroke_path(&pts, true, st.width, col(st.stroke, self.dark, op), st.dash, false);
                }
            }
        }
        if !skip_text && !b.label.text.is_empty() && b.label.size as f32 * self.cam.zoom >= 3.0 {
            let lay = b.layout(self.measurer);
            let fr = f;
            self.paint_lines(&lay, &b.label, b.label_rect(), &|p| fr.to_world(p), f.angle as f32, label_color, false);
        }
    }

    /// Dessine un bloc de texte déjà mis en page (une galerie de texte par ligne).
    #[allow(clippy::too_many_arguments)]
    pub fn paint_lines(&self, lay: &TextLayout, label: &Label, area: Rect, to_world: &dyn Fn(Pt) -> Pt, angle: f32, base: Color32, bold_all: bool) {
        let size = label.size as f32 * self.cam.zoom;
        if size < 3.0 {
            return;
        }
        let mut y = if lay.height > area.h { area.y } else { area.y + (area.h - lay.height) / 2.0 };
        for line in &lay.lines {
            let mut job = LayoutJob::default();
            job.wrap.max_width = f32::INFINITY;
            for run in line {
                let color = run.style.color.map_or(base, |c| col(c, self.dark, f32::from(base.a()) / 255.0));
                job.append(
                    &run.text,
                    0.0,
                    TextFormat {
                        font_id: font_id(label.family, size, run.style.bold || bold_all),
                        color,
                        background: run.style.highlight.map_or(Color32::TRANSPARENT, |h| col(h, self.dark, 0.55)),
                        italics: run.style.italic,
                        underline: if run.style.underline { Stroke::new(1.0, color) } else { Stroke::NONE },
                        strikethrough: if run.style.strike { Stroke::new(1.0, color) } else { Stroke::NONE },
                        ..Default::default()
                    },
                );
            }
            if !line.is_empty() {
                let galley = self.painter.layout_job(job);
                let lw = f64::from(galley.size().x / self.cam.zoom);
                let x = match label.align {
                    Align::Left => area.x,
                    Align::Center => area.x + area.w / 2.0 - lw / 2.0,
                    Align::Right => area.right() - lw,
                };
                let pos = self.cam.s(to_world(Pt::new(x, y)));
                self.painter.add(TextShape::new(pos, galley, base).with_angle(angle));
            }
            y += lay.line_h;
        }
    }

    fn pen(&self, o: &Object, p: &PenObj, op: f32) {
        let st = &o.style;
        let c = col(st.stroke, self.dark, op);
        if p.pts.len() < 2 {
            return;
        }
        let (sm, pr) = wb_core::pen::smooth(&p.pts, p.pressures.as_ref().map(|v| v.as_slice()), 4);
        if p.pressures.is_some() && !p.highlighter {
            self.pressure_stroke(&sm, &pr, st.width, c);
        } else {
            let pts: Vec<Pos2> = sm.iter().map(|q| self.cam.s(*q)).collect();
            self.stroke_path(&pts, false, st.width, c, Dash::Solid, !p.highlighter);
        }
    }

    /// Trait d'épaisseur variable : bande de triangles dont la largeur suit la pression, avec embouts arrondis.
    fn pressure_stroke(&self, pts: &[Pt], pressures: &[f32], base: f64, color: Color32) {
        let widths: Vec<f64> = pressures.iter().map(|p| wb_core::pen::pressure_width(base, *p)).collect();
        let (left, right) = wb_core::pen::stroke_sides(pts, &widths);
        let mut mesh = Mesh::default();
        for (l, r) in left.iter().zip(&right) {
            mesh.vertices.push(Vertex { pos: self.cam.s(*l), uv: egui::epaint::WHITE_UV, color });
            mesh.vertices.push(Vertex { pos: self.cam.s(*r), uv: egui::epaint::WHITE_UV, color });
        }
        for i in 0..pts.len().saturating_sub(1) as u32 {
            let k = i * 2;
            mesh.indices.extend_from_slice(&[k, k + 1, k + 2, k + 1, k + 3, k + 2]);
        }
        self.painter.add(Shape::mesh(mesh));
        for (i, p) in [(0, pts[0]), (pts.len() - 1, pts[pts.len() - 1])] {
            self.painter.circle_filled(self.cam.s(p), (widths[i] as f32 * self.cam.zoom / 2.0).max(0.4), color);
        }
    }

    fn connector<'x>(&self, o: &Object, c: &Connector, op: f32, get: &dyn Fn(&Id) -> Option<&'x Object>) {
        let Some(path) = self.page.connector_path_with(c, get) else { return };
        if path.len() < 2 {
            return;
        }
        let st = &o.style;
        let color = col(st.stroke, self.dark, op);
        let pts: Vec<Pos2> = path.iter().map(|p| self.cam.s(*p)).collect();
        self.stroke_path(&pts, false, st.width, color, st.dash, true);
        let size = (st.width * 4.5).max(12.0);
        for (head, tip, from) in [(c.head_start, path[0], path[1]), (c.head_end, path[path.len() - 1], path[path.len() - 2])] {
            let hp: Vec<Pos2> = head_points(tip, from, size, head).iter().map(|p| self.cam.s(*p)).collect();
            let w = st.width as f32 * self.cam.zoom;
            match head {
                Arrowhead::None => {}
                Arrowhead::Arrow => {
                    self.painter.add(Shape::line(hp.clone(), Stroke::new(w, color)));
                    for p in &hp {
                        self.painter.circle_filled(*p, w / 2.0, color);
                    }
                }
                Arrowhead::Triangle => {
                    self.painter.add(Shape::convex_polygon(hp.clone(), color, Stroke::new(w * 0.6, color)));
                }
                Arrowhead::Bar => {
                    self.painter.add(Shape::line(hp, Stroke::new(w, color)));
                }
                Arrowhead::Dot => {
                    self.painter.circle_filled(hp[0], size as f32 * 0.4 * self.cam.zoom, color);
                }
            }
        }
        if let Some(l) = c.label.as_ref().filter(|l| !l.text.is_empty()) {
            let (mid, _) = path_midpoint(&path);
            let lay = layout_rich(&l.text, None, self.measurer, l.family, l.size);
            let area = Rect::new(mid.x - lay.width / 2.0, mid.y - lay.height / 2.0, lay.width, lay.height);
            let r = self.cam.rect_s(area).expand(3.0 * self.cam.zoom.min(1.0));
            self.painter.rect_filled(r, 3.0, self.bg);
            let label = Label { align: Align::Center, ..l.clone() };
            self.paint_lines(&lay, &label, area, &|p| p, 0.0, color, false);
        }
    }

    fn image(&mut self, i: &ImageObj, op: f32) {
        let Some(asset) = self.doc.assets.get(&i.asset) else { return };
        let ctx = self.painter.ctx().clone();
        let corners: Vec<Pos2> = i.frame.corners().iter().map(|p| self.cam.s(*p)).collect();
        match self.textures.get(&ctx, &i.asset, asset) {
            Some(tex) => {
                let tint = Color32::from_white_alpha((op * 255.0) as u8);
                let uvs = [pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0)];
                let mut mesh = Mesh::with_texture(tex.id());
                for (p, uv) in corners.iter().zip(uvs) {
                    mesh.vertices.push(Vertex { pos: *p, uv, color: tint });
                }
                mesh.indices = vec![0, 1, 2, 0, 2, 3];
                self.painter.add(Shape::mesh(mesh));
            }
            None => {
                self.painter.add(Shape::convex_polygon(corners, Color32::from_gray(128).gamma_multiply(0.3), Stroke::NONE));
            }
        }
    }

    fn table(&self, o: &Object, t: &Table, op: f32) {
        let st = &o.style;
        let c = col(st.stroke, self.dark, op);
        let r = t.rect();
        let w = st.width.max(1.0);
        if t.header {
            self.painter.rect_filled(self.cam.rect_s(Rect::new(r.x, r.y, r.w, t.row_h[0])), 0.0, c.gamma_multiply(0.12));
        }
        let mut x = r.x;
        for cw in &t.col_w[..t.cols() - 1] {
            x += cw;
            self.painter.line_segment([self.cam.s(Pt::new(x, r.y)), self.cam.s(Pt::new(x, r.bottom()))], Stroke::new((w * 0.6) as f32 * self.cam.zoom, c));
        }
        let mut y = r.y;
        for rh in &t.row_h[..t.rows() - 1] {
            y += rh;
            self.painter.line_segment([self.cam.s(Pt::new(r.x, y)), self.cam.s(Pt::new(r.right(), y))], Stroke::new((w * 0.6) as f32 * self.cam.zoom, c));
        }
        self.painter.rect_stroke(self.cam.rect_s(r), 0.0, Stroke::new(w as f32 * self.cam.zoom, c), egui::StrokeKind::Middle);
        let label = Label { text: RichText::default(), family: t.family, size: t.size, align: Align::Left };
        for ri in 0..t.rows() {
            for ci in 0..t.cols() {
                let Some(cell) = t.cell(ri, ci).filter(|c| !c.is_empty()) else { continue };
                let cr = t.cell_rect(ri, ci);
                let area = Rect::new(cr.x + 8.0, cr.y + 4.0, (cr.w - 16.0).max(1.0), (cr.h - 8.0).max(1.0));
                let lay = layout_rich(cell, Some(area.w), self.measurer, t.family, t.size);
                self.paint_lines(&lay, &label, area, &|p| p, 0.0, c, t.header && ri == 0);
            }
        }
    }
}

/// Rectangle d'une forme « boîte » en coordonnées écran pour superposer un éditeur de texte.
pub fn label_screen_rect(cam: &Cam, frame: &Frame, area: Rect) -> egui::Rect {
    let tl = cam.s(frame.to_world(Pt::new(area.x, area.y)));
    let br = cam.s(frame.to_world(Pt::new(area.right(), area.bottom())));
    egui::Rect::from_two_pos(tl, br)
}
