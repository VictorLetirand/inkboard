//! Polices : chargement des polices système (Windows) et mesure de texte pour le moteur.

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily as EFamily, FontId};
use inkboard_core::FontFamily;
use inkboard_core::text::Measurer;
use std::sync::Arc;

const SANS: &str = "wb-sans";
const SANS_BOLD: &str = "wb-sans-bold";
const HAND: &str = "wb-hand";
const SERIF: &str = "wb-serif";
const MONO: &str = "wb-mono";

fn read_system_font(file: &str) -> Option<Vec<u8>> {
    let dirs: [&str; 3] = ["C:\\Windows\\Fonts", "/usr/share/fonts/truetype/dejavu", "/System/Library/Fonts/Supplemental"];
    dirs.iter().find_map(|d| std::fs::read(std::path::Path::new(d).join(file)).ok())
}

/// Installe les polices du tableau (Segoe UI, Segoe Print, Georgia, Consolas quand elles existent).
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
    if has_sans {
        defs.families.entry(EFamily::Proportional).or_default().insert(0, "segoeui".to_owned());
    }
    ctx.set_fonts(defs);
}

/// Police egui correspondant à une famille du modèle.
pub fn font_id(f: FontFamily, size: f32, bold: bool) -> FontId {
    let name = match (f, bold) {
        (FontFamily::Sans, true) => SANS_BOLD,
        (FontFamily::Sans, false) => SANS,
        (FontFamily::Hand, _) => HAND,
        (FontFamily::Serif, _) => SERIF,
        (FontFamily::Mono, _) => MONO,
    };
    FontId::new(size.max(1.0), EFamily::Name(name.into()))
}

/// Mesure exacte du texte avec les polices de l'application.
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
