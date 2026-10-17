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

use std::sync::atomic::{AtomicBool, Ordering};

static ERASER: AtomicBool = AtomicBool::new(false);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eraser_is_detected_from_pen_flags() {
        assert!(!eraser_from_flags(0));
        assert!(!eraser_from_flags(PEN_FLAG_BARREL), "le bouton latéral n'est pas la gomme");
        assert!(eraser_from_flags(PEN_FLAG_INVERTED));
        assert!(eraser_from_flags(PEN_FLAG_ERASER));
        assert!(eraser_from_flags(PEN_FLAG_BARREL | PEN_FLAG_INVERTED));
    }
}
