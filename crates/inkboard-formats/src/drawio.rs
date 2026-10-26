//! draw.io / diagrams.net (`.drawio`, mxGraph XML).
//!
//! Export: uncompressed XML, one `<diagram>` per page. Bound connectors
//! stay bound (`source` / `target` attributes), pen strokes become SVG images.
//! Import: compressed or not, several pages, shapes, rich text, embedded images,
//! edges (bound or free, waypoints, arrows, labels).

use crate::common::{FormatError, IconResolver, Imported, NoIcons, Report, data_url, font_family, parse_color, parse_data_url, round2};
use crate::xml::{self, Node, esc};
use flate2::read::DeflateDecoder;
use inkboard_core::svg::{SvgOptions, export as export_svg};
use inkboard_core::text::Measurer;
use inkboard_core::{
    Align, Anchor, Arrowhead, Asset, AssetId, BgPreset, BoxGeom, BoxObj, Color, Connector, Dash, Document, End, FontFamily, Frame, HeadExt, Id, ImageObj, Kind,
    Label, Object, Page, Pattern, Pt, Rect, RichText, Route, Span, SpanStyle, Style, num,
};
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;
use std::io::Read;
use std::sync::Arc;

/* ===================================================================== Export */

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn rich_to_html(rt: &RichText) -> String {
    let mut out = String::new();
    for sp in &rt.0 {
        let mut open = String::new();
        let mut close = String::new();
        let mut css = String::new();
        if let Some(Color::Rgb(r, g, b)) = sp.style.color {
            let _ = write!(css, "color:#{r:02x}{g:02x}{b:02x};");
        }
        if let Some(Color::Rgb(r, g, b)) = sp.style.highlight {
            let _ = write!(css, "background-color:#{r:02x}{g:02x}{b:02x};");
        }
        if !css.is_empty() {
            let _ = write!(open, "<span style=\"{css}\">");
            close.insert_str(0, "</span>");
        }
        for (on, t) in [(sp.style.bold, "b"), (sp.style.italic, "i"), (sp.style.underline, "u"), (sp.style.strike, "s")] {
            if on {
                let _ = write!(open, "<{t}>");
                close.insert_str(0, &format!("</{t}>"));
            }
        }
        out.push_str(&open);
        out.push_str(&html_escape(&sp.text).replace('\n', "<br>"));
        out.push_str(&close);
    }
    out
}

fn hex(c: Color) -> String {
    c.resolve_hex(false)
}

fn dash_style(d: Dash) -> &'static str {
    match d {
        Dash::Solid => "",
        Dash::Dashed => "dashed=1;",
        Dash::Dotted => "dashed=1;dashPattern=1 4;",
    }
}

fn shadow_style(on: bool) -> &'static str {
    if on { "shadow=1;" } else { "" }
}

/// Size of an arrowhead in draw.io when `startSize` / `endSize` are absent.
const DEFAULT_ARROW_SIZE: f64 = 6.0;

/// draw.io name of an arrowhead and whether it is filled (`endFill` / `startFill`).
fn arrow_name(h: Arrowhead, ext: Option<HeadExt>) -> (&'static str, u8) {
    match ext {
        Some(HeadExt::Diamond) => ("diamond", 1),
        Some(HeadExt::DiamondOpen) => ("diamond", 0),
        Some(HeadExt::TriangleOpen) => ("block", 0),
        Some(HeadExt::CrowFoot) => ("ERmany", 1),
        Some(HeadExt::CrowFootOne) => ("ERoneToMany", 1),
        Some(HeadExt::CrowFootZero) => ("ERzeroToMany", 1),
        Some(HeadExt::OneOnly) => ("ERmandOne", 1),
        Some(HeadExt::ZeroOrOne) => ("ERzeroToOne", 1),
        None => (
            match h {
                Arrowhead::None => "none",
                Arrowhead::Arrow => "open",
                Arrowhead::Triangle => "block",
                Arrowhead::Dot => "oval",
                Arrowhead::Bar => "dash",
            },
            1,
        ),
    }
}

fn font_name(f: FontFamily) -> &'static str {
    match f {
        FontFamily::Sans => "Segoe UI",
        FontFamily::Hand => "Segoe Print",
        FontFamily::Serif => "Georgia",
        FontFamily::Mono => "Consolas",
    }
}

fn align_name(a: Align) -> &'static str {
    match a {
        Align::Left => "left",
        Align::Center => "center",
        Align::Right => "right",
    }
}

/// Cell id of an object.
fn cell_id(id: &Id) -> String {
    format!("tb-{}", id.as_str())
}

fn opacity_style(s: &Style) -> String {
    if s.opacity < 1.0 { format!("opacity={};", (s.opacity * 100.0).round()) } else { String::new() }
}

/// draw.io style of a shape.
fn box_style(bx: &BoxObj, st: &Style) -> String {
    let f = &bx.frame;
    let rot = if f.angle == 0.0 { String::new() } else { format!("rotation={};", round2(f.angle.to_degrees())) };
    let base = match bx.geom {
        BoxGeom::Rect => format!("rounded={};arcSize={};", u8::from(st.roundness > 0.0), (st.roundness * 50.0).round()),
        BoxGeom::Ellipse => "ellipse;".to_owned(),
        BoxGeom::Diamond => "rhombus;".to_owned(),
        BoxGeom::Triangle => "triangle;direction=north;".to_owned(),
        BoxGeom::Sticky => "rounded=0;shadow=1;tbType=sticky;".to_owned(),
        BoxGeom::Text => "text;spacing=0;verticalAlign=top;".to_owned(),
    };
    let (fill, stroke) = match bx.geom {
        BoxGeom::Text => ("fillColor=none;".to_owned(), "strokeColor=none;".to_owned()),
        BoxGeom::Sticky => (format!("fillColor={};", hex(st.fill.unwrap_or(inkboard_core::style::NOTE_YELLOW))), "strokeColor=none;".to_owned()),
        _ => (
            st.fill.map_or("fillColor=none;".to_owned(), |c| format!("fillColor={};", hex(c))),
            if st.width > 0.0 { format!("strokeColor={};strokeWidth={};", hex(st.stroke), st.width) } else { "strokeColor=none;".to_owned() },
        ),
    };
    let fc = if bx.geom == BoxGeom::Sticky { "#1E1E1E".to_owned() } else { hex(st.stroke) };
    let l = &bx.label;
    format!(
        "{base}whiteSpace=wrap;html=1;{fill}{stroke}{}{}fontFamily={};fontSize={};align={};{}fontColor={fc};{rot}{}",
        dash_style(st.dash),
        opacity_style(st),
        font_name(l.family),
        l.size,
        align_name(l.align),
        if bx.geom == BoxGeom::Text { "" } else { "verticalAlign=middle;" },
        shadow_style(bx.geom != BoxGeom::Sticky && st.shadow),
    )
}

/// Edge cell of a connector (`None` when it has no path); `(ox, oy)`: offset of the page.
fn connector_cell(page: &Page, c: &Connector, st: &Style, id: &str, (ox, oy): (f64, f64)) -> Option<String> {
    let path = page.connector_path(c)?;
    let mut style = String::from("html=1;rounded=0;");
    match c.route {
        Route::Elbow => style.push_str("edgeStyle=orthogonalEdgeStyle;"),
        Route::Curve => style.push_str("curved=1;"),
        Route::Straight => {}
    }
    let ((end_arrow, end_fill), (start_arrow, start_fill)) = (arrow_name(c.head_end, c.head_end_ext), arrow_name(c.head_start, c.head_start_ext));
    let _ = write!(
        style,
        "endArrow={end_arrow};startArrow={start_arrow};endFill={end_fill};startFill={start_fill};strokeColor={};strokeWidth={};{}{}",
        hex(st.stroke),
        st.width,
        dash_style(st.dash),
        opacity_style(st)
    );
    style.push_str(shadow_style(st.shadow));
    if c.has_custom_head_scale() {
        let size = (f64::from(c.head_scale) * DEFAULT_ARROW_SIZE).round();
        let _ = write!(style, "startSize={size};endSize={size};");
    }
    let mut attrs = String::new();
    let mut geom = String::from("<mxGeometry relative=\"1\" as=\"geometry\">");
    for (e, tag, name) in [(&c.start, "source", "sourcePoint"), (&c.end, "target", "targetPoint")] {
        match e {
            End::Bound { target, anchor } => {
                let _ = write!(attrs, " {tag}=\"{}\"", cell_id(target));
                if let Anchor::Frac { fx, fy } = anchor {
                    let p = if tag == "source" { "exit" } else { "entry" };
                    let _ = write!(style, "{p}X={};{p}Y={};", round2(*fx), round2(*fy));
                }
            }
            End::Free { .. } => {
                let p = if tag == "source" { path[0] } else { path[path.len() - 1] };
                let _ = write!(geom, "<mxPoint x=\"{}\" y=\"{}\" as=\"{name}\"/>", round2(p.x + ox), round2(p.y + oy));
            }
        }
    }
    if !c.via.is_empty() {
        geom.push_str("<Array as=\"points\">");
        for v in &c.via {
            let _ = write!(geom, "<mxPoint x=\"{}\" y=\"{}\"/>", round2(v.x + ox), round2(v.y + oy));
        }
        geom.push_str("</Array>");
    }
    geom.push_str("</mxGeometry>");
    let value = c.label.as_ref().map(|l| rich_to_html(&l.text)).unwrap_or_default();
    if let Some(l) = &c.label {
        let _ = write!(style, "fontFamily={};fontSize={};fontColor={};", font_name(l.family), l.size, hex(st.stroke));
    }
    Some(format!("<mxCell id=\"{id}\" value=\"{}\" style=\"{}\" edge=\"1\" parent=\"1\"{attrs}>{geom}</mxCell>", esc(&value), esc(&style)))
}

fn page_xml(doc: &Document, page: &Page, m: &dyn Measurer) -> String {
    let b = page.content_bounds().unwrap_or(Rect::new(0.0, 0.0, 100.0, 100.0));
    let (ox, oy) = (40.0 - b.x, 40.0 - b.y);
    let mut cells = String::new();
    let geo = |x: f64, y: f64, w: f64, h: f64| {
        format!("<mxGeometry x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" as=\"geometry\"/>", round2(x + ox), round2(y + oy), round2(w), round2(h))
    };
    let vertex = |cells: &mut String, id: &str, value: &str, style: &str, g: String| {
        let _ = write!(cells, "<mxCell id=\"{id}\" value=\"{}\" style=\"{}\" vertex=\"1\" parent=\"1\">{g}</mxCell>", esc(value), esc(style));
    };
    for o in page.objects.iter().filter(|o| page.is_visible(o)) {
        let id = cell_id(&o.id);
        let st = &o.style;
        match &o.kind {
            Kind::Box(bx) => {
                let f = &bx.frame;
                vertex(&mut cells, &id, &rich_to_html(&bx.label.text), &box_style(bx, st), geo(f.x, f.y, f.w, f.h));
            }
            Kind::Image(img) => {
                let Some(a) = doc.assets.get(&img.asset) else { continue };
                let f = &img.frame;
                let bytes = img.export_bytes(&a.mime, &a.data, st.stroke.resolve(doc.background.preset.is_dark()));
                let url = data_url(&a.mime, &bytes).replace(";base64,", ",");
                let rot = if f.angle == 0.0 { String::new() } else { format!("rotation={};", round2(f.angle.to_degrees())) };
                vertex(
                    &mut cells,
                    &id,
                    "",
                    &format!(
                        "shape=image;html=1;imageAspect=0;verticalLabelPosition=bottom;verticalAlign=top;image={url};{rot}{}{}",
                        opacity_style(st),
                        shadow_style(st.shadow)
                    ),
                    geo(f.x, f.y, f.w, f.h),
                );
            }
            Kind::Pen(_) => {
                let bb = page.bounds(o);
                let Some(svg) = export_svg(doc, page, Some(std::slice::from_ref(&o.id)), m, &SvgOptions { padding: 0.0, transparent: true }) else { continue };
                let url = data_url("image/svg+xml", svg.as_bytes()).replace(";base64,", ",");
                vertex(
                    &mut cells,
                    &id,
                    "",
                    &format!("shape=image;html=1;imageAspect=0;verticalLabelPosition=bottom;verticalAlign=top;tbType=pen;image={url};"),
                    geo(bb.x, bb.y, bb.w, bb.h),
                );
            }
            Kind::Table(t) => {
                // invisible outline carrying the table id: the end of a connector bound to the table
                if page.objects.iter().any(|p| matches!(&p.kind, Kind::Connector(c) if c.targets().any(|t| *t == o.id))) {
                    let bb = page.bounds(o);
                    vertex(&mut cells, &id, "", "rounded=0;html=1;fillColor=none;strokeColor=none;", geo(bb.x, bb.y, bb.w, bb.h));
                }
                for r in 0..t.rows() {
                    for c in 0..t.cols() {
                        let cr = t.cell_rect(r, c);
                        let text = t.cell(r, c).map(rich_to_html).unwrap_or_default();
                        let head = t.header && r == 0;
                        let style = format!(
                            "rounded=0;whiteSpace=wrap;html=1;fillColor={};strokeColor={};strokeWidth={};fontFamily={};fontSize={};align=left;spacingLeft=8;fontColor={};{}",
                            if head { "#8e8e8e1a".to_owned() } else { "none".to_owned() },
                            hex(st.stroke),
                            st.width.max(1.0),
                            font_name(t.family),
                            t.size,
                            hex(st.stroke),
                            if head { "fontStyle=1;" } else { "" }
                        );
                        vertex(&mut cells, &format!("{id}-{r}-{c}"), &text, &style, geo(cr.x, cr.y, cr.w, cr.h));
                    }
                }
            }
            Kind::Connector(c) => {
                if let Some(cell) = connector_cell(page, c, st, &id, (ox, oy)) {
                    cells.push_str(&cell);
                }
            }
        }
    }
    format!(
        "<diagram id=\"{}\" name=\"{}\"><mxGraphModel dx=\"1200\" dy=\"800\" grid=\"{}\" gridSize=\"10\" guides=\"1\" tooltips=\"1\" connect=\"1\" arrows=\"1\" fold=\"1\" page=\"0\" pageScale=\"1\" pageWidth=\"1169\" pageHeight=\"827\" math=\"0\" shadow=\"0\" background=\"{}\"><root><mxCell id=\"0\"/><mxCell id=\"1\" parent=\"0\"/>{cells}</root></mxGraphModel></diagram>",
        esc(page.id.as_str()),
        esc(&page.name),
        u8::from(doc.background.pattern != Pattern::None),
        doc.background.preset.hex()
    )
}

/// Exports all the document's pages in draw.io format (uncompressed XML).
pub fn export(doc: &Document, m: &dyn Measurer) -> String {
    let pages: String = doc.pages.iter().map(|p| page_xml(doc, p, m)).collect();
    format!("<mxfile host=\"Inkboard\" agent=\"Inkboard\" version=\"1.0\" type=\"device\">{pages}</mxfile>")
}

/* ===================================================================== Import */

#[derive(Debug, Default)]
struct Geo {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    source_pt: Option<Pt>,
    target_pt: Option<Pt>,
    points: Vec<Pt>,
}

#[derive(Debug)]
struct Cell {
    id: String,
    parent: String,
    value: String,
    style: HashMap<String, String>,
    names: Vec<String>,
    vertex: bool,
    edge: bool,
    source: Option<String>,
    target: Option<String>,
    geo: Option<Geo>,
}

fn parse_style(s: &str) -> (HashMap<String, String>, Vec<String>) {
    let mut m = HashMap::new();
    let mut names = Vec::new();
    for tok in s.split(';').filter(|t| !t.is_empty()) {
        match tok.split_once('=') {
            Some((k, v)) => {
                m.insert(k.to_owned(), v.to_owned());
            }
            None => names.push(tok.to_owned()),
        }
    }
    (m, names)
}

fn num(n: &Node, k: &str) -> f64 {
    n.attr(k).and_then(|v| v.parse().ok()).unwrap_or(0.0)
}

fn read_cell(cell: &Node, wrapper: Option<&Node>) -> Cell {
    let src = wrapper.unwrap_or(cell);
    let (style, names) = parse_style(cell.attr("style").unwrap_or(""));
    let geo = cell.child("mxGeometry").map(|g| {
        let mut geo = Geo { x: num(g, "x"), y: num(g, "y"), w: num(g, "width"), h: num(g, "height"), ..Geo::default() };
        for c in &g.children {
            let p = || Pt::new(num(c, "x"), num(c, "y"));
            match (c.name.as_str(), c.attr("as")) {
                ("mxPoint", Some("sourcePoint")) => geo.source_pt = Some(p()),
                ("mxPoint", Some("targetPoint")) => geo.target_pt = Some(p()),
                ("Array", _) => geo.points = c.children.iter().filter(|q| q.name == "mxPoint").map(|q| Pt::new(num(q, "x"), num(q, "y"))).collect(),
                _ => {}
            }
        }
        geo
    });
    Cell {
        id: src.attr("id").unwrap_or("").to_owned(),
        parent: cell.attr("parent").unwrap_or("").to_owned(),
        value: if wrapper.is_some() { src.attr("label").unwrap_or("") } else { cell.attr("value").unwrap_or("") }.to_owned(),
        style,
        names,
        vertex: cell.attr("vertex") == Some("1"),
        edge: cell.attr("edge") == Some("1"),
        source: cell.attr("source").map(str::to_owned),
        target: cell.attr("target").map(str::to_owned),
        geo,
    }
}

fn collect_cells(root: &Node) -> Vec<Cell> {
    let mut out = Vec::new();
    for c in &root.children {
        match c.name.as_str() {
            "mxCell" => out.push(read_cell(c, None)),
            "object" | "UserObject" => {
                if let Some(inner) = c.child("mxCell") {
                    out.push(read_cell(inner, Some(c)));
                }
            }
            _ => {}
        }
    }
    out
}

fn entity(name: &str) -> Option<char> {
    match name {
        "lt" => Some('<'),
        "gt" => Some('>'),
        "amp" => Some('&'),
        "quot" => Some('"'),
        "apos" | "#39" => Some('\''),
        "nbsp" => Some(' '),
        n if n.starts_with("#x") => u32::from_str_radix(&n[2..], 16).ok().and_then(char::from_u32),
        n if n.starts_with('#') => n[1..].parse().ok().and_then(char::from_u32),
        _ => None,
    }
}

/// Formatting started by an opening inline tag (`attr`: value of one of its attributes).
fn apply_tag(name: &str, attr: impl Fn(&str) -> Option<String>, cur: &mut SpanStyle) {
    match name {
        "b" | "strong" => cur.bold = true,
        "i" | "em" => cur.italic = true,
        "u" => cur.underline = true,
        "s" | "strike" | "del" => cur.strike = true,
        "font" => {
            if let Some(c) = attr("color").and_then(|c| parse_color(&c)) {
                cur.color = Some(c);
            }
        }
        "span" => {
            if let Some(style) = attr("style") {
                apply_css(&style, cur);
            }
        }
        _ => {}
    }
}

/// Formatting of an inline CSS `style` attribute (color, background, weight, style, decoration).
fn apply_css(style: &str, cur: &mut SpanStyle) {
    for decl in style.split(';') {
        let Some((k, v)) = decl.split_once(':') else { continue };
        let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
        match k.as_str() {
            "color" => cur.color = parse_color(v).or(cur.color),
            "background-color" | "background" => cur.highlight = parse_color(v),
            "font-weight" if v == "bold" || v.parse::<u32>().is_ok_and(|n| n >= 600) => cur.bold = true,
            "font-style" if v == "italic" => cur.italic = true,
            "text-decoration" | "text-decoration-line" => {
                cur.underline |= v.contains("underline");
                cur.strike |= v.contains("line-through");
            }
            _ => {}
        }
    }
}

/// Converts draw.io label HTML (common subset) to rich text.
#[must_use]
pub fn html_to_rich(h: &str) -> RichText {
    let mut spans: Vec<Span> = Vec::new();
    let mut stack: Vec<(String, SpanStyle)> = Vec::new();
    let mut cur = SpanStyle::default();
    let push = |spans: &mut Vec<Span>, c: char, st: SpanStyle| match spans.last_mut() {
        Some(l) if l.style == st => l.text.push(c),
        _ => spans.push(Span { text: c.to_string(), style: st }),
    };
    let ends_nl = |spans: &Vec<Span>| spans.last().is_none_or(|s| s.text.ends_with('\n'));
    let chars: Vec<char> = h.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '<' => {
                let Some(end) = chars[i..].iter().position(|c| *c == '>') else {
                    push(&mut spans, '<', cur);
                    i += 1;
                    continue;
                };
                let tag: String = chars[i + 1..i + end].iter().collect();
                i += end + 1;
                let closing = tag.starts_with('/');
                let body = tag.trim_start_matches('/').trim_end_matches('/');
                let name = body.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
                let attr = |key: &str| -> Option<String> {
                    let pat = format!("{key}=");
                    let p = body.to_ascii_lowercase().find(&pat)?;
                    let rest = &body[p + pat.len()..];
                    let q = rest.chars().next()?;
                    if q == '"' || q == '\'' { rest[1..].split(q).next().map(str::to_owned) } else { rest.split_whitespace().next().map(str::to_owned) }
                };
                if closing {
                    if let Some(pos) = stack.iter().rposition(|(n, _)| *n == name) {
                        cur = stack[pos].1;
                        stack.truncate(pos);
                    }
                    if matches!(name.as_str(), "div" | "p" | "li") && !ends_nl(&spans) {
                        push(&mut spans, '\n', cur);
                    }
                    continue;
                }
                match name.as_str() {
                    "br" => push(&mut spans, '\n', cur),
                    "div" | "p" | "li" | "h1" | "h2" | "h3" => {
                        if !ends_nl(&spans) {
                            push(&mut spans, '\n', cur);
                        }
                    }
                    "b" | "strong" | "i" | "em" | "u" | "s" | "strike" | "del" | "font" | "span" | "sub" | "sup" => {
                        stack.push((name.clone(), cur));
                        apply_tag(&name, attr, &mut cur);
                    }
                    _ => {}
                }
            }
            '&' => {
                if let Some(end) = chars[i..].iter().take(10).position(|c| *c == ';') {
                    let name: String = chars[i + 1..i + end].iter().collect();
                    if let Some(c) = entity(&name) {
                        push(&mut spans, c, cur);
                        i += end + 1;
                        continue;
                    }
                }
                push(&mut spans, '&', cur);
                i += 1;
            }
            c => {
                push(&mut spans, c, cur);
                i += 1;
            }
        }
    }
    while let Some(l) = spans.last_mut()
        && l.text.ends_with('\n')
    {
        l.text.pop();
        if l.text.is_empty() {
            spans.pop();
        }
    }
    RichText(spans)
}

/// Largest compressed diagram once inflated (a few kilobytes of deflate can stand for gigabytes).
const MAX_INFLATED_BYTES: u64 = 64 << 20;

fn inflate(raw: &str) -> Result<String, FormatError> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let bytes =
        STANDARD.decode(raw.split_whitespace().collect::<String>().as_bytes()).map_err(|_| FormatError::Invalid("unreadable compressed diagram".into()))?;
    let mut s = String::new();
    DeflateDecoder::new(bytes.as_slice()).take(MAX_INFLATED_BYTES + 1).read_to_string(&mut s).map_err(|_| FormatError::Invalid("cannot decompress".into()))?;
    if s.len() as u64 > MAX_INFLATED_BYTES {
        return Err(FormatError::Invalid("compressed diagram too large once decompressed".into()));
    }
    let decoded = percent_encoding::percent_decode_str(&s).decode_utf8_lossy().into_owned();
    Ok(decoded)
}

struct Builder<'a> {
    m: &'a dyn Measurer,
    icons: &'a dyn IconResolver,
    assets: std::collections::BTreeMap<AssetId, Asset>,
    report: Report,
}

fn style_for(c: &Cell, is_text: bool) -> Style {
    let s = &c.style;
    let width = if s.get("strokeColor").is_some_and(|v| v == "none") || is_text {
        0.0
    } else {
        (s.get("strokeWidth").and_then(|v| v.parse::<f64>().ok()).unwrap_or(1.0) * 2.0).max(2.0)
    };
    let font_c = s.get("fontColor").and_then(|v| parse_color(v));
    let stroke = if width == 0.0 { font_c } else { s.get("strokeColor").and_then(|v| parse_color(v)).or(font_c) }.unwrap_or(Color::Ink);
    Style {
        stroke,
        fill: if is_text { None } else { s.get("fillColor").and_then(|v| parse_color(v)) },
        width,
        dash: if s.get("dashed").is_some_and(|v| v == "1") {
            if s.get("dashPattern").is_some_and(|p| p.starts_with("1 ")) { Dash::Dotted } else { Dash::Dashed }
        } else {
            Dash::Solid
        },
        opacity: s.get("opacity").and_then(|v| v.parse::<f32>().ok()).map_or(1.0, |o| (o / 100.0).clamp(0.05, 1.0)),
        roundness: if s.get("rounded").is_some_and(|v| v == "1") {
            s.get("arcSize").and_then(|v| v.parse::<f32>().ok()).map_or(0.45, |a| (a / 50.0).clamp(0.05, 1.0))
        } else {
            0.0
        },
        // a sticky note always has its own shadow, which the export writes as `shadow=1`
        shadow: s.get("shadow").is_some_and(|v| v == "1") && s.get("tbType").is_none_or(|v| v != "sticky"),
    }
}

fn label_for(c: &Cell, default_align: Align) -> Label {
    let s = &c.style;
    let text = if s.get("html").is_some_and(|v| v == "1") { html_to_rich(&c.value) } else { RichText::plain(c.value.clone()) };
    Label {
        text,
        family: font_family(s.get("fontFamily").map_or("", String::as_str)),
        size: s.get("fontSize").and_then(|v| v.parse().ok()).unwrap_or(12.0),
        align: match s.get("align").map(String::as_str) {
            Some("left") => Align::Left,
            Some("right") => Align::Right,
            Some("center") => Align::Center,
            _ => default_align,
        },
    }
}

fn rect_of(r: &Rect) -> Frame {
    Frame::from_rect(*r)
}

impl Builder<'_> {
    fn page(&mut self, model: &Node, name: &str) -> Page {
        let mut page = Page::new(name);
        let Some(root) = model.child("root") else { return page };
        let cells = collect_cells(root);
        let by_id: HashMap<&str, &Cell> = cells.iter().map(|c| (c.id.as_str(), c)).collect();
        let origin = |c: &Cell| -> Pt {
            let (mut x, mut y) = (0.0, 0.0);
            let mut p = by_id.get(c.parent.as_str());
            let mut guard = 0;
            while let Some(pc) = p {
                if pc.id == "0" || pc.id == "1" || guard > 64 {
                    break;
                }
                if let (true, Some(g)) = (pc.vertex, &pc.geo) {
                    x += g.x;
                    y += g.y;
                }
                p = by_id.get(pc.parent.as_str());
                guard += 1;
            }
            Pt::new(x, y)
        };
        let mut ids: HashMap<String, Id> = HashMap::new();
        for c in cells.iter().filter(|c| c.vertex && c.id != "0" && c.id != "1") {
            let Some(g) = &c.geo else { continue };
            let o = origin(c);
            let rect = Rect::new(g.x + o.x, g.y + o.y, g.w, g.h);
            if let Some(obj) = self.vertex(c, rect, &mut page.objects) {
                ids.insert(c.id.clone(), obj);
            }
        }
        let rects: HashMap<&Id, Rect> = page.objects.iter().filter_map(|o| o.frame().map(|f| (&o.id, f.aabb()))).collect();
        let _ = rects;
        for c in cells.iter().filter(|c| c.edge) {
            self.edge(c, &origin(c), &ids, &mut page.objects);
        }
        page
    }

    /// Text of an image shape, placed below it (draw.io default), above it or over it according to `verticalLabelPosition`.
    fn image_label(&mut self, c: &Cell, r: Rect, out: &mut Vec<Object>) {
        let label = label_for(c, Align::Center);
        if label.text.is_empty() {
            return;
        }
        let mut tb = Object::new_box(BoxGeom::Text, Frame::new(0.0, 0.0, 10.0, 10.0), style_for(c, true), label);
        if let Kind::Box(b) = &mut tb.kind {
            b.fit(self.m);
            b.frame.x = r.x + r.w / 2.0 - b.frame.w / 2.0;
            b.frame.y = match c.style.get("verticalLabelPosition").map(String::as_str) {
                Some("top") => r.y - b.frame.h - 2.0,
                Some("middle") => r.y + r.h / 2.0 - b.frame.h / 2.0,
                _ => r.bottom() + 2.0,
            };
        }
        out.push(tb);
    }

    /// A built-in icon that is not in the icon library: a rectangle with its label (never lost), counted in `Report::placeholders`.
    fn placeholder(&mut self, c: &Cell, r: Rect, angle: f64, out: &mut Vec<Object>) -> Id {
        self.report.placeholders += 1;
        let mut frame = rect_of(&r);
        frame.angle = angle;
        let mut style = style_for(c, false);
        style.fill = style.fill.or(Some(Color::Rgb(0xdd, 0xdd, 0xdd)));
        style.stroke = Color::Ink;
        style.width = 2.0;
        let obj = Object::new_box(BoxGeom::Rect, frame, style, label_for(c, Align::Center));
        let id = obj.id.clone();
        out.push(obj);
        id
    }

    /// A free text cell (`None` when it holds no text).
    fn free_text(&self, c: &Cell, r: Rect, angle: f64) -> Option<Object> {
        let s = &c.style;
        let mut label = label_for(c, Align::Left);
        if label.text.is_empty() {
            return None;
        }
        label.align = match s.get("align").map(String::as_str) {
            Some("center") => Align::Center,
            Some("right") => Align::Right,
            _ => Align::Left,
        };
        let mut obj = Object::new_box(BoxGeom::Text, rect_of(&r), style_for(c, true), label);
        if let Kind::Box(b) = &mut obj.kind {
            b.fit(self.m);
            b.frame.x = match b.label.align {
                Align::Center => r.x + r.w / 2.0 - b.frame.w / 2.0,
                Align::Right => r.right() - b.frame.w,
                Align::Left => r.x,
            };
            b.frame.y = if s.get("verticalAlign").is_some_and(|v| v == "middle") { r.y + r.h / 2.0 - b.frame.h / 2.0 } else { r.y };
            b.frame.angle = angle;
        }
        Some(obj)
    }

    /// Moves the text of `label` to a separate text box along the top (or bottom) edge of `r`.
    fn edge_label(&self, label: &mut Label, r: Rect, top: bool, stroke: Color) -> Object {
        let mut t = Label { align: if label.align == Align::Center { Align::Center } else { label.align }, ..label.clone() };
        t.text = std::mem::take(&mut label.text);
        let mut tb = Object::new_box(BoxGeom::Text, Frame::new(0.0, 0.0, 10.0, 10.0), Style { stroke, width: 0.0, ..Style::default() }, t);
        if let Kind::Box(b) = &mut tb.kind {
            b.fit(self.m);
            b.frame.x = match b.label.align {
                Align::Left => r.x + 8.0,
                Align::Right => r.right() - b.frame.w - 8.0,
                Align::Center => r.x + r.w / 2.0 - b.frame.w / 2.0,
            };
            b.frame.y = if top { r.y + 6.0 } else { r.bottom() - b.frame.h - 6.0 };
        }
        tb
    }

    /// Creates the object of a vertex; returns its id when it can receive connectors.
    fn vertex(&mut self, c: &Cell, r: Rect, out: &mut Vec<Object>) -> Option<Id> {
        let s = &c.style;
        let shape = s.get("shape").map_or_else(|| c.names.first().map(|n| n.to_ascii_lowercase()).unwrap_or_default(), |v| v.to_ascii_lowercase());
        let angle = s.get("rotation").and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0).to_radians();
        // images
        if s.get("shape").is_some_and(|v| v == "image") || (s.contains_key("image") && !c.names.iter().any(|n| n == "text")) {
            let url = s.get("image").map_or("", String::as_str);
            let embedded = parse_data_url(&format!("data:{}", url.trim_start_matches("data:")));
            // built-in draw.io icon: the library's one, else a rectangle; the label of an image is kept as a text next to it
            let (mime, data, mono) = match embedded {
                Some((mime, data)) => (mime, data, false),
                None if url.contains("img/lib/") => match self.icons.resolve_drawio_image(url) {
                    Some((svg, mono)) => ("image/svg+xml".to_owned(), svg.into_bytes(), mono),
                    None => return Some(self.placeholder(c, r, angle, out)),
                },
                None => {
                    self.report.skipped += 1;
                    return None;
                }
            };
            let aid = Id::new();
            self.assets.insert(aid.clone(), Asset { mime, data: Arc::new(data) });
            let mut f = rect_of(&r);
            f.angle = angle;
            let obj = Object::new(
                Kind::Image(ImageObj { frame: f, asset: aid, latex: None, mono }),
                Style { width: 0.0, opacity: style_for(c, true).opacity, ..Style::default() },
            );
            let id = obj.id.clone();
            out.push(obj);
            self.image_label(c, r, out);
            return Some(id);
        }
        // free text
        if c.names.iter().any(|n| n == "text") || s.get("shape").is_some_and(|v| v == "text") {
            let obj = self.free_text(c, r, angle)?;
            let id = obj.id.clone();
            out.push(obj);
            return Some(id);
        }
        let mut geom = BoxGeom::Rect;
        let mut frame = rect_of(&r);
        let mut approx = false;
        if shape == "ellipse" || shape == "doubleellipse" || c.names.iter().any(|n| n == "ellipse") {
            geom = BoxGeom::Ellipse;
        } else if shape == "rhombus" || shape.contains("decision") || shape.contains("diamond") {
            geom = BoxGeom::Diamond;
        } else if shape == "triangle" {
            geom = BoxGeom::Triangle;
            let dir = s.get("direction").map_or("east", String::as_str);
            let center = r.center();
            let (a, swap) = match dir {
                "north" => (0.0, false),
                "east" => (std::f64::consts::FRAC_PI_2, true),
                "south" => (std::f64::consts::PI, false),
                _ => (-std::f64::consts::FRAC_PI_2, true),
            };
            let (box_w, box_h) = if swap { (r.h, r.w) } else { (r.w, r.h) };
            frame = Frame { x: center.x - box_w / 2.0, y: center.y - box_h / 2.0, w: box_w, h: box_h, angle: a };
        } else if shape == "note" || s.get("tbType").is_some_and(|v| v == "sticky") {
            geom = BoxGeom::Sticky;
        } else if shape.contains("terminator") || shape.contains("stadium") {
            // capsule
        } else if !shape.is_empty() && !matches!(shape.as_str(), "rectangle" | "process" | "mxgraph.basic.rect") && s.contains_key("shape")
            || shape == "swimlane"
        {
            approx = true;
        }
        frame.angle += angle;
        if approx {
            self.report.approximated += 1;
        }
        let mut style = style_for(c, false);
        if geom == BoxGeom::Sticky {
            style.fill = style.fill.or(Some(inkboard_core::style::NOTE_YELLOW));
            style.width = 0.0;
            style.stroke = Color::Ink;
        }
        if shape.contains("terminator") || shape.contains("stadium") {
            style.roundness = 1.0;
        }
        let mut label = label_for(c, Align::Center);
        let valign = s.get("verticalAlign").map_or("middle", String::as_str);
        let separate = (!label.text.is_empty() && (valign == "top" || valign == "bottom") && r.h >= 50.0)
            .then(|| self.edge_label(&mut label, r, valign == "top", style.stroke));
        let obj = Object::new_box(geom, frame, style, label);
        let id = obj.id.clone();
        out.push(obj);
        out.extend(separate);
        Some(id)
    }

    fn edge(&mut self, c: &Cell, origin: &Pt, ids: &HashMap<String, Id>, out: &mut Vec<Object>) {
        let Some(g) = &c.geo else { return };
        let s = &c.style;
        let end = |cell_id: &Option<String>, pt: Option<Pt>, ex: &str, ey: &str| -> Option<End> {
            if let Some(t) = cell_id.as_ref().and_then(|i| ids.get(i)) {
                let fx = s.get(ex).and_then(|v| v.parse::<f64>().ok());
                let fy = s.get(ey).and_then(|v| v.parse::<f64>().ok());
                return Some(End::Bound {
                    target: t.clone(),
                    anchor: match (fx, fy) {
                        (Some(fx), Some(fy)) => Anchor::Frac { fx, fy },
                        _ => Anchor::Auto,
                    },
                });
            }
            pt.map(|p| End::free(p + *origin))
        };
        let (Some(start), Some(finish)) = (end(&c.source, g.source_pt, "exitX", "exitY"), end(&c.target, g.target_pt, "entryX", "entryY")) else {
            self.report.skipped += 1;
            return;
        };
        // an arrowhead: an original one, or an extended one when draw.io has the symbol (the fill key tells a hollow one from a filled one)
        let head = |conn: &mut Connector, at_end: bool, key: &str, fill_key: &str, default: &str| {
            let hollow = s.get(fill_key).is_some_and(|v| v == "0");
            let ext = match s.get(key).map_or(default, String::as_str) {
                "diamond" | "diamondThin" => Some(if hollow { HeadExt::DiamondOpen } else { HeadExt::Diamond }),
                "block" | "blockThin" | "classic" | "classicThin" if hollow => Some(HeadExt::TriangleOpen),
                "ERmany" => Some(HeadExt::CrowFoot),
                "ERoneToMany" => Some(HeadExt::CrowFootOne),
                "ERzeroToMany" => Some(HeadExt::CrowFootZero),
                "ERmandOne" => Some(HeadExt::OneOnly),
                "ERzeroToOne" => Some(HeadExt::ZeroOrOne),
                _ => None,
            };
            let original = match s.get(key).map_or(default, String::as_str) {
                "none" => Arrowhead::None,
                "open" | "openThin" => Arrowhead::Arrow,
                "oval" | "ovalThin" => Arrowhead::Dot,
                "dash" => Arrowhead::Bar,
                _ => Arrowhead::Triangle,
            };
            match (ext, at_end) {
                (Some(e), true) => conn.set_ext_end(e),
                (Some(e), false) => conn.set_ext_start(e),
                (None, true) => conn.set_head_end(original),
                (None, false) => conn.set_head_start(original),
            }
        };
        let st = style_for(c, false);
        let mut conn = Connector::new(start, finish);
        head(&mut conn, true, "endArrow", "endFill", "classic");
        head(&mut conn, false, "startArrow", "startFill", "none");
        // draw.io sizes each head on its own; one factor is kept for both (the end one first)
        if let Some(size) = ["endSize", "startSize"].iter().find_map(|k| s.get(*k).and_then(|v| v.parse::<f64>().ok())).filter(|v| v.is_finite() && *v > 0.0) {
            conn.set_head_scale(num::f32(size / DEFAULT_ARROW_SIZE));
        }
        conn.via = g.points.iter().map(|p| *p + *origin).collect();
        let es = s.get("edgeStyle").map_or("", String::as_str);
        conn.route = if es.contains("orthogonal") || es.contains("elbow") {
            Route::Elbow
        } else if s.get("curved").is_some_and(|v| v == "1") {
            Route::Curve
        } else {
            Route::Straight
        };
        let label = label_for(c, Align::Center);
        if !label.text.is_empty() {
            conn.label = Some(label);
        }
        out.push(Object::new(Kind::Connector(conn), Style { fill: None, ..st }));
    }
}

/// Imports a draw.io file (compressed or not, one or several pages).
///
/// # Errors
///
/// [`FormatError`] when the content is not a draw.io diagram or cannot be decompressed.
pub fn import(content: &str, name: &str, m: &dyn Measurer) -> Result<Imported, FormatError> {
    import_with_icons(content, name, m, &NoIcons)
}

/// Like [`import`], resolving the built-in icons (`image=img/lib/...`) in `icons`.
///
/// # Errors
///
/// Same as [`import`].
pub fn import_with_icons(content: &str, name: &str, m: &dyn Measurer, icons: &dyn IconResolver) -> Result<Imported, FormatError> {
    let root = xml::parse(content).map_err(FormatError::Invalid)?;
    let mut diagrams: Vec<&Node> = Vec::new();
    if root.name == "mxGraphModel" {
        diagrams.push(&root);
    } else {
        root.find_all("diagram", &mut diagrams);
        if diagrams.is_empty() {
            return Err(FormatError::Invalid("no draw.io diagram".into()));
        }
    }
    let mut b = Builder { m, icons, assets: BTreeMap::default(), report: Report::default() };
    let mut doc = Document::new(name);
    doc.pages.clear();
    for (i, d) in diagrams.iter().enumerate() {
        let (model_owned, model): (Option<Node>, &Node) = if d.name == "mxGraphModel" {
            (None, d)
        } else if let Some(mm) = d.child("mxGraphModel") {
            (None, mm)
        } else {
            let inner = inflate(d.text.trim())?;
            let parsed = xml::parse(&inner).map_err(FormatError::Invalid)?;
            (Some(parsed), &Node::default())
        };
        let model = model_owned.as_ref().unwrap_or(model);
        let pname = d.attr("name").map_or_else(|| format!("Page {}", i + 1), str::to_owned);
        if i == 0
            && let Some(bg) = model.attr("background").and_then(parse_color)
        {
            doc.background.preset = BgPreset::nearest(bg.resolve(false));
        }
        let page = b.page(model, &pname);
        doc.pages.push(page);
    }
    doc.assets = b.assets;
    inkboard_core::serial::sanitize(&mut doc);
    Ok(Imported { doc, report: b.report })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample_document;
    use inkboard_core::text::ApproxMeasurer;

    fn kinds(p: &Page) -> Vec<&'static str> {
        p.objects
            .iter()
            .map(|o| match &o.kind {
                Kind::Box(b) => match b.geom {
                    BoxGeom::Rect => "rect",
                    BoxGeom::Ellipse => "ellipse",
                    BoxGeom::Diamond => "diamond",
                    BoxGeom::Triangle => "triangle",
                    BoxGeom::Sticky => "sticky",
                    BoxGeom::Text => "text",
                },
                Kind::Pen(_) => "pen",
                Kind::Connector(_) => "conn",
                Kind::Image(_) => "image",
                Kind::Table(_) => "table",
            })
            .collect()
    }

    #[test]
    fn export_is_wellformed_and_roundtrips_structure() {
        let d = sample_document();
        let xml = export(&d, &ApproxMeasurer);
        let root = crate::xml::parse(&xml).expect("well-formed XML");
        assert_eq!(root.name, "mxfile");
        let r = import(&xml, "x", &ApproxMeasurer).unwrap();
        let p = &r.doc.pages[0];
        // the pen comes back as an image, everything else keeps its nature
        let k = kinds(p);
        assert_eq!(k.iter().filter(|x| **x == "conn").count(), 2);
        for want in ["rect", "ellipse", "diamond", "triangle", "sticky", "text"] {
            assert!(k.contains(&want), "{want} missing from {k:?}");
        }
        assert_eq!(k.iter().filter(|x| **x == "image").count(), 2, "image + stroke converted to an image");
        assert_eq!(r.report.skipped, 0);
    }

    #[test]
    fn connectors_stay_bound_through_roundtrip() {
        let d = sample_document();
        let r = import(&export(&d, &ApproxMeasurer), "x", &ApproxMeasurer).unwrap();
        let p = &r.doc.pages[0];
        let bound: Vec<&Connector> = p.objects.iter().filter_map(|o| if let Kind::Connector(c) = &o.kind { Some(c) } else { None }).collect();
        assert!(matches!(bound[0].start, End::Bound { .. }) && matches!(bound[0].end, End::Bound { .. }));
        assert!(p.connector_path(bound[0]).is_some());
        assert!(matches!(bound[1].start, End::Bound { .. }) && matches!(bound[1].end, End::Free { .. }));
        assert_eq!(bound[0].label.as_ref().unwrap().text.to_plain(), "yes");
    }

    #[test]
    fn rich_text_and_style_survive() {
        let d = sample_document();
        let r = import(&export(&d, &ApproxMeasurer), "x", &ApproxMeasurer).unwrap();
        let rect =
            r.doc.pages[0].objects.iter().find_map(|o| if let Kind::Box(b) = &o.kind { (b.geom == BoxGeom::Rect).then_some((o, b)) } else { None }).unwrap();
        assert_eq!(rect.1.label.text.to_plain(), "Start\nline 2");
        assert!(rect.1.label.text.0[0].style.bold, "bold kept: {:?}", rect.1.label.text);
        assert_eq!(rect.0.style.fill, Some(Color::Rgb(165, 216, 255)));
        assert!(rect.0.style.roundness > 0.3);
        let ell =
            r.doc.pages[0].objects.iter().find_map(|o| if let Kind::Box(b) = &o.kind { (b.geom == BoxGeom::Ellipse).then_some((o, b)) } else { None }).unwrap();
        assert_eq!(ell.0.style.dash, Dash::Dashed);
        assert!((ell.1.frame.angle - 0.3).abs() < 0.01, "rotation kept");
    }

    #[test]
    fn imports_foreign_drawio_with_edges_waypoints_and_unknown_shapes() {
        let src = r##"<mxfile><diagram name="P"><mxGraphModel background="#ffffff"><root><mxCell id="0"/><mxCell id="1" parent="0"/>
 <mxCell id="a" value="Start" style="ellipse;whiteSpace=wrap;html=1;fillColor=#d5e8d4;strokeColor=#82b366;" vertex="1" parent="1"><mxGeometry x="40" y="40" width="80" height="80" as="geometry"/></mxCell>
 <mxCell id="b" value="&lt;b&gt;Step&lt;/b&gt;&lt;br&gt;2" style="rounded=1;whiteSpace=wrap;html=1;" vertex="1" parent="1"><mxGeometry x="240" y="50" width="120" height="60" as="geometry"/></mxCell>
 <mxCell id="c" value="Cyl" style="shape=cylinder3;whiteSpace=wrap;html=1;" vertex="1" parent="1"><mxGeometry x="440" y="40" width="70" height="90" as="geometry"/></mxCell>
 <object label="Object" id="d"><mxCell style="rhombus;whiteSpace=wrap;html=1;" vertex="1" parent="1"><mxGeometry x="40" y="200" width="100" height="80" as="geometry"/></mxCell></object>
 <mxCell id="t" value="Title" style="text;html=1;align=center;verticalAlign=middle;" vertex="1" parent="1"><mxGeometry x="200" y="0" width="100" height="30" as="geometry"/></mxCell>
 <mxCell id="e1" value="go" style="edgeStyle=orthogonalEdgeStyle;rounded=0;html=1;" edge="1" parent="1" source="a" target="b"><mxGeometry relative="1" as="geometry"/></mxCell>
 <mxCell id="e2" style="endArrow=none;startArrow=oval;dashed=1;html=1;" edge="1" parent="1" source="b" target="c"><mxGeometry relative="1" as="geometry"><Array as="points"><mxPoint x="400" y="160"/></Array></mxGeometry></mxCell>
 <mxCell id="e3" style="html=1;" edge="1" parent="1"><mxGeometry relative="1" as="geometry"><mxPoint x="10" y="300" as="sourcePoint"/><mxPoint x="200" y="300" as="targetPoint"/></mxGeometry></mxCell>
 <mxCell id="e4" style="html=1;" edge="1" parent="1" source="zzz"><mxGeometry relative="1" as="geometry"/></mxCell>
 </root></mxGraphModel></diagram></mxfile>"##;
        let r = import(src, "ext", &ApproxMeasurer).unwrap();
        let p = &r.doc.pages[0];
        assert_eq!(p.name, "P");
        assert_eq!(r.report.approximated, 1, "cylinder approximated");
        assert_eq!(r.report.skipped, 1, "edge without ends");
        let conns: Vec<&Connector> = p.objects.iter().filter_map(|o| if let Kind::Connector(c) = &o.kind { Some(c) } else { None }).collect();
        assert_eq!(conns.len(), 3);
        assert_eq!(conns[0].route, Route::Elbow);
        assert_eq!(conns[0].head_end, Arrowhead::Triangle, "classic by default");
        assert_eq!(conns[0].label.as_ref().unwrap().text.to_plain(), "go");
        assert_eq!(conns[1].head_end, Arrowhead::None);
        assert_eq!(conns[1].head_start, Arrowhead::Dot);
        assert_eq!(conns[1].via, vec![Pt::new(400.0, 160.0)]);
        assert!(matches!(conns[2].start, End::Free { .. }));
        // rich text (bold) of step 2 and a wrapped object
        let b = p
            .objects
            .iter()
            .find_map(|o| if let Kind::Box(b) = &o.kind { (b.label.text.to_plain() == "Step\n2").then_some(b) } else { None })
            .expect("HTML label converted");
        assert!(b.label.text.0[0].style.bold);
        assert!(p.objects.iter().any(|o| matches!(&o.kind, Kind::Box(b) if b.label.text.to_plain() == "Object" && b.geom == BoxGeom::Diamond)));
        assert!(p.objects.iter().any(|o| matches!(&o.kind, Kind::Box(b) if b.geom == BoxGeom::Text && b.label.text.to_plain() == "Title")));
        assert_eq!(r.doc.background.preset, BgPreset::White);
    }

    #[test]
    fn imports_compressed_diagrams() {
        use base64::{Engine, engine::general_purpose::STANDARD};
        use flate2::{Compression, write::DeflateEncoder};
        use std::io::Write;
        let inner = r#"<mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/><mxCell id="a" value="Zip é" style="rounded=0;" vertex="1" parent="1"><mxGeometry x="0" y="0" width="100" height="60" as="geometry"/></mxCell></root></mxGraphModel>"#;
        let enc = percent_encoding::utf8_percent_encode(inner, percent_encoding::NON_ALPHANUMERIC).to_string();
        let mut z = DeflateEncoder::new(Vec::new(), Compression::default());
        z.write_all(enc.as_bytes()).unwrap();
        let packed = STANDARD.encode(z.finish().unwrap());
        let src = format!("<mxfile><diagram name=\"Compressed\" id=\"x\">{packed}</diagram></mxfile>");
        let r = import(&src, "z", &ApproxMeasurer).unwrap();
        assert_eq!(r.doc.pages[0].name, "Compressed");
        assert!(matches!(&r.doc.pages[0].objects[0].kind, Kind::Box(b) if b.label.text.to_plain() == "Zip é"));
        assert!(import("<mxfile><diagram>!!!not-base64!!!</diagram></mxfile>", "z", &ApproxMeasurer).is_err());
    }

    #[test]
    fn multiple_pages_roundtrip_and_containers_offset_children() {
        let mut d = sample_document();
        let mut p2 = Page::new("Second");
        p2.objects.push(Object::new_box(BoxGeom::Rect, Frame::new(5.0, 5.0, 50.0, 50.0), Style::default(), Label::new("p2", 20.0, Align::Center)));
        d.pages.push(p2);
        let r = import(&export(&d, &ApproxMeasurer), "x", &ApproxMeasurer).unwrap();
        assert_eq!(r.doc.pages.len(), 2);
        assert_eq!(r.doc.pages[1].name, "Second");
        let src = r#"<mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/>
          <mxCell id="g" value="" style="swimlane;" vertex="1" parent="1"><mxGeometry x="100" y="100" width="300" height="200" as="geometry"/></mxCell>
          <mxCell id="k" value="enfant" style="rounded=0;" vertex="1" parent="g"><mxGeometry x="20" y="40" width="80" height="40" as="geometry"/></mxCell></root></mxGraphModel>"#;
        let r = import(src, "x", &ApproxMeasurer).unwrap();
        let child = r.doc.pages[0]
            .objects
            .iter()
            .find_map(|o| if let Kind::Box(b) = &o.kind { (b.label.text.to_plain() == "enfant").then_some(b.frame) } else { None })
            .unwrap();
        assert_eq!((child.x, child.y), (120.0, 140.0), "coordinates relative to the container");
    }

    #[test]
    fn html_conversion_handles_entities_and_blocks() {
        let r = html_to_rich(
            "a &lt;b&gt; &amp; c<div>line 2</div><span style=\"color:#ff0000;background-color:#ffff00\">red</span><font color=\"#00ff00\">green</font>",
        );
        assert_eq!(r.to_plain(), "a <b> & c\nline 2\nredgreen");
        let red = r.0.iter().find(|s| s.text == "red").unwrap();
        assert_eq!(red.style.color, Some(Color::Rgb(255, 0, 0)));
        assert!(red.style.highlight.is_some());
        assert_eq!(r.0.iter().find(|s| s.text == "green").unwrap().style.color, Some(Color::Rgb(0, 255, 0)));
        assert!(html_to_rich("<b>x</b>").0[0].style.bold);
        assert!(html_to_rich("").is_empty());
    }

    /// One icon only: `img/lib/azure2/compute/Virtual_Machine.svg`.
    struct OneIcon;

    impl IconResolver for OneIcon {
        fn resolve_drawio_image(&self, path: &str) -> Option<(String, bool)> {
            (path == "img/lib/azure2/compute/Virtual_Machine.svg").then(|| ("<svg viewBox=\"0 0 10 10\"/>".to_owned(), false))
        }
    }

    const BUILT_IN: &str = r#"<mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/>
 <mxCell id="vm" value="SEA_VM_1" style="aspect=fixed;html=1;align=center;image;fontSize=12;image=img/lib/azure2/compute/Virtual_Machine.svg;fontColor=#336699;" vertex="1" parent="1"><mxGeometry x="100" y="100" width="68" height="68" as="geometry"/></mxCell>
 <mxCell id="no" value="Unknown" style="aspect=fixed;html=1;image;image=img/lib/azure2/compute/Nope.svg;" vertex="1" parent="1"><mxGeometry x="300" y="100" width="60" height="40" as="geometry"/></mxCell>
 <mxCell id="e" style="html=1;" edge="1" parent="1" source="vm" target="no"><mxGeometry relative="1" as="geometry"/></mxCell>
 </root></mxGraphModel>"#;

    fn texts(p: &Page) -> Vec<String> {
        p.objects.iter().filter_map(|o| if let Kind::Box(b) = &o.kind { Some(b.label.text.to_plain()) } else { None }).collect()
    }

    #[test]
    fn a_built_in_icon_found_in_the_library_becomes_an_image_with_its_label_below() {
        let r = import_with_icons(BUILT_IN, "x", &ApproxMeasurer, &OneIcon).unwrap();
        let p = &r.doc.pages[0];
        let img = p.objects.iter().find_map(|o| if let Kind::Image(i) = &o.kind { Some(i) } else { None }).expect("image object");
        assert_eq!(img.frame.aabb(), Rect::new(100.0, 100.0, 68.0, 68.0));
        let asset = &r.doc.assets[&img.asset];
        assert_eq!(asset.mime, "image/svg+xml");
        assert!(asset.data.starts_with(b"<svg"));
        let label =
            p.objects.iter().find_map(|o| if let Kind::Box(b) = &o.kind { (b.label.text.to_plain() == "SEA_VM_1").then_some((b, &o.style)) } else { None });
        let (label, style) = label.expect("label kept");
        assert!(label.frame.y >= 168.0, "under the icon");
        assert_eq!(style.stroke, parse_color("#336699").unwrap(), "font color");
        assert!(p.objects.iter().any(|o| matches!(&o.kind, Kind::Connector(c) if c.targets().count() == 2)), "connectors stay bound");
        assert_eq!((r.report.skipped, r.report.placeholders), (0, 1), "only the unknown icon is a placeholder");
    }

    #[test]
    fn a_built_in_icon_missing_from_the_library_becomes_a_rectangle_with_its_label() {
        let r = import(BUILT_IN, "x", &ApproxMeasurer).unwrap();
        let p = &r.doc.pages[0];
        assert!(!p.objects.iter().any(|o| matches!(o.kind, Kind::Image(_))));
        let mut got = texts(p);
        got.sort();
        assert_eq!(got, ["SEA_VM_1", "Unknown"], "no label lost");
        let rect = p.objects.iter().find_map(|o| if let Kind::Box(b) = &o.kind { (b.label.text.to_plain() == "Unknown").then_some(b) } else { None }).unwrap();
        assert_eq!((rect.geom, rect.frame.aabb()), (BoxGeom::Rect, Rect::new(300.0, 100.0, 60.0, 40.0)));
        assert!(p.objects.iter().any(|o| matches!(&o.kind, Kind::Connector(c) if c.targets().count() == 2)), "connectors stay bound");
        assert_eq!((r.report.skipped, r.report.placeholders), (0, 2));
        assert!(r.report.summary().contains("2 built-in icon(s) replaced by a placeholder"));
    }

    #[test]
    fn an_image_without_a_usable_source_is_still_skipped() {
        let src = r#"<mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/>
 <mxCell id="a" value="Web" style="shape=image;image=https://example.com/a.png;" vertex="1" parent="1"><mxGeometry x="0" y="0" width="40" height="40" as="geometry"/></mxCell>
 </root></mxGraphModel>"#;
        let r = import(src, "x", &ApproxMeasurer).unwrap();
        assert_eq!((r.report.skipped, r.report.placeholders), (1, 0));
    }

    /// A page with two shapes and one connector per extended head, each at both ends.
    fn doc_with_every_extended_head() -> (inkboard_core::Document, Vec<HeadExt>) {
        let mut d = inkboard_core::Document::new("heads");
        let boxes: Vec<Object> =
            (0..2).map(|i| Object::new_box(BoxGeom::Rect, Frame::new(f64::from(i) * 300.0, 0.0, 80.0, 40.0), Style::default(), Label::default())).collect();
        let ids: Vec<inkboard_core::Id> = boxes.iter().map(|b| b.id.clone()).collect();
        d.pages[0].objects.extend(boxes);
        for ext in HeadExt::ALL {
            let mut c = Connector::new(End::bound(ids[0].clone()), End::bound(ids[1].clone()));
            c.set_ext_start(ext);
            c.set_ext_end(ext);
            d.pages[0].objects.push(Object::new(Kind::Connector(c), Style::default()));
        }
        (d, HeadExt::ALL.to_vec())
    }

    #[test]
    fn extended_heads_use_the_draw_io_symbols_and_come_back_as_extended_heads() {
        let (d, heads) = doc_with_every_extended_head();
        let xml = export(&d, &ApproxMeasurer);
        for (name, fill) in
            [("diamond", 1), ("diamond", 0), ("block", 0), ("ERmany", 1), ("ERoneToMany", 1), ("ERzeroToMany", 1), ("ERmandOne", 1), ("ERzeroToOne", 1)]
        {
            assert!(xml.contains(&format!("endArrow={name};")) && xml.contains(&format!("endFill={fill};")), "{name} {fill}");
        }
        let r = import(&xml, "x", &ApproxMeasurer).unwrap();
        let back: Vec<(Option<HeadExt>, Option<HeadExt>)> = r.doc.pages[0]
            .objects
            .iter()
            .filter_map(|o| if let Kind::Connector(c) = &o.kind { Some((c.head_start_ext, c.head_end_ext)) } else { None })
            .collect();
        assert_eq!(back, heads.iter().map(|h| (Some(*h), Some(*h))).collect::<Vec<_>>(), "every head is read back as it was written");
    }

    #[test]
    fn arrowhead_size_goes_through_start_size_and_end_size_and_the_default_writes_nothing() {
        let mut d = Document::default();
        let mut big = Connector::straight(Pt::new(0.0, 0.0), Pt::new(200.0, 0.0));
        big.set_head_scale(2.0);
        let plain = Connector::straight(Pt::new(0.0, 50.0), Pt::new(200.0, 50.0));
        d.pages[0].objects = vec![Object::new(Kind::Connector(big), Style::default()), Object::new(Kind::Connector(plain), Style::default())];
        let xml = export(&d, &ApproxMeasurer);
        assert_eq!(xml.matches("endSize=12;").count(), 1, "twice the draw.io default of 6, for the changed connector only");
        assert_eq!(xml.matches("startSize=12;").count(), 1);
        let r = import(&xml, "x", &ApproxMeasurer).unwrap();
        let scales: Vec<f32> = r.doc.pages[0].objects.iter().filter_map(|o| if let Kind::Connector(c) = &o.kind { Some(c.head_scale) } else { None }).collect();
        assert_eq!(scales, [2.0, 1.0]);
        // a foreign size is converted from the default of 6 and kept within the allowed range
        let src = r##"<mxfile><diagram name="P"><mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/>
 <mxCell id="e1" style="endArrow=block;endSize=3;html=1;" edge="1" parent="1"><mxGeometry relative="1" as="geometry"><mxPoint x="0" y="0" as="sourcePoint"/><mxPoint x="90" y="0" as="targetPoint"/></mxGeometry></mxCell>
 <mxCell id="e2" style="endArrow=block;endSize=300;html=1;" edge="1" parent="1"><mxGeometry relative="1" as="geometry"><mxPoint x="0" y="9" as="sourcePoint"/><mxPoint x="90" y="9" as="targetPoint"/></mxGeometry></mxCell>
 </root></mxGraphModel></diagram></mxfile>"##;
        let r = import(src, "x", &ApproxMeasurer).unwrap();
        let scales: Vec<f32> = r.doc.pages[0].objects.iter().filter_map(|o| if let Kind::Connector(c) = &o.kind { Some(c.head_scale) } else { None }).collect();
        assert_eq!(scales, [0.5, 4.0]);
    }

    #[test]
    fn drop_shadow_goes_through_shadow_1_and_a_sticky_note_keeps_its_own() {
        let shadowed = Style { shadow: true, ..Style::default() };
        let rect = |y: f64, st: &Style| Object::new_box(BoxGeom::Rect, Frame::new(0.0, y, 80.0, 40.0), st.clone(), Label::default());
        let sticky = Object::new_box(BoxGeom::Sticky, Frame::new(0.0, 200.0, 80.0, 80.0), Style::default(), Label::default());
        let line = Object::new(Kind::Connector(Connector::straight(Pt::new(0.0, 300.0), Pt::new(100.0, 300.0))), shadowed.clone());
        let mut d = Document::default();
        d.pages[0].objects = vec![rect(0.0, &shadowed), rect(100.0, &Style::default()), sticky, line];
        let xml = export(&d, &ApproxMeasurer);
        assert_eq!(xml.matches("shadow=1;").count(), 3, "the shadowed box and connector, and the sticky note as always");
        let r = import(&xml, "x", &ApproxMeasurer).unwrap();
        let flags: Vec<bool> = r.doc.pages[0].objects.iter().map(|o| o.style.shadow).collect();
        assert_eq!(flags, [true, false, false, true], "the sticky note's shadow is its own, not a setting");
        // a file from draw.io
        let src = r##"<mxfile><diagram name="P"><mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/>
 <mxCell id="a" value="A" style="rounded=0;shadow=1;" vertex="1" parent="1"><mxGeometry x="0" y="0" width="80" height="40" as="geometry"/></mxCell>
 <mxCell id="b" value="B" style="rounded=0;shadow=0;" vertex="1" parent="1"><mxGeometry x="200" y="0" width="80" height="40" as="geometry"/></mxCell>
 </root></mxGraphModel></diagram></mxfile>"##;
        let r = import(src, "x", &ApproxMeasurer).unwrap();
        assert_eq!(r.doc.pages[0].objects.iter().map(|o| o.style.shadow).collect::<Vec<_>>(), [true, false]);
    }

    #[test]
    fn foreign_draw_io_er_arrows_and_hollow_ones_are_read_as_extended_heads_and_the_others_as_before() {
        let src = r##"<mxfile><diagram name="P"><mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/>
 <mxCell id="a" value="A" style="rounded=0;" vertex="1" parent="1"><mxGeometry x="0" y="0" width="80" height="40" as="geometry"/></mxCell>
 <mxCell id="b" value="B" style="rounded=0;" vertex="1" parent="1"><mxGeometry x="300" y="0" width="80" height="40" as="geometry"/></mxCell>
 <mxCell id="e1" style="endArrow=ERmany;startArrow=ERmandOne;html=1;" edge="1" parent="1" source="a" target="b"><mxGeometry relative="1" as="geometry"/></mxCell>
 <mxCell id="e2" style="endArrow=diamondThin;endFill=0;startArrow=block;startFill=0;html=1;" edge="1" parent="1" source="a" target="b"><mxGeometry relative="1" as="geometry"/></mxCell>
 <mxCell id="e3" style="endArrow=block;endFill=1;startArrow=oval;html=1;" edge="1" parent="1" source="a" target="b"><mxGeometry relative="1" as="geometry"/></mxCell>
 </root></mxGraphModel></diagram></mxfile>"##;
        let r = import(src, "x", &ApproxMeasurer).unwrap();
        let c: Vec<&Connector> = r.doc.pages[0].objects.iter().filter_map(|o| if let Kind::Connector(c) = &o.kind { Some(c) } else { None }).collect();
        assert_eq!((c[0].head_end_ext, c[0].head_start_ext), (Some(HeadExt::CrowFoot), Some(HeadExt::OneOnly)));
        assert_eq!((c[1].head_end_ext, c[1].head_start_ext), (Some(HeadExt::DiamondOpen), Some(HeadExt::TriangleOpen)));
        assert_eq!(
            (c[2].head_end, c[2].head_end_ext, c[2].head_start, c[2].head_start_ext),
            (Arrowhead::Triangle, None, Arrowhead::Dot, None),
            "filled block and oval stay original heads"
        );
    }
}
