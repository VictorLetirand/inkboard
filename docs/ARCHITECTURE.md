# Architecture

*Version française : [fr/ARCHITECTURE.md](fr/ARCHITECTURE.md)*

## Principles

1. **The engine knows nothing about the interface.** `inkboard-core` has no window, no font, no graphics dependency. Everything that can
   be tested without a screen is tested in `inkboard-core` / `inkboard-formats`; the interface (`inkboard-app`) is a thin layer on top.
   The same engine serves the MCP server, the CLI and possible other front ends (WASM).
2. **Connectors do not store their path.** A connector keeps its ends (`Free(point)` or
   `Bound{target, anchor}`); the path is **recomputed on demand** from the page's current state
   (`Page::connector_path`). Moving, resizing or rotating a shape therefore makes its arrows follow, with no update step
   and no possible stale state. Deleting a shape detaches its connectors in the same transaction.
3. **Every modification goes through the `Editor`.** Operations (`Insert`, `Remove`, `Replace`, `Reorder`, `SetLayers`, pages, background)
   are reversible; one transaction = one undo step. During a gesture (move, resize…), the interface
   modifies the page "live" without history, then records **a single** before/after on release.
4. **Text is measured by an injected `Measurer`.** The interface provides the real fonts (egui); the CLI uses an
   approximate measure. Text layout (`layout_rich`) is therefore identical for the screen, the SVG and the exchange formats.
5. **Rich text in the model from day one**: `RichText` = styled fragments (bold, italic, underline, strikethrough, color,
   highlight). Word colors and highlights are already rendered (screen, SVG, draw.io).
6. **Exchange formats = pure functions** `content → Document` and `Document → content`, tested by round trips.
   An import returns a `Report` (approximated / skipped elements) instead of failing silently.
7. **English in the source, translations in a table.** UI strings are written in English in the code and looked up through `t()` / `tr!`; `i18n_fr.rs` holds the French table. The language is detected from the system and can be overridden in the menu. Libraries and the MCP server speak English only.

## Model

```
Document { pages[], background, assets (images, deduplicated) }
Page     { layers[], objects[] (order = z), view }
Object   { id, style, group, lock, layer, kind }
Kind     = Box (rect/ellipse/diamond/triangle/sticky/text) | Pen | Connector | Image | Table
```

Strokes (`Pen`) store their points in world coordinates in an `Arc<Vec<_>>` (cheap copy for the history).

## Interface (`inkboard-app`)

- `render.rs`: draws a page (shapes, rich text, smoothed variable-width strokes, connectors and arrowheads, images, tables). Level of detail:
  below 3 px an object becomes a dot; the application caches (`Caches`: id → position, spatial index) serve rendering and selections.
- `tools.rs`: state machine of the gestures (`Drag`), handles, object creation, text editing.
  Handle computations (resizing a rotated frame, rotation, snapping) live in `inkboard-core::transform` and are tested separately.
- `rich_edit.rs`: rich-text editing. The egui input field keeps the raw string; `RichText` follows every keystroke
  (`apply_edit`: common prefix/suffix) and a `layouter` displays the styles. The floating bar and the shortcuts call
  `RichText::format` (engine). The selection is remembered, then restored after a click on the bar.
- `keymap.rs`: `Action` (41 commands) → `Shortcut`s, replacement on conflict, reserved combinations, tolerant JSON file.
  `App::run_action` is the single entry point for commands (shortcuts, menus).
- `i18n.rs` / `i18n_fr.rs`: language detection and preference, `t()` / `tr!` lookups, French table.
- `theme.rs`: the prototype's light/dark palette, egui styles (rounding, shadows, text sizes) and shared widgets (icon buttons, menu rows, segmented buttons, floating cards);
  `icons.rs`: the prototype's SVG icon set, rasterized once (resvg, 3×) into a white texture, then tinted; `chrome.rs`: the cards (title + menu, tools, pages, zoom, help), the main menu and the context menus;
  `panel.rs`: floating style panel (swatches, segmented buttons, order, alignment, layers); `ui.rs`: presentation, dialogs centered on a dimmed background, notifications.
  The canvas fills the whole window; the cards are `egui::Area`s above it (a newly appeared card's area is too wide during its very first frames: the tests let the panel settle with `T::tool`). `app.rs`: state, files, autosave.
- `clipboard.rs`: clipboard (internal objects, system images and text). `insert.rs`: code block, formula, vote dot.
  Presentation mode (`App::presenting`, `present_overlay`) and the timer are in `app.rs` / `ui.rs`.
- `tests.rs`: tests that drive the application with a simulated mouse and keyboard (`egui_kittest`), without a window.

## Modules added to the core (`inkboard-core`)

- `spatial.rs`: grid spatial index (`SpatialIndex`), id → position table, fast access to objects by id.
- `route.rs`: A* orthogonal routing on the grid formed by the obstacles' edges (elbow penalty); `connector.rs` uses it for
  elbow connectors (`avoid_obstacles`) only when the simple path crosses a shape. Disabled beyond 5,000 objects.
- `pen.rs`: stroke smoothing, pressure-dependent width, polygonal outline of the stroke.
- `highlight.rs`: generic syntax coloring of code blocks (colored `RichText`).

`inkboard-formats::math` typesets LaTeX formulas to SVG (`latex-rust` crate, embedded STIX Two Math font). A formula is an `ImageObj` whose asset is that SVG
**drawn in white** and whose `latex` field keeps the source: the screen tints the texture with the stroke color, SVG export replaces the white with that color.

`inkboard-mcp`: MCP server (JSON-RPC over stdio, hand-written) on top of `inkboard-core`/`inkboard-formats`; each tool is a function `apply(&mut Editor, name, args)`; file mode opens the file, applies it and saves atomically, live mode has the application run it (`inkboard-app::live`: local 127.0.0.1 server + token, executed on the UI thread in one transaction).
`inkboard-app::watch` reloads the open file when another program modifies it (unless there are unsaved local changes). Details: [MCP.md](MCP.md).

`inkboard-mcp::resources`: MCP resources and the watcher thread (600 ms polling → `notifications/resources/updated`); `tools::apply_tracked`: tracking of the user's changes (per-object fingerprint, the agent's own changes are absorbed into the baseline).

`inkboard-pen` (Windows) installs a window *subclass* that reads `WM_POINTER*` without consuming them and exposes `eraser_active()` and `last_sample()` (pressure, flags, tilt; used by the `--pen-test` diagnostic, `inkboard-app::pen_test`); the application then replaces the tool with the eraser
for the gesture (`App::eraser_probe`, substitutable in tests). It is the only crate that allows `unsafe`.

### Why egui?

A single language and a native executable, offline by construction, with a drawing API that gives full control over the board's
rendering. Known limits, accepted for this milestone: no system icons (hand-drawn icons), basic text editor,
limited accessibility. If rendering ever outgrew egui
(very large boards, effects), `inkboard-core` stays independent: the renderer could be changed without touching the rest.

## Tests

| Crate | Tests | What is covered |
|---|---|---|
| `inkboard-core` | 101 | geometry, model, connectors (rotation, elbow, curve), selection, history, saving, SVG, layout, handles, rich text (insert/delete/style toggling), obstacle routing, spatial index, pen pressure, highlighting |
| `inkboard-formats` | 46 | draw.io / Excalidraw / JSON Canvas round trips, foreign imports, compressed draw.io, Mermaid (flowchart, state, sequence, class, ER), PNG, PDF, LaTeX typesetting, realistic Excalidraw/Canvas files |
| `inkboard-mcp` | 8 | full session on the real binary (handshake, tools, root confinement, exports, resources and notifications); live mode and live `board_changes` are tested in `inkboard-app` (editing with undo, targeting the open file, token, resources and notifications, changes and images) |
| `inkboard-app` | 88 | creation, moving with bound arrows, resizing, rotation, lasso, eraser, undoing a gesture, text, tables, import, zoom; **rich text** (typing, keyboard selection, bold/color/highlight); **shortcuts** (reassigning, removing, recording a key, persistence, shortcuts while editing); **waypoints**, obstacle avoidance, code block (colored), typeset formula (insert, edit, tinted export), vote, pen eraser, presentation, timer, clipboard, pen, languages |

In addition: the draw.io export was opened in the official diagrams.net viewer (shapes, bound arrows, dashes, rotation).
