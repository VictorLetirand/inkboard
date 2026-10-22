# Inkboard in the browser

*Version française : [fr/WEB.md](fr/WEB.md)*

The same application runs in a browser: it is compiled to WebAssembly (`wasm32-unknown-unknown`) and drawn on a `<canvas>`. The web version
is the local application only (draw, edit, undo, icon library, open / download files); joining a shared session comes with the web
transport and is not connected yet.

## Build the web version

Needs Rust 1.91 or later, the WebAssembly target, and [Trunk](https://trunkrs.dev):

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk wasm-bindgen-cli --locked
```

Then, from the repository root:

```bash
scripts/build_web.sh          # Windows: .\scripts\build_web.ps1
```

This runs `trunk build --release` in `crates/inkboard-app/web/` and writes the static page to **`crates/inkboard-app/web/dist/`**:
`index.html`, a `.js` and a `.wasm` file, and `icons.bin` (the icon library, 8 MB). Any static web server can host that folder (the links are
relative); to try it locally run `trunk serve` in `crates/inkboard-app/web/` and open <http://127.0.0.1:8080>.

- **Renderer**: WebGL (`glow`, the default, lighter). `scripts/build_web.sh wgpu` (Windows: `-Renderer wgpu`) builds the WebGPU renderer with a WebGL fallback
  (`web-wgpu` feature, a bit larger).
- **Toolchain**: if the Rust that `rustup` selects is older than 1.91, set `RUSTUP_TOOLCHAIN` (for example `RUSTUP_TOOLCHAIN=1.99.1`).
- The desktop build is not affected: `cargo build -p inkboard-app` is unchanged and still embeds the icon pack.

## What differs from the desktop application

| | Desktop | Browser |
|---|---|---|
| Icon library | embedded in the program | `icons.bin` is downloaded the first time the library is opened (a loading indicator is shown), then cached by the browser |
| Open | file dialog | the browser's file picker, or drop an image on the page |
| Save / Export | writes the file | **downloads** the file |
| Clipboard | system clipboard | paste text and images with Ctrl+V; "copy as image" uses the browser's asynchronous clipboard (secure pages only: HTTPS or localhost); the "Paste" menu item pastes the board's own clipboard only |
| Autosave, settings, custom shortcuts | files of the data folder | **not kept** between visits (download the board to keep it) |
| Fonts | system fonts | built-in fonts (text may wrap differently from a desktop board) |
| Pen pressure, eraser end | yes (Windows) | no (the browser's pointer events carry no pressure here) |
| MCP agent (live mode), file watching | yes | no |
| Sharing a session (host) | yes | never (the web version can only join) |

Recent versions of Chrome, Edge, Firefox and Safari are the target. The page needs WebGL 2 (or WebGPU in the `web-wgpu` build): without it a
message explains what to do.

## Test the page

`crates/inkboard-app/web/test/page.mjs` loads the built page in an installed Chrome (`puppeteer-core`), creates a shape, types text, opens
the icon library (and checks that the pack is requested only then), downloads and re-opens a board, pastes and drops an image, checks that two
sessions never draw the same object ids, and saves screenshots to `web/test/results/`:

```bash
cd crates/inkboard-app/web/test
npm install
node page.mjs            # CHROME=<path> if Chrome is not in its default Windows location
node sizes.mjs ../dist   # raw / gzip / brotli sizes
```

Keep `node_modules` out of the repository tree when you can: the repository's test that looks for French text in sources walks every folder.
