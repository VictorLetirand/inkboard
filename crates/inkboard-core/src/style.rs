//! Colors, stroke and text styles.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Color: "ink" (black on a light background, white on a dark one) or fixed RGB.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Color {
    #[default]
    Ink,
    Rgb(u8, u8, u8),
}

impl Color {
    #[must_use]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Color::Rgb(r, g, b)
    }

    /// Parses `"ink"`, `"#rgb"` or `"#rrggbb"` (`#rrggbbaa`: the alpha is ignored).
    #[must_use]
    pub fn parse(s: &str) -> Option<Color> {
        let s = s.trim();
        if s.eq_ignore_ascii_case("ink") {
            return Some(Color::Ink);
        }
        let h = s.strip_prefix('#')?;
        let v = |i: usize, n: usize| u8::from_str_radix(h.get(i..i + n)?, 16).ok();
        match h.len() {
            3 => {
                let c = |i| v(i, 1).map(|x| x * 17);
                Some(Color::Rgb(c(0)?, c(1)?, c(2)?))
            }
            6 | 8 => Some(Color::Rgb(v(0, 2)?, v(2, 2)?, v(4, 2)?)),
            _ => None,
        }
    }

    #[must_use]
    pub fn to_hex(self) -> String {
        match self {
            Color::Ink => "ink".to_owned(),
            Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        }
    }

    /// Concrete color depending on whether the background is dark.
    #[must_use]
    pub fn resolve(self, dark_bg: bool) -> (u8, u8, u8) {
        match self {
            Color::Ink if dark_bg => (0xf1, 0xf3, 0xf5),
            Color::Ink => (0x1e, 0x1e, 0x1e),
            Color::Rgb(r, g, b) => (r, g, b),
        }
    }

    #[must_use]
    pub fn resolve_hex(self, dark_bg: bool) -> String {
        let (r, g, b) = self.resolve(dark_bg);
        format!("#{r:02x}{g:02x}{b:02x}")
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}
impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Color::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("invalid color: {s}")))
    }
}

/// Base palette offered in the interface.
pub const PALETTE: [Color; 10] = [
    Color::Ink,
    Color::rgb(0xe0, 0x31, 0x31),
    Color::rgb(0xf0, 0x8c, 0x00),
    Color::rgb(0xf2, 0xc6, 0x00),
    Color::rgb(0x2f, 0x9e, 0x44),
    Color::rgb(0x0c, 0x85, 0x99),
    Color::rgb(0x19, 0x71, 0xc2),
    Color::rgb(0x70, 0x48, 0xe8),
    Color::rgb(0xd6, 0x33, 0x6c),
    Color::rgb(0x86, 0x8e, 0x96),
];
/// Remplissages pastel.
pub const FILLS: [Color; 10] = [
    Color::rgb(0xff, 0xc9, 0xc9),
    Color::rgb(0xff, 0xd8, 0xa8),
    Color::rgb(0xff, 0xec, 0x99),
    Color::rgb(0xb2, 0xf2, 0xbb),
    Color::rgb(0x99, 0xe9, 0xf2),
    Color::rgb(0xa5, 0xd8, 0xff),
    Color::rgb(0xd0, 0xbf, 0xff),
    Color::rgb(0xfc, 0xc2, 0xd7),
    Color::rgb(0xde, 0xe2, 0xe6),
    Color::rgb(0xff, 0xff, 0xff),
];
pub const NOTE_YELLOW: Color = Color::rgb(0xff, 0xec, 0x99);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dash {
    #[default]
    Solid,
    Dashed,
    Dotted,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FontFamily {
    #[default]
    Sans,
    Hand,
    Serif,
    Mono,
}

impl FontFamily {
    /// CSS font stack (single quotes, usable in an SVG attribute).
    #[must_use]
    pub fn css(self) -> &'static str {
        match self {
            FontFamily::Sans => "'Segoe UI', system-ui, -apple-system, Roboto, Helvetica, Arial, sans-serif",
            FontFamily::Hand => "'Segoe Print', 'Bradley Hand', 'Comic Sans MS', 'Chalkboard SE', cursive",
            FontFamily::Serif => "Georgia, 'Times New Roman', serif",
            FontFamily::Mono => "Consolas, 'SF Mono', 'Courier New', monospace",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// Graphic appearance common to all objects.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Style {
    pub stroke: Color,
    /// `None` : transparent.
    #[serde(default)]
    pub fill: Option<Color>,
    /// Stroke width (0: no outline).
    pub width: f64,
    #[serde(default)]
    pub dash: Dash,
    /// 0.0 (invisible) to 1.0 (opaque).
    pub opacity: f32,
    /// Corner rounding, from 0.0 (sharp) to 1.0 (capsule).
    #[serde(default)]
    pub roundness: f32,
    /// Soft drop shadow under the object.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub shadow: bool,
}

impl Default for Style {
    fn default() -> Self {
        Self { stroke: Color::Ink, fill: None, width: 3.0, dash: Dash::Solid, opacity: 1.0, roundness: 0.0, shadow: false }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_colors() {
        assert_eq!(Color::parse("ink"), Some(Color::Ink));
        assert_eq!(Color::parse("#fff"), Some(Color::Rgb(255, 255, 255)));
        assert_eq!(Color::parse("#1971c2"), Some(Color::Rgb(0x19, 0x71, 0xc2)));
        assert_eq!(Color::parse("#1971c2ff"), Some(Color::Rgb(0x19, 0x71, 0xc2)));
        assert_eq!(Color::parse("rouge"), None);
        assert_eq!(Color::parse("#12"), None);
    }

    #[test]
    fn color_serde_roundtrip() {
        let j = serde_json::to_string(&Color::Rgb(1, 2, 3)).unwrap();
        assert_eq!(j, "\"#010203\"");
        assert_eq!(serde_json::from_str::<Color>(&j).unwrap(), Color::Rgb(1, 2, 3));
        assert_eq!(serde_json::from_str::<Color>("\"ink\"").unwrap(), Color::Ink);
    }

    #[test]
    fn shadow_is_written_only_when_on_and_an_old_style_is_rewritten_identically() {
        let old = r##"{"stroke":"ink","fill":null,"width":3.0,"dash":"solid","opacity":1.0,"roundness":0.0}"##;
        let s: Style = serde_json::from_str(old).unwrap();
        assert!(!s.shadow);
        assert_eq!(serde_json::to_value(&s).unwrap(), serde_json::from_str::<serde_json::Value>(old).unwrap(), "no new key for a style without shadow");
        let on = Style { shadow: true, ..s };
        let json = serde_json::to_string(&on).unwrap();
        assert!(json.contains(r#""shadow":true"#), "{json}");
        assert_eq!(serde_json::from_str::<Style>(&json).unwrap(), on);
    }

    #[test]
    fn ink_follows_background() {
        assert_eq!(Color::Ink.resolve(false), (0x1e, 0x1e, 0x1e));
        assert_eq!(Color::Ink.resolve(true), (0xf1, 0xf3, 0xf5));
    }
}
