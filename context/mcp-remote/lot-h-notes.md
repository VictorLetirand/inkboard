# Lot H - embedded fonts shared by host and guests

Branch `feat/remote-h` (from `feat/remote` d1b586f). Crate `crates/inkboard-fonts`: `definitions() -> egui::FontDefinitions`, builds for `wasm32-unknown-unknown`.

## Fonts and licenses (all free, texts in `THIRD_PARTY_LICENSES.md`, OFL texts next to the files)
| Family | Font | License | Size (raw / gzip) |
|---|---|---|---|
| sans, sans bold | Noto Sans 400 / 700 (instanced from the variable font) | OFL 1.1 | 130 + 131 KB / 71 + 73 KB |
| serif, serif bold | Gelasio 400 / 700 (metric-compatible with Georgia) | OFL 1.1 | 81 + 82 KB / 43 + 43 KB |
| handwriting | Patrick Hand (no bold upstream), scaled 1.12 | OFL 1.1 | 77 KB / 31 KB |
| mono | Hack Regular (already in egui) + Hack Bold subset | MIT + Bitstream Vera | 116 KB / 61 KB (bold) |
| fallbacks | egui's Ubuntu-Light (Greek, Cyrillic), NotoEmoji, emoji-icon-font | UFL, OFL, MIT | already in the binary |
Added weight: 617 KB raw, 321 KB gzip (budget 3 MB). Subsets: Latin, Latin Extended, punctuation, currency, arrows, math, box drawing, dingbats; no CJK; hinting dropped. Italic = egui skew (same everywhere).
Reserved Font Names: none declared by the three OFL fonts. Sources: google/fonts `ofl/` (Noto Sans, Gelasio, Patrick Hand), source-foundry/Hack (Hack-Bold).

## Behavior
- `fonts::apply(ctx, embedded)`; the web build (`ALWAYS_EMBEDDED`) always uses the embedded set and hides the setting.
- `App::sync_fonts` (first line of `update`) compares `wants_embedded_fonts()` (setting OR `guest.is_some()` OR `share.host.is_some()`) with the installed set: no hook inside `session_host` / `session_guest`, so every start/stop/error path is covered. egui applies `set_fonts` at the start of the next frame: a frame never mixes both sets, atlas and galleys are rebuilt by egui, text is model data (nothing lost).
- Setting "Use built-in fonts" (main menu, after "Animate the agent"), file `embedded_fonts` in the data folder ("1"/"0", default off). `--embedded-fonts` forces it for one run (screenshots).
- Bold now exists for serif and mono too (system: georgiab.ttf / consolab.ttf when present; before, bold was ignored outside sans).

## What changes visually (captures `lot-h-system.png` / `lot-h-embedded.png`)
- Sans: Noto Sans is about 7 % wider than Segoe UI: some lines wrap one word earlier. Serif: almost identical to Georgia (Gelasio). Handwriting: Patrick Hand is narrower and neater than Segoe Print (it no longer overflows the diamond). Mono: Hack is wider than Consolas (code blocks look looser).
- Existing quirks identical in both: table row heights do not grow with wrapped cells; a space after an italic run touches it.

## Tests
`fonts::tests` (identical widths and `layout_rich` lines in two independent contexts: 4 families x 5 sizes x bold x 3 widths; pinned widths so every platform agrees; real bold; non-Latin/emoji fallback; switch both ways), `tests_session_host` and `tests_session_guest` (switch on start/stop/leave), `inkboard-fonts` unit tests (families resolve, size budget, licenses shipped). Parallel `inkboard-net` loopback test `a_wrong_key_is_refused_by_the_host` failed once under full-workspace load and passes alone.

## Not verified
Real browser rendering (F1 must call `fonts::apply` in its `build`); Linux/macOS renderings; Segoe-based layouts saved by older hosts keep their stored box sizes (only text wrapping changes). `crates/inkboard-app/Cargo.toml` edited (dependency), outside the listed files.
