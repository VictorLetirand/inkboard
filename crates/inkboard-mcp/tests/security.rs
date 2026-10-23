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
    let err = c.tool("create_board", json!({"path": "../evil.inkboard"})).unwrap_err();
    assert!(err.contains(OUTSIDE), "{err}");
    let err = c.tool("create_board", json!({"path": "sub/../../evil.inkboard"})).unwrap_err();
    assert!(err.contains(OUTSIDE) || err.contains("not accessible"), "{err}");
    assert!(!root.parent().unwrap().join("evil.inkboard").exists());
}

#[test]
fn absolute_path_outside_the_root_is_refused() {
    let root = temp_dir("sec-abs");
    let outside = temp_dir("sec-abs-out");
    let mut c = Client::start(&root);
    let target = outside.join("evil.inkboard");
    let err = c.tool("create_board", json!({"path": target.to_str().unwrap()})).unwrap_err();
    assert!(err.contains(OUTSIDE), "{err}");
    assert!(!target.exists());
    // the same for the output of an export and the source of an image
    c.tool("create_board", json!({"path": "b.inkboard"})).unwrap();
    let err = c.tool("export", json!({"path": "b.inkboard", "output": outside.join("o.svg").to_str().unwrap()})).unwrap_err();
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
    let err = c.tool("create_board", json!({"path": "link/evil.inkboard"})).unwrap_err();
    assert!(err.contains(OUTSIDE), "{err}");
    assert!(!outside.join("evil.inkboard").exists());
}

/// A real board outside the root, reachable through `link.inkboard` inside the root. `None` if links cannot be created.
fn root_with_link_to_outside_file(tag: &str) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let root = temp_dir(&format!("{tag}-root"));
    let outside = temp_dir(&format!("{tag}-out"));
    let secret = outside.join("secret.inkboard");
    {
        let mut c = Client::start(&outside);
        c.tool("create_board", json!({"path": secret.to_str().unwrap()})).unwrap();
    }
    symlink(&secret, &root.join("link.inkboard"), false).then_some((root, secret))
}

// A final file that is a link to a file outside the root must be refused (reading and writing).
#[test]
fn symlinked_final_file_pointing_outside_the_root_is_refused() {
    let Some((root, secret)) = root_with_link_to_outside_file("sec-file") else { return };
    let mut c = Client::start(&root);
    let read = c.tool("describe_board", json!({"path": "link.inkboard"}));
    let write = c.tool("add_shape", json!({"path": "link.inkboard", "geom": "rect", "text": "pwned"}));
    assert!(read.is_err() && write.is_err(), "link to a file outside the root accepted: read {read:?}, write {write:?}");
    assert!(secret.exists());
}

#[test]
fn symlinked_final_file_pointing_outside_the_root_is_never_modified() {
    let Some((root, secret)) = root_with_link_to_outside_file("sec-file-w") else { return };
    let before = std::fs::read(&secret).unwrap();
    let mut c = Client::start(&root);
    let _ = c.tool("add_shape", json!({"path": "link.inkboard", "geom": "rect", "text": "pwned"}));
    assert_eq!(std::fs::read(&secret).unwrap(), before, "the file outside the root was modified through the link");
}

/// The confinement does not depend on the extension: links named `.inkboard.json` or `.tableau.json` are refused too.
#[test]
fn symlinks_with_the_older_native_names_are_refused_too() {
    let root = temp_dir("sec-old-root");
    let outside = temp_dir("sec-old-out");
    let secret = outside.join("secret.inkboard");
    {
        let mut c = Client::start(&outside);
        c.tool("create_board", json!({"path": secret.to_str().unwrap()})).unwrap();
    }
    let before = std::fs::read(&secret).unwrap();
    let mut c = Client::start(&root);
    for name in ["old.inkboard.json", "older.tableau.json"] {
        if !symlink(&secret, &root.join(name), false) {
            return;
        }
        assert!(c.tool("describe_board", json!({"path": name})).is_err(), "{name}: read through the link accepted");
        assert!(c.tool("add_shape", json!({"path": name, "geom": "rect", "text": "pwned"})).is_err(), "{name}: write through the link accepted");
    }
    assert_eq!(std::fs::read(&secret).unwrap(), before);
}

/* ------------------------------------------------------------------ batch */

const BATCH_PATHS: &str = "paths cannot be set inside a batch operation";

#[test]
fn batch_operations_cannot_carry_path_output_or_source() {
    let root = temp_dir("sec-batch");
    let outside = temp_dir("sec-batch-out");
    let secret = outside.join("secret.inkboard");
    {
        let mut c = Client::start(&outside);
        c.tool("create_board", json!({"path": secret.to_str().unwrap()})).unwrap();
    }
    let secret_before = std::fs::read(&secret).unwrap();
    let mut c = Client::start(&root);
    c.tool("create_board", json!({"path": "b.inkboard"})).unwrap();
    let board_before = std::fs::read(root.join("b.inkboard")).unwrap();
    let out = secret.to_str().unwrap();
    let svg = outside.join("o.svg");
    let cases = [
        // first level of the arguments, where `call()` does not look for a batch operation
        json!({"tool": "add_shape", "args": {"geom": "rect", "path": out}}),
        json!({"tool": "add_image", "args": {"source": out}}),
        json!({"tool": "describe_board", "args": {"path": out}}),
        json!({"tool": "add_shape", "args": {"geom": "rect", "output": svg.to_str().unwrap()}}),
        // next to `tool`, and nested deeper
        json!({"tool": "describe_board", "path": out}),
        json!({"tool": "add_image", "source": out}),
        json!({"tool": "add_shape", "args": {"geom": "rect", "extra": [{"deep": {"source": out}}]}}),
    ];
    for op in cases {
        let ops = json!([{"tool": "add_shape", "args": {"geom": "rect", "text": "must not appear"}}, op.clone()]);
        let e = c.tool("batch", json!({"path": "b.inkboard", "operations": ops})).unwrap_err();
        assert!(e.contains(BATCH_PATHS), "{op}: {e}");
        assert_eq!(std::fs::read(&secret).unwrap(), secret_before, "{op}: the board outside the root was touched");
        assert_eq!(std::fs::read(root.join("b.inkboard")).unwrap(), board_before, "{op}: the first operation ran");
    }
    assert!(!svg.exists());
    // the non-atomic mode is no way around it
    let e = c.tool("batch", json!({"path": "b.inkboard", "atomic": false, "operations": [{"tool": "describe_board", "args": {"path": out}}]})).unwrap_err();
    assert!(e.contains(BATCH_PATHS), "{e}");
    assert_eq!(c.request("ping", json!({}))["result"], json!({}));
}

#[test]
fn batch_level_path_is_confined_like_any_other() {
    let root = temp_dir("sec-batch-lvl");
    let outside = temp_dir("sec-batch-lvl-out");
    let target = outside.join("evil.inkboard");
    let mut c = Client::start(&root);
    let e = c.tool("batch", json!({"path": target.to_str().unwrap(), "operations": [{"tool": "add_shape", "args": {"geom": "rect"}}]})).unwrap_err();
    assert!(e.contains(OUTSIDE), "{e}");
    assert!(!target.exists());
    let e = c.tool("batch", json!({"path": "../evil.inkboard", "operations": [{"tool": "add_shape", "args": {"geom": "rect"}}]})).unwrap_err();
    assert!(e.contains(OUTSIDE) || e.contains("not accessible"), "{e}");
}

#[test]
fn image_size_limit_still_applies_inside_a_batch() {
    use base64::Engine;
    let root = temp_dir("sec-batch-img");
    let mut c = Client::start(&root);
    c.tool("create_board", json!({"path": "b.inkboard"})).unwrap();
    let before = std::fs::read(root.join("b.inkboard")).unwrap();
    // a PNG signature followed by zeros, one byte over the 15 MiB limit
    let mut data = vec![0u8; (15 << 20) + 1];
    data[..4].copy_from_slice(&[0x89, b'P', b'N', b'G']);
    let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
    let e = c.tool("batch", json!({"path": "b.inkboard", "operations": [{"tool": "add_image", "args": {"data_base64": b64}}]})).unwrap_err();
    assert!(e.contains("image too large"), "{}", &e[..e.len().min(300)]);
    assert_eq!(std::fs::read(root.join("b.inkboard")).unwrap(), before);
    assert_eq!(c.request("ping", json!({}))["result"], json!({}));
}
