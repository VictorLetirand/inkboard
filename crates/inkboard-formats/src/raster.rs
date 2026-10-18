//! Rendu PNG d'un SVG exporté par le moteur (pur Rust, sans fenêtre).

use crate::common::FormatError;
use resvg::{tiny_skia, usvg};

/// Polices du système, chargées une seule fois (le chargement coûte plusieurs dizaines de millisecondes).
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

/// Image décodée : largeur, hauteur et pixels RGBA **prémultipliés**.
#[derive(Debug)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub rgba_premultiplied: Vec<u8>,
}

/// Rastérise un SVG. `with_fonts` : charge les polices du système (nécessaire seulement si le SVG contient du texte ; les icônes n'en ont pas).
pub fn svg_to_raster(svg: &str, scale: f32, with_fonts: bool) -> Result<Raster, FormatError> {
    let fontdb = if with_fonts { system_fonts() } else { std::sync::Arc::new(usvg::fontdb::Database::new()) };
    let opt = usvg::Options { fontdb, ..usvg::Options::default() };
    let tree = usvg::Tree::from_str(svg, &opt).map_err(|e| FormatError::Invalid(format!("SVG : {e}")))?;
    let size = tree.size();
    let (w, h) = ((size.width() * scale).ceil() as u32, (size.height() * scale).ceil() as u32);
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 200_000_000 {
        return Err(FormatError::Unsupported("dimensions d'image invalides ou trop grandes".into()));
    }
    let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or_else(|| FormatError::Unsupported("image trop grande".into()))?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    Ok(Raster { width: w, height: h, rgba_premultiplied: pixmap.take() })
}

/// Convertit un SVG en PNG. `scale` : 1.0 = taille du SVG, 2.0 = double résolution.
pub fn svg_to_png(svg: &str, scale: f32) -> Result<Vec<u8>, FormatError> {
    let opt = usvg::Options { fontdb: system_fonts(), ..usvg::Options::default() };
    let tree = usvg::Tree::from_str(svg, &opt).map_err(|e| FormatError::Invalid(format!("SVG : {e}")))?;
    let size = tree.size();
    let (w, h) = ((size.width() * scale).ceil() as u32, (size.height() * scale).ceil() as u32);
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 200_000_000 {
        return Err(FormatError::Unsupported("dimensions d'image invalides ou trop grandes".into()));
    }
    let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or_else(|| FormatError::Unsupported("image trop grande".into()))?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|e| FormatError::Invalid(format!("PNG : {e}")))
}

/// Convertit un SVG en PDF vectoriel (texte et images conservés).
pub fn svg_to_pdf(svg: &str) -> Result<Vec<u8>, FormatError> {
    use svg2pdf::usvg as pdf_usvg;
    let mut fontdb = pdf_usvg::fontdb::Database::new();
    fontdb.load_system_fonts();
    let opt = pdf_usvg::Options { fontdb: std::sync::Arc::new(fontdb), ..pdf_usvg::Options::default() };
    let tree = pdf_usvg::Tree::from_str(svg, &opt).map_err(|e| FormatError::Invalid(format!("SVG : {e}")))?;
    svg2pdf::to_pdf(&tree, svg2pdf::ConversionOptions::default(), svg2pdf::PageOptions::default()).map_err(|e| FormatError::Invalid(format!("PDF : {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // blanc à 50 % prémultiplié : canaux ≈ alpha ≈ 128
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
