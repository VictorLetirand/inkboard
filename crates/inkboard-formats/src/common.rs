//! Types et utilitaires partagés par les importeurs / exporteurs.

use base64::{Engine, engine::general_purpose::STANDARD};
use inkboard_core::{BgPreset, Color, Document, FontFamily};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FormatError {
    #[error("format de fichier non reconnu")]
    Unknown,
    #[error("fichier invalide : {0}")]
    Invalid(String),
    #[error("{0}")]
    Unsupported(String),
}

impl From<serde_json::Error> for FormatError {
    fn from(e: serde_json::Error) -> Self {
        FormatError::Invalid(format!("JSON : {e}"))
    }
}

/// Ce qui n'a pas pu être repris fidèlement lors d'un import.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Éléments importés sous une forme approchée (ex. cylindre → rectangle).
    pub approximated: usize,
    /// Éléments ignorés (non gérés ou invalides).
    pub skipped: usize,
}

impl Report {
    pub fn summary(&self) -> String {
        let mut p = Vec::new();
        if self.approximated > 0 {
            p.push(format!("{} élément(s) approximé(s)", self.approximated));
        }
        if self.skipped > 0 {
            p.push(format!("{} élément(s) ignoré(s)", self.skipped));
        }
        p.join(", ")
    }
}

#[derive(Debug)]
pub struct Imported {
    pub doc: Document,
    pub report: Report,
}

/// Analyse une couleur CSS ; le noir pur est converti en « encre » (il suit le fond clair/sombre).
pub fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim().to_ascii_lowercase();
    let named = match s.as_str() {
        "none" | "transparent" | "default" | "" => return None,
        "black" => "#000000",
        "white" => "#ffffff",
        "red" => "#ff0000",
        "green" => "#008000",
        "blue" => "#0000ff",
        "yellow" => "#ffff00",
        "gray" | "grey" => "#808080",
        "orange" => "#ffa500",
        "purple" => "#800080",
        other => {
            // « light-dark(#aaa, #bbb) » et assimilés : première couleur trouvée
            let i = other.find('#')?;
            let hex: String = other[i..].chars().take_while(|c| *c == '#' || c.is_ascii_hexdigit()).collect();
            return Color::parse(&hex).map(ink_if_black);
        }
    };
    Color::parse(named).map(ink_if_black)
}

pub fn ink_if_black(c: Color) -> Color {
    match c {
        Color::Rgb(0, 0, 0) | Color::Rgb(0x1e, 0x1e, 0x1e) | Color::Rgb(0x19, 0x19, 0x19) => Color::Ink,
        c => c,
    }
}

pub fn parse_data_url(s: &str) -> Option<(String, Vec<u8>)> {
    let rest = s.strip_prefix("data:")?;
    let (meta, payload) = rest.split_once(',')?;
    let mime = meta.split(';').next().filter(|m| !m.is_empty()).unwrap_or("application/octet-stream").to_owned();
    if meta.contains(";base64")
        || (!payload.contains('%') && !payload.contains('<') && payload.bytes().all(|b| b.is_ascii_alphanumeric() || b"+/=\r\n ".contains(&b)))
    {
        let clean: String = payload.chars().filter(|c| !c.is_whitespace()).collect();
        STANDARD.decode(clean.as_bytes()).ok().map(|d| (mime, d))
    } else {
        let bytes = percent_decode(payload);
        Some((mime, bytes))
    }
}

pub fn data_url(mime: &str, data: &[u8]) -> String {
    format!("data:{mime};base64,{}", STANDARD.encode(data))
}

pub fn percent_decode(s: &str) -> Vec<u8> {
    percent_encoding::percent_decode_str(s).collect()
}

pub fn font_family(name: &str) -> FontFamily {
    let n = name.to_ascii_lowercase();
    if ["mono", "courier", "consolas", "cascadia"].iter().any(|k| n.contains(k)) {
        FontFamily::Mono
    } else if ["print", "comic", "hand", "virgil", "kalam", "caveat", "excalifont"].iter().any(|k| n.contains(k)) {
        FontFamily::Hand
    } else if ["times", "georgia", "garamond", "serif"].iter().any(|k| n.contains(k)) && !n.contains("sans") {
        FontFamily::Serif
    } else {
        FontFamily::Sans
    }
}

pub fn nearest_background(c: Color) -> BgPreset {
    BgPreset::nearest(c.resolve(false))
}

pub fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors() {
        assert_eq!(parse_color("#000000"), Some(Color::Ink));
        assert_eq!(parse_color("#1e1e1e"), Some(Color::Ink));
        assert_eq!(parse_color("#ff0000"), Some(Color::Rgb(255, 0, 0)));
        assert_eq!(parse_color("none"), None);
        assert_eq!(parse_color("default"), None);
        assert_eq!(parse_color("light-dark(#d5e8d4, #1f2f1e)"), Some(Color::Rgb(0xd5, 0xe8, 0xd4)));
        assert_eq!(parse_color("white"), Some(Color::Rgb(255, 255, 255)));
    }

    #[test]
    fn data_urls() {
        let u = data_url("image/png", &[1, 2, 3, 250]);
        assert_eq!(parse_data_url(&u), Some(("image/png".into(), vec![1, 2, 3, 250])));
        // variante draw.io : base64 sans « ;base64 »
        assert_eq!(parse_data_url("data:image/png,AQID+g=="), Some(("image/png".into(), vec![1, 2, 3, 250])));
        // SVG non encodé
        let (m, d) = parse_data_url("data:image/svg+xml,%3Csvg%3E").unwrap();
        assert_eq!(m, "image/svg+xml");
        assert_eq!(d, b"<svg>");
    }

    #[test]
    fn fonts() {
        assert_eq!(font_family("Courier New"), FontFamily::Mono);
        assert_eq!(font_family("Segoe Print"), FontFamily::Hand);
        assert_eq!(font_family("Helvetica"), FontFamily::Sans);
        assert_eq!(font_family("Times New Roman"), FontFamily::Serif);
    }
}
