//! Interface languages. The code contains the **English** text; other languages are translation tables keyed by the English text
//! (see `i18n_fr.rs`). A missing translation falls back to English, and a test checks that every text used in the code is translated.

use std::cell::Cell;
use std::fmt::Display;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    Fr,
}

/// User choice: follow the system language, or force one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pref {
    Auto,
    Lang(Lang),
}

static GLOBAL: AtomicU8 = AtomicU8::new(0);

thread_local! {
    /// Per-thread override (tests run in parallel and must not change each other's language).
    static OVERRIDE: Cell<Option<Lang>> = const { Cell::new(None) };
}

fn code(l: Lang) -> u8 {
    match l {
        Lang::En => 0,
        Lang::Fr => 1,
    }
}

/// Language used by `t()` on the current thread.
pub fn current() -> Lang {
    OVERRIDE.with(Cell::get).unwrap_or(if GLOBAL.load(Ordering::Relaxed) == 1 { Lang::Fr } else { Lang::En })
}

/// Sets the application language.
pub fn set(l: Lang) {
    GLOBAL.store(code(l), Ordering::Relaxed);
}

/// Sets the language for the current thread only (tests).
#[cfg(test)]
pub fn set_for_thread(l: Option<Lang>) {
    OVERRIDE.with(|c| c.set(l));
}

/// Language of the system (`fr*` → French, anything else → English).
pub fn system() -> Lang {
    match sys_locale::get_locale() {
        Some(l) if l.to_ascii_lowercase().starts_with("fr") => Lang::Fr,
        _ => Lang::En,
    }
}

pub fn resolve(p: Pref) -> Lang {
    match p {
        Pref::Auto => system(),
        Pref::Lang(l) => l,
    }
}

fn table() -> &'static std::collections::HashMap<&'static str, &'static str> {
    static T: OnceLock<std::collections::HashMap<&str, &str>> = OnceLock::new();
    T.get_or_init(|| crate::i18n_fr::FR.iter().copied().collect())
}

/// Translates an English text into the current language (English when no translation exists).
pub fn t(s: &str) -> String {
    match current() {
        Lang::En => s.to_owned(),
        Lang::Fr if s.is_empty() => String::new(),
        Lang::Fr => match table().get(s) {
            Some(fr) => (*fr).to_owned(),
            None => {
                #[cfg(test)]
                MISSING.with(|m| m.borrow_mut().push(s.to_owned()));
                s.to_owned()
            }
        },
    }
}

#[cfg(test)]
thread_local! {
    /// English texts looked up in French mode without a translation (tests only).
    pub static MISSING: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Fills the `{}` placeholders of a (translated) template with `args`, in order.
pub fn fill(template: String, args: &[&dyn Display]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template.as_str();
    let mut it = args.iter();
    while let Some(i) = rest.find("{}") {
        out.push_str(&rest[..i]);
        match it.next() {
            Some(a) => out.push_str(&a.to_string()),
            None => out.push_str("{}"),
        }
        rest = &rest[i + 2..];
    }
    out.push_str(rest);
    out
}

/// `tr!("Cell row {} · column {}", r, c)` : translated template, then arguments inserted in order.
macro_rules! tr {
    ($t:literal $(, $a:expr)* $(,)?) => {
        $crate::i18n::fill($crate::i18n::t($t), &[$(&$a as &dyn std::fmt::Display),*])
    };
}
pub(crate) use tr;

/* ---------------------------------------------------------------- preference file */

fn pref_path() -> std::path::PathBuf {
    inkboard_mcp::live::data_dir().join("language")
}

pub fn load_pref() -> Pref {
    match std::fs::read_to_string(pref_path()).map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Ok("en") => Pref::Lang(Lang::En),
        Ok("fr") => Pref::Lang(Lang::Fr),
        _ => Pref::Auto,
    }
}

pub fn save_pref(p: Pref) {
    let v = match p {
        Pref::Auto => "auto",
        Pref::Lang(Lang::En) => "en",
        Pref::Lang(Lang::Fr) => "fr",
    };
    let path = pref_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, v);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_are_filled_in_order_and_missing_arguments_are_kept() {
        assert_eq!(fill("{} of {}".to_owned(), &[&3, &"five"]), "3 of five");
        assert_eq!(fill("only {} {}".to_owned(), &[&1]), "only 1 {}");
        assert_eq!(fill("none".to_owned(), &[]), "none");
    }

    #[test]
    fn english_is_the_default_and_french_comes_from_the_table() {
        set_for_thread(Some(Lang::En));
        assert_eq!(t("Cut"), "Cut");
        set_for_thread(Some(Lang::Fr));
        assert_eq!(t("Cut"), "Couper");
        assert_eq!(t("a text that is not in the table"), "a text that is not in the table");
        assert_eq!(tr!("Cell row {} · column {}", 2, 3), "Cellule ligne 2 · colonne 3");
        set_for_thread(None);
    }

    #[test]
    fn french_table_has_no_duplicate_keys() {
        let mut keys: Vec<&str> = crate::i18n_fr::FR.iter().map(|(k, _)| *k).collect();
        keys.sort_unstable();
        let n = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), n, "duplicate English keys in i18n_fr.rs");
    }

    /// Source files of this crate that contain UI texts (the translation table and this module are excluded).
    fn ui_sources() -> Vec<(String, String)> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut v = Vec::new();
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            if p.extension().is_some_and(|x| x == "rs" || x == "toml") && !matches!(name.as_str(), "i18n_fr.rs" | "i18n.rs" | "tests.rs") {
                v.push((name, std::fs::read_to_string(&p).unwrap()));
            }
        }
        v
    }

    /// String literals passed directly to `t("…")` or `tr!("…")`.
    fn translated_literals(src: &str) -> Vec<String> {
        let b = src.as_bytes();
        let mut out = Vec::new();
        for pat in ["t(\"", "tr!(\""] {
            let mut from = 0;
            while let Some(i) = src[from..].find(pat) {
                let start = from + i;
                from = start + pat.len();
                if start > 0 && (b[start - 1].is_ascii_alphanumeric() || b[start - 1] == b'_') {
                    continue;
                }
                let mut s = String::new();
                let mut chars = src[from..].chars();
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some(o) => s.push(o),
                            None => break,
                        },
                        c => s.push(c),
                    }
                }
                out.push(s);
            }
        }
        out
    }

    #[test]
    fn every_text_passed_to_t_has_a_french_translation() {
        let table: std::collections::HashSet<&str> = crate::i18n_fr::FR.iter().map(|(k, _)| *k).collect();
        let mut missing = Vec::new();
        for (file, src) in ui_sources() {
            for lit in translated_literals(&src) {
                if !table.contains(lit.as_str()) {
                    missing.push(format!("{file}: {lit:?}"));
                }
            }
        }
        assert!(missing.is_empty(), "texts without a French translation in i18n_fr.rs:\n{}", missing.join("\n"));
    }

    #[test]
    fn every_french_key_is_still_used_in_the_source() {
        let mut all: String = ui_sources().into_iter().map(|(_, s)| s).collect();
        // background preset names come from the engine
        all.push_str(&std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../inkboard-core/src/model.rs")).unwrap());
        let unused: Vec<&str> = crate::i18n_fr::FR.iter().map(|(k, _)| *k).filter(|k| !all.contains(&format!("{k:?}"))).collect();
        assert!(unused.is_empty(), "translation keys not found in the source (renamed or removed text?):\n{}", unused.join("\n"));
    }

    #[test]
    fn no_french_is_left_in_the_source_outside_the_translation_table() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let accents = |l: &str| l.chars().any(|c| "àâçèêëîïôûùüœÀÉÈÊÎÔÛ".contains(c));
        // common French words that have no accent (the accent check cannot see them), looked for inside string literals
        let has_french_word = |line: &str| {
            const WORDS: &[&str] = &[
                "invalide",
                "valide",
                "manquant",
                "absent",
                "volumineuse",
                "lecture",
                "Bonjour",
                "inconnu",
                "inconnue",
                "vide",
                "Titre",
                "Objet",
                "Exemple",
                "fichier",
                "tableau",
                "erreur",
                "outil",
                "oui",
                "Fin",
                "monde",
                "Nouveau",
            ];
            line.split('"').skip(1).step_by(2).any(|lit| lit.split(|c: char| !c.is_alphanumeric()).any(|w| WORDS.contains(&w)))
        };
        let allowed = [
            "sans-serif",
            "wb-sans",
            "Sans serif",
            "tableau-blanc",
            ".tableau.json",
            r"\inconnu",
            "let has_french_word",
            "const WORDS",
            "let accents",
            "let allowed",
            "accents(line)",
            "Français",
            "é.inkboard",
            "Zip é",
            "\"é",
            "é🙂",
            "Café",
            "Cellule ligne",
        ];
        let mut found = Vec::new();
        let mut stack = vec![root];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap() {
                let p = e.unwrap().path();
                let name = p.file_name().unwrap().to_string_lossy().into_owned();
                if p.is_dir() {
                    if name != "target" && !name.starts_with('.') {
                        stack.push(p);
                    }
                } else if p.extension().is_some_and(|x| x == "rs") && !matches!(name.as_str(), "i18n_fr.rs" | "i18n.rs") {
                    for (n, line) in std::fs::read_to_string(&p).unwrap().lines().enumerate() {
                        if (accents(line) || line.contains('é') || has_french_word(line)) && !allowed.iter().any(|a| line.contains(a)) {
                            found.push(format!("{}:{}: {}", p.display(), n + 1, line.trim()));
                        }
                    }
                }
            }
        }
        assert!(found.is_empty(), "French text left in the source:\n{}", found.join("\n"));
    }
}
