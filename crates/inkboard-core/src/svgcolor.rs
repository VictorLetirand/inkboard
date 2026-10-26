//! Colors of an SVG: list the distinct colors it paints with and replace them in a single pass.
//!
//! Only explicit colors are seen: the `fill`, `stroke`, `stop-color`, `flood-color`, `lighting-color` and `color`
//! properties, as attributes, in `style="..."` attributes and in `<style>` blocks. Accepted values are `#rgb`, `#rrggbb`,
//! `rgb(r, g, b)` and the common CSS keywords; `none`, `currentColor`, `url(#...)` and the rest are left alone.
//!
//! Implicit black: an SVG whose drawing elements never mention `fill` paints them black. That color is reported
//! as `(0, 0, 0)` (first of the list) and, when it is remapped, a `fill` attribute is injected on the root `<svg>`
//! (inherited by every shape). Mixed documents (some shapes without `fill` next to others with one) keep the plain behavior:
//! their implicit black is not reported.

use crate::num;

/// An sRGB color.
pub type Rgb = (u8, u8, u8);

const PROPS: [&str; 6] = ["stop-color", "flood-color", "lighting-color", "stroke", "fill", "color"];
const SHAPES: [&str; 8] = ["<path", "<rect", "<circle", "<ellipse", "<polygon", "<polyline", "<text", "<line"];

const KEYWORDS: [(&str, Rgb); 20] = [
    ("black", (0, 0, 0)),
    ("white", (255, 255, 255)),
    ("red", (255, 0, 0)),
    ("green", (0, 128, 0)),
    ("blue", (0, 0, 255)),
    ("gray", (128, 128, 128)),
    ("grey", (128, 128, 128)),
    ("yellow", (255, 255, 0)),
    ("orange", (255, 165, 0)),
    ("purple", (128, 0, 128)),
    ("cyan", (0, 255, 255)),
    ("aqua", (0, 255, 255)),
    ("magenta", (255, 0, 255)),
    ("fuchsia", (255, 0, 255)),
    ("lime", (0, 255, 0)),
    ("navy", (0, 0, 128)),
    ("teal", (0, 128, 128)),
    ("maroon", (128, 0, 0)),
    ("silver", (192, 192, 192)),
    ("pink", (255, 192, 203)),
];

/// One color value found in the text: its byte range and its color.
struct Hit {
    start: usize,
    end: usize,
    rgb: Rgb,
}

struct Scan {
    hits: Vec<Hit>,
    /// A `fill` property exists somewhere (with any value).
    has_fill: bool,
}

fn parse_color(v: &str) -> Option<Rgb> {
    let v = v.trim().to_ascii_lowercase();
    if let Some(h) = v.strip_prefix('#') {
        if !h.is_ascii() {
            return None;
        }
        let d = |i: usize, n: usize| u8::from_str_radix(&h[i..i + n], 16).ok();
        return match h.len() {
            // `#rgba` and `#rrggbbaa`: the alpha is kept by `apply_recolor`
            3 | 4 => (h.len() == 3 || d(3, 1).is_some()).then_some((d(0, 1)? * 17, d(1, 1)? * 17, d(2, 1)? * 17)),
            6 | 8 => (h.len() == 6 || d(6, 2).is_some()).then_some((d(0, 2)?, d(2, 2)?, d(4, 2)?)),
            _ => None,
        };
    }
    if let Some(args) = v.strip_prefix("rgb(").and_then(|r| r.strip_suffix(')')) {
        let mut it = args.split(',').map(str::trim);
        let mut ch = || {
            let s = it.next()?;
            match s.strip_suffix('%') {
                Some(p) => Some(num::to_u8((p.trim().parse::<f64>().ok()? * 2.55).round())),
                None => Some(num::to_u8(s.parse::<f64>().ok()?.round())),
            }
        };
        let (r, g, b) = (ch()?, ch()?, ch()?);
        return it.next().is_none().then_some((r, g, b));
    }
    KEYWORDS.iter().find(|(k, _)| *k == v).map(|&(_, c)| c)
}

fn scan(text: &str) -> Scan {
    let lower = text.to_ascii_lowercase();
    let b = lower.as_bytes();
    let mut out = Scan { hits: Vec::new(), has_fill: false };
    let mut i = 0;
    while i < b.len() {
        let prev_ok = i == 0 || !(b[i - 1].is_ascii_alphanumeric() || matches!(b[i - 1], b'-' | b'_' | b':' | b'.'));
        let Some(name) = PROPS.iter().find(|p| prev_ok && lower[i..].starts_with(**p)) else {
            i += 1;
            continue;
        };
        let mut j = i + name.len();
        i = j;
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        let Some(&sep) = b.get(j) else { break };
        if sep != b'=' && sep != b':' {
            continue;
        }
        if *name == "fill" {
            out.has_fill = true;
        }
        j += 1;
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        let stops: &[u8] = if sep == b'=' {
            match b.get(j) {
                Some(&q @ (b'"' | b'\'')) => {
                    j += 1;
                    if q == b'"' { b"\"" } else { b"'" }
                }
                _ => continue,
            }
        } else {
            b";}\"'<!"
        };
        let end = b[j..].iter().position(|c| stops.contains(c)).map_or(b.len(), |p| j + p);
        let raw = &text[j..end];
        let start = j + (raw.len() - raw.trim_start().len());
        let end = start + raw.trim().len();
        if let Some(rgb) = parse_color(&text[start..end]) {
            out.hits.push(Hit { start, end, rgb });
        }
        i = end;
    }
    out
}

/// Is black implicit (shapes, and no `fill` property at all)?
fn implicit_black(text: &str, s: &Scan) -> bool {
    if s.has_fill {
        return false;
    }
    let lower = text.to_ascii_lowercase();
    SHAPES.iter().any(|t| lower.contains(t))
}

/// Distinct explicit colors of the SVG, in order of appearance (implicit black first, see the module docs).
#[must_use]
pub fn svg_colors(data: &[u8]) -> Vec<Rgb> {
    let Ok(text) = std::str::from_utf8(data) else { return Vec::new() };
    let s = scan(text);
    let mut out = Vec::new();
    if implicit_black(text, &s) {
        out.push((0, 0, 0));
    }
    for h in &s.hits {
        if !out.contains(&h.rgb) {
            out.push(h.rgb);
        }
    }
    out
}

fn hex((r, g, b): Rgb) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Alpha digits of an `#rgba` / `#rrggbbaa` value, as two digits (empty for any other value).
fn alpha(v: &str) -> String {
    match v.strip_prefix('#').filter(|h| h.is_ascii()) {
        Some(h) if h.len() == 4 => h[3..].repeat(2),
        Some(h) if h.len() == 8 => h[6..].to_owned(),
        _ => String::new(),
    }
}

/// Replaces colors in one pass: each occurrence is looked up in `map` (original, new) once, so `A -> B` and `B -> C`
/// never chain. An empty map, or one matching nothing, returns the bytes unchanged.
#[must_use]
pub fn apply_recolor(data: &[u8], map: &[(Rgb, Rgb)]) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(data) else { return data.to_vec() };
    if map.is_empty() {
        return data.to_vec();
    }
    let find = |c: Rgb| map.iter().find(|(from, _)| *from == c).map(|&(_, to)| to);
    let s = scan(text);
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for h in &s.hits {
        if let Some(to) = find(h.rgb) {
            out.push_str(&text[at..h.start]);
            out.push_str(&hex(to));
            out.push_str(&alpha(&text[h.start..h.end]));
            at = h.end;
        }
    }
    out.push_str(&text[at..]);
    if implicit_black(text, &s)
        && let Some(to) = find((0, 0, 0))
    {
        let lower = out.to_ascii_lowercase();
        if let Some(p) = lower.find("<svg").filter(|p| !lower.as_bytes().get(p + 4).is_some_and(u8::is_ascii_alphanumeric)) {
            out.insert_str(p + 4, &format!(" fill=\"{}\"", hex(to)));
        }
    }
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_are_found_in_every_syntax_in_order_of_appearance() {
        let svg = br##"<svg><style>.a{fill:#F00;stroke : blue}</style>
            <path fill="#0f0" stroke='rgb(1, 2, 3)' style="stop-color:Gray; fill-opacity:.5"/>
            <rect fill="none" stroke="currentColor" color="url(#g)"/><circle fill="#ff0000" stroke="rgb(100%,0%,0%)"/></svg>"##;
        assert_eq!(svg_colors(svg), vec![(255, 0, 0), (0, 0, 255), (0, 255, 0), (1, 2, 3), (128, 128, 128)]);
    }

    #[test]
    fn similar_property_names_are_not_colors() {
        let svg = br##"<svg><path fill="#112233" stroke-width="2" fill-opacity="1" background-color="red" data-fill="blue"/></svg>"##;
        assert_eq!(svg_colors(svg), vec![(0x11, 0x22, 0x33)]);
    }

    #[test]
    fn recoloring_does_not_chain() {
        let svg = br##"<svg><path fill="#ff0000" stroke="#00ff00"/><path fill="#00ff00" style="stroke:red"/></svg>"##;
        let map = [((255, 0, 0), (0, 255, 0)), ((0, 255, 0), (0, 0, 255))];
        let out = String::from_utf8(apply_recolor(svg, &map)).unwrap();
        assert_eq!(out, r##"<svg><path fill="#00ff00" stroke="#0000ff"/><path fill="#0000ff" style="stroke:#00ff00"/></svg>"##);
    }

    #[test]
    fn colors_with_alpha_are_found_and_keep_their_alpha() {
        let svg = br##"<svg><path fill="#FF000080"/><path style="stroke:#0f0c"/><path fill="#ff00008"/></svg>"##;
        assert_eq!(svg_colors(svg), vec![(255, 0, 0), (0, 255, 0)]);
        let map = [((255, 0, 0), (0, 0, 255)), ((0, 255, 0), (1, 2, 3))];
        let out = String::from_utf8(apply_recolor(svg, &map)).unwrap();
        assert_eq!(out, r##"<svg><path fill="#0000ff80"/><path style="stroke:#010203cc"/><path fill="#ff00008"/></svg>"##);
    }

    #[test]
    fn empty_or_unmatched_map_keeps_the_bytes() {
        let svg = br##"<svg><path fill="#ff0000"/></svg>"##;
        assert_eq!(apply_recolor(svg, &[]), svg.to_vec());
        assert_eq!(apply_recolor(svg, &[((1, 2, 3), (4, 5, 6))]), svg.to_vec());
    }

    #[test]
    fn implicit_black_is_reported_and_remapped_on_the_root() {
        let svg = br#"<svg viewBox="0 0 1 1"><path d="M0 0h1v1z"/></svg>"#;
        assert_eq!(svg_colors(svg), vec![(0, 0, 0)]);
        let out = String::from_utf8(apply_recolor(svg, &[((0, 0, 0), (255, 0, 0))])).unwrap();
        assert_eq!(out, r##"<svg fill="#ff0000" viewBox="0 0 1 1"><path d="M0 0h1v1z"/></svg>"##);
        let with_fill = br##"<svg><path fill="none" stroke="#fff"/></svg>"##;
        assert_eq!(svg_colors(with_fill), vec![(255, 255, 255)]);
    }

    #[test]
    fn non_utf8_data_is_left_alone() {
        assert!(svg_colors(&[0xff, 0xfe]).is_empty());
        assert_eq!(apply_recolor(&[0xff, 0xfe], &[((0, 0, 0), (1, 1, 1))]), vec![0xff, 0xfe]);
    }
}
