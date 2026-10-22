//! Desktop side of [`crate::platform`].

use super::{Incoming, Pack};
use crate::app::RawImage;
use crate::i18n::t;
use eframe::egui;
use std::path::PathBuf;

pub use std::time::Instant;

/// The settings, the autosave and the shortcuts are files of the data folder.
pub const PERSISTS: bool = true;
/// "Save" and "Export" go through a file dialog and write the file (the web downloads it instead).
pub const DOWNLOADS: bool = false;

/// Folder of the settings and the safety copy (shared with the MCP server).
pub fn data_dir() -> PathBuf {
    inkboard_mcp::live::data_dir()
}

/// Nothing arrives late on the desktop.
pub fn poll() -> Vec<Incoming> {
    Vec::new()
}

/// The icon pack is part of the executable.
pub fn request_icon_pack() -> Pack {
    Pack::Ready(inkboard_icons::PACK)
}

/// The pack and its once-opened `Library` (resolves the built-in icons of an imported draw.io file) are shared with the MCP server.
pub fn shared_icons() -> Option<&'static inkboard_icons::Library> {
    inkboard_icons::shared()
}

/// The pack bytes when they are available without waiting.
pub fn icon_pack() -> Option<&'static [u8]> {
    Some(inkboard_icons::PACK)
}

/// Text from the system clipboard, if any.
pub fn system_text() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

/// Image from the system clipboard, if any (width, height, RGBA).
pub fn system_image() -> Option<RawImage> {
    let img = arboard::Clipboard::new().ok()?.get_image().ok()?;
    Some((img.width, img.height, img.bytes.into_owned()))
}

/// Puts an image (RGBA) in the system clipboard.
pub fn copy_image(width: usize, height: usize, rgba: Vec<u8>) -> Result<(), String> {
    let data = arboard::ImageData { width, height, bytes: rgba.into() };
    arboard::Clipboard::new().and_then(|mut c| c.set_image(data)).map_err(|e| e.to_string())
}

/// Asks for a board file to open.
pub fn pick_board() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter(t("All boards"), &["inkboard", "json", "drawio", "excalidraw", "canvas", "mmd", "mermaid", "xml"])
        .add_filter("Inkboard", &["inkboard", "json"])
        .pick_file()
}

/// Asks where to save a file named `name`; `label` and `exts` describe the file type.
pub fn pick_save_path(name: &str, label: &str, exts: &[&str]) -> Option<PathBuf> {
    rfd::FileDialog::new().set_file_name(name).add_filter(label, exts).save_file()
}

/// Asks for image files to insert.
pub fn pick_images() -> Vec<PathBuf> {
    rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "gif", "webp", "svg"]).pick_files().unwrap_or_default()
}

/// There is no download on the desktop (see [`DOWNLOADS`]).
pub fn download(_name: &str, _mime: &str, _bytes: &[u8]) -> Result<(), String> {
    Err("downloads are a web feature".to_owned())
}

/// The window title is a command to the window.
pub fn set_title(ctx: &egui::Context, title: &str) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.to_owned()));
}

pub fn set_fullscreen(ctx: &egui::Context, on: bool) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(on));
}

/// Relay addresses of an invitation link: what the network crate decides on the desktop.
pub use inkboard_net::config::{check_relay_url, is_known_relay};
