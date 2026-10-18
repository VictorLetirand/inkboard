<p align="center"><img src="crates/inkboard-app/assets/icon.svg" width="96" alt="Inkboard logo"></p>

# Inkboard

**A fast, free, fully offline infinite whiteboard — written in Rust.**
Draw, diagram and present on a native desktop app, import and export the formats you already use, and let an AI agent draw on your board.

[![CI](https://github.com/VictorLetirand/inkboard/actions/workflows/ci.yml/badge.svg)](https://github.com/VictorLetirand/inkboard/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
![Status: alpha](https://img.shields.io/badge/status-alpha-orange.svg)
![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20Linux%20%7C%20macOS-lightgrey.svg)

🇬🇧 English · 🇫🇷 [Version française](README.fr.md)

![Inkboard](docs/img/interface.png)

> **Status: alpha.** Inkboard is a young project. It is feature-rich and heavily tested (252 automated tests), but it has so far been **used by hand on Windows only**.
> Linux and macOS builds compile and pass the automated tests in CI, yet nobody has used the app there yet — expect rough edges, and please [report them](../../issues).
> The interface is available in **English and French**; it follows your system language and can be switched from the menu (*Language*).

## Why Inkboard?

Microsoft has announced it is retiring the desktop version of **Microsoft Whiteboard**, and the web alternatives all want an account, an internet connection, or a subscription.
We wanted a whiteboard that is:

- **Offline and private.** No account, no cloud, no telemetry. Your boards are plain files on your disk.
- **Fast on big boards.** Complaints about lag on large boards come up again and again for Miro, Excalidraw and Whiteboard. Inkboard stays smooth with 100 000 objects (≈ 4.5 ms per frame).
- **Interoperable.** Open and export **draw.io**, **Excalidraw**, **JSON Canvas** (Obsidian) and **Mermaid**, plus SVG, PNG and PDF. Your work is never locked in.
- **Solid where other tools are fragile.** Connectors that really stay attached (and route around shapes), real tables, layers, rich text with per-word colour and highlight, LaTeX formulas typeset like TeX, customizable shortcuts, pen pressure.
- **Ready for AI agents.** A built-in [MCP server](#use-it-with-ai-agents-mcp) lets Claude (or any MCP client) create and edit boards — live, in the open app, with every step undoable.
- **Free software.** Dual-licensed **MIT OR Apache-2.0**, built on a small, UI-independent Rust engine you can reuse.

Before writing a line of code we surveyed what users keep asking for in whiteboard tools (performance, fragile connectors, tables, layers, exports, offline use, pen support…); the [roadmap](docs/ROADMAP.md) tracks each of those requests.

## Install

### Download (recommended)

Grab the installer for your system from the [**Releases**](../../releases) page (pre-releases while Inkboard is in alpha):

| System | File | Notes |
|---|---|---|
| **Windows 10/11** | `inkboard_*_x64-setup.exe` or `inkboard_*_x64_en-US.msi` | The `.exe` installs for the current user (no administrator rights needed); the `.msi` is the classic Windows Installer package. |
| **Linux** (x86-64) | `inkboard_*_amd64.deb` or `inkboard_*_x86_64.AppImage` | `sudo apt install ./inkboard_*.deb`, or `chmod +x *.AppImage && ./*.AppImage`. Needs OpenGL and X11/Wayland libraries (`libxkbcommon`). |
| **macOS** (Apple Silicon / Intel) | `Inkboard_*.dmg` | Drag *Inkboard* to *Applications*. |

The installers also provide the `ink` command-line tool and the `inkboard-mcp` server next to the app.

> **The installers are not code-signed yet**, so your system will warn you the first time:
> on **Windows**, SmartScreen says "Windows protected your PC" → *More info* → *Run anyway*; on **macOS**, Control-click the app → *Open*.
> Every release lists SHA-256 checksums so you can verify your download. Signing is planned ([why it is not done yet](docs/DISTRIBUTION.md)).

### Build from source

You need [Rust](https://rustup.rs) 1.90 or newer.

```bash
git clone https://github.com/VictorLetirand/inkboard.git
cd inkboard
cargo run --release -p inkboard-app                    # start the app
cargo run --release -p inkboard-app -- diagram.drawio  # …or open a file directly
```

To build the installers yourself: `cargo install cargo-packager --locked`, then `cargo packager --release -c Packager.toml` ([details](docs/DISTRIBUTION.md)).

## Features

- **Drawing:** pen with pressure, highlighter, eraser, rectangles, ellipses, diamonds, triangles, lines, arrows, sticky notes, text, images, tables, vote dots, laser pointer. Infinite canvas, pages, layers, groups, alignment and distribution, snapping, undo/redo.
- **Smart connectors:** arrows are *bound* to shapes and follow them; elbow connectors route around other shapes; drag a midpoint handle to add a waypoint.
- **Rich text:** select words inside any shape, table cell or arrow label and make them bold, italic, underlined, struck, coloured or highlighted.
- **Formulas and code:** LaTeX typeset with the STIX Two Math font (fractions, roots, sums, integrals, matrices…), editable by double-click; syntax-coloured code blocks with a language picker (Rust, JavaScript/TypeScript, Python, C/C++, C#, Java, Go, Shell, SQL, JSON, HTML, CSS, YAML).
- **Present:** full-screen page-by-page mode (**F5**), countdown timer (bottom-right clock button), laser pointer (**F**).
- **Import / export:**

  | Format | Open | Save / export |
  |---|---|---|
  | Inkboard `.inkboard.json` | ✔ | ✔ (atomic save + autosave) |
  | draw.io `.drawio` (compressed or not, multi-page) | ✔ | ✔ |
  | Excalidraw `.excalidraw` | ✔ | ✔ |
  | JSON Canvas `.canvas` (Obsidian) | ✔ | ✔ |
  | Mermaid (flowchart, state, sequence) | ✔ | – |
  | SVG, PNG, PDF (vector) | – | ✔ |

- **Customizable shortcuts** (41 commands, several shortcuts per command, conflict handling) and a **light/dark theme** that follows the board background.
- **Command line:** `ink convert diagram.mmd diagram.drawio`, `ink convert board.inkboard.json board.png`, `ink info file.drawio`.

| Main menu | Dark theme |
|---|---|
| ![Menu](docs/img/menu.png) | ![Dark theme](docs/img/theme-dark.png) |

## Use it with AI agents (MCP)

`inkboard-mcp` is a [Model Context Protocol](https://modelcontextprotocol.io) server: an agent can create shapes, bound connectors, tables, code blocks, formulas, images and whole Mermaid diagrams, lay them out automatically, and export.
With Inkboard **open**, the agent edits the board you are looking at — each call is one undo step, it can see what you have selected, and it can ask what *you* changed since it last looked.
With Inkboard **closed**, it edits the `.inkboard.json` file and the app reloads it automatically.

With [Claude Code](https://claude.com/claude-code):

```bash
claude mcp add inkboard -- /path/to/inkboard-mcp --root /path/to/your/boards
```

**Always pass `--root`**: it confines every file the agent can read or write to that folder. The server exposes 16 tools (`add_shape`, `add_connector`, `import_mermaid`, `add_formula`, `export`, `board_changes`…) and MCP resources with change notifications.
Full reference, security notes and limits: [docs/MCP.md](docs/MCP.md).

## Platform status

| | Windows | Linux | macOS |
|---|---|---|---|
| Builds and passes the 252 automated tests (CI) | ✔ | ✔ | ✔ |
| App launched and used by hand | ✔ | not yet | not yet |
| Installers | built and tested | built in CI, untested | built in CI, untested |
| Pen eraser end (Windows Pointer API) | ✔ implemented, **not yet validated on real pen hardware** | – | – |

Pen pressure and the eraser end are implemented and covered by tests but **have never been tried with a real pen**: run `inkboard --pen-test` for a diagnostic report ([how](docs/STYLET.md)).

## Performance

Measured on the release build with a synthetic board (60 % labelled shapes, 25 % strokes, 15 % bound connectors), continuous zoom and pan:

| Objects | Frame time (fit-to-screen) | Frame time (100 % zoom) |
|---|---|---|
| 2 000 | 0.8 ms | 0.3 ms |
| 20 000 | 3.3 ms | 0.3 ms |
| 100 000 | 4.5 ms | 0.3 ms |

Reproduce with `ink bench 100000 big.inkboard.json && inkboard --bench big.inkboard.json`.

## Project layout

| Crate | Role |
|---|---|
| [`inkboard-core`](crates/inkboard-core) | The engine: document model, geometry, bound connectors, history, SVG export, auto-layout. No UI dependency. |
| [`inkboard-formats`](crates/inkboard-formats) | draw.io, Excalidraw, JSON Canvas, Mermaid, PNG/PDF, LaTeX typesetting. |
| [`inkboard-app`](crates/inkboard-app) | The desktop app (egui). |
| [`inkboard-cli`](crates/inkboard-cli) | The `ink` command-line converter. |
| [`inkboard-mcp`](crates/inkboard-mcp) | The MCP server. |
| [`inkboard-pen`](crates/inkboard-pen) | Pen eraser detection on Windows (the only crate with `unsafe` code). |

More: [architecture and design decisions](docs/ARCHITECTURE.md) · [roadmap](docs/ROADMAP.md) · [distribution and signing](docs/DISTRIBUTION.md) (French copies in [`docs/fr/`](docs/fr)).
The original web prototype is kept in [`prototype-web/`](prototype-web/); its `.tableau.json` boards can be opened in Inkboard.

## Roadmap

Planned or wanted next:

- **More interface languages** — English and French are built in; adding one means adding a translation table (see `crates/inkboard-app/src/i18n_fr.rs`).
- Code-signed installers (Windows via SignPath, macOS notarization) and automatic updates.
- Validation of pen pressure and eraser on real hardware; pen eraser support on macOS/Linux.
- First real-world usage on Linux and macOS, and fixes that come out of it.
- Real LaTeX beyond what the typesetting library supports; per-language code highlighting.

Have an idea or found a bug? [Open an issue](../../issues/new/choose) — see also [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. Third-party components keep their own (permissive) licenses: see [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).
Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in Inkboard shall be dual-licensed as above, without any additional terms or conditions.
