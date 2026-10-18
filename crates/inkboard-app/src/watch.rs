//! Watching the open file: another program (for example an agent through `inkboard-mcp`) may modify it on disk.
//! Without pending local changes the board is reloaded; otherwise a warning is shown (saving will overwrite the file).

use crate::app::App;
use crate::i18n::t;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

pub fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

impl App {
    /// Call on every frame; only touches the disk every 400 ms.
    pub fn watch_file(&mut self) {
        if self.last_watch.elapsed() < Duration::from_millis(400) {
            return;
        }
        self.last_watch = Instant::now();
        let Some(path) = self.path.clone() else { return };
        let now = mtime(&path);
        if now.is_none() || now == self.disk_mtime {
            return;
        }
        self.disk_mtime = now;
        if self.dirty() {
            return self.toast(&t("The file was modified on disk: saving will overwrite those changes"));
        }
        let Ok(content) = std::fs::read_to_string(&path) else { return };
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let Ok(imp) = inkboard_formats::import_any(name, &content, &self.measurer) else { return };
        // keep the displayed page and the views (zoom, position) of the pages that still exist
        let (current, views): (_, Vec<_>) = (self.ed.current.clone(), self.ed.doc.pages.iter().map(|p| (p.id.clone(), p.view)).collect());
        let mut doc = imp.doc;
        for p in &mut doc.pages {
            if let Some((_, v)) = views.iter().find(|(id, _)| *id == p.id) {
                p.view = *v;
            }
        }
        let keep_page = doc.pages.iter().any(|p| p.id == current);
        self.ed.reset(doc);
        if keep_page {
            self.ed.current = current;
        }
        self.sel.clear();
        self.editing = None;
        self.textures.clear();
        self.saved_rev = self.ed.revision();
        self.toast(&t("Board updated (modified by another program)"));
    }
}
