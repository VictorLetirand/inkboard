//! Consistency of the release metadata: installer version, license and license files.

use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn read(name: &str) -> String {
    std::fs::read_to_string(root().join(name)).unwrap_or_else(|e| panic!("{name} : {e}"))
}

/// Value of a top-level `key = "value"` entry.
fn value(toml: &str, key: &str) -> String {
    toml.lines()
        .find_map(|l| l.strip_prefix(key).and_then(|r| r.trim_start().strip_prefix('=')).map(|r| r.trim().trim_matches('"').to_owned()))
        .unwrap_or_else(|| panic!("key {key} is missing"))
}

#[test]
fn installer_version_matches_the_workspace_version() {
    let ws = value(&read("Cargo.toml"), "version");
    let pk = value(&read("Packager.toml"), "version");
    assert_eq!(pk, ws, "update `version` in Packager.toml together with Cargo.toml");
}

#[test]
fn project_is_dual_licensed_mit_or_apache_with_both_texts_present() {
    assert_eq!(value(&read("Cargo.toml"), "license"), "MIT OR Apache-2.0");
    assert!(read("LICENSE-MIT").contains("Permission is hereby granted, free of charge"));
    assert!(read("LICENSE-APACHE").contains("Apache License") && read("LICENSE-APACHE").contains("Version 2.0, January 2004"));
    let combined = read("packaging/LICENSE.txt");
    assert!(combined.contains("MIT") && combined.contains("Apache License"), "the text shown by the installer contains both licenses");
    assert!(read("THIRD_PARTY_LICENSES.md").contains("STIX Two Math"), "the embedded font is credited");
}

/// The `.inkboard` association is declared for cargo-packager (Linux, macOS) and for Installway (Windows, the only Windows installer).
#[test]
fn installers_declare_the_file_association() {
    let pk = read("Packager.toml");
    assert!(pk.contains("extensions = [\"inkboard\"]"), "the .inkboard association is declared");
    assert!(!pk.contains("[nsis]") && !pk.contains("[wix]"), "the Windows installer is built by Installway, not by cargo-packager");

    let iw = read("installway.toml");
    assert!(iw.contains("assoc = [\".inkboard:Inkboard board\"]"), "Installway: the .exe registers the .inkboard association");
    assert_eq!(value(&iw, "exe"), "inkboard.exe", "Installway: the association opens the application");
}

/// Targets of the released binaries (same list as `scripts/gen_third_party.py`).
const RELEASE_TARGETS: [&str; 4] = ["x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu", "aarch64-apple-darwin", "x86_64-apple-darwin"];

/// `(name, version)` of the third-party crates compiled into the binaries for one target (normal and build dependencies, no dev dependencies).
/// Prerequisite: `cargo fetch --locked` (offline, `cargo tree` needs the sources of the crates of the other platforms too; the CI runs it before the tests).
fn embedded_crates(target: &str) -> std::collections::BTreeSet<(String, String)> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let out = std::process::Command::new(cargo)
        .args(["tree", "--workspace", "-e", "normal,build", "--prefix", "none", "-f", "{p}", "--offline", "--locked", "--target", target])
        .current_dir(root())
        .output()
        .expect("cargo tree can be started");
    assert!(
        out.status.success(),
        "cargo tree --target {target} failed: {}
If a crate could not be downloaded (--offline), run `cargo fetch --locked` first: the test needs the sources of the crates of every release target.",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let (name, version, rest) = (it.next()?, it.next()?.strip_prefix('v')?, it.next());
            // Workspace crates are shown with their path in parentheses; registry crates have none.
            let local = rest.is_some_and(|r| r.starts_with('(') && !r.starts_with("(proc-macro") && !r.starts_with("(*"));
            (!local).then(|| (name.to_owned(), version.to_owned()))
        })
        .collect()
}

#[test]
fn third_party_licenses_list_every_crate_embedded_in_the_binaries() {
    let doc = read("THIRD_PARTY_LICENSES.md");
    let listed: std::collections::BTreeSet<(String, String)> = doc
        .lines()
        .filter(|l| l.starts_with("| "))
        .filter_map(|l| {
            let mut cells = l.split('|').map(str::trim).skip(1);
            Some((cells.next()?.to_owned(), cells.next()?.to_owned()))
        })
        .collect();
    let handles: Vec<_> = RELEASE_TARGETS.iter().map(|t| std::thread::spawn(move || embedded_crates(t))).collect();
    let mut needed = std::collections::BTreeSet::new();
    for h in handles {
        needed.extend(h.join().expect("cargo tree thread"));
    }
    assert!(needed.len() > 100, "suspiciously few crates found: {}", needed.len());
    let missing: Vec<String> = needed.difference(&listed).map(|(n, v)| format!("{n} {v}")).collect();
    assert!(
        missing.is_empty(),
        "THIRD_PARTY_LICENSES.md does not list {} embedded crate(s): {}. Regenerate it with `python scripts/gen_third_party.py`.",
        missing.len(),
        missing.join(", ")
    );
}
