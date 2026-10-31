# Contributing to Inkboard

Thank you for your interest! Inkboard is free software, dual-licensed **MIT OR Apache-2.0** (at the user's option), like most of the Rust ecosystem.

## License of contributions

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in Inkboard, as defined in the Apache-2.0 license,
is dual-licensed under MIT OR Apache-2.0, without any additional terms or conditions.

## Before you open a pull request

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CI runs the same checks on Windows, Linux and macOS with the latest stable Rust.

- One change = one topic; keep the diff as small as possible and avoid unrelated refactoring.
- New features and fixes come with tests. The engine (`inkboard-core`) is tested without any UI; the app is tested with `egui_kittest`
  (see `crates/inkboard-app/src/tests.rs`).
- New dependencies must have a permissive license (MIT, Apache-2.0, BSD, ISC, Zlib, Unicode, OFL for fonts). After changing dependencies, run
  `python scripts/gen_third_party.py` to refresh `THIRD_PARTY_LICENSES.md` (it fails on a non-permissive license).
- User-facing strings are written in English in the code and wrapped in `t("…")` / `tr!("…")`; French lives in `crates/inkboard-app/src/i18n_fr.rs`. Add a translation there when you add a string (a test fails if a key is missing or duplicated).
- Documentation is English first (`docs/`), with French copies in `docs/fr/`.

## Reporting bugs

Please include your system (Windows/Linux/macOS and version), how you installed Inkboard (installer or source), and — for drawing problems — the `.inkboard` file if you can share it.
Just trying Inkboard out? The [10-minute testing guide](docs/TESTING.md) lists what to try and has a "Test report" issue form.
Pen problems: attach the report from `inkboard --pen-test` ([guide](docs/STYLET.md)).

## Security

See [SECURITY.md](SECURITY.md).
