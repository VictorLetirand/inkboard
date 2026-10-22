//! Fonts: loading of system fonts (Windows) or of the embedded set shared with the web client, and text measurement for the engine.

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily as EFamily, FontId};
use inkboard_core::FontFamily;
use inkboard_core::text::Measurer;
use std::sync::Arc;

use inkboard_fonts::{HAND, MONO, MONO_BOLD, SANS, SANS_BOLD, SERIF, SERIF_BOLD};

/// The embedded fonts are the only ones available (web client): the setting does not apply.
pub const ALWAYS_EMBEDDED: bool = cfg!(target_arch = "wasm32");

fn pref_path() -> std::path::PathBuf {
    inkboard_mcp::live::data_dir().join("embedded_fonts")
}

/// Setting "Use built-in fonts" (data folder, like `language`); off by default.
pub fn load_embedded_pref() -> bool {
    std::fs::read_to_string(pref_path()).is_ok_and(|s| s.trim() == "1")
}

pub fn save_embedded_pref(on: bool) {
    let path = pref_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, if on { "1" } else { "0" });
}

/// Installs the embedded set (`embedded`) or the system fonts. egui rebuilds its glyph atlas and galleys on the next frame: until then
/// the previous fonts stay in use for both measuring and painting, so a frame never mixes the two sets.
pub fn apply(ctx: &egui::Context, embedded: bool) {
    if embedded || ALWAYS_EMBEDDED {
        ctx.set_fonts(inkboard_fonts::definitions());
    } else {
        install(ctx);
    }
}

fn read_system_font(file: &str) -> Option<Vec<u8>> {
    // tests only: `INKBOARD_NO_SYSTEM_FONTS=1` forces egui's built-in fonts, as on a machine without the Windows fonts (CI Linux / macOS)
    if cfg!(test) && std::env::var_os("INKBOARD_NO_SYSTEM_FONTS").is_some() {
        return None;
    }
    let dirs: [&str; 3] = ["C:\\Windows\\Fonts", "/usr/share/fonts/truetype/dejavu", "/System/Library/Fonts/Supplemental"];
    dirs.iter().find_map(|d| std::fs::read(std::path::Path::new(d).join(file)).ok())
}

/// Installs the board's fonts (Segoe UI, Segoe Print, Georgia, Consolas when they exist).
pub fn install(ctx: &egui::Context) {
    let mut defs = FontDefinitions::default();
    let base_prop = defs.families.get(&EFamily::Proportional).cloned().unwrap_or_default();
    let base_mono = defs.families.get(&EFamily::Monospace).cloned().unwrap_or_default();
    let mut family = |name: &str, key: &str, files: &[&str], fallback: &[String]| {
        let mut chain: Vec<String> = Vec::new();
        for f in files {
            if let Some(bytes) = read_system_font(f) {
                defs.font_data.insert(key.to_owned(), Arc::new(FontData::from_owned(bytes)));
                chain.push(key.to_owned());
                break;
            }
        }
        chain.extend(fallback.iter().cloned());
        defs.families.insert(EFamily::Name(name.into()), chain.clone());
        chain.first().is_some_and(|k| k == key)
    };
    let has_sans = family(SANS, "segoeui", &["segoeui.ttf"], &base_prop);
    family(SANS_BOLD, "segoeuib", &["segoeuib.ttf"], &base_prop);
    family(HAND, "segoepr", &["segoepr.ttf", "comic.ttf"], &base_prop);
    family(SERIF, "georgia", &["georgia.ttf", "times.ttf"], &base_prop);
    family(MONO, "consolas", &["consola.ttf"], &base_mono);
    // bold serif / monospace: the bold file when it exists, else the regular chain (as before)
    for (bold, regular, key, file) in [(SERIF_BOLD, SERIF, "georgiab", "georgiab.ttf"), (MONO_BOLD, MONO, "consolab", "consolab.ttf")] {
        let mut chain = defs.families[&EFamily::Name(regular.into())].clone();
        if let Some(bytes) = read_system_font(file) {
            defs.font_data.insert(key.to_owned(), Arc::new(FontData::from_owned(bytes)));
            chain.insert(0, key.to_owned());
        }
        defs.families.insert(EFamily::Name(bold.into()), chain);
    }
    if has_sans {
        defs.families.entry(EFamily::Proportional).or_default().insert(0, "segoeui".to_owned());
    }
    ctx.set_fonts(defs);
}

/// egui font corresponding to a model font family.
pub fn font_id(f: FontFamily, size: f32, bold: bool) -> FontId {
    let name = match (f, bold) {
        (FontFamily::Sans, true) => SANS_BOLD,
        (FontFamily::Sans, false) => SANS,
        (FontFamily::Hand, _) => HAND,
        (FontFamily::Serif, false) => SERIF,
        (FontFamily::Serif, true) => SERIF_BOLD,
        (FontFamily::Mono, false) => MONO,
        (FontFamily::Mono, true) => MONO_BOLD,
    };
    FontId::new(size.max(1.0), EFamily::Name(name.into()))
}

/// Exact text measurement with the application's fonts.
#[derive(Clone)]
pub struct EguiMeasurer(pub egui::Context);

impl Measurer for EguiMeasurer {
    fn width(&self, s: &str, font: FontFamily, size: f64, bold: bool) -> f64 {
        if s.is_empty() {
            return 0.0;
        }
        self.0.fonts_mut(|f| f64::from(f.layout_no_wrap(s.to_owned(), font_id(font, size as f32, bold), Color32::WHITE).size().x))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkboard_core::text::{RichText, SpanStyle, layout_rich};

    const FAMILIES: [FontFamily; 4] = [FontFamily::Sans, FontFamily::Serif, FontFamily::Hand, FontFamily::Mono];
    const SIZES: [f64; 5] = [9.0, 12.5, 16.0, 24.0, 41.0];
    const SAMPLE: &str = "Cañón meeting: 3 000 € budget, Łódź “quoted” (Ångström, Tromsø, Dvořák) — WWW iii 0123456789 {}[]<=>";

    /// A context built from scratch with one font set (as another computer or a browser would).
    fn context(embedded: bool) -> egui::Context {
        let ctx = egui::Context::default();
        apply(&ctx, embedded);
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        ctx
    }

    fn rich(bold: bool) -> RichText {
        let mut rt = RichText::plain(SAMPLE);
        rt.restyle(10, 40, |s: &mut SpanStyle| s.bold = bold);
        rt.restyle(41, 70, |s: &mut SpanStyle| s.italic = true);
        rt
    }

    #[test]
    fn two_independent_contexts_measure_and_wrap_identically_with_the_embedded_set() {
        let (a, b) = (EguiMeasurer(context(true)), EguiMeasurer(context(true)));
        for f in FAMILIES {
            for size in SIZES {
                for bold in [false, true] {
                    assert_eq!(a.width(SAMPLE, f, size, bold), b.width(SAMPLE, f, size, bold), "{f:?} {size} bold={bold}");
                    let rt = rich(bold);
                    for max_w in [60.0, 140.0, 333.0] {
                        let (la, lb) = (layout_rich(&rt, Some(max_w), &a, f, size), layout_rich(&rt, Some(max_w), &b, f, size));
                        assert_eq!(la.lines, lb.lines, "{f:?} {size} bold={bold} wrap {max_w}");
                        assert_eq!(la.widths, lb.widths);
                    }
                }
            }
        }
    }

    #[test]
    fn the_embedded_widths_are_pinned_so_that_every_platform_agrees() {
        let m = EguiMeasurer(context(true));
        // measured once with the files of `inkboard-fonts`; another value means other fonts, hence other line breaks
        for (f, bold, expected) in [
            (FontFamily::Sans, false, PINNED[0]),
            (FontFamily::Sans, true, PINNED[1]),
            (FontFamily::Serif, false, PINNED[2]),
            (FontFamily::Serif, true, PINNED[3]),
            (FontFamily::Hand, false, PINNED[4]),
            (FontFamily::Mono, false, PINNED[5]),
            (FontFamily::Mono, true, PINNED[6]),
        ] {
            let w = m.width("The quick brown fox", f, 20.0, bold);
            assert!((w - expected).abs() < 0.05, "{f:?} bold={bold}: {w} instead of {expected}");
        }
    }

    #[test]
    fn bold_is_real_for_sans_and_serif_and_the_handwriting_has_none() {
        let m = EguiMeasurer(context(true));
        let (t, size) = ("Whiteboard", 20.0);
        for f in [FontFamily::Sans, FontFamily::Serif] {
            assert!(m.width(t, f, size, true) > m.width(t, f, size, false) + 1.0, "{f:?}");
        }
        // Hack Bold has the advance of Hack Regular: bold changes the strokes, not the layout
        assert!((m.width(t, FontFamily::Mono, size, true) - m.width(t, FontFamily::Mono, size, false)).abs() < 0.01);
        assert_eq!(m.width(t, FontFamily::Hand, size, true), m.width(t, FontFamily::Hand, size, false));
        let narrow = m.width("iiiiiiiiii", FontFamily::Mono, size, false);
        assert!((narrow - m.width("WWWWWWWWWW", FontFamily::Mono, size, false)).abs() < 0.01, "monospace");
    }

    #[test]
    fn non_latin_and_emoji_text_falls_back_without_failing() {
        let m = EguiMeasurer(context(true));
        for f in FAMILIES {
            for text in ["日本語のテキスト", "Привет, мир", "Ελληνικά", "😀🎉 ✓ → ∑", "العربية", "a\u{0301}\u{200d}", "\u{10ffff}"]
            {
                let w = m.width(text, f, 16.0, false);
                assert!(w.is_finite() && w >= 0.0, "{f:?} {text:?}");
                let lay = layout_rich(&RichText::plain(text), Some(50.0), &m, f, 16.0);
                assert!(!lay.lines.is_empty() && lay.width.is_finite());
            }
            assert!(m.width("Привет", f, 16.0, false) > 0.0, "Cyrillic comes from the fallback font");
        }
    }

    #[test]
    fn switching_between_the_sets_keeps_every_family_usable() {
        let ctx = context(false);
        let m = EguiMeasurer(ctx.clone());
        let before = m.width(SAMPLE, FontFamily::Sans, 16.0, false);
        apply(&ctx, true);
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        let embedded = m.width(SAMPLE, FontFamily::Sans, 16.0, false);
        assert_eq!(embedded, EguiMeasurer(context(true)).width(SAMPLE, FontFamily::Sans, 16.0, false), "same as a fresh embedded context");
        apply(&ctx, false);
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        assert_eq!(m.width(SAMPLE, FontFamily::Sans, 16.0, false), before, "back to the system set");
        for f in FAMILIES {
            for bold in [false, true] {
                assert!(m.width("abc", f, 16.0, bold) > 0.0, "{f:?} bold={bold}");
            }
        }
    }

    /// Sans, sans bold, serif, serif bold, handwriting, mono, mono bold.
    const PINNED: [f64; 7] = [190.59375, 201.5625, 180.09375, 209.75, 161.03125, 228.03125, 228.03125];
}
