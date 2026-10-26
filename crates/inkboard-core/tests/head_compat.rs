//! The arrowheads added after the first format (`HeadExt`): stored next to the original ones, so that a file or a message written with them is still
//! read by a version that does not know them (which draws the closest original head), and a file without them is written exactly as before.

use inkboard_core::connector::{Arrowhead, Connector, End, HeadExt, HeadPart, head_glyph};
use inkboard_core::geom::Pt;
use inkboard_core::model::{Document, Kind, Object};
use inkboard_core::style::Style;
use inkboard_core::{BoxGeom, Frame, Label, serial};
use serde::Deserialize;

/// The connector as the first version of the format read it: no extended head, and no check that unknown fields are absent.
#[derive(Debug, Deserialize)]
struct OldConnector {
    #[serde(default)]
    head_start: Arrowhead,
    #[serde(default)]
    head_end: Arrowhead,
    #[serde(default)]
    via: Vec<Pt>,
}

fn connector(set: impl FnOnce(&mut Connector)) -> Connector {
    let mut c = Connector::new(End::free(Pt::new(0.0, 0.0)), End::free(Pt::new(100.0, 0.0)));
    set(&mut c);
    c
}

#[test]
fn every_extended_head_survives_json_and_older_readers_get_the_fallback() {
    for ext in HeadExt::ALL {
        let c = connector(|c| {
            c.set_ext_start(ext);
            c.set_ext_end(ext);
        });
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains(ext.name()), "{json}");
        let back: Connector = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c, "{ext:?}");
        assert_eq!((back.head_start_ext, back.head_end_ext), (Some(ext), Some(ext)));
        // a reader that knows only the first format: it ignores the new fields and keeps the original ones, which hold the fallback
        let old: OldConnector = serde_json::from_str(&json).unwrap();
        assert_eq!((old.head_start, old.head_end), (ext.fallback(), ext.fallback()), "{ext:?}");
        assert_eq!(old.via.len(), 0);
    }
}

#[test]
fn a_connector_without_extended_head_is_written_exactly_as_before() {
    let c = connector(|c| c.set_head_start(Arrowhead::Dot));
    let v = serde_json::to_value(&c).unwrap();
    let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["end", "head_end", "head_start", "route", "start", "via"], "no new key is written");
    // a connector written by the first version (none of the new keys) loads, with no extended head
    let c: Connector = serde_json::from_value(v).unwrap();
    assert_eq!((c.head_start, c.head_end, c.head_start_ext, c.head_end_ext), (Arrowhead::Dot, Arrowhead::Arrow, None, None));
}

#[test]
fn a_whole_board_round_trips_with_extended_heads_and_stays_the_same_without() {
    let mut doc = Document::default();
    let shape = Object::new_box(BoxGeom::Rect, Frame::new(0.0, 0.0, 80.0, 40.0), Style::default(), Label::default());
    let plain = Object::new(Kind::Connector(connector(|c| c.set_head_end(Arrowhead::Arrow))), Style::default());
    let extended = Object::new(Kind::Connector(connector(|c| c.set_ext_end(HeadExt::CrowFootZero))), Style::default());
    doc.pages[0].objects = vec![shape, plain, extended];
    let json = serial::to_json(&doc, true);
    let back = serial::from_json(&json).unwrap();
    assert_eq!(back.pages[0].objects, doc.pages[0].objects);
    assert_eq!(json.matches("head_end_ext").count(), 1, "only the connector that has one writes the key");
    // an object written this way, read by the first format's connector, loses nothing it could draw
    let Kind::Connector(c) = &back.pages[0].objects[2].kind else { panic!("connector") };
    assert_eq!((c.head_end, c.head_end_ext), (Arrowhead::Arrow, Some(HeadExt::CrowFootZero)));
}

#[test]
fn setting_an_original_head_drops_the_extended_one_and_the_other_way_round() {
    let mut c = connector(|_| {});
    c.set_ext_end(HeadExt::Diamond);
    assert_eq!((c.head_end, c.head_end_ext), (Arrowhead::Dot, Some(HeadExt::Diamond)));
    c.set_head_end(Arrowhead::Bar);
    assert_eq!((c.head_end, c.head_end_ext), (Arrowhead::Bar, None));
    c.set_ext_start(HeadExt::TriangleOpen);
    assert!(c.has_head_start());
    c.set_head_start(Arrowhead::None);
    assert!(!c.has_head_start() && c.head_start_ext.is_none());
    assert!(connector(|c| c.set_ext_end(HeadExt::OneOnly)).has_head_end());
}

#[test]
fn head_scale_is_written_only_when_changed_and_an_old_file_is_rewritten_identically() {
    let plain = connector(|_| {});
    let json = serde_json::to_string(&plain).unwrap();
    assert!(!json.contains("head_scale"), "{json}");
    assert_eq!(serde_json::from_str::<Connector>(&json).unwrap(), plain);
    // a file written before the field existed loads with the usual size and is written back without it
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let loaded: Connector = serde_json::from_value(v.clone()).unwrap();
    assert!((loaded.head_scale - 1.0).abs() < f32::EPSILON);
    assert_eq!(serde_json::to_value(&loaded).unwrap(), v);
    // a changed size round-trips
    let big = connector(|c| c.set_head_scale(2.5));
    let json = serde_json::to_string(&big).unwrap();
    assert!(json.contains("head_scale"), "{json}");
    assert_eq!(serde_json::from_str::<Connector>(&json).unwrap(), big);
}

#[test]
fn head_scale_out_of_range_is_brought_back_and_a_non_finite_one_is_ignored() {
    let mut c = connector(|_| {});
    for (asked, kept) in [(0.1, 0.5), (9.0, 4.0), (0.5, 0.5), (4.0, 4.0), (1.7, 1.7)] {
        c.set_head_scale(asked);
        assert!((c.head_scale - kept).abs() < 1e-6, "{asked}");
    }
    c.set_head_scale(2.0);
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        c.set_head_scale(bad);
        assert!((c.head_scale - 2.0).abs() < 1e-6, "{bad}");
    }
    // a hostile file cannot make the drawn size leave the range either
    c.head_scale = 1000.0;
    assert!((c.head_size(3.0) - 13.5 * 4.0).abs() < 1e-9);
    c.head_scale = f32::NAN;
    assert!((c.head_size(3.0) - 13.5).abs() < 1e-9);
    assert!((connector(|_| {}).head_size(3.0) - 13.5).abs() < 1e-9, "the default size is the one always drawn");
}

#[test]
fn names_are_unique_and_read_back() {
    let mut seen = std::collections::HashSet::new();
    for e in HeadExt::ALL {
        assert!(seen.insert(e.name()), "{e:?} shares its name");
        assert_eq!(HeadExt::from_name(e.name()), Some(e));
    }
    assert_eq!(HeadExt::from_name("none"), None);
    assert_eq!(HeadExt::from_name("Diamond"), None, "names are lowercase");
    // an unknown name inside a file is an error for this field only when the value is not one of ours: it must not be silently accepted
    assert!(serde_json::from_str::<HeadExt>("\"hexagon\"").is_err());
}

#[test]
fn the_glyph_of_an_original_head_is_what_was_always_drawn() {
    let (tip, from) = (Pt::new(100.0, 0.0), Pt::new(0.0, 0.0));
    assert_eq!(head_glyph(tip, from, 12.0, Arrowhead::None, None).len(), 0);
    let pts = |parts: Vec<HeadPart>| match &parts[..] {
        [HeadPart::Line(p) | HeadPart::Polygon { pts: p, .. }] => p.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(pts(head_glyph(tip, from, 12.0, Arrowhead::Triangle, None)), inkboard_core::connector::head_points(tip, from, 12.0, Arrowhead::Triangle));
    assert_eq!(pts(head_glyph(tip, from, 12.0, Arrowhead::Arrow, None)), inkboard_core::connector::head_points(tip, from, 12.0, Arrowhead::Arrow));
    assert!(matches!(&head_glyph(tip, from, 12.0, Arrowhead::Triangle, None)[..], [HeadPart::Polygon { solid: true, .. }]));
    assert!(matches!(&head_glyph(tip, from, 12.0, Arrowhead::Dot, None)[..], [HeadPart::Circle { solid: true, .. }]));
}

#[test]
fn extended_glyphs_sit_at_the_tip_whatever_the_direction() {
    let size = 14.0;
    for angle in [0.0f64, 0.7, std::f64::consts::FRAC_PI_2, std::f64::consts::PI, -2.4] {
        let d = Pt::new(angle.cos(), angle.sin());
        let (tip, from) = (Pt::new(50.0, 50.0), Pt::new(50.0 - d.x * 80.0, 50.0 - d.y * 80.0));
        for ext in HeadExt::ALL {
            let parts = head_glyph(tip, from, size, Arrowhead::None, Some(ext));
            assert!(!parts.is_empty(), "{ext:?}");
            let points: Vec<Pt> = parts
                .iter()
                .flat_map(|p| match p {
                    HeadPart::Line(pts) | HeadPart::Polygon { pts, .. } => pts.clone(),
                    HeadPart::Circle { center, radius, .. } => {
                        vec![*center + Pt::new(*radius, 0.0), *center - Pt::new(*radius, 0.0), *center + Pt::new(0.0, *radius), *center - Pt::new(0.0, *radius)]
                    }
                })
                .collect();
            for p in &points {
                // along the line: from the tip back to at most 2 sizes; across: at most one size
                let rel = *p - tip;
                let along = -(rel.x * d.x + rel.y * d.y);
                let across = (rel.x * -d.y + rel.y * d.x).abs();
                assert!((-1e-6..=size * 2.0).contains(&along), "{ext:?} at {angle}: along {along}");
                assert!(across <= size * 0.6, "{ext:?} at {angle}: across {across}");
            }
            // a head touches the tip (the end of the line is at the border of the shape)
            assert!(points.iter().any(|p| (*p - tip).len() < size * 0.7), "{ext:?} does not reach the tip");
        }
    }
}

#[test]
fn hollow_pieces_are_the_hollow_heads_and_the_circles() {
    let (tip, from) = (Pt::new(60.0, 0.0), Pt::new(0.0, 0.0));
    let hollow = |e| {
        head_glyph(tip, from, 12.0, Arrowhead::None, Some(e))
            .iter()
            .any(|p| matches!(p, HeadPart::Polygon { solid: false, .. } | HeadPart::Circle { solid: false, .. }))
    };
    for (e, expected) in [
        (HeadExt::Diamond, false),
        (HeadExt::DiamondOpen, true),
        (HeadExt::TriangleOpen, true),
        (HeadExt::CrowFoot, false),
        (HeadExt::CrowFootOne, false),
        (HeadExt::CrowFootZero, true),
        (HeadExt::OneOnly, false),
        (HeadExt::ZeroOrOne, true),
    ] {
        assert_eq!(hollow(e), expected, "{e:?}");
    }
    assert!(head_glyph(tip, from, 12.0, Arrowhead::None, Some(HeadExt::Diamond)).iter().any(|p| matches!(p, HeadPart::Polygon { solid: true, .. })));
}
