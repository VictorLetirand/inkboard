//! Customizable keyboard shortcuts: actions, key combinations, settings file.

use crate::tools::Tool;
use eframe::egui::{Key, Modifiers};
use inkboard_core::BoxGeom;
use std::collections::BTreeMap;

/// Application command that can be bound to a shortcut.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Action {
    New,
    Open,
    Save,
    SaveAs,
    Undo,
    Redo,
    SelectAll,
    Duplicate,
    Delete,
    Group,
    Ungroup,
    Front,
    Forward,
    Backward,
    Back,
    Zoom100,
    ZoomFit,
    ZoomIn,
    ZoomOut,
    EditText,
    Bold,
    Italic,
    Underline,
    Strike,
    ToolLock,
    ToolSelect,
    ToolHand,
    ToolPen,
    ToolHighlighter,
    ToolEraser,
    ToolRect,
    ToolEllipse,
    ToolDiamond,
    ToolTriangle,
    ToolLine,
    ToolArrow,
    ToolText,
    ToolSticky,
    ToolTable,
    ToolLaser,
    Present,
}

impl Action {
    pub const ALL: [Action; 41] = [
        Action::New,
        Action::Open,
        Action::Save,
        Action::SaveAs,
        Action::Undo,
        Action::Redo,
        Action::SelectAll,
        Action::Duplicate,
        Action::Delete,
        Action::Group,
        Action::Ungroup,
        Action::Front,
        Action::Forward,
        Action::Backward,
        Action::Back,
        Action::Zoom100,
        Action::ZoomFit,
        Action::ZoomIn,
        Action::ZoomOut,
        Action::EditText,
        Action::Bold,
        Action::Italic,
        Action::Underline,
        Action::Strike,
        Action::ToolLock,
        Action::ToolSelect,
        Action::ToolHand,
        Action::ToolPen,
        Action::ToolHighlighter,
        Action::ToolEraser,
        Action::ToolRect,
        Action::ToolEllipse,
        Action::ToolDiamond,
        Action::ToolTriangle,
        Action::ToolLine,
        Action::ToolArrow,
        Action::ToolText,
        Action::ToolSticky,
        Action::ToolTable,
        Action::ToolLaser,
        Action::Present,
    ];

    /// Stable identifier used in the settings file.
    pub fn id(self) -> &'static str {
        match self {
            Action::New => "file.new",
            Action::Open => "file.open",
            Action::Save => "file.save",
            Action::SaveAs => "file.save_as",
            Action::Undo => "edit.undo",
            Action::Redo => "edit.redo",
            Action::SelectAll => "edit.select_all",
            Action::Duplicate => "edit.duplicate",
            Action::Delete => "edit.delete",
            Action::Group => "edit.group",
            Action::Ungroup => "edit.ungroup",
            Action::Front => "order.front",
            Action::Forward => "order.forward",
            Action::Backward => "order.backward",
            Action::Back => "order.back",
            Action::Zoom100 => "view.zoom_100",
            Action::ZoomFit => "view.zoom_fit",
            Action::ZoomIn => "view.zoom_in",
            Action::ZoomOut => "view.zoom_out",
            Action::EditText => "edit.text",
            Action::Bold => "text.bold",
            Action::Italic => "text.italic",
            Action::Underline => "text.underline",
            Action::Strike => "text.strike",
            Action::ToolLock => "tool.lock",
            Action::ToolSelect => "tool.select",
            Action::ToolHand => "tool.hand",
            Action::ToolPen => "tool.pen",
            Action::ToolHighlighter => "tool.highlighter",
            Action::ToolEraser => "tool.eraser",
            Action::ToolRect => "tool.rect",
            Action::ToolEllipse => "tool.ellipse",
            Action::ToolDiamond => "tool.diamond",
            Action::ToolTriangle => "tool.triangle",
            Action::ToolLine => "tool.line",
            Action::ToolArrow => "tool.arrow",
            Action::ToolText => "tool.text",
            Action::ToolSticky => "tool.sticky",
            Action::ToolTable => "tool.table",
            Action::ToolLaser => "tool.laser",
            Action::Present => "view.present",
        }
    }

    pub fn from_id(id: &str) -> Option<Action> {
        Action::ALL.into_iter().find(|a| a.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Action::New => "New board",
            Action::Open => "Open / import",
            Action::Save => "Save",
            Action::SaveAs => "Save as",
            Action::Undo => "Undo",
            Action::Redo => "Redo",
            Action::SelectAll => "Select all",
            Action::Duplicate => "Duplicate",
            Action::Delete => "Delete",
            Action::Group => "Group",
            Action::Ungroup => "Ungroup",
            Action::Front => "Bring to front",
            Action::Forward => "Bring forward",
            Action::Backward => "Send backward",
            Action::Back => "Send to back",
            Action::Zoom100 => "Zoom 100 %",
            Action::ZoomFit => "Fit all",
            Action::ZoomIn => "Zoom in",
            Action::ZoomOut => "Zoom out",
            Action::EditText => "Edit text",
            Action::Bold => "Bold",
            Action::Italic => "Italic",
            Action::Underline => "Underline",
            Action::Strike => "Strikethrough",
            Action::ToolLock => "Keep the tool active",
            Action::ToolSelect => "Tool: select",
            Action::ToolHand => "Tool: hand",
            Action::ToolPen => "Tool: pen",
            Action::ToolHighlighter => "Tool: highlighter",
            Action::ToolEraser => "Tool: eraser",
            Action::ToolRect => "Tool: rectangle",
            Action::ToolEllipse => "Tool: ellipse",
            Action::ToolDiamond => "Tool: diamond",
            Action::ToolTriangle => "Tool: triangle",
            Action::ToolLine => "Tool: line",
            Action::ToolArrow => "Tool: arrow",
            Action::ToolText => "Tool: text",
            Action::ToolSticky => "Tool: sticky note",
            Action::ToolTable => "Tool: table",
            Action::ToolLaser => "Tool: laser pointer",
            Action::Present => "Present full screen",
        }
    }

    pub fn group(self) -> &'static str {
        match self {
            Action::New | Action::Open | Action::Save | Action::SaveAs => "File",
            Action::Undo | Action::Redo | Action::SelectAll | Action::Duplicate | Action::Delete | Action::Group | Action::Ungroup | Action::EditText => "Edit",
            Action::Front | Action::Forward | Action::Backward | Action::Back => "Order",
            Action::Zoom100 | Action::ZoomFit | Action::ZoomIn | Action::ZoomOut | Action::Present => "View",
            Action::Bold | Action::Italic | Action::Underline | Action::Strike => "Text",
            _ => "Tools",
        }
    }

    pub fn tool(self) -> Option<Tool> {
        Some(match self {
            Action::ToolSelect => Tool::Select,
            Action::ToolHand => Tool::Hand,
            Action::ToolPen => Tool::Pen,
            Action::ToolHighlighter => Tool::Highlighter,
            Action::ToolEraser => Tool::Eraser,
            Action::ToolRect => Tool::Shape(BoxGeom::Rect),
            Action::ToolEllipse => Tool::Shape(BoxGeom::Ellipse),
            Action::ToolDiamond => Tool::Shape(BoxGeom::Diamond),
            Action::ToolTriangle => Tool::Shape(BoxGeom::Triangle),
            Action::ToolLine => Tool::Line,
            Action::ToolArrow => Tool::Arrow,
            Action::ToolText => Tool::Text,
            Action::ToolSticky => Tool::Sticky,
            Action::ToolTable => Tool::Table,
            Action::ToolLaser => Tool::Laser,
            _ => return None,
        })
    }

    pub fn for_tool(t: Tool) -> Option<Action> {
        Action::ALL.into_iter().find(|a| a.tool() == Some(t))
    }
}

/// Combinaison de touches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortcut {
    pub key: Key,
    /// Ctrl (Cmd on Mac).
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Shortcut {
    pub const fn new(key: Key, ctrl: bool, shift: bool, alt: bool) -> Self {
        Self { key, ctrl, shift, alt }
    }
    pub const fn key(key: Key) -> Self {
        Self::new(key, false, false, false)
    }
    pub const fn ctrl(key: Key) -> Self {
        Self::new(key, true, false, false)
    }
    pub const fn ctrl_shift(key: Key) -> Self {
        Self::new(key, true, true, false)
    }

    pub fn modifiers(&self) -> Modifiers {
        Modifiers { alt: self.alt, shift: self.shift, ..if self.ctrl { Modifiers::COMMAND } else { Modifiers::NONE } }
    }

    /// Exact match: the modifiers must be identical.
    pub fn matches(&self, key: Key, m: Modifiers) -> bool {
        self.key == key && self.ctrl == m.command && self.shift == m.shift && self.alt == m.alt
    }

    /// Form stored in the settings file: "Ctrl+Shift+G".
    pub fn to_id(self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push_str("Ctrl+");
        }
        if self.shift {
            s.push_str("Shift+");
        }
        if self.alt {
            s.push_str("Alt+");
        }
        s.push_str(self.key.name());
        s
    }

    pub fn parse(s: &str) -> Option<Shortcut> {
        let mut sc = Shortcut::new(Key::A, false, false, false);
        let mut key = None;
        for part in s.split('+').map(str::trim).filter(|p| !p.is_empty()) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "cmd" | "command" | "ctl" => sc.ctrl = true,
                "shift" | "maj" => sc.shift = true,
                "alt" => sc.alt = true,
                _ => key = Some(Key::from_name(part)?),
            }
        }
        sc.key = key?;
        Some(sc)
    }

    /// Text shown to the user (translated).
    pub fn label(&self) -> String {
        let key = match self.key {
            Key::Delete => crate::i18n::t("Delete"),
            Key::Backspace => crate::i18n::t("Backspace"),
            Key::Enter => crate::i18n::t("Enter"),
            Key::Escape => crate::i18n::t("Esc"),
            Key::Space => crate::i18n::t("Space"),
            Key::ArrowLeft => "←".to_owned(),
            Key::ArrowRight => "→".to_owned(),
            Key::ArrowUp => "↑".to_owned(),
            Key::ArrowDown => "↓".to_owned(),
            k => k.symbol_or_name().to_owned(),
        };
        let mut s = String::new();
        if self.ctrl {
            s.push_str("Ctrl+");
        }
        if self.shift {
            s.push_str(&crate::i18n::t("Shift+"));
        }
        if self.alt {
            s.push_str("Alt+");
        }
        s + &key
    }
}

/// Keys that cannot be reassigned (reserved by the application or the system).
pub fn is_reserved(sc: &Shortcut) -> bool {
    matches!(sc.key, Key::Escape | Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown)
        || (sc.ctrl && !sc.shift && !sc.alt && matches!(sc.key, Key::C | Key::X | Key::V))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Keymap {
    map: BTreeMap<Action, Vec<Shortcut>>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self { map: Action::ALL.into_iter().map(|a| (a, Keymap::default_for(a))).collect() }
    }
}

impl Keymap {
    /// Default shortcuts for an action.
    pub fn default_for(a: Action) -> Vec<Shortcut> {
        use Shortcut as S;
        match a {
            Action::New => vec![S::ctrl(Key::N)],
            Action::Open => vec![S::ctrl(Key::O)],
            Action::Save => vec![S::ctrl(Key::S)],
            Action::SaveAs => vec![S::ctrl_shift(Key::S)],
            Action::Undo => vec![S::ctrl(Key::Z)],
            Action::Redo => vec![S::ctrl(Key::Y), S::ctrl_shift(Key::Z)],
            Action::SelectAll => vec![S::ctrl(Key::A)],
            Action::Duplicate => vec![S::ctrl(Key::D)],
            Action::Delete => vec![S::key(Key::Delete), S::key(Key::Backspace)],
            Action::Group => vec![S::ctrl(Key::G)],
            Action::Ungroup => vec![S::ctrl_shift(Key::G)],
            Action::Front => vec![S::ctrl_shift(Key::CloseBracket)],
            Action::Forward => vec![S::ctrl(Key::CloseBracket)],
            Action::Backward => vec![S::ctrl(Key::OpenBracket)],
            Action::Back => vec![S::ctrl_shift(Key::OpenBracket)],
            Action::Zoom100 => vec![S::ctrl(Key::Num0)],
            Action::ZoomFit => vec![S::ctrl(Key::Num1)],
            Action::ZoomIn => vec![S::ctrl(Key::Equals), S::ctrl(Key::Plus)],
            Action::ZoomOut => vec![S::ctrl(Key::Minus)],
            Action::EditText => vec![S::key(Key::Enter)],
            Action::Bold => vec![S::ctrl(Key::B)],
            Action::Italic => vec![S::ctrl(Key::I)],
            Action::Underline => vec![S::ctrl(Key::U)],
            Action::Strike => vec![S::ctrl_shift(Key::X)],
            Action::ToolLock => vec![S::key(Key::Q)],
            Action::ToolSelect => vec![S::key(Key::V)],
            Action::ToolHand => vec![S::key(Key::H)],
            Action::ToolPen => vec![S::key(Key::P)],
            Action::ToolHighlighter => vec![S::key(Key::M)],
            Action::ToolEraser => vec![S::key(Key::E)],
            Action::ToolRect => vec![S::key(Key::R)],
            Action::ToolEllipse => vec![S::key(Key::O)],
            Action::ToolDiamond => vec![S::key(Key::D)],
            Action::ToolTriangle => vec![S::key(Key::Y)],
            Action::ToolLine => vec![S::key(Key::L)],
            Action::ToolArrow => vec![S::key(Key::A)],
            Action::ToolText => vec![S::key(Key::T)],
            Action::ToolSticky => vec![S::key(Key::N)],
            Action::ToolTable => vec![S::key(Key::B)],
            Action::ToolLaser => vec![S::key(Key::F)],
            Action::Present => vec![S::key(Key::F5)],
        }
    }

    pub fn bindings(&self, a: Action) -> &[Shortcut] {
        self.map.get(&a).map_or(&[], Vec::as_slice)
    }

    /// Action bound to this combination, if any.
    pub fn action_for(&self, key: Key, m: Modifiers) -> Option<Action> {
        self.map.iter().find(|(_, v)| v.iter().any(|s| s.matches(key, m))).map(|(a, _)| *a)
    }

    /// First shortcut of an action, ready to display (empty when there is none).
    pub fn label(&self, a: Action) -> String {
        self.bindings(a).first().map(Shortcut::label).unwrap_or_default()
    }

    /// Binds a shortcut to an action. If it was already used by another action it is removed from it,
    /// and that other action is returned. Reserved combinations are refused.
    pub fn add(&mut self, a: Action, sc: Shortcut) -> Result<Option<Action>, String> {
        if is_reserved(&sc) {
            return Err(crate::i18n::tr!("\"{}\" is reserved (escape, arrows, copy/cut/paste)", sc.label()));
        }
        let mut stolen = None;
        for (other, list) in &mut self.map {
            if *other != a && list.contains(&sc) {
                list.retain(|s| *s != sc);
                stolen = Some(*other);
            }
        }
        let list = self.map.entry(a).or_default();
        if !list.contains(&sc) {
            list.push(sc);
        }
        Ok(stolen)
    }

    pub fn remove(&mut self, a: Action, sc: &Shortcut) {
        if let Some(l) = self.map.get_mut(&a) {
            l.retain(|s| s != sc);
        }
    }

    pub fn reset(&mut self, a: Action) {
        // default shortcuts must not remain duplicated elsewhere
        for sc in Keymap::default_for(a) {
            for (other, list) in &mut self.map {
                if *other != a {
                    list.retain(|s| *s != sc);
                }
            }
        }
        self.map.insert(a, Keymap::default_for(a));
    }

    pub fn reset_all(&mut self) {
        *self = Keymap::default();
    }

    pub fn is_default(&self, a: Action) -> bool {
        self.bindings(a) == Keymap::default_for(a).as_slice()
    }

    /// Actions that share a shortcut (should not happen, except with a hand-edited file).
    pub fn conflicts(&self) -> Vec<(Shortcut, Action, Action)> {
        let mut out = Vec::new();
        let all: Vec<(Action, Shortcut)> = self.map.iter().flat_map(|(a, v)| v.iter().map(move |s| (*a, *s))).collect();
        for (i, (a, s)) in all.iter().enumerate() {
            for (b, t) in &all[i + 1..] {
                if a != b && s == t {
                    out.push((*s, *a, *b));
                }
            }
        }
        out
    }

    pub fn to_json(&self) -> String {
        let bindings: serde_json::Map<String, serde_json::Value> = self
            .map
            .iter()
            .map(|(a, v)| (a.id().to_owned(), serde_json::Value::Array(v.iter().map(|s| serde_json::Value::String(s.to_id())).collect())))
            .collect();
        serde_json::to_string_pretty(&serde_json::json!({ "version": 1, "bindings": bindings })).expect("valid JSON")
    }

    /// Text of a settings file: UTF-16 (LE/BE, with BOM, e.g. a PowerShell 5 redirection) or UTF-8.
    pub fn decode_text(bytes: &[u8]) -> Result<String, String> {
        let utf16 = |b: &[u8], from: fn([u8; 2]) -> u16| {
            if !b.len().is_multiple_of(2) {
                return Err("truncated UTF-16 file".to_owned());
            }
            String::from_utf16(&b.chunks_exact(2).map(|c| from([c[0], c[1]])).collect::<Vec<_>>()).map_err(|_| "invalid UTF-16 file".to_owned())
        };
        match bytes {
            [0xFF, 0xFE, rest @ ..] => utf16(rest, u16::from_le_bytes),
            [0xFE, 0xFF, rest @ ..] => utf16(rest, u16::from_be_bytes),
            _ => String::from_utf8(bytes.to_vec()).map_err(|_| "file is neither UTF-8 nor UTF-16".to_owned()),
        }
    }

    /// Reads a settings file. Missing actions keep their default shortcut; unknown or unreadable
    /// entries are ignored. Also returns the number of ignored entries.
    pub fn from_json(s: &str) -> Result<(Keymap, usize), String> {
        let v: serde_json::Value = serde_json::from_str(s).map_err(|e| format!("invalid JSON: {e}"))?;
        let b = v.get("bindings").and_then(|b| b.as_object()).ok_or("settings without \"bindings\"")?;
        let mut km = Keymap::default();
        let mut ignored = 0;
        for (id, list) in b {
            let Some(a) = Action::from_id(id) else {
                ignored += 1;
                continue;
            };
            let Some(list) = list.as_array() else {
                ignored += 1; // not a list (e.g. "R" instead of ["R"]): the default shortcut stays
                continue;
            };
            let mut scs = Vec::new();
            for item in list {
                match item.as_str().and_then(Shortcut::parse) {
                    Some(sc) if !is_reserved(&sc) => scs.push(sc),
                    _ => ignored += 1,
                }
            }
            km.map.insert(a, scs);
        }
        Ok((km, ignored))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_cover_every_action_without_conflicts() {
        let km = Keymap::default();
        for a in Action::ALL {
            assert!(!km.bindings(a).is_empty(), "{a:?} has no default shortcut");
        }
        assert!(km.conflicts().is_empty(), "{:?}", km.conflicts());
        assert_eq!(Action::ALL.len(), 41);
    }

    #[test]
    fn ids_are_unique_and_roundtrip() {
        let mut seen = std::collections::HashSet::new();
        for a in Action::ALL {
            assert!(seen.insert(a.id()), "duplicate id: {}", a.id());
            assert_eq!(Action::from_id(a.id()), Some(a));
        }
        assert_eq!(Action::from_id("unknown"), None);
    }

    #[test]
    fn lookup_requires_exact_modifiers() {
        let km = Keymap::default();
        assert_eq!(km.action_for(Key::Z, Modifiers::COMMAND), Some(Action::Undo));
        assert_eq!(km.action_for(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT), Some(Action::Redo));
        assert_eq!(km.action_for(Key::Z, Modifiers::NONE), None);
        assert_eq!(km.action_for(Key::R, Modifiers::NONE), Some(Action::ToolRect));
        assert_eq!(km.action_for(Key::R, Modifiers::COMMAND), None, "Ctrl+R is not the R key alone");
        assert_eq!(km.action_for(Key::R, Modifiers::ALT), None);
    }

    #[test]
    fn shortcut_text_roundtrip() {
        for s in ["Ctrl+Shift+G", "Ctrl+Z", "Delete", "Shift+Alt+F5", "Ctrl+CloseBracket"] {
            let sc = Shortcut::parse(s).unwrap_or_else(|| panic!("illisible : {s}"));
            assert_eq!(Shortcut::parse(&sc.to_id()), Some(sc));
        }
        assert_eq!(Shortcut::parse("Ctrl+"), None);
        assert_eq!(Shortcut::parse("Ctrl+Unknown key"), None);
        assert_eq!(Shortcut::parse("maj+ctrl+g"), Some(Shortcut::ctrl_shift(Key::G)));
        assert_eq!(Shortcut::ctrl_shift(Key::G).label(), "Ctrl+Shift+G");
        assert_eq!(Shortcut::key(Key::Delete).label(), "Delete");
    }

    #[test]
    fn rebinding_steals_from_the_previous_owner() {
        let mut km = Keymap::default();
        // "R" belonged to the rectangle; give it to the ellipse
        let stolen = km.add(Action::ToolEllipse, Shortcut::key(Key::R)).unwrap();
        assert_eq!(stolen, Some(Action::ToolRect));
        assert_eq!(km.action_for(Key::R, Modifiers::NONE), Some(Action::ToolEllipse));
        assert!(km.bindings(Action::ToolRect).is_empty());
        assert!(km.conflicts().is_empty());
        // adding again does not duplicate
        assert_eq!(km.add(Action::ToolEllipse, Shortcut::key(Key::R)).unwrap(), None);
        assert_eq!(km.bindings(Action::ToolEllipse).iter().filter(|s| **s == Shortcut::key(Key::R)).count(), 1);
    }

    #[test]
    fn reserved_combinations_are_refused() {
        let mut km = Keymap::default();
        assert!(km.add(Action::Undo, Shortcut::key(Key::Escape)).is_err());
        assert!(km.add(Action::Undo, Shortcut::ctrl(Key::V)).is_err());
        assert!(km.add(Action::Undo, Shortcut::key(Key::ArrowLeft)).is_err());
        assert_eq!(km, Keymap::default());
    }

    #[test]
    fn reset_restores_defaults_and_removes_clones() {
        let mut km = Keymap::default();
        km.add(Action::ToolEllipse, Shortcut::key(Key::R)).unwrap();
        km.reset(Action::ToolRect);
        assert!(km.is_default(Action::ToolRect));
        assert_eq!(km.action_for(Key::R, Modifiers::NONE), Some(Action::ToolRect), "the original shortcut is given back");
        assert!(km.conflicts().is_empty());
        km.reset_all();
        assert_eq!(km, Keymap::default());
    }

    #[test]
    fn json_roundtrip_and_tolerance() {
        let mut km = Keymap::default();
        km.add(Action::ToolPen, Shortcut::new(Key::K, true, false, true)).unwrap();
        km.remove(Action::Delete, &Shortcut::key(Key::Backspace));
        let (back, ignored) = Keymap::from_json(&km.to_json()).unwrap();
        assert_eq!(back, km);
        assert_eq!(ignored, 0);
        // partial file with errors: keep what is valid, the rest stays default
        let (k2, ignored) = Keymap::from_json(
            r#"{"version":1,"bindings":{"tool.rect":["K"],"action.invented":["X"],"edit.undo":["Ctrl+Thing","Ctrl+W"],"file.save":["Escape"]}}"#,
        )
        .unwrap();
        assert_eq!(k2.action_for(Key::K, Modifiers::NONE), Some(Action::ToolRect));
        assert_eq!(k2.bindings(Action::Undo), &[Shortcut::ctrl(Key::W)]);
        assert!(k2.bindings(Action::Save).is_empty(), "Escape is reserved: entry ignored");
        assert_eq!(ignored, 3);
        assert!(k2.is_default(Action::Open), "actions not mentioned: default values");
        assert!(Keymap::from_json("not json").is_err());
        assert!(Keymap::from_json("{}").is_err());
    }

    #[test]
    fn settings_text_is_decoded_from_utf8_and_utf16() {
        let text = "{\u{e9}}";
        let le: Vec<u8> = [0xFF, 0xFE].into_iter().chain(text.encode_utf16().flat_map(u16::to_le_bytes)).collect();
        let be: Vec<u8> = [0xFE, 0xFF].into_iter().chain(text.encode_utf16().flat_map(u16::to_be_bytes)).collect();
        assert_eq!(Keymap::decode_text(&le).unwrap(), text);
        assert_eq!(Keymap::decode_text(&be).unwrap(), text);
        assert_eq!(Keymap::decode_text(text.as_bytes()).unwrap(), text);
        assert!(Keymap::decode_text(&[0xFF, 0xFE, 0x41]).is_err(), "odd length");
        assert!(Keymap::decode_text(&[0x80, 0x81]).is_err(), "not UTF-8");
    }

    #[test]
    fn tools_map_both_ways() {
        for a in Action::ALL {
            if let Some(t) = a.tool() {
                assert_eq!(Action::for_tool(t), Some(a));
            }
        }
    }
}
