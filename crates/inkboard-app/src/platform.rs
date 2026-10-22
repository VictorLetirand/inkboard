//! The seam between the desktop application and the browser build: everything that only exists on one of them lives behind this module
//! (`platform_native.rs` / `platform_web.rs`, same functions), so the rest of the application has no `cfg` of its own.
//!
//! Desktop: system clipboard, native file dialogs, files on disk, the embedded icon pack. Web: the browser clipboard events, file
//! pickers that return bytes, downloads, the icon pack fetched on demand. What a browser delivers later (a picked file, a pasted image)
//! is queued and handed to the application by [`poll`] once per frame.

#[cfg(not(target_arch = "wasm32"))]
#[path = "platform_native.rs"]
mod imp;
#[cfg(target_arch = "wasm32")]
#[path = "platform_web.rs"]
mod imp;

pub use imp::*;

/// Something the browser handed over after the fact (always empty on the desktop, whose dialogs and clipboard answer at once).
// the desktop never builds these variants (its answers are immediate)
#[allow(dead_code)]
#[derive(Debug)]
pub enum Incoming {
    /// A board file picked by the user: its name and its content.
    Board { name: String, bytes: Vec<u8> },
    /// Image files picked by the user: (lowercase extension, content).
    Images(Vec<(Option<String>, Vec<u8>)>),
    /// An image pasted from the clipboard: width, height, RGBA pixels.
    PastedImage(crate::app::RawImage),
}

/// State of the icon pack, which the web build downloads when the library is first opened.
#[allow(dead_code)]
#[derive(Debug)]
pub enum Pack {
    Ready(&'static [u8]),
    Loading,
    Failed(String),
}
