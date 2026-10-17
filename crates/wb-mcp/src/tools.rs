//! Outils MCP : chaque outil ouvre le fichier `path`, applique une modification par l'`Editor` du moteur et réenregistre (atomiquement).

use crate::Server;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use wb_core::layout::{Direction, auto_layout};
use wb_core::svg::{SvgOptions, export as export_svg};
use wb_core::*;

type Res<T> = Result<T, String>;

/// Extensions de fichiers que les outils acceptent d'écrire.
const EXPORTS: [&str; 8] = [".tableau.json", ".drawio", ".excalidraw", ".canvas", ".svg", ".png", ".pdf", ".json"];

fn obj(props: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": props, "required": required})
}

fn path_prop() -> Value {
    json!({"type": "string", "description": "Chemin du fichier tableau (.tableau.json). Facultatif quand l'application est ouverte : l'outil agit alors en direct sur le tableau affiché (annulable avec Ctrl+Z)."})
}

fn page_prop() -> Value {
    json!({"description": "Page : numéro (à partir de 1) ou nom. Par défaut, la première.", "oneOf": [{"type": "integer"}, {"type": "string"}]})
}

fn pos_props(mut p: Value) -> Value {
    let m = p.as_object_mut().expect("objet");
    m.insert("path".into(), path_prop());
    m.insert("page".into(), page_prop());
    m.insert("x".into(), json!({"type": "number", "description": "Abscisse du coin supérieur gauche. Par défaut : sous le contenu existant."}));
    m.insert("y".into(), json!({"type": "number", "description": "Ordonnée du coin supérieur gauche."}));
    p
}

/// Description des outils (nom, description, schéma d'entrée) pour `tools/list`.
pub fn list() -> Vec<Value> {
    let color = json!({"type": "string", "description": "Couleur « #rrggbb » ou « ink » (couleur du texte du thème)."});
    vec![
        json!({"name": "create_board", "description": "Crée un tableau vide (fichier .tableau.json).",
            "inputSchema": obj(json!({"path": path_prop(), "name": {"type": "string"}}), &["path"])}),
        json!({"name": "save_board", "description": "Enregistre le tableau affiché dans l'application (mode direct ; il doit déjà avoir un fichier). Sans effet en mode fichier : chaque outil y enregistre déjà.",
            "inputSchema": obj(json!({}), &[])}),
        json!({"name": "describe_board", "description": "Liste les pages et les objets d'un tableau (id, type, texte, position, liens des connecteurs). En mode direct, indique aussi la sélection de l'utilisateur (`selection`). Accepte aussi les fichiers draw.io, Excalidraw, JSON Canvas et Mermaid en lecture.",
            "inputSchema": obj(json!({"path": path_prop(), "page": page_prop()}), &[])}),
        json!({"name": "board_changes", "description": "Signale ce que l'UTILISATEUR a modifié dans le tableau depuis votre dernier regard (describe_board ou board_changes) : objets ajoutés, modifiés, supprimés. Vos propres modifications ne sont pas signalées. Le premier appel enregistre une référence.",
            "inputSchema": obj(json!({"path": path_prop()}), &[])}),
        json!({"name": "add_image", "description": "Ajoute une image (PNG, JPEG, GIF, WebP ou SVG, 15 Mio au plus) à partir d'un fichier (`source`, sous la racine autorisée) ou de données base64 (`data_base64`). La taille naturelle est réduite à 600 au plus ; w et/ou h la remplacent.",
            "inputSchema": obj(pos_props(json!({"source": {"type": "string", "description": "Chemin du fichier image."}, "data_base64": {"type": "string"}, "w": {"type": "number"}, "h": {"type": "number"}})), &[])}),
        json!({"name": "add_shape", "description": "Ajoute une forme avec du texte (rect, ellipse, diamond, triangle, sticky = note adhésive, text = texte libre). Renvoie l'id.",
            "inputSchema": obj(pos_props(json!({
                "geom": {"type": "string", "enum": ["rect", "ellipse", "diamond", "triangle", "sticky", "text"]},
                "text": {"type": "string"}, "w": {"type": "number"}, "h": {"type": "number"},
                "font_size": {"type": "number"}, "fill": color.clone(), "stroke": color.clone()})), &["geom"])}),
        json!({"name": "add_connector", "description": "Relie deux objets par une flèche qui les suit quand ils bougent (ids renvoyés par add_shape ou describe_board).",
            "inputSchema": obj(json!({"path": path_prop(), "page": page_prop(), "from": {"type": "string"}, "to": {"type": "string"},
                "route": {"type": "string", "enum": ["straight", "elbow", "curve"]},
                "arrow": {"type": "string", "enum": ["end", "start", "both", "none"], "description": "Pointes de flèche (défaut : end)."},
                "label": {"type": "string"}, "stroke": color.clone()}), &["from", "to"])}),
        json!({"name": "add_table", "description": "Ajoute un tableau de cellules ; `rows` est une liste de lignes (listes de textes). La première ligne est l'en-tête.",
            "inputSchema": obj(pos_props(json!({"rows": {"type": "array", "items": {"type": "array", "items": {"type": "string"}}}})), &["rows"])}),
        json!({"name": "add_code_block", "description": "Ajoute un bloc de code (police à chasse fixe, fond sombre, coloration syntaxique).",
            "inputSchema": obj(pos_props(json!({"code": {"type": "string"}})), &["code"])}),
        json!({"name": "add_formula", "description": "Ajoute une formule LaTeX (mode mathématique) composée comme en TeX. Erreur si le LaTeX n'est pas géré.",
            "inputSchema": obj(pos_props(json!({"latex": {"type": "string"}, "color": color.clone()})), &["latex"])}),
        json!({"name": "import_mermaid", "description": "Ajoute un diagramme Mermaid (flowchart/graph, stateDiagram, sequenceDiagram) avec mise en page automatique. Renvoie les ids créés.",
            "inputSchema": obj(pos_props(json!({"mermaid": {"type": "string"}, "new_page": {"type": "boolean", "description": "Créer une nouvelle page pour le diagramme."}})), &["mermaid"])}),
        json!({"name": "update_object", "description": "Modifie un objet : texte, position (coin supérieur gauche), taille (formes), couleurs.",
            "inputSchema": obj(json!({"path": path_prop(), "page": page_prop(), "id": {"type": "string"}, "text": {"type": "string"},
                "x": {"type": "number"}, "y": {"type": "number"}, "w": {"type": "number"}, "h": {"type": "number"},
                "fill": color.clone(), "stroke": color.clone()}), &["id"])}),
        json!({"name": "delete_object", "description": "Supprime un objet (les connecteurs qui y étaient liés restent, détachés).",
            "inputSchema": obj(json!({"path": path_prop(), "page": page_prop(), "id": {"type": "string"}}), &["id"])}),
        json!({"name": "auto_layout", "description": "Réorganise les formes reliées par des connecteurs en graphe en couches.",
            "inputSchema": obj(json!({"path": path_prop(), "page": page_prop(), "direction": {"type": "string", "enum": ["TB", "LR", "BT", "RL"], "description": "Défaut : TB."}}), &[])}),
        json!({"name": "add_page", "description": "Ajoute une page. Renvoie son numéro.",
            "inputSchema": obj(json!({"path": path_prop(), "name": {"type": "string"}}), &[])}),
        json!({"name": "export", "description": "Exporte une page vers un fichier ; le format est choisi par l'extension de `output` : .svg, .png, .pdf, .drawio, .excalidraw, .canvas, .tableau.json.",
            "inputSchema": obj(json!({"path": path_prop(), "output": {"type": "string"}, "page": page_prop()}), &["output"])}),
    ]
}

/* ------------------------------------------------------------ arguments */

fn s<'a>(a: &'a Value, k: &str) -> Res<&'a str> {
    a.get(k).and_then(Value::as_str).ok_or_else(|| format!("paramètre texte « {k} » manquant"))
}
fn os<'a>(a: &'a Value, k: &str) -> Option<&'a str> {
    a.get(k).and_then(Value::as_str)
}
fn of(a: &Value, k: &str) -> Option<f64> {
    a.get(k).and_then(Value::as_f64)
}
fn ocolor(a: &Value, k: &str) -> Res<Option<Color>> {
    match os(a, k) {
        None => Ok(None),
        Some(c) => Color::parse(c).map(Some).ok_or_else(|| format!("couleur invalide « {c} » (attendu #rrggbb ou ink)")),
    }
}

/// Résout un chemin, confiné à la racine du serveur si elle est définie (les liens et `..` sont neutralisés par canonicalisation du dossier parent).
fn resolve(srv: &Server, p: &str) -> Res<PathBuf> {
    let path = PathBuf::from(p);
    let Some(root) = &srv.root else { return Ok(path) };
    let root = root.canonicalize().map_err(|e| format!("racine inaccessible : {e}"))?;
    let abs = if path.is_absolute() { path } else { root.join(path) };
    let parent = abs.parent().ok_or("chemin invalide")?;
    let parent = parent.canonicalize().map_err(|e| format!("dossier inaccessible ({}) : {e}", parent.display()))?;
    if !parent.starts_with(&root) {
        return Err(format!("chemin hors de la racine autorisée ({})", root.display()));
    }
    Ok(parent.join(abs.file_name().ok_or("chemin invalide")?))
}

fn lower_name(p: &Path) -> String {
    p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase()
}

fn load(path: &Path) -> Res<Document> {
    let content = std::fs::read_to_string(path).map_err(|e| format!("lecture de {} : {e}", path.display()))?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    wb_formats::import_any(name, &content, &ApproxMeasurer).map(|i| i.doc).map_err(|e| format!("ouverture de {name} : {e}"))
}

/// Enregistrement : uniquement le format natif, pour ne jamais écraser un fichier d'un autre outil avec un autre contenu.
fn save(doc: &Document, path: &Path) -> Res<()> {
    if !lower_name(path).ends_with(".tableau.json") {
        return Err("les modifications s'enregistrent dans un fichier .tableau.json (utilisez `export` pour les autres formats)".into());
    }
    wb_core::serial::save(doc, path).map_err(|e| format!("écriture de {} : {e}", path.display()))
}

fn select_page(ed: &mut Editor, spec: Option<&Value>) -> Res<()> {
    let Some(spec) = spec else { return Ok(()) };
    let pages = &ed.doc.pages;
    let found = match spec {
        Value::Number(n) => n.as_u64().and_then(|n| usize::try_from(n).ok()).and_then(|n| n.checked_sub(1)).and_then(|i| pages.get(i)),
        Value::String(name) => pages.iter().find(|p| &p.name == name),
        _ => None,
    };
    let id = found.ok_or_else(|| format!("page introuvable ({spec}) ; le tableau a {} page(s)", pages.len()))?.id.clone();
    ed.current = id;
    Ok(())
}

fn origin(ed: &Editor, a: &Value) -> Pt {
    match (of(a, "x"), of(a, "y")) {
        (Some(x), Some(y)) => Pt::new(x, y),
        _ => ed.page().content_bounds().map_or(Pt::new(0.0, 0.0), |b| Pt::new(of(a, "x").unwrap_or(b.x), of(a, "y").unwrap_or(b.bottom() + 40.0))),
    }
}

fn find<'a>(ed: &'a Editor, id: &str) -> Res<&'a Object> {
    ed.page().get(&Id(id.to_owned())).ok_or_else(|| format!("objet introuvable : {id}"))
}

/* ----------------------------------------------------------------- appel */

/// Outils qui ne modifient pas le tableau (le fichier n'est pas réécrit).
const READ_ONLY: [&str; 3] = ["describe_board", "export", "board_changes"];

/// Outils qui s'appliquent à un tableau ouvert (fichier ou application) ; `create_board` ne crée que des fichiers.
const EDITOR_TOOLS: [&str; 14] = [
    "describe_board",
    "board_changes",
    "add_image",
    "add_shape",
    "add_connector",
    "add_table",
    "add_code_block",
    "add_formula",
    "import_mermaid",
    "update_object",
    "delete_object",
    "auto_layout",
    "add_page",
    "export",
];

/// Applique un outil à un tableau en mémoire. Utilisé tel quel par le mode fichier et par l'application (mode direct, avec son historique d'annulation).
pub fn apply(ed: &mut Editor, name: &str, a: &Value) -> Res<Value> {
    if name == "save_board" {
        return Ok(json!({"saved": true}));
    }
    if !EDITOR_TOOLS.contains(&name) {
        return Err(format!("outil inconnu : {name}"));
    }
    if name != "add_page" {
        select_page(ed, a.get("page"))?;
    }
    match name {
        "describe_board" => describe(ed, a),
        "board_changes" => Err("board_changes n'a de sens qu'avec le suivi des changements (apply_tracked)".into()),
        "add_image" => add_image(ed, a),
        "add_shape" => add_shape(ed, a),
        "add_connector" => add_connector(ed, a),
        "add_table" => add_table(ed, a),
        "add_code_block" => add_code_block(ed, a),
        "add_formula" => add_formula(ed, a),
        "import_mermaid" => import_mermaid(ed, a),
        "update_object" => update_object(ed, a),
        "delete_object" => {
            let id = find(ed, s(a, "id")?)?.id.clone();
            ed.remove(std::slice::from_ref(&id));
            Ok(json!({"deleted": id.0}))
        }
        "auto_layout" => {
            let dir = match os(a, "direction").unwrap_or("TB") {
                "TB" => Direction::TopDown,
                "LR" => Direction::LeftRight,
                "BT" => Direction::BottomUp,
                "RL" => Direction::RightLeft,
                d => return Err(format!("direction inconnue « {d} » (TB, LR, BT, RL)")),
            };
            let ids: Vec<Id> = ed.page().objects.iter().map(|o| o.id.clone()).collect();
            auto_layout(ed, &ids, dir);
            Ok(json!({"layout": "ok", "objects": ids.len()}))
        }
        "add_page" => {
            let n = ed.doc.pages.len() + 1;
            ed.add_page(os(a, "name").map_or_else(|| format!("Page {n}"), str::to_owned).as_str());
            Ok(json!({"page": n, "pages": n}))
        }
        "export" => export(ed, a),
        _ => unreachable!("outil vérifié ci-dessus"),
    }
}

/// Exécute l'outil `name` : dans l'application ouverte si elle est joignable et affiche ce tableau (mode direct), sinon sur le fichier `path`.
/// Le résultat est un texte JSON destiné à l'agent.
pub fn call(srv: &Server, name: &str, a: &Value) -> Res<String> {
    if name != "create_board" && name != "save_board" && !EDITOR_TOOLS.contains(&name) {
        return Err(format!("outil inconnu : {name}"));
    }
    let mut a = a.clone();
    // les chemins sont résolus (et confinés à la racine) ici, jamais dans l'application
    for key in ["path", "output"] {
        if let Some(p) = os(&a, key) {
            let abs = resolve(srv, p)?;
            a[key] = json!(abs.display().to_string());
        }
    }
    if name == "add_image"
        && let Some(src) = os(&a, "source").map(str::to_owned)
    {
        // le fichier est lu ici (confiné à la racine) ; seules les données sont transmises à l'application
        let abs = resolve(srv, &src)?;
        let meta = std::fs::metadata(&abs).map_err(|e| format!("lecture de {} : {e}", abs.display()))?;
        if meta.len() as usize > IMAGE_MAX_BYTES {
            return Err(format!("image trop volumineuse ({} Mio, maximum {})", meta.len() >> 20, IMAGE_MAX_BYTES >> 20));
        }
        use base64::Engine;
        let bytes = std::fs::read(&abs).map_err(|e| format!("lecture de {} : {e}", abs.display()))?;
        a["data_base64"] = json!(base64::engine::general_purpose::STANDARD.encode(bytes));
        a.as_object_mut().map(|m| m.remove("source"));
    }
    if name != "create_board" {
        match crate::live::forward(srv, name, &a) {
            crate::live::Forward::Done(r) => return r.map(|v| v.to_string()),
            crate::live::Forward::NotLive => {}
        }
    }
    if name == "create_board" {
        let path = PathBuf::from(s(&a, "path")?);
        if path.exists() {
            return Err(format!("{} existe déjà", path.display()));
        }
        let doc = Document::new(os(&a, "name").unwrap_or("Tableau"));
        save(&doc, &path)?;
        return Ok(json!({"path": path.display().to_string(), "pages": 1}).to_string());
    }
    let Some(path) = os(&a, "path").map(PathBuf::from) else {
        return Err(
            "paramètre « path » manquant : aucun tableau n'est ouvert dans l'application (ou elle est fermée) ; indiquez le fichier .tableau.json".into()
        );
    };
    let mut ed = Editor::new(load(&path)?);
    let mut tracker = srv.trackers.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&path);
    let r = apply_tracked(&mut ed, name, &a, &mut tracker);
    if let Some(t) = tracker {
        srv.trackers.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(path.clone(), t);
    }
    let r = r?;
    if !READ_ONLY.contains(&name) && name != "save_board" {
        save(&ed.doc, &path)?;
    }
    Ok(r.to_string())
}

fn geom_of(g: &str) -> Res<BoxGeom> {
    Ok(match g {
        "rect" => BoxGeom::Rect,
        "ellipse" => BoxGeom::Ellipse,
        "diamond" => BoxGeom::Diamond,
        "triangle" => BoxGeom::Triangle,
        "sticky" => BoxGeom::Sticky,
        "text" => BoxGeom::Text,
        other => return Err(format!("forme inconnue « {other} » (rect, ellipse, diamond, triangle, sticky, text)")),
    })
}

fn kind_name(o: &Object) -> &'static str {
    match &o.kind {
        Kind::Box(b) => match b.geom {
            BoxGeom::Rect => "rect",
            BoxGeom::Ellipse => "ellipse",
            BoxGeom::Diamond => "diamond",
            BoxGeom::Triangle => "triangle",
            BoxGeom::Sticky => "sticky",
            BoxGeom::Text => "text",
        },
        Kind::Pen(_) => "pen",
        Kind::Connector(_) => "connector",
        Kind::Image(i) if i.latex.is_some() => "formula",
        Kind::Image(_) => "image",
        Kind::Table(_) => "table",
    }
}

fn end_json(e: &connector::End) -> Value {
    match e {
        connector::End::Bound { target, .. } => json!({"object": target.0}),
        connector::End::Free { at } => json!({"x": at.x, "y": at.y}),
    }
}

/// Résumé JSON d'un objet (id, type, position, texte, liens).
pub fn summary(o: &Object) -> Value {
    let mut v = json!({"id": o.id.0, "type": kind_name(o)});
    let m = v.as_object_mut().expect("objet");
    if let Some(f) = o.frame() {
        m.insert("x".into(), json!(f.x.round()));
        m.insert("y".into(), json!(f.y.round()));
        m.insert("w".into(), json!(f.w.round()));
        m.insert("h".into(), json!(f.h.round()));
    }
    match &o.kind {
        Kind::Box(b) => {
            m.insert("text".into(), json!(b.label.text.to_plain()));
        }
        Kind::Connector(c) => {
            m.insert("from".into(), end_json(&c.start));
            m.insert("to".into(), end_json(&c.end));
            if let Some(l) = &c.label {
                m.insert("label".into(), json!(l.text.to_plain()));
            }
        }
        Kind::Table(t) => {
            m.insert("rows".into(), json!(t.rows()));
            m.insert("cols".into(), json!(t.cols()));
            let r = t.rect();
            m.insert("x".into(), json!(r.x.round()));
            m.insert("y".into(), json!(r.y.round()));
            m.insert("w".into(), json!(r.w.round()));
            m.insert("h".into(), json!(r.h.round()));
        }
        Kind::Image(i) => {
            if let Some(l) = &i.latex {
                m.insert("latex".into(), json!(l));
            }
        }
        Kind::Pen(_) => {}
    }
    v
}

fn describe(ed: &mut Editor, _a: &Value) -> Res<Value> {
    let page = ed.page();
    let objects: Vec<Value> = page.objects.iter().map(summary).collect();
    let pages: Vec<Value> = ed.doc.pages.iter().enumerate().map(|(i, p)| json!({"number": i + 1, "name": p.name, "objects": p.objects.len()})).collect();
    Ok(json!({"name": ed.doc.name, "pages": pages, "page": {"name": page.name, "objects": objects}}))
}

/* ------------------------------------------------- changements faits par l'utilisateur */

/// Empreinte du contenu d'un tableau : une valeur par objet, pour détecter ce qui a été ajouté, supprimé ou modifié.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    objects: std::collections::HashMap<String, u64>,
    pages: Vec<(String, String)>,
}

/// Au-delà, le suivi des changements est désactivé (le coût est linéaire à chaque appel d'outil).
const TRACK_MAX_OBJECTS: usize = 20_000;

impl Snapshot {
    pub fn of(ed: &Editor) -> Self {
        use std::hash::{Hash, Hasher};
        let mut objects = std::collections::HashMap::new();
        for p in &ed.doc.pages {
            for o in &p.objects {
                let mut h = std::collections::hash_map::DefaultHasher::new();
                serde_json::to_string(o).unwrap_or_default().hash(&mut h);
                objects.insert(o.id.0.clone(), h.finish());
            }
        }
        Snapshot { objects, pages: ed.doc.pages.iter().map(|p| (p.id.0.clone(), p.name.clone())).collect() }
    }

    fn too_big(ed: &Editor) -> bool {
        ed.doc.pages.iter().map(|p| p.objects.len()).sum::<usize>() > TRACK_MAX_OBJECTS
    }
}

fn find_anywhere<'a>(ed: &'a Editor, id: &str) -> Option<&'a Object> {
    ed.doc.pages.iter().find_map(|p| p.objects.iter().find(|o| o.id.0 == id))
}

/// Différences entre la référence `old` et l'état courant.
fn diff(old: &Snapshot, ed: &Editor) -> Value {
    let now = Snapshot::of(ed);
    let (mut added, mut modified, mut removed) = (Vec::new(), Vec::new(), Vec::new());
    for (id, h) in &now.objects {
        match old.objects.get(id) {
            None => added.push(id.clone()),
            Some(o) if o != h => modified.push(id.clone()),
            _ => {}
        }
    }
    removed.extend(old.objects.keys().filter(|id| !now.objects.contains_key(*id)).cloned());
    for v in [&mut added, &mut modified, &mut removed] {
        v.sort();
    }
    let sum = |ids: &[String]| -> Vec<Value> { ids.iter().filter_map(|id| find_anywhere(ed, id)).map(summary).collect() };
    json!({
        "added": sum(&added), "modified": sum(&modified), "removed": removed,
        "pages_changed": old.pages != now.pages,
        "unchanged": added.is_empty() && modified.is_empty() && removed.is_empty() && old.pages == now.pages,
    })
}

/// Intègre à la référence les seules modifications faites par l'agent (`before` → `after`), pour que celles de l'utilisateur restent à signaler.
fn absorb(reference: &mut Snapshot, before: &Snapshot, after: &Snapshot) {
    for (id, h) in &after.objects {
        if before.objects.get(id) != Some(h) {
            reference.objects.insert(id.clone(), *h);
        }
    }
    for id in before.objects.keys().filter(|id| !after.objects.contains_key(*id)) {
        reference.objects.remove(id);
    }
    if before.pages != after.pages {
        reference.pages = after.pages.clone();
    }
}

/// Comme [`apply`], en suivant les changements : `board_changes` rapporte ce que l'utilisateur a modifié depuis le dernier regard de l'agent
/// (appel de `describe_board` ou de `board_changes`) ; les modifications de l'agent lui-même ne sont pas rapportées.
pub fn apply_tracked(ed: &mut Editor, name: &str, a: &Value, tracker: &mut Option<Snapshot>) -> Res<Value> {
    if Snapshot::too_big(ed) {
        return if name == "board_changes" { Err("tableau trop volumineux pour le suivi des changements".into()) } else { apply(ed, name, a) };
    }
    if name == "board_changes" {
        let out = match tracker {
            None => {
                json!({"baseline": true, "message": "Référence enregistrée : les prochains appels signaleront les modifications faites ensuite par l'utilisateur."})
            }
            Some(old) => diff(old, ed),
        };
        *tracker = Some(Snapshot::of(ed));
        return Ok(out);
    }
    let before = Snapshot::of(ed);
    let r = apply(ed, name, a)?;
    let after = Snapshot::of(ed);
    // première observation ou regard complet : l'état courant devient la référence ; sinon seules les modifications de l'agent sont absorbées
    match tracker.as_mut() {
        Some(t) if name != "describe_board" => absorb(t, &before, &after),
        _ => *tracker = Some(after),
    }
    Ok(r)
}

/* ------------------------------------------------------------------ images */

/// Taille maximale d'une image insérée.
const IMAGE_MAX_BYTES: usize = 15 << 20;

/// Type d'image reconnu d'après les premiers octets (le type annoncé par l'appelant n'est jamais cru).
fn sniff_image(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("image/png")
    } else if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if data.starts_with(b"GIF8") {
        Some("image/gif")
    } else if data.len() > 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        Some("image/webp")
    } else if std::str::from_utf8(&data[..data.len().min(2048)]).is_ok_and(|t| t.contains("<svg")) {
        Some("image/svg+xml")
    } else {
        None
    }
}

/// Dimensions naturelles d'une image (pixels, ou unités du `viewBox` pour un SVG).
fn image_size(mime: &str, data: &[u8]) -> Option<(f64, f64)> {
    if mime == "image/svg+xml" {
        let t = std::str::from_utf8(data).ok()?;
        let vb = t.split("viewBox=\"").nth(1)?.split('"').next()?;
        let v: Vec<f64> = vb.split([' ', ',']).filter_map(|x| x.parse().ok()).collect();
        return (v.len() == 4 && v[2] > 0.0 && v[3] > 0.0).then(|| (v[2], v[3]));
    }
    let (w, h) = image::ImageReader::new(std::io::Cursor::new(data)).with_guessed_format().ok()?.into_dimensions().ok()?;
    Some((f64::from(w), f64::from(h)))
}

fn add_image(ed: &mut Editor, a: &Value) -> Res<Value> {
    use base64::Engine;
    let b64 = s(a, "data_base64").map_err(|_| "paramètre « source » (fichier image) ou « data_base64 » manquant".to_owned())?;
    let data = base64::engine::general_purpose::STANDARD.decode(b64.trim()).map_err(|e| format!("base64 invalide : {e}"))?;
    if data.len() > IMAGE_MAX_BYTES {
        return Err(format!("image trop volumineuse ({} Mio, maximum {})", data.len() >> 20, IMAGE_MAX_BYTES >> 20));
    }
    let mime = sniff_image(&data).ok_or("format d'image non reconnu (PNG, JPEG, GIF, WebP ou SVG)")?;
    let (nw, nh) = image_size(mime, &data).ok_or("image illisible (dimensions introuvables)")?;
    // taille d'affichage : demandée (le rapport est conservé si une seule dimension est donnée), sinon naturelle réduite à 600 au plus
    let (w, h) = match (of(a, "w"), of(a, "h")) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (w, w * nh / nw),
        (None, Some(h)) => (h * nw / nh, h),
        (None, None) => {
            let k = (600.0 / nw.max(nh)).min(1.0);
            (nw * k, nh * k)
        }
    };
    if w < 1.0 || h < 1.0 || w > 20_000.0 || h > 20_000.0 {
        return Err("taille d'affichage invalide (entre 1 et 20 000)".into());
    }
    let aid = ed.doc.add_asset(mime, data);
    let at = origin(ed, a);
    let img = ImageObj { frame: Frame::new(at.x, at.y, w, h), asset: aid, latex: None };
    let id = ed.insert(Object::new(Kind::Image(img), Style { width: 0.0, fill: None, ..Style::default() }));
    Ok(json!({"id": id.0, "w": w.round(), "h": h.round(), "type": mime}))
}

fn add_shape(ed: &mut Editor, a: &Value) -> Res<Value> {
    let geom = geom_of(s(a, "geom")?)?;
    let at = origin(ed, a);
    let text = os(a, "text").unwrap_or("");
    let (dw, dh) = match geom {
        BoxGeom::Sticky => (200.0, 200.0),
        BoxGeom::Ellipse | BoxGeom::Diamond => (180.0, 110.0),
        BoxGeom::Triangle => (150.0, 130.0),
        _ => (180.0, 90.0),
    };
    let fill = ocolor(a, "fill")?.or(match geom {
        BoxGeom::Sticky => Some(Color::rgb(0xff, 0xec, 0x99)),
        _ => None,
    });
    let style = Style {
        stroke: ocolor(a, "stroke")?.unwrap_or(Color::Ink),
        fill,
        width: if geom == BoxGeom::Text { 0.0 } else { 2.0 },
        roundness: if geom == BoxGeom::Rect { 0.15 } else { 0.0 },
        ..Style::default()
    };
    let label = Label::new(text, of(a, "font_size").unwrap_or(20.0), if geom == BoxGeom::Text { Align::Left } else { Align::Center });
    let mut o = Object::new_box(geom, Frame::new(at.x, at.y, of(a, "w").unwrap_or(dw), of(a, "h").unwrap_or(dh)), style, label);
    if let Kind::Box(b) = &mut o.kind {
        b.fit(&ApproxMeasurer);
    }
    let id = ed.insert(o);
    let f = ed.page().get(&id).and_then(Object::frame).ok_or("objet non créé")?;
    Ok(json!({"id": id.0, "x": f.x.round(), "y": f.y.round(), "w": f.w.round(), "h": f.h.round()}))
}

fn add_connector(ed: &mut Editor, a: &Value) -> Res<Value> {
    let (from, to) = (find(ed, s(a, "from")?)?, find(ed, s(a, "to")?)?);
    for o in [from, to] {
        if !o.is_attachable() {
            return Err(format!("l'objet {} ne peut pas être relié (formes, tableaux et images seulement)", o.id.0));
        }
    }
    let mut c = Connector::new(End::bound(from.id.clone()), End::bound(to.id.clone()));
    c.route = match os(a, "route").unwrap_or("straight") {
        "straight" => Route::Straight,
        "elbow" => Route::Elbow,
        "curve" => Route::Curve,
        r => return Err(format!("tracé inconnu « {r} » (straight, elbow, curve)")),
    };
    (c.head_start, c.head_end) = match os(a, "arrow").unwrap_or("end") {
        "end" => (Arrowhead::None, Arrowhead::Arrow),
        "start" => (Arrowhead::Arrow, Arrowhead::None),
        "both" => (Arrowhead::Arrow, Arrowhead::Arrow),
        "none" => (Arrowhead::None, Arrowhead::None),
        r => return Err(format!("pointe inconnue « {r} » (end, start, both, none)")),
    };
    if let Some(l) = os(a, "label").filter(|l| !l.is_empty()) {
        c.label = Some(Label::new(l, 16.0, Align::Center));
    }
    let style = Style { stroke: ocolor(a, "stroke")?.unwrap_or(Color::Ink), width: 2.0, ..Style::default() };
    let id = ed.insert(Object::new(Kind::Connector(c), style));
    Ok(json!({"id": id.0}))
}

fn add_table(ed: &mut Editor, a: &Value) -> Res<Value> {
    let rows: Vec<Vec<String>> = a
        .get("rows")
        .and_then(Value::as_array)
        .ok_or("paramètre « rows » manquant")?
        .iter()
        .map(|r| {
            r.as_array()
                .map(|r| r.iter().map(|c| c.as_str().map_or_else(|| c.to_string(), str::to_owned)).collect())
                .ok_or("chaque ligne de « rows » doit être une liste")
        })
        .collect::<Result<_, _>>()?;
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    if rows.is_empty() || cols == 0 || rows.len() > 200 || cols > 50 {
        return Err("« rows » doit contenir de 1 à 200 lignes et de 1 à 50 colonnes".into());
    }
    let at = origin(ed, a);
    let mut t = Table::new(at, rows.len(), cols);
    t.header = true;
    for (r, row) in rows.iter().enumerate() {
        for (c, text) in row.iter().enumerate() {
            if let Some(cell) = t.cell_mut(r, c) {
                *cell = RichText::plain(text);
            }
        }
    }
    // largeur de colonne d'après le texte le plus long (au moins la largeur par défaut)
    for c in 0..cols {
        let widest = rows.iter().filter_map(|r| r.get(c)).map(|t| ApproxMeasurer.width(t, FontFamily::Sans, 18.0, true)).fold(0.0, f64::max);
        t.col_w[c] = t.col_w[c].max((widest + 28.0).ceil());
    }
    let id = ed.insert(Object::new(Kind::Table(t), Style { width: 2.0, fill: None, ..Style::default() }));
    Ok(json!({"id": id.0, "rows": rows.len(), "cols": cols}))
}

fn add_code_block(ed: &mut Editor, a: &Value) -> Res<Value> {
    let code = s(a, "code")?;
    let at = origin(ed, a);
    let style = Style { stroke: Color::rgb(0xe6, 0xe6, 0xe6), fill: Some(Color::rgb(0x1e, 0x20, 0x2e)), width: 0.0, roundness: 0.1, ..Style::default() };
    let label = Label { text: wb_core::highlight::highlight(code), family: FontFamily::Mono, size: 18.0, align: Align::Left };
    let mut o = Object::new_box(BoxGeom::Rect, Frame::new(at.x, at.y, 420.0, 120.0), style, label);
    if let Kind::Box(b) = &mut o.kind {
        let lay = b.layout(&ApproxMeasurer);
        b.frame.w = (lay.width + 48.0).max(260.0);
        b.frame.h = (lay.height + 40.0).max(90.0);
    }
    let id = ed.insert(o);
    Ok(json!({"id": id.0}))
}

fn add_formula(ed: &mut Editor, a: &Value) -> Res<Value> {
    let latex = s(a, "latex")?;
    let f = wb_formats::math::render(latex, 36.0).map_err(|e| e.to_string())?;
    let aid = ed.doc.add_asset("image/svg+xml", f.svg.into_bytes());
    let at = origin(ed, a);
    let style = Style { stroke: ocolor(a, "color")?.unwrap_or(Color::Ink), width: 0.0, fill: None, ..Style::default() };
    let img = ImageObj { frame: Frame::new(at.x, at.y, f.w, f.h), asset: aid, latex: Some(latex.to_owned()) };
    let id = ed.insert(Object::new(Kind::Image(img), style));
    Ok(json!({"id": id.0, "w": f.w.round(), "h": f.h.round()}))
}

fn import_mermaid(ed: &mut Editor, a: &Value) -> Res<Value> {
    let mut objs = wb_formats::mermaid::to_objects(s(a, "mermaid")?, &ApproxMeasurer).map_err(|e| e.to_string())?;
    if objs.is_empty() {
        return Err("le diagramme Mermaid est vide".into());
    }
    if a.get("new_page").and_then(Value::as_bool).unwrap_or(false) {
        let n = ed.doc.pages.len() + 1;
        let id = ed.add_page(&format!("Mermaid {n}"));
        ed.current = id;
    }
    let at = origin(ed, a);
    let top_left =
        objs.iter().filter_map(Object::frame).fold(None::<Pt>, |acc, f| Some(acc.map_or(Pt::new(f.x, f.y), |p| Pt::new(p.x.min(f.x), p.y.min(f.y)))));
    let d = top_left.map_or(Pt::new(0.0, 0.0), |p| at - p);
    for o in &mut objs {
        o.translate(d);
    }
    let ids = ed.insert_all(objs);
    Ok(json!({"created": ids.len(), "ids": ids.iter().map(|i| i.0.clone()).collect::<Vec<_>>()}))
}

fn update_object(ed: &mut Editor, a: &Value) -> Res<Value> {
    let id = find(ed, s(a, "id")?)?.id.clone();
    let (fill, stroke) = (ocolor(a, "fill")?, ocolor(a, "stroke")?);
    let (text, x, y, w, h) = (os(a, "text").map(str::to_owned), of(a, "x"), of(a, "y"), of(a, "w"), of(a, "h"));
    let mut warn: Vec<&str> = Vec::new();
    ed.update(&id, |o| {
        if let Some(f) = fill {
            o.style.fill = Some(f);
        }
        if let Some(s) = stroke {
            o.style.stroke = s;
        }
        match &mut o.kind {
            Kind::Box(b) => {
                if let Some(t) = &text {
                    b.label.text = RichText::plain(t);
                }
                if let Some(w) = w {
                    b.frame.w = w.max(4.0);
                }
                if let Some(h) = h {
                    b.frame.h = h.max(4.0);
                }
                b.fit(&ApproxMeasurer);
            }
            Kind::Connector(c) => {
                if let Some(t) = &text {
                    c.label = (!t.is_empty()).then(|| Label::new(t, 16.0, Align::Center));
                }
            }
            _ if text.is_some() => warn.push("le texte n'est modifiable que pour les formes et les connecteurs"),
            _ => {}
        }
        if x.is_some() || y.is_some() {
            if let Some(f) = o.frame().map(|f| f.aabb()).or_else(|| matches!(o.kind, Kind::Table(_)).then(|| table_rect(o))) {
                o.translate(Pt::new(x.map_or(0.0, |x| x - f.x), y.map_or(0.0, |y| y - f.y)));
            } else {
                warn.push("cet objet ne peut pas être déplacé par x/y");
            }
        }
    });
    Ok(json!({"id": id.0, "avertissements": warn}))
}

fn table_rect(o: &Object) -> Rect {
    match &o.kind {
        Kind::Table(t) => t.rect(),
        _ => Rect::new(0.0, 0.0, 0.0, 0.0),
    }
}

fn export(ed: &mut Editor, a: &Value) -> Res<Value> {
    let output = PathBuf::from(s(a, "output")?);
    let name = lower_name(&output);
    if !EXPORTS.iter().any(|e| name.ends_with(e)) {
        return Err(format!("extension de sortie non gérée (attendu : {})", EXPORTS.join(", ")));
    }
    let doc = &ed.doc;
    let page = ed.page();
    let svg = || export_svg(doc, page, None, &ApproxMeasurer, &SvgOptions::default()).ok_or_else(|| "la page est vide : rien à exporter".to_owned());
    let bytes: Vec<u8> = if name.ends_with(".svg") {
        svg()?.into_bytes()
    } else if name.ends_with(".png") {
        wb_formats::raster::svg_to_png(&svg()?, 2.0).map_err(|e| e.to_string())?
    } else if name.ends_with(".pdf") {
        wb_formats::raster::svg_to_pdf(&svg()?).map_err(|e| e.to_string())?
    } else if name.ends_with(".drawio") {
        wb_formats::drawio::export(doc, &ApproxMeasurer).into_bytes()
    } else if name.ends_with(".excalidraw") {
        wb_formats::excalidraw::export(doc, page, &ApproxMeasurer).into_bytes()
    } else if name.ends_with(".canvas") {
        wb_formats::jsoncanvas::export(page).0.into_bytes()
    } else {
        wb_core::serial::to_json(doc, true).into_bytes()
    };
    std::fs::write(&output, &bytes).map_err(|e| format!("écriture de {} : {e}", output.display()))?;
    Ok(json!({"output": output.display().to_string(), "bytes": bytes.len()}))
}
