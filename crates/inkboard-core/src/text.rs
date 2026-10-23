//! Rich text (styled spans), abstract measurement and line wrapping.
//!
//! The engine depends on no font: the interface provides a real [`Measurer`]
//! (the application's fonts); command-line tools use
//! [`ApproxMeasurer`].

use crate::num;
use crate::style::{Color, FontFamily};
use serde::{Deserialize, Serialize};

/// Formatting of a text fragment.
#[expect(clippy::struct_excessive_bools, reason = "independent decorations, stored as is in the board file")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpanStyle {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub underline: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub strike: bool,
    /// Text color; `None`: the object's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    /// Highlight behind the text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub highlight: Option<Color>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub text: String,
    #[serde(flatten)]
    pub style: SpanStyle,
}

/// Text with per-fragment formatting (color, highlight, bold…).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RichText(pub Vec<Span>);

impl RichText {
    pub fn plain(s: impl Into<String>) -> Self {
        let text: String = s.into();
        if text.is_empty() { Self(Vec::new()) } else { Self(vec![Span { text, style: SpanStyle::default() }]) }
    }
    #[must_use]
    pub fn to_plain(&self) -> String {
        self.0.iter().map(|s| s.text.as_str()).collect()
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|s| s.text.is_empty())
    }
    /// Replaces the whole text keeping the style of the first fragment.
    pub fn set_plain(&mut self, s: &str) {
        let style = self.0.first().map(|x| x.style).unwrap_or_default();
        self.0 = if s.is_empty() { Vec::new() } else { vec![Span { text: s.to_owned(), style }] };
    }
    /// Applies `f` to the style of characters `[from, to)` (character indices).
    pub fn restyle(&mut self, from: usize, to: usize, f: impl Fn(&mut SpanStyle)) {
        let mut out: Vec<Span> = Vec::new();
        let mut pos = 0usize;
        for sp in &self.0 {
            for ch in sp.text.chars() {
                let mut st = sp.style;
                if pos >= from && pos < to {
                    f(&mut st);
                }
                match out.last_mut() {
                    Some(l) if l.style == st => l.text.push(ch),
                    _ => out.push(Span { text: ch.to_string(), style: st }),
                }
                pos += 1;
            }
        }
        self.0 = out;
    }
    fn chars_with_style(&self) -> Vec<(char, SpanStyle)> {
        self.0.iter().flat_map(|s| s.text.chars().map(move |c| (c, s.style))).collect()
    }

    fn from_chars(v: Vec<(char, SpanStyle)>) -> Self {
        let mut out: Vec<Span> = Vec::new();
        for (c, st) in v {
            match out.last_mut() {
                Some(l) if l.style == st => l.text.push(c),
                _ => out.push(Span { text: c.to_string(), style: st }),
            }
        }
        Self(out)
    }

    /// Number of characters.
    #[must_use]
    pub fn len_chars(&self) -> usize {
        self.0.iter().map(|s| s.text.chars().count()).sum()
    }

    /// Style of the character at index `i`.
    #[must_use]
    pub fn style_at(&self, i: usize) -> Option<SpanStyle> {
        self.chars_with_style().get(i).map(|c| c.1)
    }

    /// Replaces the characters `[from, to)` with `text`. The inserted text takes the style of the first replaced
    /// character, otherwise that of the previous one (or the next at the start of the text): typing continues "as before".
    pub fn splice(&mut self, from: usize, to: usize, text: &str) {
        let mut v = self.chars_with_style();
        let from = from.min(v.len());
        let to = to.clamp(from, v.len());
        let style = if from < to {
            v[from].1
        } else if from > 0 {
            v[from - 1].1
        } else {
            v.get(to).map(|c| c.1).unwrap_or_default()
        };
        v.splice(from..to, text.chars().map(|c| (c, style)));
        *self = Self::from_chars(v);
    }

    /// Reflects on the styles a modification of the raw text (`old` → `new`), found by common prefix and suffix.
    /// If `old` no longer matches the content (out of sync), the text is simply set again with the first style.
    pub fn apply_edit(&mut self, old: &str, new: &str) {
        if old == new {
            return;
        }
        if self.to_plain() != old {
            self.set_plain(new);
            return;
        }
        let (o, n): (Vec<char>, Vec<char>) = (old.chars().collect(), new.chars().collect());
        let prefix = o.iter().zip(&n).take_while(|(a, b)| a == b).count();
        let max_suffix = o.len().min(n.len()) - prefix;
        let suffix = o.iter().rev().zip(n.iter().rev()).take(max_suffix).take_while(|(a, b)| a == b).count();
        let inserted: String = n[prefix..n.len() - suffix].iter().collect();
        self.splice(prefix, o.len() - suffix, &inserted);
    }

    /// True when all the characters of `[from, to)` satisfy `pred` (false on an empty range).
    pub fn all(&self, from: usize, to: usize, pred: impl Fn(&SpanStyle) -> bool) -> bool {
        let v = self.chars_with_style();
        let to = to.min(v.len());
        from < to && v[from..to].iter().all(|c| pred(&c.1))
    }

    /// Toggles an attribute on `[from, to)`: when it is already everywhere it is removed, otherwise applied everywhere.
    pub fn toggle(&mut self, from: usize, to: usize, get: fn(&SpanStyle) -> bool, set: fn(&mut SpanStyle, bool)) {
        let on = !self.all(from, to, get);
        self.restyle(from, to, |s| set(s, on));
    }
}

/// Formatting applicable to a portion of text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Bold,
    Italic,
    Underline,
    Strike,
    /// Text color; `None`: back to the object's color.
    Color(Option<Color>),
    /// Highlight; `None`: removes it.
    Highlight(Option<Color>),
    /// Removes all formatting.
    Clear,
}

impl Format {
    /// True when the range `[from, to)` already carries this formatting on all its characters.
    #[must_use]
    pub fn is_active(self, rt: &RichText, from: usize, to: usize) -> bool {
        match self {
            Format::Bold => rt.all(from, to, |s| s.bold),
            Format::Italic => rt.all(from, to, |s| s.italic),
            Format::Underline => rt.all(from, to, |s| s.underline),
            Format::Strike => rt.all(from, to, |s| s.strike),
            Format::Color(c) => c.is_some() && rt.all(from, to, |s| s.color == c),
            Format::Highlight(c) => c.is_some() && rt.all(from, to, |s| s.highlight == c),
            Format::Clear => false,
        }
    }
}

impl RichText {
    /// Applies a format to `[from, to)`. Bold, italic, underline and strikethrough toggle;
    /// a color or highlight identical to the existing one is removed (same gesture = remove).
    pub fn format(&mut self, from: usize, to: usize, f: Format) {
        let active = f.is_active(self, from, to);
        match f {
            Format::Bold => self.toggle(from, to, |s| s.bold, |s, v| s.bold = v),
            Format::Italic => self.toggle(from, to, |s| s.italic, |s, v| s.italic = v),
            Format::Underline => self.toggle(from, to, |s| s.underline, |s, v| s.underline = v),
            Format::Strike => self.toggle(from, to, |s| s.strike, |s, v| s.strike = v),
            Format::Color(c) => self.restyle(from, to, |s| s.color = if active { None } else { c }),
            Format::Highlight(c) => self.restyle(from, to, |s| s.highlight = if active { None } else { c }),
            Format::Clear => self.restyle(from, to, |s| *s = SpanStyle::default()),
        }
    }
}

/// Measurement of the text width.
pub trait Measurer {
    fn width(&self, s: &str, font: FontFamily, size: f64, bold: bool) -> f64;
    fn line_height(&self, size: f64) -> f64 {
        size * 1.3
    }
}

/// Approximate measurement (no font): enough for layout outside the interface.
#[derive(Clone, Copy, Debug, Default)]
pub struct ApproxMeasurer;

impl Measurer for ApproxMeasurer {
    fn width(&self, s: &str, font: FontFamily, size: f64, bold: bool) -> f64 {
        let base = match font {
            FontFamily::Sans => 0.53,
            FontFamily::Hand => 0.50,
            FontFamily::Serif => 0.49,
            FontFamily::Mono => 0.60,
        };
        let mut em = 0.0;
        for c in s.chars() {
            em += if font == FontFamily::Mono {
                base
            } else if c == ' ' {
                0.28
            } else if "il.,;:'|!ij".contains(c) {
                base * 0.55
            } else if "mwMW@".contains(c) {
                base * 1.45
            } else if c.is_uppercase() {
                base * 1.2
            } else if (c as u32) >= 0x2E80 {
                1.0
            } else {
                base
            };
        }
        em * size * if bold { 1.06 } else { 1.0 }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub text: String,
    pub style: SpanStyle,
}
pub type Line = Vec<Run>;

/// Result of laying out a block of text.
#[derive(Clone, Debug, Default)]
pub struct TextLayout {
    pub lines: Vec<Line>,
    pub widths: Vec<f64>,
    pub line_h: f64,
    pub width: f64,
    pub height: f64,
}

fn run_width(chars: &[(char, SpanStyle)], m: &dyn Measurer, font: FontFamily, size: f64) -> f64 {
    let mut total = 0.0;
    let mut i = 0;
    while i < chars.len() {
        let bold = chars[i].1.bold;
        let mut j = i;
        let mut s = String::new();
        while j < chars.len() && chars[j].1.bold == bold {
            s.push(chars[j].0);
            j += 1;
        }
        total += m.width(&s, font, size, bold);
        i = j;
    }
    total
}

fn to_line(chars: &[(char, SpanStyle)]) -> Line {
    let mut line: Line = Vec::new();
    for (c, st) in chars {
        match line.last_mut() {
            Some(r) if r.style == *st => r.text.push(*c),
            _ => line.push(Run { text: c.to_string(), style: *st }),
        }
    }
    line
}

/// Lays out `rt`. `max_w = None`: no automatic wrapping (only `\n` breaks).
pub fn layout_rich(rt: &RichText, max_w: Option<f64>, m: &dyn Measurer, font: FontFamily, size: f64) -> TextLayout {
    let chars = rt.chars_with_style();
    let mut rows: Vec<Vec<(char, SpanStyle)>> = Vec::new();
    for para in chars.split(|(c, _)| *c == '\n') {
        let Some(max) = max_w else {
            rows.push(para.to_vec());
            continue;
        };
        if para.is_empty() {
            rows.push(Vec::new());
            continue;
        }
        let mut cur: Vec<(char, SpanStyle)> = Vec::new();
        // tokens: word + the spaces that follow it
        let mut tokens: Vec<&[(char, SpanStyle)]> = Vec::new();
        let mut start = 0;
        for i in 0..para.len() {
            let ends_word = para[i].0 == ' ' && (i + 1 == para.len() || para[i + 1].0 != ' ');
            if ends_word {
                tokens.push(&para[start..=i]);
                start = i + 1;
            }
        }
        if start < para.len() {
            tokens.push(&para[start..]);
        }
        let last_token = tokens.len().saturating_sub(1);
        for (n, tok) in tokens.into_iter().enumerate() {
            let trimmed_len = tok.iter().rposition(|(c, _)| *c != ' ').map_or(0, |p| p + 1);
            let tok_w = run_width(&tok[..trimmed_len], m, font, size);
            let cur_w = run_width(&cur, m, font, size);
            // The editor (egui) counts the space that follows a word when deciding whether it fits, and allows for it by widening the line by one
            // space; the last word of a paragraph has none, so it may end up to one space past the wrap width. The final rendering does the same,
            // otherwise the editor and the rendering would break the line at different places for such a word.
            let max = if n == last_token && trimmed_len == tok.len() { max + m.width(" ", font, size, false) } else { max };
            if !cur.is_empty() && cur_w + tok_w > max {
                // strip trailing spaces before breaking
                while cur.last().is_some_and(|(c, _)| *c == ' ') {
                    cur.pop();
                }
                rows.push(std::mem::take(&mut cur));
            }
            if tok_w > max && cur.is_empty() {
                // word wider than the line: break character by character
                for item in tok {
                    cur.push(*item);
                    if cur.len() > 1
                        && run_width(&cur, m, font, size) > max
                        && let Some(last) = cur.pop()
                    {
                        rows.push(std::mem::take(&mut cur));
                        cur.push(last);
                    }
                }
            } else {
                cur.extend_from_slice(tok);
            }
        }
        while cur.last().is_some_and(|(c, _)| *c == ' ') {
            cur.pop();
        }
        rows.push(cur);
    }
    let line_h = m.line_height(size);
    let widths: Vec<f64> = rows.iter().map(|r| run_width(r, m, font, size)).collect();
    let width = widths.iter().copied().fold(0.0, f64::max);
    TextLayout { height: num::f64_of(rows.len().max(1)) * line_h, lines: rows.iter().map(|r| to_line(r)).collect(), widths, line_h, width }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_roundtrip() {
        let rt = RichText::plain("Hello");
        assert_eq!(rt.to_plain(), "Hello");
        assert!(RichText::plain("").is_empty());
    }

    #[test]
    fn restyle_splits_spans() {
        let mut rt = RichText::plain("abcdef");
        rt.restyle(2, 4, |s| s.bold = true);
        assert_eq!(rt.0.len(), 3);
        assert_eq!(rt.0[1].text, "cd");
        assert!(rt.0[1].style.bold);
        assert_eq!(rt.to_plain(), "abcdef");
    }

    #[test]
    fn wraps_on_words() {
        let rt = RichText::plain("un deux trois quatre cinq six sept");
        let l = layout_rich(&rt, Some(80.0), &ApproxMeasurer, FontFamily::Sans, 16.0);
        assert!(l.lines.len() > 1);
        assert!(l.widths.iter().all(|w| *w <= 80.0 + 1.0));
        let joined: String = l.lines.iter().map(|ln| ln.iter().map(|r| r.text.as_str()).collect::<String>()).collect::<Vec<_>>().join(" ");
        assert_eq!(joined, "un deux trois quatre cinq six sept");
    }

    #[test]
    fn the_last_word_of_a_paragraph_may_end_one_space_past_the_wrap_width() {
        let m = ApproxMeasurer;
        let w = |s: &str| m.width(s, FontFamily::Sans, 16.0, false);
        let max = w("alpha beta") - w(" ") / 2.0;
        // "beta" ends just past `max` but has no space after it: same rule as the editor, it stays on the line
        let l = layout_rich(&RichText::plain("alpha beta"), Some(max), &m, FontFamily::Sans, 16.0);
        assert_eq!(l.lines.len(), 1);
        // followed by a space (or anything else) the word needs to end within `max`
        let l = layout_rich(&RichText::plain("alpha beta gamma"), Some(max), &m, FontFamily::Sans, 16.0);
        assert_eq!(l.lines.len(), 3);
        // two spaces beyond the allowance: it wraps
        let l = layout_rich(&RichText::plain("alpha beta"), Some(max - w(" ")), &m, FontFamily::Sans, 16.0);
        assert_eq!(l.lines.len(), 2);
    }

    #[test]
    fn breaks_very_long_word() {
        let rt = RichText::plain("Supercalifragilisticexpialidocious");
        let l = layout_rich(&rt, Some(60.0), &ApproxMeasurer, FontFamily::Mono, 12.0);
        assert!(l.lines.len() > 1);
    }

    #[test]
    fn explicit_newlines_without_width() {
        let rt = RichText::plain("a\nb\n\nc");
        let l = layout_rich(&rt, None, &ApproxMeasurer, FontFamily::Sans, 10.0);
        assert_eq!(l.lines.len(), 4);
    }

    #[test]
    fn styles_survive_wrapping() {
        let mut rt = RichText::plain("alpha beta gamma");
        rt.restyle(6, 10, |s| s.bold = true);
        let l = layout_rich(&rt, Some(500.0), &ApproxMeasurer, FontFamily::Sans, 14.0);
        assert!(l.lines[0].iter().any(|r| r.style.bold && r.text == "beta"));
    }

    #[test]
    fn typing_inherits_the_previous_style() {
        let mut rt = RichText::plain("ab");
        rt.restyle(0, 2, |s| s.bold = true);
        rt.apply_edit("ab", "abc");
        assert_eq!(rt.0.len(), 1);
        assert!(rt.0[0].style.bold && rt.to_plain() == "abc");
    }

    #[test]
    fn typing_in_the_middle_keeps_neighbour_styles() {
        let mut rt = RichText::plain("alpha beta");
        rt.restyle(6, 10, |s| s.italic = true);
        rt.apply_edit("alpha beta", "alpha XX beta");
        assert_eq!(rt.to_plain(), "alpha XX beta");
        // "XX" inserted before "beta" inherit the previous character (not italic)
        assert!(!rt.style_at(6).unwrap().italic);
        assert!(rt.style_at(9).unwrap().italic, "\"beta\" stays italic");
    }

    #[test]
    fn replacing_a_selection_keeps_its_first_style() {
        let mut rt = RichText::plain("un deux trois");
        rt.restyle(3, 7, |s| s.color = Some(Color::rgb(255, 0, 0)));
        rt.apply_edit("un deux trois", "un 2 trois");
        assert_eq!(rt.to_plain(), "un 2 trois");
        assert_eq!(rt.style_at(3).unwrap().color, Some(Color::rgb(255, 0, 0)));
        assert_eq!(rt.style_at(5).unwrap().color, None);
    }

    #[test]
    fn deletion_and_unicode() {
        let mut rt = RichText::plain("é🙂b");
        rt.restyle(1, 2, |s| s.bold = true);
        rt.apply_edit("é🙂b", "éb");
        assert_eq!(rt.to_plain(), "éb");
        assert!(!rt.style_at(1).unwrap().bold);
        assert_eq!(rt.len_chars(), 2);
        rt.apply_edit("éb", "");
        assert!(rt.is_empty());
    }

    #[test]
    fn desynchronised_text_is_reset_instead_of_panicking() {
        let mut rt = RichText::plain("abc");
        rt.apply_edit("autre chose", "xyz");
        assert_eq!(rt.to_plain(), "xyz");
    }

    #[test]
    fn toggle_applies_then_removes() {
        let mut rt = RichText::plain("abcdef");
        rt.toggle(1, 4, |s| s.bold, |s, v| s.bold = v);
        assert!(rt.all(1, 4, |s| s.bold) && !rt.all(0, 6, |s| s.bold));
        rt.toggle(1, 4, |s| s.bold, |s, v| s.bold = v);
        assert!(!rt.all(0, 6, |s| s.bold));
        // selection partly bold: apply everywhere
        rt.restyle(0, 2, |s| s.bold = true);
        rt.toggle(0, 4, |s| s.bold, |s, v| s.bold = v);
        assert!(rt.all(0, 4, |s| s.bold));
        assert!(!rt.all(0, 0, |s| s.bold), "empty range: false");
    }

    #[test]
    fn format_toggles_and_sets() {
        let mut rt = RichText::plain("un deux trois");
        let red = Color::rgb(224, 49, 49);
        let yellow = Color::rgb(255, 236, 153);
        rt.format(3, 7, Format::Bold);
        rt.format(3, 7, Format::Color(Some(red)));
        rt.format(3, 7, Format::Highlight(Some(yellow)));
        let st = rt.style_at(4).unwrap();
        assert!(st.bold && st.color == Some(red) && st.highlight == Some(yellow));
        assert_eq!(rt.style_at(2).unwrap(), SpanStyle::default(), "outside the selection: unchanged");
        assert_eq!(rt.style_at(7).unwrap(), SpanStyle::default());
        // same color a second time: remove it
        rt.format(3, 7, Format::Color(Some(red)));
        assert_eq!(rt.style_at(4).unwrap().color, None);
        // another color: replace
        rt.format(3, 7, Format::Color(Some(red)));
        rt.format(3, 7, Format::Color(Some(yellow)));
        assert_eq!(rt.style_at(4).unwrap().color, Some(yellow), "another tint replaces the old one");
    }

    #[test]
    fn format_clear_removes_everything() {
        let mut rt = RichText::plain("abc");
        rt.format(0, 3, Format::Bold);
        rt.format(0, 3, Format::Underline);
        rt.format(0, 3, Format::Clear);
        assert_eq!(rt.0.len(), 1);
        assert_eq!(rt.0[0].style, SpanStyle::default());
    }
}
