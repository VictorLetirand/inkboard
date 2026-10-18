//! Mermaid : `flowchart` / `graph`, `stateDiagram`, `sequenceDiagram`.
//!
//! The text becomes real shapes linked by real connectors: moving a node
//! makes its arrows follow. The layout comes from `inkboard_core::layout`.

use crate::common::{FormatError, Imported, Report, parse_color};
use inkboard_core::layout::{Direction, NodeSpec, layered};
use inkboard_core::text::Measurer;
use inkboard_core::*;
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

const FONT: f64 = 18.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    Rect,
    Round,
    Stadium,
    Circle,
    Diamond,
    Cylinder,
    Hexagon,
    Subroutine,
    Parallelogram,
    Trapezoid,
    Flag,
    Start,
    End,
    Bar,
}

#[derive(Debug)]
struct Node {
    id: String,
    label: String,
    shape: Shape,
    props: HashMap<String, String>,
    classes: Vec<String>,
}

#[derive(Debug, Clone)]
struct Link {
    from: String,
    to: String,
    label: String,
    start: bool,
    end: bool,
    dashed: bool,
    thick: bool,
    invisible: bool,
}

#[derive(Debug)]
struct Sub {
    id: String,
    title: String,
    members: Vec<String>,
    parent: Option<usize>,
}

#[derive(Debug)]
struct Flow {
    nodes: Vec<Node>,
    index: HashMap<String, usize>,
    links: Vec<Link>,
    subs: Vec<Sub>,
    stack: Vec<usize>,
    classes: HashMap<String, HashMap<String, String>>,
    dir: Direction,
}

impl Flow {
    fn new(dir: Direction) -> Self {
        Flow { nodes: Vec::new(), index: HashMap::new(), links: Vec::new(), subs: Vec::new(), stack: Vec::new(), classes: HashMap::new(), dir }
    }

    fn node(&mut self, id: &str, label: Option<String>, shape: Option<Shape>, class: Option<&str>) -> usize {
        let i = match self.index.get(id) {
            Some(&i) => i,
            None => {
                let sub = self.stack.last().copied();
                self.nodes.push(Node { id: id.to_owned(), label: id.to_owned(), shape: Shape::Rect, props: HashMap::new(), classes: Vec::new() });
                let i = self.nodes.len() - 1;
                self.index.insert(id.to_owned(), i);
                if let Some(s) = sub {
                    self.subs[s].members.push(id.to_owned());
                }
                i
            }
        };
        if let Some(l) = label {
            self.nodes[i].label = clean(&l);
        }
        if let Some(s) = shape {
            self.nodes[i].shape = s;
        }
        if let Some(c) = class {
            self.nodes[i].classes.push(c.to_owned());
        }
        i
    }
}

fn clean(s: &str) -> String {
    static BR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<br\s*/?>").unwrap());
    static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());
    let s = BR.replace_all(s, "\n");
    let s = TAG.replace_all(&s, "");
    s.trim().trim_matches('"').replace("&quot;", "\"").replace("#quot;", "\"")
}

struct Tok {
    id: String,
    label: Option<String>,
    shape: Option<Shape>,
    class: Option<String>,
    end: usize,
}

const OPENERS: [(&str, &str, Shape); 14] = [
    ("(((", ")))", Shape::Circle),
    ("((", "))", Shape::Circle),
    ("([", "])", Shape::Stadium),
    ("[(", ")]", Shape::Cylinder),
    ("{{", "}}", Shape::Hexagon),
    ("[[", "]]", Shape::Subroutine),
    ("[/", "/]", Shape::Parallelogram),
    ("[\\", "\\]", Shape::Parallelogram),
    ("[/", "\\]", Shape::Trapezoid),
    ("[\\", "/]", Shape::Trapezoid),
    ("[", "]", Shape::Rect),
    ("(", ")", Shape::Round),
    ("{", "}", Shape::Diamond),
    (">", "]", Shape::Flag),
];

fn parse_node(s: &str, i: usize) -> Option<Tok> {
    let rest = &s[i..];
    let id_len = rest.char_indices().find(|(_, c)| !(c.is_alphanumeric() || *c == '_')).map_or(rest.len(), |(p, _)| p);
    if id_len == 0 {
        return None;
    }
    let id = rest[..id_len].to_owned();
    let mut j = i + id_len;
    let (mut label, mut shape) = (None, None);
    for (open, close, sh) in OPENERS {
        if !s[j..].starts_with(open) {
            continue;
        }
        let k = j + open.len();
        let (text, end) = if s[k..].starts_with('"') {
            let Some(q) = s[k + 1..].find('"') else { break };
            let q = k + 1 + q;
            let Some(e) = s[q + 1..].find(close) else { break };
            (s[k + 1..q].to_owned(), q + 1 + e + close.len())
        } else {
            let Some(e) = s[k..].find(close) else { break };
            (s[k..k + e].to_owned(), k + e + close.len())
        };
        label = Some(text);
        shape = Some(sh);
        j = end;
        break;
    }
    let mut class = None;
    if let Some(r) = s[j..].strip_prefix(":::") {
        let n = r.find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-')).unwrap_or(r.len());
        class = Some(r[..n].to_owned());
        j += 3 + n;
    }
    Some(Tok { id, label, shape, class, end: j })
}

fn read_link(s: &str) -> Option<(usize, Link)> {
    static LABELED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(<?)(--|==)\s+([^<>\-][^<>]*?)\s+(-{2,}|={2,})([>xo]?)").unwrap());
    static DOT_LABELED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(<?)-\.\s+([^<>]*?)\s+\.-+([>xo]?)").unwrap());
    static DOTTED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(<?)-\.+-([>xo]?)(?:\|([^|]*)\|)?").unwrap());
    static PLAIN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(<?)(-{2,}|={2,})([>xo]?)(?:\|([^|]*)\|)?").unwrap());
    let base = Link { from: String::new(), to: String::new(), label: String::new(), start: false, end: false, dashed: false, thick: false, invisible: false };
    if let Some(m) = LABELED.captures(s) {
        return Some((m[0].len(), Link { label: clean(&m[3]), start: !m[1].is_empty(), end: !m[5].is_empty(), thick: &m[2] == "==", ..base }));
    }
    if let Some(m) = DOT_LABELED.captures(s) {
        return Some((m[0].len(), Link { label: clean(&m[2]), start: !m[1].is_empty(), end: !m[3].is_empty(), dashed: true, ..base }));
    }
    if let Some(m) = DOTTED.captures(s) {
        return Some((
            m[0].len(),
            Link { label: m.get(3).map_or(String::new(), |l| clean(l.as_str())), start: !m[1].is_empty(), end: !m[2].is_empty(), dashed: true, ..base },
        ));
    }
    if s.starts_with("~~~") {
        return Some((3, Link { invisible: true, ..base }));
    }
    if let Some(m) = PLAIN.captures(s) {
        return Some((
            m[0].len(),
            Link {
                label: m.get(4).map_or(String::new(), |l| clean(l.as_str())),
                start: !m[1].is_empty(),
                end: !m[3].is_empty(),
                thick: m[2].starts_with('='),
                ..base
            },
        ));
    }
    None
}

fn style_props(s: &str) -> HashMap<String, String> {
    s.split(',').filter_map(|kv| kv.split_once(':')).map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned())).collect()
}

fn parse_flow_line(s: &str, f: &mut Flow) {
    let mut i = 0;
    let skip = |i: &mut usize| {
        while s[*i..].starts_with(' ') {
            *i += 1;
        }
    };
    let read_group = |i: &mut usize, f: &mut Flow| -> Option<Vec<usize>> {
        let mut g = Vec::new();
        loop {
            skip(i);
            let Some(t) = parse_node(s, *i) else { return if g.is_empty() { None } else { Some(g) } };
            *i = t.end;
            g.push(f.node(&t.id, t.label, t.shape, t.class.as_deref()));
            skip(i);
            if s[*i..].starts_with('&') {
                *i += 1;
                continue;
            }
            return Some(g);
        }
    };
    let Some(mut prev) = read_group(&mut i, f) else { return };
    loop {
        skip(&mut i);
        if i >= s.len() {
            break;
        }
        let Some((len, link)) = read_link(&s[i..]) else { break };
        i += len;
        let Some(next) = read_group(&mut i, f) else { break };
        for &a in &prev {
            for &b in &next {
                let l = Link { from: f.nodes[a].id.clone(), to: f.nodes[b].id.clone(), ..link.clone() };
                f.links.push(l);
            }
        }
        prev = next;
    }
}

fn parse_flowchart(lines: &[String], f: &mut Flow) {
    static SUB: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^subgraph\s+(.+)$").unwrap());
    static SUB_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([\w\p{L}]+)\s*\[(.*)\]\s*$").unwrap());
    static CLASSDEF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^classDef\s+([\w,-]+)\s+(.+)$").unwrap());
    static CLASS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^class\s+(\S+)\s+([\w-]+)$").unwrap());
    static STYLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^style\s+([\w\p{L}]+)\s+(.+)$").unwrap());
    for raw in lines {
        for stmt in raw.split(';') {
            let s = stmt.trim();
            if s.is_empty() {
                continue;
            }
            if let Some(m) = SUB.captures(s) {
                let rest = m[1].trim();
                let (id, title) = SUB_ID.captures(rest).map_or((rest.to_owned(), clean(rest)), |c| (c[1].to_owned(), clean(&c[2])));
                f.subs.push(Sub { id, title, members: Vec::new(), parent: f.stack.last().copied() });
                f.stack.push(f.subs.len() - 1);
            } else if s.eq_ignore_ascii_case("end") {
                f.stack.pop();
            } else if s.starts_with("direction ") || ["linkStyle", "click", "accTitle", "accDescr", "title"].iter().any(|k| s.starts_with(k)) {
                continue;
            } else if let Some(m) = CLASSDEF.captures(s) {
                for c in m[1].split(',') {
                    f.classes.insert(c.to_owned(), style_props(&m[2]));
                }
            } else if let Some(m) = CLASS.captures(s) {
                for id in m[1].split(',') {
                    if let Some(&i) = f.index.get(id) {
                        f.nodes[i].classes.push(m[2].to_owned());
                    }
                }
            } else if let Some(m) = STYLE.captures(s) {
                if let Some(&i) = f.index.get(&m[1]) {
                    f.nodes[i].props.extend(style_props(&m[2]));
                }
            } else {
                parse_flow_line(s, f);
            }
        }
    }
}

fn parse_state(lines: &[String], f: &mut Flow) {
    static ALIAS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^state\s+"(.+)"\s+as\s+(\w+)"#).unwrap());
    static SPECIAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^state\s+(\w+)\s*<<(choice|fork|join)>>").unwrap());
    static COMPOSITE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^state\s+(?:"([^"]+)"\s+as\s+)?(\w+)\s*\{$"#).unwrap());
    static TRANS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\[\*\]|\w+)\s*-->\s*(\[\*\]|\w+)\s*(?::\s*(.*))?$").unwrap());
    static DESC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\w+)\s*:\s*(.+)$").unwrap());
    static DIR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^direction\s+(TB|TD|LR|RL|BT)").unwrap());
    for s in lines {
        if let Some(m) = DIR.captures(s) {
            f.dir = dir_of(&m[1]);
        } else if let Some(m) = ALIAS.captures(s) {
            f.node(&m[2], Some(m[1].to_owned()), Some(Shape::Round), None);
        } else if let Some(m) = SPECIAL.captures(s) {
            f.node(&m[1], Some(String::new()), Some(if &m[2] == "choice" { Shape::Diamond } else { Shape::Bar }), None);
        } else if let Some(m) = COMPOSITE.captures(s) {
            let title = m.get(1).map_or(m[2].to_owned(), |t| t.as_str().to_owned());
            f.subs.push(Sub { id: m[2].to_owned(), title, members: Vec::new(), parent: f.stack.last().copied() });
            f.stack.push(f.subs.len() - 1);
        } else if s == "}" {
            f.stack.pop();
        } else if let Some(m) = TRANS.captures(s) {
            let scope = f.stack.last().map_or(String::new(), |&i| f.subs[i].id.clone());
            let end = |id: &str, source: bool, f: &mut Flow| -> String {
                if id == "[*]" {
                    let key = format!("{}{}", if source { "__start" } else { "__end" }, scope);
                    let i = f.node(&key, Some(String::new()), Some(if source { Shape::Start } else { Shape::End }), None);
                    f.nodes[i].id.clone()
                } else {
                    let i = f.node(id, None, None, None);
                    f.nodes[i].id.clone()
                }
            };
            let (a, b) = (end(&m[1], true, f), end(&m[2], false, f));
            f.links.push(Link {
                from: a,
                to: b,
                label: clean(m.get(3).map_or("", |x| x.as_str())),
                start: false,
                end: true,
                dashed: false,
                thick: false,
                invisible: false,
            });
        } else if let Some(m) = DESC.captures(s) {
            f.node(&m[1], Some(m[2].to_owned()), Some(Shape::Round), None);
        }
    }
}

fn dir_of(s: &str) -> Direction {
    match s.to_ascii_uppercase().as_str() {
        "LR" => Direction::LeftRight,
        "RL" => Direction::RightLeft,
        "BT" => Direction::BottomUp,
        _ => Direction::TopDown,
    }
}

/// Dimensions of a node according to its text.
fn node_size(n: &Node, m: &dyn Measurer) -> (BoxGeom, f64, f64) {
    let (geom, mut w, mut h) = match n.shape {
        Shape::Start => return (BoxGeom::Ellipse, 26.0, 26.0),
        Shape::End => return (BoxGeom::Ellipse, 28.0, 28.0),
        Shape::Bar => return (BoxGeom::Rect, 90.0, 10.0),
        Shape::Circle => (BoxGeom::Ellipse, 90.0, 60.0),
        Shape::Diamond => (BoxGeom::Diamond, 130.0, 84.0),
        _ => (BoxGeom::Rect, 90.0, 52.0),
    };
    let tw = n.label.lines().map(|l| m.width(l, FontFamily::Sans, FONT, false)).fold(0.0, f64::max);
    w = match geom {
        BoxGeom::Ellipse => (tw / 0.7 + 32.0).clamp(w, 300.0),
        BoxGeom::Diamond => (tw / 0.5 + 24.0).clamp(w, 340.0),
        _ => (tw + 40.0).clamp(w, 300.0),
    };
    let mut b = BoxObj { geom, frame: Frame::new(0.0, 0.0, w, h), label: Label::new(&n.label, FONT, Align::Center) };
    for _ in 0..6 {
        let before = b.frame.h;
        b.fit(m);
        if (b.frame.h - before).abs() < 0.5 {
            break;
        }
        b.frame.h += 4.0;
    }
    h = b.frame.h;
    (geom, w, h)
}

fn node_object(n: &Node, geom: BoxGeom, frame: Frame, classes: &HashMap<String, HashMap<String, String>>) -> Object {
    let mut label = Label::new(&n.label, FONT, Align::Center);
    let mut style = Style {
        width: 2.5,
        roundness: if matches!(n.shape, Shape::Round) {
            0.45
        } else if n.shape == Shape::Stadium {
            1.0
        } else {
            0.0
        },
        ..Style::default()
    };
    match n.shape {
        Shape::Start | Shape::Bar => {
            style.fill = Some(Color::Ink);
            label.text = RichText::default();
        }
        Shape::End => {
            style.width = 5.0;
            label.text = RichText::default();
        }
        _ => {}
    }
    let mut merged: HashMap<String, String> = HashMap::new();
    for c in &n.classes {
        if let Some(cp) = classes.get(c) {
            merged.extend(cp.clone());
        }
    }
    merged.extend(n.props.clone());
    let p = &merged;
    if let Some(f) = p.get("fill").and_then(|c| parse_color(c)) {
        style.fill = Some(f);
    }
    if let Some(s) = p.get("stroke").and_then(|c| parse_color(c)) {
        style.stroke = s;
    }
    if let Some(w) = p.get("stroke-width").and_then(|v| v.trim_end_matches("px").parse::<f64>().ok()) {
        style.width = (w * 1.5).clamp(1.5, 10.0);
    }
    if p.contains_key("stroke-dasharray") {
        style.dash = Dash::Dashed;
    }
    if let Some(c) = p.get("color").and_then(|c| parse_color(c)) {
        let len = label.text.to_plain().chars().count();
        label.text.restyle(0, len, |s| s.color = Some(c));
    }
    Object::new_box(geom, frame, style, label)
}

fn build_flow(f: &Flow, m: &dyn Measurer) -> Vec<Object> {
    let sizes: Vec<(BoxGeom, f64, f64)> = f.nodes.iter().map(|n| node_size(n, m)).collect();
    let specs: Vec<NodeSpec> = sizes.iter().map(|s| NodeSpec { w: s.1, h: s.2 }).collect();
    let edges: Vec<(usize, usize)> = f.links.iter().filter_map(|l| Some((*f.index.get(&l.from)?, *f.index.get(&l.to)?))).collect();
    let link_idx: Vec<usize> = (0..f.links.len()).filter(|&i| f.index.contains_key(&f.links[i].from) && f.index.contains_key(&f.links[i].to)).collect();
    let lay = layered(&specs, &edges, f.dir);
    let mut nodes: Vec<Object> = Vec::new();
    let mut rects: Vec<Rect> = Vec::new();
    for (i, n) in f.nodes.iter().enumerate() {
        let (g, w, h) = sizes[i];
        let c = lay.centers[i];
        let r = Rect::new(c.x - w / 2.0, c.y - h / 2.0, w, h);
        rects.push(r);
        nodes.push(node_object(n, g, Frame::from_rect(r), &f.classes));
    }
    // subgraphs
    let mut bounds: HashMap<usize, Option<Rect>> = HashMap::new();
    fn bounds_of(i: usize, f: &Flow, rects: &[Rect], cache: &mut HashMap<usize, Option<Rect>>) -> Option<Rect> {
        if let Some(b) = cache.get(&i) {
            return *b;
        }
        let mut acc: Option<Rect> = None;
        let mut add = |r: Rect| acc = Some(acc.map_or(r, |a| a.union(&r)));
        for id in &f.subs[i].members {
            if let Some(&k) = f.index.get(id) {
                add(rects[k]);
            }
        }
        for (k, s) in f.subs.iter().enumerate() {
            if s.parent == Some(i)
                && let Some(b) = bounds_of(k, f, rects, cache)
            {
                add(b);
            }
        }
        let r = acc.map(|a| Rect::new(a.x - 28.0, a.y - 46.0, a.w + 56.0, a.h + 74.0));
        cache.insert(i, r);
        r
    }
    let depth = |mut i: usize| {
        let mut d = 0;
        while let Some(p) = f.subs[i].parent {
            d += 1;
            i = p;
        }
        d
    };
    let mut order: Vec<usize> = (0..f.subs.len()).collect();
    order.sort_by_key(|&i| depth(i));
    let mut out: Vec<Object> = Vec::new();
    for i in order {
        let Some(b) = bounds_of(i, f, &rects, &mut bounds) else { continue };
        out.push(Object::new_box(
            BoxGeom::Rect,
            Frame::from_rect(b),
            Style { dash: Dash::Dashed, width: 2.0, opacity: 0.85, roundness: 0.1, ..Style::default() },
            Label::default(),
        ));
        let mut t = Object::new_box(
            BoxGeom::Text,
            Frame::new(b.x + 12.0, b.y + 8.0, 10.0, 10.0),
            Style { width: 0.0, ..Style::default() },
            Label::new(&f.subs[i].title, 16.0, Align::Left),
        );
        if let Kind::Box(bx) = &mut t.kind {
            bx.fit(m);
        }
        out.push(t);
    }
    let ids: Vec<Id> = nodes.iter().map(|o| o.id.clone()).collect();
    out.extend(nodes);
    // edges: real bound connectors; layout relays become waypoints
    let mut pair_count: HashMap<(usize, usize), usize> = HashMap::new();
    for &(a, b) in &edges {
        *pair_count.entry((a.min(b), a.max(b))).or_default() += 1;
    }
    let mut pair_seen: HashMap<(usize, usize), usize> = HashMap::new();
    for (k, &li) in link_idx.iter().enumerate() {
        let l = &f.links[li];
        let (a, b) = edges[k];
        if l.invisible {
            continue;
        }
        let head = |on: bool| if on { Arrowhead::Triangle } else { Arrowhead::None };
        let style = Style { width: if l.thick { 5.0 } else { 2.5 }, dash: if l.dashed { Dash::Dashed } else { Dash::Solid }, ..Style::default() };
        let mut c = Connector::new(End::bound(ids[a].clone()), End::bound(ids[b].clone()));
        c.head_end = head(l.end);
        c.head_start = head(l.start);
        if !l.label.is_empty() {
            c.label = Some(Label::new(&l.label, 15.0, Align::Center));
        }
        if a == b {
            let r = rects[a];
            c.start = End::Bound { target: ids[a].clone(), anchor: Anchor::Frac { fx: 1.0, fy: 0.25 } };
            c.end = End::Bound { target: ids[a].clone(), anchor: Anchor::Frac { fx: 1.0, fy: 0.75 } };
            c.via = vec![Pt::new(r.right() + 34.0, r.y + r.h * 0.25), Pt::new(r.right() + 34.0, r.y + r.h * 0.75)];
        } else {
            let route = &lay.routes[k];
            if route.len() > 2 {
                c.via = route[1..route.len() - 1].to_vec();
            } else {
                let key = (a.min(b), a.max(b));
                let n = pair_count[&key];
                let idx = pair_seen.entry(key).or_default();
                if n > 1 {
                    let (p, q) = (rects[key.0].center(), rects[key.1].center());
                    let d = q - p;
                    let len = d.len().max(1.0);
                    let perp = Pt::new(-d.y / len, d.x / len);
                    let off = (*idx as f64 - (n as f64 - 1.0) / 2.0) * 44.0;
                    c.via = vec![p.lerp(q, 0.5) + perp * off];
                    c.route = Route::Curve;
                }
                *idx += 1;
            }
        }
        out.push(Object::new(Kind::Connector(c), style));
    }
    out
}

fn text_object(text: &str, x: f64, y: f64, size: f64, align: Align, m: &dyn Measurer) -> Object {
    let mut t = Object::new_box(BoxGeom::Text, Frame::new(x, y, 10.0, 10.0), Style { width: 0.0, ..Style::default() }, Label::new(text, size, align));
    if let Kind::Box(b) = &mut t.kind {
        b.fit(m);
    }
    t
}

fn build_sequence(lines: &[String], m: &dyn Measurer) -> Result<Vec<Object>, FormatError> {
    static PARTICIPANT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(?:participant|actor)\s+(\S+)(?:\s+as\s+(.+))?$").unwrap());
    static BLOCK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(loop|alt|opt|par|critical|break|rect)\b\s*(.*)$").unwrap());
    static MARK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(else|and|option)\b\s*(.*)$").unwrap());
    static NOTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^Note\s+(over|left of|right of)\s+([^:]+):\s*(.*)$").unwrap());
    static MSG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([^\s>:\-]+)\s*(-->>|->>|-->|->|--x|-x|--\)|-\))\s*[+-]?([^\s:]+)\s*:\s*(.*)$").unwrap());
    enum Row {
        Msg { from: String, to: String, arrow: String, text: String, r: f64 },
        Note { kind: String, ids: Vec<String>, text: String, r: f64 },
    }
    let mut parts: Vec<(String, String)> = Vec::new();
    let part = |parts: &mut Vec<(String, String)>, id: &str, label: Option<String>| match parts.iter_mut().find(|p| p.0 == id) {
        Some(p) => {
            if let Some(l) = label {
                p.1 = l;
            }
        }
        None => parts.push((id.to_owned(), label.unwrap_or_else(|| id.to_owned()))),
    };
    let (mut auto, mut count, mut row) = (false, 0, 0.0f64);
    let mut rows: Vec<Row> = Vec::new();
    let mut stack: Vec<(String, f64)> = Vec::new();
    let mut frames: Vec<(String, f64, f64)> = Vec::new();
    let mut marks: Vec<(String, f64)> = Vec::new();
    for s in lines {
        if let Some(c) = PARTICIPANT.captures(s) {
            part(&mut parts, &c[1], c.get(2).map(|l| clean(l.as_str())));
        } else if s.to_ascii_lowercase().starts_with("autonumber") {
            auto = true;
        } else if let Some(c) = BLOCK.captures(s) {
            stack.push((format!("{} {}", &c[1], c[2].trim()).trim().to_owned(), row));
            row += 0.6;
        } else if let Some(c) = MARK.captures(s) {
            marks.push((format!("[{}]", if c[2].is_empty() { &c[1] } else { &c[2] }), row));
            row += 0.6;
        } else if s.eq_ignore_ascii_case("end") {
            if let Some((l, r0)) = stack.pop() {
                frames.push((l, r0, row));
            }
            row += 0.4;
        } else if let Some(c) = NOTE.captures(s) {
            let ids: Vec<String> = c[2].split(',').map(|x| x.trim().to_owned()).collect();
            for id in &ids {
                part(&mut parts, id, None);
            }
            rows.push(Row::Note { kind: c[1].to_ascii_lowercase(), ids, text: clean(&c[3]), r: row });
            row += 1.0;
        } else if let Some(c) = MSG.captures(s) {
            part(&mut parts, &c[1], None);
            part(&mut parts, &c[3], None);
            count += 1;
            rows.push(Row::Msg {
                from: c[1].to_owned(),
                to: c[3].to_owned(),
                arrow: c[2].to_owned(),
                text: format!("{}{}", if auto { format!("{count}. ") } else { String::new() }, clean(&c[4])),
                r: row,
            });
            row += 1.0;
        }
    }
    if parts.is_empty() {
        return Err(FormatError::Invalid("no message found".into()));
    }
    let widths: Vec<f64> = parts.iter().map(|p| (m.width(&p.1, FontFamily::Sans, FONT, false) + 36.0).max(120.0)).collect();
    let mut xs: HashMap<&str, f64> = HashMap::new();
    let mut x = 0.0;
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            x += ((widths[i - 1] + widths[i]) / 2.0 + 60.0).max(220.0);
        }
        xs.insert(p.0.as_str(), x);
    }
    let y_of = |r: f64| 130.0 + r * 56.0;
    let bottom = y_of(row) + 20.0;
    let (left, right) = (xs.values().copied().fold(f64::INFINITY, f64::min) - 90.0, xs.values().copied().fold(f64::NEG_INFINITY, f64::max) + 90.0);
    let mut out: Vec<Object> = Vec::new();
    let line = |a: Pt, b: Pt, head: bool, dashed: bool, w: f64| {
        let mut c = Connector::straight(a, b);
        c.head_end = if head { Arrowhead::Triangle } else { Arrowhead::None };
        Object::new(Kind::Connector(c), Style { width: w, dash: if dashed { Dash::Dashed } else { Dash::Solid }, ..Style::default() })
    };
    for p in &parts {
        out.push(line(Pt::new(xs[p.0.as_str()], 54.0), Pt::new(xs[p.0.as_str()], bottom), false, true, 1.5));
    }
    for (label, r0, r1) in &frames {
        out.push(Object::new_box(
            BoxGeom::Rect,
            Frame::new(left, y_of(*r0) - 24.0, right - left, y_of(*r1) - y_of(*r0) + 4.0),
            Style { dash: Dash::Dashed, width: 1.5, opacity: 0.7, ..Style::default() },
            Label::default(),
        ));
        out.push(text_object(label, left + 8.0, y_of(*r0) - 20.0, 14.0, Align::Left, m));
    }
    for (label, r) in &marks {
        out.push(line(Pt::new(left, y_of(*r) - 12.0), Pt::new(right, y_of(*r) - 12.0), false, true, 1.5));
        out.push(text_object(label, left + 8.0, y_of(*r) - 12.0, 14.0, Align::Left, m));
    }
    for (i, p) in parts.iter().enumerate() {
        let w = widths[i];
        out.push(Object::new_box(
            BoxGeom::Rect,
            Frame::new(xs[p.0.as_str()] - w / 2.0, 0.0, w, 54.0),
            Style { width: 2.5, roundness: 0.45, ..Style::default() },
            Label::new(&p.1, FONT, Align::Center),
        ));
    }
    for r in &rows {
        match r {
            Row::Note { kind, ids, text, r } => {
                let cx: Vec<f64> = ids.iter().filter_map(|i| xs.get(i.as_str()).copied()).collect();
                let (mn, mx) = (cx.iter().copied().fold(f64::INFINITY, f64::min), cx.iter().copied().fold(f64::NEG_INFINITY, f64::max));
                let w = 160.0f64.max(mx - mn + 100.0);
                let sx = match kind.as_str() {
                    "left of" => mn - w / 2.0 - 20.0,
                    "right of" => mx + w / 2.0 + 20.0,
                    _ => (mn + mx) / 2.0,
                };
                let mut n = Object::new_box(
                    BoxGeom::Sticky,
                    Frame::new(sx - w / 2.0, y_of(*r) - 20.0, w, 54.0),
                    Style { fill: Some(inkboard_core::style::NOTE_YELLOW), width: 0.0, ..Style::default() },
                    Label::new(text, 15.0, Align::Center),
                );
                if let Kind::Box(b) = &mut n.kind {
                    b.fit(m);
                }
                out.push(n);
            }
            Row::Msg { from, to, arrow, text, r } => {
                let (a, b, y) = (xs[from.as_str()], xs[to.as_str()], y_of(*r));
                let dashed = arrow.starts_with("--");
                let head = arrow.ends_with(">>") || arrow.ends_with('x') || arrow.ends_with(')');
                if from == to {
                    let mut c = Connector::new(End::free(Pt::new(a, y - 10.0)), End::free(Pt::new(a, y + 14.0)));
                    c.via = vec![Pt::new(a + 46.0, y - 10.0), Pt::new(a + 46.0, y + 14.0)];
                    c.head_end = Arrowhead::Triangle;
                    out.push(Object::new(Kind::Connector(c), Style { width: 2.5, dash: if dashed { Dash::Dashed } else { Dash::Solid }, ..Style::default() }));
                    if !text.is_empty() {
                        out.push(text_object(text, a + 56.0, y - 8.0, 15.0, Align::Left, m));
                    }
                } else {
                    out.push(line(Pt::new(a, y), Pt::new(b, y), head, dashed, 2.5));
                    if !text.is_empty() {
                        let mut t = text_object(text, 0.0, 0.0, 15.0, Align::Center, m);
                        if let Kind::Box(bx) = &mut t.kind {
                            bx.frame.x = (a + b) / 2.0 - bx.frame.w / 2.0;
                            bx.frame.y = y - bx.frame.h - 4.0;
                        }
                        out.push(t);
                    }
                }
            }
        }
    }
    Ok(out)
}

/// Converts Mermaid text to objects ready to be placed on a page.
pub fn to_objects(src: &str, m: &dyn Measurer) -> Result<Vec<Object>, FormatError> {
    let mut lines: Vec<&str> = src.lines().collect();
    if lines.first().is_some_and(|l| l.trim() == "---")
        && let Some(end) = lines.iter().skip(1).position(|l| l.trim() == "---")
    {
        lines.drain(..=end + 1);
    }
    let lines: Vec<String> = lines.iter().map(|l| l.split("%%").next().unwrap_or("").trim().to_owned()).filter(|l| !l.is_empty()).collect();
    let Some(head) = lines.first() else { return Err(FormatError::Invalid("empty text".into())) };
    static FLOW: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(?:flowchart|graph)(?:-elk)?\b\s*(TD|TB|LR|RL|BT)?").unwrap());
    if let Some(c) = FLOW.captures(head) {
        let mut f = Flow::new(dir_of(c.get(1).map_or("TD", |d| d.as_str())));
        parse_flowchart(&lines[1..], &mut f);
        if f.nodes.is_empty() {
            return Err(FormatError::Invalid("no node found".into()));
        }
        return Ok(build_flow(&f, m));
    }
    if head.starts_with("stateDiagram") {
        let mut f = Flow::new(Direction::TopDown);
        parse_state(&lines[1..], &mut f);
        if f.nodes.is_empty() {
            return Err(FormatError::Invalid("no state found".into()));
        }
        return Ok(build_flow(&f, m));
    }
    if head.starts_with("sequenceDiagram") {
        return build_sequence(&lines[1..], m);
    }
    let kind = head.split_whitespace().next().unwrap_or("");
    Err(FormatError::Unsupported(format!("unsupported diagram type (\"{kind}\"). Supported: flowchart/graph, stateDiagram, sequenceDiagram")))
}

pub fn import(content: &str, name: &str, m: &dyn Measurer) -> Result<Imported, FormatError> {
    let objs = to_objects(content, m)?;
    let mut doc = Document::new(name);
    doc.pages[0].name = "Mermaid".into();
    doc.pages[0].objects = objs;
    Ok(Imported { doc, report: Report::default() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkboard_core::text::ApproxMeasurer;

    fn objs(src: &str) -> Vec<Object> {
        to_objects(src, &ApproxMeasurer).unwrap()
    }
    fn boxes(o: &[Object]) -> Vec<(&Object, &BoxObj)> {
        o.iter().filter_map(|x| if let Kind::Box(b) = &x.kind { Some((x, b)) } else { None }).collect()
    }
    fn conns(o: &[Object]) -> Vec<&Connector> {
        o.iter().filter_map(|x| if let Kind::Connector(c) = &x.kind { Some(c) } else { None }).collect()
    }
    fn page_of(o: Vec<Object>) -> Page {
        let mut p = Page::new("t");
        p.objects = o;
        p
    }

    const FLOW: &str = "flowchart LR\n  subgraph Front [Frontend]\n    A[Client] --> B(API Gateway)\n  end\n  B -->|REST| C{Auth ?}\n  C -->|ok| D[(Database)]\n  C -->|ko| E((Error))\n  D -.-> F[Cache]\n  E --> A\n  A --> A";

    #[test]
    fn flowchart_builds_bound_connectors_and_clusters() {
        let o = objs(FLOW);
        let b = boxes(&o);
        let texts: Vec<String> = b.iter().map(|x| x.1.label.text.to_plain()).collect();
        for want in ["Client", "API Gateway", "Auth ?", "Database", "Error", "Cache", "Frontend"] {
            assert!(texts.iter().any(|t| t == want), "{want} missing from {texts:?}");
        }
        assert!(b.iter().any(|x| x.1.geom == BoxGeom::Diamond) && b.iter().any(|x| x.1.geom == BoxGeom::Ellipse));
        let c = conns(&o);
        assert_eq!(c.len(), 7);
        assert!(c.iter().all(|c| c.targets().count() == 2), "all the arrows are bound to shapes");
        let p = page_of(o.clone());
        assert!(conns(&p.objects).iter().all(|c| p.connector_path(c).is_some()));
        let rest = c.iter().find(|c| c.label.as_ref().is_some_and(|l| l.text.to_plain() == "REST")).unwrap();
        assert_eq!(rest.head_end, Arrowhead::Triangle);
        assert!(o.iter().any(|x| x.style.dash == Dash::Dashed && matches!(x.kind, Kind::Connector(_))), "dotted arrow");
        assert!(o.iter().any(|x| x.style.dash == Dash::Dashed && matches!(&x.kind, Kind::Box(b) if b.geom == BoxGeom::Rect)), "subgraph frame");
    }

    #[test]
    fn layout_direction_and_no_overlap() {
        let o = objs("graph TD\n A --> B\n B --> C\n A --> C\n C --> D");
        let b = boxes(&o);
        let y = |t: &str| b.iter().find(|x| x.1.label.text.to_plain() == t).unwrap().1.frame.y;
        assert!(y("A") < y("B") && y("B") < y("C") && y("C") < y("D"));
        for (i, p) in b.iter().enumerate() {
            for q in b.iter().skip(i + 1) {
                assert!(!p.1.frame.rect().intersects(&q.1.frame.rect()), "chevauchement");
            }
        }
        let lr = objs("graph LR\n A --> B");
        let bb = boxes(&lr);
        assert!(bb[0].1.frame.x < bb[1].1.frame.x);
    }

    #[test]
    fn long_edge_gets_via_points() {
        let o = objs("graph TD\n A --> B --> C --> D\n A --> D");
        let long = conns(&o).into_iter().find(|c| !c.via.is_empty()).expect("long edge with relays");
        assert_eq!(long.via.len(), 2);
    }

    #[test]
    fn styles_classes_and_chains() {
        let o = objs(
            "flowchart TD\n A[One]:::ok --> B[Two] & C[Three]\n classDef ok fill:#d3f9d8,stroke:#2f9e44,stroke-width:3px\n style B fill:#ffc9c9,color:#e03131",
        );
        let b = boxes(&o);
        let find = |t: &str| b.iter().find(|x| x.1.label.text.to_plain() == t).unwrap();
        assert_eq!(find("One").0.style.fill, Some(Color::Rgb(0xd3, 0xf9, 0xd8)));
        assert_eq!(find("One").0.style.stroke, Color::Rgb(0x2f, 0x9e, 0x44));
        assert_eq!(find("Two").0.style.fill, Some(Color::Rgb(0xff, 0xc9, 0xc9)));
        assert_eq!(find("Two").1.label.text.0[0].style.color, Some(Color::Rgb(0xe0, 0x31, 0x31)));
        assert_eq!(conns(&o).len(), 2, "A --> B & C: two edges");
    }

    #[test]
    fn link_syntaxes() {
        let o = objs("graph LR\n A --- B\n B ==> C\n C -- text --> D\n D <--> E\n E -.- F\n F ~~~ G");
        let c = conns(&o);
        assert_eq!(c.len(), 5, "the invisible link is not drawn");
        assert_eq!(c[0].head_end, Arrowhead::None);
        assert_eq!(c[2].label.as_ref().unwrap().text.to_plain(), "text");
        assert!(c[3].head_start == Arrowhead::Triangle && c[3].head_end == Arrowhead::Triangle);
        assert!(o.iter().any(|x| x.style.width > 4.0 && matches!(x.kind, Kind::Connector(_))), "thick stroke");
        // the 7 nodes A..G exist, G is placed thanks to the invisible link
        assert_eq!(boxes(&o).len(), 7);
    }

    #[test]
    fn state_diagram_with_start_end_choice_and_composite() {
        let o = objs(
            "stateDiagram-v2\n [*] --> Idle\n Idle --> Running : start\n Running --> Idle : stop\n Running --> [*]\n state c <<choice>>\n Idle --> c\n state Group {\n  [*] --> Inner\n }",
        );
        let b = boxes(&o);
        assert!(b.iter().any(|x| x.1.geom == BoxGeom::Diamond));
        assert!(b.iter().any(|x| x.1.label.text.to_plain() == "Inner"));
        assert!(b.iter().any(|x| x.1.label.text.to_plain() == "Group"), "composite state = titled frame");
        let c = conns(&o);
        assert!(c.iter().any(|c| c.label.as_ref().is_some_and(|l| l.text.to_plain() == "stop")));
        let p = page_of(o.clone());
        assert!(conns(&p.objects).iter().all(|c| p.connector_path(c).is_some()));
    }

    #[test]
    fn opposite_edges_and_self_loops_do_not_overlap_exactly() {
        let o = objs("graph LR\n A --> B\n B --> A\n A --> A");
        let c = conns(&o);
        let p = page_of(o.clone());
        let (p0, p1) = (p.connector_path(c[0]).unwrap(), p.connector_path(c[1]).unwrap());
        let mid = |v: &Vec<Pt>| v[v.len() / 2];
        assert!(mid(&p0).dist(mid(&p1)) > 20.0, "the two opposite arrows are spread apart");
        assert!(c[2].via.len() == 2, "self loop");
    }

    #[test]
    fn sequence_diagram_has_lifelines_messages_notes_and_blocks() {
        let o = objs(
            "sequenceDiagram\n autonumber\n participant U as User\n U->>API: Request\n loop Retry\n API-->>DB: Read\n end\n Note right of DB: warm cache\n API->>U: Response\n U->>U: compute",
        );
        let b = boxes(&o);
        for want in ["User", "API", "DB", "1. Request", "2. Read", "warm cache", "3. Response", "loop Retry"] {
            assert!(b.iter().any(|x| x.1.label.text.to_plain() == want), "{want} manquant");
        }
        assert!(b.iter().any(|x| x.1.geom == BoxGeom::Sticky));
        let c = conns(&o);
        assert_eq!(c.iter().filter(|c| c.head_end == Arrowhead::Triangle).count(), 4, "4 arrowed messages");
        assert!(c.iter().filter(|c| c.head_end == Arrowhead::None).count() >= 3, "lifelines");
        let y = |t: &str| b.iter().find(|x| x.1.label.text.to_plain() == t).unwrap().1.frame.y;
        assert!(y("1. Request") < y("2. Read") && y("2. Read") < y("3. Response"));
    }

    #[test]
    fn errors_are_explicit() {
        assert!(matches!(to_objects("erDiagram\n A ||--o{ B : has", &ApproxMeasurer), Err(FormatError::Unsupported(_))));
        assert!(to_objects("", &ApproxMeasurer).is_err());
        assert!(to_objects("graph TD", &ApproxMeasurer).is_err());
        assert!(to_objects("sequenceDiagram", &ApproxMeasurer).is_err());
        assert!(to_objects("%% nothing\n", &ApproxMeasurer).is_err());
    }

    #[test]
    fn front_matter_comments_and_semicolons_are_handled() {
        let o = objs("---\ntitle: x\n---\n%% commentaire\ngraph TD;\n A-->B; B-->C %% fin");
        assert_eq!(boxes(&o).len(), 3);
        assert_eq!(conns(&o).len(), 2);
    }

    #[test]
    fn multiline_and_quoted_labels() {
        let o = objs("graph TD\n A[\"Text with (parentheses)\"] --> B[Line 1<br/>Line 2]");
        let t: Vec<String> = boxes(&o).iter().map(|x| x.1.label.text.to_plain()).collect();
        assert!(t.contains(&"Text with (parentheses)".to_owned()));
        assert!(t.contains(&"Line 1\nLine 2".to_owned()));
    }
}
