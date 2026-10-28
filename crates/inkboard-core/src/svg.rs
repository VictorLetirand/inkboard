//! SVG export of a page (or a selection), with no UI dependency.

use crate::connector::{HeadPart, head_glyph, path_midpoint};
use crate::geom::{Pt, Rect};
use crate::model::{BoxGeom, BoxObj, Document, Id, Kind, Label, Object, Page, Table};
use crate::style::{Align, Color, Dash, NOTE_YELLOW, Style};
use crate::text::{Measurer, RichText, TextLayout, layout_rich};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::fmt::Write;

#[derive(Clone, Debug)]
pub struct SvgOptions {
    pub padding: f64,
    pub transparent: bool,
}
impl Default for SvgOptions {
    fn default() -> Self {
        Self { padding: 24.0, transparent: false }
    }
}

fn n(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}
fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

struct Ctx<'a> {
    dark: bool,
    doc: &'a Document,
    page: &'a Page,
    m: &'a dyn Measurer,
    /// Shadow filters emitted so far (gives each its own id).
    shadows: std::cell::Cell<usize>,
}

impl Ctx<'_> {
    fn col(&self, c: Color) -> String {
        c.resolve_hex(self.dark)
    }
    fn stroke(&self, st: &Style, butt: bool) -> String {
        let mut s = format!(
            r#"stroke="{}" stroke-width="{}" stroke-linecap="{}" stroke-linejoin="round""#,
            self.col(st.stroke),
            n(st.width),
            if butt { "butt" } else { "round" }
        );
        match st.dash {
            Dash::Solid => {}
            Dash::Dashed => {
                let _ = write!(s, r#" stroke-dasharray="{} {}""#, n(st.width * 3.2 + 3.0), n(st.width * 2.4 + 3.0));
            }
            Dash::Dotted => {
                let _ = write!(s, r#" stroke-dasharray="0.1 {}""#, n(st.width * 2.2 + 2.0));
            }
        }
        s
    }
    fn fill(&self, c: Option<Color>) -> String {
        c.map_or_else(|| "none".to_owned(), |c| self.col(c))
    }

    /// Text block: lines vertically centered in `area` (current frame).
    fn text(&self, lay: &TextLayout, area: Rect, label: &Label, color: Color, bold_all: bool) -> String {
        let mut out = String::new();
        let li = lay.line_h;
        let mut y = if lay.height > area.h { area.y + li / 2.0 } else { area.y + (area.h - lay.height) / 2.0 + li / 2.0 };
        let (anchor, ax) = match label.align {
            Align::Left => ("start", area.x),
            Align::Center => ("middle", area.x + area.w / 2.0),
            Align::Right => ("end", area.x + area.w),
        };
        for (i, line) in lay.lines.iter().enumerate() {
            let lw = lay.widths.get(i).copied().unwrap_or(0.0);
            let x0 = match label.align {
                Align::Left => area.x,
                Align::Center => ax - lw / 2.0,
                Align::Right => ax - lw,
            };
            // surlignages
            let mut cx = x0;
            for r in line {
                let w = self.m.width(&r.text, label.family, label.size, r.style.bold || bold_all);
                if let Some(h) = r.style.highlight {
                    let _ = write!(
                        out,
                        r#"<rect x="{}" y="{}" width="{}" height="{}" fill="{}" opacity="0.55"/>"#,
                        n(cx),
                        n(y - li / 2.0 + 2.0),
                        n(w),
                        n(li - 4.0),
                        self.col(h)
                    );
                }
                cx += w;
            }
            let mut spans = String::new();
            for r in line {
                let mut attrs = String::new();
                if r.style.bold || bold_all {
                    attrs.push_str(r#" font-weight="bold""#);
                }
                if r.style.italic {
                    attrs.push_str(r#" font-style="italic""#);
                }
                if r.style.underline || r.style.strike {
                    let _ = write!(
                        attrs,
                        r#" text-decoration="{}{}""#,
                        if r.style.underline { "underline" } else { "" },
                        if r.style.strike { " line-through" } else { "" }
                    );
                }
                if let Some(c) = r.style.color {
                    let _ = write!(attrs, r#" fill="{}""#, self.col(c));
                }
                let _ = write!(spans, "<tspan{attrs}>{}</tspan>", esc(&r.text));
            }
            let _ = write!(out, r#"<text x="{}" y="{}" text-anchor="{anchor}" dominant-baseline="central" xml:space="preserve">{spans}</text>"#, n(ax), n(y));
            y += li;
        }
        format!(r#"<g font-family="{}" font-size="{}" fill="{}">{out}</g>"#, label.family.css(), n(label.size), self.col(color))
    }

    fn boxed(&self, o: &Object, b: &BoxObj) -> String {
        let f = &b.frame;
        let (hw, hh) = (f.w / 2.0, f.h / 2.0);
        let st = &o.style;
        let c = f.center();
        let rot = if f.angle == 0.0 { String::new() } else { format!(" rotate({})", n(f.angle.to_degrees())) };
        let mut inner = String::new();
        let stroke = if st.width > 0.0 { self.stroke(st, false) } else { r#"stroke="none""#.to_owned() };
        let mut label_color = st.stroke;
        match b.geom {
            BoxGeom::Rect => {
                let radius = f64::from(st.roundness) * f.w.min(f.h) / 2.0;
                let _ = write!(
                    inner,
                    r#"<rect x="{}" y="{}" width="{}" height="{}" rx="{}" fill="{}" {stroke}/>"#,
                    n(-hw),
                    n(-hh),
                    n(f.w),
                    n(f.h),
                    n(radius),
                    self.fill(st.fill)
                );
            }
            BoxGeom::Ellipse => {
                let _ = write!(inner, r#"<ellipse rx="{}" ry="{}" fill="{}" {stroke}/>"#, n(hw), n(hh), self.fill(st.fill));
            }
            BoxGeom::Diamond | BoxGeom::Triangle => {
                let poly = b.geom.outline().polygon(f.w, f.h);
                let pts: Vec<String> = poly.iter().map(|p| format!("{},{}", n(p.x), n(p.y))).collect();
                let _ = write!(inner, r#"<polygon points="{}" fill="{}" {stroke}/>"#, pts.join(" "), self.fill(st.fill));
            }
            BoxGeom::Sticky => {
                let fill = self.col(st.fill.unwrap_or(NOTE_YELLOW));
                let _ = write!(
                    inner,
                    r#"<rect x="{}" y="{}" width="{}" height="{}" fill="rgba(0,0,0,.10)"/><rect x="{}" y="{}" width="{}" height="{}" fill="rgba(0,0,0,.08)"/><rect x="{}" y="{}" width="{}" height="{}" fill="{fill}"/>"#,
                    n(-hw + 4.0),
                    n(-hh + 8.0),
                    n(f.w),
                    n(f.h),
                    n(-hw + 2.0),
                    n(-hh + 4.0),
                    n(f.w),
                    n(f.h),
                    n(-hw),
                    n(-hh),
                    n(f.w),
                    n(f.h)
                );
                label_color = Color::Rgb(0x1e, 0x1e, 0x1e);
            }
            BoxGeom::Text => {}
        }
        if !b.label.text.is_empty() {
            let lay = b.layout(self.m);
            inner.push_str(&self.text(&lay, b.label_rect(), &b.label, label_color, false));
        }
        let op = if st.opacity < 1.0 { format!(r#" opacity="{}""#, n(f64::from(st.opacity))) } else { String::new() };
        format!(r#"<g transform="translate({} {}){rot}"{op}>{inner}</g>"#, n(c.x), n(c.y))
    }

    /// Pen-pressure stroke: a filled polygon whose width follows the pressure.
    fn pressure_pen(&self, o: &Object, pts: &[Pt], pressures: &[f32]) -> String {
        let st = &o.style;
        let (p, smooth_pressures) = crate::pen::smooth(pts, Some(pressures), 4);
        let widths: Vec<f64> = smooth_pressures.iter().map(|v| crate::pen::pressure_width(st.width, *v)).collect();
        let (left, right) = crate::pen::stroke_sides(&p, &widths);
        let color = self.col(st.stroke);
        let op = if st.opacity < 1.0 { format!(r#" opacity="{}""#, n(f64::from(st.opacity))) } else { String::new() };
        let mut outline = String::new();
        for (i, pt) in left.iter().chain(right.iter().rev()).enumerate() {
            let _ = write!(outline, "{}{} {}", if i == 0 { "M" } else { "L" }, n(pt.x), n(pt.y));
        }
        let (a, z) = (p[0], p[p.len() - 1]);
        format!(
            r#"<g fill="{color}"{op}><path d="{outline}Z"/><circle cx="{}" cy="{}" r="{}"/><circle cx="{}" cy="{}" r="{}"/></g>"#,
            n(a.x),
            n(a.y),
            n(widths[0] / 2.0),
            n(z.x),
            n(z.y),
            n(widths[widths.len() - 1] / 2.0)
        )
    }

    fn pen(&self, o: &Object, pts: &[Pt], highlighter: bool) -> String {
        let st = &o.style;
        let op = if st.opacity < 1.0 { format!(r#" opacity="{}""#, n(f64::from(st.opacity))) } else { String::new() };
        let Some(first) = pts.first() else { return String::new() };
        if pts.len() == 1 {
            return format!(r#"<circle cx="{}" cy="{}" r="{}" fill="{}"{op}/>"#, n(first.x), n(first.y), n(st.width / 2.0), self.col(st.stroke));
        }
        let mut d = format!("M{} {}", n(first.x), n(first.y));
        if pts.len() == 2 {
            let _ = write!(d, "L{} {}", n(pts[1].x), n(pts[1].y));
        } else {
            for i in 1..pts.len() - 1 {
                let mid = pts[i].lerp(pts[i + 1], 0.5);
                let _ = write!(d, "Q{} {} {} {}", n(pts[i].x), n(pts[i].y), n(mid.x), n(mid.y));
            }
            let l = pts[pts.len() - 1];
            let _ = write!(d, "L{} {}", n(l.x), n(l.y));
        }
        format!(r#"<path d="{d}" fill="none" {}{op}/>"#, self.stroke(st, highlighter))
    }

    fn connector(&self, o: &Object, c: &crate::connector::Connector) -> String {
        let Some(path) = self.page.connector_path(c) else { return String::new() };
        if path.len() < 2 {
            return String::new();
        }
        let st = &o.style;
        let op = if st.opacity < 1.0 { format!(r#" opacity="{}""#, n(f64::from(st.opacity))) } else { String::new() };
        let d: Vec<String> = path.iter().enumerate().map(|(i, p)| format!("{}{} {}", if i == 0 { "M" } else { "L" }, n(p.x), n(p.y))).collect();
        let mut out = format!(r#"<path d="{}" fill="none" {}/>"#, d.join(""), self.stroke(st, false));
        let size = c.head_size_on(st.width, &path);
        let col = self.col(st.stroke);
        let background = if self.dark { "#262b38" } else { "#ffffff" };
        let heads = [(c.head_start, c.head_start_ext, path[0], path[1]), (c.head_end, c.head_end_ext, path[path.len() - 1], path[path.len() - 2])];
        for (head, ext, tip, from) in heads {
            for part in head_glyph(tip, from, size, head, ext) {
                let list = |pts: &[crate::geom::Pt]| pts.iter().map(|p| format!("{},{}", n(p.x), n(p.y))).collect::<Vec<_>>().join(" ");
                match part {
                    HeadPart::Line(pts) => {
                        let _ = write!(
                            out,
                            r#"<polyline points="{}" fill="none" stroke="{col}" stroke-width="{}" stroke-linecap="round" stroke-linejoin="round"/>"#,
                            list(&pts),
                            n(st.width)
                        );
                    }
                    HeadPart::Polygon { pts, solid } => {
                        let fill = if solid { col.as_str() } else { background };
                        let _ = write!(
                            out,
                            r#"<polygon points="{}" fill="{fill}" stroke="{col}" stroke-width="{}" stroke-linejoin="round"/>"#,
                            list(&pts),
                            n(st.width)
                        );
                    }
                    HeadPart::Circle { center, radius, solid: true } => {
                        let _ = write!(out, r#"<circle cx="{}" cy="{}" r="{}" fill="{col}"/>"#, n(center.x), n(center.y), n(radius));
                    }
                    HeadPart::Circle { center, radius, solid: false } => {
                        let _ = write!(
                            out,
                            r#"<circle cx="{}" cy="{}" r="{}" fill="{background}" stroke="{col}" stroke-width="{}"/>"#,
                            n(center.x),
                            n(center.y),
                            n(radius),
                            n(st.width)
                        );
                    }
                }
            }
        }
        if let Some(l) = c.label.as_ref().filter(|l| !l.text.is_empty()) {
            let (mid, _) = path_midpoint(&path);
            let lay = layout_rich(&l.text, None, self.m, l.family, l.size);
            let area = Rect::new(-lay.width / 2.0, -lay.height / 2.0, lay.width, lay.height);
            let block = self.text(&lay, area, &Label { align: Align::Center, ..l.clone() }, st.stroke, false);
            let halo = if self.dark { "#262b38" } else { "#ffffff" };
            let _ = write!(
                out,
                r#"<g transform="translate({} {})" stroke="{halo}" stroke-width="5" stroke-linejoin="round" paint-order="stroke">{block}</g>"#,
                n(mid.x),
                n(mid.y)
            );
        }
        format!("<g{op}>{out}</g>")
    }

    fn table(&self, o: &Object, t: &Table) -> String {
        let st = &o.style;
        let rect = t.rect();
        let col = self.col(st.stroke);
        let line_w = st.width.max(1.0);
        let mut out = String::new();
        if t.header {
            let _ =
                write!(out, r#"<rect x="{}" y="{}" width="{}" height="{}" fill="{col}" fill-opacity="0.10"/>"#, n(rect.x), n(rect.y), n(rect.w), n(t.row_h[0]));
        }
        let _ = write!(
            out,
            r#"<rect x="{}" y="{}" width="{}" height="{}" fill="none" stroke="{col}" stroke-width="{}"/>"#,
            n(rect.x),
            n(rect.y),
            n(rect.w),
            n(rect.h),
            n(line_w)
        );
        let mut x = rect.x;
        for cw in &t.col_w[..t.cols() - 1] {
            x += cw;
            let _ = write!(
                out,
                r#"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="{col}" stroke-width="{}"/>"#,
                n(x),
                n(rect.y),
                n(x),
                n(rect.bottom()),
                n(line_w * 0.6)
            );
        }
        let mut y = rect.y;
        for rh in &t.row_h[..t.rows() - 1] {
            y += rh;
            let _ = write!(
                out,
                r#"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="{col}" stroke-width="{}"/>"#,
                n(rect.x),
                n(y),
                n(rect.right()),
                n(y),
                n(line_w * 0.6)
            );
        }
        let label = Label { text: RichText::default(), family: t.family, size: t.size, align: Align::Left };
        for ri in 0..t.rows() {
            for ci in 0..t.cols() {
                let Some(cell) = t.cell(ri, ci).filter(|c| !c.is_empty()) else { continue };
                let cr = t.cell_rect(ri, ci);
                let area = Rect::new(cr.x + 8.0, cr.y + 4.0, (cr.w - 16.0).max(1.0), (cr.h - 8.0).max(1.0));
                let lay = layout_rich(cell, Some(area.w), self.m, t.family, t.size);
                out.push_str(&self.text(&lay, area, &label, st.stroke, t.header && ri == 0));
            }
        }
        let op = if st.opacity < 1.0 { format!(r#" opacity="{}""#, n(f64::from(st.opacity))) } else { String::new() };
        format!("<g{op}>{out}</g>")
    }

    /// The object, in a group that references its own drop shadow filter when it has a shadow. The filter region is the object's box:
    /// a region as big as the drawing, repeated for every shadowed object, makes the PNG and PDF export grow with objects x page area.
    fn object(&self, o: &Object) -> String {
        let inner = self.plain_object(o);
        if !casts_shadow(o) || inner.is_empty() {
            return inner;
        }
        let k = self.shadows.get();
        self.shadows.set(k + 1);
        // given in user space (a straight line has no box to size a relative region on), with room for the offset, the blur and the stroke
        let r = self.page.bounds(o).expand(SHADOW_ROOM + o.style.width);
        format!(
            r##"<filter id="{SHADOW_FILTER}-{k}" filterUnits="userSpaceOnUse" x="{}" y="{}" width="{}" height="{}"><feDropShadow dx="4" dy="4" stdDeviation="2" flood-color="#000000" flood-opacity="{}"/></filter><g filter="url(#{SHADOW_FILTER}-{k})">{inner}</g>"##,
            n(r.x),
            n(r.y),
            n(r.w),
            n(r.h),
            if self.dark { "0.5" } else { "0.25" }
        )
    }

    fn plain_object(&self, o: &Object) -> String {
        match &o.kind {
            Kind::Box(b) => self.boxed(o, b),
            Kind::Pen(p) => match &p.pressures {
                Some(pr) if !p.highlighter && p.pts.len() >= 2 => self.pressure_pen(o, &p.pts, pr),
                _ => self.pen(o, &p.pts, p.highlighter),
            },
            Kind::Connector(c) => self.connector(o, c),
            Kind::Table(t) => self.table(o, t),
            Kind::Image(i) => {
                let Some(a) = self.doc.assets.get(&i.asset) else { return String::new() };
                let f = i.frame;
                // formula or monochrome icon: white SVG tinted with the object's stroke color
                let data = i.export_bytes(&a.mime, a.data.as_slice(), o.style.stroke.resolve(self.dark));
                let center = f.center();
                let rot = if f.angle == 0.0 { String::new() } else { format!(" rotate({})", n(f.angle.to_degrees())) };
                let op = if o.style.opacity < 1.0 { format!(r#" opacity="{}""#, n(f64::from(o.style.opacity))) } else { String::new() };
                format!(
                    r#"<g transform="translate({} {}){rot}"{op}><image x="{}" y="{}" width="{}" height="{}" preserveAspectRatio="none" href="data:{};base64,{}"/></g>"#,
                    n(center.x),
                    n(center.y),
                    n(-f.w / 2.0),
                    n(-f.h / 2.0),
                    n(f.w),
                    n(f.h),
                    esc(&a.mime),
                    STANDARD.encode(&data)
                )
            }
        }
    }
}

/// Prefix of the ids of the drop shadow filters.
const SHADOW_FILTER: &str = "ink-shadow";
/// Room around an object for its shadow (offset 4 and blur), on top of its stroke width.
const SHADOW_ROOM: f64 = 12.0;

/// Whether the object is drawn with a drop shadow: not a pen stroke, and not a sticky note (which has its own).
fn casts_shadow(o: &Object) -> bool {
    o.style.shadow && !matches!(&o.kind, Kind::Pen(_)) && !matches!(&o.kind, Kind::Box(b) if b.geom == BoxGeom::Sticky)
}

/// Value of the attribute `name` of the first tag in `tag`.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let from = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    tag[from..].split('"').next()
}

/// The SVG with each drop shadow filter replaced by a soft shadow made of plain shapes, for the PDF export: PDF converters ignore SVG
/// filters. The shadow is a few black, nearly transparent copies of the object (its alpha channel, through a mask) around the
/// offset (4, 4), which add up to the opacity of the filter; the PNG and SVG keep the filter.
#[must_use]
pub fn with_soft_shadows(svg: &str) -> String {
    /// Offsets of the copies around the offset of the shadow (the blur of the filter, 2, reached by the corners).
    const SPREAD: [(f64, f64); 5] = [(0.0, 0.0), (-2.0, -2.0), (2.0, -2.0), (-2.0, 2.0), (2.0, 2.0)];
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(i) = rest.find(&format!("<filter id=\"{SHADOW_FILTER}-")) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let parsed = (|| {
            let end = tail.find("</filter>")? + "</filter>".len();
            let filter = &tail[..end];
            let id = attr(filter, "id")?;
            let after = tail[end..].strip_prefix(&format!("<g filter=\"url(#{id})\">"))?;
            let (mut depth, mut pos) = (1, 0);
            while depth > 0 {
                let open = after[pos..].find("<g").filter(|&o| after[pos + o + 2..].starts_with([' ', '>', '/']));
                let close = after[pos..].find("</g>")?;
                match open {
                    Some(o) if o < close => (depth, pos) = (depth + 1, pos + o + 2),
                    _ => (depth, pos) = (depth - 1, pos + close + 4),
                }
            }
            let inner = &after[..pos - 4];
            let region: Vec<&str> = ["x", "y", "width", "height"].iter().map(|a| attr(filter, a)).collect::<Option<_>>()?;
            let total: f64 = attr(filter, "flood-opacity")?.parse().ok()?;
            Some((id, inner, region, total, tail.len() - after[pos..].len()))
        })();
        let Some((id, inner, region, total, used)) = parsed else {
            // not what `object` writes: leave the rest as it is
            out.push_str(tail);
            return out;
        };
        let (x, y, w, h) = (region[0], region[1], region[2], region[3]);
        // the five copies stack up to `total`: 1 - (1 - each)^5 = total
        let each = 1.0 - (1.0 - total).powf(1.0 / 5.0);
        let _ = write!(out, r#"<mask id="{id}" mask-type="alpha" maskUnits="userSpaceOnUse" x="{x}" y="{y}" width="{w}" height="{h}">{inner}</mask>"#);
        for (dx, dy) in SPREAD {
            let _ = write!(
                out,
                r##"<g transform="translate({} {})"><rect x="{x}" y="{y}" width="{w}" height="{h}" fill="#000000" opacity="{:.4}" mask="url(#{id})"/></g>"##,
                n(4.0 + dx),
                n(4.0 + dy),
                each
            );
        }
        out.push_str(inner);
        rest = &tail[used..];
    }
    out.push_str(rest);
    out
}

/// Exports a page (or only `ids`) to SVG. `None` when there is nothing to export.
pub fn export(doc: &Document, page: &Page, ids: Option<&[Id]>, m: &dyn Measurer, opts: &SvgOptions) -> Option<String> {
    let objs: Vec<&Object> = page.objects.iter().filter(|o| page.is_visible(o) && ids.is_none_or(|ids| ids.contains(&o.id))).collect();
    let mut bounds = objs.iter().map(|o| page.bounds(o)).reduce(|a, b| a.union(&b))?;
    let shadows = objs.iter().any(|o| casts_shadow(o));
    if shadows || objs.iter().any(|o| matches!(&o.kind, Kind::Box(b) if b.geom == BoxGeom::Sticky)) {
        bounds = bounds.expand(10.0);
    }
    let b = bounds.expand(opts.padding);
    let ctx = Ctx { dark: doc.background.preset.is_dark(), doc, page, m, shadows: std::cell::Cell::new(0) };
    let mut s = String::new();
    let _ = write!(
        s,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="{} {} {} {}">"#,
        b.w.ceil(),
        b.h.ceil(),
        n(b.x),
        n(b.y),
        n(b.w),
        n(b.h)
    );
    if !opts.transparent {
        let _ = write!(s, r#"<rect x="{}" y="{}" width="{}" height="{}" fill="{}"/>"#, n(b.x), n(b.y), n(b.w), n(b.h), doc.background.preset.hex());
    }
    for o in objs {
        s.push_str(&ctx.object(o));
    }
    s.push_str("</svg>");
    Some(s)
}

/// Quick version with the approximate measurer (command-line tools, tests).
#[must_use]
pub fn export_approx(doc: &Document, page: &Page) -> Option<String> {
    export(doc, page, None, &crate::text::ApproxMeasurer, &SvgOptions::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connector::{Arrowhead, Connector, End};
    use crate::geom::Frame;
    use crate::model::Document;

    fn doc_with(objs: Vec<Object>) -> Document {
        let mut d = Document::default();
        d.pages[0].objects = objs;
        d
    }

    #[test]
    fn empty_page_exports_nothing() {
        let d = Document::default();
        assert!(export_approx(&d, &d.pages[0]).is_none());
    }

    #[test]
    fn exports_wellformed_svg_with_all_kinds() {
        let mut label = Label::new("Hello <&> \"world\"", 20.0, Align::Center);
        label.text.restyle(0, 3, |s| {
            s.bold = true;
            s.highlight = Some(Color::rgb(255, 236, 153));
        });
        let a = Object::new_box(
            BoxGeom::Rect,
            Frame::new(0.0, 0.0, 200.0, 80.0),
            Style { fill: Some(Color::rgb(165, 216, 255)), roundness: 0.5, ..Default::default() },
            label,
        );
        let b = Object::new_box(
            BoxGeom::Sticky,
            Frame { x: 400.0, y: 0.0, w: 120.0, h: 120.0, angle: 0.2 },
            Style::default(),
            Label::new("Note", 18.0, Align::Center),
        );
        let mut conn = Connector::new(End::bound(a.id.clone()), End::bound(b.id.clone()));
        conn.head_start = Arrowhead::Dot;
        conn.label = Some(Label::new("yes", 14.0, Align::Center));
        let c = Object::new(Kind::Connector(conn), Style::default());
        let pen = Object::new_pen(vec![Pt::new(0.0, 200.0), Pt::new(20.0, 230.0), Pt::new(50.0, 190.0)], false, Style::default());
        let mut t = Table::new(Pt::new(0.0, 300.0), 2, 2);
        t.cell_mut(0, 0).unwrap().set_plain("Title");
        let tb = Object::new(Kind::Table(t), Style { width: 2.0, ..Default::default() });
        let doc = doc_with(vec![a, b, c, pen, tb]);
        let svg = export_approx(&doc, &doc.pages[0]).unwrap();
        assert!(svg.starts_with("<?xml"));
        assert!(svg.contains("&lt;&amp;&gt;"));
        assert!(svg.contains("font-weight=\"bold\""));
        assert!(svg.contains("<polyline") || svg.contains("<circle"));
        assert!(svg.contains("paint-order=\"stroke\""));
        assert!(svg.contains("rotate("));
        assert!(svg.ends_with("</svg>"));
        // balanced <g> tags
        assert_eq!(svg.matches("<g").count(), svg.matches("</g>").count());
    }

    #[test]
    fn selection_export_restricts_objects_and_dark_background_flips_ink() {
        let a = Object::new_box(BoxGeom::Rect, Frame::new(0.0, 0.0, 50.0, 50.0), Style::default(), Label::default());
        let b = Object::new_box(BoxGeom::Rect, Frame::new(1000.0, 0.0, 50.0, 50.0), Style::default(), Label::default());
        let ida = a.id.clone();
        let mut d = doc_with(vec![a, b]);
        d.background.preset = crate::model::BgPreset::Black;
        let svg = export(&d, &d.pages[0], Some(&[ida]), &crate::text::ApproxMeasurer, &SvgOptions { padding: 10.0, transparent: true }).unwrap();
        assert_eq!(svg.matches("<rect").count(), 1);
        assert!(svg.contains("#f1f3f5"));
        assert!(!svg.contains("#101114"), "transparent background: no background rectangle");
    }

    #[test]
    fn hidden_layer_is_not_exported() {
        let a = Object::new_box(BoxGeom::Rect, Frame::new(0.0, 0.0, 50.0, 50.0), Style::default(), Label::default());
        let mut d = doc_with(vec![a]);
        d.pages[0].layers[0].visible = false;
        assert!(export_approx(&d, &d.pages[0]).is_none());
    }

    #[test]
    fn a_larger_head_scale_draws_a_larger_arrowhead_in_the_export() {
        let head_width = |scale: f32| {
            let mut c = Connector::straight(Pt::new(0.0, 0.0), Pt::new(300.0, 0.0));
            c.set_head_end(Arrowhead::Triangle);
            c.set_head_scale(scale);
            let d = doc_with(vec![Object::new(Kind::Connector(c), Style::default())]);
            let svg = export_approx(&d, &d.pages[0]).unwrap();
            let pts = svg.split("<polygon points=\"").nth(1).and_then(|s| s.split('"').next()).expect("a head polygon").to_owned();
            let xs: Vec<f64> = pts.split(' ').filter_map(|p| p.split(',').next()?.parse().ok()).collect();
            xs.iter().copied().fold(f64::MIN, f64::max) - xs.iter().copied().fold(f64::MAX, f64::min)
        };
        let (normal, big) = (head_width(1.0), head_width(3.0));
        assert!((big / normal - 3.0).abs() < 0.05, "{normal} {big}");
    }

    #[test]
    fn the_shadow_filter_is_defined_once_and_used_by_the_objects_that_cast_one() {
        let shadowed = Style { shadow: true, ..Style::default() };
        let boxed = |x: f64, st: &Style| Object::new_box(BoxGeom::Rect, Frame::new(x, 0.0, 50.0, 50.0), st.clone(), Label::default());
        let sticky = Object::new_box(BoxGeom::Sticky, Frame::new(300.0, 0.0, 50.0, 50.0), shadowed.clone(), Label::default());
        let pen = Object::new_pen(vec![Pt::new(0.0, 100.0), Pt::new(40.0, 120.0)], false, shadowed.clone());
        let line = Object::new(Kind::Connector(Connector::straight(Pt::new(0.0, 200.0), Pt::new(100.0, 200.0))), shadowed.clone());
        let d = doc_with(vec![boxed(0.0, &shadowed), boxed(100.0, &shadowed), boxed(200.0, &Style::default()), sticky, pen, line]);
        let svg = export_approx(&d, &d.pages[0]).unwrap();
        assert_eq!(svg.matches("<filter").count(), 3, "one filter per shadowed object, sized to it");
        assert!(svg.contains("feDropShadow"));
        // the region is the object's, not the drawing's (the drawing is more than 350 wide)
        for f in svg.split("<filter ").skip(1) {
            let w: f64 = f.split("width=\"").nth(1).and_then(|s| s.split('"').next()).and_then(|s| s.parse().ok()).unwrap();
            assert!(w < 150.0, "filter region {w} wide");
        }
        assert_eq!(svg.matches("filter=\"url(#ink-shadow-").count(), 3, "two boxes and the connector; not the plain box, the sticky note or the pen");
        assert_eq!(svg.matches("<g").count(), svg.matches("</g>").count());
        // nothing shadowed: no filter at all, and the exported size is the one it always was
        let plain = doc_with(vec![boxed(0.0, &Style::default())]);
        let plain_svg = export_approx(&plain, &plain.pages[0]).unwrap();
        assert!(!plain_svg.contains("filter"));
        let shadow_only = doc_with(vec![boxed(0.0, &shadowed)]);
        let width_of = |svg: &str| svg.split("width=\"").nth(1).and_then(|s| s.split('"').next()).and_then(|w| w.parse::<f64>().ok()).unwrap();
        let (with, without) = (width_of(&export_approx(&shadow_only, &shadow_only.pages[0]).unwrap()), width_of(&plain_svg));
        assert!((with - without - 20.0).abs() < 1.5, "room left for the shadow: {with} {without}");
    }

    #[test]
    fn pressure_pen_exports_a_filled_outline() {
        let pts = vec![Pt::new(0.0, 0.0), Pt::new(20.0, 10.0), Pt::new(40.0, 0.0), Pt::new(60.0, 10.0)];
        let o = Object::new_pen_with_pressure(pts.clone(), vec![0.1, 0.9, 0.2, 0.8], false, Style { width: 6.0, ..Style::default() });
        let plain = Object::new_pen(pts, false, Style { width: 6.0, ..Style::default() });
        let d = doc_with(vec![o]);
        let svg = export_approx(&d, &d.pages[0]).unwrap();
        assert!(svg.contains("<path d=\"M") && svg.contains("Z\"/>") && svg.contains("<circle"), "{svg}");
        assert!(!svg.contains("stroke-width"), "filled, no constant-width stroke");
        let d2 = doc_with(vec![plain]);
        assert!(export_approx(&d2, &d2.pages[0]).unwrap().contains("stroke-width"));
    }
}
