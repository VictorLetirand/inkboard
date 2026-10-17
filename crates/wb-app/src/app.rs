//! État de l'application, fichiers, raccourcis clavier et boucle principale.

use crate::fonts::{self, EguiMeasurer};
use crate::keymap::{Action, Keymap};
use crate::render::TextureCache;
use crate::tools::{Drag, Editing, Tool};
use crate::ui;
use eframe::egui::{self, Key, Modifiers};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use wb_core::editor::Order;
use wb_core::*;

pub enum Dialog {
    Mermaid {
        text: String,
        new_board: bool,
        error: Option<String>,
    },
    RenamePage {
        id: Id,
        name: String,
    },
    Shortcuts {
        filter: String,
        recording: Option<Action>,
    },
    /// Formule LaTeX : insertion, ou modification de l'objet `editing`.
    Formula {
        src: String,
        editing: Option<Id>,
        preview: FormulaPreview,
    },
}

/// Aperçu composé de la formule en cours de saisie (recalculé quand le source change).
#[derive(Default)]
pub struct FormulaPreview {
    pub src: Option<String>,
    pub tex: Option<egui::TextureHandle>,
    pub error: Option<String>,
}

/// Mesure de performance (`--bench`) : temps de construction d'une image, en millisecondes.
#[derive(Default)]
pub struct Bench {
    pub frame_ms: Vec<f64>,
    /// Durée d'une recherche d'objet sous le pointeur (survol), en millisecondes.
    pub pick_ms: Vec<f64>,
    /// Vue rapprochée (zoom 100 %) au lieu de « tout afficher ».
    pub close_up: bool,
}

/// Image brute : largeur, hauteur, pixels RGBA.
pub type RawImage = (usize, usize, Vec<u8>);

/// Caches recalculés quand la révision du document change : positions par identifiant et index spatial.
pub struct Caches {
    pub rev: u64,
    pub page: Id,
    pub len: usize,
    pub ids: std::collections::HashMap<Id, usize>,
    pub spatial: Option<wb_core::spatial::SpatialIndex>,
    /// Durée de la dernière reconstruction (ms).
    pub build_ms: f64,
}

impl Default for Caches {
    fn default() -> Self {
        Self { rev: u64::MAX, page: Id::new(), len: 0, ids: Default::default(), spatial: None, build_ms: 0.0 }
    }
}

/// Sous ce nombre d'objets, le parcours linéaire est plus simple et aussi rapide : pas d'index spatial.
const INDEX_MIN_OBJECTS: usize = 400;

pub struct App {
    pub caches: Caches,
    pub bench: Option<Bench>,
    pub keymap: Keymap,
    /// Dernière pression de stylet reçue (0..1) pendant un contact, si le matériel la fournit.
    pub pen_force: Option<f32>,
    /// Lecture d'une image du presse-papiers système (remplaçable dans les tests).
    pub system_image: fn() -> Option<RawImage>,
    /// Le stylet est-il utilisé côté gomme ? (extrémité gomme, via `wb-pen`) ; remplaçable dans les tests.
    pub eraser_probe: fn() -> bool,
    /// Diagnostic du stylet (`--pen-test`).
    pub pen_test: Option<crate::pen_test::PenReport>,
    /// Serveur local du mode direct de `wb-mcp` (absent dans les tests et les captures).
    pub live: Option<crate::live::LiveServer>,
    /// Référence des changements pour l'outil `board_changes` (ce que l'utilisateur a modifié depuis le dernier regard de l'agent).
    pub tracker: Option<wb_mcp::tools::Snapshot>,
    /// Date de modification du fichier ouvert lors de la dernière lecture ou écriture (voir `watch.rs`).
    pub disk_mtime: Option<std::time::SystemTime>,
    pub last_watch: Instant,
    /// Point monde du dernier clic droit (menu contextuel des tableaux).
    pub ctx_pos: Pt,
    pub keymap_path: Option<PathBuf>,
    pub ui_dark: bool,
    /// Le panneau de droite montre la liste des calques.
    pub layers_open: bool,
    /// Traînée du pointeur laser : positions écran et instants.
    pub laser: Vec<(egui::Pos2, Instant)>,
    /// Identifiant du menu principal (pour l'ouvrir dans les captures `--ui-demo menu`).
    pub menu_popup: Option<egui::Id>,
    /// Démonstration pour les captures : `menu`, `panel`, `dark` (séparés par des virgules).
    pub ui_demo: String,
    pub edit_demo: bool,
    /// Démonstration pour les captures : insère quelques formules composées.
    pub formula_demo: bool,
    pub ed: Editor,
    pub sel: Vec<Id>,
    pub tool: Tool,
    pub tool_lock: bool,
    pub drag: Option<Drag>,
    pub draft: Option<Object>,
    pub clipboard: Vec<Object>,
    pub clipboard_assets: Vec<(Id, Asset)>,
    pub path: Option<PathBuf>,
    pub saved_rev: u64,
    pub textures: TextureCache,
    pub measurer: EguiMeasurer,
    pub editing: Option<Editing>,
    /// Style appliqué aux nouveaux objets.
    pub style: Style,
    pub label: Label,
    pub route: Route,
    pub snap: bool,
    pub toast: Option<(String, Instant)>,
    pub dialog: Option<Dialog>,
    pub prop_edit: Option<Vec<Object>>,
    pub space_down: bool,
    pub screenshot: Option<PathBuf>,
    pub frames: u32,
    pub fit_pending: bool,
    /// Mode présentation : plein écran, sans interface, navigation page par page.
    pub presenting: bool,
    /// Demande d'entrée (`true`) ou de sortie (`false`) du mode présentation, traitée à la prochaine image.
    pub present_req: Option<bool>,
    /// Outil actif avant la présentation.
    pub prev_tool: Tool,
    /// Minuteur : instant de fin et durée totale.
    pub timer: Option<(Instant, Duration)>,
    pub last_autosave: Instant,
    pub last_title: String,
    pub paste_count: u32,
    pub canvas_size: (f32, f32),
    pub fit_selection_next: bool,
    pub pending_open: Option<PathBuf>,
    pub canvas_rect: egui::Rect,
}

fn autosave_path() -> PathBuf {
    wb_mcp::live::data_dir().join("autosave.tableau.json")
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, open: Option<PathBuf>, screenshot: Option<PathBuf>) -> Self {
        Self::build(cc, open, screenshot, true)
    }

    /// `restore` : recharge le dernier enregistrement automatique (désactivé dans les tests).
    pub fn build(cc: &eframe::CreationContext<'_>, open: Option<PathBuf>, screenshot: Option<PathBuf>, restore: bool) -> Self {
        fonts::install(&cc.egui_ctx);
        crate::theme::apply(&cc.egui_ctx, false);
        let measurer = EguiMeasurer(cc.egui_ctx.clone());
        let mut app = Self {
            caches: Caches::default(),
            bench: None,
            keymap: Keymap::default(),
            pen_force: None,
            system_image: crate::clipboard::system_image,
            eraser_probe: wb_pen::eraser_active,
            pen_test: None,
            live: None,
            tracker: None,
            disk_mtime: None,
            last_watch: Instant::now(),
            ctx_pos: Pt::ZERO,
            keymap_path: None,
            ui_dark: false,
            layers_open: false,
            laser: Vec::new(),
            menu_popup: None,
            ui_demo: String::new(),
            edit_demo: false,
            formula_demo: false,
            ed: Editor::new(Document::default()),
            sel: Vec::new(),
            tool: Tool::Select,
            tool_lock: false,
            drag: None,
            draft: None,
            clipboard: Vec::new(),
            clipboard_assets: Vec::new(),
            path: None,
            saved_rev: 0,
            textures: TextureCache::default(),
            measurer,
            editing: None,
            style: Style::default(),
            label: Label::default(),
            route: Route::Straight,
            snap: false,
            toast: None,
            dialog: None,
            prop_edit: None,
            space_down: false,
            screenshot,
            frames: 0,
            fit_pending: true,
            presenting: false,
            present_req: None,
            prev_tool: Tool::Select,
            timer: None,
            last_autosave: Instant::now(),
            last_title: String::new(),
            paste_count: 0,
            canvas_size: (1000.0, 700.0),
            fit_selection_next: false,
            pending_open: None,
            canvas_rect: egui::Rect::ZERO,
        };
        if restore {
            let kp = autosave_path().with_file_name("keymap.json");
            if let Ok(text) = std::fs::read_to_string(&kp) {
                match Keymap::from_json(&text) {
                    Ok((km, ignored)) => {
                        app.keymap = km;
                        if ignored > 0 {
                            app.toast(&format!("Raccourcis : {ignored} entrée(s) invalide(s) ignorée(s)"));
                        }
                    }
                    Err(e) => app.toast(&format!("Fichier de raccourcis ignoré : {e}")),
                }
            }
            app.keymap_path = Some(kp);
        }
        match open {
            // les polices n'existent qu'après la première image : l'ouverture est différée
            Some(p) => app.pending_open = Some(p),
            None if restore && app.screenshot.is_none() => {
                let a = autosave_path();
                if a.exists() && wb_core::serial::load(&a).map(|d| app.ed.reset(d)).is_ok() {
                    app.toast("Tableau précédent restauré");
                }
            }
            None => {}
        }
        app.saved_rev = app.ed.revision();
        app
    }

    /// Met à jour les caches si le document a changé depuis leur construction.
    pub fn refresh_caches(&mut self) {
        let page = self.ed.page();
        if self.caches.rev == self.ed.revision() && self.caches.page == page.id && self.caches.len == page.objects.len() {
            return;
        }
        let t0 = Instant::now();
        let ids = wb_core::spatial::id_index(page);
        let spatial = (page.objects.len() >= INDEX_MIN_OBJECTS).then(|| wb_core::spatial::SpatialIndex::build(page, &ids));
        self.caches =
            Caches { rev: self.ed.revision(), page: page.id.clone(), len: page.objects.len(), ids, spatial, build_ms: t0.elapsed().as_secs_f64() * 1000.0 };
    }

    /// Vrai pendant un geste qui modifie des objets sans passer par l'historique : l'index spatial est alors périmé.
    pub fn objects_moving(&self) -> bool {
        matches!(self.drag, Some(Drag::Move { .. } | Drag::Resize { .. } | Drag::Rotate { .. } | Drag::Endpoint { .. } | Drag::Via { .. }))
            || self.prop_edit.is_some()
    }

    /// Objet le plus au premier plan sous `w` (index spatial quand il est valable).
    pub fn pick_at(&mut self, w: Pt, tol: f64) -> Option<usize> {
        self.refresh_caches();
        let page = self.ed.page();
        let ids = &self.caches.ids;
        let get = |id: &Id| ids.get(id).map(|&i| &page.objects[i]);
        let cands = if self.objects_moving() { None } else { self.caches.spatial.as_ref().map(|s| s.query_point(w, tol)) };
        page.pick_with(w, tol, &get, cands.as_deref())
    }

    /// Objets d'un rectangle de sélection (lasso).
    pub fn rect_select(&mut self, r: Rect, contain: bool) -> Vec<Id> {
        self.refresh_caches();
        let page = self.ed.page();
        match (&self.caches.spatial, self.objects_moving()) {
            (Some(ix), false) => page.objects_in_rect_idx(r, contain, ix),
            _ => page.objects_in_rect(r, contain),
        }
    }

    pub fn toast(&mut self, msg: &str) {
        self.toast = Some((msg.to_owned(), Instant::now()));
    }

    pub fn dirty(&self) -> bool {
        self.ed.revision() != self.saved_rev
    }

    pub fn title(&self) -> String {
        let name = self.path.as_ref().and_then(|p| p.file_name()).and_then(|n| n.to_str()).map_or_else(|| self.ed.doc.name.clone(), str::to_owned);
        format!("{}{} — Tableau blanc", if self.dirty() { "● " } else { "" }, name)
    }

    pub fn new_document(&mut self) {
        self.ed.reset(Document::default());
        self.path = None;
        self.sel.clear();
        self.textures.clear();
        self.saved_rev = self.ed.revision();
        self.fit_pending = true;
        let _ = std::fs::remove_file(autosave_path());
    }

    pub fn open_path(&mut self, path: &Path) {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => return self.toast(&format!("Lecture impossible : {e}")),
        };
        match wb_formats::import_any(name, &content, &self.measurer) {
            Ok(imp) => {
                let native = wb_formats::detect(name, &content) == Some(wb_formats::Format::Native);
                let mut doc = imp.doc;
                if !native {
                    doc.name = path.file_stem().and_then(|n| n.to_str()).unwrap_or("Importé").to_owned();
                }
                self.ed.reset(doc);
                self.sel.clear();
                self.textures.clear();
                self.fit_pending = true;
                self.path = native.then(|| path.to_path_buf());
                self.disk_mtime = crate::watch::mtime(path);
                self.saved_rev = self.ed.revision();
                let note = imp.report.summary();
                self.toast(&if note.is_empty() { format!("« {name} » ouvert") } else { format!("« {name} » ouvert — {note}") });
            }
            Err(e) => self.toast(&format!("Ouverture impossible : {e}")),
        }
    }

    pub fn open_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Tous les tableaux", &["json", "drawio", "excalidraw", "canvas", "mmd", "mermaid", "xml"])
            .add_filter("Tableau blanc", &["json"])
            .pick_file()
        {
            self.open_path(&p);
        }
    }

    pub fn save(&mut self) {
        match self.path.clone() {
            Some(p) => self.save_to(&p),
            None => self.save_as(),
        }
    }

    pub fn save_as(&mut self) {
        let name = format!("{}.tableau.json", self.ed.doc.name);
        if let Some(p) = rfd::FileDialog::new().add_filter("Tableau blanc", &["json"]).set_file_name(&name).save_file() {
            let p = if p.to_string_lossy().ends_with(".tableau.json") { p } else { p.with_extension("tableau.json") };
            self.save_to(&p);
        }
    }

    pub fn save_to(&mut self, p: &Path) {
        self.ed.doc.prune_assets();
        match wb_core::serial::save(&self.ed.doc, p) {
            Ok(()) => {
                self.path = Some(p.to_path_buf());
                self.disk_mtime = crate::watch::mtime(p);
                self.saved_rev = self.ed.revision();
                self.toast("Enregistré");
            }
            Err(e) => self.toast(&format!("Enregistrement impossible : {e}")),
        }
    }

    /// Exporte vers un format d'échange ; `ext` : drawio, excalidraw, canvas, svg, png, pdf.
    pub fn export(&mut self, ext: &str) {
        let page = self.ed.page().clone();
        let ids: Option<Vec<Id>> = (!self.sel.is_empty()).then(|| self.sel.clone());
        let text: Option<String> = match ext {
            "drawio" => Some(wb_formats::drawio::export(&self.ed.doc, &self.measurer)),
            "excalidraw" => Some(wb_formats::excalidraw::export(&self.ed.doc, &page, &self.measurer)),
            "canvas" => {
                let (json, dropped) = wb_formats::jsoncanvas::export(&page);
                if dropped > 0 {
                    self.toast(&format!("JSON Canvas : {dropped} élément(s) non représentables ignorés"));
                }
                Some(json)
            }
            "svg" => wb_core::svg::export(&self.ed.doc, &page, ids.as_deref(), &self.measurer, &Default::default()),
            "png" | "pdf" => wb_core::svg::export(&self.ed.doc, &page, ids.as_deref(), &self.measurer, &Default::default()),
            _ => None,
        };
        let Some(text) = text else { return self.toast("Rien à exporter") };
        let name = format!("{}.{ext}", self.ed.doc.name);
        if let Some(p) = rfd::FileDialog::new().set_file_name(&name).add_filter(ext, &[ext]).save_file() {
            let bytes: Result<Vec<u8>, String> = match ext {
                "png" => wb_formats::raster::svg_to_png(&text, 2.0).map_err(|e| e.to_string()),
                "pdf" => wb_formats::raster::svg_to_pdf(&text).map_err(|e| e.to_string()),
                _ => Ok(text.into_bytes()),
            };
            match bytes.and_then(|b| std::fs::write(&p, b).map_err(|e| e.to_string())) {
                Ok(()) => self.toast(&format!("Exporté : {}", p.display())),
                Err(e) => self.toast(&format!("Export impossible : {e}")),
            }
        }
    }

    pub fn import_mermaid(&mut self, text: &str, new_board: bool) -> Result<(), String> {
        let objs = wb_formats::mermaid::to_objects(text, &self.measurer).map_err(|e| e.to_string())?;
        if new_board {
            let mut doc = Document::new("Mermaid");
            doc.pages[0].objects = objs;
            self.ed.reset(doc);
            self.path = None;
            self.sel.clear();
            self.fit_pending = true;
            self.saved_rev = self.ed.revision().wrapping_sub(1);
        } else {
            let b = Rect::from_points(objs.iter().flat_map(|o| {
                let r = Page::new("t").bounds(o);
                [Pt::new(r.x, r.y), Pt::new(r.right(), r.bottom())]
            }));
            let view = self.ed.page().view;
            let _ = view;
            let center = self.view_center();
            let (dx, dy) = b.map_or((0.0, 0.0), |r| (center.x - r.center().x, center.y - r.center().y));
            let moved: Vec<Object> = objs
                .into_iter()
                .map(|mut o| {
                    o.translate(Pt::new(dx, dy));
                    o
                })
                .collect();
            let ids = self.ed.insert_all(moved);
            self.sel = ids;
            self.fit_selection_next = true;
        }
        Ok(())
    }

    /// Centre de la vue en coordonnées monde (approximé avec la dernière taille connue du canvas).
    pub fn view_center(&self) -> Pt {
        let v = self.ed.page().view;
        let (cx, cy) = (f64::from(self.canvas_size.0) / 2.0, f64::from(self.canvas_size.1) / 2.0);
        Pt::new((cx - v.x) / v.zoom, (cy - v.y) / v.zoom)
    }

    /* --------------------------------------------------- Édition */

    pub fn copy_selection(&mut self) {
        let page = self.ed.page();
        self.clipboard = page.objects.iter().filter(|o| self.sel.contains(&o.id)).cloned().collect();
        self.clipboard_assets = self
            .clipboard
            .iter()
            .filter_map(|o| if let Kind::Image(i) = &o.kind { self.ed.doc.assets.get(&i.asset).map(|a| (i.asset.clone(), a.clone())) } else { None })
            .collect();
        self.paste_count = 0;
    }

    pub fn paste(&mut self) {
        if self.clipboard.is_empty() {
            return;
        }
        self.paste_count += 1;
        let d = 24.0 * f64::from(self.paste_count);
        let mut page = Page::new("clip");
        page.objects = self.clipboard.clone();
        let ids: Vec<Id> = page.objects.iter().map(|o| o.id.clone()).collect();
        let copies = Editor::clone_objects(&page, &ids, Pt::new(d, d));
        for (id, a) in &self.clipboard_assets {
            self.ed.doc.assets.entry(id.clone()).or_insert_with(|| a.clone());
        }
        self.sel = self.ed.insert_all(copies);
    }

    pub fn delete_selection(&mut self) {
        let ids: Vec<Id> = self.sel.iter().filter(|i| self.ed.page().get(i).is_some_and(|o| !self.ed.page().is_locked(o))).cloned().collect();
        if !ids.is_empty() {
            self.ed.remove(&ids);
        }
        self.sel.clear();
    }

    pub fn select_all(&mut self) {
        self.sel = self.ed.page().objects.iter().filter(|o| self.ed.page().is_visible(o)).map(|o| o.id.clone()).collect();
    }

    pub fn reorder(&mut self, m: Order) {
        let sel = self.sel.clone();
        self.ed.reorder(&sel, m);
    }

    pub fn set_tool(&mut self, t: Tool) {
        if self.editing.is_some() {
            self.commit_editing();
        }
        self.tool = t;
        if !matches!(t, Tool::Select | Tool::Hand) {
            self.sel.clear();
        }
        match t {
            Tool::Highlighter => {
                self.style.width = 18.0;
                self.style.opacity = 0.4;
                self.style.stroke = Color::rgb(0xf2, 0xc6, 0x00);
            }
            Tool::Pen if self.style.opacity < 1.0 && self.style.width >= 18.0 => {
                self.style.width = 3.0;
                self.style.opacity = 1.0;
                self.style.stroke = Color::Ink;
            }
            _ => {}
        }
    }

    pub fn fit_view(&mut self, only_selection: bool) {
        let page = self.ed.page();
        let b = if only_selection && !self.sel.is_empty() { page.bounds_of(&self.sel) } else { page.content_bounds() };
        let (w, h) = (f64::from(self.canvas_size.0.max(100.0)), f64::from(self.canvas_size.1.max(100.0)));
        let view = match b {
            Some(b) => {
                let z = ((w - 140.0) / b.w.max(1.0)).min((h - 140.0) / b.h.max(1.0)).clamp(0.05, 2.0);
                View { x: w / 2.0 - b.center().x * z, y: h / 2.0 - b.center().y * z, zoom: z }
            }
            None => View { x: w / 2.0, y: h / 2.0, zoom: 1.0 },
        };
        self.set_view(view);
    }

    pub fn set_view(&mut self, v: View) {
        let id = self.ed.current.clone();
        if let Some(p) = self.ed.doc.pages.iter_mut().find(|p| p.id == id) {
            p.view = v;
        }
    }

    fn recording_shortcut(&self) -> bool {
        matches!(self.dialog, Some(Dialog::Shortcuts { recording: Some(_), .. }))
    }

    /// Passe à la page suivante (`+1`) ou précédente (`-1`) ; sans effet aux extrémités.
    pub fn step_page(&mut self, d: i32) {
        let i = self.ed.doc.pages.iter().position(|p| p.id == self.ed.current).unwrap_or(0) as i32 + d;
        if let Some(p) = usize::try_from(i).ok().and_then(|i| self.ed.doc.pages.get(i)) {
            self.ed.current = p.id.clone();
            self.sel.clear();
            self.editing = None;
            self.fit_pending = true;
        }
    }

    fn apply_present_request(&mut self, ctx: &egui::Context) {
        let Some(on) = self.present_req.take() else { return };
        if on == self.presenting {
            return;
        }
        self.presenting = on;
        self.sel.clear();
        self.editing = None;
        self.drag = None;
        self.fit_pending = true;
        if on {
            self.prev_tool = self.tool;
            self.tool = Tool::Hand;
        } else {
            self.tool = self.prev_tool;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(on));
    }

    /// Touches du mode présentation : flèches / espace / pages pour naviguer, Échap ou F5 pour quitter.
    fn present_keys(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.key_pressed(Key::F)) {
            self.tool = if self.tool == Tool::Laser { Tool::Hand } else { Tool::Laser };
        }
        let (next, prev, quit) = ctx.input(|i| {
            (
                i.key_pressed(Key::ArrowRight) || i.key_pressed(Key::PageDown) || i.key_pressed(Key::Space) || i.key_pressed(Key::Enter),
                i.key_pressed(Key::ArrowLeft) || i.key_pressed(Key::PageUp) || i.key_pressed(Key::Backspace),
                i.key_pressed(Key::Escape) || i.key_pressed(Key::F5),
            )
        });
        if quit {
            self.present_req = Some(false);
        } else if next {
            self.step_page(1);
        } else if prev {
            self.step_page(-1);
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.presenting {
            return self.present_keys(ctx);
        }
        if ctx.wants_keyboard_input() || self.editing.is_some() || self.recording_shortcut() {
            return;
        }
        let events = ctx.input(|i| i.events.clone());
        for ev in events {
            match ev {
                egui::Event::Copy => self.copy_to_system(ctx),
                egui::Event::Cut => {
                    self.copy_to_system(ctx);
                    self.delete_selection();
                }
                egui::Event::Paste(text) => self.paste_event(&text),
                egui::Event::Key { key, pressed: true, modifiers, repeat: false, .. } => self.on_key(key, modifiers),
                _ => {}
            }
        }
        self.space_down = ctx.input(|i| i.key_down(Key::Space));
    }

    fn on_key(&mut self, key: Key, m: Modifiers) {
        if let Some(a) = self.keymap.action_for(key, m) {
            self.run_action(a);
        } else {
            match key {
                Key::Escape => {
                    self.sel.clear();
                    self.drag = None;
                    self.draft = None;
                    self.tool = Tool::Select;
                }
                Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown if !self.sel.is_empty() => {
                    let step = if m.shift { 10.0 } else { 1.0 };
                    let d = match key {
                        Key::ArrowLeft => Pt::new(-step, 0.0),
                        Key::ArrowRight => Pt::new(step, 0.0),
                        Key::ArrowUp => Pt::new(0.0, -step),
                        _ => Pt::new(0.0, step),
                    };
                    let sel: Vec<Id> = self.sel.iter().filter(|i| self.ed.page().get(i).is_some_and(|o| !self.ed.page().is_locked(o))).cloned().collect();
                    self.ed.translate(&sel, d);
                }
                _ => {}
            }
        }
        self.sel.retain(|id| self.ed.page().get(id).is_some());
    }

    /// Exécute une commande (raccourci clavier, menu ou bouton).
    pub fn run_action(&mut self, a: Action) {
        let sel = self.sel.clone();
        match a {
            Action::New => self.new_document(),
            Action::Open => self.open_dialog(),
            Action::Save => self.save(),
            Action::SaveAs => self.save_as(),
            Action::Undo => {
                self.ed.undo();
            }
            Action::Redo => {
                self.ed.redo();
            }
            Action::SelectAll => self.select_all(),
            Action::Duplicate => self.sel = self.ed.duplicate(&sel, Pt::new(24.0, 24.0)),
            Action::Delete => self.delete_selection(),
            Action::Group => self.ed.group(&sel),
            Action::Ungroup => self.ed.ungroup(&sel),
            Action::Front => self.reorder(Order::Front),
            Action::Forward => self.reorder(Order::Forward),
            Action::Backward => self.reorder(Order::Backward),
            Action::Back => self.reorder(Order::Back),
            Action::Zoom100 => {
                let v = self.ed.page().view;
                self.set_view(View { zoom: 1.0, ..v });
            }
            Action::ZoomFit => self.fit_view(false),
            Action::Present => self.present_req = Some(!self.presenting),
            Action::ZoomIn => self.zoom_by(1.25),
            Action::ZoomOut => self.zoom_by(0.8),
            Action::EditText => {
                if let [id] = sel.as_slice() {
                    self.start_editing(id, None);
                }
            }
            Action::Bold => self.apply_format(Format::Bold),
            Action::Italic => self.apply_format(Format::Italic),
            Action::Underline => self.apply_format(Format::Underline),
            Action::Strike => self.apply_format(Format::Strike),
            Action::ToolLock => self.tool_lock = !self.tool_lock,
            tool_action => {
                if let Some(t) = tool_action.tool() {
                    self.set_tool(t);
                }
            }
        }
        self.sel.retain(|id| self.ed.page().get(id).is_some());
    }

    /// Zoom centré sur le milieu du canvas.
    pub fn zoom_by(&mut self, k: f64) {
        let v = self.ed.page().view;
        let (cx, cy) = (f64::from(self.canvas_size.0) / 2.0, f64::from(self.canvas_size.1) / 2.0);
        let nz = (v.zoom * k).clamp(0.05, 8.0);
        self.set_view(View { x: cx - (cx - v.x) * nz / v.zoom, y: cy - (cy - v.y) * nz / v.zoom, zoom: nz });
    }

    /// Enregistre les raccourcis personnalisés (sans effet dans les tests : pas de fichier).
    pub fn save_keymap(&mut self) {
        let Some(p) = self.keymap_path.clone() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(&p, self.keymap.to_json()) {
            self.toast(&format!("Raccourcis non enregistrés : {e}"));
        }
    }

    fn autosave(&mut self) {
        if self.dirty() && self.last_autosave.elapsed() > Duration::from_secs(10) {
            let p = autosave_path();
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = wb_core::serial::save(&self.ed.doc, &p);
            self.last_autosave = Instant::now();
        }
    }

    fn bench_step(&mut self, ctx: &egui::Context, started: Instant) {
        if self.bench.is_some() {
            let v = self.ed.page().view;
            let t0 = Instant::now();
            for k in 0..20 {
                let w =
                    Pt::new((f64::from(self.canvas_size.0) * (0.2 + 0.03 * f64::from(k)) - v.x) / v.zoom, (f64::from(self.canvas_size.1) * 0.5 - v.y) / v.zoom);
                let _ = self.pick_at(w, 6.0 / v.zoom);
            }
            let per = t0.elapsed().as_secs_f64() * 1000.0 / 20.0;
            if let Some(b) = self.bench.as_mut() {
                b.pick_ms.push(per);
            }
        }
        let Some(b) = self.bench.as_mut() else { return };
        b.frame_ms.push(started.elapsed().as_secs_f64() * 1000.0);
        let n = b.frame_ms.len();
        let close_up = b.close_up;
        let _ = &b;
        let report = (n >= 126).then(|| {
            let mut t: Vec<f64> = b.frame_ms[6..].to_vec();
            t.sort_by(f64::total_cmp);
            (t.iter().sum::<f64>() / t.len() as f64, t[t.len() / 2], t[(t.len() * 95) / 100], t[t.len() - 1])
        });
        if n == 3 {
            self.fit_pending = true;
        }
        if n == 5 && close_up {
            // vue rapprochée : zoom 100 % centré sur le milieu du contenu
            if let Some(b) = self.ed.page().content_bounds() {
                let (w, h) = (f64::from(self.canvas_size.0), f64::from(self.canvas_size.1));
                self.set_view(View { x: w / 2.0 - b.center().x, y: h / 2.0 - b.center().y, zoom: 1.0 });
            }
        }
        if n > 6 {
            let v = self.ed.page().view;
            let k = if close_up {
                1.0
            } else if (n / 40) % 2 == 0 {
                1.01
            } else {
                0.99
            };
            self.set_view(View { x: v.x + 9.0, y: v.y + 5.0, zoom: (v.zoom * k).clamp(0.05, 4.0) });
        }
        ctx.request_repaint();
        if let Some((avg, med, p95, worst)) = report {
            println!("objets : {}", self.ed.page().objects.len());
            println!("image (CPU) : moyenne {avg:.2} ms · médiane {med:.2} ms · p95 {p95:.2} ms · pire {worst:.2} ms");
            println!("reconstruction des caches (une fois par modification) : {:.1} ms", self.caches.build_ms);
            let picks = self.bench.as_ref().map(|b| b.pick_ms.clone()).unwrap_or_default();
            if !picks.is_empty() {
                println!("survol (recherche sous le pointeur) : moyenne {:.3} ms", picks.iter().sum::<f64>() / picks.len() as f64);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn screenshot_step(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot.clone() else { return };
        self.frames += 1;
        if !self.ui_demo.is_empty() && self.frames == 3 {
            let demo = self.ui_demo.clone();
            if demo.contains("dark") {
                let mut bg = self.ed.doc.background;
                bg.preset = BgPreset::Slate;
                self.ed.set_background(bg);
            }
            if demo.contains("panel") {
                self.set_tool(Tool::Shape(BoxGeom::Rect));
                if let Some(id) = self.ed.page().objects.iter().find(|o| matches!(o.kind, Kind::Box(_))).map(|o| o.id.clone()) {
                    self.sel = vec![id];
                }
            }
        }
        if self.ui_demo.contains("menu")
            && self.frames == 6
            && let Some(id) = self.menu_popup
        {
            egui::Popup::open_id(ctx, id);
        }
        if self.formula_demo && self.frames == 3 {
            let demo = [
                r"\int_0^\infty e^{-x^2}\,dx = \frac{\sqrt{\pi}}{2}",
                r"\sum_{n=1}^{\infty} \frac{1}{n^2} = \frac{\pi^2}{6}",
                r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}^{-1} = \frac{1}{ad-bc}\begin{pmatrix} d & -b \\ -c & a \end{pmatrix}",
            ];
            for (i, f) in demo.iter().enumerate() {
                let before = self.ed.page().objects.len();
                let _ = self.insert_formula(f, None);
                if self.ed.page().objects.len() > before {
                    let id = self.ed.page().objects[before].id.clone();
                    self.ed.translate(&[id], Pt::new(0.0, (i as f64 - 1.0) * 110.0));
                }
            }
            self.sel.clear();
            self.fit_pending = true;
        }
        if self.edit_demo && self.frames == 3 {
            // démonstration pour les captures : ouvre l'édition du premier texte et met quelques mots en forme
            let first = self.ed.page().objects.iter().find(|o| matches!(&o.kind, Kind::Box(b) if !b.label.text.is_empty())).map(|o| o.id.clone());
            if let Some(id) = first {
                self.sel = vec![id.clone()];
                self.start_editing(&id, None);
                if let Some(e) = self.editing.as_mut() {
                    e.sel = (0, 2);
                }
                self.apply_format(wb_core::Format::Bold);
                if let Some(e) = self.editing.as_mut() {
                    e.sel = (2, 5);
                }
                self.apply_format(wb_core::Format::Highlight(Some(Color::rgb(0xff, 0xec, 0x99))));
                self.apply_format(wb_core::Format::Color(Some(Color::rgb(0xe0, 0x31, 0x31))));
            }
        }
        ctx.request_repaint();
        if self.frames == if self.ui_demo.contains("menu") { 18 } else { 8 } {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        let shot = ctx.input(|i| i.events.iter().find_map(|e| if let egui::Event::Screenshot { image, .. } = e { Some(image.clone()) } else { None }));
        if let Some(img) = shot {
            let (w, h) = (img.width() as u32, img.height() as u32);
            let raw: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
            if let Some(buf) = image::RgbaImage::from_raw(w, h, raw) {
                let _ = buf.save(&path);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let started = Instant::now();
        if let Some(p) = self.pending_open.take() {
            self.open_path(&p);
        }
        let dark = self.ed.doc.background.preset.is_dark();
        if dark != self.ui_dark {
            crate::theme::apply(ctx, dark);
            self.ui_dark = dark;
        }
        self.apply_present_request(ctx);
        self.watch_file();
        self.poll_live();
        self.shortcuts(ctx);
        if self.presenting {
            ui::present_overlay(self, ctx);
        } else {
            crate::chrome::show(self, ctx);
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| crate::tools::canvas(self, ui));
        ui::dialogs(self, ctx);
        if let Some(r) = self.pen_test.as_mut() {
            r.feed(wb_pen::last_sample(), self.pen_force);
            crate::pen_test::panel(r, ctx, self.pen_force);
        }
        ui::toast(self, ctx);
        let title = self.title();
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
        self.autosave();
        self.screenshot_step(ctx);
        if ctx.input(|i| i.viewport().close_requested()) && self.dirty() && self.screenshot.is_none() {
            // l'enregistrement automatique conserve le travail : on laisse fermer
            let _ = wb_core::serial::save(&self.ed.doc, &autosave_path());
        }
        ctx.request_repaint_after(Duration::from_millis(500));
        self.bench_step(ctx, started);
    }
}

#[allow(dead_code)]
fn _order_used(_: Order) {}
