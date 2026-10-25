//! MCP (Model Context Protocol) server of the whiteboard, level A: the tools read and write `.inkboard` files (and the older `.inkboard.json` / `.tableau.json`).
//!
//! The protocol is implemented by hand (JSON-RPC 2.0, one JSON message per line on stdin/stdout): `initialize`, `ping`, `tools/list`, `tools/call`.
//! See [`tools`] for the list of tools.
// Test code counts, casts and compares freely: these `clippy::pedantic` lints are for the code that ships.
#![cfg_attr(
    test,
    allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_lossless,
        clippy::float_cmp,
        clippy::many_single_char_names,
        clippy::similar_names,
        clippy::assert_is_empty,
        clippy::default_trait_access,
        clippy::format_collect,
        clippy::format_push_string,
        clippy::too_many_lines,
        clippy::items_after_statements,
        clippy::needless_raw_string_hashes,
        clippy::needless_pass_by_value,
        clippy::explicit_iter_loop,
        clippy::needless_for_each,
        clippy::collapsible_else_if,
        clippy::single_match_else,
        clippy::manual_assert,
        clippy::redundant_closure_for_method_calls,
        clippy::unnested_or_patterns,
        clippy::range_plus_one,
        clippy::clone_on_copy,
        clippy::match_same_arms,
        clippy::struct_excessive_bools
    )
)]

pub mod live;
pub mod resources;
pub mod tools;

use serde_json::{Value, json};

/// Protocol versions this server can speak; the most recent one is offered by default.
const PROTOCOL_VERSIONS: [&str; 3] = ["2024-11-05", "2025-03-26", "2025-06-18"];

/// Resources a client may be subscribed to at once, and the longest URI it may use (the watcher looks at each one every 600 ms).
const MAX_SUBSCRIPTIONS: usize = 256;
const MAX_URI_LEN: usize = 4096;

/// Execution context: optional root that confines all file paths.
#[derive(Clone, Debug, Default)]
pub struct Server {
    pub root: Option<std::path::PathBuf>,
    /// Disables live mode (never calls the application, files only).
    pub no_live: bool,
    /// Application discovery file (default: `live::discovery_path()`).
    pub live_file: Option<std::path::PathBuf>,
    /// Reference of changes per file (file mode); see `tools::apply_tracked`.
    /// URIs of the resources the client is subscribed to.
    pub subs: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    pub trackers: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<std::path::PathBuf, tools::Snapshot>>>,
}

fn ok(id: &Value, result: Value) -> Value {
    let mut v = json!({"jsonrpc": "2.0", "id": id});
    v["result"] = result;
    v
}

fn err(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// MCP content of a tool result: one text block, except when the result is a JSON object with the reserved key `_image`
/// (`{"mime", "data_base64"}`): the content is then an image block followed by a text block holding the rest of the object.
fn content_blocks(text: &str) -> Value {
    if text.contains("\"_image\"")
        && let Ok(Value::Object(mut m)) = serde_json::from_str::<Value>(text)
        && let Some(img) = m.remove("_image")
        && let (Some(mime), Some(data)) = (img["mime"].as_str(), img["data_base64"].as_str())
    {
        return json!([{"type": "image", "data": data, "mimeType": mime}, {"type": "text", "text": Value::Object(m).to_string()}]);
    }
    json!([{"type": "text", "text": text}])
}

impl Server {
    /// Handles a JSON-RPC message; returns the response, or `None` for a notification.
    pub fn handle(&self, msg: &Value) -> Option<Value> {
        // responses and notifications (initialized, cancelled…): nothing to send back
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
                        "instructions": "Inkboard, an offline whiteboard. When the application is open, the tools act live on the displayed board (without `path`); otherwise they take the `path` of an .inkboard file (created by create_board). board_changes reports what the user changed. \
                                         Created objects return their `id`, to reuse for connecting shapes (add_connector) or modifying them. \
                                         Without x/y, a new object is placed below the existing content. For a complete diagram, import_mermaid is the fastest."
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
                    if uri.len() > MAX_URI_LEN || (subs.len() >= MAX_SUBSCRIPTIONS && !subs.contains(&uri)) {
                        return Some(err(id, -32602, &format!("too many subscriptions (at most {MAX_SUBSCRIPTIONS}) or URI too long")));
                    }
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
                    Ok(text) => ok(id, json!({"content": content_blocks(&text), "isError": false})),
                    Err(e) if e.starts_with("unknown tool") => err(id, -32602, &e),
                    Err(e) => ok(id, json!({"content": [{"type": "text", "text": e}], "isError": true})),
                }
            }
            _ => err(id, -32601, &format!("unknown method: {method}")),
        })
    }
}
