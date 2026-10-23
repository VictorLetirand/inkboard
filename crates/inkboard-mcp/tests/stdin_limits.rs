//! What arrives on stdin cannot end the server or fill its memory: a line that is not UTF-8, and a line of 70 MiB, are answered or skipped,
//! and the next request is served.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[test]
fn garbage_lines_do_not_end_the_server_and_an_oversized_line_is_refused_without_being_kept() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_inkboard-mcp"))
        .arg("--no-live")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("inkboard-mcp starts");
    let mut stdin = child.stdin.take().unwrap();
    let stdout = BufReader::new(child.stdout.take().unwrap());
    let writer = std::thread::spawn(move || {
        stdin.write_all(&[0xff, 0xfe, 0xc0, 0x80, b'\n']).unwrap();
        stdin.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n").unwrap();
        let chunk = vec![b'a'; 1 << 20];
        for _ in 0..70 {
            stdin.write_all(&chunk).unwrap();
        }
        stdin.write_all(b"\n").unwrap();
        stdin.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}\n").unwrap();
        // closing stdin ends the server
    });
    let replies: Vec<Value> = stdout.lines().map_while(Result::ok).map(|l| serde_json::from_str(&l).expect("every line of output is JSON")).collect();
    writer.join().unwrap();
    let _ = child.wait();
    let by_id = |id: i64| replies.iter().any(|r| r["id"] == json!(id) && r.get("result").is_some());
    assert!(by_id(1), "the ping after a line that is not UTF-8 is answered: {replies:?}");
    assert!(replies.iter().any(|r| r["error"]["code"] == json!(-32600)), "the oversized line is refused: {replies:?}");
    assert!(by_id(2), "the ping after the oversized line is answered: {replies:?}");
}
