//! Rich text for the tools: the Markdown subset (`markdown: true`), its inverse for reading, and the `format_text` tool
//! (see `context/PLAN_MCP_V2.md`, lot 4).
//!
//! Syntax: `**bold**`, `*italic*` (or `_italic_`), `__underline__`, `~~strike~~`, `{color=#rrggbb bg=#rrggbb}…{/}` (`ink` is also a color),
//! `\` escapes `* _ ~ { } \`. Layers may overlap freely (`**a *b** c*`). Markup that cannot be applied (unclosed, empty, unmatched `{/}`,
//! invalid color) stays as literal text and is reported in the warnings: no text is ever lost.

use super::{Res, find, obj, page_prop, path_prop, s};
use inkboard_core::{ApproxMeasurer, Color, Editor, Kind as ObjKind, Object, RichText, Span, SpanStyle};
use serde_json::{Value, json};
use std::fmt::Write;

/// Tools exposed by this module.
pub(super) const NAMES: &[&str] = &["format_text"];
/// Those that do not modify the board.
pub(super) const READ_ONLY: &[&str] = &[];

/// Characters that `\` escapes (and that `to_markdown` always escapes).
const SPECIAL: &str = "*_~{}\\";
/// Warnings beyond this number are summarized in one line.
const MAX_WARNINGS: usize = 20;

/* ------------------------------------------------------- Markdown → RichText */

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Bold,
    ItalicStar,
    ItalicUnder,
    Underline,
    Strike,
    /// `{color=… bg=…}`
    Tag(Option<Color>, Option<Color>),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Role {
    Pending,
    Open,
    Close,
    Literal,
}

#[derive(Debug)]
struct Mark {
    kind: Kind,
    /// The source text of the marker, kept when it turns out to be literal.
    raw: String,
    /// Character index in the source.
    at: usize,
    /// Number of characters seen before this marker (to detect empty pairs).
    texts: usize,
    role: Role,
    /// For a closing marker: the opening one.
    opener: usize,
}

#[derive(Debug)]
enum Piece {
    Text(char),
    Mark(usize),
}

/// Markers are layers of a set: each toggle kind is open at most once and closes wherever it is, so crossings (`**a *b** c*`) need no special case.
/// `{/}` closes the last open tag.
#[derive(Debug, Default)]
struct Parser {
    marks: Vec<Mark>,
    pieces: Vec<Piece>,
    /// Open markers (indices in `marks`), oldest first.
    active: Vec<usize>,
    texts: usize,
    warnings: Vec<String>,
}

impl Parser {
    fn text(&mut self, c: char) {
        self.pieces.push(Piece::Text(c));
        self.texts += 1;
    }

    fn literal(&mut self, raw: &[char]) {
        for &c in raw {
            self.text(c);
        }
    }

    fn mark(&mut self, kind: Kind, raw: &str, at: usize) -> usize {
        self.marks.push(Mark { kind, raw: raw.to_owned(), at, texts: self.texts, role: Role::Pending, opener: usize::MAX });
        self.pieces.push(Piece::Mark(self.marks.len() - 1));
        self.marks.len() - 1
    }

    /// `m` closes the open marker at `active[pos]`, unless nothing lies between them.
    fn close_with(&mut self, pos: usize, m: usize) {
        let o = self.active.remove(pos);
        if self.marks[o].texts == self.texts {
            let (a, b) = (&self.marks[o], &self.marks[m]);
            self.warnings.push(format!("empty `{}` … `{}` at character {}: kept as literal text", a.raw, b.raw, a.at));
            self.marks[o].role = Role::Literal;
            self.marks[m].role = Role::Literal;
        } else {
            self.marks[o].role = Role::Open;
            self.marks[m].role = Role::Close;
            self.marks[m].opener = o;
        }
    }

    fn toggle(&mut self, kind: Kind, raw: &str, at: usize) {
        let pos = self.active.iter().position(|&m| self.marks[m].kind == kind);
        let m = self.mark(kind, raw, at);
        match pos {
            Some(pos) => self.close_with(pos, m),
            None => self.active.push(m),
        }
    }

    fn open_tag(&mut self, color: Option<Color>, bg: Option<Color>, raw: &str, at: usize) {
        let m = self.mark(Kind::Tag(color, bg), raw, at);
        self.active.push(m);
    }

    fn close_tag(&mut self, at: usize) {
        let pos = self.active.iter().rposition(|&m| matches!(self.marks[m].kind, Kind::Tag(..)));
        let m = self.mark(Kind::Tag(None, None), "{/}", at);
        if let Some(pos) = pos {
            self.close_with(pos, m);
        } else {
            self.marks[m].role = Role::Literal;
            self.warnings.push(format!("`{{/}}` at character {at} closes nothing: kept as literal text"));
        }
    }

    /// A single `_` follows the usual Markdown flanking rules, so `snake_case_name` stays plain.
    fn underscore(&mut self, chars: &[char], at: usize) {
        let prev = at.checked_sub(1).and_then(|j| chars.get(j)).copied();
        let next = chars.get(at + 1).copied();
        let can_open = next.is_some_and(|c| !c.is_whitespace()) && !prev.is_some_and(char::is_alphanumeric);
        let can_close = prev.is_some_and(|c| !c.is_whitespace()) && !next.is_some_and(char::is_alphanumeric);
        let is_open = self.active.iter().any(|&m| self.marks[m].kind == Kind::ItalicUnder);
        if (is_open && can_close) || (!is_open && can_open) {
            self.toggle(Kind::ItalicUnder, "_", at);
        } else {
            self.text('_');
        }
    }

    fn finish(mut self) -> (RichText, Vec<String>) {
        for m in std::mem::take(&mut self.active) {
            let mk = &self.marks[m];
            self.warnings.push(format!("unclosed `{}` at character {}: kept as literal text", mk.raw, mk.at));
            self.marks[m].role = Role::Literal;
        }
        let mut layers: Vec<usize> = Vec::new();
        let mut out: Vec<(char, SpanStyle)> = Vec::new();
        for piece in &self.pieces {
            match *piece {
                Piece::Text(c) => out.push((c, style_of(&layers, &self.marks))),
                Piece::Mark(m) => match self.marks[m].role {
                    Role::Open => layers.push(m),
                    Role::Close => layers.retain(|&l| l != self.marks[m].opener),
                    Role::Literal | Role::Pending => {
                        let st = style_of(&layers, &self.marks);
                        out.extend(self.marks[m].raw.chars().map(|c| (c, st)));
                    }
                },
            }
        }
        if self.warnings.len() > MAX_WARNINGS {
            let more = self.warnings.len() - MAX_WARNINGS;
            self.warnings.truncate(MAX_WARNINGS);
            self.warnings.push(format!("… and {more} more"));
        }
        (collect(out), self.warnings)
    }
}

fn style_of(layers: &[usize], marks: &[Mark]) -> SpanStyle {
    let mut st = SpanStyle::default();
    for &l in layers {
        match marks[l].kind {
            Kind::Bold => st.bold = true,
            Kind::ItalicStar | Kind::ItalicUnder => st.italic = true,
            Kind::Underline => st.underline = true,
            Kind::Strike => st.strike = true,
            Kind::Tag(c, b) => {
                st.color = c.or(st.color);
                st.highlight = b.or(st.highlight);
            }
        }
    }
    st
}

/// Spans from styled characters (adjacent characters with the same style share a span).
fn collect(chars: Vec<(char, SpanStyle)>) -> RichText {
    let mut spans: Vec<Span> = Vec::new();
    for (c, style) in chars {
        match spans.last_mut() {
            Some(l) if l.style == style => l.text.push(c),
            _ => spans.push(Span { text: c.to_string(), style }),
        }
    }
    RichText(spans)
}

fn starts_with(chars: &[char], at: usize, s: &str) -> bool {
    s.chars().enumerate().all(|(k, c)| chars.get(at + k) == Some(&c))
}

/// `color=#e03131 bg=ink` → (color, bg).
fn parse_tag(body: &str) -> Result<(Option<Color>, Option<Color>), String> {
    let (mut color, mut bg) = (None, None);
    for part in body.split_whitespace() {
        let (key, value) = part.split_once('=').ok_or_else(|| format!("expected key=value, got \"{part}\""))?;
        let c = || Color::parse(value).ok_or_else(|| format!("invalid color \"{value}\" (expected #rrggbb or ink)"));
        match key {
            "color" => color = Some(c()?),
            "bg" => bg = Some(c()?),
            _ => return Err(format!("unknown attribute \"{key}\" (expected color= or bg=)")),
        }
    }
    Ok((color, bg))
}

/// Parses the Markdown subset. Never fails: what cannot be applied stays literal and is described in the returned warnings.
pub(super) fn from_markdown(src: &str) -> (RichText, Vec<String>) {
    let chars: Vec<char> = src.chars().collect();
    let mut p = Parser::default();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' => match chars.get(i + 1) {
                Some(&n) if SPECIAL.contains(n) => {
                    p.text(n);
                    i += 2;
                }
                _ => {
                    p.text('\\');
                    i += 1;
                }
            },
            '*' | '_' | '~' => {
                let run = chars[i..].iter().take_while(|&&x| x == c).count();
                let mut k = 0;
                while k < run {
                    let (at, double) = (i + k, run - k >= 2);
                    match (c, double) {
                        ('*', true) => p.toggle(Kind::Bold, "**", at),
                        ('*', false) => p.toggle(Kind::ItalicStar, "*", at),
                        ('_', true) => p.toggle(Kind::Underline, "__", at),
                        ('_', false) => p.underscore(&chars, at),
                        ('~', true) => p.toggle(Kind::Strike, "~~", at),
                        _ => p.text('~'),
                    }
                    k += if double { 2 } else { 1 };
                }
                i += run;
            }
            '{' if starts_with(&chars, i, "{/}") => {
                p.close_tag(i);
                i += 3;
            }
            '{' if starts_with(&chars, i, "{color=") || starts_with(&chars, i, "{bg=") => {
                let end = chars[i..].iter().position(|&x| x == '}').filter(|&e| !chars[i + 1..i + e].iter().any(|&x| x == '{' || x == '\n'));
                let Some(end) = end else {
                    p.warnings.push(format!("`{{` at character {i} starts a tag that is never closed with `}}`: kept as literal text"));
                    p.text('{');
                    i += 1;
                    continue;
                };
                let raw: String = chars[i..=i + end].iter().collect();
                let body: String = chars[i + 1..i + end].iter().collect();
                match parse_tag(&body) {
                    Ok((color, bg)) => p.open_tag(color, bg, &raw, i),
                    Err(e) => {
                        p.warnings.push(format!("{e} in `{raw}` at character {i}: kept as literal text"));
                        p.literal(&chars[i..=i + end]);
                    }
                }
                i += end + 1;
            }
            _ => {
                p.text(c);
                i += 1;
            }
        }
    }
    p.finish()
}

/* ------------------------------------------------------- RichText → Markdown */

/// True when some text carries a formatting.
pub(super) fn has_formatting(rt: &RichText) -> bool {
    rt.0.iter().any(|s| !s.text.is_empty() && s.style != SpanStyle::default())
}

fn tag_of(st: &SpanStyle) -> Option<String> {
    if st.color.is_none() && st.highlight.is_none() {
        return None;
    }
    let mut t = String::from("{");
    let mut sep = "";
    for (key, c) in [("color", st.color), ("bg", st.highlight)] {
        if let Some(c) = c {
            let _ = write!(t, "{sep}{key}={}", c.to_hex());
            sep = " ";
        }
    }
    t.push('}');
    Some(t)
}

/// Markers between two spans: what closes, then what opens.
fn transition(out: &mut String, from: &SpanStyle, to: &SpanStyle) {
    for (was, is, marker) in [(from.bold, to.bold, "**"), (from.italic, to.italic, "*"), (from.underline, to.underline, "__"), (from.strike, to.strike, "~~")] {
        if was && !is {
            out.push_str(marker);
        }
    }
    if (from.color, from.highlight) != (to.color, to.highlight) {
        if tag_of(from).is_some() {
            out.push_str("{/}");
        }
        if let Some(t) = tag_of(to) {
            out.push_str(&t);
        }
    }
    for (was, is, marker) in [(from.bold, to.bold, "**"), (from.italic, to.italic, "*"), (from.underline, to.underline, "__"), (from.strike, to.strike, "~~")] {
        if !was && is {
            out.push_str(marker);
        }
    }
}

/// Markdown of a rich text, in the syntax `from_markdown` reads (every special character is escaped).
pub(super) fn to_markdown(rt: &RichText) -> String {
    let mut out = String::new();
    let mut cur = SpanStyle::default();
    for sp in rt.0.iter().filter(|s| !s.text.is_empty()) {
        transition(&mut out, &cur, &sp.style);
        for c in sp.text.chars() {
            if SPECIAL.contains(c) {
                out.push('\\');
            }
            out.push(c);
        }
        cur = sp.style;
    }
    transition(&mut out, &cur, &SpanStyle::default());
    out
}

/* ------------------------------------------------------- helpers of the tools */

/// The `markdown` parameter (default false).
pub(super) fn markdown_flag(a: &Value) -> Res<bool> {
    match a.get("markdown") {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err("\"markdown\" must be true or false".into()),
    }
}

/// Schema of the `markdown` parameter; `what` names the text it applies to.
pub(super) fn markdown_prop(what: &str) -> Value {
    json!({"type": "boolean", "description": format!("Default false (text taken literally). true: {what} is Markdown: **bold**, *italic* (or _italic_), __underline__, ~~strike~~, \
        {{color=#e03131}}text{{/}}, {{bg=#ffec99}}text{{/}}, {{color=#e03131 bg=ink}}text{{/}} (colors: #rrggbb or ink); nesting allowed; \\ escapes * _ ~ {{ }} \\. \
        Markup that cannot be applied stays literal text and is listed in the `warnings` of the result.")})
}

/// Text of an object: plain as it always was, or parsed as Markdown (warnings are appended).
pub(super) fn rich(text: &str, markdown: bool, warnings: &mut Vec<String>) -> RichText {
    if !markdown {
        return RichText::plain(text);
    }
    let (rt, w) = from_markdown(text);
    warnings.extend(w);
    rt
}

/// Adds the `warnings` field to a tool result when there is something to report.
pub(super) fn add_warnings(out: &mut Value, mut warnings: Vec<String>) {
    if warnings.len() > MAX_WARNINGS {
        let more = warnings.len() - MAX_WARNINGS;
        warnings.truncate(MAX_WARNINGS);
        warnings.push(format!("… and {more} more"));
    }
    if !warnings.is_empty() {
        out["warnings"] = json!(warnings);
    }
}

/* --------------------------------------------------------------- format_text */

/// Schemas for `tools/list`.
pub(super) fn specs() -> Vec<Value> {
    let color = |what: &str| json!({"description": format!("{what}: \"#rrggbb\" or \"ink\" sets it, null removes it."), "oneOf": [{"type": "string"}, {"type": "null"}]});
    let flag = |what: &str| json!({"type": "boolean", "description": format!("true applies {what}, false removes it.")});
    vec![json!({"name": "format_text",
        "description": "Formats text already on the board, like selecting it in the application: bold, italic, underline, strike, color, highlight. Works on a shape's text, a connector's label, or one table cell (`cell`). \
            Choose the part with `match` (first occurrence; every one with `all: true`) or `range` [start, end) (character indices of the plain text, end excluded); with neither, the whole text. \
            `clear: true` removes all formatting of the part first (then the other options apply). Returns `changed`: the number of characters whose formatting changed. Error if `match` is not found.",
        "inputSchema": obj(json!({
            "path": path_prop(), "page": page_prop(),
            "id": {"type": "string", "description": "Object id (add_shape, describe_board…)."},
            "match": {"type": "string", "description": "Substring to format (case-sensitive)."},
            "all": {"type": "boolean", "description": "With `match`: format every occurrence, not only the first."},
            "range": {"type": "array", "items": {"type": "integer", "minimum": 0}, "minItems": 2, "maxItems": 2, "description": "[start, end): character indices, start included, end excluded."},
            "cell": {"type": "array", "items": {"type": "integer", "minimum": 0}, "minItems": 2, "maxItems": 2, "description": "[row, column] (from 0) of the table cell to format; required for a table, invalid otherwise."},
            "bold": flag("bold"), "italic": flag("italic"), "underline": flag("underline"), "strike": flag("strikethrough"),
            "color": color("Text color"), "highlight": color("Highlight color behind the text"),
            "clear": {"type": "boolean", "description": "true removes every formatting of the part."},
        }), &["id"])})]
}

/// Runs the tool when this module owns it, `None` otherwise.
pub(super) fn apply(ed: &mut Editor, name: &str, a: &Value) -> Option<Res<Value>> {
    (name == "format_text").then(|| format_text(ed, a))
}

/// What to do to the characters of the part (`None` = leave as is).
struct Edit {
    clear: bool,
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    color: ColorEdit,
    highlight: ColorEdit,
}

/// Change of an optional color: absent from the request, `null`, or a color.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ColorEdit {
    Keep,
    Remove,
    Set(Color),
}

impl ColorEdit {
    fn apply(self, current: Option<Color>) -> Option<Color> {
        match self {
            ColorEdit::Keep => current,
            ColorEdit::Remove => None,
            ColorEdit::Set(c) => Some(c),
        }
    }
}

impl Edit {
    fn parse(a: &Value) -> Res<Self> {
        let flag = |k: &str| match a.get(k) {
            None => Ok(None),
            Some(Value::Bool(b)) => Ok(Some(*b)),
            Some(_) => Err(format!("\"{k}\" must be true or false")),
        };
        let color = |k: &str| match a.get(k) {
            None => Ok(ColorEdit::Keep),
            Some(Value::Null) => Ok(ColorEdit::Remove),
            Some(Value::String(c)) => {
                Color::parse(c).map(ColorEdit::Set).ok_or_else(|| format!("invalid color \"{c}\" for \"{k}\" (expected #rrggbb or ink, or null to remove)"))
            }
            Some(_) => Err(format!("\"{k}\" must be a color string or null")),
        };
        let e = Edit {
            clear: flag("clear")?.unwrap_or(false),
            bold: flag("bold")?,
            italic: flag("italic")?,
            underline: flag("underline")?,
            strike: flag("strike")?,
            color: color("color")?,
            highlight: color("highlight")?,
        };
        let nothing = e.bold.is_none()
            && e.italic.is_none()
            && e.underline.is_none()
            && e.strike.is_none()
            && e.color == ColorEdit::Keep
            && e.highlight == ColorEdit::Keep;
        if nothing && !e.clear {
            return Err("nothing to do: give at least one of bold, italic, underline, strike, color, highlight or clear".into());
        }
        Ok(e)
    }

    fn apply(&self, st: &mut SpanStyle) {
        if self.clear {
            *st = SpanStyle::default();
        }
        st.bold = self.bold.unwrap_or(st.bold);
        st.italic = self.italic.unwrap_or(st.italic);
        st.underline = self.underline.unwrap_or(st.underline);
        st.strike = self.strike.unwrap_or(st.strike);
        st.color = self.color.apply(st.color);
        st.highlight = self.highlight.apply(st.highlight);
    }
}

/// A pair of non-negative integers (`range`, `cell`).
fn pair(a: &Value, k: &str) -> Res<Option<(usize, usize)>> {
    let Some(value) = a.get(k).filter(|v| !v.is_null()) else { return Ok(None) };
    let get = |i: usize| value.get(i).and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok());
    match (value.as_array().map(Vec::len), get(0), get(1)) {
        (Some(2), Some(x), Some(y)) => Ok(Some((x, y))),
        _ => Err(format!("\"{k}\" must be a list of two non-negative integers, e.g. [0, 3]")),
    }
}

/// First characters of a text, for error messages.
fn excerpt(text: &str) -> String {
    let head: String = text.chars().take(80).collect();
    if head.len() < text.len() { format!("{head}…") } else { head }
}

/// The char ranges `[from, to)` of `plain` selected by `match` / `all` / `range` (the whole text when none is given).
fn selection(a: &Value, plain: &str) -> Res<Vec<(usize, usize)>> {
    let len = plain.chars().count();
    let (m, range) = (a.get("match").filter(|v| !v.is_null()), pair(a, "range")?);
    let all = match a.get("all") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err("\"all\" must be true or false".into()),
    };
    match (m, range) {
        (Some(_), Some(_)) => Err("give either \"match\" or \"range\", not both".into()),
        (None, _) if all => Err("\"all\" only applies together with \"match\"".into()),
        (Some(m), None) => {
            let m = m.as_str().ok_or("\"match\" must be a string")?;
            if m.is_empty() {
                return Err("\"match\" is empty".into());
            }
            let n = m.chars().count();
            let mut found: Vec<(usize, usize)> = plain.match_indices(m).map(|(b, _)| plain[..b].chars().count()).map(|f| (f, f + n)).collect();
            if found.is_empty() {
                return Err(format!("text not found: \"{m}\" (the text is \"{}\")", excerpt(plain)));
            }
            if !all {
                found.truncate(1);
            }
            Ok(found)
        }
        (None, Some((from, to))) => {
            if from >= to || to > len {
                return Err(format!("invalid range [{from}, {to}]: the text has {len} characters (start included, end excluded, start < end)"));
            }
            Ok(vec![(from, to)])
        }
        (None, None) if len == 0 => Err("the text is empty: nothing to format".into()),
        (None, None) => Ok(vec![(0, len)]),
    }
}

/// The text `format_text` acts on: a shape's text, a connector's label or a table cell.
fn target(o: &mut Object, cell: Option<(usize, usize)>) -> Res<&mut RichText> {
    let kind = super::kind_name(o);
    match (&mut o.kind, cell) {
        (ObjKind::Table(t), Some((r, c))) => {
            let (rows, cols) = (t.rows(), t.cols());
            t.cell_mut(r, c).ok_or_else(|| format!("cell [{r}, {c}] is outside the table ({rows} rows, {cols} columns; indices start at 0)"))
        }
        (ObjKind::Table(_), None) => Err("a table needs \"cell\": [row, column] (indices start at 0)".into()),
        (_, Some(_)) => Err("\"cell\" only applies to tables".into()),
        (ObjKind::Box(b), None) => Ok(&mut b.label.text),
        (ObjKind::Connector(c), None) => {
            c.label.as_mut().map(|l| &mut l.text).ok_or_else(|| "this connector has no label (add one with update_object)".to_owned())
        }
        (_, None) => Err(format!("format_text works on shapes, connector labels and table cells (this object is a {kind})")),
    }
}

fn styles(rt: &RichText) -> Vec<SpanStyle> {
    rt.0.iter().flat_map(|s| s.text.chars().map(move |_| s.style)).collect()
}

fn format_text(ed: &mut Editor, a: &Value) -> Res<Value> {
    let id = find(ed, s(a, "id")?)?.id.clone();
    let edit = Edit::parse(a)?;
    let cell = pair(a, "cell")?;
    // everything is checked on a copy first: an error leaves the board untouched
    let mut probe = find(ed, &id.0)?.clone();
    let plain = target(&mut probe, cell)?.to_plain();
    let ranges = selection(a, &plain)?;
    let mut changed = 0;
    ed.update(&id, |o| {
        if let Ok(rt) = target(o, cell) {
            let before = styles(rt);
            for &(from, to) in &ranges {
                rt.restyle(from, to, |st| edit.apply(st));
            }
            changed = before.iter().zip(styles(rt)).filter(|(b, n)| *b != n).count();
        }
        if let ObjKind::Box(b) = &mut o.kind {
            b.fit(&ApproxMeasurer);
        }
    });
    Ok(json!({"id": id.0, "changed": changed}))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Color = Color::Rgb(0xe0, 0x31, 0x31);
    const YELLOW: Color = Color::Rgb(0xff, 0xec, 0x99);

    /// Deterministic xorshift generator (no dependency).
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: usize) -> usize {
            usize::try_from(self.next() % n as u64).expect("below n")
        }
        fn string(&mut self, alphabet: &[&str], max: usize) -> String {
            (0..self.below(max + 1)).map(|_| alphabet[self.below(alphabet.len())]).collect()
        }
    }

    fn parsed(src: &str) -> (Vec<(String, SpanStyle)>, Vec<String>) {
        let (rt, w) = from_markdown(src);
        (rt.0.into_iter().map(|s| (s.text, s.style)).collect(), w)
    }

    fn bold() -> SpanStyle {
        SpanStyle { bold: true, ..SpanStyle::default() }
    }

    #[test]
    fn plain_text_is_one_default_span() {
        let (rt, w) = from_markdown("hello\nworld");
        assert_eq!(rt, RichText::plain("hello\nworld"));
        assert_eq!(w, [] as [std::string::String; 0]);
        assert_eq!(from_markdown("").0, RichText::default());
    }

    #[test]
    fn every_marker_gives_its_style() {
        let (spans, w) = parsed("a **b** *c* _d_ __e__ ~~f~~ {color=#e03131}g{/} {bg=#ffec99}h{/} {color=ink bg=#ffec99}i{/}");
        assert!(w.is_empty(), "{w:?}");
        let by = |t: &str| spans.iter().find(|s| s.0 == t).unwrap_or_else(|| panic!("no span {t:?} in {spans:?}")).1;
        assert!(by("b").bold && by("c").italic && by("d").italic && by("e").underline && by("f").strike);
        assert_eq!(by("g").color, Some(RED));
        assert_eq!(by("h").highlight, Some(YELLOW));
        assert_eq!((by("i").color, by("i").highlight), (Some(Color::Ink), Some(YELLOW)));
    }

    #[test]
    fn nesting_and_overlap() {
        let (spans, w) = parsed("***x*** **a *b** c*");
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(spans[0].1, SpanStyle { bold: true, italic: true, ..SpanStyle::default() });
        let b = spans.iter().find(|s| s.0 == "b").unwrap().1;
        assert!(b.bold && b.italic);
        let (rt, _) = from_markdown("{color=#e03131}a **b** {bg=#ffec99}c{/} d{/}");
        assert_eq!(rt.style_at(2).unwrap(), SpanStyle { bold: true, color: Some(RED), ..SpanStyle::default() });
        assert_eq!(rt.style_at(4).unwrap(), SpanStyle { color: Some(RED), highlight: Some(YELLOW), ..SpanStyle::default() });
        assert_eq!(rt.style_at(6).unwrap(), SpanStyle { color: Some(RED), ..SpanStyle::default() });
    }

    #[test]
    fn escapes_give_literal_characters() {
        let (spans, w) = parsed(r"\*not italic\* \_ \~ \{ \} \\ \n end\");
        assert_eq!(w, [] as [std::string::String; 0]);
        assert_eq!(spans, vec![(r"*not italic* _ ~ { } \ \n end\".to_owned(), SpanStyle::default())]);
    }

    #[test]
    fn invalid_markup_stays_literal_with_a_warning() {
        for src in [
            "**open",
            "a *b",
            "{color=#e03131}open",
            "stray {/} here",
            "{color=red}x{/}",
            "{color=#e03131",
            "{size=3}x{/}",
            "**{/}**",
            "{bg=#ffec99}{/}",
            "****",
        ] {
            let (rt, w) = from_markdown(src);
            assert!(!w.is_empty(), "{src}: a warning is expected");
            assert_eq!(rt.to_plain(), src, "{src}");
            assert!(!has_formatting(&rt), "{src}");
        }
    }

    #[test]
    fn a_marker_that_is_not_markup_is_silent() {
        for src in ["snake_case_name", "a ~ b", "_ alone", "alone _", "~5 min"] {
            let (rt, w) = from_markdown(src);
            assert_eq!((rt.to_plain().as_str(), w.is_empty()), (src, true), "{src}");
            assert!(!has_formatting(&rt));
        }
    }

    #[test]
    fn warnings_are_capped() {
        let src = "{color=#e03131}a ".repeat(100);
        let (rt, w) = from_markdown(&src);
        assert_eq!(rt.to_plain(), src);
        assert_eq!(w.len(), MAX_WARNINGS + 1);
        assert!(w.last().unwrap().starts_with('…'));
    }

    #[test]
    fn to_markdown_examples() {
        let mut rt = RichText::plain("a bc d");
        rt.restyle(2, 4, |s| s.bold = true);
        assert_eq!(to_markdown(&rt), "a **bc** d");
        rt.restyle(2, 3, |s| s.color = Some(RED));
        assert_eq!(to_markdown(&rt), "a {color=#e03131}**b{/}c** d");
        assert_eq!(to_markdown(&RichText::plain("1 * 2_x")), r"1 \* 2\_x");
        assert_eq!(parsed("**a**").0, vec![("a".to_owned(), bold())]);
    }

    fn random_rich(rng: &mut Rng) -> RichText {
        let alphabet = ["a", "b", " ", "Z", "_", "*", "~", "{", "}", "\\", "\n", "/", "=", "#", "é", "🙂", "0", "f"];
        let mut rt = RichText::plain(rng.string(&alphabet, 24));
        let n = rt.len_chars();
        let palette = [Color::Ink, RED, YELLOW, Color::Rgb(0, 0, 0)];
        for _ in 0..rng.below(5) {
            let (a, b) = (rng.below(n + 1), rng.below(n + 1));
            let (flag, color) = (rng.below(2) == 0, palette[rng.below(palette.len())]);
            let which = rng.below(8);
            rt.restyle(a.min(b), a.max(b), |s| match which {
                0 => s.bold = flag,
                1 => s.italic = flag,
                2 => s.underline = flag,
                3 => s.strike = flag,
                4 => s.color = Some(color),
                5 => s.highlight = Some(color),
                6 => s.color = None,
                _ => s.highlight = None,
            });
        }
        rt
    }

    #[test]
    fn markdown_roundtrip_on_random_rich_texts() {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        let mut formatted = 0;
        for case in 0..3000 {
            let rt = random_rich(&mut rng);
            let md = to_markdown(&rt);
            let (back, w) = from_markdown(&md);
            assert_eq!(back, rt, "case {case}: {md:?}");
            assert!(w.is_empty(), "case {case}: {md:?}: {w:?}");
            formatted += usize::from(has_formatting(&rt));
        }
        assert!(formatted > 1000, "the generator must produce formatted texts ({formatted})");
    }

    #[test]
    fn parsing_arbitrary_input_never_loses_text_and_is_stable() {
        let mut rng = Rng(0xDEAD_BEEF_CAFE_F00D);
        let alphabet = [
            "a",
            "b",
            " ",
            "\n",
            "\\",
            "{",
            "}",
            "*",
            "**",
            "***",
            "_",
            "__",
            "~",
            "~~",
            "{/}",
            "{color=#e03131}",
            "{bg=ink}",
            "{color=ink bg=#ffec99}",
            "{color=bad}",
        ];
        for case in 0..3000 {
            let src = rng.string(&alphabet, 30);
            let (rt, _) = from_markdown(&src);
            // markers either apply or stay: the text can only get shorter by the markers that applied
            assert!(rt.len_chars() <= src.chars().count(), "case {case}: {src:?}");
            // what the parser produced survives a trip through Markdown
            let md = to_markdown(&rt);
            let (again, w) = from_markdown(&md);
            assert_eq!(again, rt, "case {case}: {src:?} -> {md:?}");
            assert!(w.is_empty(), "case {case}: {md:?}: {w:?}");
        }
    }
}
