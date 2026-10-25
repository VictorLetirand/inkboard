//! Live mode of `inkboard-mcp`: a local server (127.0.0.1, ephemeral port, secret token) that applies an agent's tools to the displayed board.
//!
//! Requests arrive on network threads but are executed **on the UI thread** (`App::poll_live`), in a transaction:
//! one tool call = one undo step (Ctrl+Z). Protocol: see `inkboard_mcp::live`.

use crate::app::App;
use inkboard_core::Id;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

/// The interface's reply to a request.
#[derive(Debug)]
pub enum Reply {
    Ok(Value),
    Err(String),
    /// The application displays a different file than the requested one.
    OtherFile,
}

/// Tool request waiting to be executed on the UI thread.
#[derive(Debug)]
pub struct Request {
    pub tool: String,
    pub args: Value,
    pub reply: mpsc::Sender<Reply>,
}

#[derive(Debug)]
pub struct LiveServer {
    rx: mpsc::Receiver<Request>,
    token: String,
    file: PathBuf,
}

fn new_token() -> String {
    use std::fmt::Write;
    use std::hash::{BuildHasher, Hasher};
    (0..4).fold(String::with_capacity(64), |mut s, _| {
        let _ = write!(s, "{:016x}", std::collections::hash_map::RandomState::new().build_hasher().finish());
        s
    })
}

fn same_token(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

impl LiveServer {
    /// Opens the server and publishes `{port, token, pid}` in `file` (discovery file read by `inkboard-mcp`).
    pub fn start(ctx: eframe::egui::Context, file: PathBuf) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        let token = new_token();
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&file, json!({"port": port, "token": token, "pid": std::process::id()}).to_string())?;
        let (tx, rx) = mpsc::channel();
        let t = token.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (tx, ctx, t) = (tx.clone(), ctx.clone(), t.clone());
                std::thread::spawn(move || serve(&stream, &tx, &ctx, &t));
            }
        });
        Ok(Self { rx, token, file })
    }

    fn poll(&self) -> Option<Request> {
        self.rx.try_recv().ok()
    }
}

impl Drop for LiveServer {
    fn drop(&mut self) {
        // remove the discovery file if it is still ours (another instance may have replaced it)
        if std::fs::read_to_string(&self.file).is_ok_and(|s| s.contains(&self.token)) {
            let _ = std::fs::remove_file(&self.file);
        }
    }
}

fn respond(mut s: &TcpStream, v: &Value) {
    let _ = writeln!(s, "{v}");
    let _ = s.flush();
}

fn serve(stream: &TcpStream, tx: &mpsc::Sender<Request>, ctx: &eframe::egui::Context, token: &str) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut line = String::new();
    // a request fits on one line; 16 MiB at most
    if BufReader::new(stream.take(16 << 20)).read_line(&mut line).is_err() {
        return;
    }
    let Ok(req) = serde_json::from_str::<Value>(&line) else { return respond(stream, &json!({"ok": false, "error": "invalid request"})) };
    if !req.get("token").and_then(Value::as_str).is_some_and(|t| same_token(t, token)) {
        return respond(stream, &json!({"ok": false, "error": "token refused"}));
    }
    let tool = req.get("tool").and_then(Value::as_str).unwrap_or("").to_owned();
    let args = req.get("args").cloned().unwrap_or_else(|| json!({}));
    let (rtx, rrx) = mpsc::channel();
    if tx.send(Request { tool, args, reply: rtx }).is_err() {
        return respond(stream, &json!({"ok": false, "error": "application is closing"}));
    }
    ctx.request_repaint();
    let out = match rrx.recv_timeout(Duration::from_secs(30)) {
        Ok(Reply::Ok(v)) => json!({"ok": true, "result": v}),
        Ok(Reply::Err(e)) => json!({"ok": false, "error": e}),
        Ok(Reply::OtherFile) => json!({"ok": false, "other_file": true}),
        Err(_) => json!({"ok": false, "error": "the application did not answer (window busy or stuck)"}),
    };
    respond(stream, &out);
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

impl App {
    /// Call on every frame: executes the pending agent requests.
    pub fn poll_live(&mut self) {
        while let Some(req) = self.live.as_ref().and_then(LiveServer::poll) {
            let reply = self.run_live(&req.tool, &req.args);
            let _ = req.reply.send(reply);
        }
    }

    fn run_live(&mut self, tool: &str, args: &Value) -> Reply {
        if tool == "__status" {
            // polled by inkboard-mcp for resource notifications: changes when the board or the selection changes
            return Reply::Ok(json!({
                "revision": self.ed.revision(),
                "path": self.path.as_ref().map(|p| p.display().to_string()),
                "selection": self.sel.iter().map(|i| i.0.clone()).collect::<Vec<_>>(),
            }));
        }
        if let Some(p) = args.get("path").and_then(Value::as_str)
            && !self.path.as_deref().is_some_and(|mine| same_file(mine, Path::new(p)))
        {
            return Reply::OtherFile;
        }
        self.commit_editing();
        if tool == "save_board" {
            return match self.path.clone() {
                Some(p) => {
                    self.save();
                    Reply::Ok(json!({"saved": true, "path": p.display().to_string()}))
                }
                None => Reply::Err("the board has no file yet: the user must save it (Ctrl+S) first".into()),
            };
        }
        let was_empty = self.ed.page().objects.is_empty();
        let tracker = &mut self.tracker;
        let result = self.ed.transact(|ed| inkboard_mcp::tools::apply_tracked(ed, tool, args, tracker));
        match result {
            Ok(mut v) => {
                // the user's selection follows what the agent has just created
                let created: Vec<Id> = match (&v["id"], &v["ids"]) {
                    (Value::String(i), _) => vec![Id(i.clone())],
                    (_, Value::Array(a)) => a.iter().filter_map(|i| i.as_str().map(|s| Id(s.to_owned()))).collect(),
                    _ => Vec::new(),
                };
                if !created.is_empty() {
                    self.sel = created;
                }
                self.sel.retain(|i| self.ed.page().get(i).is_some());
                if was_empty && !self.ed.page().objects.is_empty() {
                    self.fit_pending = true;
                }
                if tool == "describe_board" || tool == "board_changes" {
                    v["live"] = json!(true);
                    v["selection"] = json!(self.sel.iter().map(|i| i.0.clone()).collect::<Vec<_>>());
                } else {
                    self.toast(&format!("Agent : {tool}"));
                }
                Reply::Ok(v)
            }
            Err(e) => Reply::Err(e),
        }
    }
}
