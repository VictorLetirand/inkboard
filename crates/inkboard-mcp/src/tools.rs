//! MCP tools: each tool opens the file `path`, applies a change through the engine's `Editor` and saves it again (atomically).

use crate::Server;
use inkboard_core::layout::{Direction, auto_layout};
use inkboard_core::svg::{SvgOptions, export as export_svg};
use inkboard_core::*;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

mod batch;
mod edit;
mod icons;
mod render;
mod richtext;

type Res<T> = Result<T, String>;

/// File extensions the tools accept to write.
const EXPORTS: [&str; 9] = [".inkboard", ".inkboard.json", ".drawio", ".excalidraw", ".canvas", ".svg", ".png", ".pdf", ".json"];

fn obj(props: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": props, "required": required})
}

fn path_prop() -> Value {
    json!({"type": "string", "description": "Path of the board file (.inkboard; .inkboard.json and .tableau.json also accepted). Optional when the application is open: the tool then acts live on the displayed board (undoable with Ctrl+Z)."})
}

fn page_prop() -> Value {
    json!({"description": "Page: number (from 1) or name. Defaults to the first.", "oneOf": [{"type": "integer"}, {"type": "string"}]})
}

fn pos_props(mut p: Value) -> Value {
    let m = p.as_object_mut().expect("object");
    m.insert("path".into(), path_prop());
    m.insert("page".into(), page_prop());
    m.insert("x".into(), json!({"type": "number", "description": "X of the top-left corner. Defaults to below the existing content."}));
    m.insert("y".into(), json!({"type": "number", "description": "Y of the top-left corner."}));
    p
}

/// Description of the tools (name, description, input schema) for `tools/list`.
pub fn list() -> Vec<Value> {
    let color = json!({"type": "string", "description": "Color \"#rrggbb\" or \"ink\" (the theme's text color)."});
    let mut tools = vec![
        json!({"name": "create_board", "description": "Creates an empty board (.inkboard file).",
            "inputSchema": obj(json!({"path": path_prop(), "name": {"type": "string"}}), &["path"])}),
        json!({"name": "save_board", "description": "Saves the board displayed in the application (live mode; it must already have a file). No effect in file mode: every tool already saves there.",
            "inputSchema": obj(json!({}), &[])}),
        json!({"name": "describe_board", "description": "Lists the pages and objects of a board (id, type, text, position, connector links). In live mode, also reports the user's selection (`selection`). Also reads draw.io, Excalidraw, JSON Canvas and Mermaid files.",
            "inputSchema": obj(json!({"path": path_prop(), "page": page_prop()}), &[])}),
        json!({"name": "board_changes", "description": "Reports what the USER changed on the board since your last look (describe_board or board_changes): objects added, modified, deleted. Your own changes are not reported. The first call records a baseline.",
            "inputSchema": obj(json!({"path": path_prop()}), &[])}),
        json!({"name": "add_image", "description": "Adds an image (PNG, JPEG, GIF, WebP or SVG, 15 MiB at most) from a file (`source`, under the allowed root) or from base64 data (`data_base64`). The natural size is reduced to at most 600; w and/or h override it.",
            "inputSchema": obj(pos_props(json!({"source": {"type": "string", "description": "Path of the image file."}, "data_base64": {"type": "string"}, "w": {"type": "number"}, "h": {"type": "number"}})), &[])}),
        json!({"name": "add_shape", "description": "Adds a shape with text (rect, ellipse, diamond, triangle, sticky = sticky note, text = free text). Returns the id.",
            "inputSchema": obj(pos_props(json!({
                "geom": {"type": "string", "enum": ["rect", "ellipse", "diamond", "triangle", "sticky", "text"]},
                "text": {"type": "string"}, "w": {"type": "number"}, "h": {"type": "number"},
                "font_size": {"type": "number"}, "fill": color.clone(), "stroke": color.clone()})), &["geom"])}),
        json!({"name": "add_connector", "description": "Connects two objects with an arrow that follows them when they move (ids returned by add_shape or describe_board).",
            "inputSchema": obj(json!({"path": path_prop(), "page": page_prop(), "from": {"type": "string"}, "to": {"type": "string"},
                "route": {"type": "string", "enum": ["straight", "elbow", "curve"]},
                "arrow": {"type": "string", "enum": ["end", "start", "both", "none"], "description": "Arrowheads (default: end)."},
                "label": {"type": "string"}, "stroke": color.clone()}), &["from", "to"])}),
        json!({"name": "add_table", "description": "Adds a table of cells; `rows` is a list of rows (lists of texts). The first row is the header.",
            "inputSchema": obj(pos_props(json!({"rows": {"type": "array", "items": {"type": "array", "items": {"type": "string"}}}})), &["rows"])}),
        json!({"name": "add_code_block", "description": "Adds a code block (monospace font, dark background, syntax highlighting).",
            "inputSchema": obj(pos_props(json!({"code": {"type": "string"}})), &["code"])}),
        json!({"name": "add_formula", "description": "Adds a LaTeX formula (math mode) typeset the way TeX does. Errors if the LaTeX is not supported.",
            "inputSchema": obj(pos_props(json!({"latex": {"type": "string"}, "color": color.clone()})), &["latex"])}),
        json!({"name": "import_mermaid", "description": "Adds a Mermaid diagram (flowchart/graph, stateDiagram, sequenceDiagram) with automatic layout. Returns the ids created.",
            "inputSchema": obj(pos_props(json!({"mermaid": {"type": "string"}, "new_page": {"type": "boolean", "description": "Create a new page for the diagram."}})), &["mermaid"])}),
        json!({"name": "update_object", "description": "Modifies an object: text, position (top-left corner), size (shapes), colors.",
            "inputSchema": obj(json!({"path": path_prop(), "page": page_prop(), "id": {"type": "string"}, "text": {"type": "string"},
                "x": {"type": "number"}, "y": {"type": "number"}, "w": {"type": "number"}, "h": {"type": "number"},
                "fill": color.clone(), "stroke": color.clone()}), &["id"])}),
        json!({"name": "delete_object", "description": "Deletes an object (connectors that were bound to it remain, detached).",
            "inputSchema": obj(json!({"path": path_prop(), "page": page_prop(), "id": {"type": "string"}}), &["id"])}),
        json!({"name": "auto_layout", "description": "Rearranges the shapes joined by connectors into a layered graph.",
            "inputSchema": obj(json!({"path": path_prop(), "page": page_prop(), "direction": {"type": "string", "enum": ["TB", "LR", "BT", "RL"], "description": "Default: TB."}}), &[])}),
        json!({"name": "add_page", "description": "Adds a page. Returns its number.",
            "inputSchema": obj(json!({"path": path_prop(), "name": {"type": "string"}}), &[])}),
        json!({"name": "export", "description": "Exports a page to a file; the format is chosen by the extension of `output`: .svg, .png, .pdf, .drawio, .excalidraw, .canvas, .inkboard.",
            "inputSchema": obj(json!({"path": path_prop(), "output": {"type": "string"}, "page": page_prop()}), &["output"])}),
    ];
    tools.extend(ext_specs());
    tools
}

/* ------------------------------------------------- tools of the submodules */

fn ext_specs() -> Vec<Value> {
    [icons::specs(), render::specs(), edit::specs(), batch::specs()].concat()
}

fn ext_is_tool(name: &str) -> bool {
    [icons::NAMES, render::NAMES, edit::NAMES, batch::NAMES].iter().any(|l| l.contains(&name))
}

fn ext_is_read_only(name: &str) -> bool {
    [icons::READ_ONLY, render::READ_ONLY, edit::READ_ONLY, batch::READ_ONLY].iter().any(|l| l.contains(&name))
}

fn ext_apply(ed: &mut Editor, name: &str, a: &Value) -> Option<Res<Value>> {
    icons::apply(ed, name, a).or_else(|| render::apply(ed, name, a)).or_else(|| edit::apply(ed, name, a)).or_else(|| batch::apply(ed, name, a))
}

/* ------------------------------------------------------------ arguments */

fn s<'a>(a: &'a Value, k: &str) -> Res<&'a str> {
    a.get(k).and_then(Value::as_str).ok_or_else(|| format!("missing text parameter \"{k}\""))
}
fn os<'a>(a: &'a Value, k: &str) -> Option<&'a str> {
    a.get(k).and_then(Value::as_str)
}
fn of(a: &Value, k: &str) -> Option<f64> {
    a.get(k).and_then(Value::as_f64)
}
fn ocolor(a: &Value, k: &str) -> Res<Option<Color>> {
    match os(a, k) {
        None => Ok(None),
        Some(c) => Color::parse(c).map(Some).ok_or_else(|| format!("invalid color \"{c}\" (expected #rrggbb or ink)")),
    }
}

/// Resolves a path, confined to the server root when one is set (links and `..` are neutralized by canonicalizing the parent folder).
fn resolve(srv: &Server, p: &str) -> Res<PathBuf> {
    let path = PathBuf::from(p);
    let Some(root) = &srv.root else { return Ok(path) };
    let root = root.canonicalize().map_err(|e| format!("root not accessible: {e}"))?;
    let abs = if path.is_absolute() { path } else { root.join(path) };
    let parent = abs.parent().ok_or("invalid path")?;
    let parent = parent.canonicalize().map_err(|e| format!("folder not accessible ({}): {e}", parent.display()))?;
    if !parent.starts_with(&root) {
        return Err(format!("path outside the allowed root ({})", root.display()));
    }
    let full = parent.join(abs.file_name().ok_or("invalid path")?);
    // an existing final entry may itself be a link: resolve it too (a dangling link cannot be checked, so it is refused)
    if std::fs::symlink_metadata(&full).is_ok() {
        let real = full.canonicalize().map_err(|e| format!("file not accessible ({}): {e}", full.display()))?;
        if !real.starts_with(&root) {
            return Err(format!("path outside the allowed root ({})", root.display()));
        }
    }
    Ok(full)
}

fn lower_name(p: &Path) -> String {
    p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase()
}

fn load(path: &Path) -> Res<Document> {
    let content = std::fs::read_to_string(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    inkboard_formats::import_any(name, &content, &ApproxMeasurer).map(|i| i.doc).map_err(|e| format!("opening {name}: {e}"))
}

/// Saving: native format only, so a file from another tool is never overwritten with different content.
fn save(doc: &Document, path: &Path) -> Res<()> {
    // `.inkboard.json` / `.tableau.json`: older names, still accepted for existing files
    if !inkboard_core::serial::is_native_name(&lower_name(path)) {
        return Err("changes are saved to an .inkboard file (or an existing .inkboard.json / .tableau.json; use `export` for other formats)".into());
    }
    inkboard_core::serial::save(doc, path).map_err(|e| format!("writing {}: {e}", path.display()))
}

fn select_page(ed: &mut Editor, spec: Option<&Value>) -> Res<()> {
    let Some(spec) = spec else { return Ok(()) };
    let pages = &ed.doc.pages;
    let found = match spec {
        Value::Number(n) => n.as_u64().and_then(|n| usize::try_from(n).ok()).and_then(|n| n.checked_sub(1)).and_then(|i| pages.get(i)),
        Value::String(name) => pages.iter().find(|p| &p.name == name),
        _ => None,
    };
    let id = found.ok_or_else(|| format!("page not found ({spec}); the board has {} page(s)", pages.len()))?.id.clone();
    ed.current = id;
    Ok(())
}

fn origin(ed: &Editor, a: &Value) -> Pt {
    match (of(a, "x"), of(a, "y")) {
        (Some(x), Some(y)) => Pt::new(x, y),
        _ => ed.page().content_bounds().map_or(Pt::new(0.0, 0.0), |b| Pt::new(of(a, "x").unwrap_or(b.x), of(a, "y").unwrap_or(b.bottom() + 40.0))),
    }
}

fn find<'a>(ed: &'a Editor, id: &str) -> Res<&'a Object> {
    ed.page().get(&Id(id.to_owned())).ok_or_else(|| format!("object not found: {id}"))
}

/* ----------------------------------------------------------------- appel */

/// Tools that do not modify the board (the file is not rewritten).
const READ_ONLY: [&str; 3] = ["describe_board", "export", "board_changes"];

/// Tools that apply to an open board (file or application); `create_board` only creates files.
const EDITOR_TOOLS: [&str; 14] = [
    "describe_board",
    "board_changes",
    "add_image",
    "add_shape",
    "add_connector",
    "add_table",
    "add_code_block",
    "add_formula",
    "import_mermaid",
    "update_object",
    "delete_object",
    "auto_layout",
    "add_page",
    "export",
];

/// Applies a tool to an in-memory board. Used as is by file mode and by the application (live mode, with its undo history).
pub fn apply(ed: &mut Editor, name: &str, a: &Value) -> Res<Value> {
    if name == "save_board" {
        return Ok(json!({"saved": true}));
    }
    if !EDITOR_TOOLS.contains(&name) && !ext_is_tool(name) {
        return Err(format!("unknown tool: {name}"));
    }
    if name != "add_page" {
        select_page(ed, a.get("page"))?;
    }
    if let Some(r) = ext_apply(ed, name, a) {
        return r;
    }
    match name {
        "describe_board" => describe(ed, a),
        "board_changes" => Err("board_changes only makes sense with change tracking (apply_tracked)".into()),
        "add_image" => add_image(ed, a),
        "add_shape" => add_shape(ed, a),
        "add_connector" => add_connector(ed, a),
        "add_table" => add_table(ed, a),
        "add_code_block" => add_code_block(ed, a),
        "add_formula" => add_formula(ed, a),
        "import_mermaid" => import_mermaid(ed, a),
        "update_object" => update_object(ed, a),
        "delete_object" => {
            let id = find(ed, s(a, "id")?)?.id.clone();
            ed.remove(std::slice::from_ref(&id));
            Ok(json!({"deleted": id.0}))
        }
        "auto_layout" => {
            let dir = match os(a, "direction").unwrap_or("TB") {
                "TB" => Direction::TopDown,
                "LR" => Direction::LeftRight,
                "BT" => Direction::BottomUp,
                "RL" => Direction::RightLeft,
                d => return Err(format!("unknown direction \"{d}\" (TB, LR, BT, RL)")),
            };
            let ids: Vec<Id> = ed.page().objects.iter().map(|o| o.id.clone()).collect();
            auto_layout(ed, &ids, dir);
            Ok(json!({"layout": "ok", "objects": ids.len()}))
        }
        "add_page" => {
            let n = ed.doc.pages.len() + 1;
            ed.add_page(os(a, "name").map_or_else(|| format!("Page {n}"), str::to_owned).as_str());
            Ok(json!({"page": n, "pages": n}))
        }
        "export" => export(ed, a),
        _ => unreachable!("tool checked above"),
    }
}

/// Runs the tool `name`: in the open application if it is reachable and shows this board (live mode), otherwise on the file `path`.
/// The result is a JSON text intended for the agent.
pub fn call(srv: &Server, name: &str, a: &Value) -> Res<String> {
    if name != "create_board" && name != "save_board" && !EDITOR_TOOLS.contains(&name) && !ext_is_tool(name) {
        return Err(format!("unknown tool: {name}"));
    }
    let mut a = a.clone();
    // paths are resolved (and confined to the root) here, never in the application
    for key in ["path", "output"] {
        if let Some(p) = os(&a, key) {
            let abs = resolve(srv, p)?;
            a[key] = json!(abs.display().to_string());
        }
    }
    if name == "add_image"
        && let Some(src) = os(&a, "source").map(str::to_owned)
    {
        // the file is read here (confined to the root); only the data is passed to the application
        let abs = resolve(srv, &src)?;
        let meta = std::fs::metadata(&abs).map_err(|e| format!("reading {}: {e}", abs.display()))?;
        if meta.len() as usize > IMAGE_MAX_BYTES {
            return Err(format!("image too large ({} MiB, maximum {})", meta.len() >> 20, IMAGE_MAX_BYTES >> 20));
        }
        use base64::Engine;
        let bytes = std::fs::read(&abs).map_err(|e| format!("reading {}: {e}", abs.display()))?;
        a["data_base64"] = json!(base64::engine::general_purpose::STANDARD.encode(bytes));
        a.as_object_mut().map(|m| m.remove("source"));
    }
    if name != "create_board" {
        match crate::live::forward(srv, name, &a) {
            crate::live::Forward::Done(r) => return r.map(|v| v.to_string()),
            crate::live::Forward::NotLive => {}
        }
    }
    if name == "create_board" {
        let path = PathBuf::from(s(&a, "path")?);
        if path.exists() {
            return Err(format!("{} already exists", path.display()));
        }
        let doc = Document::new(os(&a, "name").unwrap_or("Board"));
        save(&doc, &path)?;
        return Ok(json!({"path": path.display().to_string(), "pages": 1}).to_string());
    }
    let Some(path) = os(&a, "path").map(PathBuf::from) else {
        return Err("missing \"path\" parameter: no board is open in the application (or it is closed); give the .inkboard file".into());
    };
    let mut ed = Editor::new(load(&path)?);
    let mut tracker = srv.trackers.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&path);
    let r = apply_tracked(&mut ed, name, &a, &mut tracker);
    if let Some(t) = tracker {
        srv.trackers.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(path.clone(), t);
    }
    let r = r?;
    if !READ_ONLY.contains(&name) && !ext_is_read_only(name) && name != "save_board" {
        save(&ed.doc, &path)?;
    }
    Ok(r.to_string())
}

fn geom_of(g: &str) -> Res<BoxGeom> {
    Ok(match g {
        "rect" => BoxGeom::Rect,
        "ellipse" => BoxGeom::Ellipse,
        "diamond" => BoxGeom::Diamond,
        "triangle" => BoxGeom::Triangle,
        "sticky" => BoxGeom::Sticky,
        "text" => BoxGeom::Text,
        other => return Err(format!("unknown shape \"{other}\" (rect, ellipse, diamond, triangle, sticky, text)")),
    })
}

fn kind_name(o: &Object) -> &'static str {
    match &o.kind {
        Kind::Box(b) => match b.geom {
            BoxGeom::Rect => "rect",
            BoxGeom::Ellipse => "ellipse",
            BoxGeom::Diamond => "diamond",
            BoxGeom::Triangle => "triangle",
            BoxGeom::Sticky => "sticky",
            BoxGeom::Text => "text",
        },
        Kind::Pen(_) => "pen",
        Kind::Connector(_) => "connector",
        Kind::Image(i) if i.latex.is_some() => "formula",
        Kind::Image(_) => "image",
        Kind::Table(_) => "table",
    }
}

fn end_json(e: &connector::End) -> Value {
    match e {
        connector::End::Bound { target, .. } => json!({"object": target.0}),
        connector::End::Free { at } => json!({"x": at.x, "y": at.y}),
    }
}

/// JSON summary of an object (id, type, position, text, links).
pub fn summary(o: &Object) -> Value {
    let mut v = json!({"id": o.id.0, "type": kind_name(o)});
    let m = v.as_object_mut().expect("object");
    if let Some(f) = o.frame() {
        m.insert("x".into(), json!(f.x.round()));
        m.insert("y".into(), json!(f.y.round()));
        m.insert("w".into(), json!(f.w.round()));
        m.insert("h".into(), json!(f.h.round()));
    }
    match &o.kind {
        Kind::Box(b) => {
            m.insert("text".into(), json!(b.label.text.to_plain()));
        }
        Kind::Connector(c) => {
            m.insert("from".into(), end_json(&c.start));
            m.insert("to".into(), end_json(&c.end));
            if let Some(l) = &c.label {
                m.insert("label".into(), json!(l.text.to_plain()));
            }
        }
        Kind::Table(t) => {
            m.insert("rows".into(), json!(t.rows()));
            m.insert("cols".into(), json!(t.cols()));
            let r = t.rect();
            m.insert("x".into(), json!(r.x.round()));
            m.insert("y".into(), json!(r.y.round()));
            m.insert("w".into(), json!(r.w.round()));
            m.insert("h".into(), json!(r.h.round()));
        }
        Kind::Image(i) => {
            if let Some(l) = &i.latex {
                m.insert("latex".into(), json!(l));
            }
        }
        Kind::Pen(_) => {}
    }
    v
}

fn describe(ed: &mut Editor, _a: &Value) -> Res<Value> {
    let page = ed.page();
    let objects: Vec<Value> = page.objects.iter().map(summary).collect();
    let pages: Vec<Value> = ed.doc.pages.iter().enumerate().map(|(i, p)| json!({"number": i + 1, "name": p.name, "objects": p.objects.len()})).collect();
    Ok(json!({"name": ed.doc.name, "pages": pages, "page": {"name": page.name, "objects": objects}}))
}

/* ------------------------------------------------- changements faits par l'utilisateur */

/// Fingerprint of a board's content: one value per object, to detect what was added, deleted or modified.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    objects: std::collections::HashMap<String, u64>,
    pages: Vec<(String, String)>,
}

/// Beyond that, change tracking is disabled (the cost is linear on every tool call).
const TRACK_MAX_OBJECTS: usize = 20_000;

impl Snapshot {
    pub fn of(ed: &Editor) -> Self {
        use std::hash::{Hash, Hasher};
        let mut objects = std::collections::HashMap::new();
        for p in &ed.doc.pages {
            for o in &p.objects {
                let mut h = std::collections::hash_map::DefaultHasher::new();
                serde_json::to_string(o).unwrap_or_default().hash(&mut h);
                objects.insert(o.id.0.clone(), h.finish());
            }
        }
        Snapshot { objects, pages: ed.doc.pages.iter().map(|p| (p.id.0.clone(), p.name.clone())).collect() }
    }

    fn too_big(ed: &Editor) -> bool {
        ed.doc.pages.iter().map(|p| p.objects.len()).sum::<usize>() > TRACK_MAX_OBJECTS
    }
}

fn find_anywhere<'a>(ed: &'a Editor, id: &str) -> Option<&'a Object> {
    ed.doc.pages.iter().find_map(|p| p.objects.iter().find(|o| o.id.0 == id))
}

/// Differences between the baseline `old` and the current state.
fn diff(old: &Snapshot, ed: &Editor) -> Value {
    let now = Snapshot::of(ed);
    let (mut added, mut modified, mut removed) = (Vec::new(), Vec::new(), Vec::new());
    for (id, h) in &now.objects {
        match old.objects.get(id) {
            None => added.push(id.clone()),
            Some(o) if o != h => modified.push(id.clone()),
            _ => {}
        }
    }
    removed.extend(old.objects.keys().filter(|id| !now.objects.contains_key(*id)).cloned());
    for v in [&mut added, &mut modified, &mut removed] {
        v.sort();
    }
    let sum = |ids: &[String]| -> Vec<Value> { ids.iter().filter_map(|id| find_anywhere(ed, id)).map(summary).collect() };
    json!({
        "added": sum(&added), "modified": sum(&modified), "removed": removed,
        "pages_changed": old.pages != now.pages,
        "unchanged": added.is_empty() && modified.is_empty() && removed.is_empty() && old.pages == now.pages,
    })
}

/// Folds into the baseline only the changes made by the agent (`before` → `after`), so the user's changes remain to be reported.
fn absorb(reference: &mut Snapshot, before: &Snapshot, after: &Snapshot) {
    for (id, h) in &after.objects {
        if before.objects.get(id) != Some(h) {
            reference.objects.insert(id.clone(), *h);
        }
    }
    for id in before.objects.keys().filter(|id| !after.objects.contains_key(*id)) {
        reference.objects.remove(id);
    }
    if before.pages != after.pages {
        reference.pages = after.pages.clone();
    }
}

/// Like [`apply`], while tracking changes: `board_changes` reports what the user modified since the agent's last look
/// (a `describe_board` or `board_changes` call); the agent's own changes are not reported.
pub fn apply_tracked(ed: &mut Editor, name: &str, a: &Value, tracker: &mut Option<Snapshot>) -> Res<Value> {
    if Snapshot::too_big(ed) {
        return if name == "board_changes" { Err("board too large for change tracking".into()) } else { apply(ed, name, a) };
    }
    if name == "board_changes" {
        let out = match tracker {
            None => {
                json!({"baseline": true, "message": "Baseline recorded: the next calls will report the changes the user makes afterwards."})
            }
            Some(old) => diff(old, ed),
        };
        *tracker = Some(Snapshot::of(ed));
        return Ok(out);
    }
    let before = Snapshot::of(ed);
    let r = apply(ed, name, a)?;
    let after = Snapshot::of(ed);
    // first observation or full look: the current state becomes the baseline; otherwise only the agent's changes are absorbed
    match tracker.as_mut() {
        Some(t) if name != "describe_board" => absorb(t, &before, &after),
        _ => *tracker = Some(after),
    }
    Ok(r)
}

/* ------------------------------------------------------------------ images */

/// Maximum size of an inserted image.
const IMAGE_MAX_BYTES: usize = 15 << 20;

/// Image type recognized from the first bytes (the type declared by the caller is never trusted).
fn sniff_image(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("image/png")
    } else if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if data.starts_with(b"GIF8") {
        Some("image/gif")
    } else if data.len() > 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        Some("image/webp")
    } else if std::str::from_utf8(&data[..data.len().min(2048)]).is_ok_and(|t| t.contains("<svg")) {
        Some("image/svg+xml")
    } else {
        None
    }
}

/// Natural dimensions of an image (pixels, or `viewBox` units for an SVG).
fn image_size(mime: &str, data: &[u8]) -> Option<(f64, f64)> {
    if mime == "image/svg+xml" {
        let t = std::str::from_utf8(data).ok()?;
        let vb = t.split("viewBox=\"").nth(1)?.split('"').next()?;
        let v: Vec<f64> = vb.split([' ', ',']).filter_map(|x| x.parse().ok()).collect();
        return (v.len() == 4 && v[2] > 0.0 && v[3] > 0.0).then(|| (v[2], v[3]));
    }
    let (w, h) = image::ImageReader::new(std::io::Cursor::new(data)).with_guessed_format().ok()?.into_dimensions().ok()?;
    Some((f64::from(w), f64::from(h)))
}

fn add_image(ed: &mut Editor, a: &Value) -> Res<Value> {
    use base64::Engine;
    let b64 = s(a, "data_base64").map_err(|_| "missing \"source\" (image file) or \"data_base64\" parameter".to_owned())?;
    let data = base64::engine::general_purpose::STANDARD.decode(b64.trim()).map_err(|e| format!("invalid base64: {e}"))?;
    if data.len() > IMAGE_MAX_BYTES {
        return Err(format!("image too large ({} MiB, maximum {})", data.len() >> 20, IMAGE_MAX_BYTES >> 20));
    }
    let mime = sniff_image(&data).ok_or("unrecognized image format (PNG, JPEG, GIF, WebP or SVG)")?;
    let (nw, nh) = image_size(mime, &data).ok_or("image illisible (dimensions introuvables)")?;
    // display size: requested (the ratio is kept when only one dimension is given), otherwise natural, reduced to at most 600
    let (w, h) = match (of(a, "w"), of(a, "h")) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (w, w * nh / nw),
        (None, Some(h)) => (h * nw / nh, h),
        (None, None) => {
            let k = (600.0 / nw.max(nh)).min(1.0);
            (nw * k, nh * k)
        }
    };
    if w < 1.0 || h < 1.0 || w > 20_000.0 || h > 20_000.0 {
        return Err("invalid display size (between 1 and 20,000)".into());
    }
    let aid = ed.doc.add_asset(mime, data);
    let at = origin(ed, a);
    let img = ImageObj { frame: Frame::new(at.x, at.y, w, h), asset: aid, latex: None, mono: false };
    let id = ed.insert(Object::new(Kind::Image(img), Style { width: 0.0, fill: None, ..Style::default() }));
    Ok(json!({"id": id.0, "w": w.round(), "h": h.round(), "type": mime}))
}

fn add_shape(ed: &mut Editor, a: &Value) -> Res<Value> {
    let geom = geom_of(s(a, "geom")?)?;
    let at = origin(ed, a);
    let text = os(a, "text").unwrap_or("");
    let (dw, dh) = match geom {
        BoxGeom::Sticky => (200.0, 200.0),
        BoxGeom::Ellipse | BoxGeom::Diamond => (180.0, 110.0),
        BoxGeom::Triangle => (150.0, 130.0),
        _ => (180.0, 90.0),
    };
    let fill = ocolor(a, "fill")?.or(match geom {
        BoxGeom::Sticky => Some(Color::rgb(0xff, 0xec, 0x99)),
        _ => None,
    });
    let style = Style {
        stroke: ocolor(a, "stroke")?.unwrap_or(Color::Ink),
        fill,
        width: if geom == BoxGeom::Text { 0.0 } else { 2.0 },
        roundness: if geom == BoxGeom::Rect { 0.15 } else { 0.0 },
        ..Style::default()
    };
    let label = Label::new(text, of(a, "font_size").unwrap_or(20.0), if geom == BoxGeom::Text { Align::Left } else { Align::Center });
    let mut o = Object::new_box(geom, Frame::new(at.x, at.y, of(a, "w").unwrap_or(dw), of(a, "h").unwrap_or(dh)), style, label);
    if let Kind::Box(b) = &mut o.kind {
        b.fit(&ApproxMeasurer);
    }
    let id = ed.insert(o);
    let f = ed.page().get(&id).and_then(Object::frame).ok_or("object not created")?;
    Ok(json!({"id": id.0, "x": f.x.round(), "y": f.y.round(), "w": f.w.round(), "h": f.h.round()}))
}

fn add_connector(ed: &mut Editor, a: &Value) -> Res<Value> {
    let (from, to) = (find(ed, s(a, "from")?)?, find(ed, s(a, "to")?)?);
    for o in [from, to] {
        if !o.is_attachable() {
            return Err(format!("object {} cannot be connected (shapes, tables and images only)", o.id.0));
        }
    }
    let mut c = Connector::new(End::bound(from.id.clone()), End::bound(to.id.clone()));
    c.route = match os(a, "route").unwrap_or("straight") {
        "straight" => Route::Straight,
        "elbow" => Route::Elbow,
        "curve" => Route::Curve,
        r => return Err(format!("unknown route \"{r}\" (straight, elbow, curve)")),
    };
    (c.head_start, c.head_end) = match os(a, "arrow").unwrap_or("end") {
        "end" => (Arrowhead::None, Arrowhead::Arrow),
        "start" => (Arrowhead::Arrow, Arrowhead::None),
        "both" => (Arrowhead::Arrow, Arrowhead::Arrow),
        "none" => (Arrowhead::None, Arrowhead::None),
        r => return Err(format!("unknown arrowhead \"{r}\" (end, start, both, none)")),
    };
    if let Some(l) = os(a, "label").filter(|l| !l.is_empty()) {
        c.label = Some(Label::new(l, 16.0, Align::Center));
    }
    let style = Style { stroke: ocolor(a, "stroke")?.unwrap_or(Color::Ink), width: 2.0, ..Style::default() };
    let id = ed.insert(Object::new(Kind::Connector(c), style));
    Ok(json!({"id": id.0}))
}

fn add_table(ed: &mut Editor, a: &Value) -> Res<Value> {
    let rows: Vec<Vec<String>> = a
        .get("rows")
        .and_then(Value::as_array)
        .ok_or("missing \"rows\" parameter")?
        .iter()
        .map(|r| {
            r.as_array()
                .map(|r| r.iter().map(|c| c.as_str().map_or_else(|| c.to_string(), str::to_owned)).collect())
                .ok_or("each row of \"rows\" must be a list")
        })
        .collect::<Result<_, _>>()?;
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    if rows.is_empty() || cols == 0 || rows.len() > 200 || cols > 50 {
        return Err("\"rows\" must contain 1 to 200 rows and 1 to 50 columns".into());
    }
    let at = origin(ed, a);
    let mut t = Table::new(at, rows.len(), cols);
    t.header = true;
    for (r, row) in rows.iter().enumerate() {
        for (c, text) in row.iter().enumerate() {
            if let Some(cell) = t.cell_mut(r, c) {
                *cell = RichText::plain(text);
            }
        }
    }
    // column width from the longest text (at least the default width)
    for c in 0..cols {
        let widest = rows.iter().filter_map(|r| r.get(c)).map(|t| ApproxMeasurer.width(t, FontFamily::Sans, 18.0, true)).fold(0.0, f64::max);
        t.col_w[c] = t.col_w[c].max((widest + 28.0).ceil());
    }
    let id = ed.insert(Object::new(Kind::Table(t), Style { width: 2.0, fill: None, ..Style::default() }));
    Ok(json!({"id": id.0, "rows": rows.len(), "cols": cols}))
}

fn add_code_block(ed: &mut Editor, a: &Value) -> Res<Value> {
    let code = s(a, "code")?;
    let at = origin(ed, a);
    let style = Style { stroke: Color::rgb(0xe6, 0xe6, 0xe6), fill: Some(Color::rgb(0x1e, 0x20, 0x2e)), width: 0.0, roundness: 0.1, ..Style::default() };
    let label = Label { text: inkboard_core::highlight::highlight(code), family: FontFamily::Mono, size: 18.0, align: Align::Left };
    let mut o = Object::new_box(BoxGeom::Rect, Frame::new(at.x, at.y, 420.0, 120.0), style, label);
    if let Kind::Box(b) = &mut o.kind {
        let lay = b.layout(&ApproxMeasurer);
        b.frame.w = (lay.width + 48.0).max(260.0);
        b.frame.h = (lay.height + 40.0).max(90.0);
    }
    let id = ed.insert(o);
    Ok(json!({"id": id.0}))
}

fn add_formula(ed: &mut Editor, a: &Value) -> Res<Value> {
    let latex = s(a, "latex")?;
    let f = inkboard_formats::math::render(latex, 36.0).map_err(|e| e.to_string())?;
    let aid = ed.doc.add_asset("image/svg+xml", f.svg.into_bytes());
    let at = origin(ed, a);
    let style = Style { stroke: ocolor(a, "color")?.unwrap_or(Color::Ink), width: 0.0, fill: None, ..Style::default() };
    let img = ImageObj { frame: Frame::new(at.x, at.y, f.w, f.h), asset: aid, latex: Some(latex.to_owned()), mono: false };
    let id = ed.insert(Object::new(Kind::Image(img), style));
    Ok(json!({"id": id.0, "w": f.w.round(), "h": f.h.round()}))
}

fn import_mermaid(ed: &mut Editor, a: &Value) -> Res<Value> {
    let mut objs = inkboard_formats::mermaid::to_objects(s(a, "mermaid")?, &ApproxMeasurer).map_err(|e| e.to_string())?;
    if objs.is_empty() {
        return Err("the Mermaid diagram is empty".into());
    }
    if a.get("new_page").and_then(Value::as_bool).unwrap_or(false) {
        let n = ed.doc.pages.len() + 1;
        let id = ed.add_page(&format!("Mermaid {n}"));
        ed.current = id;
    }
    let at = origin(ed, a);
    let top_left =
        objs.iter().filter_map(Object::frame).fold(None::<Pt>, |acc, f| Some(acc.map_or(Pt::new(f.x, f.y), |p| Pt::new(p.x.min(f.x), p.y.min(f.y)))));
    let d = top_left.map_or(Pt::new(0.0, 0.0), |p| at - p);
    for o in &mut objs {
        o.translate(d);
    }
    let ids = ed.insert_all(objs);
    Ok(json!({"created": ids.len(), "ids": ids.iter().map(|i| i.0.clone()).collect::<Vec<_>>()}))
}

fn update_object(ed: &mut Editor, a: &Value) -> Res<Value> {
    let id = find(ed, s(a, "id")?)?.id.clone();
    let (fill, stroke) = (ocolor(a, "fill")?, ocolor(a, "stroke")?);
    let (text, x, y, w, h) = (os(a, "text").map(str::to_owned), of(a, "x"), of(a, "y"), of(a, "w"), of(a, "h"));
    let mut warn: Vec<&str> = Vec::new();
    ed.update(&id, |o| {
        if let Some(f) = fill {
            o.style.fill = Some(f);
        }
        if let Some(s) = stroke {
            o.style.stroke = s;
        }
        match &mut o.kind {
            Kind::Box(b) => {
                if let Some(t) = &text {
                    b.label.text = RichText::plain(t);
                }
                if let Some(w) = w {
                    b.frame.w = w.max(4.0);
                }
                if let Some(h) = h {
                    b.frame.h = h.max(4.0);
                }
                b.fit(&ApproxMeasurer);
            }
            Kind::Connector(c) => {
                if let Some(t) = &text {
                    c.label = (!t.is_empty()).then(|| Label::new(t, 16.0, Align::Center));
                }
            }
            _ if text.is_some() => warn.push("text can only be changed on shapes and connectors"),
            _ => {}
        }
        if x.is_some() || y.is_some() {
            if let Some(f) = o.frame().map(|f| f.aabb()).or_else(|| matches!(o.kind, Kind::Table(_)).then(|| table_rect(o))) {
                o.translate(Pt::new(x.map_or(0.0, |x| x - f.x), y.map_or(0.0, |y| y - f.y)));
            } else {
                warn.push("this object cannot be moved with x/y");
            }
        }
    });
    Ok(json!({"id": id.0, "avertissements": warn}))
}

fn table_rect(o: &Object) -> Rect {
    match &o.kind {
        Kind::Table(t) => t.rect(),
        _ => Rect::new(0.0, 0.0, 0.0, 0.0),
    }
}

fn export(ed: &mut Editor, a: &Value) -> Res<Value> {
    let output = PathBuf::from(s(a, "output")?);
    let name = lower_name(&output);
    if !EXPORTS.iter().any(|e| name.ends_with(e)) {
        return Err(format!("unsupported output extension (expected: {})", EXPORTS.join(", ")));
    }
    let doc = &ed.doc;
    let page = ed.page();
    let svg = || export_svg(doc, page, None, &ApproxMeasurer, &SvgOptions::default()).ok_or_else(|| "the page is empty: nothing to export".to_owned());
    let bytes: Vec<u8> = if name.ends_with(".svg") {
        svg()?.into_bytes()
    } else if name.ends_with(".png") {
        inkboard_formats::raster::svg_to_png(&svg()?, 2.0).map_err(|e| e.to_string())?
    } else if name.ends_with(".pdf") {
        inkboard_formats::raster::svg_to_pdf(&svg()?).map_err(|e| e.to_string())?
    } else if name.ends_with(".drawio") {
        inkboard_formats::drawio::export(doc, &ApproxMeasurer).into_bytes()
    } else if name.ends_with(".excalidraw") {
        inkboard_formats::excalidraw::export(doc, page, &ApproxMeasurer).into_bytes()
    } else if name.ends_with(".canvas") {
        inkboard_formats::jsoncanvas::export(page).0.into_bytes()
    } else {
        inkboard_core::serial::to_json(doc, true).into_bytes()
    };
    std::fs::write(&output, &bytes).map_err(|e| format!("writing {}: {e}", output.display()))?;
    Ok(json!({"output": output.display().to_string(), "bytes": bytes.len()}))
}
