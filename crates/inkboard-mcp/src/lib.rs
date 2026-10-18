//! Serveur MCP (Model Context Protocol) du tableau blanc, niveau A : les outils lisent et écrivent des fichiers `.inkboard.json`.
//!
//! Le protocole est implémenté à la main (JSON-RPC 2.0, un message JSON par ligne sur stdin/stdout) : `initialize`, `ping`, `tools/list`, `tools/call`.
//! Voir [`tools`] pour la liste des outils.

pub mod live;
pub mod resources;
pub mod tools;

use serde_json::{Value, json};

/// Versions du protocole que ce serveur sait parler ; la plus récente est proposée par défaut.
const PROTOCOL_VERSIONS: [&str; 3] = ["2024-11-05", "2025-03-26", "2025-06-18"];

/// Contexte d'exécution : racine optionnelle qui confine tous les chemins de fichiers.
#[derive(Clone, Debug, Default)]
pub struct Server {
    pub root: Option<std::path::PathBuf>,
    /// Désactive le mode direct (jamais d'appel à l'application, uniquement les fichiers).
    pub no_live: bool,
    /// Fichier de découverte de l'application (défaut : `live::discovery_path()`).
    pub live_file: Option<std::path::PathBuf>,
    /// Référence des changements par fichier (mode fichier) ; voir `tools::apply_tracked`.
    /// URI des ressources auxquelles le client est abonné.
    pub subs: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    pub trackers: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<std::path::PathBuf, tools::Snapshot>>>,
}

fn ok(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn err(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

impl Server {
    /// Traite un message JSON-RPC ; renvoie la réponse, ou `None` pour une notification.
    pub fn handle(&self, msg: &Value) -> Option<Value> {
        // réponses et notifications (initialized, cancelled…) : rien à renvoyer
        let method = msg.get("method").and_then(Value::as_str)?;
        let id = msg.get("id")?;
        Some(match method {
            "initialize" => {
                let asked = msg.pointer("/params/protocolVersion").and_then(Value::as_str).unwrap_or("");
                let version = PROTOCOL_VERSIONS.iter().find(|v| **v == asked).unwrap_or(&PROTOCOL_VERSIONS[PROTOCOL_VERSIONS.len() - 1]);
                ok(
                    id,
                    json!({
                        "protocolVersion": version,
                        "capabilities": {"tools": {"listChanged": false}, "resources": {"subscribe": true, "listChanged": true}},
                        "serverInfo": {"name": "inkboard-mcp", "title": "Inkboard", "version": env!("CARGO_PKG_VERSION")},
                        "instructions": "Inkboard, tableau blanc hors ligne. Si l'application est ouverte, les outils agissent en direct sur le tableau affiché (sans `path`) ; sinon ils prennent le chemin `path` d'un fichier .inkboard.json (créé par create_board). board_changes signale ce que l'utilisateur a modifié. \
                                         les objets créés renvoient leur `id`, à réutiliser pour relier des formes (add_connector) ou les modifier. \
                                         Sans x/y, un nouvel objet est placé sous le contenu existant. Pour un schéma complet, import_mermaid est le plus rapide."
                    }),
                )
            }
            "ping" => ok(id, json!({})),
            "resources/list" => ok(id, json!({"resources": self.list_resources()})),
            "resources/templates/list" => ok(id, json!({"resourceTemplates": []})),
            "resources/read" => {
                let uri = msg.pointer("/params/uri").and_then(Value::as_str).unwrap_or("");
                match self.read_resource(uri) {
                    Ok(text) => ok(id, json!({"contents": [{"uri": uri, "mimeType": "application/json", "text": text}]})),
                    Err(e) => err(id, -32002, &e),
                }
            }
            "resources/subscribe" | "resources/unsubscribe" => {
                let uri = msg.pointer("/params/uri").and_then(Value::as_str).unwrap_or("").to_owned();
                let mut subs = self.subs.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                if method == "resources/subscribe" {
                    subs.insert(uri);
                } else {
                    subs.remove(&uri);
                }
                ok(id, json!({}))
            }
            "tools/list" => ok(id, json!({"tools": tools::list()})),
            "tools/call" => {
                let name = msg.pointer("/params/name").and_then(Value::as_str).unwrap_or("");
                let args = msg.pointer("/params/arguments").cloned().unwrap_or_else(|| json!({}));
                match tools::call(self, name, &args) {
                    Ok(text) => ok(id, json!({"content": [{"type": "text", "text": text}], "isError": false})),
                    Err(e) if e.starts_with("outil inconnu") => err(id, -32602, &e),
                    Err(e) => ok(id, json!({"content": [{"type": "text", "text": e}], "isError": true})),
                }
            }
            _ => err(id, -32601, &format!("méthode inconnue : {method}")),
        })
    }
}
