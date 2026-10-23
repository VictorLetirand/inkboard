//! Bombs and malformed files on the paths a remote peer or an agent can reach: images and SVGs received in a shared session, imported
//! diagrams, the native format. The question is always the same: an explicit error, no panic, no stack overflow, no unbounded memory.
//! Sizes are chosen so that each test is quick; none measures a duration.

use base64::Engine as _;
use inkboard_core::ApproxMeasurer;
use inkboard_formats::raster::{MAX_BITMAP_PIXELS, MAX_BITMAP_SIDE, check_untrusted_svg, declared_dimensions, svg_to_raster_fit, svg_to_raster_max_side};
use inkboard_formats::{FormatError, import_any};
use proptest::prelude::*;
use proptest::test_runner::RngSeed;

fn config() -> ProptestConfig {
    let mut c = ProptestConfig { cases: 400, failure_persistence: None, ..ProptestConfig::default() };
    if matches!(c.rng_seed, RngSeed::Random) {
        c.rng_seed = RngSeed::Fixed(77);
    }
    c
}

fn import(name: &str, text: &str) -> Result<(), FormatError> {
    import_any(name, text, &ApproxMeasurer).map(|_| ())
}

const SVG_OPEN: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="100" height="100">"#;

/* ------------------------------------------------------------------------------------------------------- XML and deflate */

#[test]
fn a_deeply_nested_drawio_file_is_an_error_not_a_stack_overflow() {
    let xml = format!("<mxfile><diagram>{}{}</diagram></mxfile>", "<a>".repeat(100_000), "</a>".repeat(100_000));
    assert!(matches!(import("a.drawio", &xml), Err(FormatError::Invalid(_))));
    let model = format!("<mxGraphModel><root>{}{}</root></mxGraphModel>", "<mxCell>".repeat(5_000), "</mxCell>".repeat(5_000));
    assert!(import("a.drawio", &model).is_err());
}

#[test]
fn a_compressed_diagram_that_inflates_to_gigabytes_is_refused() {
    use flate2::{Compression, write::DeflateEncoder};
    use std::io::Write;
    // 72 MiB of one letter: about 70 KB compressed, over the 64 MiB ceiling once inflated
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::best());
    let chunk = vec![b'a'; 1 << 20];
    for _ in 0..72 {
        enc.write_all(&chunk).unwrap();
    }
    let compressed = enc.finish().unwrap();
    assert!(compressed.len() < 200_000, "{} bytes", compressed.len());
    let file = format!("<mxfile><diagram>{}</diagram></mxfile>", base64::engine::general_purpose::STANDARD.encode(&compressed));
    let err = import("a.drawio", &file).expect_err("a decompression bomb");
    assert!(err.to_string().contains("too large"), "{err}");
}

#[test]
fn entity_bombs_are_errors() {
    let dtd = r#"<?xml version="1.0"?><!DOCTYPE lolz [<!ENTITY a "aaaaaaaaaa"><!ENTITY b "&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;"><!ENTITY c "&b;&b;&b;&b;&b;&b;&b;&b;&b;&b;"><!ENTITY d "&c;&c;&c;&c;&c;&c;&c;&c;&c;&c;"><!ENTITY e "&d;&d;&d;&d;&d;&d;&d;&d;&d;&d;">]>"#;
    assert!(import("a.drawio", &format!("{dtd}<mxfile><diagram>&e;</diagram></mxfile>")).is_err());
    let svg = format!(r#"{dtd}<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><text>&e;</text></svg>"#);
    assert!(check_untrusted_svg(&svg).is_err(), "a DOCTYPE is refused for an SVG written by somebody else");
    // and the renderer itself (used on the host's own images) refuses the loop instead of expanding it
    assert!(svg_to_raster_max_side(&svg, 1.0, 512, false).is_err());
}

/* --------------------------------------------------------------------------------------------------------------- SVG */

#[test]
fn an_svg_with_enormous_geometry_is_refused_to_guests() {
    let mut d = String::from("M0 0");
    for i in 0..150_000 {
        d.push_str(&format!(" L{} {}", i % 997, (i * 7) % 991));
    }
    assert!(d.len() > (1 << 20));
    let big = format!(r#"{SVG_OPEN}<path d="{d}" stroke="black" fill="none"/></svg>"#);
    assert!(check_untrusted_svg(&big).is_err(), "{} bytes of path data", d.len());
    // several moderate paths add up
    let one = &d[..400_000];
    let many = format!("{SVG_OPEN}{}</svg>", format!(r#"<path d="{one}"/>"#).repeat(4));
    assert!(check_untrusted_svg(&many).is_err());
    let polyline = format!(r#"{SVG_OPEN}<polyline points="{}"/></svg>"#, "1,2 ".repeat(300_000));
    assert!(check_untrusted_svg(&polyline).is_err());
    // an ordinary icon is fine
    assert!(check_untrusted_svg(&format!(r#"{SVG_OPEN}<path d="M0 0 L10 10 L0 10 Z" fill="red"/></svg>"#)).is_ok());
}

#[test]
fn svg_bombs_are_refused_to_guests_before_any_rendering() {
    let reject = |body: &str| assert!(check_untrusted_svg(&format!("{SVG_OPEN}{body}</svg>")).is_err(), "{body}");
    reject(r##"<filter id="f"><feGaussianBlur stdDeviation="300"/></filter><rect width="9" height="9" filter="url(#f)"/>"##);
    reject(r##"<pattern id="p" width="1" height="1"><rect width="1" height="1"/></pattern>"##);
    reject(r##"<mask id="m"><rect width="9" height="9"/></mask>"##);
    reject(r##"<rect width="9" height="9" style="filter:blur(99px)"/>"##);
    reject(r##"<image href="file:///C:/secret.png" width="9" height="9"/>"##);
    reject(r##"<image href="\\attacker\share\a.png" width="9" height="9"/>"##);
    reject(r##"<image href="http://attacker.example/a.png" width="9" height="9"/>"##);
    reject(r##"<use href="http://attacker.example/a.svg#x"/>"##);
    reject(r##"<style>@import url(http://attacker.example/a.css);</style>"##);
    reject(r##"<script>alert(1)</script>"##);
    reject(r##"<foreignObject><div>x</div></foreignObject>"##);
    reject(&"<g>".repeat(60));
    reject(&r##"<rect width="1" height="1"/>"##.repeat(20_001));
    reject(&r##"<use href="#a"/>"##.repeat(201));
    // an embedded bitmap that declares 65535 x 65535 pixels
    let png = {
        let mut v = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        v.extend(65_535u32.to_be_bytes());
        v.extend(65_535u32.to_be_bytes());
        v.extend([8, 6, 0, 0, 0]);
        v
    };
    reject(&format!(r#"<image href="data:image/png;base64,{}" width="9" height="9"/>"#, base64::engine::general_purpose::STANDARD.encode(png)));
    // not even XML
    for junk in ["", "<", "<svg", "\u{0}", "<svg><", "<svg></g>", "<!DOCTYPE svg><svg/>"] {
        let _ = check_untrusted_svg(junk);
    }
}

#[test]
fn the_exponential_use_chain_that_fits_the_use_limit_is_stopped_by_the_renderer() {
    // 95 levels, two <use> each: 190 <use>, within the guests' limit, 2^95 instances if expanded
    let mut svg = String::from(SVG_OPEN);
    svg.push_str(r##"<defs><g id="l0"><rect width="1" height="1"/></g>"##);
    for i in 1..=95 {
        svg.push_str(&format!(r##"<g id="l{i}"><use href="#l{}"/><use href="#l{}" x="1"/></g>"##, i - 1, i - 1));
    }
    svg.push_str(r##"</defs><use href="#l95"/></svg>"##);
    assert!(check_untrusted_svg(&svg).is_ok(), "the chain passes the structural limits");
    assert!(svg_to_raster_max_side(&svg, 1.0, 512, false).is_err(), "the renderer gives up on it (its own node limit)");
}

#[test]
fn absurd_sizes_give_a_bounded_raster() {
    for (w, h) in [("1e9", "1e9"), ("1e-30", "1e-30"), ("NaN", "inf"), ("-5", "-5"), ("0", "0"), ("1e38", "1"), ("65535", "65535")] {
        let svg = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}"><rect width="5" height="5"/></svg>"#);
        if let Ok(r) = svg_to_raster_max_side(&svg, 4.0, 4096, false) {
            assert!(r.width <= 4096 && r.height <= 4096, "{w} x {h} gave {} x {}", r.width, r.height);
            assert_eq!(r.rgba_premultiplied.len(), r.width as usize * r.height as usize * 4);
        }
        if let Ok(r) = svg_to_raster_fit(&svg, 4096, false) {
            assert!(r.width <= 4096 && r.height <= 4096);
        }
    }
}

/* -------------------------------------------------------------------------------------------------------------- bitmaps */

fn png_header(w: u32, h: u32) -> Vec<u8> {
    let mut v = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    v.extend(w.to_be_bytes());
    v.extend(h.to_be_bytes());
    v.extend([8, 6, 0, 0, 0]);
    v
}

#[test]
fn headers_that_declare_huge_bitmaps_are_read_without_decoding() {
    assert_eq!(declared_dimensions(&png_header(65_535, 65_535)), Some((65_535, 65_535)));
    assert_eq!(declared_dimensions(&png_header(u32::MAX, u32::MAX)), Some((u32::MAX, u32::MAX)));
    let mut gif = b"GIF89a".to_vec();
    gif.extend([0xff, 0xff, 0xff, 0xff]);
    assert_eq!(declared_dimensions(&gif), Some((65_535, 65_535)));
    let jpeg = [0xff, 0xd8, 0xff, 0xc0, 0, 17, 8, 0xff, 0xff, 0xff, 0xff, 3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1];
    assert_eq!(declared_dimensions(&jpeg), Some((65_535, 65_535)));
    let mut webp = b"RIFF\0\0\0\0WEBPVP8X\x0a\0\0\0\0\0\0\0".to_vec();
    webp.extend([0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
    webp.extend([0; 8]);
    assert_eq!(declared_dimensions(&webp), Some((1 << 24, 1 << 24)));
    // all of them are over the bounds the host applies
    assert!(65_535 > MAX_BITMAP_SIDE && 65_535u64 * 65_535 > MAX_BITMAP_PIXELS);
    // truncated at every length: nothing to say, nothing to panic about
    for full in [png_header(10, 10), gif, jpeg.to_vec(), webp] {
        for n in 0..full.len() {
            let _ = declared_dimensions(&full[..n]);
        }
    }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn random_bytes_never_panic_the_header_reader(bytes in prop::collection::vec(any::<u8>(), 0..300), head in 0usize..5) {
        let mut b = [&b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"[..], b"GIF89a", &[0xff, 0xd8], b"RIFF\0\0\0\0WEBPVP8X", b""][head].to_vec();
        b.extend(bytes);
        let _ = declared_dimensions(&b);
    }

    #[test]
    fn random_and_damaged_svg_never_panics_the_guest_check(cut in any::<prop::sample::Index>(), flips in prop::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 0..8)) {
        let base = format!(
            r##"{SVG_OPEN}<defs><linearGradient id="g"><stop offset="0" stop-color="#f00"/></linearGradient></defs><g style="fill:url(#g)"><path d="M0 0 L9 9"/><text x="1" y="2">t</text><use href="#g"/><image href="data:image/png;base64,AAAA"/></g></svg>"##
        );
        let mut b = base.into_bytes();
        for (at, byte) in flips {
            let i = at.index(b.len());
            b[i] = byte;
        }
        let n = cut.index(b.len() + 1);
        let _ = check_untrusted_svg(&String::from_utf8_lossy(&b[..n]));
    }
}

/* -------------------------------------------------------------------------------------------------------- diagram text */

#[test]
fn a_flowchart_too_large_to_lay_out_is_refused_before_the_layout() {
    let chain = |n: usize| (0..n).map(|i| format!("n{i} --> n{}\n", i + 1)).collect::<String>();
    let ok = format!("flowchart LR\n{}", chain(300));
    assert!(import("a.mmd", &ok).is_ok());
    let nodes = format!("flowchart LR\n{}", chain(1_100));
    let err = import("a.mmd", &nodes).expect_err("1 100 nodes");
    assert!(err.to_string().contains("too large"), "{err}");
    let links: String = (0..2_600).map(|i| format!("a{} --> b{}\n", i % 40, (i * 7) % 40)).collect();
    assert!(import("a.mmd", &format!("flowchart TD\n{links}")).is_err(), "2 600 links between 80 nodes");
    let states: String = (0..1_100).map(|i| format!("s{i} --> s{}\n", i + 1)).collect();
    assert!(import("a.mmd", &format!("stateDiagram-v2\n{states}")).is_err());
}

#[test]
fn mermaid_oddities_are_errors_or_diagrams() {
    // a very long line, an endless chain on one line, unclosed brackets and subgraphs, unicode, an empty label
    let long_label = format!("flowchart LR\nA[{}] --> B\n", "x".repeat(200_000));
    let _ = import("a.mmd", &long_label);
    let chain_line = format!("flowchart LR\n{}\n", (0..4_000).map(|i| format!("n{i}")).collect::<Vec<_>>().join(" --> "));
    let _ = import("a.mmd", &chain_line);
    let _ = import("a.mmd", &format!("flowchart TD\n{}A --> B\n", "subgraph s\n".repeat(500)));
    let _ = import("a.mmd", "flowchart LR\nA[( --> B{{ \nC((((( --> D\n\u{202e}x --> \u{0}\n");
    let _ = import("a.mmd", &format!("sequenceDiagram\n{}", "A->>B: hi\n".repeat(20_000)));
    let _ = import("a.mmd", &format!("sequenceDiagram\n{}", "loop x\n".repeat(2_000)));
}

/* ----------------------------------------------------------------------------------------------------- native and others */

#[test]
fn hostile_native_boards_are_errors_or_boards_that_are_consistent() {
    let deep = format!("{}{}", "[".repeat(200_000), "]".repeat(200_000));
    assert!(inkboard_core::serial::from_json(&format!(r#"{{"format":"inkboard","pages":{deep}}}"#)).is_err());
    assert!(inkboard_core::serial::from_json(&"{\"a\":".repeat(100_000)).is_err());
    assert!(inkboard_core::serial::from_json("{\"format\":\"inkboard\",\"pages\":[]").is_err());
    for text in [
        r#"{"format":"inkboard","version":1,"name":"x","pages":[]}"#,
        r#"{"format":"inkboard","name":"x","pages":[{"id":"p","name":"p","layers":[],"objects":[{"id":"a","kind":{"type":"image","frame":{"x":1e308,"y":-1e308,"w":1e308,"h":1e308,"angle":1e308},"asset":"nope"}}]}]}"#,
        r#"{"format":"inkboard","name":"x","pages":[{"id":"p","name":"p","layers":[],"objects":[{"id":"a","kind":{"type":"connector","start":{"kind":"bound","target":"a"},"end":{"kind":"bound","target":"a"}}},{"id":"a","kind":{"type":"connector","start":{"kind":"bound","target":"a"},"end":{"kind":"free","at":{"x":1e308,"y":0}}}}]}]}"#,
    ] {
        if let Ok(doc) = inkboard_core::serial::from_json(text) {
            let mut ids = std::collections::HashSet::new();
            assert!(!doc.pages.is_empty());
            for p in &doc.pages {
                assert!(p.objects.iter().all(|o| ids.insert((&p.id, &o.id))), "duplicate ids survive loading");
            }
        }
    }
}
