# Contribuer

Merci de votre intérêt ! Ce projet est libre, sous la double licence **MIT OU Apache-2.0** (au choix de l'utilisateur), comme la plupart des projets Rust.

## Licence des contributions

Sauf mention contraire explicite de votre part, toute contribution que vous soumettez volontairement pour inclusion dans ce projet, au sens de la licence Apache-2.0,
est placée sous la double licence MIT OU Apache-2.0, sans condition supplémentaire.

## Avant de proposer une modification

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

- Un changement = un sujet ; le diff le plus petit possible, sans refactor non demandé.
- Une fonctionnalité ou une correction vient avec ses tests (le moteur `inkboard-core` se teste sans interface ; l'interface se teste avec `egui_kittest`, voir `crates/inkboard-app/src/tests.rs`).
- Les dépendances ajoutées doivent être sous licence permissive (MIT, Apache-2.0, BSD, ISC, Zlib, Unicode, OFL pour les polices) : voir `THIRD_PARTY_LICENSES.md` et `docs/DISTRIBUTION.md`.
- Documentation et messages en français (le code et ses identifiants restent en anglais).
