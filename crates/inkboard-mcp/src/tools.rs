//! MCP tools: each tool opens the file `path`, applies a change through the engine's `Editor` and saves it again (atomically).

use crate::Server;
use base64::Engine;
use inkboard_core::layout::{Direction, auto_layout};
use inkboard_core::svg::{SvgOptions, export as export_svg};
use inkboard_core::{
    Align, ApproxMeasurer, Arrowhead, BoxGeom, Color, Connector, Document, Editor, End, FontFamily, Frame, Id, ImageObj, Kind, Label, Measurer, Object, Pt,
    Rect, RichText, Route, Style, Table, connector,
};
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
    let mut v = json!({"type": "object", "required": required});
    v["properties"] = props;
    v
}

fn path_prop() -> Value {
    json!({"type": "string", "description": "Path of the board file (.inkboard; .inkboard.json and .tableau.json also accepted). Optional when the application is open: the tool then acts live on the displayed board (undoable with Ctrl+Z)."})
}

/// Schema of `head_start` / `head_end`: the name of an arrowhead.
fn head_prop(end: &str) -> Value {
    json!({"type": "string", "enum": edit::head_names(),
        "description": format!("Arrowhead at the {end} of the connector, which takes precedence over `arrow` for that end. Original heads: none, arrow, triangle, dot, bar. Added heads, shown by older versions as the closest original one: diamond (UML composition), diamond_open (aggregation), triangle_open (inheritance), crow_foot (many), crow_foot_one (one or many), crow_foot_zero (zero or many), one_only (exactly one), zero_or_one.")})
}

/// Schema of `shadow`: the drop shadow of an object.
fn shadow_prop() -> Value {
    json!({"type": "boolean", "description": "Soft drop shadow under the object (not drawn for pen strokes)."})
}

/// Schema of `head_scale`: the size factor of the arrowheads.
fn head_scale_prop() -> Value {
    json!({"type": "number", "minimum": 0.5, "maximum": 4, "description": "Connector arrowhead size factor (1 = usual size, 0.5 to 4); applies to both ends."})
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
#[must_use]
pub fn list() -> Vec<Value> {
    let color = json!({"type": "string", "description": "Color \"#rrggbb\" or \"ink\" (the theme's text color)."});
    let mut tools = vec![
        json!({"name": "create_board", "description": "Creates an empty board (.inkboard file).",
            "inputSchema": obj(json!({"path": path_prop(), "name": {"type": "string"}}), &["path"])}),
        json!({"name": "save_board", "description": "Saves the board displayed in the application (live mode; it must already have a file). No effect in file mode: every tool already saves there.",
            "inputSchema": obj(json!({}), &[])}),
        json!({"name": "describe_board", "description": "Lists the pages and objects of a board (id, type, text, position, connector links). In live mode, also reports the user's selection (`selection`). Also reads draw.io, Excalidraw, JSON Canvas and Mermaid files. `detail: \"full\"` adds each object's style, font, stacking order `z`, group, lock, rotation and (connectors) route and arrow; tables also list their `cells`, and SVG images the `colors` found in the file (see `recolor` of update_object).",
            "inputSchema": obj(json!({"path": path_prop(), "page": page_prop(),
                "detail": {"type": "string", "enum": ["brief", "full"], "description": "Default brief (id, type, text, position). full: also style {stroke, fill, width, dash, opacity, roundness}, font {family, size, align}, z, group, locked, rotation (degrees), route, arrow."}}), &[])}),
        json!({"name": "board_changes", "description": "Reports what the USER changed on the board since your last look (describe_board or board_changes): objects added, modified, deleted. Your own changes are not reported. The first call records a baseline.",
            "inputSchema": obj(json!({"path": path_prop()}), &[])}),
        json!({"name": "add_image", "description": "Adds an image (PNG, JPEG, GIF, WebP or SVG, 15 MiB at most) from a file (`source`, under the allowed root) or from base64 data (`data_base64`). The natural size is reduced to at most 600; w and/or h override it.",
            "inputSchema": obj(pos_props(json!({"source": {"type": "string", "description": "Path of the image file."}, "data_base64": {"type": "string"}, "w": {"type": "number"}, "h": {"type": "number"}})), &[])}),
        json!({"name": "add_shape", "description": "Adds a shape with text (rect, ellipse, diamond, triangle, sticky = sticky note, text = free text). Returns the id.",
            "inputSchema": obj(pos_props(json!({
                "geom": {"type": "string", "enum": ["rect", "ellipse", "diamond", "triangle", "sticky", "text"]},
                "text": {"type": "string"}, "markdown": richtext::markdown_prop("`text`"), "w": {"type": "number"}, "h": {"type": "number"},
                "font_size": {"type": "number", "minimum": 4, "maximum": 400}, "fill": color.clone(), "stroke": color.clone(), "shadow": shadow_prop()})), &["geom"])}),
        json!({"name": "add_connector", "description": "Connects two objects with an arrow that follows them when they move (ids returned by add_shape or describe_board).",
            "inputSchema": obj(json!({"path": path_prop(), "page": page_prop(), "from": {"type": "string"}, "to": {"type": "string"},
                "route": {"type": "string", "enum": ["straight", "elbow", "curve"]},
                "arrow": {"type": "string", "enum": ["end", "start", "both", "none"], "description": "Arrowheads (default: end)."},
                "head_start": head_prop("start"), "head_end": head_prop("end"),
                "head_scale": head_scale_prop(),
                "label": {"type": "string"}, "markdown": richtext::markdown_prop("`label`"), "stroke": color.clone()}), &["from", "to"])}),
        json!({"name": "add_table", "description": "Adds a table of cells; `rows` is a list of rows (lists of texts). The first row is the header.",
            "inputSchema": obj(pos_props(json!({"rows": {"type": "array", "items": {"type": "array", "items": {"type": "string"}}},
                "markdown": richtext::markdown_prop("every cell")})), &["rows"])}),
        json!({"name": "add_code_block", "description": "Adds a code block (monospace font, dark background, syntax highlighting).",
            "inputSchema": obj(pos_props(json!({"code": {"type": "string"}})), &["code"])}),
        json!({"name": "add_formula", "description": "Adds a LaTeX formula (math mode) typeset the way TeX does. Errors if the LaTeX is not supported.",
            "inputSchema": obj(pos_props(json!({"latex": {"type": "string"}, "color": color.clone()})), &["latex"])}),
        json!({"name": "import_mermaid", "description": "Adds a Mermaid diagram (flowchart/graph, stateDiagram, sequenceDiagram, classDiagram, erDiagram) with automatic layout. Returns the ids created.",
            "inputSchema": obj(pos_props(json!({"mermaid": {"type": "string"}, "new_page": {"type": "boolean", "description": "Create a new page for the diagram."}})), &["mermaid"])}),
        json!({"name": "update_object", "description": "Modifies an object: text, position (top-left corner), size (shapes), colors, line style, font, lock, rotation; a connector's route, arrowheads and ends; one table cell. \
            Everything is validated before anything is applied (an error changes nothing); a parameter that does not suit the object's type is ignored and listed in `avertissements`. Locked objects can still be modified here.",
            "inputSchema": obj(json!({"path": path_prop(), "page": page_prop(), "id": {"type": "string"}, "text": {"type": "string", "description": "New text of a shape or connector label; with `cell`, the new text of that table cell."},
                "markdown": richtext::markdown_prop("`text`"), "x": {"type": "number"}, "y": {"type": "number"}, "w": {"type": "number"}, "h": {"type": "number"},
                "fill": color.clone(), "stroke": color.clone(),
                "stroke_width": {"type": "number", "minimum": 0, "maximum": 100, "description": "Outline width (0: no outline)."},
                "dash": {"type": "string", "enum": ["solid", "dashed", "dotted"]},
                "opacity": {"type": "number", "minimum": 0, "maximum": 1}, "roundness": {"type": "number", "minimum": 0, "maximum": 1, "description": "Corner rounding (0 sharp, 1 capsule)."},
                "shadow": shadow_prop(),
                "font_size": {"type": "number", "minimum": 4, "maximum": 400}, "font_family": {"type": "string", "enum": ["sans", "hand", "serif", "mono"]},
                "align": {"type": "string", "enum": ["left", "center", "right"], "description": "Text alignment (shapes and connector labels)."},
                "locked": {"type": "boolean", "description": "Lock or unlock the object in the application."},
                "rotation": {"type": "number", "minimum": -360, "maximum": 360, "description": "Absolute angle in degrees (shapes and images; 0 resets)."},
                "route": {"type": "string", "enum": ["straight", "elbow", "curve"], "description": "Connector only."},
                "arrow": {"type": "string", "enum": ["end", "start", "both", "none"], "description": "Connector only: arrowheads."},
                "head_start": head_prop("start"), "head_end": head_prop("end"),
                "head_scale": head_scale_prop(),
                "from": {"type": "string", "description": "Connector only: id of the object to connect the start to (shape, table or image)."},
                "to": {"type": "string", "description": "Connector only: id of the object to connect the end to."},
                "cell": {"type": "array", "items": {"type": "integer", "minimum": 0}, "minItems": 2, "maxItems": 2, "description": "Table only: [row, column] (from 0) of the cell whose text `text` replaces."},
                "recolor": {"type": ["object", "array"], "description": "Imported SVG image only (not formulas or monochrome icons): replaces colors of the image. The `colors` of the object (describe_board with detail \"full\") lists the colors found in the file. Give an object {\"#original\": \"#new\"} (e.g. {\"#ff0000\": \"#0000ff\"}) or an array of [\"#original\", \"#new\"] pairs; originals are always the colors of the file, so a swap {\"#ff0000\": \"#0000ff\", \"#0000ff\": \"#ff0000\"} works. This replaces the whole previous recoloration: `{}` restores the original colors. To make an image single-colored, map every color of `colors` to the same one. Max 256 pairs; an invalid color is an error and changes nothing.",
                    "additionalProperties": {"type": "string"}, "items": {"type": "array", "items": {"type": "string"}, "minItems": 2, "maxItems": 2}}}), &["id"])}),
        json!({"name": "delete_object", "description": "Deletes an object, or several with `ids` (connectors that were bound to them remain, detached). Give `id` or `ids`.",
            "inputSchema": obj(json!({"path": path_prop(), "page": page_prop(), "id": {"type": "string"},
                "ids": {"type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": 2000, "description": "Several objects to delete at once (result: `deleted` is then a list)."}}), &[])}),
        json!({"name": "auto_layout", "description": "Rearranges the shapes joined by connectors into a layered graph.",
            "inputSchema": obj(json!({"path": path_prop(), "page": page_prop(), "direction": {"type": "string", "enum": ["TB", "LR", "BT", "RL"], "description": "Default: TB."}}), &[])}),
        json!({"name": "add_page", "description": "Adds a page. Returns its number.",
            "inputSchema": obj(json!({"path": path_prop(), "name": {"type": "string"}}), &[])}),
        json!({"name": "export", "description": "Exports a page to a file; the format is chosen by the extension of `output`: .svg, .png, .pdf, .drawio, .excalidraw, .canvas, .inkboard. Missing folders of `output` are created (under the allowed root). For a PNG, `width` fixes the width in pixels (the same width for every diagram, whatever its size) or `scale` the pixels per board unit (default 2).",
            "inputSchema": obj(json!({"path": path_prop(), "output": {"type": "string"}, "page": page_prop(),
                "width": {"type": "integer", "minimum": PNG_WIDTH_MIN, "maximum": PNG_WIDTH_MAX, "description": "PNG only: width of the image in pixels."},
                "scale": {"type": "number", "minimum": PNG_SCALE_MIN, "maximum": PNG_SCALE_MAX, "description": "PNG only: pixels per board unit. Default 2. Not with `width`."}}), &["output"])}),
    ];
    tools.extend(ext_specs());
    tools
}

/* ------------------------------------------------- tools of the submodules */

fn ext_specs() -> Vec<Value> {
    [icons::specs(), render::specs(), edit::specs(), batch::specs(), richtext::specs()].concat()
}

fn ext_is_tool(name: &str) -> bool {
    [icons::NAMES, render::NAMES, edit::NAMES, batch::NAMES, richtext::NAMES].iter().any(|l| l.contains(&name))
}

fn ext_is_read_only(name: &str) -> bool {
    [icons::READ_ONLY, render::READ_ONLY, edit::READ_ONLY, batch::READ_ONLY, richtext::READ_ONLY].iter().any(|l| l.contains(&name))
}

fn ext_apply(ed: &mut Editor, name: &str, a: &Value) -> Option<Res<Value>> {
    icons::apply(ed, name, a)
        .or_else(|| render::apply(ed, name, a))
        .or_else(|| edit::apply(ed, name, a))
        .or_else(|| batch::apply(ed, name, a))
        .or_else(|| richtext::apply(ed, name, a))
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

/// Like [`resolve`] for a file about to be written: the folders missing above it are created, once it is known that the file stays under
/// the root (the deepest existing folder is resolved, so a link or `..` cannot lead outside; `..` is refused in the part to create).
fn resolve_new(srv: &Server, p: &str) -> Res<PathBuf> {
    let path = PathBuf::from(p);
    if let Some(root) = &srv.root {
        let root = root.canonicalize().map_err(|e| format!("root not accessible: {e}"))?;
        let abs = if path.is_absolute() { path } else { root.join(path) };
        if let Some(parent) = abs.parent() {
            let existing = parent.ancestors().find(|a| a.exists()).ok_or("invalid path")?;
            let real = existing.canonicalize().map_err(|e| format!("folder not accessible ({}): {e}", existing.display()))?;
            if !real.starts_with(&root) {
                return Err(format!("path outside the allowed root ({})", root.display()));
            }
            let rest = parent.strip_prefix(existing).map_err(|_| "invalid path")?;
            if rest.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
                return Err("invalid path".into());
            }
            let wanted = real.join(rest);
            std::fs::create_dir_all(&wanted).map_err(|e| format!("creating folder {}: {e}", wanted.display()))?;
        }
        return resolve(srv, &abs.display().to_string());
    }
    if let Some(parent) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| format!("creating folder {}: {e}", parent.display()))?;
    }
    resolve(srv, p)
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
        // a name first; a number written as text ("2") is the page number, as models often send it
        Value::String(name) => {
            pages.iter().find(|p| &p.name == name).or_else(|| name.trim().parse::<usize>().ok().and_then(|n| n.checked_sub(1)).and_then(|i| pages.get(i)))
        }
        _ => None,
    };
    let id = found.ok_or_else(|| format!("page not found ({spec}); the board has {} page(s)", pages.len()))?.id.clone();
    ed.current = id;
    Ok(())
}

fn origin(ed: &Editor, a: &Value) -> Pt {
    let at = match (of(a, "x"), of(a, "y")) {
        (Some(x), Some(y)) => Pt::new(x, y),
        _ => ed.page().content_bounds().map_or(Pt::new(0.0, 0.0), |b| Pt::new(of(a, "x").unwrap_or(b.x), of(a, "y").unwrap_or(b.bottom() + 40.0))),
    };
    // "below the existing content" can run past the largest position a shared session relays
    Pt::new(at.x.clamp(-MAX_ARG_ABS, MAX_ARG_ABS), at.y.clamp(-MAX_ARG_ABS, MAX_ARG_ABS))
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

/// Bounds on what an agent may ask for. They are the limits a shared session applies to every object it relays (positions, sizes, font
/// and line widths), so that nothing the agent adds can make the guests refuse the board; text is bounded in total so that a call stays
/// quick whatever it carries.
const MAX_ARG_ABS: f64 = 1e7;
const MAX_ARG_SIZE: f64 = 1e6;
const MAX_ARG_FONT: f64 = 2_000.0;
const MAX_ARG_STROKE: f64 = 1_000.0;
const MAX_ARG_CHARS: usize = 200_000;
const MAX_ARG_DEPTH: usize = 32;
const MAX_ARG_NAME: usize = 200;
/// Arguments that carry an encoded file (their own, larger limit is checked where they are decoded).
const BULK_KEYS: [&str; 1] = ["data_base64"];

/// Refuses numbers that are out of range, text that is too long in total, and nesting that is too deep, anywhere in the arguments.
fn check_args(a: &Value) -> Res<()> {
    fn walk(v: &Value, key: &str, depth: usize, chars: &mut usize) -> Res<()> {
        if depth > MAX_ARG_DEPTH {
            return Err("arguments nested too deeply".into());
        }
        match v {
            Value::Number(n) => {
                let x = n.as_f64().unwrap_or(f64::INFINITY);
                let (lo, hi, what) = match key {
                    // a negative size is for each tool (or the check after it) to refuse, with its own message
                    "w" | "h" => (-MAX_ARG_SIZE, MAX_ARG_SIZE, "a size within 1,000,000"),
                    "font_size" => (0.0, MAX_ARG_FONT, "a font size between 0 and 2,000"),
                    "stroke_width" => (0.0, MAX_ARG_STROKE, "a line width between 0 and 1,000"),
                    _ => (-MAX_ARG_ABS, MAX_ARG_ABS, "a number between -10,000,000 and 10,000,000"),
                };
                if !x.is_finite() || x < lo || x > hi {
                    return Err(format!("\"{key}\" is out of range ({n}): expected {what}"));
                }
            }
            Value::String(t) if !BULK_KEYS.contains(&key) => {
                let n = t.chars().count();
                // names of boards and pages (a shared session refuses longer ones)
                if key == "name" && n > MAX_ARG_NAME {
                    return Err(format!("\"name\" is too long ({n} characters, at most {MAX_ARG_NAME})"));
                }
                *chars += n;
                if *chars > MAX_ARG_CHARS {
                    return Err(format!("the text of this call is too long (more than {MAX_ARG_CHARS} characters in all)"));
                }
            }
            Value::Array(items) => items.iter().try_for_each(|x| walk(x, key, depth + 1, chars))?,
            Value::Object(m) => m.iter().try_for_each(|(k, x)| walk(x, k, depth + 1, chars))?,
            _ => {}
        }
        Ok(())
    }
    walk(a, "", 0, &mut 0)
}

/// Applies a tool to an in-memory board. Used as is by file mode and by the application (live mode, with its undo history).
///
/// # Errors
///
/// A message for the agent when the tool is unknown or its arguments are invalid; the board is then unchanged.
pub fn apply(ed: &mut Editor, name: &str, a: &Value) -> Res<Value> {
    if name == "save_board" {
        return Ok(json!({"saved": true}));
    }
    if !EDITOR_TOOLS.contains(&name) && !ext_is_tool(name) {
        return Err(format!("unknown tool: {name}"));
    }
    check_args(a)?;
    if name == "describe_board" || ext_is_read_only(name) || READ_ONLY.contains(&name) {
        return apply_checked(ed, name, a);
    }
    // a tool that moves things (layout, offsets, "below the content") must not push any of them out of the area a shared session relays
    ed.try_transact(|ed| {
        let r = apply_checked(ed, name, a)?;
        let outside = ed
            .doc
            .pages
            .iter()
            .flat_map(|p| p.objects.iter())
            .filter_map(Object::frame)
            .any(|f| ![f.x, f.y].iter().all(|v| v.abs() <= MAX_ARG_ABS) || ![f.w, f.h].iter().all(|v| (0.0..=MAX_ARG_SIZE).contains(v)));
        if outside {
            return Err(format!(
                "the result would put objects outside the usable area (positions within {MAX_ARG_ABS}, sizes within {MAX_ARG_SIZE}): nothing was changed"
            ));
        }
        Ok(r)
    })
}

fn apply_checked(ed: &mut Editor, name: &str, a: &Value) -> Res<Value> {
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
///
/// # Errors
///
/// A message for the agent when the tool fails (see [`apply`]) or the board file cannot be read or written.
pub fn call(srv: &Server, name: &str, a: &Value) -> Res<String> {
    if name != "create_board" && name != "save_board" && !EDITOR_TOOLS.contains(&name) && !ext_is_tool(name) {
        return Err(format!("unknown tool: {name}"));
    }
    let mut a = a.clone();
    // paths are resolved (and confined to the root) here, never in the application
    for key in ["path", "output"] {
        if let Some(p) = os(&a, key) {
            let abs = if key == "output" && (name == "export" || name == "render") { resolve_new(srv, p)? } else { resolve(srv, p)? };
            a[key] = json!(abs.display().to_string());
        }
    }
    if name == "add_image"
        && let Some(src) = os(&a, "source").map(str::to_owned)
    {
        // the file is read here (confined to the root); only the data is passed to the application
        let abs = resolve(srv, &src)?;
        let meta = std::fs::metadata(&abs).map_err(|e| format!("reading {}: {e}", abs.display()))?;
        if usize::try_from(meta.len()).map_or(true, |n| n > IMAGE_MAX_BYTES) {
            return Err(format!("image too large ({} MiB, maximum {})", meta.len() >> 20, IMAGE_MAX_BYTES >> 20));
        }
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
    let mut m = serde_json::Map::new();
    m.insert("id".into(), json!(o.id.0));
    m.insert("type".into(), json!(kind_name(o)));
    if let Some(f) = o.frame() {
        m.insert("x".into(), json!(f.x.round()));
        m.insert("y".into(), json!(f.y.round()));
        m.insert("w".into(), json!(f.w.round()));
        m.insert("h".into(), json!(f.h.round()));
    }
    match &o.kind {
        Kind::Box(b) => {
            m.insert("text".into(), json!(b.label.text.to_plain()));
            if richtext::has_formatting(&b.label.text) {
                m.insert("markdown".into(), json!(richtext::to_markdown(&b.label.text)));
            }
        }
        Kind::Connector(c) => {
            m.insert("from".into(), end_json(&c.start));
            m.insert("to".into(), end_json(&c.end));
            if let Some(l) = &c.label {
                m.insert("label".into(), json!(l.text.to_plain()));
                if richtext::has_formatting(&l.text) {
                    m.insert("markdown".into(), json!(richtext::to_markdown(&l.text)));
                }
            }
        }
        Kind::Table(t) => {
            m.insert("rows".into(), json!(t.rows()));
            m.insert("cols".into(), json!(t.cols()));
            if t.cells.iter().any(richtext::has_formatting) {
                let md: Vec<Vec<String>> = t.cells.chunks(t.cols().max(1)).map(|row| row.iter().map(richtext::to_markdown).collect()).collect();
                m.insert("cells_markdown".into(), json!(md));
            }
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
            if !i.recolor.is_empty() {
                m.insert("recolor".into(), Value::Object(i.recolor.iter().map(|(f, t)| (f.to_hex(), json!(t.to_hex()))).collect()));
            }
        }
        Kind::Pen(_) => {}
    }
    Value::Object(m)
}

/// `summary` of an object, plus `colors` (the colors found in the file) for an SVG image that can be recolored.
#[must_use]
pub fn summary_in(doc: &Document, o: &Object) -> Value {
    let mut v = summary(o);
    if let Kind::Image(i) = &o.kind
        && !i.is_tinted()
        && let Some(a) = doc.assets.get(&i.asset).filter(|a| a.mime == "image/svg+xml")
    {
        let colors: Vec<String> = inkboard_core::svgcolor::svg_colors(&a.data).into_iter().map(|(r, g, b)| Color::rgb(r, g, b).to_hex()).collect();
        v["colors"] = json!(colors);
    }
    v
}

fn describe(ed: &mut Editor, a: &Value) -> Res<Value> {
    let page = ed.page();
    let objects: Vec<Value> = match os(a, "detail").unwrap_or("brief") {
        "brief" => page.objects.iter().map(summary).collect(),
        "full" => page.objects.iter().enumerate().map(|(z, o)| edit::full(&ed.doc, z, o)).collect(),
        d => return Err(format!("unknown detail \"{d}\" (brief, full)")),
    };
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
    #[must_use]
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
        reference.pages.clone_from(&after.pages);
    }
}

/// Like [`apply`], while tracking changes: `board_changes` reports what the user modified since the agent's last look
/// (a `describe_board` or `board_changes` call); the agent's own changes are not reported.
///
/// # Errors
///
/// Same as [`apply`].
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
    let img = ImageObj { frame: Frame::new(at.x, at.y, w, h), asset: aid, latex: None, mono: false, recolor: Vec::new() };
    let id = ed.insert(Object::new(Kind::Image(img), Style { width: 0.0, fill: None, ..Style::default() }));
    Ok(json!({"id": id.0, "w": w.round(), "h": h.round(), "type": mime}))
}

fn add_shape(ed: &mut Editor, a: &Value) -> Res<Value> {
    let geom = geom_of(s(a, "geom")?)?;
    let at = origin(ed, a);
    let text = os(a, "text").unwrap_or("");
    let markdown = richtext::markdown_flag(a)?;
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
        shadow: edit::bool_arg(a, "shadow")?.unwrap_or(false),
        ..Style::default()
    };
    // as `update_object`: a size of 0 or a huge one would make the guests of a shared board refuse it
    let size = edit::num_arg(a, "font_size", 4.0, 400.0)?.unwrap_or(20.0);
    let mut label = Label::new(text, size, if geom == BoxGeom::Text { Align::Left } else { Align::Center });
    let mut warnings = Vec::new();
    label.text = richtext::rich(text, markdown, &mut warnings);
    let mut o = Object::new_box(geom, Frame::new(at.x, at.y, of(a, "w").unwrap_or(dw), of(a, "h").unwrap_or(dh)), style, label);
    if let Kind::Box(b) = &mut o.kind {
        b.fit(&ApproxMeasurer);
    }
    let id = ed.insert(o);
    let f = ed.page().get(&id).and_then(Object::frame).ok_or("object not created")?;
    let mut out = json!({"id": id.0, "x": f.x.round(), "y": f.y.round(), "w": f.w.round(), "h": f.h.round()});
    richtext::add_warnings(&mut out, warnings);
    Ok(out)
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
    // a head named for one end replaces what `arrow` gave it
    if let Some(h) = edit::head_arg(a, "head_start")? {
        h.apply(&mut c, false);
    }
    if let Some(h) = edit::head_arg(a, "head_end")? {
        h.apply(&mut c, true);
    }
    if let Some(v) = edit::head_scale_arg(a)? {
        c.set_head_scale(v);
    }
    let markdown = richtext::markdown_flag(a)?;
    let mut warnings = Vec::new();
    if let Some(l) = os(a, "label").filter(|l| !l.is_empty()) {
        let mut label = Label::new(l, 16.0, Align::Center);
        label.text = richtext::rich(l, markdown, &mut warnings);
        c.label = Some(label);
    }
    let style = Style { stroke: ocolor(a, "stroke")?.unwrap_or(Color::Ink), width: 2.0, ..Style::default() };
    let id = ed.insert(Object::new(Kind::Connector(c), style));
    let mut out = json!({"id": id.0});
    richtext::add_warnings(&mut out, warnings);
    Ok(out)
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
    let markdown = richtext::markdown_flag(a)?;
    let mut warnings = Vec::new();
    let texts: Vec<Vec<RichText>> = rows
        .iter()
        .enumerate()
        .map(|(r, row)| {
            row.iter()
                .enumerate()
                .map(|(c, text)| {
                    let mut w = Vec::new();
                    let rt = richtext::rich(text, markdown, &mut w);
                    warnings.extend(w.into_iter().map(|w| format!("cell [{r}, {c}]: {w}")));
                    rt
                })
                .collect()
        })
        .collect();
    let at = origin(ed, a);
    let mut t = Table::new(at, rows.len(), cols);
    t.header = true;
    for (r, row) in texts.iter().enumerate() {
        for (c, text) in row.iter().enumerate() {
            if let Some(cell) = t.cell_mut(r, c) {
                *cell = text.clone();
            }
        }
    }
    // column width from the longest text (at least the default width)
    for c in 0..cols {
        let widest = texts.iter().filter_map(|r| r.get(c)).map(|t| ApproxMeasurer.width(&t.to_plain(), FontFamily::Sans, 18.0, true)).fold(0.0, f64::max);
        // a column is at most 2 000 wide (the text wraps): a longer line would exceed what a shared session relays
        t.col_w[c] = t.col_w[c].max((widest + 28.0).ceil().min(2_000.0));
    }
    let id = ed.insert(Object::new(Kind::Table(t), Style { width: 2.0, fill: None, ..Style::default() }));
    let mut out = json!({"id": id.0, "rows": rows.len(), "cols": cols});
    richtext::add_warnings(&mut out, warnings);
    Ok(out)
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
    if latex.chars().count() > 5_000 {
        return Err("the formula is too long (5,000 characters at most)".into());
    }
    let f = inkboard_formats::math::render(latex, 36.0).map_err(|e| e.to_string())?;
    if !(f.w.is_finite() && f.h.is_finite() && f.w <= MAX_ARG_SIZE && f.h <= MAX_ARG_SIZE) {
        return Err("the formula is too large once typeset".into());
    }
    let aid = ed.doc.add_asset("image/svg+xml", f.svg.into_bytes());
    let at = origin(ed, a);
    let style = Style { stroke: ocolor(a, "color")?.unwrap_or(Color::Ink), width: 0.0, fill: None, ..Style::default() };
    let img = ImageObj { frame: Frame::new(at.x, at.y, f.w, f.h), asset: aid, latex: Some(latex.to_owned()), mono: false, recolor: Vec::new() };
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

fn update_object(ed: &mut Editor, args: &Value) -> Res<Value> {
    let id = find(ed, s(args, "id")?)?.id.clone();
    let (fill, stroke) = (ocolor(args, "fill")?, ocolor(args, "stroke")?);
    let (text, x, y, w, h) = (os(args, "text").map(str::to_owned), of(args, "x"), of(args, "y"), of(args, "w"), of(args, "h"));
    let mut warn: Vec<String> = Vec::new();
    let markdown = richtext::markdown_flag(args)?;
    // everything is validated here, before the first change
    let patch = edit::Patch::parse(ed, args, find(ed, &id.0)?)?;
    let mut md_warnings = Vec::new();
    // with `cell`, `text` is the text of that cell (parsed by the patch)
    let rich = text.as_deref().filter(|_| !patch.has_cell()).map(|t| richtext::rich(t, markdown, &mut md_warnings));
    md_warnings.extend(patch.warnings.iter().cloned());
    ed.update(&id, |o| {
        if let Some(f) = fill {
            o.style.fill = Some(f);
        }
        if let Some(s) = stroke {
            o.style.stroke = s;
        }
        match &mut o.kind {
            Kind::Box(shape) => {
                if let Some(t) = &rich {
                    shape.label.text = t.clone();
                }
                if let Some(w) = w {
                    shape.frame.w = w.max(4.0);
                }
                if let Some(h) = h {
                    shape.frame.h = h.max(4.0);
                }
                shape.fit(&ApproxMeasurer);
            }
            Kind::Connector(conn) => {
                if let Some(t) = &rich {
                    conn.label = (!t.is_empty()).then(|| Label { text: t.clone(), ..Label::new("", 16.0, Align::Center) });
                }
            }
            _ if text.is_some() && !patch.has_cell() => warn.push("text can only be changed on shapes and connectors".to_owned()),
            _ => {}
        }
        patch.apply(o, &mut warn);
        if x.is_some() || y.is_some() {
            if let Some(f) = o.frame().map(|f| f.aabb()).or_else(|| matches!(o.kind, Kind::Table(_)).then(|| table_rect(o))) {
                o.translate(Pt::new(x.map_or(0.0, |x| x - f.x), y.map_or(0.0, |y| y - f.y)));
            } else {
                warn.push("this object cannot be moved with x/y".to_owned());
            }
        }
    });
    let mut out = json!({"id": id.0, "avertissements": warn});
    richtext::add_warnings(&mut out, md_warnings);
    Ok(out)
}

fn table_rect(o: &Object) -> Rect {
    match &o.kind {
        Kind::Table(t) => t.rect(),
        _ => Rect::new(0.0, 0.0, 0.0, 0.0),
    }
}

const PNG_WIDTH_MIN: u32 = 16;
const PNG_WIDTH_MAX: u32 = 8000;
const PNG_SCALE_MIN: f64 = 0.1;
const PNG_SCALE_MAX: f64 = 8.0;

/// The size asked for a PNG export: a width in pixels, or a scale (default 2).
fn png_size(a: &Value) -> Res<(Option<u32>, f32)> {
    let width = match a.get("width").filter(|v| !v.is_null()) {
        None => None,
        Some(v) => Some(
            v.as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| (PNG_WIDTH_MIN..=PNG_WIDTH_MAX).contains(n))
                .ok_or_else(|| format!("\"width\" must be an integer between {PNG_WIDTH_MIN} and {PNG_WIDTH_MAX}"))?,
        ),
    };
    let scale = match a.get("scale").filter(|v| !v.is_null()) {
        None => 2.0,
        Some(_) if width.is_some() => return Err("give \"width\" or \"scale\", not both".into()),
        Some(v) => v
            .as_f64()
            .filter(|n| (PNG_SCALE_MIN..=PNG_SCALE_MAX).contains(n))
            .ok_or_else(|| format!("\"scale\" must be a number between {PNG_SCALE_MIN} and {PNG_SCALE_MAX}"))?,
    };
    Ok((width, inkboard_core::num::f32(scale)))
}

fn export(ed: &mut Editor, a: &Value) -> Res<Value> {
    let output = PathBuf::from(s(a, "output")?);
    let name = lower_name(&output);
    if !EXPORTS.iter().any(|e| name.ends_with(e)) {
        return Err(format!("unsupported output extension (expected: {})", EXPORTS.join(", ")));
    }
    let ext = output.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if ext != "png" && (a.get("width").is_some_and(|v| !v.is_null()) || a.get("scale").is_some_and(|v| !v.is_null())) {
        return Err("\"width\" and \"scale\" only apply to a .png output".into());
    }
    let doc = &ed.doc;
    let page = ed.page();
    let svg = || export_svg(doc, page, None, &ApproxMeasurer, &SvgOptions::default()).ok_or_else(|| "the page is empty: nothing to export".to_owned());
    let bytes: Vec<u8> = if ext == "svg" {
        svg()?.into_bytes()
    } else if ext == "png" {
        let (width, scale) = png_size(a)?;
        let svg = svg()?;
        match width {
            Some(w) => inkboard_formats::raster::svg_to_png_width(&svg, w),
            None => inkboard_formats::raster::svg_to_png(&svg, scale),
        }
        .map_err(|e| e.to_string())?
    } else if ext == "pdf" {
        inkboard_formats::raster::svg_to_pdf(&svg()?).map_err(|e| e.to_string())?
    } else if ext == "drawio" {
        inkboard_formats::drawio::export(doc, &ApproxMeasurer).into_bytes()
    } else if ext == "excalidraw" {
        inkboard_formats::excalidraw::export(doc, page, &ApproxMeasurer).into_bytes()
    } else if ext == "canvas" {
        inkboard_formats::jsoncanvas::export(page).0.into_bytes()
    } else {
        inkboard_core::serial::to_json(doc, true).into_bytes()
    };
    std::fs::write(&output, &bytes).map_err(|e| format!("writing {}: {e}", output.display()))?;
    Ok(json!({"output": output.display().to_string(), "bytes": bytes.len()}))
}
