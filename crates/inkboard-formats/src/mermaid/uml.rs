//! Mermaid `classDiagram` and `erDiagram`: one table per class or entity (name, then attributes, then methods), linked by bound connectors.
//!
//! The ends of the links use the extended arrowheads (`HeadExt`): hollow triangle for inheritance and realization, diamond (filled or hollow) for
//! composition and aggregation, and the crow's foot family for the cardinalities of an entity-relationship link, which are also spelled out in the
//! label (versions that do not know the extended heads draw the closest original ones: triangle, dot, bar, arrow).

use super::clean;
use crate::common::FormatError;
use inkboard_core::layout::{Direction, NodeSpec, layered};
use inkboard_core::num;
use inkboard_core::text::{Measurer, layout_rich};
use inkboard_core::{Align, Arrowhead, Connector, Dash, End, FontFamily, HeadExt, Kind, Label, Object, Pt, Rect, RichText, Route, Style, Table};
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

const SIZE: f64 = 16.0;
const MAX_ENTITIES: usize = 1_000;
const MAX_LINKS: usize = 2_500;

/// A class or an entity: its table is made of `rows`, each row holding one text per column.
struct Entity {
    id: String,
    /// Lines of the first (header) row.
    header: Vec<String>,
    /// Compartments of a class (attributes, methods).
    sections: Vec<Vec<String>>,
    /// Rows of an entity: `[type, name, keys, comment]`.
    attributes: Vec<[String; 4]>,
}

/// What is drawn at an end of a link: an original arrowhead or an extended one.
#[derive(Clone, Copy)]
enum Head {
    Original(Arrowhead),
    Ext(HeadExt),
}

/// A link between two entities, with what to draw at each end.
struct Link {
    from: String,
    to: String,
    label: String,
    start: Head,
    end: Head,
    dashed: bool,
}

#[derive(Default)]
struct Diagram {
    entities: Vec<Entity>,
    index: HashMap<String, usize>,
    links: Vec<Link>,
    dir: Option<Direction>,
}

impl Diagram {
    fn entity(&mut self, name: &str) -> usize {
        if let Some(&i) = self.index.get(name) {
            return i;
        }
        self.entities.push(Entity { id: name.to_owned(), header: vec![generics(name)], sections: vec![Vec::new(), Vec::new()], attributes: Vec::new() });
        self.index.insert(name.to_owned(), self.entities.len() - 1);
        self.entities.len() - 1
    }
}

/// `List~int~` is written `List<int>`.
fn generics(name: &str) -> String {
    let mut parts = name.split('~');
    let mut out = parts.next().unwrap_or("").to_owned();
    for (i, p) in parts.enumerate() {
        out.push(if i % 2 == 0 { '<' } else { '>' });
        out.push_str(p);
    }
    out
}

fn dir_of(s: &str) -> Direction {
    match s.trim().to_ascii_uppercase().as_str() {
        "LR" => Direction::LeftRight,
        "RL" => Direction::RightLeft,
        "BT" => Direction::BottomUp,
        _ => Direction::TopDown,
    }
}

/* ------------------------------------------------------------ class diagram */

const CLASS_NAME: &str = r"[\w.$~`]+";

fn parse_class(lines: &[String]) -> Diagram {
    static BLOCK: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(&format!(r"^class\s+({CLASS_NAME})(?:\s*\[\s*{q}\s*\])?\s*\{{\s*$", q = r#""[^"]*""#)).unwrap());
    static SIMPLE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(&format!(r"^class\s+({CLASS_NAME})(?:\s*\[\s*\x22([^\x22]*)\x22\s*\])?(?:\s*:::\s*\w+)?\s*$")).unwrap());
    static MEMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"^({CLASS_NAME})\s*:\s*(.+)$")).unwrap());
    static STEREOTYPE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"^<<\s*([^>]+?)\s*>>\s*({CLASS_NAME})$")).unwrap());
    static RELATION: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(&format!(
            r#"^({CLASS_NAME})\s*(?:"([^"]*)"\s*)?(<\|--|--\|>|<\|\.\.|\.\.\|>|\*--|--\*|o--|--o|<-->|<\.\.>|-->|<--|\.\.>|<\.\.|--|\.\.)\s*(?:"([^"]*)"\s*)?({CLASS_NAME})\s*(?::\s*(.*))?$"#
        ))
        .unwrap()
    });
    let mut d = Diagram::default();
    let mut open: Option<usize> = None;
    let mut namespaces = 0;
    for s in lines {
        if let Some(i) = open {
            if s == "}" {
                open = None;
            } else if let Some(st) = s.strip_prefix("<<").and_then(|r| r.strip_suffix(">>")) {
                d.entities[i].header.insert(0, format!("\u{ab}{}\u{bb}", st.trim()));
            } else {
                add_member(&mut d.entities[i], s);
            }
        } else if s == "}" {
            namespaces = u32::saturating_sub(namespaces, 1);
        } else if s.starts_with("namespace ") && s.ends_with('{') {
            namespaces += 1;
        } else if let Some(r) = s.strip_prefix("direction ") {
            d.dir = Some(dir_of(r));
        } else if let Some(c) = BLOCK.captures(s) {
            open = Some(d.entity(&c[1]));
        } else if let Some(c) = SIMPLE.captures(s) {
            let i = d.entity(&c[1]);
            if let Some(alias) = c.get(2) {
                d.entities[i].header = vec![clean(alias.as_str())];
            }
        } else if let Some(c) = STEREOTYPE.captures(s) {
            let i = d.entity(&c[2]);
            d.entities[i].header.insert(0, format!("\u{ab}{}\u{bb}", &c[1]));
        } else if let Some(c) = RELATION.captures(s) {
            let (a, b) = (d.entity(&c[1]), d.entity(&c[5]));
            let (from, to) = (d.entities[a].id.clone(), d.entities[b].id.clone());
            d.links.push(class_link(
                from,
                to,
                &c[3],
                c.get(2).map_or("", |m| m.as_str()),
                c.get(4).map_or("", |m| m.as_str()),
                c.get(6).map_or("", |m| m.as_str()),
            ));
        } else if let Some(c) = MEMBER.captures(s) {
            let i = d.entity(&c[1]);
            add_member(&mut d.entities[i], &c[2]);
        }
    }
    d
}

/// Adds `text` to the attributes of the class, or to its methods when it has parentheses.
fn add_member(e: &mut Entity, text: &str) {
    let text = generics(clean(text).trim());
    if text.is_empty() {
        return;
    }
    e.sections[usize::from(text.contains('('))].push(text);
}

fn class_link(from: String, to: String, relation: &str, from_card: &str, to_card: &str, label: &str) -> Link {
    let dashed = relation.contains("..");
    let (mut start, mut end) = (Head::Original(Arrowhead::None), Head::Original(Arrowhead::None));
    let head = |marker: &str| match marker {
        "<|" | "|>" => Head::Ext(HeadExt::TriangleOpen),
        "*" => Head::Ext(HeadExt::Diamond),
        "o" => Head::Ext(HeadExt::DiamondOpen),
        _ => Head::Original(Arrowhead::Arrow),
    };
    let (left, right) = relation.split_once("--").or_else(|| relation.split_once("..")).unwrap_or(("", ""));
    let left = left.trim_end_matches(['-', '.']);
    let right = right.trim_start_matches(['-', '.']);
    if !left.is_empty() {
        start = head(left);
    }
    if !right.is_empty() {
        end = head(right);
    }
    let cardinalities = match (from_card.is_empty(), to_card.is_empty()) {
        (true, true) => String::new(),
        _ => format!("{from_card} \u{2014} {to_card}"),
    };
    let label = [clean(label), cardinalities].iter().filter(|p| !p.is_empty()).cloned().collect::<Vec<_>>().join("\n");
    Link { from, to, label, start, end, dashed }
}

/* ------------------------------------------------------------ entity-relationship diagram */

fn parse_er(lines: &[String]) -> Diagram {
    static BLOCK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^([\w-]+)(?:\s*\[\s*"?([^"\]]*)"?\s*\])?\s*\{\s*$"#).unwrap());
    static RELATION: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"^"?([\w-]+)"?\s+(\|\||\|o|\}o|\}\|)(--|\.\.)(\|\||o\||o\{|\|\{)\s+"?([\w-]+)"?\s*(?::\s*(.*))?$"#).unwrap());
    static ATTRIBUTE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"^(\S+)\s+(\S+)(?:\s+((?:PK|FK|UK)(?:\s*,\s*(?:PK|FK|UK))*))?(?:\s+"([^"]*)")?\s*$"#).unwrap());
    let mut d = Diagram::default();
    let mut open: Option<usize> = None;
    for s in lines {
        if let Some(i) = open {
            if s == "}" {
                open = None;
            } else if let Some(c) = ATTRIBUTE.captures(s) {
                let g = |n: usize| c.get(n).map_or(String::new(), |m| m.as_str().replace(' ', ""));
                d.entities[i].attributes.push([generics(&g(1)), g(2), g(3).replace(',', ", "), c.get(4).map_or(String::new(), |m| m.as_str().to_owned())]);
            }
        } else if let Some(r) = s.strip_prefix("direction ") {
            d.dir = Some(dir_of(r));
        } else if let Some(c) = BLOCK.captures(s) {
            let i = d.entity(&c[1]);
            if let Some(alias) = c.get(2).filter(|a| !a.as_str().trim().is_empty()) {
                d.entities[i].header = vec![clean(alias.as_str())];
            }
            open = Some(i);
        } else if let Some(c) = RELATION.captures(s) {
            let (a, b) = (d.entity(&c[1]), d.entity(&c[5]));
            let (from, to) = (d.entities[a].id.clone(), d.entities[b].id.clone());
            let (start, from_card) = er_end(&c[2]);
            let (end, to_card) = er_end(&c[4]);
            let label = [clean(c.get(6).map_or("", |m| m.as_str())), format!("{from_card} \u{2014} {to_card}")]
                .iter()
                .filter(|p| !p.is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join("\n");
            d.links.push(Link { from, to, label, start, end, dashed: &c[3] == ".." });
        } else if s.chars().all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '-') {
            d.entity(s);
        }
    }
    d
}

/// The arrowhead and the cardinality of an end of an entity-relationship link (`||`, `|o`, `o|`, `}o`, `o{`, `}|`, `|{`).
fn er_end(token: &str) -> (Head, &'static str) {
    match token {
        "||" => (Head::Ext(HeadExt::OneOnly), "1"),
        "|o" | "o|" => (Head::Ext(HeadExt::ZeroOrOne), "0..1"),
        "}o" | "o{" => (Head::Ext(HeadExt::CrowFootZero), "0..*"),
        _ => (Head::Ext(HeadExt::CrowFootOne), "1..*"),
    }
}

/* ------------------------------------------------------------ objects */

fn text_size(text: &str, m: &dyn Measurer) -> (f64, f64) {
    let lay = layout_rich(&RichText::plain(text), None, m, FontFamily::Sans, SIZE);
    (lay.width, lay.height)
}

/// The table of an entity, with its columns as wide as their longest text and its rows as high as their lines.
fn table_of(entity: &Entity, measurer: &dyn Measurer) -> Table {
    let columns = if entity.attributes.is_empty() {
        1
    } else if entity.attributes.iter().any(|attr| !attr[3].is_empty()) {
        4
    } else {
        3
    };
    let head = entity.header.join("\n");
    let mut rows: Vec<Vec<String>> = vec![std::iter::once(head.clone()).chain(std::iter::repeat_n(String::new(), columns - 1)).collect()];
    if entity.attributes.is_empty() {
        rows.extend(entity.sections.iter().filter(|section| !section.is_empty()).map(|section| vec![section.join("\n")]));
    } else {
        rows.extend(entity.attributes.iter().map(|attr| attr[..columns].to_vec()));
    }
    let mut table = Table::new(Pt::ZERO, rows.len(), columns);
    table.size = SIZE;
    table.col_w = vec![0.0; columns];
    for (r, row) in rows.iter().enumerate() {
        let mut tallest = 0.0f64;
        for (c, text) in row.iter().enumerate() {
            table.cells[r * columns + c] = RichText::plain(text.as_str());
            let (width, height) = text_size(text, measurer);
            tallest = tallest.max(height);
            if r > 0 {
                table.col_w[c] = table.col_w[c].max(width + 24.0);
            }
        }
        table.row_h[r] = (tallest + 12.0).max(32.0);
    }
    // the name sits in the first column and must fit there
    table.col_w[0] = table.col_w[0].max(text_size(&head, measurer).0 + 24.0);
    for width in &mut table.col_w {
        *width = width.max(60.0);
    }
    table
}

/// Waypoints of link `index`: the relays of the layout, a loop for a link of an entity to itself, or a curve when several links join the same pair.
fn waypoints(
    index: usize,
    ends: (usize, usize),
    lay: &inkboard_core::layout::Layout,
    rects: &[Rect],
    seen: &mut HashMap<(usize, usize), usize>,
    counts: &HashMap<(usize, usize), usize>,
) -> (Vec<Pt>, Route) {
    let (from, to) = ends;
    if from == to {
        let rect = rects[from];
        return (vec![Pt::new(rect.right() + 34.0, rect.y + rect.h * 0.25), Pt::new(rect.right() + 34.0, rect.y + rect.h * 0.75)], Route::Straight);
    }
    let route = &lay.routes[index];
    if route.len() > 2 {
        return (route[1..route.len() - 1].to_vec(), Route::Straight);
    }
    let pair = (from.min(to), from.max(to));
    let nth = seen.entry(pair).or_default();
    let mut out = (Vec::new(), Route::Straight);
    if counts[&pair] > 1 {
        let (first, second) = (rects[pair.0].center(), rects[pair.1].center());
        let delta = second - first;
        let len = delta.len().max(1.0);
        let perp = Pt::new(-delta.y / len, delta.x / len);
        let offset = (num::f64_of(*nth) - (num::f64_of(counts[&pair]) - 1.0) / 2.0) * 44.0;
        out = (vec![first.lerp(second, 0.5) + perp * offset], Route::Curve);
    }
    *nth += 1;
    out
}

fn build(diagram: &Diagram, measurer: &dyn Measurer) -> Result<Vec<Object>, FormatError> {
    if diagram.entities.len() > MAX_ENTITIES || diagram.links.len() > MAX_LINKS {
        return Err(FormatError::Unsupported(format!(
            "diagram too large ({} items, {} links; at most {MAX_ENTITIES} items and {MAX_LINKS} links)",
            diagram.entities.len(),
            diagram.links.len()
        )));
    }
    let mut tables: Vec<Table> = diagram.entities.iter().map(|entity| table_of(entity, measurer)).collect();
    let specs: Vec<NodeSpec> = tables.iter().map(|table| NodeSpec { w: table.rect().w, h: table.rect().h }).collect();
    let edges: Vec<(usize, usize)> = diagram.links.iter().map(|link| (diagram.index[&link.from], diagram.index[&link.to])).collect();
    let lay = layered(&specs, &edges, diagram.dir.unwrap_or(Direction::TopDown));
    let rects: Vec<Rect> =
        lay.centers.iter().zip(&specs).map(|(center, spec)| Rect::new(center.x - spec.w / 2.0, center.y - spec.h / 2.0, spec.w, spec.h)).collect();
    let mut out: Vec<Object> = Vec::new();
    let mut ids = Vec::new();
    for (table, rect) in tables.iter_mut().zip(&rects) {
        table.origin = Pt::new(rect.x, rect.y);
        let object = Object::new(Kind::Table(table.clone()), Style { width: 2.0, fill: None, ..Style::default() });
        ids.push(object.id.clone());
        out.push(object);
    }
    let mut counts: HashMap<(usize, usize), usize> = HashMap::new();
    for &(from, to) in edges.iter().filter(|(from, to)| from != to) {
        *counts.entry((from.min(to), from.max(to))).or_default() += 1;
    }
    let mut seen: HashMap<(usize, usize), usize> = HashMap::new();
    for (index, link) in diagram.links.iter().enumerate() {
        let (from, to) = edges[index];
        let mut conn = Connector::new(End::bound(ids[from].clone()), End::bound(ids[to].clone()));
        match link.start {
            Head::Original(head) => conn.set_head_start(head),
            Head::Ext(ext) => conn.set_ext_start(ext),
        }
        match link.end {
            Head::Original(head) => conn.set_head_end(head),
            Head::Ext(ext) => conn.set_ext_end(ext),
        }
        if !link.label.is_empty() {
            conn.label = Some(Label::new(&link.label, 14.0, Align::Center));
        }
        (conn.via, conn.route) = waypoints(index, (from, to), &lay, &rects, &mut seen, &counts);
        let style = Style { width: 2.5, dash: if link.dashed { Dash::Dashed } else { Dash::Solid }, ..Style::default() };
        out.push(Object::new(Kind::Connector(conn), style));
    }
    Ok(out)
}

/// Converts the lines of a `classDiagram` (without its first line) to objects.
pub(super) fn class_diagram(lines: &[String], m: &dyn Measurer) -> Result<Vec<Object>, FormatError> {
    let d = parse_class(lines);
    if d.entities.is_empty() {
        return Err(FormatError::Invalid("no class found".into()));
    }
    build(&d, m)
}

/// Converts the lines of an `erDiagram` (without its first line) to objects.
pub(super) fn er_diagram(lines: &[String], m: &dyn Measurer) -> Result<Vec<Object>, FormatError> {
    let d = parse_er(lines);
    if d.entities.is_empty() {
        return Err(FormatError::Invalid("no entity found".into()));
    }
    build(&d, m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mermaid::to_objects;
    use inkboard_core::text::ApproxMeasurer;

    fn objs(src: &str) -> Vec<Object> {
        to_objects(src, &ApproxMeasurer).unwrap()
    }
    fn tables(o: &[Object]) -> Vec<(&Object, &Table)> {
        o.iter().filter_map(|x| if let Kind::Table(t) = &x.kind { Some((x, t)) } else { None }).collect()
    }
    fn conns(o: &[Object]) -> Vec<&Connector> {
        o.iter().filter_map(|x| if let Kind::Connector(c) = &x.kind { Some(c) } else { None }).collect()
    }
    fn cell(t: &Table, r: usize, c: usize) -> String {
        t.cell(r, c).map(RichText::to_plain).unwrap_or_default()
    }
    fn table_named<'a>(o: &'a [Object], name: &str) -> (&'a Object, &'a Table) {
        tables(o).into_iter().find(|(_, t)| cell(t, 0, 0).ends_with(name)).unwrap_or_else(|| panic!("no table {name}"))
    }

    const CLASSES: &str = "classDiagram\n direction LR\n class Animal {\n  <<abstract>>\n  +String name\n  +int age\n  +eat() void\n }\n Animal <|-- Dog\n Dog : +bark() void\n Dog *-- Tail : has\n Owner \"1\" --> \"*\" Dog : owns\n Dog ..> Food\n Animal ..|> Living\n List~int~ o-- Item";

    #[test]
    fn a_class_is_a_table_with_its_name_attributes_and_methods() {
        let o = objs(CLASSES);
        let (_, animal) = table_named(&o, "Animal");
        assert_eq!(animal.rows(), 3);
        assert_eq!(cell(animal, 0, 0), "\u{ab}abstract\u{bb}\nAnimal");
        assert_eq!(cell(animal, 1, 0), "+String name\n+int age");
        assert_eq!(cell(animal, 2, 0), "+eat() void");
        assert!(animal.header);
        let (_, dog) = table_named(&o, "Dog");
        assert_eq!(dog.rows(), 2, "no attributes: name and methods");
        assert_eq!(cell(dog, 1, 0), "+bark() void");
        let (_, list) = table_named(&o, "List<int>");
        assert_eq!(list.rows(), 1);
    }

    #[test]
    fn class_links_are_bound_connectors_with_the_right_ends() {
        let o = objs(CLASSES);
        let id = |name: &str| table_named(&o, name).0.id.clone();
        let find = |from: &str, to: &str| {
            *conns(&o).iter().find(|c| c.start.target() == Some(&id(from)) && c.end.target() == Some(&id(to))).unwrap_or_else(|| panic!("{from} -> {to}"))
        };
        let inherit = find("Animal", "Dog");
        assert_eq!((inherit.head_start_ext, inherit.head_end), (Some(HeadExt::TriangleOpen), Arrowhead::None), "the parent has the hollow triangle");
        assert_eq!(inherit.head_start, Arrowhead::Triangle, "older versions draw the closest original head");
        let comp = find("Dog", "Tail");
        assert_eq!((comp.head_start_ext, comp.head_start), (Some(HeadExt::Diamond), Arrowhead::Dot));
        assert_eq!(comp.label.as_ref().unwrap().text.to_plain(), "has");
        let assoc = find("Owner", "Dog");
        assert_eq!(assoc.head_end, Arrowhead::Arrow);
        assert_eq!(assoc.label.as_ref().unwrap().text.to_plain(), "owns\n1 \u{2014} *");
        let aggregation = find("List<int>", "Item");
        assert_eq!(aggregation.head_start_ext, Some(HeadExt::DiamondOpen));
        assert!(aggregation.label.is_none());
        let styles: Vec<Dash> = o.iter().filter(|x| matches!(x.kind, Kind::Connector(_))).map(|x| x.style.dash).collect();
        assert_eq!(styles.iter().filter(|d| **d == Dash::Dashed).count(), 2, "dependency and realization are dashed");
        assert_eq!(conns(&o).len(), 6);
    }

    #[test]
    fn classes_do_not_overlap_and_the_direction_is_honoured() {
        let o = objs(CLASSES);
        let rects: Vec<Rect> = tables(&o).iter().map(|(_, t)| t.rect()).collect();
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                assert!(!a.intersects(b), "{a:?} {b:?}");
            }
        }
        let (a, d) = (table_named(&o, "Animal").1.rect(), table_named(&o, "Dog").1.rect());
        assert!(d.x > a.right(), "left to right: Dog is right of Animal");
    }

    #[test]
    fn a_class_diagram_without_class_is_an_error_and_notes_are_skipped() {
        assert!(to_objects("classDiagram\n note \"hello\"", &ApproxMeasurer).is_err());
        let o = objs("classDiagram\n class A\n note for A \"text\"\n namespace N {\n class B\n }\n A --> B");
        assert_eq!(tables(&o).len(), 2);
    }

    const ER: &str = "erDiagram\n CUSTOMER ||--o{ ORDER : places\n ORDER ||--|{ LINE_ITEM : contains\n PRODUCT |o..o| LINE_ITEM : \"is in\"\n CUSTOMER {\n  string name PK\n  string email UK \"login\"\n  int age\n }\n ORDER {\n  int id PK, FK\n }";

    #[test]
    fn an_entity_is_a_table_with_type_name_and_key_columns() {
        let o = objs(ER);
        let (_, c) = table_named(&o, "CUSTOMER");
        assert_eq!((c.rows(), c.cols()), (4, 4), "a comment column appears when one attribute has a comment");
        assert_eq!(cell(c, 1, 0), "string");
        assert_eq!(cell(c, 1, 1), "name");
        assert_eq!(cell(c, 1, 2), "PK");
        assert_eq!(cell(c, 2, 3), "login");
        let (_, order) = table_named(&o, "ORDER");
        assert_eq!((order.rows(), order.cols()), (2, 3));
        assert_eq!(cell(order, 1, 2), "PK, FK");
        let (_, item) = table_named(&o, "LINE_ITEM");
        assert_eq!((item.rows(), item.cols()), (1, 1), "an entity without attribute is its name");
    }

    #[test]
    fn entity_links_carry_the_cardinalities() {
        let o = objs(ER);
        let id = |name: &str| table_named(&o, name).0.id.clone();
        let find = |from: &str, to: &str| {
            *conns(&o).iter().find(|c| c.start.target() == Some(&id(from)) && c.end.target() == Some(&id(to))).unwrap_or_else(|| panic!("{from} -> {to}"))
        };
        let places = find("CUSTOMER", "ORDER");
        assert_eq!((places.head_start_ext, places.head_end_ext), (Some(HeadExt::OneOnly), Some(HeadExt::CrowFootZero)));
        assert_eq!((places.head_start, places.head_end), (Arrowhead::Bar, Arrowhead::Arrow), "what older versions draw");
        assert_eq!(places.label.as_ref().unwrap().text.to_plain(), "places\n1 \u{2014} 0..*");
        assert_eq!(find("ORDER", "LINE_ITEM").label.as_ref().unwrap().text.to_plain(), "contains\n1 \u{2014} 1..*");
        let optional = find("PRODUCT", "LINE_ITEM");
        assert_eq!((optional.head_start_ext, optional.head_end_ext), (Some(HeadExt::ZeroOrOne), Some(HeadExt::ZeroOrOne)));
        assert_eq!(optional.label.as_ref().unwrap().text.to_plain(), "is in\n0..1 \u{2014} 0..1");
        let styles: Vec<Dash> = o.iter().filter(|x| matches!(x.kind, Kind::Connector(_))).map(|x| x.style.dash).collect();
        assert_eq!(styles.iter().filter(|d| **d == Dash::Dashed).count(), 1, "a non-identifying relationship is dashed");
    }

    #[test]
    fn unsupported_and_empty_diagrams_are_explicit_errors() {
        assert!(to_objects("erDiagram", &ApproxMeasurer).is_err());
        assert!(matches!(to_objects("gantt\n title x", &ApproxMeasurer), Err(FormatError::Unsupported(m)) if m.contains("classDiagram, erDiagram")));
    }

    #[test]
    fn parsing_is_total_on_hostile_text() {
        for src in [
            "classDiagram\n class {",
            "classDiagram\n } } }\n A <|-- ",
            "erDiagram\n { } {\n A ||--o{",
            "classDiagram\n \"\" --> \"\"",
            "erDiagram\n A {\n  x\n }",
        ] {
            let _ = to_objects(src, &ApproxMeasurer);
        }
    }
}
