//! Session complète avec le vrai binaire `wb-mcp` sur stdin/stdout, comme le fait un client MCP.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
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
        Self { child, stdin, stdout, next: 1 }
    }

    fn send(&mut self, v: &Value) {
        writeln!(self.stdin, "{v}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        let v: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("réponse invalide ({e}) : {line}"));
        assert_eq!(v["id"], json!(id), "{v}");
        v
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
