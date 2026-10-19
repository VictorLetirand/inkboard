//! Native boards are recognized under every extension (`.inkboard`, `.inkboard.json`, `.tableau.json`) and by content alone.

use inkboard_core::{ApproxMeasurer, Document};
use inkboard_formats::{Format, detect, import_any};

fn native_json() -> String {
    inkboard_core::serial::to_json(&Document::new("Mine"), false)
}

#[test]
fn every_native_extension_opens_as_native() {
    let json = native_json();
    for name in ["a.inkboard", "a.inkboard.json", "a.tableau.json", "A.INKBOARD", "dir/with space.inkboard"] {
        assert_eq!(detect(name, &json), Some(Format::Native), "{name}");
        let imp = import_any(name, &json, &ApproxMeasurer).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(imp.doc.name, "Mine", "{name}");
    }
}

#[test]
fn native_content_is_recognized_whatever_the_name() {
    let json = native_json();
    for name in ["board.json", "board", "board.txt", "board.bak"] {
        assert_eq!(detect(name, &json), Some(Format::Native), "{name}");
    }
    // pre-rename identifier still read
    let old = json.replacen("\"inkboard\"", "\"tableau-blanc\"", 1);
    assert_ne!(old, json);
    assert_eq!(detect("old.json", &old), Some(Format::Native));
}

#[test]
fn foreign_content_under_a_native_name_is_not_native() {
    let foreign = r#"{"format":"other","version":1,"document":{"name":"x","pages":[]}}"#;
    assert_ne!(detect("x.inkboard", foreign), Some(Format::Native));
    assert!(import_any("x.inkboard", foreign, &ApproxMeasurer).is_err());
}
