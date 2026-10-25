//! Data safety of the native format: damaged files are refused with a readable error (never a panic),
//! a failed or interrupted save never touches the existing file.

use inkboard_core::connector::{Connector, End};
use inkboard_core::serial::{self, LoadError};
use inkboard_core::*;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Temporary folder under `target/`, removed on drop (also restores permissions so that it can be removed).
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("durability-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).expect("create the temporary folder");
        TempDir(dir)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if let Ok(meta) = std::fs::metadata(&self.0) {
            let mut perms = meta.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            let _ = std::fs::set_permissions(&self.0, perms);
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A document with one object of each kind that matters to the file (text, stroke, bound connector, image).
fn sample(title: &str) -> Document {
    let mut d = Document::new(title);
    let mut label = Label::new("H\u{e9}llo w\u{f6}rld \u{2014} \u{2713}", 22.0, Align::Center);
    label.text.restyle(0, 3, |s| s.bold = true);
    let a = Object::new_box(BoxGeom::Rect, Frame::new(0.0, 0.0, 120.0, 60.0), Style::default(), label);
    let b = Object::new_box(BoxGeom::Ellipse, Frame::new(300.0, 0.0, 100.0, 80.0), Style::default(), Label::default());
    let c = Object::new(Kind::Connector(Connector::new(End::bound(a.id.clone()), End::bound(b.id.clone()))), Style::default());
    let pen = Object::new_pen_with_pressure(vec![Pt::new(0.0, 0.0), Pt::new(5.0, 5.0), Pt::new(9.0, 2.0)], vec![0.1, 0.8, 0.5], false, Style::default());
    let asset = d.add_asset("image/png", vec![1, 2, 3, 4]);
    let img = Object::new(Kind::Image(ImageObj { frame: Frame::new(500.0, 0.0, 40.0, 40.0), asset, latex: None, mono: false }), Style::default());
    d.pages[0].objects.extend([a, b, c, pen, img]);
    d
}

/// The message of an error must be readable: non-empty, on one line.
fn assert_readable(e: &LoadError) {
    let msg = e.to_string();
    assert!(!msg.trim().is_empty(), "empty error message");
    assert!(!msg.contains('\n'), "multi-line error message: {msg:?}");
}

/// Message of a caught panic (`panic!` payloads are `&str` or `String`).
fn panic_message(p: &(dyn std::any::Any + Send)) -> &str {
    p.downcast_ref::<&str>().copied().or_else(|| p.downcast_ref::<String>().map(String::as_str)).unwrap_or("non-string payload")
}

fn assert_rejected(json: &str, what: &str) -> LoadError {
    match std::panic::catch_unwind(|| serial::from_json(json)) {
        Err(p) => panic!("{what}: from_json panicked: {}", panic_message(&*p)),
        Ok(Ok(_)) => panic!("{what}: a damaged file was accepted"),
        Ok(Err(e)) => {
            assert_readable(&e);
            e
        }
    }
}

#[test]
fn truncated_files_are_refused_at_every_cut_point() {
    let dir = TempDir::new();
    let doc = sample("truncate");
    for pretty in [false, true] {
        let full = serial::to_json(&doc, pretty).into_bytes();
        assert_eq!(serial::from_json(std::str::from_utf8(&full).unwrap()).unwrap(), doc, "the reference file must load");
        // every strict prefix, in memory (a cut inside a multi-byte character becomes a replacement character)
        for len in 0..full.len() {
            let text = String::from_utf8_lossy(&full[..len]).into_owned();
            assert_rejected(&text, &format!("a text cut at {len}/{} bytes", full.len()));
        }
        // and through the disk (reading as UTF-8 may fail first), at 40 cut points
        let path = dir.path("cut.inkboard.json");
        for k in 0..40 {
            let len = full.len() * k / 40;
            std::fs::write(&path, &full[..len]).unwrap();
            match std::panic::catch_unwind(|| serial::load(&path)) {
                Err(p) => panic!("load panicked on a file cut at {len}/{} bytes: {}", full.len(), panic_message(&*p)),
                Ok(Ok(_)) => panic!("a file cut at {len}/{} bytes was accepted", full.len()),
                Ok(Err(e)) => assert_readable(&e),
            }
        }
    }
}

#[test]
fn empty_and_non_json_files_are_refused() {
    for (what, text) in [("empty", ""), ("spaces", "   \n"), ("text", "hello"), ("array", "[]"), ("null", "null"), ("number", "42"), ("html", "<html></html>")]
    {
        let e = assert_rejected(text, what);
        assert!(matches!(e, LoadError::Json(_)), "{what}: expected a JSON error, got {e}");
    }
}

#[test]
fn bad_versions_and_formats_are_refused_with_a_clear_message() {
    let envelope = |format: Value, version: Value| json!({ "format": format, "version": version, "document": { "name": "x", "pages": [] } }).to_string();
    let e = assert_rejected(&envelope(json!("inkboard"), json!(serial::VERSION + 1)), "newer version");
    assert!(matches!(e, LoadError::TooNew(v) if v == serial::VERSION + 1), "got {e}");
    assert!(e.to_string().contains("newer version"), "the message must say why: {e}");
    let e = assert_rejected(&envelope(json!("inkboard"), json!(u32::MAX)), "huge version");
    assert!(matches!(e, LoadError::TooNew(u32::MAX)), "got {e}");
    let e = assert_rejected(&envelope(json!("excalidraw"), json!(2)), "foreign format");
    assert!(matches!(&e, LoadError::WrongFormat(f) if f == "excalidraw"), "got {e}");
    // a version that does not fit the field is a JSON error, not a panic
    for (what, version) in
        [("negative", json!(-1)), ("too large", json!(u64::from(u32::MAX) + 1)), ("string", json!("2")), ("float", json!(2.5)), ("null", Value::Null)]
    {
        let e = assert_rejected(&envelope(json!("inkboard"), version), what);
        assert!(matches!(e, LoadError::Json(_)), "{what}: got {e}");
    }
    let e = assert_rejected(&envelope(json!(7), json!(2)), "format of the wrong type");
    assert!(matches!(e, LoadError::Json(_)), "got {e}");
    // missing fields
    for missing in ["format", "version", "document"] {
        let mut v = json!({ "format": "inkboard", "version": 2, "document": { "name": "x", "pages": [] } });
        v.as_object_mut().unwrap().remove(missing);
        let e = assert_rejected(&v.to_string(), missing);
        assert!(matches!(e, LoadError::Json(_)), "missing {missing}: got {e}");
        assert!(e.to_string().contains(missing), "the message must name the missing field: {e}");
    }
}

#[test]
fn wrongly_typed_content_is_refused() {
    let good: Value = serde_json::from_str(&serial::to_json(&sample("types"), false)).unwrap();
    for (what, path, bad) in [
        ("document is a number", "/document", json!(5)),
        ("pages is an object", "/document/pages", json!({})),
        ("pages is a string", "/document/pages", json!("none")),
        ("object is a number", "/document/pages/0/objects/0", json!(1)),
        ("frame is a string", "/document/pages/0/objects/0/kind/frame", json!("big")),
        ("unknown object kind", "/document/pages/0/objects/0/kind", json!({ "type": "hologram" })),
        ("style is a string", "/document/pages/0/objects/0/style", json!("bold")),
        ("kind is a number", "/document/pages/0/objects/0/kind", json!(3)),
        ("assets is a list", "/document/assets", json!([1, 2])),
        ("name is a list", "/document/name", json!([])),
    ] {
        let mut v = good.clone();
        match v.pointer_mut(path) {
            Some(slot) => *slot = bad,
            None => panic!("{what}: the path {path} does not exist in a saved document (test out of date)"),
        }
        assert_rejected(&v.to_string(), what);
    }
}

/// Fields written by a newer minor version must not make the board unreadable, nor be mistaken for data.
#[test]
fn unknown_fields_are_ignored_without_loss() {
    let doc = sample("unknown fields");
    let mut v: Value = serde_json::from_str(&serial::to_json(&doc, false)).unwrap();
    let add = |slot: &mut Value| {
        slot.as_object_mut().expect("an object").insert("zz_unknown".into(), json!({ "from_the_future": [1, 2, { "deep": null }], "flag": true }));
    };
    add(&mut v);
    add(v.pointer_mut("/document").unwrap());
    for page in v.pointer_mut("/document/pages").unwrap().as_array_mut().unwrap() {
        add(page);
        for o in page.pointer_mut("/objects").unwrap().as_array_mut().unwrap() {
            add(o);
        }
    }
    let back = serial::from_json(&v.to_string()).expect("unknown fields must be tolerated");
    assert_eq!(back, doc, "unknown fields must not change what is loaded");
}

#[test]
fn random_corruption_never_panics() {
    // fixed seed (xorshift): the same 400 corruptions on every run
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut rnd = move |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        usize::try_from(seed % n as u64).expect("below n")
    };
    let full = serial::to_json(&sample("fuzz"), false).into_bytes();
    let structural = br#"{}[]":,0-"#;
    for i in 0..400 {
        let mut bytes = full.clone();
        let at = rnd(full.len());
        let span_end = (at + 1 + rnd(40)).min(full.len());
        match i % 4 {
            0 => bytes[at] ^= 1 << rnd(8),                      // flip one bit
            1 => bytes[at] = structural[rnd(structural.len())], // replace by a structural character
            2 => {
                bytes.drain(at..span_end); // delete a span
            }
            _ => {
                let dup = bytes[at..span_end].to_vec();
                bytes.splice(at..at, dup); // duplicate a span
            }
        }
        let text = String::from_utf8_lossy(&bytes).into_owned();
        // either outcome is acceptable (a flipped digit can still be a valid board); a panic is not
        match std::panic::catch_unwind(|| serial::from_json(&text)) {
            Err(p) => panic!("from_json panicked on corruption #{i}: {}: {text}", panic_message(&*p)),
            Ok(Err(e)) => assert_readable(&e),
            Ok(Ok(_)) => {}
        }
    }
}

#[test]
fn saving_into_an_unwritable_place_fails_and_keeps_the_existing_file() {
    let dir = TempDir::new();
    let target = dir.path("board.inkboard.json");
    let old = sample("old");
    serial::save(&old, &target).unwrap();
    let old_bytes = std::fs::read(&target).unwrap();
    let new = sample("new");

    // The temporary file cannot be created: a folder holds its name. Fails on every system, whatever the user's rights.
    let tmp = target.with_extension("tmp-save");
    std::fs::create_dir(&tmp).unwrap();
    let err = serial::save(&new, &target).expect_err("saving must fail when the temporary file cannot be written");
    assert!(matches!(err, LoadError::Io(_)), "got {err}");
    assert_readable(&err);
    assert_eq!(std::fs::read(&target).unwrap(), old_bytes, "the existing file must be untouched");
    assert_eq!(serial::load(&target).unwrap(), old);
    std::fs::remove_dir(&tmp).unwrap();

    // The destination folder is read-only. Not reliable everywhere (Windows ignores the flag on folders; root ignores permissions):
    // when the folder stays writable the test is skipped, not failed.
    let mut perms = std::fs::metadata(&dir.0).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&dir.0, perms).unwrap();
    let probe = dir.path("probe");
    if std::fs::write(&probe, b"x").is_ok() {
        eprintln!("SKIPPED read-only folder: the folder is still writable on this system (Windows or elevated user)");
        return;
    }
    let err = serial::save(&new, &target).expect_err("saving into a read-only folder must fail");
    assert!(matches!(err, LoadError::Io(_)), "got {err}");
    assert_readable(&err);
    assert_eq!(std::fs::read(&target).unwrap(), old_bytes, "the existing file must be untouched");
    assert!(!tmp.exists(), "no temporary file must remain");
}

#[test]
fn a_new_file_in_a_missing_folder_fails_cleanly() {
    let dir = TempDir::new();
    let target = dir.path("missing-folder").join("board.inkboard.json");
    let err = serial::save(&sample("x"), &target).expect_err("the folder does not exist");
    assert!(matches!(err, LoadError::Io(_)), "got {err}");
    assert!(!target.exists());
}

/// A crash between writing the temporary file and renaming it leaves `*.tmp-save` behind, possibly half written.
#[test]
fn a_leftover_temporary_file_does_not_affect_the_target() {
    let dir = TempDir::new();
    let target = dir.path("board.inkboard.json");
    let tmp = target.with_extension("tmp-save");
    let old = sample("old");
    serial::save(&old, &target).unwrap();
    let new_json = serial::to_json(&sample("new"), false);
    for leftover in [new_json.as_bytes()[..new_json.len() / 2].to_vec(), Vec::new(), new_json.clone().into_bytes(), vec![0u8; 64]] {
        std::fs::write(&tmp, &leftover).unwrap();
        assert_eq!(serial::load(&target).unwrap(), old, "the target must stay readable and unchanged");
        // the next save reuses the name of the leftover and still succeeds
        let next = sample("next");
        serial::save(&next, &target).unwrap();
        assert!(!tmp.exists(), "a successful save consumes the temporary file");
        assert_eq!(serial::load(&target).unwrap(), next);
        serial::save(&old, &target).unwrap();
    }
    // no target yet, only a leftover: opening is an error, not a panic
    let lonely = dir.path("lonely.inkboard.json");
    std::fs::write(lonely.with_extension("tmp-save"), &new_json[..new_json.len() / 2]).unwrap();
    let err = serial::load(&lonely).expect_err("there is no board here");
    assert!(matches!(err, LoadError::Io(_)), "got {err}");
    assert_readable(&err);
}
