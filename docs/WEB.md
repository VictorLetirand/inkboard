# Inkboard in the browser

*Version française : [fr/WEB.md](fr/WEB.md)*

The same application runs in a browser: it is compiled to WebAssembly (`wasm32-unknown-unknown`) and drawn on a `<canvas>`. The web version
is the local application (draw, edit, undo, icon library, open / download files) **and a guest of shared sessions**: somebody who was sent a link
joins a host's board from the page, with nothing to install (see [Join a session from the browser](#join-a-session-from-the-browser)). A browser
cannot host a session.

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
- **C compiler**: the network transport (`iroh`, through `ring`) compiles C for WebAssembly, so the build needs a `clang` with the WebAssembly backend (LLVM 15 or later; on Linux the `clang` package). If `clang` is not the one on the `PATH`, set `CC_wasm32_unknown_unknown` and `AR_wasm32_unknown_unknown` to a `clang` and a `llvm-ar`.
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
| Joining a session | link or menu, direct connection when possible | link or menu, **through the relay only** (a page cannot open a direct connection) |

## Join a session from the browser

**The link** is the address of the page followed by `#` and the ticket: `https://<page address>/#<ticket>`. `<ticket>` is exactly the part
after `inkboard://join#` (or `inkboard://join/`) of the link that the host's application gives. The page also reads `#join=<ticket>`. The ticket is in
the fragment (after `#`): a browser never sends it to a server, so the web host of the page never sees it. As soon as the page has read it, it
**removes it from the address bar** (`history.replaceState`), so it stays neither in the address nor in the history entry. The page is published at
`https://victorletirand.github.io/inkboard/`, and the application builds the web link for you: in the Share window, **Copy web link** (set the
`INKBOARD_WEB_URL` environment variable to point it at another address, for example a page served on a local network). You can also give the guest
the page address and the native link.

**What the guest sees:**

1. The page loads (progress bar; about 5 MB the first time, cached afterwards).
2. If the address had a ticket, the **Join a session** window opens with the link ("Link received") and the **name** field, already selected; the
   name of the last visit is pre-filled (kept in the browser's local storage under `inkboard.display_name`: this device only) and cleaned like on the
   desktop (control characters, extra spaces, 40 characters). Without a ticket in the address: main menu, **Join a session…**, then paste a link or a
   ticket (`inkboard://join#…`, `https://…#…` or the bare ticket). A link that is not valid opens the window with the reason and the text to correct.
   The host's name is not in the ticket: it appears once the host has answered ("Session of *name*").
3. **Join**, then the host's approval ("Waiting for the host's approval…", then the board). The guest edits and sees the other participants' cursors and
   names and the MCP agent's cursor, like an installed guest.
4. **States and errors** are the application's own, translated (French / English): connecting, "Reconnecting…" after about 2.5 s without news (changes
   kept and sent later), refusal by the host, session full, link not valid any more, host gone (with **Save a copy of the board**), and, when the relay cannot
   be reached (company proxy, school network), the message with its **three things to try** and **Copy the diagnostic** (browser clipboard). A page cannot
   probe the relay or read a gateway's 403, so it only knows whether its WebSocket reached the relay: the message is generic but the tips are the same.
5. **Quick resume**: the page listens to the browser's `online` event and to `visibilitychange` (the tab visible again: a hidden tab draws no frame, so the
   session has been asleep) and reconnects at once instead of waiting for its pauses between attempts.
6. **Images**: Ctrl+V of a clipboard image and a file dropped on the page are inserted and sent to the host in pieces, acknowledged, with the same limits as
   the application's guests. "Copy as image" writes a PNG to the browser clipboard (HTTPS or localhost only). **Save** (Ctrl+S) and the export entries
   (PNG, SVG, PDF…) **download** a copy; there is no file dialog.
7. **Phone and tablet**: the page has a viewport that does not zoom by itself; in the Join window and the session bar, buttons and fields get touch-sized
   (40 px) when the main pointer is a finger, and the windows fit the width of the screen. The rest of the interface (toolbar, panels) is unchanged.

## Browsers

Tested, with the bench below (65+ checks; a real native host and the public relay), on Windows 11, headless:

| Engine | Result |
|---|---|
| **Chrome 155** (and a phone emulated by Chrome) | all checks pass |
| **Edge 155** (Chromium) | all checks pass |
| **Firefox 158** (stock, driven through WebDriver BiDi) | all checks pass; 2 checks cannot be made (the `online` / `offline` events: the network cut is made through a proxy, and the guest resumes through its own retries) |
| **WebKit** (Playwright's build for Windows, **an approximation of Safari, not Safari**) | all checks pass; 4 cannot be made with this build (it refuses any image write to the clipboard, the clipboard cannot be read by automation, a background tab stays visible) |

The public page (`https://victorletirand.github.io/inkboard/`) also starts and draws on the four engines, and so does a copy served over plain `http://` on a LAN address
(an insecure context: it starts and draws; the clipboard is not available there, as said above).
**Not tested: Safari itself, iOS / iPadOS, real phones or tablets** (virtual keyboard, pinch, real touch latency), a real company gateway. Differences seen: Firefox gives a
script-made paste event no files (a real Ctrl+V of an image works); in a WebKit build "Copy as image" was refused by the browser while the application still showed "Image copied"
(to check on a real Safari); Safari's engine logs an unknown viewport key (`interactive-widget`) in the console, harmlessly. A page that cannot run says why instead of
staying blank: no WebAssembly / WebSocket / `crypto.getRandomValues` ("this browser is too old"), neither WebGL 2 nor WebGPU ("no WebGL: turn on hardware acceleration…").

## Known limits (browser guest)

- Not a host; relay only (no direct path), so latency is that of the relay (~50 ms on the public one) and the public relays' bandwidth limit applies.
- The relay's domain must be reachable by the browser's network (see [SHARING.md](SHARING.md#restricted-networks-company-school)).
- Only the guest's display name is kept between visits; settings and autosave are not.
- No pen pressure; the "Paste" menu item pastes the board's own clipboard only (Ctrl+V works for images and text).
- The page is 18 MB uncompressed (4.7 MB Brotli, 7.7 MB gzip); the icon library (8 MB) is only fetched when opened.
- A hidden tab does not draw, so it does not process the session until it is visible again (the browser decides; the session catches up on return).

## Test the page

Two scripts, both by hand (never `cargo test`): `page.mjs` (the local page) and `e2e.mjs` (**a real native host and browser guests through the public relay;
needs Internet**).

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

`e2e.mjs` drives Chrome (or `--browser edge|firefox|webkit`) through the real interface and checks: the loading progress, the "too old" and "no WebGL" messages, a link with the ticket in the
fragment (name box, join, snapshot, ticket removed from the address bar), an object drawn in the browser reaching the host, pasted and dropped images reaching
the host, the download of a copy, two browser guests seeing each other's cursors and names, the MCP agent's cursor, a network cut with changes kept and resumed,
a read-only link, an invalid link, a relay that cannot be reached (enterprise message, three tips, copied diagnostic, in English and French), an emulated phone
(touch targets, virtual-keyboard focus, joining with a finger), the host leaving. It saves a screenshot of each state to `--out`
(`context/mcp-remote/screenshots/f3/`):

```bash
export PUPPETEER_DIR=<folder of puppeteer-core>      # outside the repository tree
export CC_wasm32_unknown_unknown=<clang>  AR_wasm32_unknown_unknown=<llvm-ar>   # for ring, as above
node crates/inkboard-app/web/test/e2e.mjs [--browser chrome|edge|firefox|webkit] [--skip-build] [--out <dir>] [--headed]
# Firefox: FIREFOX=<firefox.exe>; WebKit: PLAYWRIGHT_DIR=<playwright-core> PLAYWRIGHT_BROWSERS_PATH=<folder>; EDGE=<msedge.exe>
node crates/inkboard-app/web/test/smoke-public.mjs --browser firefox     # the published page only
```
