//! Gomme du stylet.
//!
//! winit et egui ne transmettent pas l'état des boutons / de l'extrémité « gomme » d'un stylet. Sous Windows, ces informations sont dans les
//! messages `WM_POINTER*` (indicateurs `PEN_FLAG_ERASER` / `PEN_FLAG_INVERTED` de `POINTER_PEN_INFO`). Ce crate installe un *subclass* de la
//! fenêtre qui lit ces messages sans les consommer et mémorise l'état ; l'application l'interroge avec [`eraser_active`].
//!
//! Sur les autres plateformes, [`install`] est sans effet et [`eraser_active`] renvoie toujours `false`.
//!
//! **Non validé sur du matériel réel** : la logique d'interprétation des indicateurs est testée, l'installation du *subclass* est testée sur une
//! fenêtre factice, mais aucun stylet physique n'était disponible.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

static ERASER: AtomicBool = AtomicBool::new(false);
static LAST: Mutex<PenSample> = Mutex::new(PenSample::NONE);

/// Type de pointeur vu en dernier.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PointerKind {
    #[default]
    None,
    Mouse,
    Touch,
    Pen,
}

/// Dernière mesure du pointeur, telle que la voit Windows (pour le diagnostic du stylet).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PenSample {
    pub kind: PointerKind,
    /// Pression de 0.0 à 1.0 (stylet uniquement).
    pub pressure: f32,
    /// Indicateurs `PEN_FLAG_*` (stylet uniquement).
    pub flags: u32,
    /// Inclinaison en degrés (stylet uniquement).
    pub tilt: (i32, i32),
    /// Nombre total de mesures reçues (permet de détecter qu'une nouvelle est arrivée).
    pub seq: u64,
}

impl PenSample {
    pub const NONE: PenSample = PenSample { kind: PointerKind::None, pressure: 0.0, flags: 0, tilt: (0, 0), seq: 0 };
    pub fn barrel(&self) -> bool {
        self.flags & PEN_FLAG_BARREL != 0
    }
    pub fn eraser(&self) -> bool {
        eraser_from_flags(self.flags)
    }
}

/// Dernière mesure reçue (`PenSample::NONE` tant qu'aucun message n'est arrivé).
pub fn last_sample() -> PenSample {
    *LAST.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Enregistre une mesure : met à jour l'état de la gomme et incrémente `seq` (usage interne et tests).
pub fn record(mut s: PenSample) {
    let mut g = LAST.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    s.seq = g.seq + 1;
    *g = s;
    ERASER.store(s.kind == PointerKind::Pen && s.eraser(), Ordering::Relaxed);
}

/// Indicateurs de stylet (valeurs de l'API Windows `PEN_FLAG_*`).
pub const PEN_FLAG_BARREL: u32 = 0x0000_0001;
pub const PEN_FLAG_INVERTED: u32 = 0x0000_0002;
pub const PEN_FLAG_ERASER: u32 = 0x0000_0004;

/// L'extrémité gomme est-elle utilisée d'après ces indicateurs ? (extrémité retournée, ou bouton « gomme » enfoncé)
pub fn eraser_from_flags(flags: u32) -> bool {
    flags & (PEN_FLAG_INVERTED | PEN_FLAG_ERASER) != 0
}

/// Vrai tant que le stylet approché ou posé est utilisé côté gomme.
pub fn eraser_active() -> bool {
    ERASER.load(Ordering::Relaxed)
}

/// Fixe l'état (usage interne et tests).
pub fn set_eraser(active: bool) {
    ERASER.store(active, Ordering::Relaxed);
}

#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use win::install;

/// Installe la détection sur la fenêtre `hwnd`. Sans effet hors Windows.
#[cfg(not(windows))]
pub fn install(_hwnd: std::num::NonZeroIsize) -> bool {
    false
}

/// Les tests partagent l'état global (dernier échantillon, gomme) : ils se sérialisent avec ce verrou.
#[cfg(test)]
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorded_samples_drive_the_eraser_state_and_sequence() {
        let _g = TEST_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        record(PenSample { kind: PointerKind::Pen, pressure: 0.5, flags: PEN_FLAG_INVERTED, tilt: (10, -5), seq: 0 });
        let a = last_sample();
        assert!(eraser_active() && a.eraser() && !a.barrel());
        assert_eq!((a.pressure, a.tilt), (0.5, (10, -5)));
        record(PenSample { kind: PointerKind::Mouse, ..PenSample::NONE });
        let b = last_sample();
        assert!(!eraser_active(), "la souris n'est jamais la gomme");
        assert_eq!(b.seq, a.seq + 1);
    }

    #[test]
    fn eraser_is_detected_from_pen_flags() {
        assert!(!eraser_from_flags(0));
        assert!(!eraser_from_flags(PEN_FLAG_BARREL), "le bouton latéral n'est pas la gomme");
        assert!(eraser_from_flags(PEN_FLAG_INVERTED));
        assert!(eraser_from_flags(PEN_FLAG_ERASER));
        assert!(eraser_from_flags(PEN_FLAG_BARREL | PEN_FLAG_INVERTED));
    }
}
