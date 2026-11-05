# Changelog

All notable changes to Inkboard are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
follows [Semantic Versioning](https://semver.org/) (pre-releases carry a hyphen, e.g. `0.2.0-beta.1`).

## [Unreleased]

## [0.2.0-rc.5] - 2026-11-05

**Release candidate.** Two points of friction from daily use: the page bar adapts to the room available (ellipsized tabs, then a compact drop-down) and exports can cover several pages at once. No change to the file format or to live sharing.

### Added

- **Export dialog:** *Export as PNG / SVG / PDF / Excalidraw / JSON Canvas* now lets you tick the pages to export (All / None / Current page), export the selection only, and pick the PNG scale. Several pages give one file per page in a chosen folder (one download each on the web), PDF gives a single multi-page file.

### Changed

- **Page switcher:** the page bar no longer grows under the other cards. It measures the room left between the zoom and help cards, caps long tab names with an ellipsis (full name in a tooltip), then falls back to a compact `◀ name ▾ ▶ +` mode with a drop-down list of all pages (search field above 12 pages).

## [0.2.0-rc.4] - 2026-11-05

**Release candidate.** The MCP server learns to draw clean diagrams on its own: a `check_layout` tool that lists the layout defects with their ids, titled frames (`add_frame`), arrow attachment sides and label positions, grids and a richer `auto_layout`. Live sharing moves to protocol v6: update every app that hosts or joins a session.

### Added

- **MCP, unknown arguments:** a tool call (also inside `batch`) with an argument that is not in the tool's schema still runs and returns a `warnings` entry (`unknown argument "bold" ignored (did you mean ...?)`), once, in the dispatcher.
- **MCP, `check_layout`:** new warning `sparse_frame` (content under 30% of the frame, or an empty band over 200 units); `unbalanced` also reports `staircase` (top-level frames side by side with tops more than 60 units apart); new metrics `coverage` and `frame_top_offset`.
- **MCP, `create_board`:** `overwrite: true` replaces an existing file with a fresh empty board; without it an existing file is an error that says so.
- **MCP, `add_frame`:** `title_color` (default dark grey `#343a40`, kept on a re-fit).
- **MCP, `add_frame`:** new tool that draws a grouping frame around existing objects in one call: one `rect` with its own title (top left, bold), sized to the union of `ids` plus `padding` (default 40, room added for the title) and placed behind them, also correct inside another frame. Calling it again with `frame` re-fits the same frame after the content moved.
- **MCP, grid:** `arrange_objects` with `action: "grid"` (`columns`, `gap_x`, `gap_y`, `origin`, `cell` `max` / `fit`, `align` `center` / `start`) lays objects out row by row in the order given; an icon and its label count as one item.
- **MCP, `auto_layout`:** `gap_x` / `gap_y` set the spacing, `ids` limits the layout to some objects, an icon and its label (any group) are one node, and a frame is laid out as a cluster around its content like a Mermaid subgraph (the engine's `auto_layout_with`; the application's command gets the same group and frame handling).
- **MCP, connectors:** `add_connector` accepts `dash`, `font_size` (label) and `stroke_width`; the label is `label` or `text` in both `add_connector` and `update_object`.
- **MCP, icons:** `add_icon` takes `label_font_size`, `label_max_width` (long labels now wrap, default `max(2 × icon size, 140)`) and `label_side` (`bottom`, `top`, `right`, `left`). `search_icons` returns each icon's `aspect` and `wide` (wordmark logos), and retries an empty query with synonyms (`note`).
- **MCP, shapes:** `fit: "width"` (with `max_lines`) on `add_shape` and `update_object` computes the width that holds the text without splitting a word; both warn when the height had to grow or a word is wider than the text area.
- **MCP, `check_layout`:** new read-only tool that lists the measurable layout defects of a page with the ids involved: overlaps, text overflow and split words, text across a frame border, arrows through objects, crossing or merged arrows, arrow label collisions, duplicate or unattached arrows, text too small once exported (`export_width`), unbalanced layout; plus metrics (content box, aspect, smallest text in pixels, empty cells).
- **Arrow attachment side:** a connector end bound to an object can carry a side (`top`, `bottom`, `left`, `right`): it attaches at the middle of that side and the route leaves perpendicular to it (straight, elbow and curve). MCP: `from_side` / `to_side` on `add_connector` and `update_object` (`auto` clears). draw.io `exitX` / `exitY` / `entryX` / `entryY` and JSON Canvas `fromSide` / `toSide` map to it.
- **Arrow label position:** a connector label can sit anywhere along the path (`label_pos`, 0 to 1) and beside the line (`label_offset`); the canvas, the SVG / PDF / PNG export, the text editor and `check_layout` use it. MCP: `label_pos`, `label_offset` on `add_connector` and `update_object`. draw.io edge label geometry maps to it.
- **Title inside a shape:** a text can be aligned to the top or the bottom of its shape (`valign`), so a frame `rect` carries its own title (`align: left`, `valign: top`). MCP: `valign` on `add_shape` and `update_object` (and `align` on `add_shape`); draw.io `verticalAlign` and Excalidraw `verticalAlign` map to it, and a draw.io rectangle with its text at the top or bottom no longer gets a separate text object.
- **Live sharing protocol v6:** objects carry the new fields above, so every app of a session (host and guests) must be updated to this version. Boards that do not use the new fields are saved exactly as before, and older files open unchanged.

### Changed

- **Left/top-aligned text in a rect:** its text area starts 16 units (was 8) from the left border for left-aligned text and from the top border for top-aligned text, so a frame title clears the rounded corner (canvas, export, text fit and `check_layout` agree; `add_frame` and `auto_layout` leave the room needed).
- **Elbow arrows no longer share a line:** elbow connectors whose middle segments would run along the same line and overlap (one source fanning out to a column of targets, parallel links between two columns) are spread over parallel channels 10 units apart, on screen and in every export; `check_layout` no longer reports them as `shared_segment`, nor two arrows that only share their first segment when leaving the same point. Applies to arrows without waypoints, on pages of at most 150 elbow arrows.
- **Arrows on icons:** an arrow attached to an image (an icon) with no side now attaches at the middle of the side facing the other end instead of a point of its box that could be a corner.
- **MCP:** `move_objects` also moves the other members of the groups of the given ids (an icon and its label); `group: false` restores the previous behavior.
- **MCP:** `update_object` returns its warnings under `warnings` like every other tool (ignored parameters included, with the Markdown ones). `avertissements`, still returned with the ignored parameters only, is deprecated.

## [0.2.0-rc.3] - 2026-10-31

**Release candidate.** The first requests from outside users: an arrowhead at the end of a freehand stroke, a shape that opens its text when drawn, and named color swatches. Live sharing moves to protocol v5: update every app that hosts or joins a session.

### Added

- **Arrow at the end of a freehand stroke:** the style panel of the pen offers an arrowhead at the end (→ ▶ ● |), for the selected strokes and the next ones. Live sharing moves to protocol v5: update every app that hosts or joins a session.

### Changed

- **Shapes:** a shape opens its text right after it is drawn, ready to type (Escape or a click elsewhere leaves it empty).
- **Tooltips:** every color swatch and every arrowhead button now has a name in its tooltip (English and French); no more empty tooltip bubble.

## [0.2.0-rc.2] - 2026-10-30

**Release candidate.** Fixes the Linux AppImage, which stopped at startup on a system without `libxkbcommon-x11`; the macOS Apple Silicon installer is now built on `macos-15`.

### Fixed

- **Linux:** Inkboard no longer stops at startup on an X11 desktop without `libxkbcommon-x11` ("Library libxkbcommon-x11.so could not be loaded"): the AppImage now bundles it, and the .deb depends on `libxkbcommon-x11-0`.

## [0.2.0-rc.1] - 2026-10-30

**Release candidate.** Same code as 0.2.0-beta.4: the 0.2.0 features are complete and now being stabilized. Following versions are named
`rc.N` up to 0.2.0 (the AppImage catalog leaves out releases named `beta`).

## [0.2.0-beta.4] - 2026-10-29

**Pre-release.** Fixes for the 0.2.0-beta.3 features, and an AppImage that starts for every user.

### Fixed

- **Paste:** when the clipboard holds both text and a picture, the text wins: a single Excel cell now becomes a text box instead of a picture. A picture alone (a screenshot, an image copied from a browser) is still pasted as a picture. A trailing line break is no longer kept in a pasted text box.
- **Drop shadow:** turning the shadow on applies to the selection only; the next objects you draw no longer inherit it.
- **Drop shadow in the PDF export:** shadows are now drawn, as a soft vector shadow (the converter ignores SVG filters). In the PDF, the shadow of an image follows its rectangle rather than its transparency.
- **Arrowheads:** on a very short connector, heads are shortened (at most 40 % of the connector each) instead of overrunning its ends; other connectors are drawn as before.
- **Recolor an SVG:** the color popups close after a palette color is picked, and *All one color* starts from the image's first color instead of a fixed blue.
- **Browser end-to-end bench:** the three relay-blocked scenarios no longer fail with `puppeteer is not defined`.
- **AppImage:** the image is readable and runnable by every user (`AppRun` and folders were owner-only), so it starts under firejail and in the AppImage catalog test.

## [0.2.0-beta.3] - 2026-10-27

**Pre-release.** Four of the most requested whiteboard features, and live sharing moves to protocol v4: update every app that hosts or joins a session.

### Added

- **Paste from a spreadsheet:** a range copied from Excel, LibreOffice or Google Sheets (tab-separated text, quoted cells included) becomes a table, sized to its content, in one undo step. Up to 200 rows × 50 columns (a message says when the paste was cut). Tab-indented text such as code is still pasted as text.
- **Recolor an SVG image:** the style panel shows one swatch per color found in an imported SVG (multicolor logos included); each one can be replaced, all can be set to one color, and *Original colors* undoes it. The SVG / PNG / PDF, draw.io and Excalidraw exports use the new colors. MCP: `update_object` takes `recolor`.
- **Arrowhead size:** S / M / L / XL in the style panel, for every kind of head; draw.io `startSize` / `endSize`. MCP: `head_scale` on `add_connector` and `update_object`.
- **Drop shadow** on shapes, images, tables and connectors: a button in the style panel, in the SVG / PNG exports and draw.io (`shadow=1`). MCP: `shadow`. Not yet in the PDF export, whose converter ignores SVG filters.

### Changed

- **Live sharing: protocol v4.** Objects carry the new fields above, which an older version would drop when it edits them, so an older app and this one no longer join each other: the guest is told to update. The web page speaks v4 once it is published, so a host on 0.2.0-beta.2 or older must update for browser guests to join.

## [0.2.0-beta.2] - 2026-10-26

**Pre-release.** Follow-up of 0.2.0-beta.1, mostly for what an AI agent draws through the MCP server and for the Mermaid import, after a first
use on a documentation set of about 400 diagrams. The known limitations of 0.2.0-beta.1 below still apply.

### Fixed

- **Mermaid import:** a link no longer runs straight through a node it does not link (it used to happen to links between subgraphs and to long links in dense diagrams): its waypoints now go around the nodes in its way. Class and entity-relationship diagrams too.
- **Mermaid import:** the content of each subgraph is laid out on its own and placed as one block, so the frames of subgraphs no longer overlap (nested ones stay inside their parent), and the `direction` written in a subgraph is honoured when no link joins it to the outside (state diagrams: composite states too).
- **Mermaid import:** a link written to a subgraph (`B --> Back`) no longer creates a stray box named like the subgraph: it is attached to the subgraph's frame. A `;` inside a quoted label (`A["a; b"]`) no longer cuts the node in two.
- **MCP server:** a page given as text (`"2"`) selects that page number; `batch` accepts the `inkboard_` prefix that some clients add to tool names.

### Added

- **UML and entity-relationship arrowheads:** diamond (filled and hollow), hollow triangle and the crow's foot family (many, one or many, zero or many, exactly one, zero or one), in the arrowhead buttons of the style panel, in the MCP tools (`head_start`, `head_end`), in the SVG / PNG / PDF exports, in draw.io (`diamond`, `ER…`, `endFill`) and Excalidraw (`diamond`, `crowfoot_…`). They are stored in separate optional fields of the connector, next to the original heads, which keep the closest one: an older version ignores the new fields and draws that head, and a board that does not use them is written exactly as before. Class and entity-relationship diagrams imported from Mermaid now use them (the «composition» / «aggregation» names on the links are gone).
- **Mermaid import:** `classDiagram` and `erDiagram`. A class or an entity becomes a table (name, attributes, methods; or type, name, keys and comment) and links become bound connectors: inheritance and realization end in a hollow triangle, composition and aggregation in a diamond, and an entity link ends in crow's foot symbols and spells out its cardinalities. `direction` is honoured.
- **MCP server in a container:** a `Dockerfile` (`packaging/docker/mcp.Dockerfile`) builds `inkboard-mcp` in file mode (`--no-live`, confined to a mounted folder), for MCP clients and directories that start servers through Docker. A `glama.json` declares the maintainer of the listing on Glama.
- **MCP `export`:** creates the missing folders of `output` (under the allowed root, as `render` does), and a `.png` can be given a `width` in pixels or a `scale`, so that diagrams come out at the same width.

### Changed

- **Automatic layout** (Mermaid import, MCP `import_mermaid`, the *Auto layout* command): large and cyclic graphs are drawn much more compactly. Cycles are broken by reversing fewer edges, a node that only points far down is no longer left on the first rank, the crossing reduction tries three starting orders and swaps neighbours, and the lines of long edges run closer together. On a random graph of 40 modules with cycles, the drawing is about half as high with about half the edge length; on layered dependency graphs, crossings drop by 45 to 50 %. Only the positions computed by the layout change: files and the sharing protocol are untouched, and boards already saved keep their positions until the layout is run again.

## [0.2.0-beta.1] - 2026-10-25

**Pre-release.** The headline feature, live sharing, is new: it is covered by automated tests (including simulated crowds and hostile inputs) and
has been tried by hand on a few devices and networks, but not widely. See the known limitations below.

### Added

- **Live sharing (beta).** Click *Share* to get a link; people who open it edit the same board in real time.
  - Your computer hosts the session: no server to install. You approve each person, with an *edit* link or a *read-only* link.
  - Guests join from the application or from **a web page with nothing to install** ([victorletirand.github.io/inkboard](https://victorletirand.github.io/inkboard/);
    *Copy web link* in the Share window). Chrome, Edge and Firefox are tested.
  - Names and live cursors for the host, every guest and the AI agent; each guest undoes only their own edits; images are shared.
  - End-to-end encrypted; connections are direct when possible and go through a relay otherwise (a public one by default; you can
    [run your own](docs/RELAY.md)); short network cuts are survived and the session reconnects on its own.
  - On networks that block the relay (some company gateways), the application says so and offers *Copy the diagnostic*.
  - Designed for 20 participants (default cap 10), load-tested with up to 100 simulated guests. See [docs/SHARING.md](docs/SHARING.md) and [docs/WEB.md](docs/WEB.md).
- **Web build** of the application (guest page), built and deployed by CI to GitHub Pages.
- **Built-in fonts** (Noto Sans, Gelasio, Patrick Hand, Hack) so that text lays out identically on every platform and in the browser; setting *Use built-in fonts*.
- **MCP server v2:** 27 tools (icons, rendering to an image, edit / find / arrange, rich text, atomic `batch`); the agent's cursor is shown to every participant.
- Load-testing tool `inkboard-crowd` and a browser end-to-end bench (`crates/inkboard-app/web/test/`).

### Changed

- **Windows installer:** built with [Installway](https://github.com/tooltip-focus/installway) (per-user `.exe`, no administrator rights) instead of WiX/NSIS; the `.msi` is gone (thanks @gaetandezeiraud, #4).
- Code cleanup and `clippy::pedantic` enforced across the workspace (thanks @gaetandezeiraud, #5).
- Minimum supported Rust version is now 1.91.
- The licensing section of the READMEs explains the dual MIT / Apache-2.0 license (#7).

### Known limitations

- **Company networks:** the public relay is blocked behind some TLS-inspecting gateways. The error message was reworked and tested in simulation only; it has
  not been confirmed behind a real gateway.
- **Safari and iOS are not tested** (WebKit was exercised through an approximation only). The web page can join a session but cannot host one.
- **Windows:** the installer is not code-signed (SmartScreen shows "Windows protected your PC"), and it registers the `.inkboard` file type only: a shared
  `inkboard://` link must be pasted into *File → Join a session…* (or use the web link). Linux and macOS packages register the scheme.
- 100 guests work but are not a normal use (multi-second latency peaks); very large boards (20,000 objects and more) take a few seconds to reach a new guest.
- Open security findings (see `context/mcp-remote/security-review-1.md`): a heavy SVG filter sent by a hostile host (S-09), a public DNS name resolving to a
  private address (S-06), and a single frame over 1 MiB from the host (S-04, recovered by a snapshot).
- Linux and macOS builds pass CI but have not been used by hand yet.

[Unreleased]: https://github.com/VictorLetirand/inkboard/compare/v0.2.0-rc.5...HEAD
[0.2.0-rc.5]: https://github.com/VictorLetirand/inkboard/releases/tag/v0.2.0-rc.5
[0.2.0-rc.4]: https://github.com/VictorLetirand/inkboard/releases/tag/v0.2.0-rc.4
[0.2.0-rc.3]: https://github.com/VictorLetirand/inkboard/releases/tag/v0.2.0-rc.3
[0.2.0-rc.2]: https://github.com/VictorLetirand/inkboard/releases/tag/v0.2.0-rc.2
[0.2.0-rc.1]: https://github.com/VictorLetirand/inkboard/releases/tag/v0.2.0-rc.1
[0.2.0-beta.4]: https://github.com/VictorLetirand/inkboard/releases/tag/v0.2.0-beta.4
[0.2.0-beta.3]: https://github.com/VictorLetirand/inkboard/releases/tag/v0.2.0-beta.3
[0.2.0-beta.2]: https://github.com/VictorLetirand/inkboard/releases/tag/v0.2.0-beta.2
[0.2.0-beta.1]: https://github.com/VictorLetirand/inkboard/releases/tag/v0.2.0-beta.1
