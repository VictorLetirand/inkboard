//! draw.io / diagrams.net (`.drawio`, mxGraph XML).
//!
//! Export : XML non compressé, une `<diagram>` par page. Les connecteurs liés
//! restent liés (attributs `source` / `target`), les traits au crayon deviennent des images SVG.
//! Import : XML compressé ou non, plusieurs pages, formes, texte riche, images intégrées,
//! arêtes (liées ou libres, points de passage, flèches, étiquettes).

use crate::common::{FormatError, Imported, Report, data_url, font_family, parse_color, parse_data_url, round2};
use crate::xml::{self, Node, esc};
use flate2::read::DeflateDecoder;
use std::collections::HashMap;
use std::fmt::Write;
use std::io::Read;
use std::sync::Arc;
use wb_core::svg::{SvgOptions, export as export_svg};
use wb_core::text::Measurer;
use wb_core::*;

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
            open.push_str(&format!("<span style=\"{css}\">"));
            close.insert_str(0, "</span>");
        }
        for (on, t) in [(sp.style.bold, "b"), (sp.style.italic, "i"), (sp.style.underline, "u"), (sp.style.strike, "s")] {
            if on {
                open.push_str(&format!("<{t}>"));
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

fn arrow_name(h: Arrowhead) -> &'static str {
    match h {
        Arrowhead::None => "none",
        Arrowhead::Arrow => "open",
        Arrowhead::Triangle => "block",
        Arrowhead::Dot => "oval",
        Arrowhead::Bar => "dash",
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

fn page_xml(doc: &Document, page: &Page, m: &dyn Measurer) -> String {
    let b = page.content_bounds().unwrap_or(Rect::new(0.0, 0.0, 100.0, 100.0));
    let (ox, oy) = (40.0 - b.x, 40.0 - b.y);
    let cid = |id: &Id| format!("tb-{}", id.as_str());
    let mut cells = String::new();
    let geo = |x: f64, y: f64, w: f64, h: f64| {
        format!("<mxGeometry x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" as=\"geometry\"/>", round2(x + ox), round2(y + oy), round2(w), round2(h))
    };
    let op = |s: &Style| if s.opacity < 1.0 { format!("opacity={};", (s.opacity * 100.0).round()) } else { String::new() };
    let font = |l: &Label| format!("fontFamily={};fontSize={};align={};", font_name(l.family), l.size, align_name(l.align));
    let vertex = |cells: &mut String, id: &str, value: &str, style: &str, g: String| {
        let _ = write!(cells, "<mxCell id=\"{id}\" value=\"{}\" style=\"{}\" vertex=\"1\" parent=\"1\">{g}</mxCell>", esc(value), esc(style));
    };
    for o in page.objects.iter().filter(|o| page.is_visible(o)) {
        let id = cid(&o.id);
        let st = &o.style;
        let stroke = if st.width > 0.0 { format!("strokeColor={};strokeWidth={};", hex(st.stroke), st.width) } else { "strokeColor=none;".to_owned() };
        let fill = st.fill.map_or("fillColor=none;".to_owned(), |c| format!("fillColor={};", hex(c)));
        match &o.kind {
            Kind::Box(bx) => {
                let f = &bx.frame;
                let rot = if f.angle != 0.0 { format!("rotation={};", round2(f.angle.to_degrees())) } else { String::new() };
                let label = bx.label.clone();
                let value = rich_to_html(&label.text);
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
                    BoxGeom::Sticky => (format!("fillColor={};", hex(st.fill.unwrap_or(wb_core::style::NOTE_YELLOW))), "strokeColor=none;".to_owned()),
                    _ => (fill, stroke),
                };
                let fc = if bx.geom == BoxGeom::Sticky { "#1E1E1E".to_owned() } else { hex(st.stroke) };
                let style = format!(
                    "{base}whiteSpace=wrap;html=1;{fill}{stroke}{}{}{}{}fontColor={fc};{}{rot}",
                    dash_style(st.dash),
                    op(st),
                    font(&label),
                    if bx.geom == BoxGeom::Text { "" } else { "verticalAlign=middle;" },
                    ""
                );
                vertex(&mut cells, &id, &value, &style, geo(f.x, f.y, f.w, f.h));
            }
            Kind::Image(img) => {
                let Some(a) = doc.assets.get(&img.asset) else { continue };
                let f = &img.frame;
                let url = data_url(&a.mime, &a.data).replace(";base64,", ",");
                let rot = if f.angle != 0.0 { format!("rotation={};", round2(f.angle.to_degrees())) } else { String::new() };
                vertex(
                    &mut cells,
                    &id,
                    "",
                    &format!("shape=image;html=1;imageAspect=0;verticalLabelPosition=bottom;verticalAlign=top;image={url};{rot}{}", op(st)),
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
                let Some(path) = page.connector_path(c) else { continue };
                let mut style = String::from("html=1;rounded=0;");
                match c.route {
                    Route::Elbow => style.push_str("edgeStyle=orthogonalEdgeStyle;"),
                    Route::Curve => style.push_str("curved=1;"),
                    Route::Straight => {}
                }
                let _ = write!(
                    style,
                    "endArrow={};startArrow={};endFill=1;startFill=1;strokeColor={};strokeWidth={};{}{}",
                    arrow_name(c.head_end),
                    arrow_name(c.head_start),
                    hex(st.stroke),
                    st.width,
                    dash_style(st.dash),
                    op(st)
                );
                let mut attrs = String::new();
                let mut geom = String::from("<mxGeometry relative=\"1\" as=\"geometry\">");
                for (e, tag, name) in [(&c.start, "source", "sourcePoint"), (&c.end, "target", "targetPoint")] {
                    match e {
                        End::Bound { target, anchor } => {
                            let _ = write!(attrs, " {tag}=\"{}\"", cid(target));
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
                let _ = write!(cells, "<mxCell id=\"{id}\" value=\"{}\" style=\"{}\" edge=\"1\" parent=\"1\"{attrs}>{geom}</mxCell>", esc(&value), esc(&style));
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

/// Exporte toutes les pages du document au format draw.io (XML non compressé).
pub fn export(doc: &Document, m: &dyn Measurer) -> String {
    let pages: String = doc.pages.iter().map(|p| page_xml(doc, p, m)).collect();
    format!("<mxfile host=\"Tableau blanc\" agent=\"Tableau blanc\" version=\"1.0\" type=\"device\">{pages}</mxfile>")
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

/// Convertit le HTML des étiquettes draw.io (sous-ensemble courant) en texte riche.
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
                        match name.as_str() {
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
                            }
                            _ => {}
                        }
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
    while spans.last().is_some_and(|s| s.text.ends_with('\n')) {
        let l = spans.last_mut().unwrap();
        l.text.pop();
        if l.text.is_empty() {
            spans.pop();
        }
    }
    RichText(spans)
}

fn inflate(raw: &str) -> Result<String, FormatError> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let bytes =
        STANDARD.decode(raw.split_whitespace().collect::<String>().as_bytes()).map_err(|_| FormatError::Invalid("diagramme compressé illisible".into()))?;
    let mut s = String::new();
    DeflateDecoder::new(bytes.as_slice()).read_to_string(&mut s).map_err(|_| FormatError::Invalid("décompression impossible".into()))?;
    let decoded = percent_encoding::percent_decode_str(&s).decode_utf8_lossy().into_owned();
    Ok(decoded)
}

struct Builder<'a> {
    m: &'a dyn Measurer,
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

    /// Crée l'objet d'un sommet ; renvoie l'identifiant s'il peut recevoir des connecteurs.
    fn vertex(&mut self, c: &Cell, r: Rect, out: &mut Vec<Object>) -> Option<Id> {
        let s = &c.style;
        let shape = s.get("shape").map(|v| v.to_ascii_lowercase()).unwrap_or_else(|| c.names.first().map(|n| n.to_ascii_lowercase()).unwrap_or_default());
        let angle = s.get("rotation").and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0).to_radians();
        // images
        if s.get("shape").is_some_and(|v| v == "image") || (s.contains_key("image") && !c.names.iter().any(|n| n == "text")) {
            let Some((mime, data)) = s.get("image").and_then(|u| parse_data_url(&format!("data:{}", u.trim_start_matches("data:")))) else {
                self.report.skipped += 1;
                return None;
            };
            let aid = Id::new();
            self.assets.insert(aid.clone(), Asset { mime, data: Arc::new(data) });
            let mut f = rect_of(&r);
            f.angle = angle;
            let obj =
                Object::new(Kind::Image(ImageObj { frame: f, asset: aid }), Style { width: 0.0, opacity: style_for(c, true).opacity, ..Style::default() });
            let id = obj.id.clone();
            out.push(obj);
            return Some(id);
        }
        // texte libre
        if c.names.iter().any(|n| n == "text") || s.get("shape").is_some_and(|v| v == "text") {
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
            let (w, h) = if swap { (r.h, r.w) } else { (r.w, r.h) };
            frame = Frame { x: center.x - w / 2.0, y: center.y - h / 2.0, w, h, angle: a };
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
            style.fill = style.fill.or(Some(wb_core::style::NOTE_YELLOW));
            style.width = 0.0;
            style.stroke = Color::Ink;
        }
        if shape.contains("terminator") || shape.contains("stadium") {
            style.roundness = 1.0;
        }
        let mut label = label_for(c, Align::Center);
        let valign = s.get("verticalAlign").map_or("middle", String::as_str);
        let mut separate: Option<Object> = None;
        if !label.text.is_empty() && (valign == "top" || valign == "bottom") && r.h >= 50.0 {
            let mut t = Label { align: if label.align == Align::Center { Align::Center } else { label.align }, ..label.clone() };
            t.text = std::mem::take(&mut label.text);
            let mut tb = Object::new_box(BoxGeom::Text, Frame::new(0.0, 0.0, 10.0, 10.0), Style { stroke: style.stroke, width: 0.0, ..Style::default() }, t);
            if let Kind::Box(b) = &mut tb.kind {
                b.fit(self.m);
                b.frame.x = match b.label.align {
                    Align::Left => r.x + 8.0,
                    Align::Right => r.right() - b.frame.w - 8.0,
                    Align::Center => r.x + r.w / 2.0 - b.frame.w / 2.0,
                };
                b.frame.y = if valign == "top" { r.y + 6.0 } else { r.bottom() - b.frame.h - 6.0 };
            }
            separate = Some(tb);
        }
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
        let head = |key: &str, default: &str| match s.get(key).map_or(default, String::as_str) {
            "none" => Arrowhead::None,
            "open" | "openThin" => Arrowhead::Arrow,
            "oval" | "ovalThin" => Arrowhead::Dot,
            "dash" => Arrowhead::Bar,
            _ => Arrowhead::Triangle,
        };
        let st = style_for(c, false);
        let mut conn = Connector::new(start, finish);
        conn.head_end = head("endArrow", "classic");
        conn.head_start = head("startArrow", "none");
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

/// Importe un fichier draw.io (compressé ou non, une ou plusieurs pages).
pub fn import(content: &str, name: &str, m: &dyn Measurer) -> Result<Imported, FormatError> {
    let root = xml::parse(content).map_err(FormatError::Invalid)?;
    let mut diagrams: Vec<&Node> = Vec::new();
    if root.name == "mxGraphModel" {
        diagrams.push(&root);
    } else {
        root.find_all("diagram", &mut diagrams);
        if diagrams.is_empty() {
            return Err(FormatError::Invalid("aucun diagramme draw.io".into()));
        }
    }
    let mut b = Builder { m, assets: Default::default(), report: Report::default() };
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
        let pname = d.attr("name").map(str::to_owned).unwrap_or_else(|| format!("Page {}", i + 1));
        if i == 0
            && let Some(bg) = model.attr("background").and_then(parse_color)
        {
            doc.background.preset = BgPreset::nearest(bg.resolve(false));
        }
        let page = b.page(model, &pname);
        doc.pages.push(page);
    }
    doc.assets = b.assets;
    wb_core::serial::sanitize(&mut doc);
    Ok(Imported { doc, report: b.report })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample_document;
    use wb_core::text::ApproxMeasurer;

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
        let root = crate::xml::parse(&xml).expect("XML bien formé");
        assert_eq!(root.name, "mxfile");
        let r = import(&xml, "x", &ApproxMeasurer).unwrap();
        let p = &r.doc.pages[0];
        // le crayon revient sous forme d'image, tout le reste garde sa nature
        let k = kinds(p);
        assert_eq!(k.iter().filter(|x| **x == "conn").count(), 2);
        for want in ["rect", "ellipse", "diamond", "triangle", "sticky", "text"] {
            assert!(k.contains(&want), "{want} manquant dans {k:?}");
        }
        assert_eq!(k.iter().filter(|x| **x == "image").count(), 2, "image + trait converti en image");
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
        assert_eq!(bound[0].label.as_ref().unwrap().text.to_plain(), "oui");
    }

    #[test]
    fn rich_text_and_style_survive() {
        let d = sample_document();
        let r = import(&export(&d, &ApproxMeasurer), "x", &ApproxMeasurer).unwrap();
        let rect =
            r.doc.pages[0].objects.iter().find_map(|o| if let Kind::Box(b) = &o.kind { (b.geom == BoxGeom::Rect).then_some((o, b)) } else { None }).unwrap();
        assert_eq!(rect.1.label.text.to_plain(), "Début\nligne 2");
        assert!(rect.1.label.text.0[0].style.bold, "gras conservé : {:?}", rect.1.label.text);
        assert_eq!(rect.0.style.fill, Some(Color::Rgb(165, 216, 255)));
        assert!(rect.0.style.roundness > 0.3);
        let ell =
            r.doc.pages[0].objects.iter().find_map(|o| if let Kind::Box(b) = &o.kind { (b.geom == BoxGeom::Ellipse).then_some((o, b)) } else { None }).unwrap();
        assert_eq!(ell.0.style.dash, Dash::Dashed);
        assert!((ell.1.frame.angle - 0.3).abs() < 0.01, "rotation conservée");
    }

    #[test]
    fn imports_foreign_drawio_with_edges_waypoints_and_unknown_shapes() {
        let src = r##"<mxfile><diagram name="P"><mxGraphModel background="#ffffff"><root><mxCell id="0"/><mxCell id="1" parent="0"/>
 <mxCell id="a" value="Start" style="ellipse;whiteSpace=wrap;html=1;fillColor=#d5e8d4;strokeColor=#82b366;" vertex="1" parent="1"><mxGeometry x="40" y="40" width="80" height="80" as="geometry"/></mxCell>
 <mxCell id="b" value="&lt;b&gt;Étape&lt;/b&gt;&lt;br&gt;2" style="rounded=1;whiteSpace=wrap;html=1;" vertex="1" parent="1"><mxGeometry x="240" y="50" width="120" height="60" as="geometry"/></mxCell>
 <mxCell id="c" value="Cyl" style="shape=cylinder3;whiteSpace=wrap;html=1;" vertex="1" parent="1"><mxGeometry x="440" y="40" width="70" height="90" as="geometry"/></mxCell>
 <object label="Objet" id="d"><mxCell style="rhombus;whiteSpace=wrap;html=1;" vertex="1" parent="1"><mxGeometry x="40" y="200" width="100" height="80" as="geometry"/></mxCell></object>
 <mxCell id="t" value="Titre" style="text;html=1;align=center;verticalAlign=middle;" vertex="1" parent="1"><mxGeometry x="200" y="0" width="100" height="30" as="geometry"/></mxCell>
 <mxCell id="e1" value="go" style="edgeStyle=orthogonalEdgeStyle;rounded=0;html=1;" edge="1" parent="1" source="a" target="b"><mxGeometry relative="1" as="geometry"/></mxCell>
 <mxCell id="e2" style="endArrow=none;startArrow=oval;dashed=1;html=1;" edge="1" parent="1" source="b" target="c"><mxGeometry relative="1" as="geometry"><Array as="points"><mxPoint x="400" y="160"/></Array></mxGeometry></mxCell>
 <mxCell id="e3" style="html=1;" edge="1" parent="1"><mxGeometry relative="1" as="geometry"><mxPoint x="10" y="300" as="sourcePoint"/><mxPoint x="200" y="300" as="targetPoint"/></mxGeometry></mxCell>
 <mxCell id="e4" style="html=1;" edge="1" parent="1" source="zzz"><mxGeometry relative="1" as="geometry"/></mxCell>
 </root></mxGraphModel></diagram></mxfile>"##;
        let r = import(src, "ext", &ApproxMeasurer).unwrap();
        let p = &r.doc.pages[0];
        assert_eq!(p.name, "P");
        assert_eq!(r.report.approximated, 1, "cylindre approximé");
        assert_eq!(r.report.skipped, 1, "arête sans extrémités");
        let conns: Vec<&Connector> = p.objects.iter().filter_map(|o| if let Kind::Connector(c) = &o.kind { Some(c) } else { None }).collect();
        assert_eq!(conns.len(), 3);
        assert_eq!(conns[0].route, Route::Elbow);
        assert_eq!(conns[0].head_end, Arrowhead::Triangle, "classic par défaut");
        assert_eq!(conns[0].label.as_ref().unwrap().text.to_plain(), "go");
        assert_eq!(conns[1].head_end, Arrowhead::None);
        assert_eq!(conns[1].head_start, Arrowhead::Dot);
        assert_eq!(conns[1].via, vec![Pt::new(400.0, 160.0)]);
        assert!(matches!(conns[2].start, End::Free { .. }));
        // texte riche (gras) de l'étape 2 et objet enveloppé
        let b = p
            .objects
            .iter()
            .find_map(|o| if let Kind::Box(b) = &o.kind { (b.label.text.to_plain() == "Étape\n2").then_some(b) } else { None })
            .expect("étiquette HTML convertie");
        assert!(b.label.text.0[0].style.bold);
        assert!(p.objects.iter().any(|o| matches!(&o.kind, Kind::Box(b) if b.label.text.to_plain() == "Objet" && b.geom == BoxGeom::Diamond)));
        assert!(p.objects.iter().any(|o| matches!(&o.kind, Kind::Box(b) if b.geom == BoxGeom::Text && b.label.text.to_plain() == "Titre")));
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
        let src = format!("<mxfile><diagram name=\"Compressé\" id=\"x\">{packed}</diagram></mxfile>");
        let r = import(&src, "z", &ApproxMeasurer).unwrap();
        assert_eq!(r.doc.pages[0].name, "Compressé");
        assert!(matches!(&r.doc.pages[0].objects[0].kind, Kind::Box(b) if b.label.text.to_plain() == "Zip é"));
        assert!(import("<mxfile><diagram>!!!pas-du-base64!!!</diagram></mxfile>", "z", &ApproxMeasurer).is_err());
    }

    #[test]
    fn multiple_pages_roundtrip_and_containers_offset_children() {
        let mut d = sample_document();
        let mut p2 = Page::new("Deuxième");
        p2.objects.push(Object::new_box(BoxGeom::Rect, Frame::new(5.0, 5.0, 50.0, 50.0), Style::default(), Label::new("p2", 20.0, Align::Center)));
        d.pages.push(p2);
        let r = import(&export(&d, &ApproxMeasurer), "x", &ApproxMeasurer).unwrap();
        assert_eq!(r.doc.pages.len(), 2);
        assert_eq!(r.doc.pages[1].name, "Deuxième");
        let src = r#"<mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/>
          <mxCell id="g" value="" style="swimlane;" vertex="1" parent="1"><mxGeometry x="100" y="100" width="300" height="200" as="geometry"/></mxCell>
          <mxCell id="k" value="enfant" style="rounded=0;" vertex="1" parent="g"><mxGeometry x="20" y="40" width="80" height="40" as="geometry"/></mxCell></root></mxGraphModel>"#;
        let r = import(src, "x", &ApproxMeasurer).unwrap();
        let child = r.doc.pages[0]
            .objects
            .iter()
            .find_map(|o| if let Kind::Box(b) = &o.kind { (b.label.text.to_plain() == "enfant").then_some(b.frame) } else { None })
            .unwrap();
        assert_eq!((child.x, child.y), (120.0, 140.0), "coordonnées relatives au conteneur");
    }

    #[test]
    fn html_conversion_handles_entities_and_blocks() {
        let r = html_to_rich(
            "a &lt;b&gt; &amp; c<div>ligne 2</div><span style=\"color:#ff0000;background-color:#ffff00\">rouge</span><font color=\"#00ff00\">vert</font>",
        );
        assert_eq!(r.to_plain(), "a <b> & c\nligne 2\nrougevert");
        let red = r.0.iter().find(|s| s.text == "rouge").unwrap();
        assert_eq!(red.style.color, Some(Color::Rgb(255, 0, 0)));
        assert!(red.style.highlight.is_some());
        assert_eq!(r.0.iter().find(|s| s.text == "vert").unwrap().style.color, Some(Color::Rgb(0, 255, 0)));
        assert!(html_to_rich("<b>x</b>").0[0].style.bold);
        assert!(html_to_rich("").is_empty());
    }
}
