//! Embedded fonts shared by the native application (during a shared session, or when the user asks for it) and the web client.
//!
//! The same font files everywhere means the same text widths, hence the same line breaks for every participant.
//! Sources and licenses (all free, see `fonts/*-OFL.txt` and `THIRD_PARTY_LICENSES.md`):
//! - sans: Noto Sans Regular / Bold (OFL 1.1), serif: Gelasio Regular / Bold (OFL 1.1, metric-compatible with Georgia),
//!   handwriting: Patrick Hand (OFL 1.1), monospace: Hack Regular / Bold (MIT + Bitstream Vera);
//! - fallbacks (Greek, Cyrillic, symbols, emoji): egui's own Ubuntu-Light, NotoEmoji and emoji-icon-font.
//!
//! The files of this crate are subsets (Latin, Latin Extended, punctuation, common symbols; no CJK) instanced from the upstream
//! variable fonts where needed. Italic is synthesized by egui (a skew), bold is real except for the handwriting font (no bold upstream).

use egui::{FontData, FontDefinitions, FontFamily, FontTweak};
use std::sync::Arc;

/// Names of the custom font families, as used by [`FontFamily::Name`].
pub const SANS: &str = "wb-sans";
pub const SANS_BOLD: &str = "wb-sans-bold";
pub const HAND: &str = "wb-hand";
pub const SERIF: &str = "wb-serif";
pub const SERIF_BOLD: &str = "wb-serif-bold";
pub const MONO: &str = "wb-mono";
pub const MONO_BOLD: &str = "wb-mono-bold";

const NOTO_SANS: &[u8] = include_bytes!("../fonts/NotoSans-Regular.ttf");
const NOTO_SANS_BOLD: &[u8] = include_bytes!("../fonts/NotoSans-Bold.ttf");
const GELASIO: &[u8] = include_bytes!("../fonts/Gelasio-Regular.ttf");
const GELASIO_BOLD: &[u8] = include_bytes!("../fonts/Gelasio-Bold.ttf");
const PATRICK_HAND: &[u8] = include_bytes!("../fonts/PatrickHand-Regular.ttf");
const HACK_BOLD: &[u8] = include_bytes!("../fonts/Hack-Bold.ttf");

/// Total size of the files owned by this crate (the fallbacks come from `epaint_default_fonts`, already in the binary).
pub const OWN_BYTES: usize = NOTO_SANS.len() + NOTO_SANS_BOLD.len() + GELASIO.len() + GELASIO_BOLD.len() + PATRICK_HAND.len() + HACK_BOLD.len();

/// The complete font set: egui's default fonts (same keys) plus the board families. `Proportional` (the interface) starts with Noto Sans.
pub fn definitions() -> FontDefinitions {
    let mut defs = FontDefinitions::empty();
    let mut add = |key: &str, data: FontData| {
        defs.font_data.insert(key.to_owned(), Arc::new(data));
    };
    // egui's defaults, same keys and tweaks as `FontDefinitions::default()`
    add("Hack", FontData::from_static(epaint_default_fonts::HACK_REGULAR));
    add("NotoEmoji-Regular", FontData::from_static(epaint_default_fonts::NOTO_EMOJI_REGULAR).tweak(FontTweak { scale: 0.81, ..Default::default() }));
    add("Ubuntu-Light", FontData::from_static(epaint_default_fonts::UBUNTU_LIGHT));
    add("emoji-icon-font", FontData::from_static(epaint_default_fonts::EMOJI_ICON).tweak(FontTweak { scale: 0.90, ..Default::default() }));
    add("noto-sans", FontData::from_static(NOTO_SANS));
    add("noto-sans-bold", FontData::from_static(NOTO_SANS_BOLD));
    add("gelasio", FontData::from_static(GELASIO));
    add("gelasio-bold", FontData::from_static(GELASIO_BOLD));
    // Patrick Hand has a small x-height: enlarged so that a sticky note reads like the other families at the same size
    add("patrick-hand", FontData::from_static(PATRICK_HAND).tweak(FontTweak { scale: 1.12, ..Default::default() }));
    add("hack-bold", FontData::from_static(HACK_BOLD));

    // Greek, Cyrillic, symbols and emoji fall back on egui's fonts
    let tail = ["Ubuntu-Light", "NotoEmoji-Regular", "emoji-icon-font"];
    let chain = |first: &[&str]| first.iter().chain(tail.iter()).map(|s| (*s).to_owned()).collect::<Vec<_>>();
    defs.families.insert(FontFamily::Proportional, chain(&["noto-sans"]));
    defs.families.insert(FontFamily::Monospace, chain(&["Hack"]));
    for (name, first) in [
        (SANS, &["noto-sans"][..]),
        (SANS_BOLD, &["noto-sans-bold", "noto-sans"]),
        (HAND, &["patrick-hand", "noto-sans"]),
        (SERIF, &["gelasio", "noto-sans"]),
        (SERIF_BOLD, &["gelasio-bold", "gelasio", "noto-sans"]),
        (MONO, &["Hack"]),
        (MONO_BOLD, &["hack-bold", "Hack"]),
    ] {
        defs.families.insert(FontFamily::Name(name.into()), chain(first));
    }
    defs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_family_has_fonts_that_exist() {
        let d = definitions();
        for (fam, keys) in &d.families {
            assert!(!keys.is_empty(), "{fam:?} has no font");
            for k in keys {
                assert!(d.font_data.contains_key(k), "{fam:?}: unknown font {k}");
            }
        }
        for name in [SANS, SANS_BOLD, HAND, SERIF, SERIF_BOLD, MONO, MONO_BOLD] {
            assert!(d.families.contains_key(&FontFamily::Name(name.into())), "{name}");
        }
    }

    #[test]
    fn the_added_files_stay_within_budget() {
        const { assert!(OWN_BYTES < 1_000_000) };
    }

    #[test]
    fn the_license_texts_are_shipped_next_to_the_fonts() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fonts");
        for f in ["NotoSans-OFL.txt", "Gelasio-OFL.txt", "PatrickHand-OFL.txt"] {
            let t = std::fs::read_to_string(dir.join(f)).unwrap();
            assert!(t.contains("SIL OPEN FONT LICENSE Version 1.1"), "{f}");
        }
    }
}
