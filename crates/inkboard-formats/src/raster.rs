//! PNG rendering of an SVG exported by the engine (pure Rust, no window).

use crate::common::FormatError;
use inkboard_core::num;
use resvg::{tiny_skia, usvg};

use resvg::usvg::fontdb as render_fontdb;
use svg2pdf::usvg::fontdb as pdf_fontdb;

// `resvg` and `svg2pdf` depend on different `usvg`/`fontdb` versions: one body, two instantiations.
/// Font database: system fonts (when `system`) plus egui's embedded fonts (Ubuntu-Light, Hack) as a fallback.
/// A generic family (`sans-serif`, `serif`, `cursive`, `monospace`) that the system does not satisfy is mapped
/// to an embedded font, so that the text of an export never silently disappears on a machine without fonts.
macro_rules! fallback_fontdb {
    ($name:ident, $fontdb:ident) => {
        fn $name(system: bool) -> $fontdb::Database {
            let mut db = $fontdb::Database::new();
            if system {
                db.load_system_fonts();
            }
            let mut embedded = |data: &[u8]| {
                db.load_font_data(data.to_vec());
                db.faces().last().and_then(|f| f.families.first()).map(|(name, _)| name.clone())
            };
            let sans = embedded(epaint_default_fonts::UBUNTU_LIGHT);
            let mono = embedded(epaint_default_fonts::HACK_REGULAR);
            let lacks = |db: &$fontdb::Database, family| db.query(&$fontdb::Query { families: &[family], ..$fontdb::Query::default() }).is_none();
            let (no_sans, no_serif, no_cursive, no_mono) = (
                lacks(&db, $fontdb::Family::SansSerif),
                lacks(&db, $fontdb::Family::Serif),
                lacks(&db, $fontdb::Family::Cursive),
                lacks(&db, $fontdb::Family::Monospace),
            );
            if let Some(sans) = sans {
                if no_sans {
                    db.set_sans_serif_family(sans.clone());
                }
                if no_serif {
                    db.set_serif_family(sans.clone());
                }
                if no_cursive {
                    db.set_cursive_family(sans);
                }
            }
            if let Some(mono) = mono.filter(|_| no_mono) {
                db.set_monospace_family(mono);
            }
            db
        }
    };
}
fallback_fontdb!(render_fonts, render_fontdb);
fallback_fontdb!(pdf_fonts, pdf_fontdb);

/// Tests only: `INKBOARD_NO_SYSTEM_FONTS=1` ignores the system fonts, as on a minimal Linux (container, CI).
fn use_system_fonts() -> bool {
    !(cfg!(test) && std::env::var_os("INKBOARD_NO_SYSTEM_FONTS").is_some())
}

/// `usvg` options for any SVG this application rasterizes: rendering must NEVER read a local file (`C:/...`, UNC path, relative path)
/// nor open the network, whoever wrote the SVG. Only `data:` URLs are resolved (a nested SVG gets the same rule from `usvg`).
macro_rules! sandboxed_options {
    ($usvg:ident, $fontdb:expr) => {
        $usvg::Options {
            fontdb: $fontdb,
            image_href_resolver: $usvg::ImageHrefResolver {
                resolve_data: $usvg::ImageHrefResolver::default_data_resolver(),
                resolve_string: Box::new(|_, _| None),
            },
            ..$usvg::Options::default()
        }
    };
}

/// Fonts (system + embedded fallback), loaded only once (loading costs tens of milliseconds).
fn system_fonts() -> std::sync::Arc<usvg::fontdb::Database> {
    static FONTS: std::sync::OnceLock<std::sync::Arc<usvg::fontdb::Database>> = std::sync::OnceLock::new();
    FONTS.get_or_init(|| std::sync::Arc::new(render_fonts(use_system_fonts()))).clone()
}

/// Decoded image: width, height and **premultiplied** RGBA pixels.
#[derive(Debug)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub rgba_premultiplied: Vec<u8>,
}

/// Rasterizes an SVG so that its longest side is exactly `longest_px` pixels (the size comes from the parsed document, whatever its attributes).
///
/// # Errors
///
/// [`FormatError`] when the SVG cannot be parsed or its pixel size is invalid or too large.
pub fn svg_to_raster_fit(svg: &str, longest_px: u32, with_fonts: bool) -> Result<Raster, FormatError> {
    let fontdb = if with_fonts { system_fonts() } else { std::sync::Arc::new(usvg::fontdb::Database::new()) };
    let opt = sandboxed_options!(usvg, fontdb);
    let tree = usvg::Tree::from_str(svg, &opt).map_err(|e| FormatError::Invalid(format!("SVG: {e}")))?;
    let size = tree.size();
    let longest = size.width().max(size.height());
    if longest.is_nan() || longest <= 0.0 {
        return Err(FormatError::Unsupported("invalid image dimensions".into()));
    }
    let scale = num::f32(f64::from(longest_px)) / longest;
    let (w, h) = (
        num::to_u32(f64::from((size.width() * scale).round())).clamp(1, longest_px),
        num::to_u32(f64::from((size.height() * scale).round())).clamp(1, longest_px),
    );
    let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or_else(|| FormatError::Unsupported("image too large".into()))?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(num::f32(f64::from(w)) / size.width(), num::f32(f64::from(h)) / size.height()), &mut pixmap.as_mut());
    Ok(Raster { width: w, height: h, rgba_premultiplied: pixmap.take() })
}

/// Rasterizes an SVG. `with_fonts`: loads the system fonts (only needed when the SVG contains text; icons have none).
///
/// # Errors
///
/// [`FormatError`] when the SVG cannot be parsed or its pixel size is invalid or too large.
pub fn svg_to_raster(svg: &str, scale: f32, with_fonts: bool) -> Result<Raster, FormatError> {
    svg_to_raster_max_side(svg, scale, u32::MAX, with_fonts)
}

/// Like [`svg_to_raster`], but the scale is lowered when needed so that the longest side does not exceed `max_side` pixels (a texture limit).
#[allow(clippy::missing_errors_doc, reason = "as `svg_to_raster`: the error says what the SVG lacked")]
pub fn svg_to_raster_max_side(svg: &str, scale: f32, max_side: u32, with_fonts: bool) -> Result<Raster, FormatError> {
    let fontdb = if with_fonts { system_fonts() } else { std::sync::Arc::new(usvg::fontdb::Database::new()) };
    let opt = sandboxed_options!(usvg, fontdb);
    let tree = usvg::Tree::from_str(svg, &opt).map_err(|e| FormatError::Invalid(format!("SVG: {e}")))?;
    let size = tree.size();
    let longest = size.width().max(size.height()) * scale;
    #[allow(clippy::cast_precision_loss, reason = "pixel sizes are far below 2^24")]
    let limit = max_side as f32;
    let scale = if longest.is_finite() && longest > limit { scale * limit / longest } else { scale };
    let (w, h) = (num::to_u32(f64::from((size.width() * scale).ceil())), num::to_u32(f64::from((size.height() * scale).ceil())));
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 200_000_000 {
        return Err(FormatError::Unsupported("invalid or too large image dimensions".into()));
    }
    let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or_else(|| FormatError::Unsupported("image too large".into()))?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    Ok(Raster { width: w, height: h, rgba_premultiplied: pixmap.take() })
}

/// Converts an SVG to PNG. `scale`: 1.0 = SVG size, 2.0 = double resolution.
///
/// # Errors
///
/// [`FormatError`] when the SVG cannot be parsed or rasterized, or the PNG encoding fails.
pub fn svg_to_png(svg: &str, scale: f32) -> Result<Vec<u8>, FormatError> {
    svg_to_png_with(svg, scale, system_fonts())
}

/// Converts an SVG to PNG of exactly `width` pixels across (the height follows the aspect ratio), whatever the size of the SVG.
///
/// # Errors
///
/// Same as [`svg_to_png`].
pub fn svg_to_png_width(svg: &str, width: u32) -> Result<Vec<u8>, FormatError> {
    png_with(svg, Sizing::Width(width), system_fonts())
}

/// How big a PNG is drawn: `Scale` times the size of the SVG, or a fixed `Width` in pixels.
#[derive(Clone, Copy)]
enum Sizing {
    Scale(f32),
    Width(u32),
}

fn svg_to_png_with(svg: &str, scale: f32, fontdb: std::sync::Arc<usvg::fontdb::Database>) -> Result<Vec<u8>, FormatError> {
    png_with(svg, Sizing::Scale(scale), fontdb)
}

fn png_with(svg: &str, sizing: Sizing, fontdb: std::sync::Arc<usvg::fontdb::Database>) -> Result<Vec<u8>, FormatError> {
    let opt = sandboxed_options!(usvg, fontdb);
    let tree = usvg::Tree::from_str(svg, &opt).map_err(|e| FormatError::Invalid(format!("SVG: {e}")))?;
    let size = tree.size();
    let scale = match sizing {
        Sizing::Scale(s) => s,
        Sizing::Width(w) if size.width() > 0.0 => num::f32(f64::from(w)) / size.width(),
        Sizing::Width(_) => 0.0,
    };
    let w = match sizing {
        Sizing::Width(w) => w,
        Sizing::Scale(_) => num::to_u32(f64::from((size.width() * scale).ceil())),
    };
    let h = num::to_u32(f64::from((size.height() * scale).ceil()));
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 200_000_000 {
        return Err(FormatError::Unsupported("invalid or too large image dimensions".into()));
    }
    let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or_else(|| FormatError::Unsupported("image too large".into()))?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|e| FormatError::Invalid(format!("PNG : {e}")))
}

/// Converts an SVG to vector PDF (text and images preserved).
///
/// # Errors
///
/// [`FormatError`] when the SVG cannot be parsed or converted.
pub fn svg_to_pdf(svg: &str) -> Result<Vec<u8>, FormatError> {
    svg_to_pdf_with(svg, pdf_fonts(use_system_fonts()))
}

fn svg_to_pdf_with(svg: &str, fontdb: pdf_fontdb::Database) -> Result<Vec<u8>, FormatError> {
    use svg2pdf::usvg as pdf_usvg;
    let opt = sandboxed_options!(pdf_usvg, std::sync::Arc::new(fontdb));
    // svg2pdf ignores SVG filters: the drop shadows are drawn as shapes instead
    let svg = inkboard_core::svg::with_soft_shadows(svg);
    let tree = pdf_usvg::Tree::from_str(&svg, &opt).map_err(|e| FormatError::Invalid(format!("SVG: {e}")))?;
    svg2pdf::to_pdf(&tree, svg2pdf::ConversionOptions::default(), svg2pdf::PageOptions::default()).map_err(|e| FormatError::Invalid(format!("PDF : {e}")))
}

/// Longest side and number of pixels a bitmap received from somebody else may declare (read from its header, before any decoding).
pub const MAX_BITMAP_SIDE: u32 = 16_384;
pub const MAX_BITMAP_PIXELS: u64 = 40_000_000;
/// Bounds of an SVG written by somebody else (see [`check_untrusted_svg`]).
const MAX_SVG_ELEMENTS: usize = 20_000;
const MAX_SVG_DEPTH: usize = 48;
const MAX_SVG_USES: usize = 200;
/// Bytes of geometry (`d`, `points`) an SVG written by somebody else may carry: drawing time grows with it (a path of 400 000 segments,
/// 3.5 MB, takes seconds to rasterize and no filter is needed for that).
const MAX_SVG_GEOMETRY_BYTES: usize = 1 << 20;

/// Width and height a PNG, GIF, JPEG or WebP declares in its header (no decoding); `None` when unknown or unreadable.
#[must_use]
#[allow(clippy::many_single_char_names, reason = "`be`/`le16`/`le32`-style readers of the headers")]
pub fn declared_dimensions(d: &[u8]) -> Option<(u32, u32)> {
    let be = |i: usize| d.get(i..i + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
    let le16 = |i: usize| d.get(i..i + 2).map(|b| u32::from(u16::from_le_bytes([b[0], b[1]])));
    let le24 = |i: usize| d.get(i..i + 3).map(|b| u32::from(b[0]) | (u32::from(b[1]) << 8) | (u32::from(b[2]) << 16));
    if d.starts_with(b"\x89PNG\r\n\x1a\n") {
        return if d.get(12..16)? == b"IHDR" { Some((be(16)?, be(20)?)) } else { None };
    }
    if d.starts_with(b"GIF87a") || d.starts_with(b"GIF89a") {
        return Some((le16(6)?, le16(8)?));
    }
    if d.len() > 30 && &d[..4] == b"RIFF" && &d[8..12] == b"WEBP" {
        return match &d[12..16] {
            b"VP8X" => Some((le24(24)? + 1, le24(27)? + 1)),
            b"VP8 " => Some((le16(26)? & 0x3fff, le16(28)? & 0x3fff)),
            b"VP8L" => {
                let v = u32::from_le_bytes([d[21], d[22], d[23], d[24]]);
                Some(((v & 0x3fff) + 1, ((v >> 14) & 0x3fff) + 1))
            }
            _ => None,
        };
    }
    if d.starts_with(&[0xff, 0xd8]) {
        let mut i = 2;
        while i + 4 <= d.len() {
            if d[i] != 0xff {
                return None;
            }
            let m = d[i + 1];
            match m {
                0xff => i += 1, // fill byte
                0x01 | 0xd0..=0xd7 => i += 2,
                0xd9 | 0xda => return None,
                0xc0..=0xcf if !matches!(m, 0xc4 | 0xc8 | 0xcc) => {
                    let (h, w) =
                        (u32::from(u16::from_be_bytes([*d.get(i + 5)?, *d.get(i + 6)?])), u32::from(u16::from_be_bytes([*d.get(i + 7)?, *d.get(i + 8)?])));
                    return Some((w, h));
                }
                _ => i += 2 + usize::from(u16::from_be_bytes([d[i + 2], d[i + 3]])),
            }
        }
    }
    None
}

/// Elements an SVG written by somebody else may contain. Everything else is refused: `filter` and `fe*` (a heavy blur freezes every
/// renderer), `pattern`, `mask`, `script`, `foreignObject`, `a`, `animate*`, `switch`...
const SVG_TAGS: &[&str] = &[
    "svg",
    "g",
    "defs",
    "title",
    "desc",
    "path",
    "rect",
    "circle",
    "ellipse",
    "line",
    "polyline",
    "polygon",
    "text",
    "tspan",
    "textPath",
    "image",
    "linearGradient",
    "radialGradient",
    "stop",
    "clipPath",
    "symbol",
    "use",
    "style",
    "marker",
];

/// Editor metadata that renderers ignore (`<metadata>`, `sodipodi:namedview`, `rdf:RDF`...).
fn is_inert_tag(name: &str) -> bool {
    name == "metadata" || ["sodipodi:", "inkscape:", "rdf:", "cc:", "dc:"].iter().any(|p| name.starts_with(p))
}

/// Every `url(...)` of a value must point inside the document (`#id`): no file, no network.
fn urls_stay_inside(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.match_indices("url(").all(|(i, _)| lower[i + 4..].trim_start().trim_start_matches(['"', '\'']).trim_start().starts_with('#'))
}

/// A CSS text (`<style>` element or `style` attribute): no `@import`, no `filter`/`mask`, no `url()` leaving the document.
fn css_is_safe(css: &str) -> bool {
    let lower = css.to_ascii_lowercase();
    !lower.contains("@import") && !lower.contains("filter") && !lower.contains("mask") && urls_stay_inside(css)
}

/// Checks an SVG that **somebody else** wrote (a guest of a shared session) before it is stored and drawn by everybody. White list of
/// elements, no filter/pattern/mask/script/`foreignObject`/animation/DOCTYPE, `use`/`href` only inside the document, `href` of an image only a
/// `data:` bitmap within the pixel bounds, `url()` only inside the document, bounded element count, nesting and `use` count. `Err` is a short
/// English reason. The user's own SVG files are not subject to this (rendering is sandboxed anyway: see `sandboxed_options`).
#[allow(clippy::missing_errors_doc, clippy::too_many_lines, reason = "the refusal is a sentence; one pass over the document, rule after rule")]
pub fn check_untrusted_svg(svg: &str) -> Result<(), String> {
    use base64::Engine as _;
    use quick_xml::XmlVersion;
    use quick_xml::events::{BytesStart, Event};
    let (mut depth, mut elements, mut uses, mut pixels, mut style_open, mut seen_root) = (0usize, 0usize, 0usize, 0u64, false, false);
    let mut geometry = 0usize;
    let mut reader = quick_xml::Reader::from_str(svg);
    let mut element = |e: &BytesStart<'_>, depth: usize, style_open: &mut bool, seen_root: &mut bool| -> Result<(), String> {
        let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
        elements += 1;
        if elements > MAX_SVG_ELEMENTS || depth > MAX_SVG_DEPTH {
            return Err("the SVG is too complex".into());
        }
        if !*seen_root {
            *seen_root = true;
            if name != "svg" {
                return Err("the document is not an SVG".into());
            }
        }
        let inert = is_inert_tag(&name);
        if !inert && !SVG_TAGS.contains(&name.as_str()) {
            return Err(format!("the SVG element <{name}> is not allowed"));
        }
        if name == "style" {
            *style_open = true;
        }
        if name == "use" {
            uses += 1;
            if uses > MAX_SVG_USES {
                return Err("the SVG is too complex".into());
            }
        }
        for a in e.attributes() {
            let a = a.map_err(|x| format!("invalid SVG: {x}"))?;
            let key = String::from_utf8_lossy(a.key.as_ref()).to_ascii_lowercase();
            let value = a.normalized_value(XmlVersion::Implicit1_0).map_err(|x| format!("invalid SVG: {x}"))?.into_owned();
            if key.starts_with("on") || matches!(key.as_str(), "filter" | "mask") {
                return Err(format!("the SVG attribute {key} is not allowed"));
            }
            if inert {
                continue;
            }
            if matches!(key.as_str(), "d" | "points") {
                geometry += value.len();
                if geometry > MAX_SVG_GEOMETRY_BYTES {
                    return Err("the SVG is too complex".into());
                }
            }
            if key == "href" || key == "xlink:href" {
                let v = value.trim();
                if name == "image" {
                    let rest = v.strip_prefix("data:image/").ok_or("an SVG image must be embedded (data:), not a link")?;
                    let (kind, payload) = rest.split_once(";base64,").ok_or("an SVG image must be embedded as base64")?;
                    if !matches!(kind.to_ascii_lowercase().as_str(), "png" | "jpeg" | "jpg" | "gif" | "webp") {
                        return Err("an image embedded in an SVG must be a bitmap".into());
                    }
                    let clean: String = payload.chars().filter(|c| !c.is_ascii_whitespace()).collect();
                    let bytes = base64::engine::general_purpose::STANDARD.decode(clean).map_err(|_| "invalid embedded image")?;
                    let (w, h) = declared_dimensions(&bytes).ok_or("unreadable embedded image")?;
                    pixels += u64::from(w) * u64::from(h);
                    if w == 0 || h == 0 || w > MAX_BITMAP_SIDE || h > MAX_BITMAP_SIDE || pixels > MAX_BITMAP_PIXELS {
                        return Err("embedded image dimensions out of bounds".into());
                    }
                } else if !v.starts_with('#') {
                    return Err("an SVG link must stay inside the document".into());
                }
            } else if key == "style" {
                if !css_is_safe(&value) {
                    return Err("the SVG style is not allowed".into());
                }
            } else if !urls_stay_inside(&value) {
                return Err("an SVG reference must stay inside the document".into());
            }
        }
        Ok(())
    };
    loop {
        match reader.read_event().map_err(|e| format!("invalid SVG: {e}"))? {
            Event::Start(e) => {
                element(&e, depth, &mut style_open, &mut seen_root)?;
                depth += 1;
            }
            Event::Empty(e) => {
                element(&e, depth, &mut style_open, &mut seen_root)?;
                style_open = false;
            }
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                style_open = false;
            }
            Event::DocType(_) | Event::PI(_) => return Err("the SVG declaration is not allowed".into()),
            Event::Text(t) if style_open => {
                if !css_is_safe(&t.decode().map_err(|e| e.to_string())?) {
                    return Err("the SVG style is not allowed".into());
                }
            }
            Event::CData(t) if style_open => {
                if !css_is_safe(&String::from_utf8_lossy(t.as_ref())) {
                    return Err("the SVG style is not allowed".into());
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if seen_root { Ok(()) } else { Err("the document is not an SVG".into()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_to_raster_fit_gives_the_requested_longest_side() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="10" viewBox="0 0 40 10"><rect width="40" height="10" fill="#f00"/></svg>"##;
        let r = svg_to_raster_fit(svg, 100, false).unwrap();
        assert_eq!((r.width, r.height), (100, 25));
        let r = svg_to_raster_fit(svg, 30, false).unwrap();
        assert!(r.width == 30 && r.height <= 30);
    }

    #[test]
    fn renders_a_png_with_the_right_size_and_signature() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20" viewBox="0 0 40 20"><rect width="40" height="20" fill="#ff0000"/></svg>"##;
        let png = svg_to_png(svg, 2.0).unwrap();
        assert_eq!(&png[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        let w = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
        let h = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
        assert_eq!((w, h), (80, 40));
    }

    #[test]
    fn rasterizes_to_premultiplied_pixels_without_system_fonts() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2" viewBox="0 0 4 2"><rect width="4" height="2" fill="#ffffff" fill-opacity="0.5"/></svg>"##;
        let r = svg_to_raster(svg, 2.0, false).unwrap();
        assert_eq!((r.width, r.height), (8, 4));
        assert_eq!(r.rgba_premultiplied.len(), 8 * 4 * 4);
        // 50 % premultiplied white: channels ≈ alpha ≈ 128
        let p = &r.rgba_premultiplied[0..4];
        assert!(p[3] > 120 && p[3] < 136 && p[0].abs_diff(p[3]) <= 2, "{p:?}");
    }

    #[test]
    fn rejects_broken_svg() {
        assert!(svg_to_png("<svg", 1.0).is_err());
    }

    #[test]
    fn exports_a_pdf_document() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20" viewBox="0 0 40 20"><rect width="40" height="20" fill="#ff0000"/><text x="2" y="14" font-size="10">Hi</text></svg>"##;
        let pdf = svg_to_pdf(svg).unwrap();
        assert_eq!(&pdf[..5], b"%PDF-");
        assert!(svg_to_pdf("<svg").is_err());
    }

    /// SVG of a red rectangle without outline, with or without a drop shadow, on a transparent background.
    fn rect_svg(shadow: bool) -> String {
        use inkboard_core::svg::{SvgOptions, export};
        use inkboard_core::{BoxGeom, Color, Document, Frame, Label, Object, Style};
        let style = Style { fill: Some(Color::rgb(255, 0, 0)), width: 0.0, shadow, ..Style::default() };
        let mut doc = Document::default();
        doc.pages[0].objects = vec![Object::new_box(BoxGeom::Rect, Frame::new(0.0, 0.0, 100.0, 60.0), style, Label::default())];
        export(&doc, &doc.pages[0], None, &inkboard_core::ApproxMeasurer, &SvgOptions { padding: 10.0, transparent: true }).unwrap()
    }

    /// Pixels of the shadow: black, translucent (the red of the rectangle has a red channel).
    fn shadow_pixels(svg: &str) -> usize {
        let r = svg_to_raster(svg, 1.0, false).unwrap();
        r.rgba_premultiplied.as_chunks::<4>().0.iter().filter(|p| p[0] == 0 && p[1] == 0 && p[2] == 0 && p[3] > 20 && p[3] < 120).count()
    }

    #[test]
    fn a_drop_shadow_is_drawn_by_the_png_and_the_pdf_export_and_only_when_asked() {
        let svg = rect_svg(true);
        assert_eq!(svg.matches("<filter").count(), 1, "one filter for the one shadowed object");
        assert!(svg.contains("feDropShadow") && svg.contains("filter=\"url(#ink-shadow-0)\""), "{svg}");
        assert!(shadow_pixels(&svg) > 200, "a translucent black band shows right of and under the rectangle");
        assert_eq!(shadow_pixels(&rect_svg(false)), 0);
        assert!(!rect_svg(false).contains("filter"));
        assert_eq!(&svg_to_pdf(&svg).unwrap()[..5], b"%PDF-");
    }

    #[test]
    fn the_pdf_gets_a_soft_vector_shadow_and_the_other_exports_keep_the_filter() {
        let svg = rect_svg(true);
        let soft = inkboard_core::svg::with_soft_shadows(&svg);
        assert!(!soft.contains("filter") && soft.contains("mask-type=\"alpha\""), "{soft}");
        assert_eq!(soft.matches("<rect").count(), 7, "five translucent copies, the object and its copy in the mask");
        // what resvg draws of the PDF variant is a translucent black band like the filter's, in the same place
        let (band, real) = (shadow_pixels(&soft), shadow_pixels(&svg));
        assert!(band > 200 && band.abs_diff(real) < real / 2, "{band} {real}");
        assert_eq!(inkboard_core::svg::with_soft_shadows(&rect_svg(false)), rect_svg(false), "no shadow, nothing changed");
        // the PDF carries the soft masks, with no bitmap
        let pdf = String::from_utf8_lossy(&svg_to_pdf(&svg).unwrap()).into_owned();
        assert!(pdf.contains("/SMask") && !pdf.contains("/Subtype /Image"), "soft mask, no image");
        // a shadow with nothing to mask stays valid
        assert!(svg_to_pdf(&rect_svg(false)).is_ok());
    }

    const TEXT_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="40" viewBox="0 0 200 40"><rect width="200" height="40" fill="#ffffff"/><g font-family="'Segoe UI', system-ui, Roboto, Helvetica, Arial, sans-serif" font-size="24" fill="#000000"><text x="4" y="30">Rectangle</text></g></svg>"##;

    fn without_text(svg: &str) -> String {
        svg.replace("<text x=\"4\" y=\"30\">Rectangle</text>", "")
    }

    fn render(svg: &str, fontdb: usvg::fontdb::Database) -> Vec<u8> {
        let png = svg_to_png_with(svg, 1.0, std::sync::Arc::new(fontdb)).unwrap();
        assert_eq!(&png[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        png
    }

    #[test]
    fn png_text_survives_a_machine_without_any_system_font() {
        // empty system database (as on a minimal Linux): only the embedded fallback can draw the text
        let with = render(TEXT_SVG, render_fonts(false));
        let without = render(&without_text(TEXT_SVG), render_fonts(false));
        assert_ne!(with, without, "the text of the export vanished (no fallback font)");
    }

    #[test]
    fn serif_and_monospace_text_also_fall_back_to_embedded_fonts() {
        for family in ["Georgia, 'Times New Roman', serif", "Consolas, 'SF Mono', 'Courier New', monospace", "'Segoe Print', cursive"] {
            let svg = TEXT_SVG.replace("'Segoe UI', system-ui, Roboto, Helvetica, Arial, sans-serif", family);
            let with = render(&svg, render_fonts(false));
            let without = render(&without_text(&svg), render_fonts(false));
            assert_ne!(with, without, "no text for {family}");
        }
    }

    /* ---- S-01: rendering never reads a local file nor opens the network ---- */

    const RED: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="#ff0000"/></svg>"##;

    fn svg_with_image(href: &str) -> String {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="10" height="10"><image xlink:href="{href}" width="10" height="10"/></svg>"#
        )
    }

    fn opaque_pixels(svg: &str) -> usize {
        let r = svg_to_raster(svg, 1.0, false).unwrap();
        r.rgba_premultiplied.chunks(4).filter(|p| p[3] > 0).count()
    }

    /// A red 10 x 10 PNG in a fresh temporary directory (removed when dropped).
    struct LocalPng(std::path::PathBuf);
    impl LocalPng {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("inkboard-sf1-{}-{:?}", std::process::id(), std::thread::current().id()).replace(['(', ')', ' '], ""));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("secret.png");
            std::fs::write(&path, svg_to_png(RED, 1.0).unwrap()).unwrap();
            LocalPng(path)
        }
        fn href(&self) -> String {
            self.0.to_string_lossy().replace('\\', "/")
        }
    }
    impl Drop for LocalPng {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(self.0.parent().unwrap());
        }
    }

    fn data_png() -> String {
        use base64::Engine as _;
        format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(svg_to_png(RED, 1.0).unwrap()))
    }

    #[test]
    fn a_data_url_image_still_renders() {
        assert_eq!(opaque_pixels(&svg_with_image(&data_png())), 100);
    }

    #[test]
    fn an_svg_image_pointing_to_a_local_file_renders_without_its_pixels() {
        let png = LocalPng::new();
        assert!(png.0.exists());
        for href in [png.href(), png.0.to_string_lossy().into_owned(), format!("file:///{}", png.href())] {
            assert_eq!(opaque_pixels(&svg_with_image(&href)), 0, "{href} was read");
        }
    }

    #[test]
    fn unc_paths_and_urls_are_ignored_by_every_renderer() {
        for href in [r"\\attacker.invalid\share\a.png", "//attacker.invalid/share/a.png", "http://127.0.0.1:9/a.png", "https://example.invalid/a.png"] {
            let svg = svg_with_image(href);
            assert_eq!(opaque_pixels(&svg), 0, "{href}");
            assert!(svg_to_png(&svg, 1.0).is_ok());
            assert!(svg_to_raster_fit(&svg, 20, false).is_ok());
        }
    }

    #[test]
    fn png_and_pdf_exports_do_not_embed_a_local_file() {
        let png = LocalPng::new();
        let svg = svg_with_image(&png.href());
        let blank = svg_to_png(&svg, 1.0).unwrap();
        let none = svg_to_png(r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"/>"#, 1.0).unwrap();
        assert_eq!(blank, none, "the export of the PNG shows the local file");
        let has_image = |pdf: &[u8]| pdf.windows(15).any(|w| w == b"/Subtype /Image");
        assert!(!has_image(&svg_to_pdf(&svg).unwrap()), "the PDF embeds the local file");
        assert!(has_image(&svg_to_pdf(&svg_with_image(&data_png())).unwrap()), "an embedded image does reach the PDF");
    }

    #[test]
    fn the_raster_is_lowered_to_the_texture_limit() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="40000" height="10"><rect width="40000" height="10"/></svg>"#;
        let r = svg_to_raster_max_side(svg, 4.0, 8192, false).unwrap();
        assert!(r.width <= 8192 && r.height >= 1, "{}x{}", r.width, r.height);
    }

    /* ---- S-02: SVG written by somebody else ---- */

    fn untrusted(inner: &str) -> Result<(), String> {
        check_untrusted_svg(&format!(r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 10 10">{inner}</svg>"#))
    }

    #[test]
    fn a_plain_svg_from_a_guest_is_accepted() {
        let plain = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><defs><linearGradient id="g"><stop offset="0" stop-color="#f00"/></linearGradient></defs><g fill="url(#g)" style="stroke:#000"><circle cx="12" cy="12" r="10"/><path d="M0 0L5 5"/><text x="1" y="2">Hi</text><use href="#g"/></g></svg>"##;
        assert_eq!(check_untrusted_svg(plain), Ok(()));
        assert_eq!(untrusted(&format!(r#"<image href="{}" width="10" height="10"/>"#, data_png())), Ok(()));
    }

    #[test]
    fn dangerous_svg_features_are_refused_for_a_guest() {
        let png = LocalPng::new();
        let cases = [
            r#"<filter id="f"><feGaussianBlur stdDeviation="300"/></filter><rect filter="url(#f)" width="5" height="5"/>"#.to_owned(),
            r#"<filter id="f"><feImage href="x"/></filter>"#.to_owned(),
            r#"<pattern id="p" width="2" height="2"><rect width="1" height="1"/></pattern>"#.to_owned(),
            r#"<mask id="m"><rect width="1" height="1"/></mask>"#.to_owned(),
            r"<script>alert(1)</script>".to_owned(),
            r#"<foreignObject width="5" height="5"><div/></foreignObject>"#.to_owned(),
            r#"<use href="http://evil.invalid/a.svg#x"/>"#.to_owned(),
            r#"<use href="other.svg#x"/>"#.to_owned(),
            format!(r#"<image href="{}" width="5" height="5"/>"#, png.href()),
            r#"<image href="\\evil.invalid\s\a.png" width="5" height="5"/>"#.to_owned(),
            r#"<image href="http://evil.invalid/a.png" width="5" height="5"/>"#.to_owned(),
            r#"<image href="data:image/svg+xml;base64,PHN2Zy8+" width="5" height="5"/>"#.to_owned(),
            r#"<rect width="5" height="5" filter="blur(300)"/>"#.to_owned(),
            r#"<rect width="5" height="5" style="filter: blur(300px)"/>"#.to_owned(),
            r"<style>rect { filter: blur(300px) }</style>".to_owned(),
            r"<style>@import url(http://evil.invalid/x.css);</style>".to_owned(),
            r#"<rect width="5" height="5" fill="url(http://evil.invalid/x)"/>"#.to_owned(),
            r#"<rect width="5" height="5" onload="x()"/>"#.to_owned(),
            r#"<a href="http://evil.invalid"><rect width="5" height="5"/></a>"#.to_owned(),
            r#"<animate attributeName="x"/>"#.to_owned(),
        ];
        for c in &cases {
            assert!(untrusted(c).is_err(), "accepted: {c}");
        }
        assert!(check_untrusted_svg("<!DOCTYPE svg [<!ENTITY a \"b\">]><svg xmlns=\"http://www.w3.org/2000/svg\"/>").is_err());
        assert!(check_untrusted_svg("<html/>").is_err() && check_untrusted_svg("<svg").is_err() && check_untrusted_svg("").is_err());
    }

    #[test]
    fn a_complex_svg_or_a_huge_embedded_bitmap_is_refused_for_a_guest() {
        let many = "<rect width=\"1\" height=\"1\"/>".repeat(MAX_SVG_ELEMENTS + 1);
        assert!(untrusted(&many).is_err(), "too many elements");
        let deep = format!("{}{}", "<g>".repeat(MAX_SVG_DEPTH + 2), "</g>".repeat(MAX_SVG_DEPTH + 2));
        assert!(untrusted(&deep).is_err(), "too deep");
        let uses = "<use href=\"#a\"/>".repeat(MAX_SVG_USES + 1);
        assert!(untrusted(&uses).is_err(), "too many <use>");
        // a PNG header declaring 60 000 x 60 000 pixels, inside a data URL
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&60_000u32.to_be_bytes());
        png.extend_from_slice(&60_000u32.to_be_bytes());
        png.extend_from_slice(&[8, 6, 0, 0, 0]);
        use base64::Engine as _;
        let href = format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(&png));
        assert!(untrusted(&format!(r#"<image href="{href}" width="5" height="5"/>"#)).is_err());
    }

    #[test]
    fn headers_of_bitmaps_give_their_declared_size() {
        assert_eq!(declared_dimensions(&svg_to_png(RED, 1.0).unwrap()), Some((10, 10)));
        assert_eq!(declared_dimensions(b"GIF89a\x07\x00\x03\x00rest"), Some((7, 3)));
        assert_eq!(declared_dimensions(b"\xff\xd8\xff\xc0\x00\x11\x08\x00\x20\x00\x40\x03\x01\x22\x00\x02\x11"), Some((64, 32)));
        assert_eq!(declared_dimensions(b"nothing"), None);
    }

    #[test]
    fn pdf_text_survives_a_machine_without_any_system_font() {
        let with = svg_to_pdf_with(TEXT_SVG, pdf_fonts(false)).unwrap();
        let without = svg_to_pdf_with(&without_text(TEXT_SVG), pdf_fonts(false)).unwrap();
        assert!(with.len() > without.len() + 1000, "the PDF has no embedded font: {} vs {} bytes", with.len(), without.len());
    }
}
