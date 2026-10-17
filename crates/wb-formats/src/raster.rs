//! Rendu PNG d'un SVG exporté par le moteur (pur Rust, sans fenêtre).

use crate::common::FormatError;
use resvg::{tiny_skia, usvg};

/// Convertit un SVG en PNG. `scale` : 1.0 = taille du SVG, 2.0 = double résolution.
pub fn svg_to_png(svg: &str, scale: f32) -> Result<Vec<u8>, FormatError> {
    let mut fontdb = usvg::fontdb::Database::new();
    fontdb.load_system_fonts();
    let opt = usvg::Options { fontdb: std::sync::Arc::new(fontdb), ..usvg::Options::default() };
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
    fn rejects_broken_svg() {
        assert!(svg_to_png("<svg", 1.0).is_err());
    }
}
