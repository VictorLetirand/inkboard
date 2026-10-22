# Lot F1 - web base (notes)

*2026-10-22, branch `feat/remote-f1` (from `feat/remote` d1b586f). Guide: `spike-s2.md`; doc for users: `docs/WEB.md`.*

## What runs in the browser
`inkboard-app` compiles for `wasm32-unknown-unknown` (`--no-default-features --features web-glow`, or `web-wgpu`) and runs through Trunk
(`crates/inkboard-app/web/`, `scripts/build_web.*`). Checked in Chrome by `web/test/page.mjs` (all checks pass): start, shape, text,
library (pack fetched only then), icon insert, download (Ctrl+S) and re-open (picker), pasted and dropped image, distinct ids in two
sessions, PNG export, "no WebGL" message (the wgpu build passes the same test in headless Chrome; the backend it picked was not confirmed). Desktop: fmt, clippy `-D warnings` (workspace, all targets), `cargo test --workspace` green; wgpu kept.

## Blockers solved (how)
- Seam: `platform.rs` + `platform_native.rs`/`platform_web.rs` (clock, data dir, clipboard, pickers, download, icon pack, relay shim, title).
  Desktop-only modules have web stand-ins (`live_web`, `watch_web`, `share_ui_web`, `session_net_web`) chosen in `main.rs` by `cfg_attr(path)`.
- `inkboard-mcp`, `inkboard-net`, `arboard`, `raw-window-handle` are desktop-only dependencies; the pack is embedded only by the desktop (via
  `inkboard-mcp`). `inkboard-pen` stays a dependency (it is a no-op outside Windows). `session_host` is compiled on the web too (the guest
  reuses 5 of its checks), never started.
- eframe `on_exit` has two signatures (`glow` feature or not): two cfg'd versions calling `App::shutdown`.
- Ids: `platform_web::random_seed` (crypto.getRandomValues) -> `seed_ids` in `web::start`; verified by the page test.
- Pasted image: own `paste` listener on `document` -> `Incoming::PastedImage` -> `insert_rgba`; text paste stays eframe's `Event::Paste`.

## Measures (release, `wasm-opt -Os`, glow, MB = 2^20)
| | raw | gzip -9 | brotli 11 |
|---|---|---|---|
| without pack (the page) | 12.49 | 5.19 | 3.49 |
| with the pack embedded (temporary build) | 20.66 | 13.36 | 11.68 |
| wgpu (WebGPU + WebGL), without pack | 15.22 | 6.11 | 4.14 |
`icons.bin` 8.18 MB, fetched on first library opening. Start (localhost, Chrome, headless): 0.3 to 2 s to the first frame (unreliable on this busy machine).

## Limits / not done
- No persistence in the browser (autosave, language, shortcuts, host name, relays): `platform::PERSISTS = false`.
- No pen pressure / eraser end (eframe gives none); mobile keyboard and touch pinch not tried.
- "Paste" menu item pastes the board's own clipboard only (no synchronous clipboard read); copy as image not tested.
- draw.io import resolves built-in icons only after the library was opened once (pack not loaded before).
- Fonts: egui built-ins (lot H). PNG export from the page works and shows the text (checked by the test); PDF not tried.
- Join window is reachable but inactive ("not connected yet"); relay shim refuses every relay until F2/F3.
- Not tested: Firefox, Safari, Edge, WebGPU on real hardware, `trunk serve`, 5 000 icons, `deny`/THIRD_PARTY (new wasm-only deps:
  web-time, wasm-bindgen(-futures), js-sys, web-sys, log, wgpu).
- `cargo clippy` for the wasm target reports a pre-existing `drop_non_drop` in `inkboard-core/src/serial.rs` (not touched).
- The test must run with `node_modules` outside the repo tree here (the French-text test walks every folder): `NODE_PATH=...`.
