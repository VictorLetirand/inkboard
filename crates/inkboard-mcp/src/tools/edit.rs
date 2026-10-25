//! Tools of the MCP v2 plan (lot 3: reading styles, editing); the module is registered in `tools.rs`.
//!
//! The engine does not enforce object locks: `translate`, `update`, `duplicate` and `remove` act on locked objects, only
//! `align` and `distribute` skip them (and the objects of a locked layer). The tools here keep that behavior and report it.

use super::{Res, find, kind_name, obj, os, page_prop, path_prop, richtext, s, summary};
use inkboard_core::editor::{AlignMode, Axis, Order};
use inkboard_core::num;
use inkboard_core::{Align, ApproxMeasurer, Arrowhead, Color, Dash, Editor, FontFamily, Id, Kind, Label, Measurer, Object, Page, Pt, Rect, RichText, Route};
use serde_json::{Value, json};
use std::collections::HashSet;

/// Tools exposed by this module (`delete_object` is an existing tool whose `ids` form is handled here).
pub(super) const NAMES: &[&str] = &["find_objects", "move_objects", "arrange_objects", "manage_pages", "duplicate_objects"];
/// Those that do not modify the board.
pub(super) const READ_ONLY: &[&str] = &["find_objects"];

/// Most objects one call can target.
const MAX_IDS: usize = 2000;
/// Coordinates and offsets given to the tools stay within this distance of the origin.
const MAX_COORD: f64 = 1_000_000.0;

const DASHES: [(&str, Dash); 3] = [("solid", Dash::Solid), ("dashed", Dash::Dashed), ("dotted", Dash::Dotted)];
const FAMILIES: [(&str, FontFamily); 4] = [("sans", FontFamily::Sans), ("hand", FontFamily::Hand), ("serif", FontFamily::Serif), ("mono", FontFamily::Mono)];
const ALIGNS: [(&str, Align); 3] = [("left", Align::Left), ("center", Align::Center), ("right", Align::Right)];
const ROUTES: [(&str, Route); 3] = [("straight", Route::Straight), ("elbow", Route::Elbow), ("curve", Route::Curve)];
const ARROWS: [(&str, (Arrowhead, Arrowhead)); 4] = [
    ("end", (Arrowhead::None, Arrowhead::Arrow)),
    ("start", (Arrowhead::Arrow, Arrowhead::None)),
    ("both", (Arrowhead::Arrow, Arrowhead::Arrow)),
    ("none", (Arrowhead::None, Arrowhead::None)),
];
const TYPES: [&str; 11] = ["rect", "ellipse", "diamond", "triangle", "sticky", "text", "connector", "table", "image", "formula", "pen"];
const ACTIONS: [&str; 16] = [
    "align_left",
    "align_center_x",
    "align_right",
    "align_top",
    "align_center_y",
    "align_bottom",
    "distribute_horizontal",
    "distribute_vertical",
    "bring_to_front",
    "bring_forward",
    "send_backward",
    "send_to_back",
    "group",
    "ungroup",
    "lock",
    "unlock",
];

fn name_of<T: PartialEq + Copy>(table: &[(&'static str, T)], v: T) -> Option<&'static str> {
    table.iter().find(|t| t.1 == v).map(|t| t.0)
}

fn names<T>(table: &[(&str, T)]) -> String {
    table.iter().map(|t| t.0).collect::<Vec<_>>().join(", ")
}

/* ------------------------------------------------------------ arguments */

/// Optional string parameter; a value of another type is an error (unlike `os`, which ignores it).
fn opt_str<'a>(a: &'a Value, k: &str) -> Res<Option<&'a str>> {
    match a.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(t)) => Ok(Some(t)),
        Some(_) => Err(format!("\"{k}\" must be a string")),
    }
}

fn bool_arg(a: &Value, k: &str) -> Res<Option<bool>> {
    match a.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(format!("\"{k}\" must be true or false")),
    }
}

fn num_arg(a: &Value, k: &str, min: f64, max: f64) -> Res<Option<f64>> {
    match a.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let n = v.as_f64().ok_or_else(|| format!("\"{k}\" must be a number between {min} and {max}"))?;
            if (min..=max).contains(&n) { Ok(Some(n)) } else { Err(format!("\"{k}\" is out of range: {n} (allowed: {min} to {max})")) }
        }
    }
}

fn enum_arg<T: Copy>(a: &Value, k: &str, table: &[(&str, T)]) -> Res<Option<T>> {
    let Some(v) = a.get(k).filter(|v| !v.is_null()) else { return Ok(None) };
    let name = v.as_str().ok_or_else(|| format!("\"{k}\" must be a string (one of: {})", names(table)))?;
    table.iter().find(|t| t.0 == name).map(|t| Some(t.1)).ok_or_else(|| format!("unknown {k} \"{name}\" (one of: {})", names(table)))
}

/// Integer in `min..=max` (`default` when absent).
fn int_arg(a: &Value, k: &str, min: u64, max: u64, default: u64) -> Res<u64> {
    match a.get(k) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => match v.as_u64() {
            Some(n) if (min..=max).contains(&n) => Ok(n),
            _ => Err(format!("\"{k}\" must be an integer between {min} and {max} (got {v})")),
        },
    }
}

/// List of existing object ids of the current page (1 to `MAX_IDS`, duplicates dropped, order kept).
fn ids_arg(ed: &Editor, a: &Value, k: &str) -> Res<Vec<Id>> {
    let list = a.get(k).and_then(Value::as_array).ok_or_else(|| format!("missing \"{k}\" parameter (a list of 1 to {MAX_IDS} object ids)"))?;
    if list.is_empty() || list.len() > MAX_IDS {
        return Err(format!("\"{k}\" must contain 1 to {MAX_IDS} ids (got {})", list.len()));
    }
    let known: HashSet<&str> = ed.page().objects.iter().map(|o| o.id.0.as_str()).collect();
    let (mut ids, mut seen, mut unknown) = (Vec::new(), HashSet::new(), Vec::new());
    for v in list {
        let id = v.as_str().ok_or_else(|| format!("\"{k}\" must contain only id strings"))?;
        if !known.contains(id) {
            unknown.push(id);
        } else if seen.insert(id) {
            ids.push(Id(id.to_owned()));
        }
    }
    if !unknown.is_empty() {
        let more = unknown.len().saturating_sub(10);
        let shown = unknown.into_iter().take(10).collect::<Vec<_>>().join(", ");
        return Err(format!("object(s) not found on this page: {shown}{}", if more > 0 { format!(" … and {more} more") } else { String::new() }));
    }
    Ok(ids)
}

fn r3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

fn degrees(rad: f64) -> f64 {
    (rad.to_degrees() * 100.0).round() / 100.0
}

/// Box used to move objects by `x`/`y`: the frame for shapes, images and tables (same as `update_object`), the drawn bounds otherwise.
fn frame_box(page: &Page, o: &Object) -> Rect {
    o.frame().map_or_else(|| page.bounds(o), |f| f.aabb())
}

/* --------------------------------------------------------------- reading */

/// `describe_board` object with `detail: "full"`: the brief summary plus style, font, stacking order, group, lock, rotation, arrow.
pub(super) fn full(z: usize, o: &Object) -> Value {
    let mut v = summary(o);
    let map = v.as_object_mut().expect("object");
    let st = &o.style;
    map.insert(
        "style".into(),
        json!({"stroke": st.stroke.to_hex(), "fill": st.fill.map(Color::to_hex), "width": st.width, "dash": name_of(&DASHES, st.dash),
            "opacity": r3(f64::from(st.opacity)), "roundness": r3(f64::from(st.roundness))}),
    );
    let font = |l: &Label| json!({"family": name_of(&FAMILIES, l.family), "size": l.size, "align": name_of(&ALIGNS, l.align)});
    match &o.kind {
        Kind::Box(b) => {
            map.insert("font".into(), font(&b.label));
            if b.frame.angle != 0.0 {
                map.insert("rotation".into(), json!(degrees(b.frame.angle)));
            }
        }
        Kind::Image(i) if i.frame.angle != 0.0 => {
            map.insert("rotation".into(), json!(degrees(i.frame.angle)));
        }
        Kind::Table(t) => {
            map.insert("font".into(), json!({"family": name_of(&FAMILIES, t.family), "size": t.size}));
            let rows: Vec<Vec<String>> = t.cells.chunks(t.cols().max(1)).map(|r| r.iter().map(RichText::to_plain).collect()).collect();
            map.insert("cells".into(), json!(rows));
        }
        Kind::Connector(c) => {
            if let Some(l) = &c.label {
                map.insert("font".into(), font(l));
            }
            map.insert("route".into(), json!(name_of(&ROUTES, c.route)));
            let has = |h: Arrowhead| h != Arrowhead::None;
            let arrow = match (has(c.head_start), has(c.head_end)) {
                (false, true) => "end",
                (true, false) => "start",
                (true, true) => "both",
                (false, false) => "none",
            };
            map.insert("arrow".into(), json!(arrow));
            let head = |h: Arrowhead| format!("{h:?}").to_lowercase();
            if ![Arrowhead::None, Arrowhead::Arrow].contains(&c.head_start) || ![Arrowhead::None, Arrowhead::Arrow].contains(&c.head_end) {
                map.insert("heads".into(), json!({"start": head(c.head_start), "end": head(c.head_end)}));
            }
            map.insert("stroke".into(), json!(st.stroke.to_hex()));
        }
        _ => {}
    }
    map.insert("z".into(), json!(z));
    if let Some(g) = &o.group {
        map.insert("group".into(), json!(g.0));
    }
    if o.locked {
        map.insert("locked".into(), json!(true));
    }
    v
}

/// Text an object can be searched by: shape text, connector label, table cells (one per line), formula source.
fn searchable(o: &Object) -> String {
    match &o.kind {
        Kind::Box(b) => b.label.text.to_plain(),
        Kind::Connector(c) => c.label.as_ref().map(|l| l.text.to_plain()).unwrap_or_default(),
        Kind::Table(t) => t.cells.iter().map(RichText::to_plain).collect::<Vec<_>>().join("\n"),
        Kind::Image(i) => i.latex.clone().unwrap_or_default(),
        Kind::Pen(_) => String::new(),
    }
}

fn find_objects(ed: &mut Editor, args: &Value) -> Res<Value> {
    let text = opt_str(args, "text")?;
    let pattern = opt_str(args, "regex")?;
    if text.is_some() && pattern.is_some() {
        return Err("give either \"text\" or \"regex\", not both".into());
    }
    if text == Some("") {
        return Err("\"text\" must not be empty".into());
    }
    let re = match pattern {
        None => None,
        Some(p) if p.is_empty() || p.len() > 500 => return Err("\"regex\" must be 1 to 500 characters".into()),
        Some(p) => Some(regex::RegexBuilder::new(p).size_limit(1 << 20).dfa_size_limit(1 << 22).build().map_err(|e| format!("invalid regex: {e}"))?),
    };
    let kind = opt_str(args, "type")?;
    if let Some(k) = kind
        && !TYPES.contains(&k)
    {
        return Err(format!("unknown type \"{k}\" (one of: {})", TYPES.join(", ")));
    }
    let region = match args.get("in_region").filter(|v| !v.is_null()) {
        None => None,
        Some(spec) => {
            let field = |k: &str| spec.get(k).and_then(Value::as_f64);
            match (field("x"), field("y"), field("w"), field("h")) {
                (Some(x), Some(y), Some(w), Some(h)) if w >= 0.0 && h >= 0.0 => Some(Rect::new(x, y, w, h)),
                _ => return Err("\"in_region\" must be {\"x\", \"y\", \"w\", \"h\"}: four numbers, w and h not negative".into()),
            }
        }
    };
    let limit = usize::try_from(int_arg(args, "limit", 1, 500, 50)?).map_err(|e| e.to_string())?;
    let needle = text.map(str::to_lowercase);
    let page = ed.page();
    let (mut count, mut found) = (0usize, Vec::new());
    for o in &page.objects {
        if kind.is_some_and(|k| k != kind_name(o)) || region.is_some_and(|r| !r.intersects(&page.bounds(o))) {
            continue;
        }
        if needle.is_some() || re.is_some() {
            let hay = searchable(o);
            let hit = needle.as_ref().is_some_and(|n| hay.to_lowercase().contains(n)) || re.as_ref().is_some_and(|r| r.is_match(&hay));
            if !hit {
                continue;
            }
        }
        count += 1;
        if found.len() < limit {
            found.push(summary(o));
        }
    }
    Ok(json!({"count": count, "truncated": count > found.len(), "objects": found}))
}

/* --------------------------------------------------------- update_object */

/// Cell of a table to rewrite.
struct Cell {
    row: usize,
    col: usize,
    text: RichText,
}

/// Parameters of `update_object` added by lot 3, all validated by [`Patch::parse`] before anything is applied.
pub(super) struct Patch {
    width: Option<f64>,
    dash: Option<Dash>,
    opacity: Option<f32>,
    roundness: Option<f32>,
    size: Option<f64>,
    family: Option<FontFamily>,
    align: Option<Align>,
    locked: Option<bool>,
    /// Radians.
    angle: Option<f64>,
    route: Option<Route>,
    arrow: Option<(Arrowhead, Arrowhead)>,
    from: Option<Id>,
    to: Option<Id>,
    cell: Option<Cell>,
    /// Markdown warnings of the cell text.
    pub(super) warnings: Vec<String>,
}

impl Patch {
    pub(super) fn parse(ed: &Editor, a: &Value, o: &Object) -> Res<Patch> {
        let attachable = |k: &str| -> Res<Option<Id>> {
            let Some(id) = opt_str(a, k)? else { return Ok(None) };
            let t = find(ed, id)?;
            if t.is_attachable() { Ok(Some(t.id.clone())) } else { Err(format!("object {id} cannot be connected (shapes, tables and images only)")) }
        };
        let mut p = Patch {
            width: num_arg(a, "stroke_width", 0.0, 100.0)?,
            dash: enum_arg(a, "dash", &DASHES)?,
            opacity: num_arg(a, "opacity", 0.0, 1.0)?.map(num::f32),
            roundness: num_arg(a, "roundness", 0.0, 1.0)?.map(num::f32),
            size: num_arg(a, "font_size", 4.0, 400.0)?,
            family: enum_arg(a, "font_family", &FAMILIES)?,
            align: enum_arg(a, "align", &ALIGNS)?,
            locked: bool_arg(a, "locked")?,
            angle: num_arg(a, "rotation", -360.0, 360.0)?.map(f64::to_radians),
            route: enum_arg(a, "route", &ROUTES)?,
            arrow: enum_arg(a, "arrow", &ARROWS)?,
            from: attachable("from")?,
            to: attachable("to")?,
            cell: None,
            warnings: Vec::new(),
        };
        if let Some(v) = a.get("cell").filter(|v| !v.is_null()) {
            let Kind::Table(t) = &o.kind else { return Err(format!("\"cell\" only applies to a table (this object is a {})", kind_name(o))) };
            let rc = v.as_array().filter(|l| l.len() == 2).and_then(|l| Some((l[0].as_u64()?, l[1].as_u64()?)));
            let Some((row, col)) = rc else { return Err("\"cell\" must be [row, column]: two integers from 0".into()) };
            let (row, col) = (usize::try_from(row).unwrap_or(usize::MAX), usize::try_from(col).unwrap_or(usize::MAX));
            if row >= t.rows() || col >= t.cols() {
                return Err(format!("cell [{row}, {col}] is outside the table ({} rows, {} columns; indices start at 0)", t.rows(), t.cols()));
            }
            let text = os(a, "text").ok_or("\"cell\" needs the new cell \"text\"")?;
            let mut cell_warnings = Vec::new();
            let rt = richtext::rich(text, richtext::markdown_flag(a)?, &mut cell_warnings);
            p.warnings = cell_warnings.into_iter().map(|w| format!("cell [{row}, {col}]: {w}")).collect();
            p.cell = Some(Cell { row, col, text: rt });
        }
        Ok(p)
    }

    /// True when `text` is the text of a table cell rather than of the object.
    pub(super) fn has_cell(&self) -> bool {
        self.cell.is_some()
    }

    fn has_font(&self) -> bool {
        self.size.is_some() || self.family.is_some() || self.align.is_some()
    }

    fn font(&self, l: &mut Label) {
        if let Some(s) = self.size {
            l.size = s;
        }
        if let Some(f) = self.family {
            l.family = f;
        }
        if let Some(a) = self.align {
            l.align = a;
        }
    }

    /// Applies the validated changes; parameters that make no sense for this kind of object are listed in `warn`.
    pub(super) fn apply(&self, o: &mut Object, warn: &mut Vec<String>) {
        if let Some(w) = self.width {
            o.style.width = w;
        }
        if let Some(d) = self.dash {
            o.style.dash = d;
        }
        if let Some(v) = self.opacity {
            o.style.opacity = v;
        }
        if let Some(v) = self.roundness {
            o.style.roundness = v;
        }
        if let Some(l) = self.locked {
            o.locked = l;
        }
        let kind = kind_name(o);
        let present = |ps: &[(&'static str, bool)]| ps.iter().filter(|p| p.1).map(|p| p.0).collect::<Vec<_>>();
        let font = [("font_size", self.size.is_some()), ("font_family", self.family.is_some()), ("align", self.align.is_some())];
        let link = [("route", self.route.is_some()), ("arrow", self.arrow.is_some()), ("from", self.from.is_some()), ("to", self.to.is_some())];
        let rotation = [("rotation", self.angle.is_some())];
        let mut ignored: Vec<&str> = Vec::new();
        match &mut o.kind {
            Kind::Box(b) => {
                if let Some(r) = self.angle {
                    b.frame.angle = r;
                }
                if self.has_font() {
                    self.font(&mut b.label);
                    b.fit(&ApproxMeasurer);
                }
                ignored.extend(present(&link));
            }
            Kind::Image(i) => {
                if let Some(r) = self.angle {
                    i.frame.angle = r;
                }
                ignored.extend(present(&font));
                ignored.extend(present(&link));
            }
            Kind::Table(t) => {
                if let Some(s) = self.size {
                    t.size = s;
                }
                if let Some(f) = self.family {
                    t.family = f;
                }
                if let Some(c) = &self.cell {
                    if let Some(slot) = t.cell_mut(c.row, c.col) {
                        *slot = c.text.clone();
                    }
                    // as `add_table`: the column is at least as wide as its widest text
                    let widest = ApproxMeasurer.width(&c.text.to_plain(), FontFamily::Sans, 18.0, true);
                    t.col_w[c.col] = t.col_w[c.col].max((widest + 28.0).ceil());
                }
                ignored.extend(present(&[("align", self.align.is_some())]));
                ignored.extend(present(&rotation));
                ignored.extend(present(&link));
            }
            Kind::Connector(c) => {
                if let Some(r) = self.route {
                    c.route = r;
                }
                if let Some((s, e)) = self.arrow {
                    (c.head_start, c.head_end) = (s, e);
                }
                if let Some(id) = &self.from {
                    c.start = inkboard_core::End::bound(id.clone());
                }
                if let Some(id) = &self.to {
                    c.end = inkboard_core::End::bound(id.clone());
                }
                match &mut c.label {
                    Some(l) => self.font(l),
                    None if self.has_font() => {
                        warn.push("font_size, font_family and align apply to the label, and this connector has no label (give `text` too)".into());
                    }
                    None => {}
                }
                ignored.extend(present(&rotation));
            }
            Kind::Pen(_) => {
                ignored.extend(present(&font));
                ignored.extend(present(&rotation));
                ignored.extend(present(&link));
            }
        }
        if !ignored.is_empty() {
            warn.push(format!("{} not applicable to a {kind} (ignored)", ignored.join(", ")));
        }
    }
}

/* ------------------------------------------------------------ edit tools */

fn delete_objects(ed: &mut Editor, a: &Value) -> Res<Value> {
    let has = |k: &str| a.get(k).is_some_and(|v| !v.is_null());
    match (has("id"), has("ids")) {
        (true, true) => Err("give either \"id\" or \"ids\", not both".into()),
        (false, false) => Err("missing \"id\" (one object) or \"ids\" (a list of objects) parameter".into()),
        (true, false) => {
            let id = find(ed, s(a, "id")?)?.id.clone();
            ed.remove(std::slice::from_ref(&id));
            Ok(json!({"deleted": id.0}))
        }
        (false, true) => {
            let ids = ids_arg(ed, a, "ids")?;
            ed.remove(&ids);
            Ok(json!({"deleted": ids.iter().map(|i| i.0.clone()).collect::<Vec<_>>()}))
        }
    }
}

fn move_objects(ed: &mut Editor, a: &Value) -> Res<Value> {
    let ids = ids_arg(ed, a, "ids")?;
    let [dx, dy, x, y] = ["dx", "dy", "x", "y"].map(|k| num_arg(a, k, -MAX_COORD, MAX_COORD));
    let (dx, dy, x, y) = (dx?, dy?, x?, y?);
    let d = match (dx.is_some() || dy.is_some(), x.is_some() || y.is_some()) {
        (true, true) => return Err("give either dx/dy (relative move) or x/y (new top-left corner), not both".into()),
        (false, false) => return Err("give dx/dy (relative move) or x/y (new top-left corner of the bounding box of the objects)".into()),
        (true, false) => Pt::new(dx.unwrap_or(0.0), dy.unwrap_or(0.0)),
        (false, true) => {
            let page = ed.page();
            let b = ids.iter().filter_map(|i| page.get(i)).map(|o| frame_box(page, o)).reduce(|p, q| p.union(&q)).ok_or("no object to move")?;
            Pt::new(x.map_or(0.0, |x| x - b.x), y.map_or(0.0, |y| y - b.y))
        }
    };
    ed.translate(&ids, d);
    Ok(json!({"moved": ids.len(), "dx": r3(d.x), "dy": r3(d.y)}))
}

fn arrange_objects(ed: &mut Editor, a: &Value) -> Res<Value> {
    let action = s(a, "action")?;
    if !ACTIONS.contains(&action) {
        return Err(format!("unknown action \"{action}\" (one of: {})", ACTIONS.join(", ")));
    }
    let ids = ids_arg(ed, a, "ids")?;
    let wanted: HashSet<&Id> = ids.iter().collect();
    let page = ed.page();
    let locked: Vec<String> = page.objects.iter().filter(|o| wanted.contains(&o.id) && page.is_locked(o)).map(|o| o.id.0.clone()).collect();
    let free = ids.len() - locked.len();
    let need = if action.starts_with("distribute") {
        3
    } else if action.starts_with("align") {
        2
    } else {
        0
    };
    if free < need {
        return Err(format!("{action} needs at least {need} unlocked objects (got {free} of {}; locked objects are skipped)", ids.len()));
    }
    if action == "group" && ids.len() < 2 {
        return Err("group needs at least 2 objects".into());
    }
    let grouped = page.objects.iter().filter(|o| wanted.contains(&o.id) && o.group.is_some()).count();
    let mut out = json!({"action": action, "objects": ids.len()});
    match action {
        "align_left" => ed.align(&ids, AlignMode::Left),
        "align_center_x" => ed.align(&ids, AlignMode::CenterX),
        "align_right" => ed.align(&ids, AlignMode::Right),
        "align_top" => ed.align(&ids, AlignMode::Top),
        "align_center_y" => ed.align(&ids, AlignMode::CenterY),
        "align_bottom" => ed.align(&ids, AlignMode::Bottom),
        "distribute_horizontal" => ed.distribute(&ids, Axis::Horizontal),
        "distribute_vertical" => ed.distribute(&ids, Axis::Vertical),
        "bring_to_front" => ed.reorder(&ids, Order::Front),
        "bring_forward" => ed.reorder(&ids, Order::Forward),
        "send_backward" => ed.reorder(&ids, Order::Backward),
        "send_to_back" => ed.reorder(&ids, Order::Back),
        "group" => {
            ed.group(&ids);
            out["group"] = json!(ed.page().get(&ids[0]).and_then(|o| o.group.as_ref()).map(|g| g.0.clone()));
        }
        "ungroup" => {
            ed.ungroup(&ids);
            out["ungrouped"] = json!(grouped);
        }
        "lock" => ed.set_locked(&ids, true),
        _ => ed.set_locked(&ids, false),
    }
    if (need > 0) && !locked.is_empty() {
        out["skipped_locked"] = json!(locked);
    }
    Ok(out)
}

fn manage_pages(ed: &mut Editor, a: &Value) -> Res<Value> {
    let action = s(a, "action")?;
    if !["rename", "delete", "list"].contains(&action) {
        return Err(format!("unknown action \"{action}\" (rename, delete, list)"));
    }
    if action == "list" {
        let pages: Vec<Value> = ed.doc.pages.iter().enumerate().map(|(i, p)| json!({"number": i + 1, "name": p.name, "objects": p.objects.len()})).collect();
        return Ok(json!({"pages": pages}));
    }
    // `apply` already resolved `page` into the current page; refuse to guess which page is meant
    if a.get("page").is_none() {
        return Err(format!("\"page\" is required to {action} a page (number from 1, or name)"));
    }
    let id = ed.current.clone();
    let index = ed.doc.pages.iter().position(|p| p.id == id).ok_or("page not found")?;
    let (number, old, objects) = (index + 1, ed.doc.pages[index].name.clone(), ed.doc.pages[index].objects.len());
    if action == "delete" {
        if ed.doc.pages.len() <= 1 {
            return Err("cannot delete the last page of a board".into());
        }
        ed.remove_page(&id);
        return Ok(json!({"deleted": {"number": number, "name": old}, "objects_removed": objects, "pages": ed.doc.pages.len()}));
    }
    let name = opt_str(a, "name")?.map(str::trim).filter(|n| !n.is_empty()).ok_or("\"name\" is required to rename a page (not empty)")?;
    if name.chars().count() > 120 {
        return Err("\"name\" is too long (120 characters at most)".into());
    }
    ed.rename_page(&id, name);
    let mut out = json!({"page": number, "name": name, "previous_name": old});
    if ed.doc.pages.iter().any(|p| p.id != id && p.name == name) {
        richtext::add_warnings(&mut out, vec![format!("another page is already named \"{name}\": refer to pages by number")]);
    }
    Ok(out)
}

fn duplicate_objects(ed: &mut Editor, a: &Value) -> Res<Value> {
    let ids = ids_arg(ed, a, "ids")?;
    let (dx, dy) = (num_arg(a, "dx", -MAX_COORD, MAX_COORD)?.unwrap_or(40.0), num_arg(a, "dy", -MAX_COORD, MAX_COORD)?.unwrap_or(40.0));
    let count = usize::try_from(int_arg(a, "count", 1, 50, 1)?).map_err(|e| e.to_string())?;
    if ids.len() * count > 10_000 {
        return Err(format!("too many copies ({} objects x {count}; 10000 at most)", ids.len()));
    }
    // copies are made in page order, whatever the order of `ids`
    let sources: Vec<String> = ed.page().objects.iter().filter(|o| ids.contains(&o.id)).map(|o| o.id.0.clone()).collect();
    let created = ed.transact(|e| {
        let mut all = Vec::new();
        for k in 1..=count {
            all.extend(e.duplicate(&ids, Pt::new(dx * num::f64_of(k), dy * num::f64_of(k))));
        }
        all
    });
    Ok(json!({"created": created.len(), "sources": sources, "ids": created.iter().map(|i| i.0.clone()).collect::<Vec<_>>()}))
}

/* ----------------------------------------------------------------- schemas */

/// Schemas for `tools/list`.
pub(super) fn specs() -> Vec<Value> {
    let ids = |what: &str| json!({"type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": MAX_IDS, "description": format!("Ids of the objects {what} (1 to {MAX_IDS}; ids come from describe_board, find_objects or the add_* tools). Groups are not expanded: list every member.")});
    vec![
        json!({"name": "find_objects",
            "description": "Finds objects on a page without reading the whole board: by text, regex, type or area. Filters combine (all must match). `text` is a case-insensitive substring of a shape's text, a connector's label, a table's cells or a formula's LaTeX. `in_region` keeps objects whose bounding box overlaps the rectangle. Returns `count` (all matches), `truncated` (more than `limit`) and `objects` (same summaries as describe_board).",
            "inputSchema": obj(json!({"path": path_prop(), "page": page_prop(),
                "text": {"type": "string", "description": "Case-insensitive substring (not empty)."},
                "regex": {"type": "string", "description": "Regular expression (Rust syntax, 500 characters at most, matched anywhere in the text; use (?i) for case-insensitive). Alternative to `text`."},
                "type": {"type": "string", "enum": TYPES},
                "in_region": {"type": "object", "properties": {"x": {"type": "number"}, "y": {"type": "number"}, "w": {"type": "number"}, "h": {"type": "number"}}, "required": ["x", "y", "w", "h"], "description": "Rectangle of the board, same units as x/y/w/h."},
                "limit": {"type": "integer", "minimum": 1, "maximum": 500, "description": "Default 50."}}), &[])}),
        json!({"name": "move_objects",
            "description": "Moves objects together. Either relative (`dx`, `dy`) or absolute (`x`, `y`: new top-left corner of the bounding box of all the objects, as update_object's x/y for one). Connectors bound to moved objects follow. Locked objects move too.",
            "inputSchema": obj(json!({"path": path_prop(), "page": page_prop(), "ids": ids("to move"),
                "dx": {"type": "number", "description": "Horizontal offset (right is positive)."}, "dy": {"type": "number", "description": "Vertical offset (down is positive)."},
                "x": {"type": "number", "description": "New left edge of the group of objects."}, "y": {"type": "number", "description": "New top edge of the group of objects."}}), &["ids"])}),
        json!({"name": "arrange_objects",
            "description": "Aligns, distributes, stacks, groups or locks objects. align_*: at least 2 unlocked objects, aligned on their common bounding box. distribute_*: at least 3 unlocked objects, equal gaps between the two outermost (an error otherwise; the engine alone would do nothing). Locked objects are skipped by align/distribute and reported in `skipped_locked`. bring_to_front/bring_forward/send_backward/send_to_back change the stacking order (`z` of describe_board detail \"full\"). group (2+ objects, returns `group`), ungroup, lock, unlock.",
            "inputSchema": obj(json!({"path": path_prop(), "page": page_prop(), "ids": ids("to arrange"), "action": {"type": "string", "enum": ACTIONS}}), &["ids", "action"])}),
        json!({"name": "manage_pages",
            "description": "Lists, renames or deletes pages (add_page creates them). `list` returns number, name and object count. `rename` and `delete` need `page` (number from 1, or name); rename also needs `name`. The last page of a board cannot be deleted; deleting a page deletes its objects.",
            "inputSchema": obj(json!({"path": path_prop(), "page": page_prop(), "action": {"type": "string", "enum": ["rename", "delete", "list"]},
                "name": {"type": "string", "description": "New name (rename), 120 characters at most."}}), &["action"])}),
        json!({"name": "duplicate_objects",
            "description": "Copies objects, each copy shifted by (dx, dy) from the previous one (default 40, 40; `count` copies, default 1, 50 at most). Connectors between copied objects are copied and linked to the copies; a connector linked to an object that is not copied becomes free in the copy. Groups are copied as new groups. Returns `sources` (copied ids in page order) and `ids` (new ids: copy 1 in the order of `sources`, then copy 2...).",
            "inputSchema": obj(json!({"path": path_prop(), "page": page_prop(), "ids": ids("to copy"),
                "dx": {"type": "number", "description": "Default 40."}, "dy": {"type": "number", "description": "Default 40."},
                "count": {"type": "integer", "minimum": 1, "maximum": 50, "description": "Default 1."}}), &["ids"])}),
    ]
}

/// Runs the tool when this module owns it, `None` otherwise.
pub(super) fn apply(ed: &mut Editor, name: &str, a: &Value) -> Option<Res<Value>> {
    Some(match name {
        "find_objects" => find_objects(ed, a),
        "move_objects" => move_objects(ed, a),
        "arrange_objects" => arrange_objects(ed, a),
        "manage_pages" => manage_pages(ed, a),
        "duplicate_objects" => duplicate_objects(ed, a),
        // intercepts the existing tool to accept `ids` (the historical arm in `tools.rs` is no longer reached)
        "delete_object" => delete_objects(ed, a),
        _ => return None,
    })
}
