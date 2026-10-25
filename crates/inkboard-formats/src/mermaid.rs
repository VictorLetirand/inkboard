//! Mermaid : `flowchart` / `graph`, `stateDiagram`, `sequenceDiagram`, `classDiagram`, `erDiagram`.
//!
//! The text becomes real shapes linked by real connectors: moving a node
//! makes its arrows follow. The layout comes from `inkboard_core::layout`.

use crate::common::{FormatError, Imported, Report, parse_color};
use inkboard_core::layout::{Direction, Layout, NodeSpec, layered};
use inkboard_core::num;
use inkboard_core::text::Measurer;
use inkboard_core::{
    Align, Anchor, Arrowhead, BoxGeom, BoxObj, Color, Connector, Dash, Document, End, FontFamily, Frame, Id, Kind, Label, Object, Pt, Rect, RichText, Route,
    Style,
};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

mod uml;

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
    line: Line,
}

/// Stroke of a link: `---`, `-.-`, `===` or `~~~` (not drawn).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Line {
    Solid,
    Dashed,
    Thick,
    Invisible,
}

#[derive(Debug)]
struct Sub {
    id: String,
    title: String,
    members: Vec<String>,
    parent: Option<usize>,
    /// `direction` written inside the subgraph.
    dir: Option<Direction>,
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
        let i = if let Some(&i) = self.index.get(id) {
            i
        } else {
            let sub = self.stack.last().copied();
            self.nodes.push(Node { id: id.to_owned(), label: id.to_owned(), shape: Shape::Rect, props: HashMap::new(), classes: Vec::new() });
            let i = self.nodes.len() - 1;
            self.index.insert(id.to_owned(), i);
            if let Some(s) = sub {
                self.subs[s].members.push(id.to_owned());
            }
            i
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
            let Some(quote_at) = s[k + 1..].find('"') else { break };
            let quote = k + 1 + quote_at;
            let Some(close_at) = s[quote + 1..].find(close) else { break };
            (s[k + 1..quote].to_owned(), quote + 1 + close_at + close.len())
        } else {
            let Some(close_rel) = s[k..].find(close) else { break };
            (s[k..k + close_rel].to_owned(), k + close_rel + close.len())
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
    let base = Link { from: String::new(), to: String::new(), label: String::new(), start: false, end: false, line: Line::Solid };
    if let Some(m) = LABELED.captures(s) {
        return Some((
            m[0].len(),
            Link { label: clean(&m[3]), start: !m[1].is_empty(), end: !m[5].is_empty(), line: if &m[2] == "==" { Line::Thick } else { Line::Solid }, ..base },
        ));
    }
    if let Some(m) = DOT_LABELED.captures(s) {
        return Some((m[0].len(), Link { label: clean(&m[2]), start: !m[1].is_empty(), end: !m[3].is_empty(), line: Line::Dashed, ..base }));
    }
    if let Some(m) = DOTTED.captures(s) {
        return Some((
            m[0].len(),
            Link { label: m.get(3).map_or(String::new(), |l| clean(l.as_str())), start: !m[1].is_empty(), end: !m[2].is_empty(), line: Line::Dashed, ..base },
        ));
    }
    if s.starts_with("~~~") {
        return Some((3, Link { line: Line::Invisible, ..base }));
    }
    if let Some(m) = PLAIN.captures(s) {
        return Some((
            m[0].len(),
            Link {
                label: m.get(4).map_or(String::new(), |l| clean(l.as_str())),
                start: !m[1].is_empty(),
                end: !m[3].is_empty(),
                line: if m[2].starts_with('=') { Line::Thick } else { Line::Solid },
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
        let mut group = Vec::new();
        loop {
            skip(i);
            let Some(t) = parse_node(s, *i) else { return if group.is_empty() { None } else { Some(group) } };
            *i = t.end;
            group.push(f.node(&t.id, t.label, t.shape, t.class.as_deref()));
            skip(i);
            if s[*i..].starts_with('&') {
                *i += 1;
                continue;
            }
            return Some(group);
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

/// Splits a line at the `;` that end a statement, not at those inside a quoted label (`A["a; b"]`).
fn split_statements(line: &str) -> Vec<&str> {
    let (mut out, mut start, mut quoted) = (Vec::new(), 0, false);
    for (i, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ';' if !quoted => {
                out.push(&line[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&line[start..]);
    out
}

fn parse_flowchart(lines: &[String], f: &mut Flow) {
    static SUB: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^subgraph\s+(.+)$").unwrap());
    static SUB_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([\w\p{L}]+)\s*\[(.*)\]\s*$").unwrap());
    static CLASSDEF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^classDef\s+([\w,-]+)\s+(.+)$").unwrap());
    static CLASS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^class\s+(\S+)\s+([\w-]+)$").unwrap());
    static STYLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^style\s+([\w\p{L}]+)\s+(.+)$").unwrap());
    for raw in lines {
        for stmt in split_statements(raw) {
            let s = stmt.trim();
            if s.is_empty() {
                continue;
            }
            if let Some(m) = SUB.captures(s) {
                let rest = m[1].trim();
                let (id, title) = SUB_ID.captures(rest).map_or((rest.to_owned(), clean(rest)), |c| (c[1].to_owned(), clean(&c[2])));
                f.subs.push(Sub { id, title, members: Vec::new(), parent: f.stack.last().copied(), dir: None });
                f.stack.push(f.subs.len() - 1);
            } else if s.eq_ignore_ascii_case("end") {
                f.stack.pop();
            } else if let Some(d) = s.strip_prefix("direction ") {
                if let Some(&i) = f.stack.last() {
                    f.subs[i].dir = Some(dir_of(d.trim()));
                }
            } else if ["linkStyle", "click", "accTitle", "accDescr", "title"].iter().any(|k| s.starts_with(k)) {
                // directives without effect on the board
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

/// Whether subgraph `i` holds a node that is not one of the `placeholders` (directly or in a nested subgraph).
fn has_real_member(i: usize, f: &Flow, placeholders: &HashSet<String>) -> bool {
    f.subs[i].members.iter().any(|m| !placeholders.contains(m))
        || f.subs.iter().enumerate().any(|(k, s)| s.parent == Some(i) && has_real_member(k, f, placeholders))
}

/// A link written to a subgraph (`B --> Back`) makes `Flow::node` create a node named like the subgraph. It is not a node of the diagram:
/// it is dropped, and `build_flow` attaches the link to the frame of the subgraph.
fn drop_subgraph_placeholders(f: &mut Flow) {
    let candidates: HashSet<String> = f
        .nodes
        .iter()
        .filter(|n| n.label == n.id && n.shape == Shape::Rect && n.props.is_empty() && n.classes.is_empty() && f.subs.iter().any(|s| s.id == n.id))
        .map(|n| n.id.clone())
        .collect();
    let dropped: HashSet<String> =
        candidates.iter().filter(|id| f.subs.iter().enumerate().any(|(i, s)| &s.id == *id && has_real_member(i, f, &candidates))).cloned().collect();
    if dropped.is_empty() {
        return;
    }
    f.nodes.retain(|n| !dropped.contains(&n.id));
    f.index = f.nodes.iter().enumerate().map(|(i, n)| (n.id.clone(), i)).collect();
    for s in &mut f.subs {
        s.members.retain(|m| !dropped.contains(m));
    }
}

/// The node standing for subgraph `i` in the layout: the first one it contains.
fn first_member(i: usize, f: &Flow) -> Option<usize> {
    f.subs[i]
        .members
        .iter()
        .find_map(|id| f.index.get(id).copied())
        .or_else(|| f.subs.iter().enumerate().filter(|(_, s)| s.parent == Some(i)).find_map(|(k, _)| first_member(k, f)))
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
            match f.stack.last() {
                Some(&i) => f.subs[i].dir = Some(dir_of(&m[1])),
                None => f.dir = dir_of(&m[1]),
            }
        } else if let Some(m) = ALIAS.captures(s) {
            f.node(&m[2], Some(m[1].to_owned()), Some(Shape::Round), None);
        } else if let Some(m) = SPECIAL.captures(s) {
            f.node(&m[1], Some(String::new()), Some(if &m[2] == "choice" { Shape::Diamond } else { Shape::Bar }), None);
        } else if let Some(m) = COMPOSITE.captures(s) {
            let title = m.get(1).map_or(m[2].to_owned(), |t| t.as_str().to_owned());
            f.subs.push(Sub { id: m[2].to_owned(), title, members: Vec::new(), parent: f.stack.last().copied(), dir: None });
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
            f.links.push(Link { from: a, to: b, label: clean(m.get(3).map_or("", |x| x.as_str())), start: false, end: true, line: Line::Solid });
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
    let mut obj = BoxObj { geom, frame: Frame::new(0.0, 0.0, w, h), label: Label::new(&n.label, FONT, Align::Center), lang: None };
    for _ in 0..6 {
        let before = obj.frame.h;
        obj.fit(m);
        if (obj.frame.h - before).abs() < 0.5 {
            break;
        }
        obj.frame.h += 4.0;
    }
    h = obj.frame.h;
    (geom, w, h)
}

/// Room between the frame of a subgraph and what it holds: on the sides, above (the title) and below.
const FRAME_SIDE: f64 = 28.0;
const FRAME_TOP: f64 = 46.0;
const FRAME_BOTTOM: f64 = 28.0;

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
    let r = acc.map(|a| Rect::new(a.x - FRAME_SIDE, a.y - FRAME_TOP, a.w + 2.0 * FRAME_SIDE, a.h + FRAME_TOP + FRAME_BOTTOM));
    cache.insert(i, r);
    r
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

/// The links whose two ends exist. An end is a node, or a subgraph: laid out as its first node, and attached to its frame.
struct Resolved {
    /// Layout edges (node indices).
    edges: Vec<(usize, usize)>,
    /// Index in `Flow::links` of each edge.
    link_idx: Vec<usize>,
    /// The subgraph that each end of each edge stands for, if any.
    frame_ends: Vec<[Option<usize>; 2]>,
}

fn resolve_links(flow: &Flow) -> Resolved {
    let sub_by_id: HashMap<&str, usize> = flow.subs.iter().enumerate().rev().map(|(i, s)| (s.id.as_str(), i)).collect();
    let end_of = |id: &str| -> Option<(usize, Option<usize>)> {
        if let Some(&k) = flow.index.get(id) {
            return Some((k, None));
        }
        let s = *sub_by_id.get(id)?;
        Some((first_member(s, flow)?, Some(s)))
    };
    let mut r = Resolved { edges: Vec::new(), link_idx: Vec::new(), frame_ends: Vec::new() };
    for (i, l) in flow.links.iter().enumerate() {
        if let (Some(a), Some(b)) = (end_of(&l.from), end_of(&l.to)) {
            r.edges.push((a.0, b.0));
            r.link_idx.push(i);
            r.frame_ends.push([a.1, b.1]);
        }
    }
    r
}

/// The frame and the title of every subgraph that holds something (outer ones first, so that inner ones are drawn above), and the id of each frame.
fn subgraph_objects(flow: &Flow, rects: &[Rect], measurer: &dyn Measurer) -> (Vec<Object>, HashMap<usize, Id>) {
    let mut bounds: HashMap<usize, Option<Rect>> = HashMap::new();
    let depth = |mut i: usize| {
        let mut d = 0;
        while let Some(p) = flow.subs[i].parent {
            d += 1;
            i = p;
        }
        d
    };
    let mut order: Vec<usize> = (0..flow.subs.len()).collect();
    order.sort_by_key(|&i| depth(i));
    let mut out: Vec<Object> = Vec::new();
    let mut frame_id: HashMap<usize, Id> = HashMap::new();
    for i in order {
        let Some(b) = bounds_of(i, flow, rects, &mut bounds) else { continue };
        let frame = Object::new_box(
            BoxGeom::Rect,
            Frame::from_rect(b),
            Style { dash: Dash::Dashed, width: 2.0, opacity: 0.85, roundness: 0.1, ..Style::default() },
            Label::default(),
        );
        frame_id.insert(i, frame.id.clone());
        out.push(frame);
        let mut t = Object::new_box(
            BoxGeom::Text,
            Frame::new(b.x + 12.0, b.y + 8.0, 10.0, 10.0),
            Style { width: 0.0, ..Style::default() },
            Label::new(&flow.subs[i].title, 16.0, Align::Left),
        );
        if let Kind::Box(bx) = &mut t.kind {
            bx.fit(measurer);
        }
        out.push(t);
    }
    (out, frame_id)
}

/// What the layout arranges inside a group: a node, or the whole content of a subgraph (a block).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Item {
    Node(usize),
    Block(usize),
}

/// A laid-out group of nodes and blocks, relative to the top-left corner of its content.
#[derive(Default)]
struct Group {
    w: f64,
    h: f64,
    /// Center of each node, by node index.
    centers: Vec<(usize, Pt)>,
    /// Waypoints of each edge, by edge index (the ends are the centers of what the edge links in this group).
    routes: Vec<(usize, Vec<Pt>)>,
}

/// Lays a flow out one subgraph at a time: the content of a subgraph is arranged on its own (with its own `direction`, when nothing links it to
/// the outside), then it takes part as one block in the layout of the group that holds it, so frames never overlap and hold their members.
struct LayoutCtx<'a> {
    flow: &'a Flow,
    sizes: &'a [(BoxGeom, f64, f64)],
    edges: &'a [(usize, usize)],
    /// Innermost subgraph of each node.
    node_sub: Vec<Option<usize>>,
}

impl LayoutCtx<'_> {
    /// What stands for `node` in group `g` (`None`: the page): the node itself, or the block of the subgraph that holds it; `None` if it is outside `g`.
    fn item_of(&self, node: usize, g: Option<usize>) -> Option<Item> {
        let mut cur = self.node_sub[node];
        if cur == g {
            return Some(Item::Node(node));
        }
        while let Some(c) = cur {
            let parent = self.flow.subs[c].parent;
            if parent == g {
                return Some(Item::Block(c));
            }
            cur = parent;
        }
        None
    }

    /// Whether an edge links the inside of subgraph `sub` to the outside.
    fn crosses(&self, sub: usize) -> bool {
        self.edges.iter().any(|&(a, b)| self.item_of(a, Some(sub)).is_some() != self.item_of(b, Some(sub)).is_some())
    }

    fn group(&self, g: Option<usize>, dir: Direction) -> Group {
        let mut items: Vec<Item> = (0..self.flow.nodes.len()).filter(|&n| self.node_sub[n] == g).map(Item::Node).collect();
        let mut children: HashMap<usize, Group> = HashMap::new();
        for (c, s) in self.flow.subs.iter().enumerate().filter(|(_, s)| s.parent == g) {
            let child = self.group(Some(c), if self.crosses(c) { dir } else { s.dir.unwrap_or(dir) });
            if !child.centers.is_empty() {
                items.push(Item::Block(c));
                children.insert(c, child);
            }
        }
        if items.is_empty() {
            return Group::default();
        }
        let size = |it: &Item| match *it {
            Item::Node(n) => NodeSpec { w: self.sizes[n].1, h: self.sizes[n].2 },
            Item::Block(c) => NodeSpec { w: children[&c].w + 2.0 * FRAME_SIDE, h: children[&c].h + FRAME_TOP + FRAME_BOTTOM },
        };
        let specs: Vec<NodeSpec> = items.iter().map(size).collect();
        let position: HashMap<Item, usize> = items.iter().enumerate().map(|(i, it)| (*it, i)).collect();
        let (mut edges, mut edge_ids) = (Vec::new(), Vec::new());
        for (k, &(a, b)) in self.edges.iter().enumerate() {
            if let (Some(ia), Some(ib)) = (self.item_of(a, g), self.item_of(b, g))
                && ia != ib
            {
                edges.push((position[&ia], position[&ib]));
                edge_ids.push(k);
            }
        }
        let lay = layered(&specs, &edges, dir);
        let (mut lo, mut hi) = (Pt::new(f64::INFINITY, f64::INFINITY), Pt::new(f64::NEG_INFINITY, f64::NEG_INFINITY));
        for (c, sp) in lay.centers.iter().zip(&specs) {
            lo = Pt::new(lo.x.min(c.x - sp.w / 2.0), lo.y.min(c.y - sp.h / 2.0));
            hi = Pt::new(hi.x.max(c.x + sp.w / 2.0), hi.y.max(c.y + sp.h / 2.0));
        }
        let shift = Pt::new(-lo.x, -lo.y);
        let mut out = Group { w: hi.x - lo.x, h: hi.y - lo.y, ..Group::default() };
        for (i, it) in items.iter().enumerate() {
            let c = lay.centers[i] + shift;
            match *it {
                Item::Node(n) => out.centers.push((n, c)),
                Item::Block(b) => {
                    let origin = c - Pt::new(specs[i].w / 2.0 - FRAME_SIDE, specs[i].h / 2.0 - FRAME_TOP);
                    let child = &children[&b];
                    out.centers.extend(child.centers.iter().map(|&(n, p)| (n, p + origin)));
                    out.routes.extend(child.routes.iter().map(|(k, r)| (*k, r.iter().map(|&p| p + origin).collect())));
                }
            }
        }
        out.routes.extend(edge_ids.iter().zip(&lay.routes).map(|(&k, r)| (k, r.iter().map(|&p| p + shift).collect())));
        out
    }
}

/// Innermost subgraph of each node.
fn innermost_subs(flow: &Flow) -> Vec<Option<usize>> {
    let mut node_sub = vec![None; flow.nodes.len()];
    for (i, s) in flow.subs.iter().enumerate() {
        for id in &s.members {
            if let Some(&k) = flow.index.get(id) {
                node_sub[k] = Some(i);
            }
        }
    }
    node_sub
}

/// Whether `node` is in subgraph `sub`, directly or in a subgraph inside it.
fn in_subgraph(flow: &Flow, node_sub: &[Option<usize>], node: usize, sub: usize) -> bool {
    let mut cur = node_sub[node];
    while let Some(c) = cur {
        if c == sub {
            return true;
        }
        cur = flow.subs[c].parent;
    }
    false
}

/// Moves the waypoints of a link around the nodes it does not link, which its straight lines would cross. `ends`: each end as a node (the one
/// the layout used) and the subgraph it stands for, if any; what is inside a subgraph that a link ends on is not in its way.
fn clear_of_nodes(conn: &mut Connector, ends: [(usize, Option<usize>); 2], flow: &Flow, node_sub: &[Option<usize>], rects: &[Rect]) {
    let blocks = |n: usize| ends.iter().all(|&(e, sub)| n != e && !sub.is_some_and(|s| in_subgraph(flow, node_sub, n, s)));
    let obstacles: Vec<Rect> = (0..rects.len()).filter(|&n| blocks(n)).map(|n| rects[n]).collect();
    let path: Vec<Pt> = std::iter::once(rects[ends[0].0].center()).chain(conn.via.iter().copied()).chain(std::iter::once(rects[ends[1].0].center())).collect();
    let routed = inkboard_core::route::detour(&path, &obstacles, 14.0);
    conn.via = routed[1..routed.len() - 1].to_vec();
}

/// Positions of the nodes (centers) and routes of the edges of a flow.
fn layout_flow(flow: &Flow, sizes: &[(BoxGeom, f64, f64)], edges: &[(usize, usize)]) -> Layout {
    let g = LayoutCtx { flow, sizes, edges, node_sub: innermost_subs(flow) }.group(None, flow.dir);
    let mut lay = Layout { centers: vec![Pt::ZERO; flow.nodes.len()], routes: vec![Vec::new(); edges.len()] };
    for (n, c) in g.centers {
        lay.centers[n] = c;
    }
    for (k, r) in g.routes {
        lay.routes[k] = r;
    }
    lay
}

fn build_flow(flow: &Flow, measurer: &dyn Measurer) -> Vec<Object> {
    let sizes: Vec<(BoxGeom, f64, f64)> = flow.nodes.iter().map(|n| node_size(n, measurer)).collect();
    let Resolved { edges, link_idx, frame_ends } = resolve_links(flow);
    let lay = layout_flow(flow, &sizes, &edges);
    let mut nodes: Vec<Object> = Vec::new();
    let mut rects: Vec<Rect> = Vec::new();
    for (i, n) in flow.nodes.iter().enumerate() {
        let (g, w, h) = sizes[i];
        let center = lay.centers[i];
        let rect = Rect::new(center.x - w / 2.0, center.y - h / 2.0, w, h);
        rects.push(rect);
        nodes.push(node_object(n, g, Frame::from_rect(rect), &flow.classes));
    }
    let (mut out, frame_id) = subgraph_objects(flow, &rects, measurer);
    let ids: Vec<Id> = nodes.iter().map(|o| o.id.clone()).collect();
    out.extend(nodes);
    // edges: real bound connectors; layout relays become waypoints
    let mut pair_count: HashMap<(usize, usize), usize> = HashMap::new();
    for (&(a, b), ends) in edges.iter().zip(&frame_ends) {
        if ends.iter().all(Option::is_none) {
            *pair_count.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    let node_sub = innermost_subs(flow);
    let mut pair_seen: HashMap<(usize, usize), usize> = HashMap::new();
    for (k, &li) in link_idx.iter().enumerate() {
        let link = &flow.links[li];
        let (a, b) = edges[k];
        if link.line == Line::Invisible {
            continue;
        }
        let head = |on: bool| if on { Arrowhead::Triangle } else { Arrowhead::None };
        let style = Style {
            width: if link.line == Line::Thick { 5.0 } else { 2.5 },
            dash: if link.line == Line::Dashed { Dash::Dashed } else { Dash::Solid },
            ..Style::default()
        };
        let [sub_a, sub_b] = frame_ends[k];
        let target = |node: usize, sub: Option<usize>| End::bound(sub.and_then(|s| frame_id.get(&s)).unwrap_or(&ids[node]).clone());
        let mut conn = Connector::new(target(a, sub_a), target(b, sub_b));
        conn.head_end = head(link.end);
        conn.head_start = head(link.start);
        if !link.label.is_empty() {
            conn.label = Some(Label::new(&link.label, 15.0, Align::Center));
        }
        if sub_a.is_some() || sub_b.is_some() {
            // attached to a frame: the waypoints computed for the node inside it would cross it
        } else if a == b {
            let src_rect = rects[a];
            conn.start = End::Bound { target: ids[a].clone(), anchor: Anchor::Frac { fx: 1.0, fy: 0.25 } };
            conn.end = End::Bound { target: ids[a].clone(), anchor: Anchor::Frac { fx: 1.0, fy: 0.75 } };
            conn.via = vec![Pt::new(src_rect.right() + 34.0, src_rect.y + src_rect.h * 0.25), Pt::new(src_rect.right() + 34.0, src_rect.y + src_rect.h * 0.75)];
        } else {
            let route = &lay.routes[k];
            if route.len() > 2 {
                conn.via = route[1..route.len() - 1].to_vec();
            } else {
                let key = (a.min(b), a.max(b));
                let pair_n = pair_count[&key];
                let idx = pair_seen.entry(key).or_default();
                if pair_n > 1 {
                    let (p, q) = (rects[key.0].center(), rects[key.1].center());
                    let delta = q - p;
                    let len = delta.len().max(1.0);
                    let perp = Pt::new(-delta.y / len, delta.x / len);
                    let off = (num::f64_of(*idx) - (num::f64_of(pair_n) - 1.0) / 2.0) * 44.0;
                    conn.via = vec![p.lerp(q, 0.5) + perp * off];
                    conn.route = Route::Curve;
                }
                *idx += 1;
            }
        }
        if a != b && conn.route == Route::Straight {
            clear_of_nodes(&mut conn, [(a, sub_a), (b, sub_b)], flow, &node_sub, &rects);
        }
        out.push(Object::new(Kind::Connector(conn), style));
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

/// A row of a sequence diagram (`r`: its position, in rows).
enum SeqRow {
    Msg { from: String, to: String, arrow: String, text: String, r: f64 },
    Note { kind: String, ids: Vec<String>, text: String, r: f64 },
}

/// Parsed sequence diagram: participants `(id, label)`, rows, frames `(label, first row, last row)`,
/// separators inside the frames `(label, row)`, and the row after the last one.
struct Sequence {
    parts: Vec<(String, String)>,
    rows: Vec<SeqRow>,
    frames: Vec<(String, f64, f64)>,
    marks: Vec<(String, f64)>,
    end: f64,
}

fn parse_sequence(lines: &[String]) -> Sequence {
    static PARTICIPANT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(?:participant|actor)\s+(\S+)(?:\s+as\s+(.+))?$").unwrap());
    static BLOCK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(loop|alt|opt|par|critical|break|rect)\b\s*(.*)$").unwrap());
    static MARK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(else|and|option)\b\s*(.*)$").unwrap());
    static NOTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^Note\s+(over|left of|right of)\s+([^:]+):\s*(.*)$").unwrap());
    static MSG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([^\s>:\-]+)\s*(-->>|->>|-->|->|--x|-x|--\)|-\))\s*[+-]?([^\s:]+)\s*:\s*(.*)$").unwrap());
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
    let mut rows: Vec<SeqRow> = Vec::new();
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
            rows.push(SeqRow::Note { kind: c[1].to_ascii_lowercase(), ids, text: clean(&c[3]), r: row });
            row += 1.0;
        } else if let Some(c) = MSG.captures(s) {
            part(&mut parts, &c[1], None);
            part(&mut parts, &c[3], None);
            count += 1;
            rows.push(SeqRow::Msg {
                from: c[1].to_owned(),
                to: c[3].to_owned(),
                arrow: c[2].to_owned(),
                text: format!("{}{}", if auto { format!("{count}. ") } else { String::new() }, clean(&c[4])),
                r: row,
            });
            row += 1.0;
        }
    }
    Sequence { parts, rows, frames, marks, end: row }
}

fn build_sequence(lines: &[String], measurer: &dyn Measurer) -> Result<Vec<Object>, FormatError> {
    let Sequence { parts, rows, frames, marks, end: row } = parse_sequence(lines);
    if parts.is_empty() {
        return Err(FormatError::Invalid("no message found".into()));
    }
    let widths: Vec<f64> = parts.iter().map(|p| (measurer.width(&p.1, FontFamily::Sans, FONT, false) + 36.0).max(120.0)).collect();
    let mut xs: HashMap<&str, f64> = HashMap::new();
    let mut x = 0.0;
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            x += (f64::midpoint(widths[i - 1], widths[i]) + 60.0).max(220.0);
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
        out.push(text_object(label, left + 8.0, y_of(*r0) - 20.0, 14.0, Align::Left, measurer));
    }
    for (label, r) in &marks {
        out.push(line(Pt::new(left, y_of(*r) - 12.0), Pt::new(right, y_of(*r) - 12.0), false, true, 1.5));
        out.push(text_object(label, left + 8.0, y_of(*r) - 12.0, 14.0, Align::Left, measurer));
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
            SeqRow::Note { kind, ids, text, r } => {
                let cx: Vec<f64> = ids.iter().filter_map(|i| xs.get(i.as_str()).copied()).collect();
                let (mn, mx) = (cx.iter().copied().fold(f64::INFINITY, f64::min), cx.iter().copied().fold(f64::NEG_INFINITY, f64::max));
                let w = 160.0f64.max(mx - mn + 100.0);
                let sx = match kind.as_str() {
                    "left of" => mn - w / 2.0 - 20.0,
                    "right of" => mx + w / 2.0 + 20.0,
                    _ => f64::midpoint(mn, mx),
                };
                let mut n = Object::new_box(
                    BoxGeom::Sticky,
                    Frame::new(sx - w / 2.0, y_of(*r) - 20.0, w, 54.0),
                    Style { fill: Some(inkboard_core::style::NOTE_YELLOW), width: 0.0, ..Style::default() },
                    Label::new(text, 15.0, Align::Center),
                );
                if let Kind::Box(b) = &mut n.kind {
                    b.fit(measurer);
                }
                out.push(n);
            }
            SeqRow::Msg { from, to, arrow, text, r } => {
                let (a, b, y) = (xs[from.as_str()], xs[to.as_str()], y_of(*r));
                let dashed = arrow.starts_with("--");
                let head = arrow.ends_with(">>") || arrow.ends_with('x') || arrow.ends_with(')');
                if from == to {
                    let mut conn = Connector::new(End::free(Pt::new(a, y - 10.0)), End::free(Pt::new(a, y + 14.0)));
                    conn.via = vec![Pt::new(a + 46.0, y - 10.0), Pt::new(a + 46.0, y + 14.0)];
                    conn.head_end = Arrowhead::Triangle;
                    out.push(Object::new(
                        Kind::Connector(conn),
                        Style { width: 2.5, dash: if dashed { Dash::Dashed } else { Dash::Solid }, ..Style::default() },
                    ));
                    if !text.is_empty() {
                        out.push(text_object(text, a + 56.0, y - 8.0, 15.0, Align::Left, measurer));
                    }
                } else {
                    out.push(line(Pt::new(a, y), Pt::new(b, y), head, dashed, 2.5));
                    if !text.is_empty() {
                        let mut caption = text_object(text, 0.0, 0.0, 15.0, Align::Center, measurer);
                        if let Kind::Box(bx) = &mut caption.kind {
                            bx.frame.x = f64::midpoint(a, b) - bx.frame.w / 2.0;
                            bx.frame.y = y - bx.frame.h - 4.0;
                        }
                        out.push(caption);
                    }
                }
            }
        }
    }
    Ok(out)
}

/// Largest flowchart imported. The automatic layout costs more than linear time (seconds for a thousand nodes), so a bigger diagram
/// would freeze the application, or the server of the agent that asked for it.
const MAX_FLOW_NODES: usize = 1_000;
const MAX_FLOW_LINKS: usize = 2_500;

fn check_flow_size(f: &Flow) -> Result<(), FormatError> {
    if f.nodes.len() > MAX_FLOW_NODES || f.links.len() > MAX_FLOW_LINKS {
        return Err(FormatError::Unsupported(format!(
            "diagram too large ({} nodes, {} links; at most {MAX_FLOW_NODES} nodes and {MAX_FLOW_LINKS} links)",
            f.nodes.len(),
            f.links.len()
        )));
    }
    Ok(())
}

/// Converts Mermaid text to objects ready to be placed on a page.
///
/// # Errors
///
/// [`FormatError`] when the text is empty, holds no node, state or message, or its diagram type is not supported.
pub fn to_objects(src: &str, m: &dyn Measurer) -> Result<Vec<Object>, FormatError> {
    static FLOW: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(?:flowchart|graph)(?:-elk)?\b\s*(TD|TB|LR|RL|BT)?").unwrap());
    let mut lines: Vec<&str> = src.lines().collect();
    if lines.first().is_some_and(|l| l.trim() == "---")
        && let Some(end) = lines.iter().skip(1).position(|l| l.trim() == "---")
    {
        lines.drain(..=end + 1);
    }
    let lines: Vec<String> = lines.iter().map(|l| l.split("%%").next().unwrap_or("").trim().to_owned()).filter(|l| !l.is_empty()).collect();
    let Some(head) = lines.first() else { return Err(FormatError::Invalid("empty text".into())) };
    if let Some(c) = FLOW.captures(head) {
        let mut f = Flow::new(dir_of(c.get(1).map_or("TD", |d| d.as_str())));
        parse_flowchart(&lines[1..], &mut f);
        drop_subgraph_placeholders(&mut f);
        if f.nodes.is_empty() {
            return Err(FormatError::Invalid("no node found".into()));
        }
        check_flow_size(&f)?;
        return Ok(build_flow(&f, m));
    }
    if head.starts_with("stateDiagram") {
        let mut f = Flow::new(Direction::TopDown);
        parse_state(&lines[1..], &mut f);
        drop_subgraph_placeholders(&mut f);
        if f.nodes.is_empty() {
            return Err(FormatError::Invalid("no state found".into()));
        }
        check_flow_size(&f)?;
        return Ok(build_flow(&f, m));
    }
    if head.starts_with("sequenceDiagram") {
        return build_sequence(&lines[1..], m);
    }
    if head.starts_with("classDiagram") {
        return uml::class_diagram(&lines[1..], m);
    }
    if head.starts_with("erDiagram") {
        return uml::er_diagram(&lines[1..], m);
    }
    let kind = head.split_whitespace().next().unwrap_or("");
    Err(FormatError::Unsupported(format!(
        "unsupported diagram type (\"{kind}\"). Supported: flowchart/graph, stateDiagram, sequenceDiagram, classDiagram, erDiagram"
    )))
}

/// Imports a Mermaid diagram as a one-page document.
///
/// # Errors
///
/// Same as [`to_objects`].
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
    use inkboard_core::Page;
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
            assert!(b.iter().any(|x| x.1.label.text.to_plain() == want), "{want} is missing");
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
        assert!(matches!(to_objects("pie\n \"a\" : 1", &ApproxMeasurer), Err(FormatError::Unsupported(_))));
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

    #[test]
    fn a_semicolon_inside_a_quoted_label_does_not_end_the_statement() {
        let o = objs("graph TD\n E[\"first; second\"] --> A; A --> B");
        let t: Vec<String> = boxes(&o).iter().map(|x| x.1.label.text.to_plain()).collect();
        assert_eq!(t.len(), 3, "{t:?}");
        assert!(t.contains(&"first; second".to_owned()), "{t:?}");
        assert_eq!(conns(&o).len(), 2);
    }

    #[test]
    fn a_link_to_a_subgraph_is_attached_to_its_frame() {
        let o = objs(
            "flowchart TD\n subgraph Front [Views]\n  A[View] --> B[Controller]\n end\n subgraph Back [Plugins]\n  C[Study] --> D[Balance]\n end\n B --> Back\n Front --> C",
        );
        let t: Vec<String> = boxes(&o).iter().map(|x| x.1.label.text.to_plain()).collect();
        // four nodes, two titles: no node named after a subgraph
        assert!(!t.contains(&"Back".to_owned()) && !t.contains(&"Front".to_owned()), "{t:?}");
        let frames: Vec<&Object> = boxes(&o).into_iter().filter(|(x, _)| x.style.dash == Dash::Dashed).map(|(x, _)| x).collect();
        assert_eq!(frames.len(), 2);
        let c = conns(&o);
        assert_eq!(c.len(), 4);
        let bound_to = |end: &End, id: &Id| end.target() == Some(id);
        assert!(c.iter().any(|k| frames.iter().any(|f| bound_to(&k.end, &f.id))), "a link ends on a frame");
        assert!(c.iter().any(|k| frames.iter().any(|f| bound_to(&k.start, &f.id))), "a link starts on a frame");
    }

    fn frames(o: &[Object]) -> Vec<Rect> {
        boxes(o).into_iter().filter(|(x, _)| x.style.dash == Dash::Dashed).map(|(x, _)| x.frame().unwrap().rect()).collect()
    }

    fn center_of(o: &[Object], text: &str) -> Pt {
        let (x, _) = boxes(o).into_iter().find(|(_, b)| b.label.text.to_plain() == text).unwrap_or_else(|| panic!("no node {text}"));
        x.frame().unwrap().center()
    }

    #[test]
    fn frames_of_sibling_subgraphs_do_not_overlap_and_hold_their_nodes() {
        let o = objs(
            "flowchart TD\n subgraph F [Views]\n  A[A] --> B[B]\n  A --> B2[B2]\n end\n subgraph P [Plugins]\n  C[C] --> D[D]\n  C --> D2[D2]\n end\n B --> C\n B2 --> D2\n A --> D",
        );
        let f = frames(&o);
        assert_eq!(f.len(), 2);
        assert!(!f[0].intersects(&f[1]), "{f:?}");
        for (names, frame) in [(["A", "B", "B2"], &f[0]), (["C", "D", "D2"], &f[1])] {
            for n in names {
                assert!(frame.contains(center_of(&o, n)), "{n} is outside its frame");
            }
        }
    }

    #[test]
    fn a_nested_subgraph_stays_inside_its_parent() {
        let o = objs("flowchart LR\n subgraph Outer\n  subgraph Inner\n   A --> B\n  end\n  B --> C\n end\n C --> D");
        let f = frames(&o);
        assert_eq!(f.len(), 2);
        let (outer, inner) = if f[0].w > f[1].w { (f[0], f[1]) } else { (f[1], f[0]) };
        assert!(outer.contains(Pt::new(inner.x, inner.y)) && outer.contains(Pt::new(inner.right(), inner.bottom())), "{outer:?} {inner:?}");
        assert!(!inner.contains(center_of(&o, "C")) && outer.contains(center_of(&o, "C")));
        assert!(!outer.contains(center_of(&o, "D")));
    }

    #[test]
    fn the_direction_of_a_subgraph_applies_when_nothing_links_it_to_the_outside() {
        let o = objs("flowchart TD\n subgraph S\n  direction LR\n  A --> B\n end\n C --> D");
        let (a, b) = (center_of(&o, "A"), center_of(&o, "B"));
        assert!((a.y - b.y).abs() < 1.0 && (a.x - b.x).abs() > 20.0, "A and B side by side: {a:?} {b:?}");
        let (c, d) = (center_of(&o, "C"), center_of(&o, "D"));
        assert!((c.x - d.x).abs() < 1.0 && (c.y - d.y).abs() > 20.0, "the rest keeps the diagram direction: {c:?} {d:?}");
    }

    #[test]
    fn the_direction_of_a_subgraph_is_ignored_when_a_link_crosses_its_frame() {
        let o = objs("flowchart TD\n subgraph S\n  direction LR\n  A --> B\n end\n C --> A");
        let (a, b) = (center_of(&o, "A"), center_of(&o, "B"));
        assert!((a.x - b.x).abs() < 1.0 && (a.y - b.y).abs() > 20.0, "inherits the direction of the diagram: {a:?} {b:?}");
    }

    /// Links whose lines (through their waypoints) cross a node they do not link. A link to a frame, and frames themselves, are not counted.
    fn lines_through_nodes(o: &[Object]) -> usize {
        let frame_ids: Vec<&Id> = boxes(o).into_iter().filter(|(x, _)| x.style.dash == Dash::Dashed).map(|(x, _)| &x.id).collect();
        let nodes: Vec<(&Id, Rect)> = o
            .iter()
            .filter(|x| !frame_ids.contains(&&x.id) && !matches!(&x.kind, Kind::Box(b) if b.geom == BoxGeom::Text))
            .filter_map(|x| x.frame().map(|f| (&x.id, f.rect())))
            .collect();
        let rect_of = |id: &Id| nodes.iter().find(|n| n.0 == id).map(|n| n.1);
        let mut count = 0;
        for c in conns(o) {
            let (Some(a), Some(b)) = (c.start.target().and_then(rect_of), c.end.target().and_then(rect_of)) else { continue };
            let path: Vec<Pt> = std::iter::once(a.center()).chain(c.via.iter().copied()).chain(std::iter::once(b.center())).collect();
            let in_the_way: Vec<Rect> = nodes
                .iter()
                .filter(|n| Some(n.0) != c.start.target() && Some(n.0) != c.end.target())
                .map(|n| Rect::new(n.1.x + 2.0, n.1.y + 2.0, n.1.w - 4.0, n.1.h - 4.0))
                .collect();
            count += usize::from(inkboard_core::route::crosses(&path, &in_the_way));
        }
        count
    }

    #[test]
    fn a_link_that_crosses_a_frame_goes_around_the_nodes_in_its_way() {
        // Model -> Export used to run straight through "Study generator"
        let o = objs(
            "flowchart TD\n subgraph Front [Views]\n  A[View table] --> B[Controller]\n  A --> B2[Model]\n end\n subgraph Back [Plugins]\n  C[Study generator] --> D[Balance]\n  C --> D2[Export]\n end\n B --> Back\n Front --> C\n E[Text] --> A\n B2 --> D2",
        );
        assert_eq!(lines_through_nodes(&o), 0);
        // the detour exists: that link has waypoints
        let model = boxes(&o).into_iter().find(|(_, b)| b.label.text.to_plain() == "Model").unwrap().0.id.clone();
        let link = conns(&o).into_iter().find(|c| c.start.target() == Some(&model)).unwrap();
        assert!(!link.via.is_empty(), "the link goes around by waypoints");
    }

    #[test]
    fn links_never_cross_nodes_on_ordinary_flowcharts_and_state_diagrams() {
        for src in [
            "flowchart TD\n A --> B --> C --> D\n A --> C\n A --> D\n B --> D",
            "flowchart LR\n A --> B & C & D\n B --> E\n C --> E\n D --> E\n A --> E",
            "stateDiagram-v2\n [*] --> A\n A --> B\n B --> C\n A --> C\n C --> [*]",
            FLOW,
        ] {
            assert_eq!(lines_through_nodes(&objs(src)), 0, "{src}");
        }
    }

    #[test]
    fn routing_around_nodes_keeps_the_diagram_valid_on_a_dense_graph() {
        // 30 nodes, about 90 links: whatever the detours, every link is still bound to its two nodes and nothing is lost
        let mut src = String::from("flowchart TD\n");
        let mut seed = 7u64;
        let mut next = |m: usize| {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            usize::try_from((seed >> 33) % u64::try_from(m).unwrap()).unwrap()
        };
        let mut links = 0;
        for i in 0..30 {
            for _ in 0..3 {
                let j = next(30);
                if i != j {
                    src.push_str(&format!(" N{i} --> N{j}\n"));
                    links += 1;
                }
            }
        }
        let o = objs(&src);
        assert_eq!(conns(&o).len(), links);
        assert!(conns(&o).iter().all(|c| c.start.target().is_some() && c.end.target().is_some()));
        assert!(lines_through_nodes(&o) <= 3, "{} of {links} links cross a node", lines_through_nodes(&o));
    }

    #[test]
    fn ordinary_nodes_are_kept_around_a_subgraph() {
        let o = objs("flowchart TD\n subgraph S\n  A --> B\n end\n C --> A");
        assert_eq!(boxes(&o).iter().filter(|x| x.0.style.dash != Dash::Dashed && x.1.geom != BoxGeom::Text).count(), 3);
    }
}
