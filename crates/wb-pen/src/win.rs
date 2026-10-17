//! Implémentation Windows : subclass de fenêtre qui observe les messages `WM_POINTER*`.

use crate::{eraser_from_flags, set_eraser};
use std::num::NonZeroIsize;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Input::Pointer::{GetPointerPenInfo, GetPointerType, POINTER_PEN_INFO};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{PT_PEN, WM_POINTERDOWN, WM_POINTERLEAVE, WM_POINTERUP, WM_POINTERUPDATE};

const SUBCLASS_ID: usize = 0x5742_5045; // « WBPE »

/// Identifiant du pointeur : mot de poids faible de `wParam` (`GET_POINTERID_WPARAM`).
fn pointer_id(wparam: WPARAM) -> u32 {
    (wparam & 0xffff) as u32
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _id: usize, _data: usize) -> LRESULT {
    if matches!(msg, WM_POINTERDOWN | WM_POINTERUPDATE | WM_POINTERUP | WM_POINTERLEAVE) {
        let id = pointer_id(wparam);
        if msg == WM_POINTERLEAVE {
            set_eraser(false);
        } else {
            let mut kind = 0;
            // SAFETY : `kind` est un entier valide pour toute la durée de l'appel ; un identifiant invalide fait simplement échouer l'appel.
            let ok = unsafe { GetPointerType(id, &mut kind) } != 0;
            if ok && kind == PT_PEN {
                let mut info = POINTER_PEN_INFO::default();
                // SAFETY : structure de sortie valide pour la durée de l'appel.
                if unsafe { GetPointerPenInfo(id, &mut info) } != 0 {
                    set_eraser(eraser_from_flags(info.penFlags));
                }
            } else if ok {
                // souris ou doigt : plus de gomme
                set_eraser(false);
            }
        }
    }
    // les messages ne sont jamais consommés : winit/egui continuent de les traiter
    // SAFETY : transmission des arguments reçus tels quels à la procédure suivante de la chaîne.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// Installe l'observation des messages stylet sur la fenêtre. `hwnd` doit être le handle d'une fenêtre de **ce** processus et de ce thread
/// (obtenu via `raw-window-handle`) ; renvoie `false` si le système refuse.
pub fn install(hwnd: NonZeroIsize) -> bool {
    // SAFETY : `proc` a la signature `SUBCLASSPROC` attendue ; le handle est celui d'une fenêtre vivante appartenant au thread appelant.
    unsafe { SetWindowSubclass(hwnd.get() as HWND, Some(proc), SUBCLASS_ID, 0) != 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, SendMessageW, WS_OVERLAPPED};

    fn dummy_window() -> HWND {
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        // SAFETY : fenêtre « STATIC » (classe système) créée sans parent ; détruite à la fin du test.
        unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                WS_OVERLAPPED,
                0,
                0,
                10,
                10,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        }
    }

    #[test]
    fn subclass_installs_and_ignores_unknown_pointers_without_consuming_messages() {
        let hwnd = dummy_window();
        assert!(!hwnd.is_null(), "fenêtre factice créée");
        assert!(install(NonZeroIsize::new(hwnd as isize).unwrap()));
        set_eraser(true);
        // pointeur inexistant : l'état n'est pas modifié (GetPointerType échoue) et le message traverse sans erreur
        // SAFETY : fenêtre valide du thread courant.
        unsafe { SendMessageW(hwnd, WM_POINTERUPDATE, 0xffff, 0) };
        assert!(crate::eraser_active(), "un pointeur inconnu ne change pas l'état");
        // POINTERLEAVE efface l'état
        unsafe { SendMessageW(hwnd, WM_POINTERLEAVE, 0xffff, 0) };
        assert!(!crate::eraser_active());
        unsafe { DestroyWindow(hwnd) };
    }
}
