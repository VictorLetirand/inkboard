//! Diagnostic du stylet (`inkboard --pen-test`) : affiche en direct ce que Windows et egui reçoivent et produit un rapport à copier,
//! pour valider la pression et la gomme sur du matériel réel.

use eframe::egui;
use inkboard_pen::{PenSample, PointerKind};

/// Cumul des observations depuis le début (ou la dernière réinitialisation).
#[derive(Debug, Default)]
pub struct PenReport {
    pub pen_samples: u64,
    pub mouse_samples: u64,
    pub touch_samples: u64,
    pub min_pressure: Option<f32>,
    pub max_pressure: f32,
    /// Niveaux de pression distincts vus (quantifiés au centième) : un vrai stylet en produit des dizaines, une simple souris un seul.
    pub levels: std::collections::BTreeSet<u32>,
    pub eraser_seen: bool,
    pub barrel_seen: bool,
    pub tilt_seen: bool,
    /// Force reçue par egui (événements tactiles) : c'est elle qui fait varier l'épaisseur du trait dans l'application.
    pub egui_force_samples: u64,
    pub egui_max_force: f32,
    last_seq: u64,
}

impl PenReport {
    /// Intègre l'état courant : `sample` vu par Windows, `egui_force` vue par egui. Une mesure n'est comptée qu'une fois.
    pub fn feed(&mut self, sample: PenSample, egui_force: Option<f32>) {
        if let Some(f) = egui_force {
            self.egui_force_samples += 1;
            self.egui_max_force = self.egui_max_force.max(f);
        }
        if sample.seq == self.last_seq {
            return;
        }
        self.last_seq = sample.seq;
        match sample.kind {
            PointerKind::Pen => {
                self.pen_samples += 1;
                self.min_pressure = Some(self.min_pressure.map_or(sample.pressure, |m| m.min(sample.pressure)));
                self.max_pressure = self.max_pressure.max(sample.pressure);
                if sample.pressure > 0.0 {
                    self.levels.insert((sample.pressure * 100.0).round() as u32);
                }
                self.eraser_seen |= sample.eraser();
                self.barrel_seen |= sample.barrel();
                self.tilt_seen |= sample.tilt != (0, 0);
            }
            PointerKind::Mouse => self.mouse_samples += 1,
            PointerKind::Touch => self.touch_samples += 1,
            PointerKind::None => {}
        }
    }

    /// Verdicts lisibles (`Some(true)` OK, `Some(false)` NON, `None` quand l'action n'a pas encore été essayée).
    pub fn verdicts(&self) -> Vec<(String, Option<bool>)> {
        let pressed = self.pen_samples > 0;
        vec![
            ("Stylet détecté par Windows".into(), Some(pressed)),
            ("Pression variable (plusieurs niveaux)".into(), pressed.then_some(self.levels.len() >= 5)),
            ("Pression reçue par l'application (egui)".into(), pressed.then_some(self.egui_force_samples > 0)),
            ("Extrémité gomme détectée".into(), self.eraser_seen.then_some(true)),
            ("Bouton latéral détecté".into(), self.barrel_seen.then_some(true)),
            ("Inclinaison détectée".into(), self.tilt_seen.then_some(true)),
        ]
    }

    pub fn to_text(&self, label: &str) -> String {
        let mut s = format!("Rapport stylet — Inkboard {}\n{label}\n", env!("CARGO_PKG_VERSION"));
        for (name, v) in self.verdicts() {
            let word = match v {
                Some(true) => "oui",
                Some(false) => "NON",
                None => "non essayé",
            };
            s.push_str(&format!("- {name} : {word}\n"));
        }
        s.push_str(&format!(
            "Mesures : stylet {}, souris {}, tactile {} ; pression {:.2}–{:.2}, {} niveaux distincts ; egui : {} mesures de force (max {:.2})\n",
            self.pen_samples,
            self.mouse_samples,
            self.touch_samples,
            self.min_pressure.unwrap_or(0.0),
            self.max_pressure,
            self.levels.len(),
            self.egui_force_samples,
            self.egui_max_force
        ));
        s
    }
}

fn yes(b: bool) -> &'static str {
    if b { "oui" } else { "non" }
}

/// Panneau flottant : valeurs en direct, verdicts, boutons.
pub fn panel(report: &mut PenReport, ctx: &egui::Context, egui_force: Option<f32>) {
    let s = inkboard_pen::last_sample();
    egui::Window::new("Diagnostic du stylet").fade_in(false).anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, 48.0)).default_width(340.0).show(ctx, |ui| {
        ui.label("Dessinez avec le crayon (variez la pression), puis retournez le stylet pour effacer, puis appuyez sur le bouton latéral. Copiez le rapport.");
        ui.separator();
        let kind = match s.kind {
            PointerKind::Pen => "stylet",
            PointerKind::Mouse => "souris",
            PointerKind::Touch => "tactile",
            PointerKind::None => "aucun message reçu",
        };
        ui.label(format!("Dernier pointeur : {kind}"));
        ui.add(egui::ProgressBar::new(s.pressure).text(format!("pression Windows : {:.2}", s.pressure)));
        ui.label(format!("pression egui (force tactile) : {}", egui_force.map_or("—".to_owned(), |f| format!("{f:.2}"))));
        ui.label(format!("gomme : {}   bouton latéral : {}   inclinaison : {}°/{}°", yes(s.eraser()), yes(s.barrel()), s.tilt.0, s.tilt.1));
        ui.separator();
        for (name, v) in report.verdicts() {
            let (mark, color) = match v {
                Some(true) => ("OK ", egui::Color32::from_rgb(0x2f, 0x9e, 0x44)),
                Some(false) => ("NON", egui::Color32::from_rgb(0xe0, 0x31, 0x31)),
                None => ("—  ", egui::Color32::GRAY),
            };
            ui.horizontal(|ui| {
                ui.colored_label(color, mark);
                ui.label(name);
            });
        }
        ui.horizontal(|ui| {
            if ui.button("Copier le rapport").clicked() {
                ctx.copy_text(report.to_text("Windows (WM_POINTER) + egui"));
            }
            if ui.button("Réinitialiser").clicked() {
                *report = PenReport::default();
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkboard_pen::{PEN_FLAG_BARREL, PEN_FLAG_INVERTED};

    fn pen(p: f32, flags: u32, seq: u64) -> PenSample {
        PenSample { kind: PointerKind::Pen, pressure: p, flags, tilt: (0, 0), seq }
    }

    #[test]
    fn a_real_pen_session_is_recognized() {
        let mut r = PenReport::default();
        for i in 0..40u64 {
            let p = i as f32 / 40.0;
            r.feed(pen(p, 0, i + 1), Some(p));
        }
        r.feed(pen(0.3, PEN_FLAG_BARREL, 100), Some(0.3));
        r.feed(pen(0.3, PEN_FLAG_INVERTED, 101), None);
        let v: std::collections::HashMap<_, _> = r.verdicts().into_iter().collect();
        assert_eq!(v["Stylet détecté par Windows"], Some(true));
        assert_eq!(v["Pression variable (plusieurs niveaux)"], Some(true));
        assert_eq!(v["Pression reçue par l'application (egui)"], Some(true));
        assert_eq!(v["Extrémité gomme détectée"], Some(true));
        assert_eq!(v["Bouton latéral détecté"], Some(true));
        assert_eq!(v["Inclinaison détectée"], None, "non essayé");
        assert!(r.to_text("test").contains("Extrémité gomme détectée : oui"));
    }

    #[test]
    fn a_mouse_is_not_a_pen_and_a_binary_pressure_is_flagged() {
        let mut r = PenReport::default();
        r.feed(PenSample { kind: PointerKind::Mouse, seq: 1, ..PenSample::NONE }, None);
        assert_eq!(r.verdicts()[0].1, Some(false), "pas de stylet");
        // un « stylet » qui ne renvoie que 0 ou 1 : pression non variable
        for i in 0..30u64 {
            r.feed(pen(if i % 2 == 0 { 1.0 } else { 0.0 }, 0, i + 2), None);
        }
        let v: std::collections::HashMap<_, _> = r.verdicts().into_iter().collect();
        assert_eq!(v["Pression variable (plusieurs niveaux)"], Some(false));
        assert_eq!(v["Pression reçue par l'application (egui)"], Some(false), "Windows voit la pression mais egui non : le bug serait là");
    }

    #[test]
    fn the_same_sample_is_not_counted_twice() {
        let mut r = PenReport::default();
        let s = pen(0.5, 0, 7);
        r.feed(s, None);
        r.feed(s, None);
        assert_eq!(r.pen_samples, 1);
    }
}
