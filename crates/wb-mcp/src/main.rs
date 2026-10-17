//! `wb-mcp` : serveur MCP du tableau blanc sur stdin/stdout.
//!
//! ```text
//! wb-mcp [--root DOSSIER] [--no-live] [--live-file FICHIER]
//! ```
//!
//! `--root` confine tous les fichiers lus et écrits sous ce dossier (recommandé quand un agent pilote le serveur).
//! `--no-live` désactive le mode direct (application ouverte) ; `--live-file` change le fichier de découverte.

use std::io::{BufRead, Write};
use wb_mcp::Server;

fn main() {
    let mut server = Server::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--root" => match args.next() {
                Some(dir) => server.root = Some(dir.into()),
                None => return eprintln!("--root attend un dossier"),
            },
            "--no-live" => server.no_live = true,
            "--live-file" => match args.next() {
                Some(f) => server.live_file = Some(f.into()),
                None => return eprintln!("--live-file attend un fichier"),
            },
            "-h" | "--help" => return println!("wb-mcp [--root DOSSIER] [--no-live] [--live-file FICHIER] — serveur MCP (stdio) du tableau blanc"),
            other => return eprintln!("argument inconnu : {other}"),
        }
    }
    // toutes les sorties (réponses et notifications) passent par un seul fil d'écriture ; `Null` le termine
    let (tx, rx) = std::sync::mpsc::channel::<serde_json::Value>();
    let writer = std::thread::spawn(move || {
        let mut out = std::io::stdout().lock();
        for v in rx {
            if v.is_null() || writeln!(out, "{v}").is_err() || out.flush().is_err() {
                break;
            }
        }
    });
    server.spawn_watcher(tx.clone());
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<serde_json::Value>(&line) {
            Ok(msg) => server.handle(&msg),
            Err(e) => Some(serde_json::json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("JSON invalide : {e}")}})),
        };
        if let Some(r) = reply
            && tx.send(r).is_err()
        {
            break;
        }
    }
    let _ = tx.send(serde_json::Value::Null);
    let _ = writer.join();
}
