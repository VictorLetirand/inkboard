# Changelog

All notable changes to Inkboard are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
follows [Semantic Versioning](https://semver.org/) (pre-releases carry a hyphen, e.g. `0.2.0-beta.1`).

## [Unreleased]

### Fixed

- **Mermaid import:** the content of each subgraph is laid out on its own and placed as one block, so the frames of subgraphs no longer overlap (nested ones stay inside their parent), and the `direction` written in a subgraph is honoured when no link joins it to the outside (state diagrams: composite states too).
- **Mermaid import:** a link written to a subgraph (`B --> Back`) no longer creates a stray box named like the subgraph: it is attached to the subgraph's frame. A `;` inside a quoted label (`A["a; b"]`) no longer cuts the node in two.
- **MCP server:** a page given as text (`"2"`) selects that page number; `batch` accepts the `inkboard_` prefix that some clients add to tool names.

### Added

- **UML and entity-relationship arrowheads:** diamond (filled and hollow), hollow triangle and the crow's foot family (many, one or many, zero or many, exactly one, zero or one), in the arrowhead buttons of the style panel, in the MCP tools (`head_start`, `head_end`), in the SVG / PNG / PDF exports, in draw.io (`diamond`, `ER…`, `endFill`) and Excalidraw (`diamond`, `crowfoot_…`). They are stored in separate optional fields of the connector, next to the original heads, which keep the closest one: an older version ignores the new fields and draws that head, and a board that does not use them is written exactly as before. Class and entity-relationship diagrams imported from Mermaid now use them (the «composition» / «aggregation» names on the links are gone).
- **Mermaid import:** `classDiagram` and `erDiagram`. A class or an entity becomes a table (name, attributes, methods; or type, name, keys and comment) and links become bound connectors: inheritance and realization end in a hollow triangle, composition and aggregation in a diamond, and an entity link ends in crow's foot symbols and spells out its cardinalities. `direction` is honoured.
- **MCP `export`:** creates the missing folders of `output` (under the allowed root, as `render` does), and a `.png` can be given a `width` in pixels or a `scale`, so that diagrams come out at the same width.

### Changed

- **Automatic layout** (Mermaid import, MCP `import_mermaid`, the *Auto layout* command): large and cyclic graphs are drawn much more compactly. Cycles are broken by reversing fewer edges, a node that only points far down is no longer left on the first rank, the crossing reduction tries three starting orders and swaps neighbours, and the lines of long edges run closer together. On a random graph of 40 modules with cycles, the drawing is about half as high with about half the edge length; on layered dependency graphs, crossings drop by 45 to 50 %. Only the positions computed by the layout change: files and the sharing protocol are untouched, and boards already saved keep their positions until the layout is run again.

## [0.2.0-beta.1] - Unreleased

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

[0.2.0-beta.1]: https://github.com/VictorLetirand/inkboard/releases/tag/v0.2.0-beta.1
