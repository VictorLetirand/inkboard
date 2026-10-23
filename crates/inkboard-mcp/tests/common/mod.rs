//! Shared by the integration tests: a minimal MCP client driving the real `inkboard-mcp` binary.
#![allow(dead_code)] // each test file compiles this module on its own and uses only part of it

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

pub struct Client {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<Value>,
    /// Messages received while waiting for a response (server notifications).
    pending: Vec<Value>,
    next: u64,
    /// How long `request` waits for a response (a slow CI runner may need more for a heavy call).
    pub timeout: Duration,
}

impl Client {
    pub fn start(root: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_inkboard-mcp"))
            .arg("--root")
            .arg(root)
            .arg("--no-live")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("lancement de inkboard-mcp");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        // a thread reads the lines so we can wait with a timeout
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for l in stdout.lines().map_while(Result::ok) {
                let v: Value = serde_json::from_str(&l).unwrap_or_else(|e| panic!("invalid output ({e}): {l}"));
                if tx.send(v).is_err() {
                    break;
                }
            }
        });
        Self { child, stdin, lines, pending: Vec::new(), next: 1, timeout: Duration::from_secs(20) }
    }

    pub fn send(&mut self, v: &Value) {
        writeln!(self.stdin, "{v}").unwrap();
        self.stdin.flush().unwrap();
    }

    /// Sends a raw line, which may not be valid JSON.
    pub fn send_raw(&mut self, line: &str) {
        writeln!(self.stdin, "{line}").unwrap();
        self.stdin.flush().unwrap();
    }

    pub fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        let mut msg = json!({"jsonrpc": "2.0", "id": id, "method": method});
        msg["params"] = params;
        self.send(&msg);
        loop {
            let v = self.lines.recv_timeout(self.timeout).expect("response expected");
            if v["id"] == json!(id) {
                return v;
            }
            self.pending.push(v);
        }
    }

    /// Waits for a `method` notification (carrying `uri` if given).
    pub fn wait_notification(&mut self, method: &str, uri: Option<&str>, timeout: Duration) -> Option<Value> {
        let matches = |v: &Value| v["method"] == json!(method) && uri.is_none_or(|u| v["params"]["uri"] == json!(u));
        if let Some(i) = self.pending.iter().position(matches) {
            return Some(self.pending.remove(i));
        }
        let end = Instant::now() + timeout;
        while let Some(left) = end.checked_duration_since(Instant::now()) {
            match self.lines.recv_timeout(left) {
                Ok(v) if matches(&v) => return Some(v),
                Ok(v) => self.pending.push(v),
                Err(_) => break,
            }
        }
        None
    }

    /// Calls a tool; returns the result JSON or the error message.
    pub fn tool(&mut self, name: &str, args: Value) -> Result<Value, String> {
        let mut params = json!({"name": name});
        params["arguments"] = args;
        let r = self.request("tools/call", params);
        let text = r["result"]["content"][0]["text"].as_str().unwrap_or_else(|| panic!("{r}")).to_owned();
        if r["result"]["isError"] == json!(true) { Err(text) } else { Ok(serde_json::from_str(&text).unwrap_or(Value::String(text))) }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

pub fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("inkboard-mcp-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.canonicalize().unwrap()
}
