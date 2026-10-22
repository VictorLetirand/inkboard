//! Web stand-in of `share_ui.rs`: hosting a session needs a local network endpoint, which a browser does not have (the web client
//! joins sessions, it does not host them).

use crate::app::App;
use eframe::egui;

/// Nothing to hold: there is no host side in a browser.
#[derive(Debug, Default)]
pub struct Share;

impl Share {
    pub fn new(_load: bool) -> Self {
        Self
    }
}

pub fn stop(_app: &mut App) {}

pub fn tick(_app: &mut App, _ctx: &egui::Context) {}

pub fn show(_app: &mut App, _ctx: &egui::Context) {}

pub fn title_button(_app: &mut App, _ui: &mut egui::Ui) {}
