//! Confinement of paths to `--root`, with the real `inkboard-mcp` binary.

mod common;

use common::{Client, temp_dir};
use serde_json::json;
use std::path::Path;

const OUTSIDE: &str = "outside the allowed root";

/// Creates a symbolic link to a file or folder; `false` if the system refuses (e.g. Windows without the privilege).
fn symlink(target: &Path, link: &Path, dir: bool) -> bool {
    #[cfg(windows)]
    let r = if dir { std::os::windows::fs::symlink_dir(target, link) } else { std::os::windows::fs::symlink_file(target, link) };
    #[cfg(unix)]
    let r = {
        let _ = dir;
        std::os::unix::fs::symlink(target, link)
    };
    match r {
        Ok(()) => true,
        Err(e) => {
            eprintln!("SKIPPED: cannot create a symbolic link here ({e})");
            false
        }
    }
}

#[test]
fn dot_dot_escape_is_refused() {
    let root = temp_dir("sec-dd");
    let mut c = Client::start(&root);
    let err = c.tool("create_board", json!({"path": "../evil.inkboard.json"})).unwrap_err();
    assert!(err.contains(OUTSIDE), "{err}");
    let err = c.tool("create_board", json!({"path": "sub/../../evil.inkboard.json"})).unwrap_err();
    assert!(err.contains(OUTSIDE) || err.contains("not accessible"), "{err}");
    assert!(!root.parent().unwrap().join("evil.inkboard.json").exists());
}

#[test]
fn absolute_path_outside_the_root_is_refused() {
    let root = temp_dir("sec-abs");
    let outside = temp_dir("sec-abs-out");
    let mut c = Client::start(&root);
    let target = outside.join("evil.inkboard.json");
    let err = c.tool("create_board", json!({"path": target.to_str().unwrap()})).unwrap_err();
    assert!(err.contains(OUTSIDE), "{err}");
    assert!(!target.exists());
    // the same for the output of an export and the source of an image
    c.tool("create_board", json!({"path": "b.inkboard.json"})).unwrap();
    let err = c.tool("export", json!({"path": "b.inkboard.json", "output": outside.join("o.svg").to_str().unwrap()})).unwrap_err();
    assert!(err.contains(OUTSIDE), "{err}");
    assert!(!outside.join("o.svg").exists());
}

#[test]
fn symlinked_folder_pointing_outside_the_root_is_refused() {
    let root = temp_dir("sec-dir");
    let outside = temp_dir("sec-dir-out");
    if !symlink(&outside, &root.join("link"), true) {
        return;
    }
    let mut c = Client::start(&root);
    let err = c.tool("create_board", json!({"path": "link/evil.inkboard.json"})).unwrap_err();
    assert!(err.contains(OUTSIDE), "{err}");
    assert!(!outside.join("evil.inkboard.json").exists());
}

/// A real board outside the root, reachable through `link.inkboard.json` inside the root. `None` if links cannot be created.
fn root_with_link_to_outside_file(tag: &str) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let root = temp_dir(&format!("{tag}-root"));
    let outside = temp_dir(&format!("{tag}-out"));
    let secret = outside.join("secret.inkboard.json");
    {
        let mut c = Client::start(&outside);
        c.tool("create_board", json!({"path": secret.to_str().unwrap()})).unwrap();
    }
    symlink(&secret, &root.join("link.inkboard.json"), false).then_some((root, secret))
}

// A final file that is a link to a file outside the root must be refused (reading and writing).
#[test]
fn symlinked_final_file_pointing_outside_the_root_is_refused() {
    let Some((root, secret)) = root_with_link_to_outside_file("sec-file") else { return };
    let mut c = Client::start(&root);
    let read = c.tool("describe_board", json!({"path": "link.inkboard.json"}));
    let write = c.tool("add_shape", json!({"path": "link.inkboard.json", "geom": "rect", "text": "pwned"}));
    assert!(read.is_err() && write.is_err(), "link to a file outside the root accepted: read {read:?}, write {write:?}");
    assert!(secret.exists());
}

#[test]
fn symlinked_final_file_pointing_outside_the_root_is_never_modified() {
    let Some((root, secret)) = root_with_link_to_outside_file("sec-file-w") else { return };
    let before = std::fs::read(&secret).unwrap();
    let mut c = Client::start(&root);
    let _ = c.tool("add_shape", json!({"path": "link.inkboard.json", "geom": "rect", "text": "pwned"}));
    assert_eq!(std::fs::read(&secret).unwrap(), before, "the file outside the root was modified through the link");
}
