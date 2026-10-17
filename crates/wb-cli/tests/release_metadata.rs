//! Cohérence des métadonnées de publication : version des installeurs, licence et fichiers de licence.

use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn read(name: &str) -> String {
    std::fs::read_to_string(root().join(name)).unwrap_or_else(|e| panic!("{name} : {e}"))
}

/// Valeur d'une clé `clé = "valeur"` de premier niveau.
fn value(toml: &str, key: &str) -> String {
    toml.lines()
        .find_map(|l| l.strip_prefix(key).and_then(|r| r.trim_start().strip_prefix('=')).map(|r| r.trim().trim_matches('"').to_owned()))
        .unwrap_or_else(|| panic!("clé {key} absente"))
}

#[test]
fn installer_version_matches_the_workspace_version() {
    let ws = value(&read("Cargo.toml"), "version");
    let pk = value(&read("Packager.toml"), "version");
    assert_eq!(pk, ws, "mettre à jour `version` dans Packager.toml en même temps que dans Cargo.toml");
}

#[test]
fn project_is_dual_licensed_mit_or_apache_with_both_texts_present() {
    assert_eq!(value(&read("Cargo.toml"), "license"), "MIT OR Apache-2.0");
    assert!(read("LICENSE-MIT").contains("Permission is hereby granted, free of charge"));
    assert!(read("LICENSE-APACHE").contains("Apache License") && read("LICENSE-APACHE").contains("Version 2.0, January 2004"));
    let combined = read("packaging/LICENSE.txt");
    assert!(combined.contains("MIT") && combined.contains("Apache License"), "le texte affiché par l'installeur contient les deux licences");
    assert!(read("THIRD_PARTY_LICENSES.md").contains("STIX Two Math"), "la police embarquée est créditée");
}
