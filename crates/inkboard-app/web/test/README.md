# Web page tests (Chrome, Edge, Firefox, WebKit)

Run by hand, never by `cargo test`. `page.mjs` needs an installed Chrome (`CHROME=<path to chrome.exe>` if it is not in its default Windows location);
`e2e.mjs` and `smoke-public.mjs` drive four engines (see "Engines" below).
Keep `node_modules` out of the repository tree (the French-text test of the repo walks every folder): install `puppeteer-core` elsewhere and
point `PUPPETEER_DIR` (`e2e.mjs`) or `NODE_PATH` (`page.mjs`) at it.

| Script | What it checks |
|---|---|
| `page.mjs` | the built page (`../dist`): start, shape, text, icon library, download/open, paste/drop, ids, no-WebGL message (Chrome only) |
| `e2e.mjs` | **a real native host and browser guests**, through the public n0 relay (Internet needed): the whole guest experience (see below) |
| `smoke-public.mjs` | a PUBLISHED page (default `https://victorletirand.github.io/inkboard/`) in one engine: starts, WebGL 2, a shape can be drawn |
| `drivers.mjs` | the engine drivers used by the two scripts above (not run alone) |
| `sizes.mjs` | raw / gzip / brotli sizes of `../dist` |

## Engines (`--browser` or `BROWSER=`)

| `--browser` | Engine | Driver | Environment |
|---|---|---|---|
| `chrome` (default) | Chrome | puppeteer-core, CDP | `CHROME=<chrome.exe>` |
| `edge` | Edge (Chromium) | puppeteer-core, CDP | `EDGE=<msedge.exe>` |
| `firefox` | stock Firefox | puppeteer-core, WebDriver BiDi | `FIREFOX=<firefox.exe>` |
| `webkit` | Playwright's WebKit build: **an approximation of Safari, not Safari** | `playwright-core` | `PLAYWRIGHT_DIR=<…/node_modules/playwright-core>`, `PLAYWRIGHT_BROWSERS_PATH=<folder>` (once: `node <playwright-core>/cli.js install webkit`) |

Everything is installed OUTSIDE the repository (puppeteer-core, playwright-core, the browsers). Firefox and WebKit have no CDP: they go through a local
proxy started by the bench (`createGate` in `drivers.mjs`) that plays the company gateway (the relay's domain gets a 403) and cuts the network (drops
the tunnels, 503). A cut made that way fires no `online` / `offline` event in Firefox: those two checks are then reported as `SKIP`. A `SKIP` line
states why; it is neither a pass nor a failure. Examples (Windows, bash):

```bash
node e2e.mjs --browser edge    --skip-build --out ../../../../context/mcp-remote/screenshots/t1/edge
node e2e.mjs --browser firefox --skip-build --out …/t1/firefox     # FIREFOX=<firefox.exe>
node e2e.mjs --browser webkit  --skip-build --out …/t1/webkit      # PLAYWRIGHT_DIR, PLAYWRIGHT_BROWSERS_PATH
node smoke-public.mjs --browser firefox                            # the public page
```

## `e2e.mjs` (lots F2, F3 and T1: the browser guest)

```bash
# once: puppeteer-core somewhere outside the repo, e.g.  mkdir ~/pptr && cd ~/pptr && npm i puppeteer-core
export PUPPETEER_DIR=~/pptr/node_modules/puppeteer-core
# `ring` (iroh) compiles C for wasm32: a clang with the WebAssembly backend (any LLVM >= 15; the Android NDK one works)
export CC_wasm32_unknown_unknown=<llvm>/bin/clang  AR_wasm32_unknown_unknown=<llvm>/bin/llvm-ar
node e2e.mjs                 # builds ../dist-e2e with trunk (release, features web-glow + web-test), then runs the scenarios
node e2e.mjs --skip-build    # reuse ../dist-e2e;  --headed to watch;  --out <dir> for the screenshots (default ./results-e2e)
# RUST_TOOLCHAIN=1.99.1 (default) picks the Rust toolchain; needs Internet (public relay) and takes about 4 minutes
```

The page is built with the `web-test` feature (adds `window.__inkboard_test`: `join`, `leave`, `copy_image`, `state()`; the join/session windows also report
where their widgets are, so the bench clicks or taps them like a person). The host is the `#[ignore]`d test `web_bench_host` of `inkboard-app`: a board of 5
objects shared through the public relay, an edit link and a read-only link, every request accepted with what its link allows; it obeys `add`, `count`,
`images`, `people`, `agent` (the MCP agent acts: guests see its cursor), `quit` on its standard input.

Scenarios (each prints PASS / FAIL lines; a failing scenario does not stop the others; no check asserts a duration):

| | |
|---|---|
| loading | progress bar and size while the `.wasm` arrives; "too old" (no WebAssembly) and "no WebGL" messages |
| a | link with the ticket in the fragment: the join box opens, ticket removed from the address bar, name typed (cleaned), join, snapshot (same object count as the host), name remembered and pre-filled |
| b | an object drawn in the browser reaches the host; a host change reaches the browser |
| images | Ctrl+V of an image (Firefox: a real paste of an image the page put on the real clipboard; a synthetic paste event loses its files there) and a dropped file reach the host (chunked upload); Ctrl+S downloads a `.inkboard` copy with every object; "Copy as image" |
| c | two browser guests: each sees the other's cursor and name; an object of one reaches the other |
| d | the host's MCP agent acts: both guests see its cursor |
| v | the tab is hidden by another tab (`visibilitychange`), then visible again: the guest is still live |
| e2 | network cut (`page.setOfflineMode`; Firefox / WebKit: the bench proxy): "reconnecting", a change kept as pending, `online` event, back live, the change reaches the host |
| f | read-only link: role `View`, drawing refused, nothing reaches the host |
| g | invalid link: the join window opens with the error and the link editable; a valid link pasted afterwards joins |
| m | emulated phone (touch, virtual keyboard focus, touch-size targets, fits the width, joins with a finger) |
| e1 | relay unreachable (Chrome / Edge `--host-resolver-rules`; Firefox / WebKit: the bench proxy answers 403): the enterprise message, its 3 tips, "Copy the diagnostic" (clipboard), "Try again", "Close"; in French; on the phone |
| z | the host ends the session: banner, "Save a copy of the board" downloads the board |

A phone is emulated by Chrome / Edge (`page.emulate`, Pixel 5) in a browser started with `--force-device-scale-factor=2`: the canvas is sized from the REAL pixels of the
screen, so an emulation whose scale factor differs from the real one would draw the interface at the wrong size. Firefox gets the same through the
preferences `layout.css.devPixelsPerPx=2`, `ui.primaryPointerCapabilities=1` (coarse pointer) and a 393x851 viewport (no real touch events: the "tap" is a
BiDi pointer action). WebKit uses Playwright's `iPhone 13` profile. Screenshots of every state go to `--out`. Exit code 1 when a check fails.
