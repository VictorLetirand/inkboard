//! MCP resources: the board shown in the application (`inkboard://live`) and the `.inkboard.json` files of the allowed root,
//! with subscription: the server notifies the client (`notifications/resources/updated`) when the content changes.
//!
//! Change is detected by polling (every 600 ms): a file's modification time, or the application's "revision + selection".

use crate::Server;
use crate::live::{self, Forward};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::Duration;

/// URI of the "board shown in the application" resource.
pub const LIVE_URI: &str = "inkboard://live";

const MAX_FILES: usize = 300;
const MAX_DEPTH: usize = 3;

/// Encodes a path as a `file://` URI (characters outside `A-Za-z0-9-._~/:` are %XX-encoded).
pub fn file_uri(p: &Path) -> String {
    let s = p.display().to_string().replace('\\', "/");
    let mut out = String::from("file://");
    if !s.starts_with('/') {
        out.push('/');
    }
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/:".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Path designated by a `file://` URI (`None` for any other scheme).
pub fn uri_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let decoded = String::from_utf8(inkboard_formats::common::percent_decode(rest)).ok()?;
    // `/E:/dossier` (Windows) → `E:/dossier`
    let b = decoded.as_bytes();
    let p = if b.len() > 2 && b[0] == b'/' && b[2] == b':' && b[1].is_ascii_alphabetic() { &decoded[1..] } else { &decoded };
    Some(PathBuf::from(p))
}

fn collect(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        if out.len() >= MAX_FILES {
            return;
        }
        let p = e.path();
        if p.is_dir() && depth < MAX_DEPTH {
            collect(&p, depth + 1, out);
        } else if p.file_name().and_then(|n| n.to_str()).is_some_and(|n| crate::tools::is_native_name(&n.to_ascii_lowercase())) {
            out.push(p);
        }
    }
}

fn live_available(srv: &Server) -> bool {
    matches!(live::forward(srv, "__status", &json!({})), Forward::Done(Ok(_)))
}

impl Server {
    /// Available resources: the application's board if it is open, and the `.inkboard.json` files under the root.
    pub fn list_resources(&self) -> Vec<Value> {
        let mut v = Vec::new();
        if live_available(self) {
            v.push(json!({
                "uri": LIVE_URI, "name": "Board shown in the application",
                "description": "Content of the open board (current page) and the user's selection. Subscribe to be notified of its changes.",
                "mimeType": "application/json",
            }));
        }
        if let Some(root) = &self.root {
            let mut files = Vec::new();
            collect(root, 0, &mut files);
            for f in files {
                let name = f.file_name().and_then(|n| n.to_str()).unwrap_or("board").to_owned();
                v.push(json!({"uri": file_uri(&f), "name": name, "mimeType": "application/json"}));
            }
        }
        v
    }

    /// JSON content of a resource (same shape as `describe_board`).
    pub fn read_resource(&self, uri: &str) -> Result<String, String> {
        if uri == LIVE_URI {
            return match live::forward(self, "describe_board", &json!({})) {
                Forward::Done(Ok(v)) => Ok(v.to_string()),
                Forward::Done(Err(e)) => Err(e),
                Forward::NotLive => Err("no board is open in the application".into()),
            };
        }
        let path = uri_path(uri).ok_or_else(|| format!("unknown resource: {uri}"))?;
        crate::tools::call(self, "describe_board", &json!({"path": path.display().to_string()}))
    }

    /// Value that changes when the resource's content changes (`None`: resource absent).
    fn change_token(&self, uri: &str) -> String {
        if uri == LIVE_URI {
            return match live::forward(self, "__status", &json!({})) {
                Forward::Done(Ok(v)) => v.to_string(),
                _ => "absent".into(),
            };
        }
        uri_path(uri)
            .and_then(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or_else(|| "absent".into(), |d| d.as_nanos().to_string())
    }

    /// Starts the watcher thread that sends resource notifications on `out` (it stops when `out` is closed).
    pub fn spawn_watcher(&self, out: Sender<Value>) {
        let srv = self.clone();
        std::thread::spawn(move || srv.watch(&out));
    }

    fn watch(&self, out: &Sender<Value>) {
        let mut last: HashMap<String, String> = HashMap::new();
        let mut list_sig: Option<String> = None;
        let mut tick = 0u32;
        loop {
            std::thread::sleep(Duration::from_millis(600));
            tick += 1;
            let subs: Vec<String> = self.subs.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().cloned().collect();
            last.retain(|u, _| subs.contains(u));
            for uri in subs {
                let token = self.change_token(&uri);
                // the first observation after subscribing serves as the reference: the client has just read the current state
                if last.insert(uri.clone(), token.clone()).is_some_and(|old| old != token)
                    && out.send(json!({"jsonrpc": "2.0", "method": "notifications/resources/updated", "params": {"uri": uri}})).is_err()
                {
                    return;
                }
            }
            if tick.is_multiple_of(3) {
                let sig = self.list_resources().iter().filter_map(|r| r["uri"].as_str()).collect::<Vec<_>>().join("\n");
                if list_sig.as_ref().is_some_and(|old| *old != sig)
                    && out.send(json!({"jsonrpc": "2.0", "method": "notifications/resources/list_changed"})).is_err()
                {
                    return;
                }
                list_sig = Some(sig);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uris_round_trip_including_spaces_and_accents() {
        for p in ["C:\\Boards\\My diagram.inkboard.json", "/home/u/a b/é.inkboard.json"] {
            let uri = file_uri(Path::new(p));
            assert!(uri.starts_with("file:///"), "{uri}");
            assert!(!uri.contains(' ') && uri.is_ascii(), "{uri}");
            let back = uri_path(&uri).unwrap();
            assert_eq!(back.display().to_string().replace('\\', "/"), p.replace('\\', "/"), "{uri}");
        }
        assert_eq!(uri_path("https://x"), None);
    }
}
