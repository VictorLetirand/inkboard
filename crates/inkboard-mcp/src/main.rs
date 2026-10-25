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

/// Longest message accepted on stdin (an image of 15 MiB in base64 fits): a longer line is skipped without being kept in memory.
const MAX_LINE_BYTES: usize = 64 << 20;

enum Line {
    End,
    TooLong,
    Text(String),
}

/// Reads one line, keeping at most `MAX_LINE_BYTES` of it (invalid UTF-8 is replaced, it does not end the server).
fn read_line(input: &mut impl BufRead) -> Line {
    let mut buf: Vec<u8> = Vec::new();
    let mut too_long = false;
    loop {
        let Ok(chunk) = input.fill_buf() else { return Line::End };
        if chunk.is_empty() {
            // end of input: what was read is a last line without a newline
            return if too_long {
                Line::TooLong
            } else if buf.is_empty() {
                Line::End
            } else {
                Line::Text(String::from_utf8_lossy(&buf).into_owned())
            };
        }
        let newline = chunk.iter().position(|b| *b == b'\n');
        let take = newline.unwrap_or(chunk.len());
        if !too_long {
            if buf.len() + take > MAX_LINE_BYTES {
                too_long = true;
                buf = Vec::new();
            } else {
                buf.extend_from_slice(&chunk[..take]);
            }
        }
        input.consume(take + usize::from(newline.is_some()));
        if newline.is_some() {
            return if too_long { Line::TooLong } else { Line::Text(String::from_utf8_lossy(&buf).into_owned()) };
        }
    }
}

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
    let mut input = stdin.lock();
    loop {
        let line = match read_line(&mut input) {
            Line::End => break,
            Line::TooLong => {
                let reply = serde_json::json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32600, "message": format!("message too large (more than {MAX_LINE_BYTES} bytes)")}});
                if tx.send(reply).is_err() {
                    break;
                }
                continue;
            }
            Line::Text(line) => line,
        };
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
