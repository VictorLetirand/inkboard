//! Icon tools (`search_icons`, `list_icon_groups`, `add_icon`): the library of the application (`inkboard-icons`, pack shared with the
//! application) made reachable to an agent. See `context/PLAN_MCP_V2.md`, lot 1.
//!
//! Icon id: `<set id>/<icon name>`; the n-th icon of a set that repeats a name (n >= 2) gets the suffix `#n`. Set ids never contain `/`
//! (checked by a test on the real pack), names may (`shadcn/ui`) or contain `#` (`F#`), so the id is split at the first `/` only.

use super::{Res, obj, ocolor, origin, os, path_prop, pos_props, s};
use inkboard_core::*;
use inkboard_icons::{IconRef, Library};
use serde_json::{Value, json};

/// Tools exposed by this module.
pub(super) const NAMES: &[&str] = &["search_icons", "list_icon_groups", "add_icon"];
/// Those that do not modify the board.
pub(super) const READ_ONLY: &[&str] = &["search_icons", "list_icon_groups"];

const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 100;
/// Longest side of a placed icon when no size is given.
const DEFAULT_SIZE: f64 = 64.0;
const MIN_SIDE: f64 = 8.0;
const MAX_SIDE: f64 = 2000.0;
const MAX_SUGGESTIONS: usize = 5;
/// Bounds of the free text typed by the agent (keeps a huge input from making the search slow).
const MAX_QUERY: usize = 200;
const MAX_ICON_ID: usize = 300;
const MAX_LABEL: usize = 500;
const LABEL_SIZE: f64 = 16.0;
/// Space between an icon and its label.
const LABEL_GAP: f64 = 6.0;

/// Schemas for `tools/list`.
pub(super) fn specs() -> Vec<Value> {
    let color = json!({"type": "string", "description": "Color \"#rrggbb\" or \"ink\" (the theme's text color)."});
    vec![
        json!({"name": "search_icons", "description": "Searches the built-in icon library (about 28,800 icons: AWS, Azure, Google Cloud, Cisco, Kubernetes, brand logos, UI and diagram symbols...) by keywords; names are in English. Every word of `query` must match (case-insensitive) the icon name, its set or its group. Returns ids to give to add_icon. Use list_icon_groups to see what the library contains, and `group` / `set` to narrow a broad search. Does not modify the board (a board `path` is still needed when the application is not open).",
            "inputSchema": obj(json!({"path": path_prop(),
                "query": {"type": "string", "description": "Keywords, e.g. \"lambda\", \"router\", \"github\", \"database server\"."},
                "group": {"type": "string", "description": "Only this group, e.g. \"AWS\", \"Brands\", \"General icons\" (list_icon_groups)."},
                "set": {"type": "string", "description": "Only this set (exact title or id, see list_icon_groups)."},
                "limit": {"type": "integer", "minimum": 1, "maximum": MAX_LIMIT, "description": "Maximum number of icons returned (default 20, at most 100); `truncated` tells when more match."}}), &["query"])}),
        json!({"name": "list_icon_groups", "description": "Lists the groups of the icon library (AWS, Azure, Brands...) with their sets and the number of icons of each, to know what to search for. Does not modify the board (a board `path` is still needed when the application is not open).",
            "inputSchema": obj(json!({"path": path_prop()}), &[])}),
        json!({"name": "add_icon", "description": "Places an icon of the library on the board, as an image object that can be connected with add_connector. `icon` is an id returned by search_icons. Size: `size` = longest side (default 64, 8 to 2000), or `w` and/or `h` (the icon's ratio is kept when only one is given). Monochrome icons (mono: true in search_icons) take the color `stroke` (default: the theme's text color); colored icons keep their own colors. `label` adds a text centered under the icon, grouped with it. Returns the image `id` (and `label_id`).",
            "inputSchema": obj(pos_props(json!({
                "icon": {"type": "string", "description": "Icon id from search_icons, e.g. \"drawio-aws4/Lambda\"."},
                "size": {"type": "number", "minimum": MIN_SIDE, "maximum": MAX_SIDE, "description": "Longest side (default 64). Not combinable with w / h."},
                "w": {"type": "number", "minimum": MIN_SIDE, "maximum": MAX_SIDE}, "h": {"type": "number", "minimum": MIN_SIDE, "maximum": MAX_SIDE},
                "stroke": color, "label": {"type": "string", "description": "Text placed under the icon (at most 500 characters)."}})), &["icon"])}),
    ]
}

/// Runs the tool when this module owns it, `None` otherwise.
pub(super) fn apply(ed: &mut Editor, name: &str, a: &Value) -> Option<Res<Value>> {
    Some(match name {
        "search_icons" => search_icons(a),
        "list_icon_groups" => list_icon_groups(),
        "add_icon" => add_icon(ed, a),
        _ => return None,
    })
}

fn library() -> Res<&'static Library> {
    inkboard_icons::shared().ok_or_else(|| "icon library unavailable (the embedded icon pack is unreadable)".to_owned())
}

/// Id of an icon: `<set id>/<name>`, with `#n` for the n-th (n >= 2) icon of the set that has this name.
fn icon_id(lib: &Library, r: IconRef) -> String {
    let set = &lib.sets[r.set];
    let name = &set.names[r.icon];
    match set.names[..r.icon].iter().filter(|n| *n == name).count() {
        0 => format!("{}/{name}", set.id),
        before => format!("{}/{name}#{}", set.id, before + 1),
    }
}

/// The icon an id designates (the exact inverse of [`icon_id`]).
fn find_icon(lib: &Library, id: &str) -> Option<IconRef> {
    let (set_id, rest) = id.split_once('/')?;
    let set = lib.sets.iter().position(|s| s.id == set_id)?;
    let names = &lib.sets[set].names;
    if let Some(icon) = names.iter().position(|n| n == rest) {
        return Some(IconRef { set, icon });
    }
    let (name, n) = rest.rsplit_once('#')?;
    let n: usize = n.parse().ok().filter(|n| *n >= 2)?;
    let icon = names.iter().enumerate().filter(|(_, x)| *x == name).nth(n - 1)?.0;
    Some(IconRef { set, icon })
}

/// Error for an unknown icon id, with up to [`MAX_SUGGESTIONS`] icons whose name looks like it.
fn not_found(lib: &Library, icon: &str) -> String {
    let text = icon.split_once('/').map_or(icon, |(_, name)| name);
    let text = text.rsplit_once('#').map_or(text, |(name, _)| name);
    let near: Vec<String> = lib.search(text, MAX_SUGGESTIONS).into_iter().map(|r| icon_id(lib, r)).collect();
    if near.is_empty() {
        format!("icon not found: \"{icon}\" ; use search_icons to find an icon id")
    } else {
        format!("icon not found: \"{icon}\" ; did you mean: {} ? (ids come from search_icons)", near.join(", "))
    }
}

fn icon_json(lib: &Library, r: IconRef) -> Value {
    let set = &lib.sets[r.set];
    json!({"id": icon_id(lib, r), "name": set.names[r.icon], "set": set.title, "group": set.group, "mono": set.mono})
}

/// Optional text parameter, an empty one counting as absent.
fn filter_text<'a>(a: &'a Value, k: &str) -> Option<&'a str> {
    os(a, k).map(str::trim).filter(|t| !t.is_empty())
}

/// Group named `name` (case-insensitive), as written in the library.
fn find_group<'a>(lib: &'a Library, name: &str) -> Res<&'a str> {
    let low = name.to_lowercase();
    let groups = lib.groups();
    groups.iter().find(|g| g.to_lowercase() == low).copied().ok_or_else(|| format!("unknown group \"{name}\" ; groups: {}", groups.join(", ")))
}

/// Index of the set whose title or id is `name` (case-insensitive).
fn find_set(lib: &Library, name: &str) -> Res<usize> {
    let low = name.to_lowercase();
    let hit = |s: &inkboard_icons::Set, exact: bool| {
        let (t, i) = (s.title.to_lowercase(), s.id.to_lowercase());
        if exact { t == low || i == low } else { t.contains(&low) || i.contains(&low) }
    };
    if let Some(i) = lib.sets.iter().position(|s| hit(s, true)) {
        return Ok(i);
    }
    let near: Vec<String> = lib.sets.iter().filter(|s| hit(s, false)).take(MAX_SUGGESTIONS).map(|s| format!("\"{}\"", s.title)).collect();
    if near.is_empty() {
        Err(format!("unknown set \"{name}\" ; list_icon_groups gives the sets"))
    } else {
        Err(format!("unknown set \"{name}\" ; did you mean: {} ?", near.join(", ")))
    }
}

/// Number parameter within `min..=max`; `None` when absent.
fn num(a: &Value, k: &str, min: f64, max: f64) -> Res<Option<f64>> {
    match a.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_f64().filter(|n| (min..=max).contains(n)).map(Some).ok_or_else(|| format!("\"{k}\" must be a number between {min} and {max}")),
    }
}

fn search_icons(a: &Value) -> Res<Value> {
    let lib = library()?;
    let query = s(a, "query")?.trim();
    if query.is_empty() {
        return Err("query is empty: give keywords such as \"lambda\", \"router\" or \"github\" (list_icon_groups shows what the library contains)".into());
    }
    if query.chars().count() > MAX_QUERY {
        return Err(format!("query too long (at most {MAX_QUERY} characters)"));
    }
    let limit = match a.get("limit") {
        None | Some(Value::Null) => DEFAULT_LIMIT,
        Some(v) => v
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| (1..=MAX_LIMIT).contains(n))
            .ok_or_else(|| format!("\"limit\" must be an integer between 1 and {MAX_LIMIT}"))?,
    };
    let group = filter_text(a, "group").map(|g| find_group(lib, g)).transpose()?;
    let set = filter_text(a, "set").map(|n| find_set(lib, n)).transpose()?;
    let hits: Vec<IconRef> = lib
        .search(query, usize::MAX)
        .into_iter()
        .filter(|r| set.is_none_or(|si| r.set == si) && group.is_none_or(|g| lib.sets[r.set].group == g))
        .take(limit + 1)
        .collect();
    let icons: Vec<Value> = hits.iter().take(limit).map(|r| icon_json(lib, *r)).collect();
    Ok(json!({"count": icons.len(), "truncated": hits.len() > limit, "icons": icons}))
}

fn list_icon_groups() -> Res<Value> {
    let lib = library()?;
    let groups: Vec<Value> = lib
        .groups()
        .into_iter()
        .map(|g| {
            let sets: Vec<Value> = lib.sets.iter().filter(|s| s.group == g).map(|s| json!({"set": s.title, "icons": s.names.len()})).collect();
            json!({"group": g, "sets": sets})
        })
        .collect();
    Ok(json!({"total": lib.icon_count(), "groups": groups}))
}

fn add_icon(ed: &mut Editor, a: &Value) -> Res<Value> {
    let lib = library()?;
    let icon = s(a, "icon")?;
    if icon.chars().count() > MAX_ICON_ID {
        return Err(format!("icon id too long (at most {MAX_ICON_ID} characters); use an id returned by search_icons"));
    }
    let r = find_icon(lib, icon).ok_or_else(|| not_found(lib, icon))?;
    let set = &lib.sets[r.set];
    let svg = lib.svg(r).ok_or("icon data unreadable in the library")?;
    let (nw, nh) = inkboard_icons::svg_dimensions(&svg).unwrap_or((24.0, 24.0));

    // everything is validated before the board is touched
    let (size, w, h) = (num(a, "size", MIN_SIDE, MAX_SIDE)?, num(a, "w", MIN_SIDE, MAX_SIDE)?, num(a, "h", MIN_SIDE, MAX_SIDE)?);
    if size.is_some() && (w.is_some() || h.is_some()) {
        return Err("give either \"size\" (longest side) or \"w\" / \"h\", not both".into());
    }
    let (w, h) = match (size, w, h) {
        (_, Some(w), Some(h)) => (w, h),
        (_, Some(w), None) => (w, w * nh / nw),
        (_, None, Some(h)) => (h * nw / nh, h),
        (size, None, None) => {
            let k = size.unwrap_or(DEFAULT_SIZE) / nw.max(nh);
            (nw * k, nh * k)
        }
    };
    if !(1.0..=20_000.0).contains(&w) || !(1.0..=20_000.0).contains(&h) {
        return Err(format!("this icon's proportions would give a {} x {} frame; give both \"w\" and \"h\"", w.round(), h.round()));
    }
    let stroke = ocolor(a, "stroke")?;
    let label = os(a, "label").filter(|l| !l.is_empty());
    if label.is_some_and(|l| l.chars().count() > MAX_LABEL) {
        return Err(format!("label too long (at most {MAX_LABEL} characters)"));
    }
    let mut warnings = Vec::new();
    if stroke.is_some() && !set.mono {
        warnings.push("stroke ignored: this icon is in color and keeps its own colors (only monochrome icons are tinted)");
    }

    // same as the palette of the application: SVG asset (deduplicated), no outline, no fill, tint by the stroke color when monochrome
    let at = origin(ed, a);
    let frame = Frame::new(at.x, at.y, w, h);
    let aid = ed.doc.add_asset("image/svg+xml", svg.into_bytes());
    let mut style = Style { width: 0.0, fill: None, ..Style::default() };
    if set.mono
        && let Some(c) = stroke
    {
        style.stroke = c;
    }
    let (id, label_id) = ed.transact(|ed| {
        let id = ed.insert(Object::new(Kind::Image(ImageObj { frame, asset: aid, latex: None, mono: set.mono }), style));
        let label_id = label.map(|text| {
            let text_style = Style { stroke: Color::Ink, fill: None, width: 0.0, roundness: 0.0, ..Style::default() };
            let mut o = Object::new_box(BoxGeom::Text, Frame::new(0.0, 0.0, 10.0, 10.0), text_style, Label::new(text, LABEL_SIZE, Align::Center));
            if let Kind::Box(b) = &mut o.kind {
                b.fit(&ApproxMeasurer);
                b.frame.x = frame.x + (frame.w - b.frame.w) / 2.0;
                b.frame.y = frame.y + frame.h + LABEL_GAP;
            }
            let label_id = ed.insert(o);
            ed.group(&[id.clone(), label_id.clone()]);
            label_id
        });
        (id, label_id)
    });
    let mut out = json!({"id": id.0, "x": frame.x.round(), "y": frame.y.round(), "w": w.round(), "h": h.round(), "icon": icon_id(lib, r)});
    if let Some(l) = label_id {
        out["label_id"] = json!(l.0);
    }
    if !warnings.is_empty() {
        out["warnings"] = json!(warnings);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every icon of the real pack has a distinct id that finds it back; set ids contain no `/` (the id is split at the first one).
    #[test]
    fn ids_are_unique_and_resolve_to_their_icon_in_the_real_pack() {
        let lib = library().unwrap();
        let mut seen = std::collections::HashSet::new();
        for (set, s) in lib.sets.iter().enumerate() {
            assert!(!s.id.contains('/'), "{}", s.id);
            for icon in 0..s.names.len() {
                let r = IconRef { set, icon };
                let id = icon_id(lib, r);
                assert!(seen.insert(id.clone()), "duplicate id {id}");
                assert_eq!(find_icon(lib, &id), Some(r), "{id}");
            }
        }
        assert_eq!(seen.len(), lib.icon_count());
    }

    #[test]
    fn repeated_names_get_a_numbered_suffix() {
        let lib = library().unwrap();
        let set = lib.sets.iter().position(|s| s.id == "simple-icons").unwrap();
        let names = &lib.sets[set].names;
        let first = names.iter().position(|n| n == "Backstage").unwrap();
        let second = names.iter().skip(first + 1).position(|n| n == "Backstage").map(|p| p + first + 1).unwrap();
        assert_eq!(icon_id(lib, IconRef { set, icon: first }), "simple-icons/Backstage");
        assert_eq!(icon_id(lib, IconRef { set, icon: second }), "simple-icons/Backstage#2");
        assert_eq!(find_icon(lib, "simple-icons/Backstage#2"), Some(IconRef { set, icon: second }));
        assert_eq!(find_icon(lib, "simple-icons/Backstage#1"), None);
        assert_eq!(find_icon(lib, "simple-icons/Backstage#9"), None);
        assert_eq!(find_icon(lib, "Backstage"), None);
    }
}
