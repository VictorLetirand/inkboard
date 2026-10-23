//! Random and mutated inputs: `import_any` may return an error, but it must never panic.
//! Mutations (truncation, flipped bytes, removed line) are applied to the real files of `tests/corpus`.

use inkboard_core::ApproxMeasurer;
use inkboard_formats::import_any;
use proptest::prelude::*;
use proptest::sample::Index;
use proptest::test_runner::RngSeed;
use std::path::PathBuf;
use std::sync::LazyLock;

/// File names that select each importer (the last one has no extension: content sniffing).
const NAMES: [&str; 6] = ["a.drawio", "a.excalidraw", "a.canvas", "a.mmd", "a.json", "a"];

/// Fragments of the supported formats, to build inputs that get past the first checks of the parsers.
const TOKENS: [&str; 30] = [
    "<mxfile>",
    "<diagram>",
    "<mxGraphModel>",
    "<root>",
    "<mxCell id=\"1\" ",
    "value=\"",
    "style=\"shape=image;image=",
    "vertex=\"1\" ",
    "edge=\"1\" source=\"1\" target=\"2\">",
    "<mxGeometry x=\"1\" y=\"2\" width=\"3\" height=\"4\" as=\"geometry\"/>",
    "</mxCell>",
    "{\"type\":\"excalidraw\",\"elements\":[",
    "{\"type\":\"arrow\",\"points\":[[0,0],[1,1]],\"startBinding\":{\"elementId\":\"a\"}",
    "{\"type\":\"text\",\"containerId\":\"a\",\"text\":\"t\"",
    "{\"nodes\":[",
    "{\"id\":\"a\",\"type\":\"text\",\"x\":1,\"y\":2,\"width\":3,\"height\":4}",
    "\"edges\":[{\"fromNode\":\"a\",\"toNode\":\"a\"}]",
    "flowchart LR\n",
    "stateDiagram-v2\n",
    "sequenceDiagram\n",
    "A-->B\n",
    "A[x] -->|y| B{z}\n",
    "subgraph s\n",
    "end\n",
    "[*] --> S\n",
    "participant A\n",
    "A->>B: hi\n",
    "\"",
    "\\",
    "\n",
];

/// `(file name, bytes)` of the corpus files of some formats.
fn load(formats: &[&str]) -> Vec<(String, Vec<u8>)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
    let mut out = Vec::new();
    for format in formats {
        for entry in std::fs::read_dir(root.join(format)).unwrap_or_else(|e| panic!("{format}: {e}")) {
            let path = entry.expect("directory entry").path();
            out.push((
                path.file_name().expect("file name").to_string_lossy().into_owned(),
                std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())),
            ));
        }
    }
    out.sort();
    assert!(!out.is_empty(), "the corpus is missing");
    out
}

/// Files that are mutated by the properties below.
static CORPUS: LazyLock<Vec<(String, Vec<u8>)>> = LazyLock::new(|| load(&["mermaid", "drawio", "excalidraw", "canvas"]));
static EXCALIDRAW: LazyLock<Vec<(String, Vec<u8>)>> = LazyLock::new(|| load(&["excalidraw"]));

fn config() -> ProptestConfig {
    // no regression file written into the repository: the failing case is printed
    let mut config = ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() };
    // fixed seed, so that the CI run is reproducible; set PROPTEST_RNG_SEED=<number> to explore other inputs
    if matches!(config.rng_seed, RngSeed::Random) {
        config.rng_seed = RngSeed::Fixed(2026);
    }
    config
}

/// The only property: no panic (the result itself may be an error).
fn import(name: &str, bytes: &[u8]) {
    let _ = import_any(name, &String::from_utf8_lossy(bytes), &ApproxMeasurer);
}

fn import_without_line(name: &str, bytes: &[u8], line: Index) {
    let text = String::from_utf8_lossy(bytes);
    let mut lines: Vec<&str> = text.split_inclusive('\n').collect();
    lines.remove(line.index(lines.len()));
    import(name, lines.concat().as_bytes());
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn random_text_never_panics(name in prop::sample::select(NAMES.to_vec()), text in "(?s).{0,300}") {
        import(name, text.as_bytes());
    }

    #[test]
    fn random_fragments_never_panic(name in prop::sample::select(NAMES.to_vec()), parts in prop::collection::vec(prop::sample::select(TOKENS.to_vec()), 0..40)) {
        import(name, parts.concat().as_bytes());
    }

    #[test]
    fn truncated_corpus_files_never_panic(file in any::<Index>(), cut in any::<Index>()) {
        let (name, bytes) = file.get(&CORPUS);
        import(name, &bytes[..cut.index(bytes.len() + 1)]);
    }

    #[test]
    fn corpus_files_with_flipped_bytes_never_panic(file in any::<Index>(), flips in prop::collection::vec((any::<Index>(), 1u8..=255), 1..16)) {
        let (name, bytes) = file.get(&CORPUS);
        let mut bytes = bytes.clone();
        for (at, mask) in flips {
            let i = at.index(bytes.len());
            bytes[i] ^= mask;
        }
        import(name, &bytes);
    }

    #[test]
    fn corpus_files_without_one_line_never_panic(file in any::<Index>(), line in any::<Index>()) {
        let (name, bytes) = file.get(&CORPUS);
        import_without_line(name, bytes, line);
    }

    #[test]
    fn bug_3_excalidraw_without_one_line_never_panics(file in any::<Index>(), line in any::<Index>()) {
        let (name, bytes) = file.get(&EXCALIDRAW);
        import_without_line(name, bytes, line);
    }
}

#[test]
fn bug_3_excalidraw_element_without_id() {
    let scene = r#"{"type":"excalidraw","elements":[{"type":"rectangle","x":0,"y":0,"width":10,"height":10}]}"#;
    let _ = import_any("a.excalidraw", scene, &ApproxMeasurer);
}
