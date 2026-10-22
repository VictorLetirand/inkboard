//! Web stand-in of `watch.rs`: a browser has no file on disk to watch.

use crate::app::App;
use std::path::Path;
use std::time::SystemTime;

pub fn mtime(_p: &Path) -> Option<SystemTime> {
    None
}

impl App {
    pub fn watch_file(&mut self) {}
}
