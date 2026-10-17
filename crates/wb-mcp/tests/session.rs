//! Session complète avec le vrai binaire `wb-mcp` sur stdin/stdout, comme le fait un client MCP.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

struct Client {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<Value>,
    /// Messages reçus pendant qu'on attendait une réponse (notifications du serveur).
    pending: Vec<Value>,
    next: u64,
}

impl Client {
    fn start(root: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_wb-mcp"))
            .arg("--root")
            .arg(root)
            .arg("--no-live")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("lancement de wb-mcp");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        // un fil lit les lignes pour pouvoir attendre avec un délai
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for l in stdout.lines().map_while(Result::ok) {
                let v: Value = serde_json::from_str(&l).unwrap_or_else(|e| panic!("sortie invalide ({e}) : {l}"));
                if tx.send(v).is_err() {
                    break;
                }
            }
        });
        Self { child, stdin, lines, pending: Vec::new(), next: 1 }
    }

    fn send(&mut self, v: &Value) {
        writeln!(self.stdin, "{v}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let v = self.lines.recv_timeout(Duration::from_secs(20)).expect("réponse attendue");
            if v["id"] == json!(id) {
                return v;
            }
            self.pending.push(v);
        }
    }

    /// Attend une notification `method` (portant `uri` si donnée).
    fn wait_notification(&mut self, method: &str, uri: Option<&str>, timeout: Duration) -> Option<Value> {
        let matches = |v: &Value| v["method"] == json!(method) && uri.is_none_or(|u| v["params"]["uri"] == json!(u));
        if let Some(i) = self.pending.iter().position(matches) {
            return Some(self.pending.remove(i));
        }
        let end = Instant::now() + timeout;
        while let Some(left) = end.checked_duration_since(Instant::now()) {
            match self.lines.recv_timeout(left) {
                Ok(v) if matches(&v) => return Some(v),
                Ok(v) => self.pending.push(v),
                Err(_) => break,
            }
        }
        None
    }

    /// Appelle un outil ; renvoie le JSON du résultat ou le message d'erreur.
    fn tool(&mut self, name: &str, args: Value) -> Result<Value, String> {
        let r = self.request("tools/call", json!({"name": name, "arguments": args}));
        let text = r["result"]["content"][0]["text"].as_str().unwrap_or_else(|| panic!("{r}")).to_owned();
        if r["result"]["isError"] == json!(true) { Err(text) } else { Ok(serde_json::from_str(&text).unwrap_or(Value::String(text))) }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("wb-mcp-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.canonicalize().unwrap()
}

#[test]
fn handshake_lists_the_tools() {
    let dir = temp_dir("hs");
    let mut c = Client::start(&dir);
    let init = c.request("initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}));
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "wb-mcp");
    c.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    assert_eq!(c.request("ping", json!({}))["result"], json!({}), "pas de réponse parasite à la notification");
    let list = c.request("tools/list", json!({}));
    let names: Vec<&str> = list["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    for n in ["create_board", "describe_board", "add_shape", "add_connector", "import_mermaid", "export", "add_formula", "auto_layout"] {
        assert!(names.contains(&n), "outil manquant : {n}");
    }
    assert!(list["result"]["tools"].as_array().unwrap().iter().all(|t| t["inputSchema"]["type"] == "object" && t["description"].is_string()));
    let unknown = c.request("nope", json!({}));
    assert_eq!(unknown["error"]["code"], -32601);
    let bad_tool = c.request("tools/call", json!({"name": "inexistant", "arguments": {}}));
    assert_eq!(bad_tool["error"]["code"], -32602);
}

#[test]
fn an_agent_builds_a_diagram_and_exports_it() {
    let dir = temp_dir("diag");
    let mut c = Client::start(&dir);
    let board = dir.join("schema.tableau.json");
    let p = board.to_str().unwrap();

    c.tool("create_board", json!({"path": p, "name": "Archi"})).unwrap();
    assert!(c.tool("create_board", json!({"path": p})).is_err(), "pas d'écrasement d'un tableau existant");

    let a = c.tool("add_shape", json!({"path": p, "geom": "rect", "text": "Client", "fill": "#a5d8ff"})).unwrap();
    let b = c.tool("add_shape", json!({"path": p, "geom": "ellipse", "text": "API"})).unwrap();
    let (ida, idb) = (a["id"].as_str().unwrap().to_owned(), b["id"].as_str().unwrap().to_owned());
    assert!(b["y"].as_f64().unwrap() > a["y"].as_f64().unwrap(), "le second objet est placé sous le premier");
    c.tool("add_connector", json!({"path": p, "from": ida, "to": idb, "route": "elbow", "label": "HTTPS"})).unwrap();
    c.tool("add_table", json!({"path": p, "rows": [["Route", "Verbe"], ["/users", "GET"]]})).unwrap();
    c.tool("add_code_block", json!({"path": p, "code": "fn main() {}"})).unwrap();
    c.tool("add_formula", json!({"path": p, "latex": r"\frac{a}{b}"})).unwrap();
    assert!(c.tool("add_formula", json!({"path": p, "latex": r"\inconnu"})).is_err());
    let mm = c.tool("import_mermaid", json!({"path": p, "mermaid": "flowchart LR\n  A[Début] --> B{Choix}\n  B -->|oui| C[Fin]", "new_page": true})).unwrap();
    assert_eq!(mm["created"], 5, "3 formes + 2 flèches : {mm}");
    c.tool("auto_layout", json!({"path": p, "page": 2, "direction": "LR"})).unwrap();

    let d = c.tool("describe_board", json!({"path": p, "page": 1})).unwrap();
    assert_eq!(d["pages"].as_array().unwrap().len(), 2);
    let objs = d["page"]["objects"].as_array().unwrap();
    let types: Vec<&str> = objs.iter().map(|o| o["type"].as_str().unwrap()).collect();
    for t in ["rect", "ellipse", "connector", "table", "rect", "formula"] {
        assert!(types.contains(&t), "{t} absent : {types:?}");
    }
    let conn = objs.iter().find(|o| o["type"] == "connector").unwrap();
    assert_eq!(conn["from"]["object"], json!(ida));
    assert_eq!(conn["label"], "HTTPS");

    // modification puis suppression
    c.tool("update_object", json!({"path": p, "id": ida, "text": "Navigateur", "x": 500, "y": 300})).unwrap();
    let d = c.tool("describe_board", json!({"path": p})).unwrap();
    let first = d["page"]["objects"].as_array().unwrap().iter().find(|o| o["id"] == json!(ida)).unwrap().clone();
    assert_eq!(first["text"], "Navigateur");
    assert_eq!((first["x"].as_f64().unwrap(), first["y"].as_f64().unwrap()), (500.0, 300.0));
    c.tool("delete_object", json!({"path": p, "id": idb})).unwrap();
    assert!(c.tool("update_object", json!({"path": p, "id": idb, "text": "x"})).is_err(), "objet supprimé");

    // exports
    for ext in ["svg", "png", "pdf", "drawio", "excalidraw", "canvas"] {
        let out = dir.join(format!("sortie.{ext}"));
        let r = c.tool("export", json!({"path": p, "output": out.to_str().unwrap()})).unwrap_or_else(|e| panic!("{ext} : {e}"));
        assert!(r["bytes"].as_u64().unwrap() > 100, "{ext} : {r}");
        assert!(out.exists());
    }
    let svg = std::fs::read_to_string(dir.join("sortie.svg")).unwrap();
    assert!(svg.contains("Navigateur"));
    assert_eq!(&std::fs::read(dir.join("sortie.pdf")).unwrap()[..5], b"%PDF-");
    // le fichier produit est relisible par l'outil lui-même (aller-retour via draw.io)
    let d = c.tool("describe_board", json!({"path": dir.join("sortie.drawio").to_str().unwrap()})).unwrap();
    assert!(!d["page"]["objects"].as_array().unwrap().is_empty());
}

#[test]
fn files_are_confined_to_the_root_and_only_native_files_are_written() {
    let dir = temp_dir("root");
    let outside = temp_dir("outside");
    let mut c = Client::start(&dir);
    let err = c.tool("create_board", json!({"path": outside.join("x.tableau.json").to_str().unwrap()})).unwrap_err();
    assert!(err.contains("hors de la racine"), "{err}");
    let err = c.tool("create_board", json!({"path": dir.join("..").join("y.tableau.json").to_str().unwrap()})).unwrap_err();
    assert!(err.contains("hors de la racine"), "{err}");
    // chemin relatif : résolu sous la racine
    c.tool("create_board", json!({"path": "rel.tableau.json"})).unwrap();
    assert!(dir.join("rel.tableau.json").exists());
    // pas d'enregistrement hors format natif
    let err = c.tool("create_board", json!({"path": "autre.drawio"})).unwrap_err();
    assert!(err.contains(".tableau.json"), "{err}");
    // export vers une extension non prévue
    let err = c.tool("export", json!({"path": "rel.tableau.json", "output": "script.ps1"})).unwrap_err();
    assert!(err.contains("extension"), "{err}");
}

#[test]
fn table_columns_grow_to_fit_long_text() {
    let dir = temp_dir("tbl");
    let mut c = Client::start(&dir);
    let p = dir.join("t.tableau.json");
    let p = p.to_str().unwrap();
    c.tool("create_board", json!({"path": p})).unwrap();
    c.tool("add_table", json!({"path": p, "rows": [["court", "une cellule avec un texte vraiment très long à afficher"]]})).unwrap();
    let d = c.tool("describe_board", json!({"path": p})).unwrap();
    let w = d["page"]["objects"][0]["w"].as_f64().unwrap();
    assert!(w > 140.0 + 300.0, "largeur du tableau : {w}");
}

/// PNG 1×1 (rouge).
const PNG_1X1: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==";

#[test]
fn images_are_added_from_files_or_base64_and_validated() {
    use std::io::Write as _;
    let dir = temp_dir("img");
    let mut c = Client::start(&dir);
    let p = dir.join("i.tableau.json");
    let p = p.to_str().unwrap();
    c.tool("create_board", json!({"path": p})).unwrap();
    // depuis un fichier de la racine : taille naturelle conservée (petite image)
    let png = base64_decode(PNG_1X1);
    std::fs::write(dir.join("rouge.png"), &png).unwrap();
    let r = c.tool("add_image", json!({"path": p, "source": "rouge.png"})).unwrap();
    assert_eq!(r["type"], "image/png");
    assert_eq!((r["w"].as_f64().unwrap(), r["h"].as_f64().unwrap()), (1.0, 1.0));
    // taille demandée : une dimension suffit, le rapport est conservé
    let r = c.tool("add_image", json!({"path": p, "data_base64": PNG_1X1, "w": 120})).unwrap();
    assert_eq!((r["w"].as_f64().unwrap(), r["h"].as_f64().unwrap()), (120.0, 120.0));
    // SVG
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 100"><rect width="200" height="100" fill="red"/></svg>"#;
    let mut f = std::fs::File::create(dir.join("a.svg")).unwrap();
    f.write_all(svg.as_bytes()).unwrap();
    let r = c.tool("add_image", json!({"path": p, "source": "a.svg"})).unwrap();
    assert_eq!((r["w"].as_f64().unwrap(), r["h"].as_f64().unwrap(), r["type"].as_str().unwrap()), (200.0, 100.0, "image/svg+xml"));
    let d = c.tool("describe_board", json!({"path": p})).unwrap();
    assert_eq!(d["page"]["objects"].as_array().unwrap().iter().filter(|o| o["type"] == "image").count(), 3);
    // refus : format inconnu, hors racine, base64 invalide
    std::fs::write(dir.join("faux.png"), b"ceci n'est pas une image").unwrap();
    assert!(c.tool("add_image", json!({"path": p, "source": "faux.png"})).unwrap_err().contains("non reconnu"));
    let outside = temp_dir("img-out");
    std::fs::write(outside.join("x.png"), &png).unwrap();
    assert!(c.tool("add_image", json!({"path": p, "source": outside.join("x.png").to_str().unwrap()})).unwrap_err().contains("hors de la racine"));
    assert!(c.tool("add_image", json!({"path": p, "data_base64": "%%%"})).is_err());
    // l'export contient bien l'image
    let out = dir.join("o.svg");
    c.tool("export", json!({"path": p, "output": out.to_str().unwrap()})).unwrap();
    assert!(std::fs::read_to_string(out).unwrap().contains("data:image/png;base64"));
}

fn base64_decode(s: &str) -> Vec<u8> {
    // petit décodeur pour ne pas ajouter de dépendance de test
    let val = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        _ => 63,
    };
    let b: Vec<u8> = s.bytes().filter(|c| *c != b'=').map(val).collect();
    b.chunks(4)
        .flat_map(|c| {
            let n = c.iter().fold(0u32, |a, v| (a << 6) | u32::from(*v)) << (6 * (4 - c.len()));
            [(n >> 16) as u8, (n >> 8) as u8, n as u8].into_iter().take(c.len() * 6 / 8)
        })
        .collect()
}

/// Modifie le fichier comme le ferait l'utilisateur dans l'application.
fn user_edit(path: &Path, f: impl FnOnce(&mut wb_core::Document)) {
    let mut doc = wb_core::serial::load(path).unwrap();
    f(&mut doc);
    wb_core::serial::save(&doc, path).unwrap();
}

#[test]
fn board_changes_reports_what_the_user_did_but_not_what_the_agent_did() {
    let dir = temp_dir("chg");
    let mut c = Client::start(&dir);
    let board = dir.join("c.tableau.json");
    let p = board.to_str().unwrap();
    c.tool("create_board", json!({"path": p})).unwrap();
    let first = c.tool("board_changes", json!({"path": p})).unwrap();
    assert_eq!(first["baseline"], true, "premier appel : référence");
    let a = c.tool("add_shape", json!({"path": p, "geom": "rect", "text": "Agent"})).unwrap()["id"].as_str().unwrap().to_owned();
    let b = c.tool("add_shape", json!({"path": p, "geom": "rect", "text": "À modifier"})).unwrap()["id"].as_str().unwrap().to_owned();
    let none = c.tool("board_changes", json!({"path": p})).unwrap();
    assert_eq!(none["unchanged"], true, "les ajouts de l'agent ne sont pas des changements de l'utilisateur : {none}");
    // l'utilisateur ajoute une forme, en modifie une, en supprime une
    user_edit(&board, |d| {
        use wb_core::*;
        d.pages[0].objects.push(Object::new_box(
            BoxGeom::Ellipse,
            Frame::new(0.0, 0.0, 80.0, 50.0),
            Style::default(),
            Label::new("Utilisateur", 20.0, Align::Center),
        ));
        for o in &mut d.pages[0].objects {
            if o.id.0 == b
                && let Kind::Box(bx) = &mut o.kind
            {
                bx.label.text = RichText::plain("Modifié");
            }
        }
        d.pages[0].objects.retain(|o| o.id.0 != a);
    });
    let ch = c.tool("board_changes", json!({"path": p})).unwrap();
    assert_eq!(ch["added"].as_array().unwrap().len(), 1, "{ch}");
    assert_eq!(ch["added"][0]["text"], "Utilisateur");
    assert_eq!(ch["modified"].as_array().unwrap().len(), 1);
    assert_eq!(ch["modified"][0]["text"], "Modifié");
    assert_eq!(ch["removed"], json!([a]));
    // déjà vu : plus rien
    assert_eq!(c.tool("board_changes", json!({"path": p})).unwrap()["unchanged"], true);
    // une modification de l'utilisateur reste à signaler même si l'agent agit entre-temps
    user_edit(&board, |d| {
        d.pages[0].objects.retain(|o| o.id.0 != b);
    });
    c.tool("add_shape", json!({"path": p, "geom": "rect", "text": "Autre ajout de l'agent"})).unwrap();
    let ch = c.tool("board_changes", json!({"path": p})).unwrap();
    assert_eq!(ch["removed"], json!([b]), "{ch}");
    assert!(ch["added"].as_array().unwrap().is_empty(), "l'ajout de l'agent n'est pas rapporté : {ch}");
}

#[test]
fn resources_are_listed_read_and_subscribers_are_notified_of_changes() {
    let dir = temp_dir("res");
    let mut c = Client::start(&dir);
    let init = c.request("initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}}));
    assert_eq!(init["result"]["capabilities"]["resources"]["subscribe"], true);
    let board = dir.join("Mon schéma.tableau.json");
    let p = board.to_str().unwrap();
    c.tool("create_board", json!({"path": p})).unwrap();
    c.tool("add_shape", json!({"path": p, "geom": "rect", "text": "Première"})).unwrap();

    let list = c.request("resources/list", json!({}));
    let res = list["result"]["resources"].as_array().unwrap();
    assert_eq!(res.len(), 1, "pas d'application : seulement le fichier : {list}");
    let uri = res[0]["uri"].as_str().unwrap().to_owned();
    assert!(uri.starts_with("file:///") && uri.is_ascii() && uri.contains("%C3%A9"), "{uri}");
    assert_eq!(res[0]["name"], "Mon schéma.tableau.json");

    let read = c.request("resources/read", json!({"uri": uri}));
    let body: Value = serde_json::from_str(read["result"]["contents"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(body["page"]["objects"][0]["text"], "Première");
    assert_eq!(c.request("resources/read", json!({"uri": "tableau-blanc://live"}))["error"]["code"], -32002, "pas d'application ouverte");
    assert_eq!(c.request("resources/read", json!({"uri": "file:///etc/passwd"}))["error"]["code"], -32002, "hors racine refusé");

    // abonnement : une modification de l'extérieur déclenche la notification
    assert_eq!(c.request("resources/subscribe", json!({"uri": uri}))["result"], json!({}));
    std::thread::sleep(Duration::from_millis(1500)); // laisse le sondage prendre sa référence
    user_edit(&board, |d| {
        use wb_core::*;
        d.pages[0].objects.push(Object::new_box(BoxGeom::Rect, Frame::new(0.0, 0.0, 50.0, 50.0), Style::default(), Label::new("x", 20.0, Align::Center)));
    });
    let f = std::fs::OpenOptions::new().write(true).open(&board).unwrap();
    f.set_modified(std::time::SystemTime::now() + Duration::from_secs(3)).unwrap();
    let n = c.wait_notification("notifications/resources/updated", Some(&uri), Duration::from_secs(8));
    assert!(n.is_some(), "notification attendue");
    // désabonné : plus de notification
    c.request("resources/unsubscribe", json!({"uri": uri}));
    let f = std::fs::OpenOptions::new().write(true).open(&board).unwrap();
    f.set_modified(std::time::SystemTime::now() + Duration::from_secs(9)).unwrap();
    assert!(c.wait_notification("notifications/resources/updated", Some(&uri), Duration::from_millis(2000)).is_none());
    // un nouveau fichier fait changer la liste
    c.tool("create_board", json!({"path": dir.join("second.tableau.json").to_str().unwrap()})).unwrap();
    assert!(c.wait_notification("notifications/resources/list_changed", None, Duration::from_secs(8)).is_some(), "list_changed attendu");
}
