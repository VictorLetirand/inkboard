//! `wb-mcp` : serveur MCP du tableau blanc sur stdin/stdout.
//!
//! ```text
//! wb-mcp [--root DOSSIER]
//! ```
//!
//! `--root` confine tous les fichiers lus et écrits sous ce dossier (recommandé quand un agent pilote le serveur).

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
            "-h" | "--help" => return println!("wb-mcp [--root DOSSIER] — serveur MCP (stdio) du tableau blanc"),
            other => return eprintln!("argument inconnu : {other}"),
        }
    }
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
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
            && (writeln!(out, "{r}").is_err() || out.flush().is_err())
        {
            break;
        }
    }
}
