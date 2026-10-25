//! Real files from public repositories (provenance and licenses: `tests/corpus/SOURCES.md`).
//! The import must never panic, must succeed on the supported formats and must not lose a label in silence.

use base64::{Engine, engine::general_purpose::STANDARD};
use flate2::read::DeflateDecoder;
use inkboard_core::{ApproxMeasurer, Kind};
use inkboard_formats::{FormatError, Imported, import_any, import_any_with_icons};
use regex::Regex;
use serde_json::Value;
use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

/// Mermaid diagram types that Inkboard does not handle: the import must return an error.
const UNSUPPORTED_MERMAID: [&str; 2] = ["gantt", "pie"];

/// `(file name, content)` of every file of a corpus folder, sorted by name.
fn files(format: &str) -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/corpus").join(format);
    let mut out: Vec<(String, String)> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.expect("directory entry").path())
        .map(|p| {
            (p.file_name().expect("file name").to_string_lossy().into_owned(), std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display())))
        })
        .collect();
    out.sort();
    out
}

/// `import_any`, which must never panic whatever the file.
fn import(name: &str, content: &str) -> Result<Imported, FormatError> {
    catch_unwind(AssertUnwindSafe(|| import_any(name, content, &ApproxMeasurer))).unwrap_or_else(|_| panic!("import_any panicked on {name}"))
}

/// Same as [`import`], with the icon library of the application (resolves the built-in icons of draw.io files).
fn import_with_library(name: &str, content: &str) -> Result<Imported, FormatError> {
    let lib = inkboard_icons::Library::open(include_bytes!("../../inkboard-app/assets/icons.bin")).expect("icon pack");
    catch_unwind(AssertUnwindSafe(|| import_any_with_icons(name, content, &ApproxMeasurer, &lib))).unwrap_or_else(|_| panic!("import panicked on {name}"))
}

fn blank(s: &str) -> bool {
    s.trim().is_empty()
}

/// Objects that carry a non-blank text: shapes, free texts, connector labels and table cells.
fn labelled(imp: &Imported) -> usize {
    imp.doc
        .pages
        .iter()
        .flat_map(|p| &p.objects)
        .map(|o| match &o.kind {
            Kind::Box(b) => usize::from(!blank(&b.label.text.to_plain())),
            Kind::Connector(c) => usize::from(c.label.as_ref().is_some_and(|l| !blank(&l.text.to_plain()))),
            Kind::Table(t) => t.cells.iter().filter(|c| !blank(&c.to_plain())).count(),
            _ => 0,
        })
        .sum()
}

/// Checks common to every imported file: something was imported and no geometry is NaN or infinite.
fn check_common(name: &str, imp: &Imported, bad: &mut Vec<String>) {
    let objects = imp.doc.pages.iter().flat_map(|p| &p.objects);
    if objects.clone().count() == 0 {
        bad.push(format!("{name}: no object imported"));
    }
    for f in objects.filter_map(inkboard_core::Object::frame) {
        if ![f.x, f.y, f.w, f.h, f.angle].iter().all(|v| v.is_finite()) {
            bad.push(format!("{name}: non-finite frame {f:?}"));
        }
    }
}

fn assert_no_problem(bad: &[String]) {
    assert!(bad.is_empty(), "{} problem(s):\n{}", bad.len(), bad.join("\n"));
}

/// The import must give back at most the labels of the source, and miss one only when the import reports it:
/// each skipped element can explain one missing label (`Report::skipped`). draw.io is stricter (see its test).
fn compare_labels(name: &str, source: usize, imp: &Imported, bad: &mut Vec<String>) {
    let got = labelled(imp);
    if got > source || source - got > imp.report.skipped {
        bad.push(format!("{name}: {source} non-empty label(s) in the source, {got} object(s) with a text after import, {} skipped", imp.report.skipped));
    }
}

/* ----------------------------------------------------------- draw.io */

/// Decompresses the body of a compressed `<diagram>` (base64, raw deflate, URL-encoded XML).
fn inflate(raw: &str) -> String {
    let bytes = STANDARD.decode(raw.split_whitespace().collect::<String>().as_bytes()).expect("base64 diagram");
    let mut s = String::new();
    DeflateDecoder::new(bytes.as_slice()).read_to_string(&mut s).expect("deflate diagram");
    percent_encoding::percent_decode_str(&s).decode_utf8_lossy().into_owned()
}

/// Text actually shown for a draw.io `value` attribute: tags removed, entities decoded.
fn visible_text(raw: &str) -> String {
    let unescape = |s: &str| {
        let s = s.replace("&nbsp;", " ").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&");
        let num = Regex::new(r"&#(x?)([0-9a-fA-F]+);").unwrap();
        num.replace_all(&s, |c: &regex::Captures| {
            let radix = if c[1].is_empty() { 10 } else { 16 };
            u32::from_str_radix(&c[2], radix).ok().and_then(char::from_u32).map_or(String::new(), String::from)
        })
        .into_owned()
    };
    let tags = Regex::new(r"<[^>]*>").unwrap();
    unescape(&tags.replace_all(&unescape(raw), " "))
}

/// Number of cells (vertices and edges) with a non-empty `value` (or `label` on `object` wrappers).
fn drawio_labels(src: &str) -> usize {
    let mut all = src.to_owned();
    let diagram = Regex::new(r"(?s)<diagram\b[^>]*>([^<]+)</diagram>").unwrap();
    for c in diagram.captures_iter(src) {
        all.push_str(&inflate(&c[1]));
    }
    let value = Regex::new(r#"<(?:mxCell|object|UserObject)\b[^>]*?\s(?:value|label)="([^"]*)""#).unwrap();
    value.captures_iter(&all).filter(|c| !blank(&visible_text(&c[1]))).count()
}

#[test]
fn drawio_files_import_without_losing_labels() {
    let mut bad = Vec::new();
    for (name, src) in files("drawio") {
        match import_with_library(&name, &src) {
            Ok(imp) => {
                check_common(&name, &imp, &mut bad);
                // a built-in icon (`image=img/lib/...`) keeps its label, found in the library or not
                let (source, got) = (drawio_labels(&src), labelled(&imp));
                if got != source {
                    bad.push(format!("{name}: {source} label(s) in the source, {got} after import (report: {:?})", imp.report));
                }
                // the same file without the library: every icon is a placeholder, no label lost either
                let plain = import(&name, &src).expect("import without library");
                if labelled(&plain) != source || plain.report.skipped != imp.report.skipped {
                    bad.push(format!("{name}: without the library, {} label(s) of {source}, report {:?}", labelled(&plain), plain.report));
                }
            }
            Err(e) => bad.push(format!("{name}: {e}")),
        }
    }
    assert_no_problem(&bad);
}

/// Built-in draw.io icons of the corpus (`image=img/lib/...`): how many exist and how many the library resolves.
#[test]
fn drawio_corpus_built_in_icons_are_resolved_by_the_library() {
    let (mut total, mut placeholders) = (0, 0);
    let re = Regex::new(r#"image=(img/lib/[^;&"]*)"#).unwrap();
    let diagram = Regex::new(r"(?s)<diagram\b[^>]*>([^<]+)</diagram>").unwrap();
    for (name, src) in files("drawio") {
        let mut all = src.clone();
        for c in diagram.captures_iter(&src) {
            all.push_str(&inflate(&c[1]));
        }
        total += re.captures_iter(&all).count();
        placeholders += import_with_library(&name, &src).expect("import").report.placeholders;
    }
    assert_eq!(total, 5, "built-in icons in the corpus");
    assert_eq!(placeholders, 0, "every one of them is in the icon library");
}

/* -------------------------------------------------------- Excalidraw */

/// Non-deleted `text` elements with a non-empty text, plus the named frames (imported as a title).
fn excalidraw_labels(src: &str) -> usize {
    let v: Value = serde_json::from_str(src).expect("JSON");
    v["elements"]
        .as_array()
        .expect("elements")
        .iter()
        .filter(|e| e["isDeleted"] != true)
        .filter(|e| match e["type"].as_str() {
            Some("text") => !blank(e["originalText"].as_str().or(e["text"].as_str()).unwrap_or("")),
            Some("frame" | "magicframe") => !blank(e["name"].as_str().unwrap_or("")),
            _ => false,
        })
        .count()
}

#[test]
fn excalidraw_files_import_without_losing_labels() {
    let mut bad = Vec::new();
    for (name, src) in files("excalidraw") {
        match import(&name, &src) {
            Ok(imp) => {
                check_common(&name, &imp, &mut bad);
                compare_labels(&name, excalidraw_labels(&src), &imp, &mut bad);
            }
            Err(e) => bad.push(format!("{name}: {e}")),
        }
    }
    assert_no_problem(&bad);
}

/* ------------------------------------------------------- JSON Canvas */

/// `(labels, skipped)` expected for a canvas: text, file and link nodes and group titles carry a text;
/// an edge is kept (with its label) only when both ends are text, file or link nodes.
fn canvas_expected(src: &str) -> (usize, usize) {
    let v: Value = serde_json::from_str(src).expect("JSON");
    let (mut labels, mut skipped) = (0, 0);
    let mut ids = Vec::new();
    for n in v["nodes"].as_array().expect("nodes") {
        let str_of = |k: &str| n[k].as_str().unwrap_or("").to_owned();
        match n["type"].as_str().unwrap_or("text") {
            // a file node always shows its path
            "file" => {
                labels += 1;
                ids.extend(n["id"].as_str());
            }
            "text" | "link" => {
                labels += usize::from(!blank(&str_of(if n["type"] == "link" { "url" } else { "text" })));
                ids.extend(n["id"].as_str());
            }
            "group" => labels += usize::from(!blank(&str_of("label"))),
            _ => skipped += 1,
        }
    }
    for e in v["edges"].as_array().map(Vec::as_slice).unwrap_or_default() {
        if e["fromNode"].as_str().is_some_and(|f| ids.contains(&f)) && e["toNode"].as_str().is_some_and(|t| ids.contains(&t)) {
            labels += usize::from(!blank(e["label"].as_str().unwrap_or("")));
        } else {
            skipped += 1;
        }
    }
    (labels, skipped)
}

#[test]
fn canvas_files_import_without_losing_labels() {
    let mut bad = Vec::new();
    for (name, src) in files("canvas") {
        match import(&name, &src) {
            Ok(imp) => {
                check_common(&name, &imp, &mut bad);
                let (labels, skipped) = canvas_expected(&src);
                compare_labels(&name, labels, &imp, &mut bad);
                if imp.report.skipped != skipped {
                    bad.push(format!("{name}: {skipped} element(s) cannot be imported, the report says {}", imp.report.skipped));
                }
            }
            Err(e) => bad.push(format!("{name}: {e}")),
        }
    }
    assert_no_problem(&bad);
}

/* ----------------------------------------------------------- Mermaid */

#[test]
fn mermaid_supported_types_import_and_the_others_are_errors() {
    let (mut bad, mut errors) = (Vec::new(), 0);
    for (name, src) in files("mermaid") {
        let result = import(&name, &src);
        if UNSUPPORTED_MERMAID.iter().any(|t| name.starts_with(t)) {
            match result {
                Err(e) if !e.to_string().is_empty() => errors += 1,
                Err(_) => bad.push(format!("{name}: empty error message")),
                Ok(_) => bad.push(format!("{name}: unsupported diagram type accepted")),
            }
            continue;
        }
        match result {
            Ok(imp) => {
                check_common(&name, &imp, &mut bad);
                if labelled(&imp) == 0 {
                    bad.push(format!("{name}: no object with a text"));
                }
            }
            Err(e) => bad.push(format!("{name}: {e}")),
        }
    }
    assert_eq!(errors, 4, "the corpus holds 2 gantt and 2 pie files");
    assert_no_problem(&bad);
}

#[test]
fn corpus_holds_5_to_15_files_per_format() {
    for format in ["mermaid", "drawio", "excalidraw", "canvas"] {
        let n = files(format).len();
        assert!((5..=15).contains(&n), "{format}: {n} file(s)");
    }
}
