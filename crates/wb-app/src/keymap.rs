//! Raccourcis clavier personnalisables : actions, combinaisons, fichier de réglages.

use crate::tools::Tool;
use eframe::egui::{Key, Modifiers};
use std::collections::BTreeMap;
use wb_core::BoxGeom;

/// Commande de l'application que l'on peut lier à un raccourci.
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
    Present,
}

impl Action {
    pub const ALL: [Action; 40] = [
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
        Action::Present,
    ];

    /// Identifiant stable utilisé dans le fichier de réglages.
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
            Action::Present => "view.present",
        }
    }

    pub fn from_id(id: &str) -> Option<Action> {
        Action::ALL.into_iter().find(|a| a.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Action::New => "Nouveau tableau",
            Action::Open => "Ouvrir / importer",
            Action::Save => "Enregistrer",
            Action::SaveAs => "Enregistrer sous",
            Action::Undo => "Annuler",
            Action::Redo => "Rétablir",
            Action::SelectAll => "Tout sélectionner",
            Action::Duplicate => "Dupliquer",
            Action::Delete => "Supprimer",
            Action::Group => "Grouper",
            Action::Ungroup => "Dégrouper",
            Action::Front => "Premier plan",
            Action::Forward => "Avancer",
            Action::Backward => "Reculer",
            Action::Back => "Arrière-plan",
            Action::Zoom100 => "Zoom 100 %",
            Action::ZoomFit => "Tout afficher",
            Action::ZoomIn => "Zoom avant",
            Action::ZoomOut => "Zoom arrière",
            Action::EditText => "Modifier le texte",
            Action::Bold => "Gras",
            Action::Italic => "Italique",
            Action::Underline => "Souligné",
            Action::Strike => "Barré",
            Action::ToolLock => "Garder l'outil actif",
            Action::ToolSelect => "Outil : sélection",
            Action::ToolHand => "Outil : main",
            Action::ToolPen => "Outil : crayon",
            Action::ToolHighlighter => "Outil : surligneur",
            Action::ToolEraser => "Outil : gomme",
            Action::ToolRect => "Outil : rectangle",
            Action::ToolEllipse => "Outil : ellipse",
            Action::ToolDiamond => "Outil : losange",
            Action::ToolTriangle => "Outil : triangle",
            Action::ToolLine => "Outil : ligne",
            Action::ToolArrow => "Outil : flèche",
            Action::ToolText => "Outil : texte",
            Action::ToolSticky => "Outil : note adhésive",
            Action::ToolTable => "Outil : tableau",
            Action::Present => "Présenter en plein écran",
        }
    }

    pub fn group(self) -> &'static str {
        match self {
            Action::New | Action::Open | Action::Save | Action::SaveAs => "Fichier",
            Action::Undo | Action::Redo | Action::SelectAll | Action::Duplicate | Action::Delete | Action::Group | Action::Ungroup | Action::EditText => {
                "Édition"
            }
            Action::Front | Action::Forward | Action::Backward | Action::Back => "Ordre",
            Action::Zoom100 | Action::ZoomFit | Action::ZoomIn | Action::ZoomOut | Action::Present => "Vue",
            Action::Bold | Action::Italic | Action::Underline | Action::Strike => "Texte",
            _ => "Outils",
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
    /// Ctrl (Cmd sur Mac).
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

    /// Correspondance exacte : les modificateurs doivent être identiques.
    pub fn matches(&self, key: Key, m: Modifiers) -> bool {
        self.key == key && self.ctrl == m.command && self.shift == m.shift && self.alt == m.alt
    }

    /// Forme stockée dans le fichier de réglages : « Ctrl+Shift+G ».
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

    /// Texte affiché à l'utilisateur.
    pub fn label(&self) -> String {
        let key = match self.key {
            Key::Delete => "Suppr".to_owned(),
            Key::Backspace => "Retour arrière".to_owned(),
            Key::Enter => "Entrée".to_owned(),
            Key::Escape => "Échap".to_owned(),
            Key::Space => "Espace".to_owned(),
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
            s.push_str("Maj+");
        }
        if self.alt {
            s.push_str("Alt+");
        }
        s + &key
    }
}

/// Touches qui ne peuvent pas être réaffectées (réservées par l'application ou par le système).
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
    /// Raccourcis proposés par défaut pour une action.
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
            Action::Present => vec![S::key(Key::F5)],
        }
    }

    pub fn bindings(&self, a: Action) -> &[Shortcut] {
        self.map.get(&a).map_or(&[], Vec::as_slice)
    }

    /// Action liée à cette combinaison, s'il y en a une.
    pub fn action_for(&self, key: Key, m: Modifiers) -> Option<Action> {
        self.map.iter().find(|(_, v)| v.iter().any(|s| s.matches(key, m))).map(|(a, _)| *a)
    }

    /// Premier raccourci d'une action, prêt à afficher (vide s'il n'y en a pas).
    pub fn label(&self, a: Action) -> String {
        self.bindings(a).first().map(Shortcut::label).unwrap_or_default()
    }

    /// Lie un raccourci à une action. S'il servait déjà à une autre action, il lui est retiré ;
    /// cette autre action est renvoyée. Les combinaisons réservées sont refusées.
    pub fn add(&mut self, a: Action, sc: Shortcut) -> Result<Option<Action>, String> {
        if is_reserved(&sc) {
            return Err(format!("« {} » est réservé (échappement, flèches, copier/couper/coller)", sc.label()));
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
        // les raccourcis par défaut ne doivent pas rester en double ailleurs
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

    /// Actions dont un raccourci est partagé (ne devrait pas arriver, sauf fichier modifié à la main).
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
        serde_json::to_string_pretty(&serde_json::json!({ "version": 1, "bindings": bindings })).expect("JSON valide")
    }

    /// Lit un fichier de réglages. Les actions absentes gardent leur raccourci par défaut ; les entrées
    /// inconnues ou illisibles sont ignorées. Renvoie aussi le nombre d'entrées ignorées.
    pub fn from_json(s: &str) -> Result<(Keymap, usize), String> {
        let v: serde_json::Value = serde_json::from_str(s).map_err(|e| format!("JSON invalide : {e}"))?;
        let b = v.get("bindings").and_then(|b| b.as_object()).ok_or("réglages sans « bindings »")?;
        let mut km = Keymap::default();
        let mut ignored = 0;
        for (id, list) in b {
            let Some(a) = Action::from_id(id) else {
                ignored += 1;
                continue;
            };
            let mut scs = Vec::new();
            for item in list.as_array().map(Vec::as_slice).unwrap_or_default() {
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
            assert!(!km.bindings(a).is_empty(), "{a:?} sans raccourci par défaut");
        }
        assert!(km.conflicts().is_empty(), "{:?}", km.conflicts());
        assert_eq!(Action::ALL.len(), 40);
    }

    #[test]
    fn ids_are_unique_and_roundtrip() {
        let mut seen = std::collections::HashSet::new();
        for a in Action::ALL {
            assert!(seen.insert(a.id()), "id en double : {}", a.id());
            assert_eq!(Action::from_id(a.id()), Some(a));
        }
        assert_eq!(Action::from_id("inconnu"), None);
    }

    #[test]
    fn lookup_requires_exact_modifiers() {
        let km = Keymap::default();
        assert_eq!(km.action_for(Key::Z, Modifiers::COMMAND), Some(Action::Undo));
        assert_eq!(km.action_for(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT), Some(Action::Redo));
        assert_eq!(km.action_for(Key::Z, Modifiers::NONE), None);
        assert_eq!(km.action_for(Key::R, Modifiers::NONE), Some(Action::ToolRect));
        assert_eq!(km.action_for(Key::R, Modifiers::COMMAND), None, "Ctrl+R n'est pas la touche R seule");
        assert_eq!(km.action_for(Key::R, Modifiers::ALT), None);
    }

    #[test]
    fn shortcut_text_roundtrip() {
        for s in ["Ctrl+Shift+G", "Ctrl+Z", "Delete", "Shift+Alt+F5", "Ctrl+CloseBracket"] {
            let sc = Shortcut::parse(s).unwrap_or_else(|| panic!("illisible : {s}"));
            assert_eq!(Shortcut::parse(&sc.to_id()), Some(sc));
        }
        assert_eq!(Shortcut::parse("Ctrl+"), None);
        assert_eq!(Shortcut::parse("Ctrl+Touche inconnue"), None);
        assert_eq!(Shortcut::parse("maj+ctrl+g"), Some(Shortcut::ctrl_shift(Key::G)));
        assert_eq!(Shortcut::ctrl_shift(Key::G).label(), "Ctrl+Maj+G");
        assert_eq!(Shortcut::key(Key::Delete).label(), "Suppr");
    }

    #[test]
    fn rebinding_steals_from_the_previous_owner() {
        let mut km = Keymap::default();
        // « R » appartenait au rectangle ; on le donne à l'ellipse
        let stolen = km.add(Action::ToolEllipse, Shortcut::key(Key::R)).unwrap();
        assert_eq!(stolen, Some(Action::ToolRect));
        assert_eq!(km.action_for(Key::R, Modifiers::NONE), Some(Action::ToolEllipse));
        assert!(km.bindings(Action::ToolRect).is_empty());
        assert!(km.conflicts().is_empty());
        // ré-ajouter ne duplique pas
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
        assert_eq!(km.action_for(Key::R, Modifiers::NONE), Some(Action::ToolRect), "le raccourci d'origine est rendu");
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
        // fichier partiel avec des erreurs : on garde ce qui est valide, le reste par défaut
        let (k2, ignored) = Keymap::from_json(
            r#"{"version":1,"bindings":{"tool.rect":["K"],"action.inventée":["X"],"edit.undo":["Ctrl+Truc","Ctrl+W"],"file.save":["Escape"]}}"#,
        )
        .unwrap();
        assert_eq!(k2.action_for(Key::K, Modifiers::NONE), Some(Action::ToolRect));
        assert_eq!(k2.bindings(Action::Undo), &[Shortcut::ctrl(Key::W)]);
        assert!(k2.bindings(Action::Save).is_empty(), "Échap est réservé : entrée ignorée");
        assert_eq!(ignored, 3);
        assert!(k2.is_default(Action::Open), "actions non mentionnées : valeurs par défaut");
        assert!(Keymap::from_json("pas du json").is_err());
        assert!(Keymap::from_json("{}").is_err());
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
