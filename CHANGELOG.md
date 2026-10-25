# Changelog

All notable changes to Inkboard are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
follows [Semantic Versioning](https://semver.org/) (pre-releases carry a hyphen, e.g. `0.2.0-beta.1`).

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
