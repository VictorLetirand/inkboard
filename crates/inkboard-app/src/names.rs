//! Text that comes from a person or a guest and ends up somewhere that matters: display names (host and guest sides share ONE function)
//! and the file name proposed for a document (security review S-17 and S-19).

/// Longest display name, in characters.
pub const NAME_MAX: usize = 40;
/// Longest file name stem proposed for a document, in characters.
const FILE_STEM_MAX: usize = 100;

/// Unicode category `Cf` (format characters, among them every bidirectional mark) as of Unicode 15.
fn is_format(c: char) -> bool {
    matches!(
        u32::from(c),
        0x00AD
            | 0x0600..=0x0605
            | 0x061C
            | 0x06DD
            | 0x070F
            | 0x0890..=0x0891
            | 0x08E2
            | 0x180E
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x2064
            | 0x2066..=0x206F
            | 0xFEFF
            | 0xFFF9..=0xFFFB
            | 0x110BD
            | 0x110CD
            | 0x13430..=0x1343F
            | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A
            | 0xE0001
            | 0xE0020..=0xE007F
    )
}

/// Characters that draw nothing but are not white space for Unicode (so a name made of them would look empty): Hangul and Khmer
/// fillers, braille blank, combining grapheme joiner.
fn is_blank_lookalike(c: char) -> bool {
    matches!(u32::from(c), 0x034F | 0x115F | 0x1160 | 0x17B4 | 0x17B5 | 0x2800 | 0x3164 | 0xFFA0)
}

/// Controls, format characters and blank look-alikes: never kept in a name.
fn is_hidden(c: char) -> bool {
    c.is_control() || is_format(c) || is_blank_lookalike(c)
}

/// Cleans a display name: hidden characters removed, runs of white space collapsed, at most [`NAME_MAX`] characters.
/// `None` when no visible character is left. This is the only name cleaner of the application (host and guest).
pub fn clean_name(raw: &str) -> Option<String> {
    let mut out = String::new();
    let mut space = false;
    for c in raw.chars().filter(|c| !is_hidden(*c)) {
        if c.is_whitespace() {
            space = !out.is_empty();
        } else {
            if space {
                out.push(' ');
                space = false;
            }
            out.push(c);
        }
    }
    let cut: String = out.chars().take(NAME_MAX).collect();
    let cut = cut.trim_end();
    (!cut.is_empty()).then(|| cut.to_owned())
}

fn is_reserved_windows_name(stem: &str) -> bool {
    // the device name counts even with an extension ("CON.txt"), and trailing spaces are ignored by Windows
    let base = stem.split('.').next().unwrap_or("").trim_end().to_uppercase();
    match base.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" => true,
        _ => {
            let digits = base.strip_prefix("COM").or_else(|| base.strip_prefix("LPT"));
            digits.is_some_and(|d| d.chars().count() == 1 && d.chars().all(|c| matches!(c, '0'..='9' | '¹' | '²' | '³')))
        }
    }
}

/// A document name made safe to be proposed as a file name: no path separators or characters Windows refuses (`\ / : * ? " < > |`), no
/// controls or bidirectional marks, no leading or trailing dot or space, not a reserved device name (`CON`, `NUL`, `COM1`...), at most 100 characters.
pub fn file_stem(raw: &str) -> String {
    let kept: String = raw.chars().filter(|c| !is_hidden(*c) && !matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|')).collect();
    let cut: String = kept.trim_matches(['.', ' ']).chars().take(FILE_STEM_MAX).collect();
    let mut stem = cut.trim_end_matches(['.', ' ']).to_owned();
    if stem.is_empty() {
        stem = "board".to_owned();
    }
    if is_reserved_windows_name(&stem) {
        stem.insert(0, '_');
    }
    stem
}

/// `<safe stem>.<ext>`: the file name proposed in a "Save" dialog for the document called `doc_name`.
pub fn file_name(doc_name: &str, ext: &str) -> String {
    format!("{}.{ext}", file_stem(doc_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invisible_and_bidi_characters_are_removed_and_nameless_input_is_refused() {
        for blank in [
            "\u{3164}",
            "\u{2800}",
            "\u{115F}",
            "\u{1160}",
            "\u{034F}",
            "\u{061C}",
            "\u{FFA0}",
            "\u{200B}\u{200E}",
            "\u{202E}",
            "\u{00AD}",
            "\u{2800} \u{3164}",
            "\t\n",
            "",
        ] {
            assert_eq!(clean_name(blank), None, "{blank:?} has no visible character");
        }
        assert_eq!(clean_name("Anna\u{3164}"), Some("Anna".to_owned()));
        assert_eq!(clean_name("\u{2800}Anna\u{2800}"), Some("Anna".to_owned()));
        assert_eq!(clean_name("An\u{061C}na"), Some("Anna".to_owned()));
        assert_eq!(clean_name("\u{202E}evil\u{202C}"), Some("evil".to_owned()));
        assert_eq!(clean_name("a\u{2066}b\u{2069}c\u{E0041}"), Some("abc".to_owned()));
        assert_eq!(clean_name("  Anna   Bob  "), Some("Anna Bob".to_owned()));
        assert_eq!(clean_name("An\u{7}na\n\tBob"), Some("AnnaBob".to_owned()));
        assert_eq!(clean_name(&"é".repeat(100)).map(|n| n.chars().count()), Some(NAME_MAX));
        assert_eq!(clean_name(&format!("{} b", "a".repeat(39))), Some("a".repeat(39)));
    }

    #[test]
    fn file_names_are_safe_on_windows() {
        assert_eq!(file_name("Plan", "inkboard"), "Plan.inkboard");
        assert_eq!(file_stem("..\\..\\Windows/evil"), "Windowsevil");
        assert_eq!(file_stem("a:b*c?d\"e<f>g|h"), "abcdefgh");
        assert_eq!(file_stem("gpj\u{202E}fdp"), "gpjfdp");
        assert_eq!(file_stem("tab\there\u{7}"), "tabhere");
        assert_eq!(file_stem("report. . "), "report");
        for reserved in ["CON", "con", "Nul", "AUX", "PRN", "COM1", "lpt9", "COM\u{b9}", "CON.txt", "nul ", "COM0"] {
            assert!(file_stem(reserved).starts_with('_'), "{reserved:?} is a Windows device name");
        }
        for fine in ["COM10", "CONSOLE", "Aux2", "console"] {
            assert_eq!(file_stem(fine), fine);
        }
        assert_eq!(file_stem(""), "board");
        assert_eq!(file_stem("///"), "board");
        assert_eq!(file_stem("\u{3164}"), "board");
        assert_eq!(file_stem(&"x".repeat(500)).chars().count(), 100);
        assert_eq!(file_name("a/b", "svg"), "ab.svg");
    }
}
