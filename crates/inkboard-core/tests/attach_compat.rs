//! The fields added for the connector attachment side (`End::Bound.side`), the label position (`Connector.label_pos`, `label_offset`) and the
//! vertical alignment of a text (`Label.valign`): written only when set, so that a board that does not use them is written exactly as before,
//! and a file written before them loads unchanged.

use inkboard_core::connector::{AttachSide, Connector, End};
use inkboard_core::geom::Pt;
use inkboard_core::model::{Document, Id, Kind, Object, VAlign};
use inkboard_core::style::{Align, Style};
use inkboard_core::{BoxGeom, Frame, Label, serial};
use serde_json::{Value, json};

/// The ends as the first format read them: no `side`, and no check that unknown fields are absent.
#[derive(Debug, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum OldEnd {
    Bound {
        target: Id,
    },
    #[allow(dead_code)]
    Free {
        at: Pt,
    },
}

fn bound() -> Connector {
    Connector::new(End::bound(Id::from("a")), End::bound(Id::from("b")))
}

#[test]
fn a_bound_end_without_a_side_is_written_as_before_and_an_old_one_loads() {
    let v = serde_json::to_value(bound()).unwrap();
    assert_eq!(v["start"], json!({"kind": "bound", "target": "a", "anchor": {"mode": "auto"}}), "no `side` key");
    assert!(v.get("label_pos").is_none() && v.get("label_offset").is_none(), "{v}");
    let loaded: Connector = serde_json::from_value(v.clone()).unwrap();
    assert_eq!((loaded.start.side(), loaded.end.side(), loaded.label_pos), (None, None, None));
    assert!(loaded.label_offset.abs() < f64::EPSILON);
    assert_eq!(serde_json::to_value(&loaded).unwrap(), v, "written back identically");
}

#[test]
fn a_side_round_trips_and_is_ignored_by_a_reader_that_does_not_know_it() {
    let mut c = bound();
    c.start.set_side(Some(AttachSide::Bottom));
    c.end.set_side(Some(AttachSide::Left));
    let json = serde_json::to_string(&c).unwrap();
    assert!(json.contains("\"side\":\"bottom\"") && json.contains("\"side\":\"left\""), "{json}");
    assert_eq!(serde_json::from_str::<Connector>(&json).unwrap(), c);
    let v: Value = serde_json::from_str(&json).unwrap();
    let old: OldEnd = serde_json::from_value(v["start"].clone()).unwrap();
    assert!(matches!(old, OldEnd::Bound { target } if target.0 == "a"));
    // a side on a free end does nothing
    let mut free = Connector::straight(Pt::ZERO, Pt::new(10.0, 0.0));
    free.start.set_side(Some(AttachSide::Top));
    assert_eq!(free.start.side(), None);
    assert_eq!(serde_json::to_value(&free).unwrap(), serde_json::to_value(Connector::straight(Pt::ZERO, Pt::new(10.0, 0.0))).unwrap());
    // a name that is not a side is refused
    assert!(serde_json::from_value::<AttachSide>(json!("middle")).is_err());
    for s in [AttachSide::Top, AttachSide::Bottom, AttachSide::Left, AttachSide::Right] {
        assert_eq!(AttachSide::from_name(s.name()), Some(s));
    }
    assert_eq!(AttachSide::from_name("auto"), None);
}

#[test]
fn label_position_and_offset_are_written_only_when_set() {
    let mut c = bound();
    c.set_label_pos(Some(0.3));
    c.set_label_offset(-12.5);
    let json = serde_json::to_string(&c).unwrap();
    assert!(json.contains("\"label_pos\":0.3") && json.contains("\"label_offset\":-12.5"), "{json}");
    assert_eq!(serde_json::from_str::<Connector>(&json).unwrap(), c);
    // only one of them
    let mut only_offset = bound();
    only_offset.set_label_offset(8.0);
    let json = serde_json::to_string(&only_offset).unwrap();
    assert!(json.contains("label_offset") && !json.contains("label_pos"), "{json}");
}

#[test]
fn a_vertical_alignment_is_written_only_when_it_is_not_the_middle() {
    let mut l = Label::new("Title", 18.0, Align::Left);
    let v = serde_json::to_value(&l).unwrap();
    assert!(v.get("valign").is_none(), "{v}");
    assert_eq!(serde_json::from_value::<Label>(v).unwrap().valign, VAlign::Middle, "a label written before the field is centered");
    for (valign, name) in [(VAlign::Top, "top"), (VAlign::Bottom, "bottom")] {
        l.valign = valign;
        let v = serde_json::to_value(&l).unwrap();
        assert_eq!(v["valign"], name);
        assert_eq!(serde_json::from_value::<Label>(v).unwrap(), l);
        assert_eq!(VAlign::from_name(name), Some(valign));
    }
    assert_eq!(VAlign::from_name("middle"), Some(VAlign::Middle));
    assert_eq!(VAlign::from_name("center"), None);
    // an explicit "middle" in a file is the default and is written back without the key
    let l: Label = serde_json::from_value(json!({"size": 20.0, "valign": "middle"})).unwrap();
    assert!(serde_json::to_value(&l).unwrap().get("valign").is_none());
}

#[test]
fn a_board_that_uses_none_of_the_new_fields_is_written_byte_identical() {
    let mut doc = Document::default();
    let a = Object::new_box(BoxGeom::Rect, Frame::new(0.0, 0.0, 80.0, 40.0), Style::default(), Label::new("A", 20.0, Align::Center));
    let b = Object::new_box(BoxGeom::Rect, Frame::new(200.0, 0.0, 80.0, 40.0), Style::default(), Label::new("B", 20.0, Align::Center));
    let mut c = Connector::new(End::bound(a.id.clone()), End::bound(b.id.clone()));
    c.label = Some(Label::new("go", 14.0, Align::Center));
    doc.pages[0].objects = vec![a, b, Object::new(Kind::Connector(c), Style::default())];
    let json = serial::to_json(&doc, true);
    for key in ["side", "valign", "label_pos", "label_offset"] {
        assert!(!json.contains(&format!("\"{key}\"")), "{key} written: {json}");
    }
    assert_eq!(serial::to_json(&serial::from_json(&json).unwrap(), true), json);
}

#[test]
fn a_board_with_the_new_fields_survives_a_save_and_a_load() {
    let mut doc = Document::default();
    let mut title = Label::new("Zone", 22.0, Align::Left);
    title.valign = VAlign::Top;
    let frame = Object::new_box(BoxGeom::Rect, Frame::new(0.0, 0.0, 300.0, 200.0), Style::default(), title);
    let other = Object::new_box(BoxGeom::Rect, Frame::new(400.0, 0.0, 80.0, 40.0), Style::default(), Label::default());
    let mut c = Connector::new(End::bound(frame.id.clone()), End::bound(other.id.clone()));
    c.start.set_side(Some(AttachSide::Right));
    c.end.set_side(Some(AttachSide::Top));
    c.label = Some(Label::new("flow", 14.0, Align::Center));
    c.set_label_pos(Some(0.7));
    c.set_label_offset(14.0);
    doc.pages[0].objects = vec![frame, other, Object::new(Kind::Connector(c), Style::default())];
    let back = serial::from_json(&serial::to_json(&doc, true)).unwrap();
    assert_eq!(back.pages[0].objects, doc.pages[0].objects);
}
