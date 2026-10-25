//! Tools of the MCP v2 plan (see `context/PLAN_MCP_V2.md`); the module is registered in `tools.rs`.
//!
//! `render`: a PNG of the board (or of a region of it) returned to the agent as an MCP image. The tool result is a JSON value carrying the
//! reserved key `_image` (`{"mime", "data_base64"}`); `lib.rs` turns it into an image content block followed by the rest as text.

use super::{Res, obj, os, page_prop, path_prop};
use base64::{Engine, engine::general_purpose::STANDARD};
use inkboard_core::num;
use inkboard_core::svg::{SvgOptions, export as export_svg};
use inkboard_core::{ApproxMeasurer, Editor, Id, Kind, Rect};
use serde_json::{Value, json};
use std::fmt::Write;
use std::path::PathBuf;

/// Tools exposed by this module.
pub(super) const NAMES: &[&str] = &["render"];
/// Those that do not modify the board.
pub(super) const READ_ONLY: &[&str] = &["render"];

/// A PNG above this size is rendered again, smaller.
const PNG_MAX_BYTES: usize = 4 << 20;
const MAX_PX_DEFAULT: u32 = 1400;
const MAX_PX_LIMIT: u32 = 3000;
const MAX_PX_MIN: u32 = 16;
/// A small region is not enlarged more than this (pixels per board unit).
const MAX_ZOOM: f64 = 4.0;
/// Font size of the id badges, in image pixels (so they stay legible whatever the zoom).
const BADGE_PX: f64 = 12.0;

/// Schemas for `tools/list`.
pub(super) fn specs() -> Vec<Value> {
    vec![json!({"name": "render",
        "description": "Returns a PNG picture of the board (or of a part of it) as an image, to check your work visually. Read-only: the board, the file and the undo history are untouched. \
            By default the whole page content is shown. `region` {x, y, w, h} (board units) shows exactly that rectangle; `ids` frames the given objects (plus `padding`) while still drawing their surroundings; give either, not both. \
            `show_ids: true` draws a small white badge with the id of every visible object (top-left corner; middle of an arrow) so you can link what you see to the ids that `update_object` or `add_connector` expect (pen strokes get none). \
            The longest side is `max_px` pixels (default 1400, 16 to 3000; a small region is enlarged 4 times at most); a PNG above 4 MiB is automatically produced smaller (said in `note`). \
            `output` also writes the PNG to a .png file under the allowed root, for clients that do not display images. Text is measured approximately, as in `export`.",
        "inputSchema": obj(json!({
            "path": path_prop(),
            "page": page_prop(),
            "region": {"type": "object", "description": "Rectangle of the board to show, in board units (as the x/y/w/h of describe_board). Padding is not added.",
                "properties": {"x": {"type": "number"}, "y": {"type": "number"}, "w": {"type": "number", "exclusiveMinimum": 0}, "h": {"type": "number", "exclusiveMinimum": 0}},
                "required": ["x", "y", "w", "h"]},
            "ids": {"type": "array", "items": {"type": "string"}, "minItems": 1, "description": "Ids of the objects to frame (not combinable with `region`)."},
            "padding": {"type": "number", "minimum": 0, "description": "Margin around the content or the framed ids, in board units. Default 24."},
            "max_px": {"type": "integer", "minimum": MAX_PX_MIN, "maximum": MAX_PX_LIMIT, "description": "Longest side of the image in pixels. Default 1400."},
            "show_ids": {"type": "boolean", "description": "Draw the object ids on the picture. Default false."},
            "output": {"type": "string", "description": "Also write the PNG to this file (must end with .png, under the allowed root)."}
        }), &[])})]
}

/// Runs the tool when this module owns it, `None` otherwise.
pub(super) fn apply(ed: &mut Editor, name: &str, a: &Value) -> Option<Res<Value>> {
    (name == "render").then(|| render(ed, a))
}

/// Optional number: absent or null gives `None`, any other type is an error.
fn number(a: &Value, k: &str) -> Res<Option<f64>> {
    match a.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_f64().filter(|n| n.is_finite()).map(Some).ok_or_else(|| format!("\"{k}\" must be a number")),
    }
}

fn parse_region(v: &Value) -> Res<Rect> {
    let g = |k: &str| {
        v.get(k)
            .and_then(Value::as_f64)
            .filter(|n| n.is_finite() && n.abs() <= 1e7)
            .ok_or_else(|| format!("region.{k} must be a number (region = {{x, y, w, h}} in board units)"))
    };
    let r = Rect::new(g("x")?, g("y")?, g("w")?, g("h")?);
    if r.w <= 0.0 || r.h <= 0.0 {
        return Err("region.w and region.h must be greater than 0".into());
    }
    Ok(r)
}

/// Image size in pixels and zoom (pixels per board unit) so that the longest side is `max_px`.
fn pixel_size(r: &Rect, max_px: u32) -> (u32, u32, f64) {
    let zoom = (f64::from(max_px) / r.w.max(r.h)).min(MAX_ZOOM);
    let px = |len: f64| num::to_u32((len * zoom).round()).clamp(1, max_px);
    (px(r.w), px(r.h), zoom)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Id badges as SVG (board units, drawn over the objects), and how many fall inside `region`.
fn badges(page: &inkboard_core::Page, region: &Rect, zoom: f64) -> (String, usize) {
    let (font, line) = (BADGE_PX / zoom, 1.0 / zoom);
    let (mut out, mut shown) = (String::new(), 0);
    for o in page.objects.iter().filter(|o| page.is_visible(o) && !matches!(o.kind, Kind::Pen(_))) {
        let (w, h) = (num::f64_of(o.id.0.chars().count()) * font * 0.6 + font * 0.8, font * 1.4);
        // anchor of the object, and where its badge goes: just above it, so the badge never hides the object's own text or arrow label
        let ((ax, ay), (x, y)) = if let Kind::Connector(c) = &o.kind {
            match page.connector_path(c) {
                // a quarter of the way along the first segment: the middle is where the arrow's label sits
                Some(path) if path.len() >= 2 => {
                    let (a, b) = (path[0], path[1]);
                    let (px, py) = (a.x + (b.x - a.x) * 0.25, a.y + (b.y - a.y) * 0.25);
                    ((px, py), (px - w / 2.0, py - h * 1.2))
                }
                _ => continue,
            }
        } else {
            let b = page.bounds(o);
            ((b.x, b.y), (b.x, b.y - h))
        };
        // the badge belongs to the picture when the object's anchor does (it may be cut at the edge)
        if ax < region.x || ay < region.y || ax > region.right() || ay > region.bottom() {
            continue;
        }
        let y = y.max(region.y);
        shown += 1;
        let _ = write!(
            out,
            r##"<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{}" fill="#ffffff" stroke="#666666" stroke-width="{line}"/><text x="{}" y="{}" text-anchor="middle" font-family="monospace" font-size="{font}" fill="#111111">{}</text>"##,
            2.0 * line,
            x + w / 2.0,
            y + h / 2.0 + font * 0.35,
            esc(&o.id.0)
        );
    }
    (out, shown)
}

/// Re-frames the engine's SVG on `region`, at the pixel size `w` × `h`, with the background and the `extra` markup on top.
fn frame(full: &str, background: &str, region: &Rect, (w, h): (u32, u32), extra: &str) -> Res<String> {
    let start = full.find("<svg").ok_or("internal error: unexpected SVG")?;
    let head_end = start + full[start..].find('>').ok_or("internal error: unexpected SVG")? + 1;
    let mut body = &full[head_end..];
    // the engine's own background rectangle is replaced by one that covers the region
    if body.starts_with("<rect")
        && let Some(end) = body.find("/>")
    {
        body = &body[end + 2..];
    }
    let body = body.strip_suffix("</svg>").ok_or("internal error: unexpected SVG")?;
    let mut s = String::with_capacity(full.len() + extra.len() + 400);
    let _ = write!(
        s,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="{x} {y} {rw} {rh}" preserveAspectRatio="none"><rect x="{x}" y="{y}" width="{rw}" height="{rh}" fill="{background}"/>"#,
        x = region.x,
        y = region.y,
        rw = region.w,
        rh = region.h
    );
    s.push_str(body);
    s.push_str(extra);
    s.push_str("</svg>");
    Ok(s)
}

fn round(v: f64, digits: i32) -> f64 {
    let m = 10f64.powi(digits);
    (v * m).round() / m
}

fn render(ed: &mut Editor, a: &Value) -> Res<Value> {
    let region_arg = a.get("region").filter(|v| !v.is_null());
    let ids_arg = a.get("ids").filter(|v| !v.is_null());
    if region_arg.is_some() && ids_arg.is_some() {
        return Err("give either region or ids, not both".into());
    }
    let region = region_arg.map(parse_region).transpose()?;
    let padding = number(a, "padding")?.unwrap_or(24.0);
    if !(0.0..=1e6).contains(&padding) {
        return Err("\"padding\" must be between 0 and 1000000".into());
    }
    let max_px = match a.get("max_px").filter(|v| !v.is_null()) {
        None => MAX_PX_DEFAULT,
        Some(v) => v
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| (MAX_PX_MIN..=MAX_PX_LIMIT).contains(n))
            .ok_or_else(|| format!("\"max_px\" must be an integer between {MAX_PX_MIN} and {MAX_PX_LIMIT}"))?,
    };
    let show_ids = match a.get("show_ids").filter(|v| !v.is_null()) {
        None => false,
        Some(v) => v.as_bool().ok_or("\"show_ids\" must be true or false")?,
    };
    let output = os(a, "output").map(PathBuf::from);
    if let Some(p) = &output
        && !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("png"))
    {
        return Err("\"output\" must be a .png file (the picture is a PNG)".into());
    }

    let doc = &ed.doc;
    let page = ed.page();
    let content = page.content_bounds().ok_or("the page is empty: nothing to render")?;
    let framed = |r: Rect| {
        let r = r.expand(padding);
        Rect::new(r.x, r.y, r.w.max(1.0), r.h.max(1.0))
    };
    let region = match (region, ids_arg) {
        (Some(r), _) => r,
        (None, Some(ids)) => {
            let ids: Vec<Id> = ids
                .as_array()
                .filter(|l| !l.is_empty())
                .ok_or("\"ids\" must be a non-empty list of object ids")?
                .iter()
                .map(|i| i.as_str().map(|s| Id(s.to_owned())).ok_or("\"ids\" must contain only strings"))
                .collect::<Result<_, _>>()?;
            let unknown: Vec<&str> = ids.iter().filter(|i| page.get(i).is_none()).map(|i| i.0.as_str()).collect();
            if !unknown.is_empty() {
                return Err(format!("object(s) not found on this page: {}", unknown.join(", ")));
            }
            framed(page.bounds_of(&ids).ok_or("\"ids\" must be a non-empty list of object ids")?)
        }
        (None, None) => framed(content),
    };

    let full = export_svg(doc, page, None, &ApproxMeasurer, &SvgOptions { padding: 0.0, transparent: false }).ok_or("the page is empty: nothing to render")?;
    let background = doc.background.preset.hex();
    let (mut limit, mut reduced) = (max_px, false);
    let (png, (w, h, zoom), shown) = loop {
        let size = pixel_size(&region, limit);
        let (extra, shown) = if show_ids { badges(page, &region, size.2) } else { (String::new(), 0) };
        let svg = frame(&full, &background, &region, (size.0, size.1), &extra)?;
        let png = inkboard_formats::raster::svg_to_png(&svg, 1.0).map_err(|e| format!("rendering failed: {e}"))?;
        if png.len() <= PNG_MAX_BYTES {
            break (png, size, shown);
        }
        // the PNG size grows roughly with the number of pixels: aim a little below the limit
        let next = num::to_u32((f64::from(limit) * 0.9 * (num::f64_of(PNG_MAX_BYTES) / num::f64_of(png.len())).sqrt()).floor());
        if next < MAX_PX_MIN {
            return Err(format!("the picture stays above {} MiB even at {MAX_PX_MIN} px: show a smaller region", PNG_MAX_BYTES >> 20));
        }
        (limit, reduced) = (next.min(limit - 1), true);
    };

    let mut out = json!({
        "_image": {"mime": "image/png", "data_base64": STANDARD.encode(&png)},
        "width_px": w,
        "height_px": h,
        "region": {"x": round(region.x, 2), "y": round(region.y, 2), "w": round(region.w, 2), "h": round(region.h, 2)},
        "scale": round(zoom, 6),
        "ids_shown": shown,
    });
    if reduced {
        out["max_px_used"] = json!(limit);
        out["note"] = json!(format!("the PNG exceeded {} MiB: rendered with max_px {limit} instead of {max_px} (see scale)", PNG_MAX_BYTES >> 20));
    }
    if let Some(p) = output {
        std::fs::write(&p, &png).map_err(|e| format!("writing {}: {e}", p.display()))?;
        out["output"] = json!(p.display().to_string());
    }
    Ok(out)
}
