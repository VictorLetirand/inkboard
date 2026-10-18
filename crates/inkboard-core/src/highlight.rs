//! Coloration syntaxique minimale pour les blocs de code (langages « famille C » : Rust, JS/TS, C/C++, Java, Go, Python, shell…).
//!
//! Ce n'est pas un analyseur : il distingue commentaires, chaînes, nombres et mots-clés courants, sur fond sombre.

use crate::style::Color;
use crate::text::RichText;

const KEYWORDS: &[&str] = &[
    "fn",
    "let",
    "mut",
    "const",
    "static",
    "pub",
    "use",
    "mod",
    "struct",
    "enum",
    "impl",
    "trait",
    "type",
    "match",
    "if",
    "else",
    "elif",
    "for",
    "while",
    "loop",
    "in",
    "return",
    "break",
    "continue",
    "as",
    "where",
    "async",
    "await",
    "move",
    "self",
    "Self",
    "crate",
    "super",
    "unsafe",
    "dyn",
    "ref",
    "function",
    "var",
    "class",
    "new",
    "this",
    "import",
    "export",
    "from",
    "def",
    "lambda",
    "pass",
    "yield",
    "with",
    "try",
    "catch",
    "except",
    "finally",
    "throw",
    "switch",
    "case",
    "default",
    "int",
    "float",
    "double",
    "char",
    "void",
    "bool",
    "string",
    "package",
    "interface",
    "extends",
    "implements",
    "public",
    "private",
    "protected",
    "true",
    "false",
    "null",
    "None",
    "True",
    "False",
    "nil",
    "undefined",
    "and",
    "or",
    "not",
    "is",
    "do",
    "done",
    "then",
    "fi",
    "esac",
];

const COMMENT: Color = Color::Rgb(0x7f, 0x8c, 0x9a);
const STRING: Color = Color::Rgb(0x98, 0xc3, 0x79);
const NUMBER: Color = Color::Rgb(0xd1, 0x9a, 0x66);
const KEYWORD: Color = Color::Rgb(0xc6, 0x78, 0xdd);

/// Intervalles `(début, fin, couleur)` en indices de caractères.
pub fn tokens(src: &str) -> Vec<(usize, usize, Color)> {
    let c: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        let start = i;
        if (ch == '/' && c.get(i + 1) == Some(&'/')) || ch == '#' {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
            out.push((start, i, COMMENT));
        } else if ch == '/' && c.get(i + 1) == Some(&'*') {
            i += 2;
            while i < c.len() && !(c[i] == '*' && c.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i = (i + 2).min(c.len());
            out.push((start, i, COMMENT));
        } else if ch == '\'' {
            // littéral de caractère (`'x'`, `'\n'`) ; sinon apostrophe isolée (durée de vie Rust `'a`)
            let close = if c.get(i + 1) == Some(&'\\') { i + 3 } else { i + 2 };
            if c.get(close) == Some(&'\'') {
                i = close + 1;
                out.push((start, i, STRING));
            } else {
                i += 1;
            }
        } else if ch == '"' || ch == '`' {
            i += 1;
            while i < c.len() && c[i] != ch && c[i] != '\n' {
                i += usize::from(c[i] == '\\') + 1;
            }
            i = (i + 1).min(c.len());
            out.push((start, i, STRING));
        } else if ch.is_ascii_digit() {
            while i < c.len() && (c[i].is_ascii_alphanumeric() || c[i] == '.' || c[i] == '_') {
                i += 1;
            }
            out.push((start, i, NUMBER));
        } else if ch.is_alphabetic() || ch == '_' {
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            let word: String = c[start..i].iter().collect();
            if KEYWORDS.contains(&word.as_str()) {
                out.push((start, i, KEYWORD));
            }
        } else {
            i += 1;
        }
    }
    out
}

/// Texte enrichi des couleurs de `src`.
pub fn highlight(src: &str) -> RichText {
    let mut rt = RichText::plain(src);
    for (a, b, col) in tokens(src) {
        rt.restyle(a, b, |s| s.color = Some(col));
    }
    rt
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colored(src: &str) -> Vec<(String, Color)> {
        tokens(src).into_iter().map(|(a, b, c)| (src.chars().skip(a).take(b - a).collect(), c)).collect()
    }

    #[test]
    fn keywords_strings_numbers_and_comments() {
        let t = colored("let x = 42; // note\nprintln!(\"a\");");
        assert!(t.contains(&("let".into(), KEYWORD)));
        assert!(t.contains(&("42".into(), NUMBER)));
        assert!(t.contains(&("// note".into(), COMMENT)));
        assert!(t.contains(&("\"a\"".into(), STRING)));
    }

    #[test]
    fn identifiers_containing_keywords_are_not_colored_and_lifetimes_are_not_strings() {
        assert!(colored("format letter").is_empty());
        let t = colored("fn f<'a>(x: &'a str)");
        assert_eq!(t, vec![("fn".to_owned(), KEYWORD)]);
        assert_eq!(colored("let c = 'x';").len(), 2);
    }

    #[test]
    fn highlight_keeps_the_text_unchanged() {
        let src = "if (x) { return \"é\"; } /* c */";
        assert_eq!(highlight(src).to_plain(), src);
    }
}
