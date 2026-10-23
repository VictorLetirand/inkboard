//! Drawing of a page with egui: same logic as the engine's SVG export.

use crate::fonts::{EguiMeasurer, font_id};
use eframe::egui::epaint::{Mesh, TextShape, Vertex};
use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{self, Color32, Painter, Pos2, Shape, Stroke, TextureHandle, pos2, vec2};
use inkboard_core::connector::{head_points, path_midpoint};
use inkboard_core::geom::{Frame, Pt, Rect};
use inkboard_core::model::Outline;
use inkboard_core::text::{TextLayout, layout_rich};
use inkboard_core::*;
use std::collections::HashMap;

pub const GRID: f32 = 24.0;

/// Camera: screen position of the world origin, and zoom.
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

/// Fallback rasterization scale of an SVG whose size cannot be read.
const SVG_FALLBACK_SCALE: f32 = 4.0;
/// Longest side (pixels) of the smallest and largest SVG rasterizations.
const SVG_MIN_PX: u32 = 64;
const SVG_MAX_PX: u32 = 4096;
/// Inflated textures kept at most (bytes); the least recently used ones are dropped beyond.
const TEXTURE_BUDGET: usize = 256 << 20;
/// Images shown smaller than this (points) are drawn as a flat quad of their average color.
const IMAGE_FLAT_PT: f32 = 9.0;
/// Time (ms) a frame may spend rasterizing SVGs: past it, the remaining ones reuse the closest texture already available and finish on the
/// next frames, so a big board never freezes the interface.
const RASTER_BUDGET_MS: f64 = 5.0;

/// Texture side assumed when the graphics context is not at hand (previews); the real limit comes from `max_texture_side`.
const DEFAULT_MAX_TEXTURE_SIDE: usize = 4096;
/// A bitmap declaring more pixels than this is not decoded at all (its decoded form would exceed the decoder's own 512 MiB allocation limit).
const MAX_DECODE_PIXELS: u64 = 100_000_000;
/// A decoded bitmap is reduced to at most this many pixels (a 40 Mpx image already takes 160 MB of texture memory).
const MAX_TEXTURE_PIXELS: u64 = 40_000_000;

/// Decoded image of an asset (bitmaps; SVGs at the fallback scale, used for previews).
pub fn decode_asset(asset: &Asset) -> Option<egui::ColorImage> {
    decode_asset_within(asset, DEFAULT_MAX_TEXTURE_SIDE)
}

/// Like [`decode_asset`], for a texture whose sides may not exceed `max_side` (the GPU limit: a larger texture aborts the program).
/// The dimensions are read from the header first: an image that declares too many pixels is never decoded; one that is merely too
/// wide or too tall (e.g. 20 000 x 1) or too big in memory is reduced, keeping its proportions.
pub fn decode_asset_within(asset: &Asset, max_side: usize) -> Option<egui::ColorImage> {
    let max_side = max_side.clamp(1, 1 << 15) as u32;
    if asset.mime == "image/svg+xml" {
        return rasterize_svg(asset, None, max_side);
    }
    let reader = || image::ImageReader::new(std::io::Cursor::new(&asset.data[..])).with_guessed_format().ok();
    let (w, h) = reader()?.into_dimensions().ok()?;
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_DECODE_PIXELS {
        return None;
    }
    let mut img = reader()?.decode().ok()?.into_rgba8();
    let scale = (f64::from(max_side) / f64::from(w))
        .min(f64::from(max_side) / f64::from(h))
        .min((MAX_TEXTURE_PIXELS as f64 / (f64::from(w) * f64::from(h))).sqrt())
        .min(1.0);
    if scale < 1.0 {
        let (nw, nh) = (((f64::from(w) * scale) as u32).clamp(1, max_side), ((f64::from(h) * scale) as u32).clamp(1, max_side));
        img = image::imageops::thumbnail(&img, nw, nh);
    }
    Some(egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()))
}

/// Rasterizes an SVG asset so that its longest side is `px` pixels (`None`: the fallback scale, lowered to `max_side` at most).
fn rasterize_svg(asset: &Asset, px: Option<u32>, max_side: u32) -> Option<egui::ColorImage> {
    let svg = std::str::from_utf8(&asset.data).ok()?;
    let r = match px {
        Some(px) => inkboard_formats::raster::svg_to_raster_fit(svg, px.min(max_side), false),
        None => inkboard_formats::raster::svg_to_raster_max_side(svg, SVG_FALLBACK_SCALE, max_side, false),
    }
    .ok()?;
    Some(egui::ColorImage::from_rgba_premultiplied([r.width as usize, r.height as usize], &r.rgba_premultiplied))
}

/// Rasterization level of an SVG shown with a longest side of `target_px` screen pixels: the next power of two, so a texture is never
/// stretched (and at most halved), whether the cause is zooming in or enlarging the object.
fn svg_level(target_px: f32, max_side: usize) -> u32 {
    (target_px.max(1.0).ceil() as u32).next_power_of_two().clamp(SVG_MIN_PX, SVG_MAX_PX.min(max_side as u32))
}

/// A texture region ready to be drawn: either a whole texture or a cell of the small-icon atlas.
#[derive(Clone)]
pub struct ImageTex {
    pub id: egui::TextureId,
    /// Part of the texture holding the image.
    pub uv: egui::Rect,
    /// Average color (premultiplied): what the image looks like when it is only a few pixels wide.
    pub avg: Color32,
    _keep: TextureHandle,
}

impl ImageTex {
    /// Size in pixels of the whole texture (an atlas page for small icons).
    #[cfg(test)]
    pub fn size(&self) -> [usize; 2] {
        self._keep.size()
    }
}

struct CachedTexture {
    level: u32,
    tex: ImageTex,
    /// Memory this entry owns (0 for atlas cells, whose pages are shared).
    bytes: usize,
    used: u64,
}

/// Counters of the texture cache (performance checks).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextureStats {
    /// SVG rasterizations done.
    pub rasterized: u32,
    /// Total time spent rasterizing SVGs, in milliseconds.
    pub raster_ms: f64,
    /// Requests answered with a texture of another resolution (or none) because the frame's rasterization budget was spent.
    pub deferred: u32,
    /// Textures dropped to stay within the memory budget.
    pub evicted: u32,
}

/// Page of the atlas holding the small SVG icons: many icons share one texture, so thousands of them are drawn in a single batch.
struct AtlasPage {
    tex: TextureHandle,
    used: usize,
}

/// Textures of the document's images, decoded on demand. SVG images are rasterized again when they are shown larger than their current texture;
/// the small ones live in a shared atlas.
pub struct TextureCache {
    /// Per asset: its textures, one per rasterization level (level 0 for bitmaps, [`ATLAS_LEVEL`] for atlas cells, the longest side in pixels otherwise).
    map: HashMap<Id, Vec<CachedTexture>>,
    atlas: Vec<AtlasPage>,
    /// Assets that cannot be decoded (not tried again).
    broken: std::collections::HashSet<Id>,
    frame: Option<u64>,
    /// Time spent rasterizing during the current frame, in milliseconds.
    spent_ms: f64,
    /// Time (ms) a frame may spend rasterizing SVGs ([`RASTER_BUDGET_MS`]; changed by tests).
    pub raster_budget_ms: f64,
    pub stats: TextureStats,
}

impl Default for TextureCache {
    fn default() -> Self {
        Self {
            map: HashMap::new(),
            atlas: Vec::new(),
            broken: Default::default(),
            frame: None,
            spent_ms: 0.0,
            raster_budget_ms: RASTER_BUDGET_MS,
            stats: TextureStats::default(),
        }
    }
}

/// Pseudo-level of the images stored in the atlas.
const ATLAS_LEVEL: u32 = 1;
/// Side of an atlas page and of one cell (the icon is drawn inside a 1 px transparent margin).
const ATLAS_PAGE: usize = 2048;
const ATLAS_CELL: usize = 32;
/// Icons shown with a longest side up to this many pixels use the atlas.
const ATLAS_MAX_PX: f32 = (ATLAS_CELL - 2) as f32;
const ATLAS_MAX_PAGES: usize = 4;

/// Average color of an image (sampled), premultiplied.
fn average_color(img: &egui::ColorImage) -> Color32 {
    let n = img.pixels.len().max(1);
    let step = (n / 1024).max(1);
    let (mut r, mut g, mut b, mut a, mut k) = (0u64, 0u64, 0u64, 0u64, 0u64);
    for p in img.pixels.iter().step_by(step) {
        r += u64::from(p.r());
        g += u64::from(p.g());
        b += u64::from(p.b());
        a += u64::from(p.a());
        k += 1;
    }
    let k = k.max(1);
    Color32::from_rgba_premultiplied((r / k) as u8, (g / k) as u8, (b / k) as u8, (a / k) as u8)
}

impl TextureCache {
    /// Texture of an image shown with a longest side of `target_px` screen pixels.
    pub fn get(&mut self, ctx: &egui::Context, id: &Id, asset: &Asset, target_px: f32) -> Option<ImageTex> {
        let now = ctx.cumulative_pass_nr();
        if self.frame != Some(now) {
            self.frame = Some(now);
            self.spent_ms = 0.0;
        }
        if self.broken.contains(id) {
            return None;
        }
        let is_svg = asset.mime == "image/svg+xml";
        let atlas_room = self.atlas.len() < ATLAS_MAX_PAGES || self.atlas.last().is_some_and(|p| p.used < (ATLAS_PAGE / ATLAS_CELL).pow(2));
        let in_atlas = is_svg && target_px <= ATLAS_MAX_PX && atlas_room;
        let level = if !is_svg {
            0
        } else if in_atlas {
            ATLAS_LEVEL
        } else {
            svg_level(target_px, ctx.input(|i| i.max_texture_side))
        };
        if let Some(list) = self.map.get_mut(id)
            && let Some(c) = list.iter_mut().find(|c| c.level == level)
        {
            c.used = now;
            return Some(c.tex.clone());
        }
        // the first rasterization of a frame is always allowed; the next ones only while the frame's budget lasts
        if is_svg && self.spent_ms > 0.0 && self.spent_ms >= self.raster_budget_ms {
            self.stats.deferred += 1;
            ctx.request_repaint();
            return self.map.get(id).and_then(|l| l.iter().min_by_key(|c| c.level.abs_diff(level))).map(|c| c.tex.clone());
        }
        let t0 = crate::platform::Instant::now();
        let max_side = ctx.input(|i| i.max_texture_side);
        let img = if !is_svg {
            decode_asset_within(asset, max_side)
        } else if in_atlas {
            rasterize_svg(asset, Some(ATLAS_CELL as u32 - 2), max_side as u32)
        } else {
            rasterize_svg(asset, Some(level), max_side as u32)
        };
        if is_svg {
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            self.spent_ms += ms;
            self.stats.rasterized += 1;
            self.stats.raster_ms += ms;
        }
        let Some(img) = img else {
            self.broken.insert(id.clone());
            return None;
        };
        let avg = average_color(&img);
        let (tex, bytes) = if in_atlas && img.size[0] <= ATLAS_CELL - 2 && img.size[1] <= ATLAS_CELL - 2 {
            (self.atlas_cell(ctx, id, &img, avg), 0)
        } else {
            let bytes = img.pixels.len() * 4;
            let h = ctx.load_texture(format!("{}@{level}", id.as_str()), img, egui::TextureOptions::LINEAR);
            (ImageTex { id: h.id(), uv: egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), avg, _keep: h }, bytes)
        };
        self.map.entry(id.clone()).or_default().push(CachedTexture { level, tex: tex.clone(), bytes, used: now });
        self.evict(id, level);
        Some(tex)
    }

    /// Copies a small icon into the next free cell of the atlas (a new page when the last one is full).
    fn atlas_cell(&mut self, ctx: &egui::Context, id: &Id, img: &egui::ColorImage, avg: Color32) -> ImageTex {
        let per_row = ATLAS_PAGE / ATLAS_CELL;
        if self.atlas.last().is_none_or(|p| p.used >= per_row * per_row) {
            let blank = egui::ColorImage::filled([ATLAS_PAGE, ATLAS_PAGE], Color32::TRANSPARENT);
            let tex = ctx.load_texture(format!("icon-atlas-{}", self.atlas.len()), blank, egui::TextureOptions::LINEAR);
            self.atlas.push(AtlasPage { tex, used: 0 });
        }
        let page = self.atlas.last_mut().expect("a page was just ensured");
        let (cx, cy) = (page.used % per_row, page.used / per_row);
        page.used += 1;
        let (x, y) = (cx * ATLAS_CELL + 1, cy * ATLAS_CELL + 1);
        page.tex.set_partial([x, y], img.clone(), egui::TextureOptions::LINEAR);
        let s = ATLAS_PAGE as f32;
        let (w, h) = (img.size[0] as f32, img.size[1] as f32);
        let uv = egui::Rect::from_min_max(pos2(x as f32 / s, y as f32 / s), pos2((x as f32 + w) / s, (y as f32 + h) / s));
        let _ = id;
        ImageTex { id: page.tex.id(), uv, avg, _keep: page.tex.clone() }
    }

    /// Drops the least recently used textures while the cache is over its budget (never the one just created).
    fn evict(&mut self, keep_id: &Id, keep_level: u32) {
        let mut total: usize = self.map.values().flatten().map(|c| c.bytes).sum();
        while total > TEXTURE_BUDGET {
            let oldest = self
                .map
                .iter()
                .flat_map(|(id, l)| l.iter().map(move |c| (id, c)))
                .filter(|(id, c)| c.bytes > 0 && !(*id == keep_id && c.level == keep_level))
                .min_by_key(|(_, c)| c.used)
                .map(|(id, c)| (id.clone(), c.level));
            let Some((id, level)) = oldest else { break };
            if let Some(list) = self.map.get_mut(&id)
                && let Some(pos) = list.iter().position(|c| c.level == level)
            {
                total -= list.remove(pos).bytes;
                self.stats.evicted += 1;
            }
            if self.map.get(&id).is_some_and(Vec::is_empty) {
                self.map.remove(&id);
            }
        }
    }

    /// Number of textures held (atlas cells count one each).
    pub fn texture_count(&self) -> usize {
        self.map.values().map(Vec::len).sum()
    }

    /// Memory held by the textures, in bytes (atlas pages included).
    pub fn bytes(&self) -> usize {
        self.map.values().flatten().map(|c| c.bytes).sum::<usize>() + self.atlas.len() * ATLAS_PAGE * ATLAS_PAGE * 4
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.atlas.clear();
        self.broken.clear();
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
    /// Id → position table (cached in the application); computed on the fly when absent.
    pub ids: Option<&'a HashMap<Id, usize>>,
    /// Up-to-date spatial index (absent during gestures that move objects).
    pub index: Option<&'a inkboard_core::spatial::SpatialIndex>,
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
                let st = Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(base, base, base, 24));
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

    /// Draws the visible objects; `skip`: ids not to draw (text being edited…).
    pub fn page(&mut self, clip: egui::Rect, skip_text: Option<&Id>, ghost: &[Id]) {
        let clip = clip.expand(60.0);
        let page: &Page = self.page;
        let owned;
        let ids: &HashMap<Id, usize> = match self.ids {
            Some(i) => i,
            None => {
                owned = inkboard_core::spatial::id_index(page);
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

    /// Level of detail: an object smaller than 3 px is reduced to a colored dot.
    fn speck(&self, o: &Object, b: egui::Rect) {
        let color = match &o.kind {
            Kind::Connector(_) => return,
            Kind::Image(_) => Color32::from_gray(150),
            _ => o.style.fill.map_or_else(|| col(o.style.stroke, self.dark, o.style.opacity), |f| col(f, self.dark, o.style.opacity)),
        };
        self.painter.rect_filled(b.expand(0.6), 0.0, color);
    }

    /// Draws a single object (draft, preview).
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
            Kind::Image(i) => self.image(i, if i.is_tinted() { col(o.style.stroke, self.dark, op) } else { Color32::from_white_alpha((op * 255.0) as u8) }),
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

    /// Draws an already laid-out block of text (one text galley per line).
    #[allow(clippy::too_many_arguments)]
    pub fn paint_lines(&self, lay: &TextLayout, label: &Label, area: Rect, to_world: &dyn Fn(Pt) -> Pt, angle: f32, base: Color32, bold_all: bool) {
        let size = label.size as f32 * self.cam.zoom;
        if size < 3.0 {
            return;
        }
        let origins = line_origins(lay, label.align, area);
        for (line, &(x, y)) in lay.lines.iter().zip(&origins) {
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
                        underline: if run.style.underline { Stroke::new(1.0_f32, color) } else { Stroke::NONE },
                        ..Default::default()
                    },
                );
            }
            if !line.is_empty() {
                let galley = self.painter.layout_job(job);
                let strike: Vec<bool> = line.iter().flat_map(|run| run.text.chars().map(|_| run.style.strike)).collect();
                let pos = self.cam.s(to_world(Pt::new(x, y)));
                if strike.iter().any(|s| *s) {
                    let (sin, cos) = angle.sin_cos();
                    let w = (size * 0.07).max(1.0);
                    for (x0, x1, y0) in strike_segments(&galley, &strike, size) {
                        let rot = |x: f32| pos + vec2(x * cos - y0 * sin, x * sin + y0 * cos);
                        let c = line
                            .iter()
                            .find(|r| r.style.strike)
                            .and_then(|r| r.style.color.map(|c| col(c, self.dark, f32::from(base.a()) / 255.0)))
                            .unwrap_or(base);
                        self.painter.line_segment([rot(x0), rot(x1)], Stroke::new(w, c));
                    }
                }
                self.painter.add(TextShape::new(pos, galley, base).with_angle(angle));
            }
        }
    }

    fn pen(&self, o: &Object, p: &PenObj, op: f32) {
        let st = &o.style;
        let c = col(st.stroke, self.dark, op);
        if p.pts.len() < 2 {
            return;
        }
        let (sm, pr) = inkboard_core::pen::smooth(&p.pts, p.pressures.as_ref().map(|v| v.as_slice()), 4);
        if p.pressures.is_some() && !p.highlighter {
            self.pressure_stroke(&sm, &pr, st.width, c);
        } else {
            let pts: Vec<Pos2> = sm.iter().map(|q| self.cam.s(*q)).collect();
            self.stroke_path(&pts, false, st.width, c, Dash::Solid, !p.highlighter);
        }
    }

    /// Variable-width stroke: a triangle strip whose width follows the pressure, with rounded caps.
    fn pressure_stroke(&self, pts: &[Pt], pressures: &[f32], base: f64, color: Color32) {
        let widths: Vec<f64> = pressures.iter().map(|p| inkboard_core::pen::pressure_width(base, *p)).collect();
        let (left, right) = inkboard_core::pen::stroke_sides(pts, &widths);
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

    fn image(&mut self, i: &ImageObj, tint: Color32) {
        let Some(asset) = self.doc.assets.get(&i.asset) else { return };
        let ctx = self.painter.ctx().clone();
        let size_pt = i.frame.w.max(i.frame.h) as f32 * self.cam.zoom;
        let target_px = size_pt * ctx.pixels_per_point();
        let tex = self.textures.get(&ctx, &i.asset, asset, target_px);
        let corners = || -> Vec<Pos2> { i.frame.corners().iter().map(|p| self.cam.s(*p)).collect() };
        match tex {
            Some(t) if size_pt < IMAGE_FLAT_PT => {
                // a few pixels wide: a flat quad of the image's average color (no texture, so thousands of them cost one batch)
                let a = f32::from(t.avg.a()) / 255.0;
                let color = if i.is_tinted() { tint.gamma_multiply(a) } else { t.avg.gamma_multiply(f32::from(tint.a()) / 255.0) };
                if i.frame.angle == 0.0 {
                    let (p, q) = (self.cam.s(Pt::new(i.frame.x, i.frame.y)), self.cam.s(Pt::new(i.frame.x + i.frame.w, i.frame.y + i.frame.h)));
                    self.painter.rect_filled(egui::Rect::from_two_pos(p, q), 0.0, color);
                } else {
                    self.painter.add(Shape::convex_polygon(corners(), color, Stroke::NONE));
                }
            }
            Some(t) => {
                let corners = corners();
                let (lo, hi) = (t.uv.min, t.uv.max);
                let uvs = [pos2(lo.x, lo.y), pos2(hi.x, lo.y), pos2(hi.x, hi.y), pos2(lo.x, hi.y)];
                let mut mesh = Mesh::with_texture(t.id);
                for (p, uv) in corners.iter().zip(uvs) {
                    mesh.vertices.push(Vertex { pos: *p, uv, color: tint });
                }
                mesh.indices = vec![0, 1, 2, 0, 2, 3];
                self.painter.add(Shape::mesh(mesh));
            }
            None => {
                self.painter.add(Shape::convex_polygon(corners(), Color32::from_gray(128).gamma_multiply(0.3), Stroke::NONE));
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

/// Top-left origin (model units) of every line of a laid-out text inside `area`: left / center / right aligned, and centered vertically when it fits.
/// The final rendering and the text editor both use it, so the text does not move when editing starts or ends.
pub fn line_origins(lay: &TextLayout, align: Align, area: Rect) -> Vec<(f64, f64)> {
    let mut y = if lay.height > area.h { area.y } else { area.y + (area.h - lay.height) / 2.0 };
    let mut out = Vec::with_capacity(lay.widths.len());
    for w in &lay.widths {
        let x = match align {
            Align::Left => area.x,
            Align::Center => area.x + area.w / 2.0 - w / 2.0,
            Align::Right => area.right() - w,
        };
        out.push((x, y));
        y += lay.line_h;
    }
    out
}

/// Strikethrough segments `(x0, x1, y)` relative to the galley origin. egui draws its own at the middle of the whole text row, which looks
/// too high; these sit at the middle of lowercase letters. `strike[i]` tells whether the i-th glyph (newlines excluded) is struck.
pub fn strike_segments(galley: &egui::Galley, strike: &[bool], size: f32) -> Vec<(f32, f32, f32)> {
    let mut out = Vec::new();
    let mut i = 0;
    for placed in &galley.rows {
        let mut seg: Option<(f32, f32)> = None;
        let base = placed.pos.y + placed.row.glyphs.first().map_or(0.0, |g| g.pos.y);
        let y = base - size * 0.28;
        for g in &placed.row.glyphs {
            let on = strike.get(i).copied().unwrap_or(false);
            i += 1;
            let (x0, x1) = (placed.pos.x + g.pos.x, placed.pos.x + g.pos.x + g.advance_width);
            if on {
                seg = Some(seg.map_or((x0, x1), |(a, _)| (a, x1)));
            } else if let Some((a, b)) = seg.take() {
                out.push((a, b, y));
            }
        }
        if let Some((a, b)) = seg {
            out.push((a, b, y));
        }
    }
    out
}

/// Rectangle of a "box" shape in screen coordinates, to overlay a text editor.
pub fn label_screen_rect(cam: &Cam, frame: &Frame, area: Rect) -> egui::Rect {
    let tl = cam.s(frame.to_world(Pt::new(area.x, area.y)));
    let br = cam.s(frame.to_world(Pt::new(area.right(), area.bottom())));
    egui::Rect::from_two_pos(tl, br)
}
