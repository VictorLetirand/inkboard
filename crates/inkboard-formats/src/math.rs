//! Typesetting of LaTeX (math) formulas to vector SVG, with the embedded STIX Two Math font (`latex-rust` crate, 100 % Rust).
//!
//! The SVG produced is drawn in **white**: the application tints it with the object's stroke color, and the SVG export replaces
//! the white with that color (see `inkboard_core::svg`).

use crate::common::FormatError;
use inkboard_core::num;
use latex_rust::{Color, Dim, MathFont, SvgOptions, latex_to_svg};

/// Fill color of the glyphs in the asset (to be tinted when drawn and exported).
pub const FILL: &str = "#ffffff";

/// Typeset formula: SVG document and dimensions (`viewBox` units).
#[derive(Debug)]
pub struct Formula {
    pub svg: String,
    pub w: f64,
    pub h: f64,
}

/// Typesets `latex` (math mode); `size` = text size (board units).
///
/// # Errors
///
/// [`FormatError`] when the formula is empty, cannot be parsed, or has no dimensions.
pub fn render(latex: &str, size: f64) -> Result<Formula, FormatError> {
    if latex.trim().is_empty() {
        return Err(FormatError::Invalid("empty formula".into()));
    }
    let font = MathFont::stix_two_math().map_err(|e| FormatError::Unsupported(format!("math font: {e}")))?;
    let opts = SvgOptions { font_size_pt: Dim::from_i64(num::to_i64(size.round().max(6.0))), color: Color::rgb(255, 255, 255), display: true };
    let svg = latex_to_svg(latex, &font, &opts).map_err(|e| FormatError::Invalid(format!("LaTeX : {e}")))?;
    let vb = svg.split("viewBox=\"").nth(1).and_then(|s| s.split('"').next()).ok_or_else(|| FormatError::Invalid("SVG without viewBox".into()))?;
    let v: Vec<f64> = vb.split_whitespace().filter_map(|t| t.parse().ok()).collect();
    let (&w, &h) =
        (v.get(2).ok_or_else(|| FormatError::Invalid("invalid viewBox".into()))?, v.get(3).ok_or_else(|| FormatError::Invalid("invalid viewBox".into()))?);
    if w <= 0.0 || h <= 0.0 {
        return Err(FormatError::Invalid("formula without dimensions".into()));
    }
    Ok(Formula { svg, w, h })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composes_fractions_roots_and_integrals() {
        for src in [r"\frac{a+b}{2}", r"\int_0^\infty e^{-x^2}\,dx = \frac{\sqrt{\pi}}{2}", r"\sum_{i=1}^{n} i = \frac{n(n+1)}{2}"] {
            let f = render(src, 32.0).unwrap_or_else(|e| panic!("{src} : {e}"));
            assert!(f.w > 10.0 && f.h > 10.0, "{src}: {}×{}", f.w, f.h);
            assert!(f.svg.contains(FILL) && f.svg.contains("<path"), "glyphs as white paths");
        }
    }

    #[test]
    fn a_bigger_size_gives_a_bigger_formula() {
        let (a, b) = (render("x^2", 20.0).unwrap(), render("x^2", 40.0).unwrap());
        assert!(b.w > a.w * 1.8 && b.h > a.h * 1.8);
    }

    #[test]
    fn rejects_empty_and_unknown_commands() {
        assert!(render("  ", 32.0).is_err());
        assert!(render(r"\notacommand{x}", 32.0).is_err());
    }

    #[test]
    fn matrices_with_line_breaks_compose() {
        assert!(render(r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}", 32.0).is_ok());
    }
}
