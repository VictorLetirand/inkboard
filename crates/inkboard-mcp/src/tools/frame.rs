//! `add_frame`: a grouping frame fitted around existing objects (or re-fitted after they moved); the module is registered in `tools.rs`.

use super::{Res, edit, find, obj, ocolor, os, page_prop, path_prop, richtext};
use inkboard_core::editor::Order;
use inkboard_core::model::{RECT_CORNER_INSET, RECT_INSET};
use inkboard_core::text::layout_rich;
use inkboard_core::{Align, ApproxMeasurer, BoxGeom, Color, Editor, Frame, Id, Kind, Label, Object, Rect, RichText, Span, SpanStyle, Style, VAlign, num};
use serde_json::{Value, json};

/// Tools exposed by this module.
pub(super) const NAMES: &[&str] = &["add_frame"];
/// Those that do not modify the board.
pub(super) const READ_ONLY: &[&str] = &[];

const PADDING: f64 = 40.0;
const TITLE_SIZE: f64 = 18.0;
/// Space between the title line and the content, added to the padding on top.
const TITLE_GAP: f64 = 12.0;
/// Extra room on top: the title starts `RECT_CORNER_INSET` below the border instead of `RECT_INSET` (see `BoxObj::label_rect`).
const TITLE_TOP_EXTRA: f64 = RECT_CORNER_INSET - RECT_INSET;
/// Default title color (dark grey, readable on the pale fill).
const TITLE_COLOR: Color = Color::rgb(0x34, 0x3a, 0x40);
const MAX_SIDE: f64 = 1_000_000.0;
const FILL: Color = Color::rgb(0xf1, 0xf3, 0xf5);
const STROKE: Color = Color::rgb(0xad, 0xb5, 0xbd);

/// Schemas for `tools/list`.
pub(super) fn specs() -> Vec<Value> {
    let color = |what: &str| json!({"type": "string", "description": format!("{what} \"#rrggbb\" or \"ink\".")});
    vec![json!({"name": "add_frame",
        "description": "Adds a grouping frame around existing objects, in one call: ONE rect whose own text is the title (top left), sized to the union of the boxes of `ids` (an icon and its label count together) plus `padding`, \
            with extra room on top for the title, and placed behind them (just under the lowest of them: a frame around objects already inside another frame is drawn above the outer one and below its own content). \
            It is not grouped with its content: moving the content later leaves it behind; call add_frame again with `frame` (its id) and `ids` to re-fit it. A title wider than the frame widens the frame. \
            Alternatively give `x`, `y`, `w`, `h` (all four) for an exact rectangle, with or without `ids` (then only used for the stacking order; with neither the frame goes to the back). \
            Returns {id, x, y, w, h}. With `frame`, the geometry is recomputed and `title`, `fill`, `stroke`, `dash`, `roundness` change only if given.",
        "inputSchema": obj(json!({"path": path_prop(), "page": page_prop(),
            "ids": {"type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": 2000, "description": "Objects to frame (required unless x, y, w and h are given). The other members of their groups are included."},
            "frame": {"type": "string", "description": "Id of an existing frame (a rect made by add_frame): re-fit it around `ids` instead of creating one."},
            "title": {"type": "string", "description": "Title, inside the frame at the top left."},
            "title_color": color("Title color (default #343a40, dark grey; not the border color). On a re-fit, the existing title color is kept unless given."),
            "padding": {"type": "number", "minimum": 0, "maximum": 10000, "description": "Space between the content and the border (default 40); a title adds its height + 20 on top."},
            "fill": color("Background (default #f1f3f5)."), "stroke": color("Border (default #adb5bd)."),
            "dash": {"type": "string", "enum": ["solid", "dashed", "dotted"], "description": "Border style (default solid)."},
            "title_font_size": {"type": "number", "minimum": 4, "maximum": 400, "description": "Default 18."},
            "title_bold": {"type": "boolean", "description": "Default true."},
            "roundness": {"type": "number", "minimum": 0, "maximum": 1, "description": "Corner rounding (default 0.15, as add_shape rect)."},
            "x": {"type": "number"}, "y": {"type": "number"}, "w": {"type": "number", "minimum": 1}, "h": {"type": "number", "minimum": 1}}), &[])})]
}

pub(super) fn apply(ed: &mut Editor, name: &str, a: &Value) -> Option<Res<Value>> {
    (name == "add_frame").then(|| add_frame(ed, a))
}

/// Height and width of the title laid out on its own lines, without wrapping; (0, 0) without title.
fn title_size(label: &Label) -> (f64, f64) {
    if label.text.is_empty() {
        return (0.0, 0.0);
    }
    let lay = layout_rich(&label.text, None, &ApproxMeasurer, label.family, label.size);
    (lay.height.max(label.line_height()), lay.width)
}

#[allow(clippy::many_single_char_names)] // x, y, w, h, a: the tool's own parameter names
#[allow(clippy::too_many_lines)] // one tool: arguments, title, geometry, stacking, edit
fn add_frame(ed: &mut Editor, a: &Value) -> Res<Value> {
    let padding = edit::num_arg(a, "padding", 0.0, 10_000.0)?.unwrap_or(PADDING);
    let title = os(a, "title");
    let size = edit::num_arg(a, "title_font_size", 4.0, 400.0)?.unwrap_or(TITLE_SIZE);
    let bold = edit::bool_arg(a, "title_bold")?.unwrap_or(true);
    let title_color = ocolor(a, "title_color")?;
    let (fill, stroke) = (ocolor(a, "fill")?, ocolor(a, "stroke")?);
    let dash = edit::dash_arg(a)?;
    let roundness = edit::num_arg(a, "roundness", 0.0, 1.0)?;
    let (x, y) = (edit::num_arg(a, "x", -MAX_SIDE, MAX_SIDE)?, edit::num_arg(a, "y", -MAX_SIDE, MAX_SIDE)?);
    let (w, h) = (edit::num_arg(a, "w", 1.0, MAX_SIDE)?, edit::num_arg(a, "h", 1.0, MAX_SIDE)?);
    let rect = match (x, y, w, h) {
        (Some(x), Some(y), Some(w), Some(h)) => Some(Rect::new(x, y, w, h)),
        (None, None, None, None) => None,
        _ => return Err("give x, y, w and h together (an exact rectangle), or none of them".into()),
    };
    let existing = os(a, "frame").map(|f| find(ed, f).map(|o| o.id.clone())).transpose()?;
    let mut ids = if a.get("ids").is_some_and(|v| !v.is_null()) {
        edit::ids_arg(ed, a, "ids")?
    } else if rect.is_some() || existing.is_some() {
        Vec::new()
    } else {
        return Err("give \"ids\" (the objects to frame) or x, y, w and h".into());
    };
    if let Some(f) = &existing {
        if !matches!(&find(ed, &f.0)?.kind, Kind::Box(b) if b.geom == BoxGeom::Rect) {
            return Err(format!("\"frame\" {} is not a rect: only a frame (a rect) can be re-fitted", f.0));
        }
        ids.retain(|i| i != f);
        if ids.is_empty() && rect.is_none() {
            return Err("re-fitting a frame needs \"ids\" (the objects it must surround) or x, y, w and h".into());
        }
    }
    let ids = ed.page().expand_groups(&ids);

    // the title: the new one, else (re-fit) the one the frame already has
    let current = existing.as_ref().and_then(|f| ed.page().get(f)).and_then(|o| if let Kind::Box(b) = &o.kind { Some(b.label.clone()) } else { None });
    let label = if let Some(t) = title {
        let mut l = Label::new(t, size, Align::Left);
        l.valign = VAlign::Top;
        // a re-fit with a new title but no color keeps the color of the old title
        let color = title_color.or_else(|| current.as_ref().and_then(|c| c.text.0.iter().find_map(|s| s.style.color))).unwrap_or(TITLE_COLOR);
        if !t.is_empty() {
            l.text = RichText(vec![Span { text: t.to_owned(), style: SpanStyle { bold, color: Some(color), ..SpanStyle::default() } }]);
        }
        l
    } else {
        let mut l = current.unwrap_or_default();
        if let Some(c) = title_color {
            for s in &mut l.text.0 {
                s.style.color = Some(c);
            }
        }
        l
    };
    let (title_h, title_w) = title_size(&label);

    let mut warnings = Vec::new();
    let mut area = match (rect, ed.page().bounds_of(&ids)) {
        (Some(r), _) => r,
        (None, Some(u)) => {
            let top = if title_h > 0.0 { title_h + TITLE_GAP + TITLE_TOP_EXTRA } else { 0.0 };
            Rect::new(u.x - padding, u.y - padding - top, u.w + 2.0 * padding, u.h + 2.0 * padding + top)
        }
        (None, None) => return Err("nothing to frame: the objects have no box".into()),
    };
    let min_w = title_w.ceil() + RECT_CORNER_INSET + RECT_INSET + 2.0;
    if title_h > 0.0 && area.w < min_w {
        area.w = min_w;
        warnings.push("the frame was widened to hold its title on one line".to_owned());
    }

    // stacking: just under the lowest framed object (at the back without any); `above`: what must end up over the frame
    let objects: Vec<Id> = ed.page().objects.iter().map(|o| o.id.clone()).collect();
    let at = objects.iter().position(|i| ids.contains(i));
    let above = |skip: Option<&Id>| objects.iter().skip(at.unwrap_or(0)).filter(|i| Some(*i) != skip).cloned().collect::<Vec<_>>();
    let style_of = |s: &mut Style| {
        if let Some(c) = fill {
            s.fill = Some(c);
        }
        if let Some(c) = stroke {
            s.stroke = c;
        }
        if let Some(d) = dash {
            s.dash = d;
        }
        if let Some(r) = roundness {
            s.roundness = num::f32(r);
        }
    };
    let id = ed.transact(|ed| {
        if let Some(f) = &existing {
            ed.update(f, |o| {
                if let Kind::Box(b) = &mut o.kind {
                    b.frame = Frame::new(area.x, area.y, area.w, area.h);
                    if title.is_some() || title_color.is_some() {
                        b.label = label.clone();
                    }
                }
                style_of(&mut o.style);
            });
            if at.is_some_and(|at| objects.iter().position(|i| i == f).is_some_and(|cur| cur > at)) {
                ed.reorder(&above(Some(f)), Order::Front);
            }
            f.clone()
        } else {
            let mut style = Style { stroke: STROKE, fill: Some(FILL), width: 2.0, roundness: 0.15, ..Style::default() };
            style_of(&mut style);
            let id = ed.insert(Object::new_box(BoxGeom::Rect, Frame::new(area.x, area.y, area.w, area.h), style, label));
            ed.reorder(&above(None), Order::Front);
            id
        }
    });
    let mut out = json!({"id": id.0, "x": area.x.round(), "y": area.y.round(), "w": area.w.round(), "h": area.h.round()});
    richtext::add_warnings(&mut out, warnings);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::apply;
    use inkboard_core::{Document, Editor};
    use serde_json::json;

    fn shape(ed: &mut Editor, x: f64) -> String {
        apply(ed, "add_shape", &json!({"geom": "rect", "x": x, "y": 0, "w": 100, "h": 60})).unwrap()["id"].as_str().unwrap().to_owned()
    }

    #[test]
    fn a_frame_a_grid_and_a_layout_are_one_undo_step_each() {
        let mut ed = Editor::new(Document::default());
        let (a, b) = (shape(&mut ed, 0.0), shape(&mut ed, 500.0));
        apply(&mut ed, "add_connector", &json!({"from": a, "to": b})).unwrap();
        let steps = |ed: &mut Editor| {
            let mut n = 0;
            while ed.undo() {
                n += 1;
            }
            n
        };
        apply(&mut ed, "add_frame", &json!({"ids": [a, b], "title": "T"})).unwrap();
        apply(&mut ed, "arrange_objects", &json!({"ids": [a, b], "action": "grid", "columns": 1})).unwrap();
        apply(&mut ed, "auto_layout", &json!({"direction": "LR"})).unwrap();
        // 2 shapes + 1 connector + frame + grid + layout
        assert_eq!(steps(&mut ed), 6);
    }
}
