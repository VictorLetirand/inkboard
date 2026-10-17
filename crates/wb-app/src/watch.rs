//! Surveillance du fichier ouvert : un autre programme (par exemple un agent via `wb-mcp`) peut le modifier sur le disque.
//! Sans modification locale en cours, le tableau est rechargé ; sinon un avertissement est affiché (l'enregistrement écrasera le fichier).

use crate::app::App;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

pub fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

impl App {
    /// À appeler à chaque image ; ne touche au disque qu'une fois toutes les 400 ms.
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
            return self.toast("Le fichier a été modifié sur le disque : enregistrer écrasera ces modifications");
        }
        let Ok(content) = std::fs::read_to_string(&path) else { return };
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let Ok(imp) = wb_formats::import_any(name, &content, &self.measurer) else { return };
        // conserve la page affichée et les vues (zoom, position) des pages qui existent encore
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
        self.toast("Tableau mis à jour (modifié par un autre programme)");
    }
}
