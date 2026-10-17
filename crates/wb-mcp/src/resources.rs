//! Ressources MCP : le tableau affiché dans l'application (`tableau-blanc://live`) et les fichiers `.tableau.json` de la racine autorisée,
//! avec abonnement : le serveur prévient le client (`notifications/resources/updated`) quand le contenu change.
//!
//! Le changement est détecté par sondage (toutes les 600 ms) : date de modification d'un fichier, ou « révision + sélection » de l'application.

use crate::Server;
use crate::live::{self, Forward};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::Duration;

/// URI de la ressource « tableau affiché dans l'application ».
pub const LIVE_URI: &str = "tableau-blanc://live";

const MAX_FILES: usize = 300;
const MAX_DEPTH: usize = 3;

/// Encode un chemin en URI `file://` (les caractères hors `A-Za-z0-9-._~/:` sont codés en %XX).
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

/// Chemin désigné par une URI `file://` (`None` pour tout autre schéma).
pub fn uri_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let decoded = String::from_utf8(wb_formats::common::percent_decode(rest)).ok()?;
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
        } else if p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.to_ascii_lowercase().ends_with(".tableau.json")) {
            out.push(p);
        }
    }
}

fn live_available(srv: &Server) -> bool {
    matches!(live::forward(srv, "__status", &json!({})), Forward::Done(Ok(_)))
}

impl Server {
    /// Ressources disponibles : le tableau de l'application s'il est ouvert, et les fichiers `.tableau.json` sous la racine.
    pub fn list_resources(&self) -> Vec<Value> {
        let mut v = Vec::new();
        if live_available(self) {
            v.push(json!({
                "uri": LIVE_URI, "name": "Tableau affiché dans l'application",
                "description": "Contenu du tableau ouvert (page courante) et sélection de l'utilisateur. Abonnez-vous pour être prévenu de ses modifications.",
                "mimeType": "application/json",
            }));
        }
        if let Some(root) = &self.root {
            let mut files = Vec::new();
            collect(root, 0, &mut files);
            for f in files {
                let name = f.file_name().and_then(|n| n.to_str()).unwrap_or("tableau").to_owned();
                v.push(json!({"uri": file_uri(&f), "name": name, "mimeType": "application/json"}));
            }
        }
        v
    }

    /// Contenu JSON d'une ressource (même forme que `describe_board`).
    pub fn read_resource(&self, uri: &str) -> Result<String, String> {
        if uri == LIVE_URI {
            return match live::forward(self, "describe_board", &json!({})) {
                Forward::Done(Ok(v)) => Ok(v.to_string()),
                Forward::Done(Err(e)) => Err(e),
                Forward::NotLive => Err("aucun tableau n'est ouvert dans l'application".into()),
            };
        }
        let path = uri_path(uri).ok_or_else(|| format!("ressource inconnue : {uri}"))?;
        crate::tools::call(self, "describe_board", &json!({"path": path.display().to_string()}))
    }

    /// Valeur qui change quand le contenu de la ressource change (`None` : ressource absente).
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

    /// Lance le fil de surveillance qui envoie les notifications de ressources sur `out` (il s'arrête quand `out` est fermé).
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
                // la première observation après l'abonnement sert de référence : le client vient de lire l'état courant
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
        for p in ["C:\\Tableaux\\Mon schéma.tableau.json", "/home/u/a b/é.tableau.json"] {
            let uri = file_uri(Path::new(p));
            assert!(uri.starts_with("file:///"), "{uri}");
            assert!(!uri.contains(' ') && uri.is_ascii(), "{uri}");
            let back = uri_path(&uri).unwrap();
            assert_eq!(back.display().to_string().replace('\\', "/"), p.replace('\\', "/"), "{uri}");
        }
        assert_eq!(uri_path("https://x"), None);
    }
}
