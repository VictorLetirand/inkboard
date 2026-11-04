//! `check_layout`: the measurable defects of a page, as a list of issues with object ids (see `inkboard_core::layout_check` for the rules).

use super::{Res, edit, obj, page_prop, path_prop};
use inkboard_core::layout_check::{DEFAULT_EXPORT_WIDTH, Options, check};
use inkboard_core::{Editor, Id};
use serde_json::{Value, json};

/// Tools exposed by this module.
pub(super) const NAMES: &[&str] = &["check_layout"];
/// Those that do not modify the board.
pub(super) const READ_ONLY: &[&str] = &["check_layout"];

const EXPORT_WIDTH_MIN: f64 = 100.0;
const EXPORT_WIDTH_MAX: f64 = 20000.0;

/// Schemas for `tools/list`.
pub(super) fn specs() -> Vec<Value> {
    vec![json!({"name": "check_layout",
        "description": "Lists the measurable layout defects of a page, with the ids of the objects involved, so you can fix them without guessing from a picture. Read-only. \
            Returns {ok, issues: [{kind, severity, ids, message, ...details}], metrics: {objects, connectors, content_box, aspect, min_text_px, empty_cells}}; `ok` is true when there is no issue (errors and warnings). \
            Errors: overlap, text_overflow, word_split, connector_through_object, unbound_connector, duplicate_connector. \
            Warnings: text_outside_frame, connector_crossing, shared_segment, label_collision, small_text, unbalanced. \
            `ids` keeps only the issues that involve one of these objects (page-level `unbalanced` is then left out). `export_width` (pixels, default 1600) is the width the page will be exported at, used for `small_text` and `min_text_px`. \
            Text is measured approximately and objects are compared by their bounding boxes, so a few reports can be false alarms (look at them with `render`). Run it after drawing, fix the errors, run it again.",
        "inputSchema": obj(json!({
            "path": path_prop(),
            "page": page_prop(),
            "ids": {"type": "array", "items": {"type": "string"}, "minItems": 1, "description": "Only report the issues that involve these objects."},
            "export_width": {"type": "number", "minimum": EXPORT_WIDTH_MIN, "maximum": EXPORT_WIDTH_MAX, "description": "Width in pixels of the planned export (default 1600), for the text-size checks."}
        }), &[])})]
}

/// Runs the tool when this module owns it, `None` otherwise.
pub(super) fn apply(ed: &mut Editor, name: &str, a: &Value) -> Option<Res<Value>> {
    (name == "check_layout").then(|| check_layout(ed, a))
}

fn check_layout(ed: &mut Editor, a: &Value) -> Res<Value> {
    let export_width = edit::num_arg(a, "export_width", EXPORT_WIDTH_MIN, EXPORT_WIDTH_MAX)?.unwrap_or(DEFAULT_EXPORT_WIDTH);
    let ids: Option<Vec<Id>> = match a.get("ids") {
        None | Some(Value::Null) => None,
        Some(_) => Some(edit::ids_arg(ed, a, "ids")?),
    };
    let report = check(ed.page(), &Options { export_width, ids: ids.as_deref() });
    let issues: Vec<Value> = report
        .issues
        .iter()
        .map(|i| {
            let mut v = json!({"kind": i.kind, "severity": i.severity.name(), "ids": i.ids.iter().map(|id| &id.0).collect::<Vec<_>>(), "message": i.message});
            for (k, d) in &i.details {
                v[*k] = d.clone();
            }
            v
        })
        .collect();
    let m = &report.metrics;
    let round = |v: f64| (v * 100.0).round() / 100.0;
    let mut out = json!({
        "ok": report.ok(),
        "issues": issues,
        "metrics": {
            "objects": m.objects,
            "connectors": m.connectors,
            "content_box": m.content_box.map(|r| json!({"x": round(r.x), "y": round(r.y), "w": round(r.w), "h": round(r.h)})),
            "aspect": m.aspect.map(round),
            "min_text_px": m.min_text_px.map(round),
            "empty_cells": m.empty_cells,
            "export_width": export_width,
        }
    });
    if report.truncated {
        out["truncated"] = json!(format!("only the first {} issues of each kind are listed", inkboard_core::layout_check::MAX_PER_KIND));
    }
    Ok(out)
}
