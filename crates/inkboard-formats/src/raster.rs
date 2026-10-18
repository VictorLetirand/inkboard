//! PNG rendering of an SVG exported by the engine (pure Rust, no window).

use crate::common::FormatError;
use resvg::{tiny_skia, usvg};

/// System fonts, loaded only once (loading costs tens of milliseconds).
fn system_fonts() -> std::sync::Arc<usvg::fontdb::Database> {
    static FONTS: std::sync::OnceLock<std::sync::Arc<usvg::fontdb::Database>> = std::sync::OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut db = usvg::fontdb::Database::new();
            db.load_system_fonts();
            std::sync::Arc::new(db)
        })
        .clone()
}

/// Decoded image: width, height and **premultiplied** RGBA pixels.
#[derive(Debug)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub rgba_premultiplied: Vec<u8>,
}

/// Rasterizes an SVG so that its longest side is exactly `longest_px` pixels (the size comes from the parsed document, whatever its attributes).
pub fn svg_to_raster_fit(svg: &str, longest_px: u32, with_fonts: bool) -> Result<Raster, FormatError> {
    let fontdb = if with_fonts { system_fonts() } else { std::sync::Arc::new(usvg::fontdb::Database::new()) };
    let opt = usvg::Options { fontdb, ..usvg::Options::default() };
    let tree = usvg::Tree::from_str(svg, &opt).map_err(|e| FormatError::Invalid(format!("SVG: {e}")))?;
    let size = tree.size();
    let longest = size.width().max(size.height());
    if longest.is_nan() || longest <= 0.0 {
        return Err(FormatError::Unsupported("invalid image dimensions".into()));
    }
    let scale = longest_px as f32 / longest;
    let (w, h) = (((size.width() * scale).round() as u32).clamp(1, longest_px), ((size.height() * scale).round() as u32).clamp(1, longest_px));
    let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or_else(|| FormatError::Unsupported("image too large".into()))?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(w as f32 / size.width(), h as f32 / size.height()), &mut pixmap.as_mut());
    Ok(Raster { width: w, height: h, rgba_premultiplied: pixmap.take() })
}

/// Rasterizes an SVG. `with_fonts`: loads the system fonts (only needed when the SVG contains text; icons have none).
pub fn svg_to_raster(svg: &str, scale: f32, with_fonts: bool) -> Result<Raster, FormatError> {
    let fontdb = if with_fonts { system_fonts() } else { std::sync::Arc::new(usvg::fontdb::Database::new()) };
    let opt = usvg::Options { fontdb, ..usvg::Options::default() };
    let tree = usvg::Tree::from_str(svg, &opt).map_err(|e| FormatError::Invalid(format!("SVG: {e}")))?;
    let size = tree.size();
    let (w, h) = ((size.width() * scale).ceil() as u32, (size.height() * scale).ceil() as u32);
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 200_000_000 {
        return Err(FormatError::Unsupported("invalid or too large image dimensions".into()));
    }
    let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or_else(|| FormatError::Unsupported("image too large".into()))?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    Ok(Raster { width: w, height: h, rgba_premultiplied: pixmap.take() })
}

/// Converts an SVG to PNG. `scale`: 1.0 = SVG size, 2.0 = double resolution.
pub fn svg_to_png(svg: &str, scale: f32) -> Result<Vec<u8>, FormatError> {
    let opt = usvg::Options { fontdb: system_fonts(), ..usvg::Options::default() };
    let tree = usvg::Tree::from_str(svg, &opt).map_err(|e| FormatError::Invalid(format!("SVG: {e}")))?;
    let size = tree.size();
    let (w, h) = ((size.width() * scale).ceil() as u32, (size.height() * scale).ceil() as u32);
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 200_000_000 {
        return Err(FormatError::Unsupported("invalid or too large image dimensions".into()));
    }
    let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or_else(|| FormatError::Unsupported("image too large".into()))?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|e| FormatError::Invalid(format!("PNG : {e}")))
}

/// Converts an SVG to vector PDF (text and images preserved).
pub fn svg_to_pdf(svg: &str) -> Result<Vec<u8>, FormatError> {
    use svg2pdf::usvg as pdf_usvg;
    let mut fontdb = pdf_usvg::fontdb::Database::new();
    fontdb.load_system_fonts();
    let opt = pdf_usvg::Options { fontdb: std::sync::Arc::new(fontdb), ..pdf_usvg::Options::default() };
    let tree = pdf_usvg::Tree::from_str(svg, &opt).map_err(|e| FormatError::Invalid(format!("SVG: {e}")))?;
    svg2pdf::to_pdf(&tree, svg2pdf::ConversionOptions::default(), svg2pdf::PageOptions::default()).map_err(|e| FormatError::Invalid(format!("PDF : {e}")))
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
}
