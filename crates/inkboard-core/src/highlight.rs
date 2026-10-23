//! Minimal syntax coloring for code blocks.
//!
//! It is not a parser: it tells comments, strings, numbers and common keywords apart, on a dark background. Each [`Lang`] is a small
//! table (comment markers, quotes, keywords); [`Lang::Auto`] is a permissive mix that suits most C-family languages.

use crate::style::Color;
use crate::text::RichText;
use serde::{Deserialize, Serialize};

/// Language of a code block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    /// Generic coloring (comments `//`, `#`, `/* */`; common keywords of many languages).
    #[default]
    Auto,
    Rust,
    JavaScript,
    Python,
    C,
    #[serde(rename = "csharp")]
    CSharp,
    Java,
    Go,
    Shell,
    Sql,
    Json,
    Html,
    Css,
    Yaml,
}

impl Lang {
    pub const ALL: [Lang; 14] = [
        Lang::Auto,
        Lang::Rust,
        Lang::JavaScript,
        Lang::Python,
        Lang::C,
        Lang::CSharp,
        Lang::Java,
        Lang::Go,
        Lang::Shell,
        Lang::Sql,
        Lang::Json,
        Lang::Html,
        Lang::Css,
        Lang::Yaml,
    ];

    /// Display name (English; "Auto" is the only one a UI needs to translate).
    pub fn label(self) -> &'static str {
        match self {
            Lang::Auto => "Auto",
            Lang::Rust => "Rust",
            Lang::JavaScript => "JavaScript / TypeScript",
            Lang::Python => "Python",
            Lang::C => "C / C++",
            Lang::CSharp => "C#",
            Lang::Java => "Java",
            Lang::Go => "Go",
            Lang::Shell => "Shell",
            Lang::Sql => "SQL",
            Lang::Json => "JSON",
            Lang::Html => "HTML / XML",
            Lang::Css => "CSS",
            Lang::Yaml => "YAML",
        }
    }
}

struct Spec {
    line: &'static [&'static str],
    block: Option<(&'static str, &'static str)>,
    /// Characters opening a string closed by the same character.
    quotes: &'static [char],
    /// `'x'` is a character literal and a lone apostrophe is ignored (Rust lifetimes, C chars).
    char_lit: bool,
    keywords: &'static [&'static str],
    /// Keywords are matched ignoring case (SQL).
    ci: bool,
    /// Markup: `<tag` and `</tag` names are colored, numbers are not.
    tags: bool,
}

const GENERIC: &[&str] = &[
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

const RUST: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop",
    "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where",
    "while",
];
const JAVASCRIPT: &[&str] = &[
    "abstract",
    "as",
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "declare",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "from",
    "function",
    "if",
    "implements",
    "import",
    "in",
    "instanceof",
    "interface",
    "keyof",
    "let",
    "namespace",
    "new",
    "null",
    "of",
    "private",
    "protected",
    "public",
    "readonly",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "type",
    "typeof",
    "undefined",
    "var",
    "void",
    "while",
    "with",
    "yield",
];
const PYTHON: &[&str] = &[
    "and", "as", "assert", "async", "await", "break", "case", "class", "continue", "def", "del", "elif", "else", "except", "False", "finally", "for", "from",
    "global", "if", "import", "in", "is", "lambda", "match", "None", "nonlocal", "not", "or", "pass", "raise", "return", "self", "True", "try", "while",
    "with", "yield",
];
const C: &[&str] = &[
    "auto",
    "bool",
    "break",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "constexpr",
    "continue",
    "default",
    "delete",
    "do",
    "double",
    "else",
    "enum",
    "explicit",
    "extern",
    "false",
    "float",
    "for",
    "friend",
    "goto",
    "if",
    "inline",
    "int",
    "long",
    "namespace",
    "new",
    "nullptr",
    "operator",
    "private",
    "protected",
    "public",
    "register",
    "return",
    "short",
    "signed",
    "sizeof",
    "static",
    "struct",
    "switch",
    "template",
    "this",
    "throw",
    "true",
    "try",
    "typedef",
    "typename",
    "union",
    "unsigned",
    "using",
    "virtual",
    "void",
    "volatile",
    "while",
];
const CSHARP: &[&str] = &[
    "abstract",
    "as",
    "async",
    "await",
    "base",
    "bool",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "decimal",
    "default",
    "delegate",
    "do",
    "double",
    "else",
    "enum",
    "event",
    "explicit",
    "extern",
    "false",
    "finally",
    "float",
    "for",
    "foreach",
    "goto",
    "if",
    "implicit",
    "in",
    "int",
    "interface",
    "internal",
    "is",
    "lock",
    "long",
    "namespace",
    "new",
    "null",
    "object",
    "operator",
    "out",
    "override",
    "params",
    "private",
    "protected",
    "public",
    "readonly",
    "record",
    "ref",
    "return",
    "sealed",
    "short",
    "static",
    "string",
    "struct",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "uint",
    "ulong",
    "unsafe",
    "ushort",
    "using",
    "var",
    "virtual",
    "void",
    "volatile",
    "while",
];
const JAVA: &[&str] = &[
    "abstract",
    "assert",
    "boolean",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extends",
    "false",
    "final",
    "finally",
    "float",
    "for",
    "if",
    "implements",
    "import",
    "instanceof",
    "int",
    "interface",
    "long",
    "native",
    "new",
    "null",
    "package",
    "private",
    "protected",
    "public",
    "record",
    "return",
    "short",
    "static",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "transient",
    "true",
    "try",
    "var",
    "void",
    "volatile",
    "while",
];
const GO: &[&str] = &[
    "break",
    "case",
    "chan",
    "const",
    "continue",
    "default",
    "defer",
    "else",
    "fallthrough",
    "false",
    "for",
    "func",
    "go",
    "goto",
    "if",
    "import",
    "interface",
    "iota",
    "map",
    "nil",
    "package",
    "range",
    "return",
    "select",
    "struct",
    "switch",
    "true",
    "type",
    "var",
];
const SHELL: &[&str] = &[
    "alias", "case", "cd", "declare", "do", "done", "echo", "elif", "else", "esac", "exit", "export", "false", "fi", "for", "function", "if", "in", "local",
    "read", "readonly", "return", "select", "set", "source", "then", "time", "true", "unset", "until", "while",
];
const SQL: &[&str] = &[
    "add",
    "all",
    "alter",
    "and",
    "as",
    "asc",
    "avg",
    "begin",
    "between",
    "by",
    "case",
    "check",
    "column",
    "commit",
    "constraint",
    "count",
    "create",
    "cross",
    "default",
    "delete",
    "desc",
    "distinct",
    "drop",
    "else",
    "end",
    "exists",
    "false",
    "foreign",
    "from",
    "full",
    "group",
    "having",
    "in",
    "index",
    "inner",
    "insert",
    "into",
    "is",
    "join",
    "key",
    "left",
    "like",
    "limit",
    "max",
    "min",
    "not",
    "null",
    "offset",
    "on",
    "or",
    "order",
    "outer",
    "primary",
    "references",
    "right",
    "rollback",
    "select",
    "set",
    "sum",
    "table",
    "then",
    "true",
    "union",
    "unique",
    "update",
    "values",
    "view",
    "when",
    "where",
];
const JSON: &[&str] = &["true", "false", "null"];
const YAML: &[&str] = &["true", "false", "null", "yes", "no", "on", "off", "True", "False"];
const CSS: &[&str] = &[
    "absolute",
    "auto",
    "block",
    "fixed",
    "flex",
    "grid",
    "hidden",
    "important",
    "inherit",
    "initial",
    "inline",
    "none",
    "relative",
    "solid",
    "transparent",
    "visible",
];

fn spec(lang: Lang) -> Spec {
    let s = |line, block, quotes, char_lit, keywords, ci, tags| Spec { line, block, quotes, char_lit, keywords, ci, tags };
    const SLASH: Option<(&str, &str)> = Some(("/*", "*/"));
    match lang {
        Lang::Auto => s(&["//", "#"], SLASH, &['"', '`'], true, GENERIC, false, false),
        Lang::Rust => s(&["//"], SLASH, &['"'], true, RUST, false, false),
        Lang::JavaScript => s(&["//"], SLASH, &['"', '\'', '`'], false, JAVASCRIPT, false, false),
        Lang::Python => s(&["#"], None, &['"', '\''], false, PYTHON, false, false),
        Lang::C => s(&["//"], SLASH, &['"'], true, C, false, false),
        Lang::CSharp => s(&["//"], SLASH, &['"'], true, CSHARP, false, false),
        Lang::Java => s(&["//"], SLASH, &['"'], true, JAVA, false, false),
        Lang::Go => s(&["//"], SLASH, &['"', '`'], true, GO, false, false),
        Lang::Shell => s(&["#"], None, &['"', '\'', '`'], false, SHELL, false, false),
        Lang::Sql => s(&["--"], SLASH, &['\'', '"', '`'], false, SQL, true, false),
        Lang::Json => s(&[], None, &['"'], false, JSON, false, false),
        Lang::Html => s(&[], Some(("<!--", "-->")), &['"', '\''], false, &[], false, true),
        Lang::Css => s(&[], SLASH, &['"', '\''], false, CSS, false, false),
        Lang::Yaml => s(&["#"], None, &['"', '\''], false, YAML, false, false),
    }
}

const COMMENT: Color = Color::Rgb(0x7f, 0x8c, 0x9a);
const STRING: Color = Color::Rgb(0x98, 0xc3, 0x79);
const NUMBER: Color = Color::Rgb(0xd1, 0x9a, 0x66);
const KEYWORD: Color = Color::Rgb(0xc6, 0x78, 0xdd);

fn starts(c: &[char], i: usize, pat: &str) -> bool {
    pat.chars().enumerate().all(|(k, p)| c.get(i + k) == Some(&p))
}

/// Ranges `(start, end, color)` in character indices (generic coloring).
pub fn tokens(src: &str) -> Vec<(usize, usize, Color)> {
    tokens_lang(src, Lang::Auto)
}

/// Ranges `(start, end, color)` in character indices for the language `lang`.
pub fn tokens_lang(src: &str, lang: Lang) -> Vec<(usize, usize, Color)> {
    let sp = spec(lang);
    let c: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        let start = i;
        if let Some(l) = sp.line.iter().find(|l| starts(&c, i, l)) {
            i += l.chars().count();
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
            out.push((start, i, COMMENT));
        } else if let Some((open, close)) = sp.block.filter(|(o, _)| starts(&c, i, o)) {
            i += open.chars().count();
            while i < c.len() && !starts(&c, i, close) {
                i += 1;
            }
            i = (i + close.chars().count()).min(c.len());
            out.push((start, i, COMMENT));
        } else if sp.char_lit && ch == '\'' {
            // character literal (`'x'`, `'\n'`); otherwise a lone apostrophe (Rust lifetime `'a`)
            let close = if c.get(i + 1) == Some(&'\\') { i + 3 } else { i + 2 };
            if c.get(close) == Some(&'\'') {
                i = close + 1;
                out.push((start, i, STRING));
            } else {
                i += 1;
            }
        } else if sp.quotes.contains(&ch) {
            i += 1;
            while i < c.len() && c[i] != ch && (c[i] != '\n' || ch == '`') {
                i += usize::from(c[i] == '\\') + 1;
            }
            i = (i + 1).min(c.len());
            out.push((start, i, STRING));
        } else if sp.tags && ch == '<' && c.get(i + 1).is_some_and(|n| n.is_alphabetic() || *n == '/') {
            i += 1 + usize::from(c[i + 1] == '/');
            let name_start = i;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '-' || c[i] == ':') {
                i += 1;
            }
            if i > name_start {
                out.push((name_start, i, KEYWORD));
            }
        } else if !sp.tags && ch.is_ascii_digit() {
            while i < c.len() && (c[i].is_ascii_alphanumeric() || c[i] == '.' || c[i] == '_') {
                i += 1;
            }
            out.push((start, i, NUMBER));
        } else if ch.is_alphabetic() || ch == '_' {
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            let word: String = c[start..i].iter().collect();
            let hit = if sp.ci { sp.keywords.iter().any(|k| k.eq_ignore_ascii_case(&word)) } else { sp.keywords.contains(&word.as_str()) };
            if hit {
                out.push((start, i, KEYWORD));
            }
        } else {
            i += 1;
        }
    }
    out
}

/// Rich text with the generic colors of `src`.
pub fn highlight(src: &str) -> RichText {
    highlight_lang(src, Lang::Auto)
}

/// Rich text with the colors of `src` for the language `lang`.
pub fn highlight_lang(src: &str, lang: Lang) -> RichText {
    let mut rt = RichText::plain(src);
    for (a, b, col) in tokens_lang(src, lang) {
        rt.restyle(a, b, |s| s.color = Some(col));
    }
    rt
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colored_in(src: &str, lang: Lang) -> Vec<(String, Color)> {
        tokens_lang(src, lang).into_iter().map(|(a, b, c)| (src.chars().skip(a).take(b - a).collect(), c)).collect()
    }

    fn colored(src: &str) -> Vec<(String, Color)> {
        colored_in(src, Lang::Auto)
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
        for lang in Lang::ALL {
            assert_eq!(highlight_lang(src, lang).to_plain(), src, "{lang:?}");
        }
    }

    #[test]
    fn each_language_uses_its_own_comments_and_keywords() {
        // `#` starts a comment in Python and Shell, but not in Rust or C (attributes, preprocessor)
        assert!(colored_in("x = 1 # note", Lang::Python).contains(&("# note".into(), COMMENT)));
        assert!(!colored_in("#[derive(Debug)] struct A;", Lang::Rust).iter().any(|(_, c)| *c == COMMENT));
        // SQL is case-insensitive and has `--` comments; `--` is not a comment in Rust
        let sql = colored_in("SELECT name FROM t -- all\nWHERE id = 3", Lang::Sql);
        for k in ["SELECT", "FROM", "WHERE"] {
            assert!(sql.contains(&(k.into(), KEYWORD)), "{k}");
        }
        assert!(sql.contains(&("-- all".into(), COMMENT)));
        assert!(!colored_in("a -- b", Lang::Rust).iter().any(|(_, c)| *c == COMMENT));
        // keywords specific to one language
        assert!(colored_in("func main() {}", Lang::Go).contains(&("func".into(), KEYWORD)));
        assert!(!colored_in("func main() {}", Lang::Rust).contains(&("func".into(), KEYWORD)));
        // single quotes are strings in JavaScript, not in Rust
        assert!(colored_in("let s = 'abc';", Lang::JavaScript).contains(&("'abc'".into(), STRING)));
    }

    #[test]
    fn json_and_html_and_yaml() {
        let j = colored_in("{\"a\": [1, true, null]}", Lang::Json);
        assert!(j.contains(&("\"a\"".into(), STRING)) && j.contains(&("true".into(), KEYWORD)) && j.contains(&("1".into(), NUMBER)));
        let h = colored_in("<!-- c --><div class=\"x\">hi 12</div>", Lang::Html);
        assert!(h.contains(&("<!-- c -->".into(), COMMENT)));
        assert!(h.contains(&("div".into(), KEYWORD)) && h.contains(&("\"x\"".into(), STRING)));
        assert!(!h.iter().any(|(_, c)| *c == NUMBER), "numbers are not colored in markup");
        let y = colored_in("on: true # c", Lang::Yaml);
        assert!(y.contains(&("# c".into(), COMMENT)) && y.contains(&("true".into(), KEYWORD)));
    }

    #[test]
    fn language_ids_are_stable_in_files() {
        assert_eq!(serde_json::to_string(&Lang::CSharp).unwrap(), "\"csharp\"");
        assert_eq!(serde_json::from_str::<Lang>("\"javascript\"").unwrap(), Lang::JavaScript);
    }
}
