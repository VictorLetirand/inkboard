//! Pen diagnostics (`inkboard --pen-test`): shows live what Windows and egui receive and produces a report to copy,
//! to validate pressure and the eraser on real hardware.

use crate::i18n::t;
use eframe::egui;
use inkboard_pen::{PenSample, PointerKind};

/// Observations accumulated since the start (or the last reset).
#[derive(Debug, Default)]
pub struct PenReport {
    pub pen_samples: u64,
    pub mouse_samples: u64,
    pub touch_samples: u64,
    pub min_pressure: Option<f32>,
    pub max_pressure: f32,
    /// Distinct pressure levels seen (quantized to hundredths): a real pen produces dozens, a plain mouse just one.
    pub levels: std::collections::BTreeSet<u32>,
    pub eraser_seen: bool,
    pub barrel_seen: bool,
    pub tilt_seen: bool,
    /// Force received by egui (touch events): this is what varies the stroke width in the application.
    pub egui_force_samples: u64,
    pub egui_max_force: f32,
    last_seq: u64,
}

impl PenReport {
    /// Integrates the current state: `sample` as seen by Windows, `egui_force` as seen by egui. A sample is counted only once.
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

    /// Readable verdicts (`Some(true)` OK, `Some(false)` NO, `None` when the action has not been tried yet).
    pub fn verdicts(&self) -> Vec<(String, Option<bool>)> {
        let pressed = self.pen_samples > 0;
        vec![
            ("Pen detected by Windows".into(), Some(pressed)),
            ("Variable pressure (several levels)".into(), pressed.then_some(self.levels.len() >= 5)),
            ("Pressure received by the application (egui)".into(), pressed.then_some(self.egui_force_samples > 0)),
            ("Eraser end detected".into(), self.eraser_seen.then_some(true)),
            ("Barrel button detected".into(), self.barrel_seen.then_some(true)),
            ("Tilt detected".into(), self.tilt_seen.then_some(true)),
        ]
    }

    pub fn to_text(&self, label: &str) -> String {
        let mut s = format!("Pen report — Inkboard {}\n{label}\n", env!("CARGO_PKG_VERSION"));
        for (name, v) in self.verdicts() {
            let word = match v {
                Some(true) => "yes",
                Some(false) => "NO",
                None => "not tried",
            };
            s.push_str(&format!("- {name}: {word}\n"));
        }
        s.push_str(&format!(
            "Samples: pen {}, mouse {}, touch {}; pressure {:.2}–{:.2}, {} distinct levels; egui: {} force samples (max {:.2})\n",
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

fn yes(b: bool) -> String {
    if b { t("yes") } else { t("no") }
}

/// Floating panel: live values, verdicts, buttons.
pub fn panel(report: &mut PenReport, ctx: &egui::Context, egui_force: Option<f32>) {
    let s = inkboard_pen::last_sample();
    egui::Window::new(t("Pen diagnostics")).fade_in(false).anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, 48.0)).default_width(340.0).show(ctx, |ui| {
        ui.label(t("Draw with the pen (vary the pressure), then flip the pen to erase, then press the barrel button. Copy the report."));
        ui.separator();
        let kind = match s.kind {
            PointerKind::Pen => t("pen"),
            PointerKind::Mouse => t("mouse"),
            PointerKind::Touch => t("touch"),
            PointerKind::None => t("no message received"),
        };
        ui.label(format!("{} {kind}", t("Last pointer:")));
        ui.add(egui::ProgressBar::new(s.pressure).text(format!("{} {:.2}", t("Windows pressure:"), s.pressure)));
        ui.label(format!("{} {}", t("egui pressure (touch force):"), egui_force.map_or("—".to_owned(), |f| format!("{f:.2}"))));
        ui.label(format!("{} {}   {} {}   {} {}°/{}°", t("eraser:"), yes(s.eraser()), t("barrel button:"), yes(s.barrel()), t("tilt:"), s.tilt.0, s.tilt.1));
        ui.separator();
        for (name, v) in report.verdicts() {
            let (mark, color) = match v {
                Some(true) => ("OK ", egui::Color32::from_rgb(0x2f, 0x9e, 0x44)),
                Some(false) => ("NO ", egui::Color32::from_rgb(0xe0, 0x31, 0x31)),
                None => ("—  ", egui::Color32::GRAY),
            };
            ui.horizontal(|ui| {
                ui.colored_label(color, mark);
                ui.label(t(&name));
            });
        }
        ui.horizontal(|ui| {
            if ui.button(t("Copy the report")).clicked() {
                ctx.copy_text(report.to_text("Windows (WM_POINTER) + egui"));
            }
            if ui.button(t("Reset")).clicked() {
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
        assert_eq!(v["Pen detected by Windows"], Some(true));
        assert_eq!(v["Variable pressure (several levels)"], Some(true));
        assert_eq!(v["Pressure received by the application (egui)"], Some(true));
        assert_eq!(v["Eraser end detected"], Some(true));
        assert_eq!(v["Barrel button detected"], Some(true));
        assert_eq!(v["Tilt detected"], None, "not tried");
        assert!(r.to_text("test").contains("Eraser end detected: yes"));
    }

    #[test]
    fn a_mouse_is_not_a_pen_and_a_binary_pressure_is_flagged() {
        let mut r = PenReport::default();
        r.feed(PenSample { kind: PointerKind::Mouse, seq: 1, ..PenSample::NONE }, None);
        assert_eq!(r.verdicts()[0].1, Some(false), "no pen");
        // a "pen" that only returns 0 or 1: non-variable pressure
        for i in 0..30u64 {
            r.feed(pen(if i % 2 == 0 { 1.0 } else { 0.0 }, 0, i + 2), None);
        }
        let v: std::collections::HashMap<_, _> = r.verdicts().into_iter().collect();
        assert_eq!(v["Variable pressure (several levels)"], Some(false));
        assert_eq!(v["Pressure received by the application (egui)"], Some(false), "Windows sees the pressure but egui does not: that would be the bug");
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
