//! Conversions between the core's index-based `Op` and the index-free `WireOp`.
// Test code counts, casts and compares freely: these `clippy::pedantic` lints are for the code that ships.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::float_cmp,
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::assert_is_empty,
    clippy::default_trait_access,
    clippy::format_collect,
    clippy::format_push_string,
    clippy::too_many_lines,
    clippy::items_after_statements,
    clippy::needless_raw_string_hashes,
    clippy::needless_pass_by_value,
    clippy::explicit_iter_loop,
    clippy::needless_for_each,
    clippy::collapsible_else_if,
    clippy::single_match_else,
    clippy::manual_assert,
    clippy::redundant_closure_for_method_calls,
    clippy::unnested_or_patterns,
    clippy::range_plus_one,
    clippy::clone_on_copy,
    clippy::match_same_arms,
    clippy::struct_excessive_bools
)]

use inkboard_core::geom::{Frame, Pt};
use inkboard_core::history::Op;
use inkboard_core::model::{Background, Document, Id, Layer, Object, Page};
use inkboard_core::style::Style;
use inkboard_core::{BgPreset, BoxGeom, Label, Pattern};
use inkboard_session::WireOp;
use inkboard_session::wire::{apply_wire, apply_wire_all, ops_to_wire, rollback};
use proptest::prelude::*;

fn obj(id: &str, x: f64) -> Object {
    let mut o = Object::new_box(BoxGeom::Rect, Frame::new(x, 0.0, 10.0, 10.0), Style::default(), Label::default());
    o.id = Id(id.into());
    o
}

fn base_doc() -> Document {
    let mut d = Document::new("doc");
    d.pages[0].id = Id("p0".into());
    d.pages[0].objects = vec![obj("a", 0.0), obj("b", 1.0), obj("c", 2.0)];
    d
}

fn ids(d: &Document, page: &str) -> Vec<String> {
    d.page(&Id(page.into())).unwrap().objects.iter().map(|o| o.id.0.clone()).collect()
}

#[test]
fn insert_at_the_top_is_a_plain_put_and_in_the_middle_adds_the_stacking_order() {
    let d = base_doc();
    let top = Op::Insert { page: "p0".into(), index: 3, obj: Box::new(obj("n", 5.0)) };
    let w = ops_to_wire(std::slice::from_ref(&top), &d);
    assert!(matches!(w.as_slice(), [WireOp::Put { .. }]));

    let mid = Op::Insert { page: "p0".into(), index: 1, obj: Box::new(obj("n", 5.0)) };
    let w = ops_to_wire(std::slice::from_ref(&mid), &d);
    let [WireOp::Put { .. }, WireOp::Order { ids: order, .. }] = w.as_slice() else { panic!("expected Put + Order, got {w:?}") };
    assert_eq!(order.iter().map(|i| i.0.as_str()).collect::<Vec<_>>(), ["a", "n", "b", "c"]);

    let mut applied = d.clone();
    apply_wire_all(&mut applied, &w);
    assert_eq!(ids(&applied, "p0"), ["a", "n", "b", "c"]);
}

#[test]
fn wire_operations_expand_to_core_operations_with_their_before_state() {
    let mut d = base_doc();
    let ops = apply_wire(&mut d, &WireOp::Delete { page: "p0".into(), ids: vec!["a".into(), "c".into(), "zzz".into()] });
    assert_eq!(ops.len(), 2, "unknown ids are ignored");
    assert!(matches!(&ops[0], Op::Remove { index: 0, obj, .. } if obj.id == Id("a".into())));
    assert!(matches!(&ops[1], Op::Remove { index: 1, obj, .. } if obj.id == Id("c".into())), "indices follow the successive removals");
    assert_eq!(ids(&d, "p0"), ["b"]);
    rollback(&mut d, &ops);
    assert_eq!(d, base_doc());

    // nothing changes: nothing is reported
    assert!(apply_wire(&mut d, &WireOp::Put { page: "p0".into(), object: Box::new(obj("a", 0.0)) }).is_empty());
    assert!(apply_wire(&mut d, &WireOp::Put { page: "nope".into(), object: Box::new(obj("x", 0.0)) }).is_empty());
    assert!(apply_wire(&mut d, &WireOp::Order { page: "p0".into(), ids: vec!["a".into(), "b".into(), "c".into()] }).is_empty());
    assert!(apply_wire(&mut d, &WireOp::RemovePage { page: "p0".into() }).is_empty(), "the last page cannot be removed");
}

#[test]
fn order_ignores_unknown_and_repeated_ids_like_the_core_does() {
    let mut d = base_doc();
    apply_wire(&mut d, &WireOp::Order { page: "p0".into(), ids: vec!["c".into(), "x".into(), "c".into(), "a".into()] });
    assert_eq!(ids(&d, "p0"), ["c", "a", "b"]);
}

/// One random core operation valid for `doc` (or `None` when the choice does not fit the document).
fn make_op(doc: &Document, n: &mut u32, code: u8, a: u16, b: u16) -> Option<Op> {
    let page = &doc.pages[usize::from(a) % doc.pages.len()];
    let pid = page.id.clone();
    let len = page.objects.len();
    let fresh = |n: &mut u32, p: &str| {
        *n += 1;
        format!("{p}{n}")
    };
    let x = f64::from(b);
    Some(match code % 11 {
        0 | 1 => Op::Insert { page: pid, index: usize::from(b) % (len + 2), obj: Box::new(obj(&fresh(n, "o"), x)) },
        2 if len > 0 => {
            let i = usize::from(b) % len;
            Op::Remove { page: pid, index: i, obj: Box::new(page.objects[i].clone()) }
        }
        3 if len > 0 => {
            let before = page.objects[usize::from(b) % len].clone();
            let mut after = before.clone();
            after.translate(Pt::new(x, 1.0));
            Op::Replace { page: pid, before: Box::new(before), after: Box::new(after) }
        }
        4 => {
            let before: Vec<Id> = page.objects.iter().map(|o| o.id.clone()).collect();
            let mut after = before.clone();
            if len > 1 {
                after.rotate_left(usize::from(b) % len);
                after.swap(0, len - 1);
            }
            after.truncate(usize::from(a) % (len + 1)); // a partial list is allowed: the rest keeps its order
            Op::Reorder { page: pid, before, after }
        }
        5 => {
            let before = page.layers.clone();
            let mut after = before.clone();
            after.push(Layer { id: Id(fresh(n, "l")), name: format!("layer {b}"), visible: a.is_multiple_of(2), locked: b.is_multiple_of(2) });
            Op::SetLayers { page: pid, before, after }
        }
        6 => {
            let mut p = Page::new("added");
            p.id = Id(fresh(n, "pg"));
            p.objects = (0..usize::from(b % 3)).map(|_| obj(&fresh(n, "po"), 0.0)).collect();
            Op::AddPage { index: usize::from(b) % (doc.pages.len() + 2), page: Box::new(p) }
        }
        7 if doc.pages.len() > 1 => {
            let i = usize::from(b) % doc.pages.len();
            Op::RemovePage { index: i, page: Box::new(doc.pages[i].clone()) }
        }
        8 => Op::RenamePage { page: pid, before: page.name.clone(), after: format!("name {b}") },
        9 => Op::SetBackground {
            before: doc.background,
            after: Background { preset: BgPreset::ALL[usize::from(b) % 6], pattern: if a.is_multiple_of(2) { Pattern::Grid } else { Pattern::Lines } },
        },
        10 => Op::SetName { before: doc.name.clone(), after: format!("doc {b}") },
        _ => return None,
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 200, failure_persistence: None, ..ProptestConfig::default() })]

    /// Op -> WireOp -> Op: the same final document, an exact rollback, and a stable second conversion.
    #[test]
    fn op_wire_op_round_trip(steps in prop::collection::vec((0u8..22, any::<u16>(), any::<u16>()), 1..40)) {
        let doc0 = base_doc();
        let mut model = doc0.clone();
        let mut ops: Vec<Op> = Vec::new();
        let mut n = 0u32;
        for (code, a, b) in steps {
            if let Some(op) = make_op(&model, &mut n, code, a, b) {
                model.apply(&op);
                ops.push(op);
            }
        }

        let wire = ops_to_wire(&ops, &doc0);

        // wire operations survive their JSON form
        let json = serde_json::to_string(&wire).unwrap();
        let back: Vec<WireOp> = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&back, &wire);

        // same final document
        let mut via_wire = doc0.clone();
        let produced = apply_wire_all(&mut via_wire, &wire);
        prop_assert_eq!(&via_wire, &model);

        // the core operations produced roll back exactly
        let mut undone = via_wire.clone();
        rollback(&mut undone, &produced);
        prop_assert_eq!(&undone, &doc0);

        // and replaying them (or converting them again) gives the same document
        let mut replay = doc0.clone();
        produced.iter().for_each(|o| replay.apply(o));
        prop_assert_eq!(&replay, &model);
        let wire2 = ops_to_wire(&produced, &doc0);
        let mut again = doc0.clone();
        apply_wire_all(&mut again, &wire2);
        prop_assert_eq!(&again, &model);
    }
}

#[test]
fn a_connector_with_an_extended_head_travels_whole_and_a_message_from_an_older_version_still_reads() {
    use inkboard_core::connector::{Connector, End, HeadExt};
    use inkboard_core::model::Kind;
    let mut c = Connector::new(End::free(Pt::new(0.0, 0.0)), End::free(Pt::new(90.0, 0.0)));
    c.set_ext_end(HeadExt::CrowFootOne);
    c.set_ext_start(HeadExt::DiamondOpen);
    let object = Object::new(Kind::Connector(c), Style::default());
    let op = WireOp::Put { page: Id("p".into()), object: Box::new(object.clone()) };
    let json = serde_json::to_string(&op).unwrap();
    let back: WireOp = serde_json::from_str(&json).unwrap();
    assert_eq!(back, op, "the whole object, extended heads included, makes the trip");
    // a message written by a version that has no extended head has none of the keys: it reads, with no extended head
    fn strip(v: &mut serde_json::Value) {
        match v {
            serde_json::Value::Object(m) => {
                m.remove("head_start_ext");
                m.remove("head_end_ext");
                m.values_mut().for_each(strip);
            }
            serde_json::Value::Array(l) => l.iter_mut().for_each(strip),
            _ => {}
        }
    }
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    strip(&mut value);
    assert!(!value.to_string().contains("_ext"));
    let WireOp::Put { object: old, .. } = serde_json::from_value(value).unwrap() else { panic!("a put") };
    let Kind::Connector(read) = &old.kind else { panic!("a connector") };
    assert_eq!((read.head_start_ext, read.head_end_ext), (None, None));
    assert_eq!((read.head_start, read.head_end), (HeadExt::DiamondOpen.fallback(), HeadExt::CrowFootOne.fallback()), "the original fields hold the fallback");
}
