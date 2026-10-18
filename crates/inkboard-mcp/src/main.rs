//! `inkboard-mcp`: MCP server of the whiteboard on stdin/stdout.
//!
//! ```text
//! inkboard-mcp [--root FOLDER] [--no-live] [--live-file FILE]
//! ```
//!
//! `--root` confines all files read and written under this folder (recommended when an agent drives the server).
//! `--no-live` disables live mode (open application); `--live-file` changes the discovery file.

use inkboard_mcp::Server;
use std::io::{BufRead, Write};

fn main() {
    let mut server = Server::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--root" => match args.next() {
                Some(dir) => server.root = Some(dir.into()),
                None => return eprintln!("--root expects a folder"),
            },
            "--no-live" => server.no_live = true,
            "--live-file" => match args.next() {
                Some(f) => server.live_file = Some(f.into()),
                None => return eprintln!("--live-file expects a file"),
            },
            "-h" | "--help" => return println!("inkboard-mcp [--root FOLDER] [--no-live] [--live-file FILE] — MCP (stdio) server of the whiteboard"),
            other => return eprintln!("unknown argument: {other}"),
        }
    }
    // all outputs (responses and notifications) go through a single writer thread; `Null` ends it
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
            Err(e) => Some(serde_json::json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("invalid JSON: {e}")}})),
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
