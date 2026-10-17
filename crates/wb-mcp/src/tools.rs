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
    json!({"type": "string", "description": "Chemin du fichier tableau (.tableau.json)."})
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
        json!({"name": "describe_board", "description": "Liste les pages et les objets d'un tableau (id, type, texte, position, liens des connecteurs). Accepte aussi les fichiers draw.io, Excalidraw, JSON Canvas et Mermaid en lecture.",
            "inputSchema": obj(json!({"path": path_prop(), "page": page_prop()}), &["path"])}),
        json!({"name": "add_shape", "description": "Ajoute une forme avec du texte (rect, ellipse, diamond, triangle, sticky = note adhésive, text = texte libre). Renvoie l'id.",
            "inputSchema": obj(pos_props(json!({
                "geom": {"type": "string", "enum": ["rect", "ellipse", "diamond", "triangle", "sticky", "text"]},
                "text": {"type": "string"}, "w": {"type": "number"}, "h": {"type": "number"},
                "font_size": {"type": "number"}, "fill": color.clone(), "stroke": color.clone()})), &["path", "geom"])}),
        json!({"name": "add_connector", "description": "Relie deux objets par une flèche qui les suit quand ils bougent (ids renvoyés par add_shape ou describe_board).",
            "inputSchema": obj(json!({"path": path_prop(), "page": page_prop(), "from": {"type": "string"}, "to": {"type": "string"},
                "route": {"type": "string", "enum": ["straight", "elbow", "curve"]},
                "arrow": {"type": "string", "enum": ["end", "start", "both", "none"], "description": "Pointes de flèche (défaut : end)."},
                "label": {"type": "string"}, "stroke": color.clone()}), &["path", "from", "to"])}),
        json!({"name": "add_table", "description": "Ajoute un tableau de cellules ; `rows` est une liste de lignes (listes de textes). La première ligne est l'en-tête.",
            "inputSchema": obj(pos_props(json!({"rows": {"type": "array", "items": {"type": "array", "items": {"type": "string"}}}})), &["path", "rows"])}),
        json!({"name": "add_code_block", "description": "Ajoute un bloc de code (police à chasse fixe, fond sombre, coloration syntaxique).",
            "inputSchema": obj(pos_props(json!({"code": {"type": "string"}})), &["path", "code"])}),
        json!({"name": "add_formula", "description": "Ajoute une formule LaTeX (mode mathématique) composée comme en TeX. Erreur si le LaTeX n'est pas géré.",
            "inputSchema": obj(pos_props(json!({"latex": {"type": "string"}, "color": color.clone()})), &["path", "latex"])}),
        json!({"name": "import_mermaid", "description": "Ajoute un diagramme Mermaid (flowchart/graph, stateDiagram, sequenceDiagram) avec mise en page automatique. Renvoie les ids créés.",
            "inputSchema": obj(pos_props(json!({"mermaid": {"type": "string"}, "new_page": {"type": "boolean", "description": "Créer une nouvelle page pour le diagramme."}})), &["path", "mermaid"])}),
        json!({"name": "update_object", "description": "Modifie un objet : texte, position (coin supérieur gauche), taille (formes), couleurs.",
            "inputSchema": obj(json!({"path": path_prop(), "page": page_prop(), "id": {"type": "string"}, "text": {"type": "string"},
                "x": {"type": "number"}, "y": {"type": "number"}, "w": {"type": "number"}, "h": {"type": "number"},
                "fill": color.clone(), "stroke": color.clone()}), &["path", "id"])}),
        json!({"name": "delete_object", "description": "Supprime un objet (les connecteurs qui y étaient liés restent, détachés).",
            "inputSchema": obj(json!({"path": path_prop(), "page": page_prop(), "id": {"type": "string"}}), &["path", "id"])}),
        json!({"name": "auto_layout", "description": "Réorganise les formes reliées par des connecteurs en graphe en couches.",
            "inputSchema": obj(json!({"path": path_prop(), "page": page_prop(), "direction": {"type": "string", "enum": ["TB", "LR", "BT", "RL"], "description": "Défaut : TB."}}), &["path"])}),
        json!({"name": "add_page", "description": "Ajoute une page. Renvoie son numéro.",
            "inputSchema": obj(json!({"path": path_prop(), "name": {"type": "string"}}), &["path"])}),
        json!({"name": "export", "description": "Exporte une page vers un fichier ; le format est choisi par l'extension de `output` : .svg, .png, .pdf, .drawio, .excalidraw, .canvas, .tableau.json.",
            "inputSchema": obj(json!({"path": path_prop(), "output": {"type": "string"}, "page": page_prop()}), &["path", "output"])}),
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

/// Ouvre le tableau et sélectionne la page demandée.
fn open(srv: &Server, a: &Value) -> Res<(PathBuf, Editor)> {
    let path = resolve(srv, s(a, "path")?)?;
    let mut ed = Editor::new(load(&path)?);
    select_page(&mut ed, a.get("page"))?;
    Ok((path, ed))
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

fn done(ed: Editor, path: &Path, result: Value) -> Res<String> {
    save(&ed.doc, path)?;
    Ok(result.to_string())
}

/* ----------------------------------------------------------------- appel */

/// Exécute l'outil `name` ; le résultat est un texte JSON destiné à l'agent.
pub fn call(srv: &Server, name: &str, a: &Value) -> Res<String> {
    match name {
        "create_board" => {
            let path = resolve(srv, s(a, "path")?)?;
            if path.exists() {
                return Err(format!("{} existe déjà", path.display()));
            }
            let doc = Document::new(os(a, "name").unwrap_or("Tableau"));
            save(&doc, &path)?;
            Ok(json!({"path": path.display().to_string(), "pages": 1}).to_string())
        }
        "describe_board" => describe(srv, a),
        "add_shape" => add_shape(srv, a),
        "add_connector" => add_connector(srv, a),
        "add_table" => add_table(srv, a),
        "add_code_block" => add_code_block(srv, a),
        "add_formula" => add_formula(srv, a),
        "import_mermaid" => import_mermaid(srv, a),
        "update_object" => update_object(srv, a),
        "delete_object" => {
            let (path, mut ed) = open(srv, a)?;
            let id = find(&ed, s(a, "id")?)?.id.clone();
            ed.remove(std::slice::from_ref(&id));
            done(ed, &path, json!({"deleted": id.0}))
        }
        "auto_layout" => {
            let (path, mut ed) = open(srv, a)?;
            let dir = match os(a, "direction").unwrap_or("TB") {
                "TB" => Direction::TopDown,
                "LR" => Direction::LeftRight,
                "BT" => Direction::BottomUp,
                "RL" => Direction::RightLeft,
                d => return Err(format!("direction inconnue « {d} » (TB, LR, BT, RL)")),
            };
            let ids: Vec<Id> = ed.page().objects.iter().map(|o| o.id.clone()).collect();
            auto_layout(&mut ed, &ids, dir);
            done(ed, &path, json!({"layout": "ok", "objects": ids.len()}))
        }
        "add_page" => {
            let (path, mut ed) = open(srv, a)?;
            let n = ed.doc.pages.len() + 1;
            ed.add_page(os(a, "name").map_or_else(|| format!("Page {n}"), str::to_owned).as_str());
            done(ed, &path, json!({"page": n, "pages": n}))
        }
        "export" => export(srv, a),
        other => Err(format!("outil inconnu : {other}")),
    }
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

fn describe(srv: &Server, a: &Value) -> Res<String> {
    let path = resolve(srv, s(a, "path")?)?;
    let doc = load(&path)?;
    let mut ed = Editor::new(doc);
    select_page(&mut ed, a.get("page"))?;
    let page = ed.page();
    let objects: Vec<Value> = page
        .objects
        .iter()
        .map(|o| {
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
                    let (x, y, w, h) = (t.rect().x, t.rect().y, t.rect().w, t.rect().h);
                    m.insert("x".into(), json!(x.round()));
                    m.insert("y".into(), json!(y.round()));
                    m.insert("w".into(), json!(w.round()));
                    m.insert("h".into(), json!(h.round()));
                }
                Kind::Image(i) => {
                    if let Some(l) = &i.latex {
                        m.insert("latex".into(), json!(l));
                    }
                }
                Kind::Pen(_) => {}
            }
            v
        })
        .collect();
    let pages: Vec<Value> = ed.doc.pages.iter().enumerate().map(|(i, p)| json!({"number": i + 1, "name": p.name, "objects": p.objects.len()})).collect();
    Ok(json!({"name": ed.doc.name, "pages": pages, "page": {"name": page.name, "objects": objects}}).to_string())
}

fn add_shape(srv: &Server, a: &Value) -> Res<String> {
    let (path, mut ed) = open(srv, a)?;
    let geom = geom_of(s(a, "geom")?)?;
    let at = origin(&ed, a);
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
    done(ed, &path, json!({"id": id.0, "x": f.x.round(), "y": f.y.round(), "w": f.w.round(), "h": f.h.round()}))
}

fn add_connector(srv: &Server, a: &Value) -> Res<String> {
    let (path, mut ed) = open(srv, a)?;
    let (from, to) = (find(&ed, s(a, "from")?)?, find(&ed, s(a, "to")?)?);
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
    done(ed, &path, json!({"id": id.0}))
}

fn add_table(srv: &Server, a: &Value) -> Res<String> {
    let (path, mut ed) = open(srv, a)?;
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
    let at = origin(&ed, a);
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
    done(ed, &path, json!({"id": id.0, "rows": rows.len(), "cols": cols}))
}

fn add_code_block(srv: &Server, a: &Value) -> Res<String> {
    let (path, mut ed) = open(srv, a)?;
    let code = s(a, "code")?;
    let at = origin(&ed, a);
    let style = Style { stroke: Color::rgb(0xe6, 0xe6, 0xe6), fill: Some(Color::rgb(0x1e, 0x20, 0x2e)), width: 0.0, roundness: 0.1, ..Style::default() };
    let label = Label { text: wb_core::highlight::highlight(code), family: FontFamily::Mono, size: 18.0, align: Align::Left };
    let mut o = Object::new_box(BoxGeom::Rect, Frame::new(at.x, at.y, 420.0, 120.0), style, label);
    if let Kind::Box(b) = &mut o.kind {
        let lay = b.layout(&ApproxMeasurer);
        b.frame.w = (lay.width + 48.0).max(260.0);
        b.frame.h = (lay.height + 40.0).max(90.0);
    }
    let id = ed.insert(o);
    done(ed, &path, json!({"id": id.0}))
}

fn add_formula(srv: &Server, a: &Value) -> Res<String> {
    let (path, mut ed) = open(srv, a)?;
    let latex = s(a, "latex")?;
    let f = wb_formats::math::render(latex, 36.0).map_err(|e| e.to_string())?;
    let aid = ed.doc.add_asset("image/svg+xml", f.svg.into_bytes());
    let at = origin(&ed, a);
    let style = Style { stroke: ocolor(a, "color")?.unwrap_or(Color::Ink), width: 0.0, fill: None, ..Style::default() };
    let img = ImageObj { frame: Frame::new(at.x, at.y, f.w, f.h), asset: aid, latex: Some(latex.to_owned()) };
    let id = ed.insert(Object::new(Kind::Image(img), style));
    done(ed, &path, json!({"id": id.0, "w": f.w.round(), "h": f.h.round()}))
}

fn import_mermaid(srv: &Server, a: &Value) -> Res<String> {
    let (path, mut ed) = open(srv, a)?;
    let mut objs = wb_formats::mermaid::to_objects(s(a, "mermaid")?, &ApproxMeasurer).map_err(|e| e.to_string())?;
    if objs.is_empty() {
        return Err("le diagramme Mermaid est vide".into());
    }
    if a.get("new_page").and_then(Value::as_bool).unwrap_or(false) {
        let n = ed.doc.pages.len() + 1;
        let id = ed.add_page(&format!("Mermaid {n}"));
        ed.current = id;
    }
    let at = origin(&ed, a);
    let top_left =
        objs.iter().filter_map(Object::frame).fold(None::<Pt>, |acc, f| Some(acc.map_or(Pt::new(f.x, f.y), |p| Pt::new(p.x.min(f.x), p.y.min(f.y)))));
    let d = top_left.map_or(Pt::new(0.0, 0.0), |p| at - p);
    for o in &mut objs {
        o.translate(d);
    }
    let ids = ed.insert_all(objs);
    done(ed, &path, json!({"created": ids.len(), "ids": ids.iter().map(|i| i.0.clone()).collect::<Vec<_>>()}))
}

fn update_object(srv: &Server, a: &Value) -> Res<String> {
    let (path, mut ed) = open(srv, a)?;
    let id = find(&ed, s(a, "id")?)?.id.clone();
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
    done(ed, &path, json!({"id": id.0, "avertissements": warn}))
}

fn table_rect(o: &Object) -> Rect {
    match &o.kind {
        Kind::Table(t) => t.rect(),
        _ => Rect::new(0.0, 0.0, 0.0, 0.0),
    }
}

fn export(srv: &Server, a: &Value) -> Res<String> {
    let path = resolve(srv, s(a, "path")?)?;
    let output = resolve(srv, s(a, "output")?)?;
    let name = lower_name(&output);
    if !EXPORTS.iter().any(|e| name.ends_with(e)) {
        return Err(format!("extension de sortie non gérée (attendu : {})", EXPORTS.join(", ")));
    }
    let doc = load(&path)?;
    let mut ed = Editor::new(doc);
    select_page(&mut ed, a.get("page"))?;
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
    Ok(json!({"output": output.display().to_string(), "bytes": bytes.len()}).to_string())
}
