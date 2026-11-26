//! Native file format (versioned JSON) and atomic saving.

use crate::connector::{Arrowhead, End};
use crate::model::{Document, Id, Kind, Label, Page, VAlign};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use thiserror::Error;

pub const FORMAT: &str = "inkboard";
/// Format identifier written by the pre-rename alpha builds; still accepted when reading.
const LEGACY_FORMAT: &str = "tableau-blanc";

/// Is `format` an identifier of the native format (current or pre-rename)?
#[must_use]
pub fn is_native_format(format: &str) -> bool {
    format == FORMAT || format == LEGACY_FORMAT
}
/// Highest version read. Written only when the board needs it (see [`required_version`]); otherwise 2.
pub const VERSION: u32 = 3;
/// Version written by a board that uses none of the fields added after it.
const BASE_VERSION: u32 = 2;

/// Extension given to new native files (`board.inkboard`).
pub const NATIVE_EXT: &str = "inkboard";

/// Is `file_name` (a name or a path) the name of a native file? `.inkboard` (current), `.inkboard.json` and `.tableau.json` (older
/// names, still read and kept as is when reopened and saved). Case-insensitive.
#[must_use]
pub fn is_native_name(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    [".inkboard", ".inkboard.json", ".tableau.json"].iter().any(|ext| lower.ends_with(ext))
}

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("this file is not an Inkboard board (format \"{0}\")")]
    WrongFormat(String),
    #[error("file created by a newer version (v{0}); update the application")]
    TooNew(u32),
    #[error("cannot read or write: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Serialize)]
struct EnvelopeOut<'a> {
    format: &'static str,
    version: u32,
    document: &'a Document,
}

#[derive(Deserialize)]
struct EnvelopeIn {
    format: String,
    version: u32,
    document: Document,
}

/// Serializes a document in the native format (`pretty`: indented).
///
/// # Panics
///
/// Never in practice: every type of the document serializes (string map keys, no fallible `Serialize`).
#[must_use]
pub fn to_json(doc: &Document, pretty: bool) -> String {
    let env = EnvelopeOut { format: FORMAT, version: required_version(doc), document: doc };
    let r = if pretty { serde_json::to_string_pretty(&env) } else { serde_json::to_string(&env) };
    r.expect("serializing a document cannot fail")
}

/// Version a document must be written with: 3 when it uses a field added after version 2, otherwise 2 (the file is then identical to before).
///
/// Every optional field added to the model must be listed here (same predicate as its `skip_serializing_if`).
#[must_use]
#[allow(clippy::float_cmp, reason = "exact comparison on purpose: only the untouched default is left out of the file")]
pub fn required_version(doc: &Document) -> u32 {
    let label_needs_v3 = |l: &Label| l.valign != VAlign::Middle;
    let side_needs_v3 = |e: &End| e.side().is_some();
    let needs_v3 = doc.pages.iter().flat_map(|p| p.objects.iter()).any(|o| {
        o.style.shadow
            || match &o.kind {
                Kind::Box(b) => label_needs_v3(&b.label),
                Kind::Pen(p) => p.head_end != Arrowhead::None,
                Kind::Connector(c) => {
                    c.head_scale != 1.0
                        || c.label_pos.is_some()
                        || c.label_offset != 0.0
                        || side_needs_v3(&c.start)
                        || side_needs_v3(&c.end)
                        || c.label.as_ref().is_some_and(label_needs_v3)
                }
                Kind::Image(i) => !i.recolor.is_empty(),
                Kind::Table(_) => false,
            }
    });
    if needs_v3 { VERSION } else { BASE_VERSION }
}

/// Removes a UTF-8 byte order mark (written by Notepad and PowerShell 5): neither JSON nor the text formats accept it.
#[must_use]
pub fn strip_bom(s: &str) -> &str {
    s.strip_prefix('\u{feff}').unwrap_or(s)
}

/// Reads a document in the native format (or an older one).
///
/// # Errors
///
/// [`LoadError`] when the text is not JSON, not an Inkboard board, or written by a newer version.
pub fn from_json(s: &str) -> Result<Document, LoadError> {
    let env: EnvelopeIn = serde_json::from_str(strip_bom(s))?;
    if !is_native_format(&env.format) {
        return Err(LoadError::WrongFormat(env.format));
    }
    if env.version > VERSION {
        return Err(LoadError::TooNew(env.version));
    }
    let mut doc = env.document;
    sanitize(&mut doc);
    Ok(doc)
}

/// Makes a document consistent: at least one page, unique ids,
/// no orphan connector, "main" layer present.
pub fn sanitize(doc: &mut Document) {
    if doc.pages.is_empty() {
        doc.pages.push(Page::new("Page 1"));
    }
    for page in &mut doc.pages {
        if page.layers.is_empty() {
            page.layers.push(crate::model::Layer::main());
        }
        let layer_ids: Vec<Id> = page.layers.iter().map(|l| l.id.clone()).collect();
        let mut seen: HashSet<Id> = HashSet::new();
        page.objects.retain(|o| seen.insert(o.id.clone()));
        let main = page.layers[0].id.clone();
        for o in &mut page.objects {
            if !layer_ids.contains(&o.layer) {
                o.layer = main.clone();
            }
        }
        let ids: HashSet<Id> = page.objects.iter().map(|o| o.id.clone()).collect();
        page.objects.retain(|o| match &o.kind {
            Kind::Connector(c) => c.targets().all(|t| ids.contains(t)),
            _ => true,
        });
    }
    let used: HashSet<Id> =
        doc.pages.iter().flat_map(|p| p.objects.iter()).filter_map(|o| if let Kind::Image(i) = &o.kind { Some(i.asset.clone()) } else { None }).collect();
    for id in used {
        if !doc.assets.contains_key(&id) {
            // image without an asset: remove it rather than leave a broken object
            for p in &mut doc.pages {
                p.objects.retain(|o| !matches!(&o.kind, Kind::Image(i) if i.asset == id));
            }
        }
    }
}

/// Writes through a temporary file, flushed to disk, then a rename: a crash never leaves a truncated file.
///
/// # Errors
///
/// The I/O error of the creation, write, flush or rename; the target file is then left untouched.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("tmp-save");
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    #[allow(clippy::drop_non_drop, reason = "on wasm32 a `File` has no destructor (the call is then only a marker of where the file is closed)")]
    drop(f);
    std::fs::rename(&tmp, path)
}

/// Saves atomically (see [`write_atomic`]).
///
/// # Errors
///
/// [`LoadError::Io`] when the file cannot be written.
pub fn save(doc: &Document, path: &Path) -> Result<(), LoadError> {
    write_atomic(path, to_json(doc, false).as_bytes())?;
    Ok(())
}

/// Reads a board file.
///
/// # Errors
///
/// [`LoadError`] when the file cannot be read or its content is not a valid board (see [`from_json`]).
pub fn load(path: &Path) -> Result<Document, LoadError> {
    from_json(&std::fs::read_to_string(path)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connector::{Connector, End};
    use crate::geom::{Frame, Pt};
    use crate::model::{BoxGeom, Label, Object};
    use crate::style::{Color, Style};
    use crate::text::RichText;

    fn sample() -> Document {
        let mut d = Document::default();
        let mut label = Label::new("Hello", 22.0, crate::style::Align::Center);
        label.text.restyle(0, 3, |s| {
            s.bold = true;
            s.highlight = Some(Color::rgb(255, 236, 153));
        });
        let a = Object::new_box(
            BoxGeom::Rect,
            Frame::new(0.0, 0.0, 120.0, 60.0),
            Style { fill: Some(Color::rgb(165, 216, 255)), roundness: 0.4, ..Default::default() },
            label,
        );
        let b = Object::new_box(BoxGeom::Diamond, Frame { x: 300.0, y: 0.0, w: 100.0, h: 80.0, angle: 0.3 }, Style::default(), Label::default());
        let c = Object::new(Kind::Connector(Connector::new(End::bound(a.id.clone()), End::bound(b.id.clone()))), Style::default());
        let pen = Object::new_pen(vec![Pt::new(0.0, 100.0), Pt::new(10.0, 120.0), Pt::new(30.0, 90.0)], true, Style::default());
        let pressured =
            Object::new_pen_with_pressure(vec![Pt::new(0.0, 0.0), Pt::new(5.0, 5.0), Pt::new(9.0, 2.0)], vec![0.1, 0.8, 0.5], false, Style::default());
        d.pages[0].objects.push(pressured);
        let aid = d.add_asset("image/png", vec![1, 2, 3, 4]);
        let img = Object::new(
            Kind::Image(crate::model::ImageObj { frame: Frame::new(500.0, 0.0, 40.0, 40.0), asset: aid, latex: None, mono: false, recolor: Vec::new() }),
            Style::default(),
        );
        d.pages[0].objects.extend([a, b, c, pen, img]);
        d
    }

    #[test]
    fn json_roundtrip_is_lossless() {
        let d = sample();
        let back = from_json(&to_json(&d, false)).unwrap();
        assert_eq!(d, back);
        let back2 = from_json(&to_json(&d, true)).unwrap();
        assert_eq!(d, back2);
    }

    #[test]
    fn rejects_foreign_and_future_files() {
        assert!(matches!(from_json(r#"{"format":"other","version":1,"document":{"name":"x","pages":[]}}"#), Err(LoadError::WrongFormat(_))));
        assert!(
            from_json(r#"{"format":"tableau-blanc","version":2,"document":{"name":"old","pages":[]}}"#).is_ok(),
            "boards saved before the rename still open"
        );
        assert!(matches!(from_json(r#"{"format":"inkboard","version":99,"document":{"name":"x","pages":[]}}"#), Err(LoadError::TooNew(99))));
        assert!(matches!(from_json("not json"), Err(LoadError::Json(_))));
    }

    fn one_object(o: Object) -> Document {
        let mut d = Document::default();
        d.pages[0].objects.push(o);
        d
    }

    fn version_of(d: &Document) -> String {
        let v: serde_json::Value = serde_json::from_str(&to_json(d, false)).unwrap();
        v["version"].to_string()
    }

    #[test]
    fn plain_board_is_written_as_version_2() {
        let mut d = sample();
        assert!(to_json(&d, false).starts_with(r#"{"format":"inkboard","version":2,"#));
        d.pages[0].objects.clear();
        assert_eq!(version_of(&d), "2");
    }

    #[test]
    fn each_new_field_requires_version_3() {
        use crate::connector::AttachSide;
        let boxed = |style: Style, label: Label| Object::new_box(BoxGeom::Rect, Frame::new(0.0, 0.0, 10.0, 10.0), style, label);
        let conn = |f: &dyn Fn(&mut Connector)| {
            let mut c = Connector::new(End::free(Pt::new(0.0, 0.0)), End::free(Pt::new(5.0, 5.0)));
            f(&mut c);
            one_object(Object::new(Kind::Connector(c), Style::default()))
        };
        let valign = Label { valign: VAlign::Top, ..Label::default() };
        let mut pen = Object::new_pen(vec![Pt::new(0.0, 0.0), Pt::new(5.0, 5.0)], false, Style::default());
        if let Kind::Pen(p) = &mut pen.kind {
            p.head_end = Arrowhead::Arrow;
        }
        let mut img = sample().pages[0].objects.pop().unwrap();
        if let Kind::Image(i) = &mut img.kind {
            i.recolor = vec![(Color::rgb(1, 2, 3), Color::rgb(4, 5, 6))];
        }
        let cases: Vec<(&str, Document)> = vec![
            ("recolor", one_object(img)),
            ("head_scale", conn(&|c| c.head_scale = 2.0)),
            ("shadow", one_object(boxed(Style { shadow: true, ..Style::default() }, Label::default()))),
            ("pen head_end", one_object(pen)),
            ("side", conn(&|c| c.start = End::Bound { target: Id::from("x"), anchor: Default::default(), side: Some(AttachSide::Top) })),
            ("label_pos", conn(&|c| c.label_pos = Some(0.25))),
            ("label_offset", conn(&|c| c.label_offset = 12.0)),
            ("valign", one_object(boxed(Style::default(), valign))),
        ];
        for (name, d) in cases {
            assert_eq!(version_of(&d), "3", "{name}");
        }
    }

    #[test]
    fn version_3_is_read_and_version_4_is_too_new() {
        assert!(from_json(r#"{"format":"inkboard","version":3,"document":{"name":"x","pages":[]}}"#).is_ok());
        assert!(matches!(from_json(r#"{"format":"inkboard","version":4,"document":{"name":"x","pages":[]}}"#), Err(LoadError::TooNew(4))));
    }

    #[test]
    fn sanitize_repairs_documents() {
        let mut d = sample();
        let dup = d.pages[0].objects[0].clone();
        d.pages[0].objects.push(dup);
        d.pages[0].objects.remove(1); // remove the connector's target
        d.pages[0].objects[0].layer = Id::from("unknown");
        sanitize(&mut d);
        let ids: HashSet<&Id> = d.pages[0].objects.iter().map(|o| &o.id).collect();
        assert_eq!(ids.len(), d.pages[0].objects.len());
        assert!(!d.pages[0].objects.iter().any(|o| matches!(o.kind, Kind::Connector(_))));
        assert_eq!(d.pages[0].objects[0].layer, Id::from("main"));
        let mut empty = Document { pages: vec![], ..Document::default() };
        sanitize(&mut empty);
        assert_eq!(empty.pages.len(), 1);
    }

    #[test]
    fn native_names_cover_the_three_extensions() {
        for n in ["a.inkboard", "A.INKBOARD", "a.inkboard.json", "dir/a b.tableau.json", r"C:\x\y.Inkboard.JSON"] {
            assert!(is_native_name(n), "{n}");
        }
        for n in ["a.json", "a.inkboard.png", "a.inkboardx", "inkboard", "a.drawio"] {
            assert!(!is_native_name(n), "{n}");
        }
    }

    #[test]
    fn save_and_load_through_disk() {
        let dir = std::env::temp_dir().join(format!("wb-test-{}", Id::new()));
        std::fs::create_dir_all(&dir).unwrap();
        let d = sample();
        for name in ["a.inkboard", "b.inkboard.json", "c.tableau.json"] {
            let path = dir.join(name);
            save(&d, &path).unwrap();
            assert!(!path.with_extension("tmp-save").exists());
            assert_eq!(load(&path).unwrap(), d, "{name}");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rich_text_stays_compact_in_json() {
        let j = serde_json::to_string(&RichText::plain("a")).unwrap();
        assert_eq!(j, r#"[{"text":"a"}]"#);
    }
}
