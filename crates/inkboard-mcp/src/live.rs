//! Live mode: tools are forwarded to the open application, which applies them to its own board (undo history, selection and
//! view preserved). The application publishes a local server (127.0.0.1) and a discovery file containing the port and a secret token.
//!
//! Protocol: one JSON line per request `{"token", "tool", "args"}`, one JSON line per response `{"ok": true, "result"}`,
//! `{"ok": false, "error"}` or `{"ok": false, "other_file": true}` (the application shows a file other than `args.path`).

use crate::Server;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Application data folder: `%APPDATA%\Inkboard` (Windows), `~/Library/Application Support/Inkboard` (macOS),
/// `$XDG_DATA_HOME/Inkboard` or `~/.local/share/Inkboard` (Linux); the current folder as a last resort.
pub fn data_dir() -> PathBuf {
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        home().map(|h| h.join("Library").join("Application Support"))
    } else {
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).or_else(|| home().map(|h| h.join(".local").join("share")))
    };
    base.unwrap_or_else(|| PathBuf::from(".")).join("Inkboard")
}

/// Discovery file: `live.json` in the data folder.
pub fn discovery_path() -> PathBuf {
    data_dir().join("live.json")
}

/// Port and token published by the application.
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    pub port: u16,
    pub token: String,
}

pub fn read_info(file: &Path) -> Option<Info> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()?;
    Some(Info { port: u16::try_from(v.get("port")?.as_u64()?).ok()?, token: v.get("token")?.as_str()?.to_owned() })
}

/// Result of a forwarding attempt.
#[derive(Debug)]
pub enum Forward {
    /// The application handled the request (tool success or error).
    Done(Result<Value, String>),
    /// No reachable application, or it shows another file: file mode takes over.
    NotLive,
}

fn request(info: &Info, req: &Value) -> std::io::Result<Value> {
    let mut s = TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], info.port)), Duration::from_millis(500))?;
    s.set_read_timeout(Some(Duration::from_secs(30)))?;
    writeln!(s, "{req}")?;
    s.flush()?;
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line)?;
    serde_json::from_str(&line).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// Forwards the tool to the application if it is open (and shows the file `args.path`, if given).
pub fn forward(srv: &Server, tool: &str, args: &Value) -> Forward {
    if srv.no_live {
        return Forward::NotLive;
    }
    let file = srv.live_file.clone().unwrap_or_else(discovery_path);
    let Some(info) = read_info(&file) else { return Forward::NotLive };
    match request(&info, &json!({"token": info.token, "tool": tool, "args": args})) {
        Err(_) => Forward::NotLive, // application closed, stale discovery file
        Ok(r) if r["other_file"] == json!(true) => Forward::NotLive,
        Ok(r) if r["ok"] == json!(true) => Forward::Done(Ok(r["result"].clone())),
        Ok(r) => Forward::Done(Err(r["error"].as_str().unwrap_or("application error").to_owned())),
    }
}
