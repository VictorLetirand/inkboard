# Roadmap

*Version française : [fr/ROADMAP.md](fr/ROADMAP.md)*

## User complaints → status

Research done in October 2026 (G2/Capterra/GetApp reviews, Excalidraw GitHub issues, Hacker News threads).

| Complaint | Tools | Status |
|---|---|---|
| Lag on large boards | Miro, Whiteboard, Excalidraw | ✔ measured: 100,000 objects at 4.5 ms/frame (spatial index, level of detail) |
| Fragile connectors / that don't follow | draw.io, Excalidraw | ✔ **bound** connectors, straight/elbow/curved, tests |
| No automatic layout | Excalidraw | ✔ "auto layout" command + Mermaid import |
| Tables of cells | Excalidraw (#1 by comments) | ✔ insertion, cell editing, rows/columns; formatting to come |
| Layers | Excalidraw, Whiteboard | ✔ visibility, locking, moving the selection |
| Several pages / tabs | Excalidraw | ✔ |
| Adjustable corner rounding, highlighter, notebook-style grid | Excalidraw | ✔ |
| Export readable in other tools | Whiteboard, Miro | ✔ draw.io, Excalidraw, JSON Canvas, SVG, PNG |
| Offline, no account, no limit on boards | Miro | ✔ by construction |
| Color / highlight words in a paragraph | Excalidraw | ✔ word selection, bold/italic/underline/strikethrough, color, highlight (milestone 2) |
| Customizable shortcuts | Excalidraw | ✔ 41 commands, several shortcuts, JSON file, conflicts handled (milestone 2) |
| Pen: pressure | Excalidraw | ◐ variable width; **not validated on real hardware** (`--pen-test` mode) |
| Pen: eraser (end) | Excalidraw | ◐ implemented through the Windows Pointer API; **not validated on real hardware** (`--pen-test` mode to validate it: docs/STYLET.md) |
| Code blocks | Excalidraw | ✔ minimal syntax coloring (comments, strings, numbers, keywords) with a language picker (13 languages + auto) |
| LaTeX | Excalidraw | ✔ TeX typesetting (STIX Two Math), editable formula, color follows |
| Presentation, timer, voting | Excalidraw | ✔ full-screen page by page, timer, dots, laser pointer |
| Polished interface (floating cards, dark theme) | – | ✔ identical to the web prototype |
| Connectors that avoid shapes | draw.io | ✔ A* routing, waypoints |
| Clipboard (images, text) | all | ✔ |
| Driving by agents (MCP) | – | ✔ `inkboard-mcp`: files and live mode (application open, undoable, selection visible) |
| PDF export | Whiteboard | ✔ vector |
| Icon / shape library (draw.io, brand logos) | draw.io, Miro | ✔ 28,800 icons offline, searchable, tinted by the outline color |
| Interface in several languages | – | ✔ English and French, detected from the system, switchable in the menu |

## To do

- More interface languages: add a table next to `i18n_fr.rs` (contributions welcome).
- `inkboard-mcp`: check what real clients (Claude Code, Claude Desktop) do with resource notifications; notifications with the detail of the change; reading image content.
- **Signed** Windows installer (needs a code-signing certificate; the `scripts/` folder provides the portable archive and a per-user install).
- Smarter syntax coloring (it is token-based, not a parser); text (`\text`) and advanced environments of the typesetting library beyond its capabilities.
- **Validation of pen pressure and eraser on real hardware** (procedure: docs/STYLET.md); eraser on macOS/Linux (not handled).
- Per-line alignment in the input field while editing multi-line centered text.
- More icon sets (the library is built from `scripts/build_icon_library.py`; see [ICONS.md](ICONS.md)) and drag-and-drop from the palette.
- WASM build of the engine (same code) if a web use is wanted.

## Known limits

- No automated visual tests (the `--screenshot` capture is for manual checking).
- Pen strokes are exported as images to draw.io (no editable equivalent).
- The exchange formats have only been checked against their own exports and hand-written files, except for the
  draw.io export verified in the official viewer; Excalidraw and Obsidian Canvas files written like the real tools do (`inkboard-formats/tests/realistic.rs`) import correctly, but no real user file has been tested.
- The CI workflow (`.github/workflows/ci.yml`, Windows + Linux + macOS) is green. On Linux and macOS the automated tests pass, but the application has never been launched by hand there; the release workflow (installers) has not yet completed a full run.
- Elbow routing with avoidance is disabled beyond 5,000 objects (linear cost per connector).
- Formulas depend on the `latex-rust` 2.1.1 crate (recent, little battle-tested; 3.x requires Rust 1.99): an unsupported command is reported in the input window. A formula is rasterized on screen (×4) but stays vector in SVG and PDF exports.
