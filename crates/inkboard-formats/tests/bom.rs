//! A UTF-8 byte order mark (Notepad, PowerShell 5 `Set-Content -Encoding utf8`) must not change the import of any format.

use inkboard_core::{ApproxMeasurer, Document};
use inkboard_formats::import_any;
use std::path::Path;

const BOM: char = '\u{feff}';

fn object_count(doc: &Document) -> usize {
    doc.pages.iter().map(|p| p.objects.len()).sum()
}

#[test]
fn a_bom_never_changes_the_import_of_a_corpus_file() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
    let mut checked = 0;
    for dir in ["mermaid", "drawio", "excalidraw", "canvas"] {
        for entry in std::fs::read_dir(root.join(dir)).expect("corpus folder") {
            let path = entry.expect("entry").path();
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let name = path.file_name().expect("name").to_string_lossy().into_owned();
            let plain = import_any(&name, &text, &ApproxMeasurer);
            let with_bom = import_any(&name, &format!("{BOM}{text}"), &ApproxMeasurer);
            match (plain, with_bom) {
                (Ok(a), Ok(b)) => assert_eq!(object_count(&a.doc), object_count(&b.doc), "{name}: objects differ with a BOM"),
                (Err(_), Err(_)) => {} // e.g. the unsupported Mermaid diagram types: still an error, not a different one
                (a, b) => panic!("{name}: result changes with a BOM (without: ok={}, with: ok={})", a.is_ok(), b.is_ok()),
            }
            checked += 1;
        }
    }
    assert!(checked >= 40, "only {checked} corpus files checked");
}

#[test]
fn a_native_board_with_a_bom_opens() {
    let json = inkboard_core::serial::to_json(&Document::new("Mine"), false);
    for name in ["a.inkboard", "a.inkboard.json", "a.json"] {
        let imp = import_any(name, &format!("{BOM}{json}"), &ApproxMeasurer).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(imp.doc.name, "Mine", "{name}");
    }
    assert!(inkboard_core::serial::from_json(&format!("{BOM}{json}")).is_ok());
}

#[test]
fn the_minimal_mermaid_file_from_the_installer_check_opens_with_a_bom() {
    // the exact case found while testing the installed application
    let text = format!("{BOM}flowchart LR\n  A[Installation] --> B[Association]\n");
    let imp = import_any("demo.mmd", &text, &ApproxMeasurer).expect("Mermaid with a BOM");
    assert!(object_count(&imp.doc) >= 2);
}
