# Web page tests (Chrome + puppeteer-core)

Run by hand, never by `cargo test`. Needs an installed Chrome (`CHROME=<path to chrome.exe>` if it is not in its default Windows location).
Keep `node_modules` out of the repository tree (the French-text test of the repo walks every folder): install `puppeteer-core` elsewhere and
point `PUPPETEER_DIR` (`e2e.mjs`) or `NODE_PATH` (`page.mjs`) at it.

| Script | What it checks |
|---|---|
| `page.mjs` | the built page (`../dist`): start, shape, text, icon library, download/open, paste/drop, ids, no-WebGL message |
| `e2e.mjs` | **a real native host and browser guests**, through the public n0 relay (Internet needed): the whole guest experience (see below) |
| `sizes.mjs` | raw / gzip / brotli sizes of `../dist` |

## `e2e.mjs` (lots F2 and F3: the browser guest)

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
| images | Ctrl+V of an image and a dropped file reach the host (chunked upload); Ctrl+S downloads a `.inkboard` copy with every object; "Copy as image" |
| c | two browser guests: each sees the other's cursor and name; an object of one reaches the other |
| d | the host's MCP agent acts: both guests see its cursor |
| e2 | network cut (`page.setOfflineMode`): "reconnecting", a change kept as pending, `online` event, back live, the change reaches the host |
| f | read-only link: role `View`, drawing refused, nothing reaches the host |
| g | invalid link: the join window opens with the error and the link editable; a valid link pasted afterwards joins |
| m | emulated phone (touch, virtual keyboard focus, touch-size targets, fits the width, joins with a finger) |
| e1 | relay unreachable (Chrome `--host-resolver-rules`): the enterprise message, its 3 tips, "Copy the diagnostic" (clipboard), "Try again", "Close"; in French; on the phone |
| e3 | relay blocked **silently** (a local port that accepts and never answers; then a non-routable address): after short time-outs, the enterprise message (class `Unreachable`) and a diagnostic holding the facts (HTTPS probe timed out, no relay connection, URLs, browser, time, cause) |
| e4 | **HTTPS passes, WebSocket does not** (a local HTTPS look-alike of the relay: `/ping` answered with CORS like a relay, WebSocket upgrade refused with 403, or never answered): class `WebSocketBlocked`, its message and tips (first: allow WebSocket to `*.relay.n0.iroh.link`, port 443), in French, on the phone |
| z | the host ends the session: banner, "Save a copy of the board" downloads the board |
| y | after z, the real relay and a host that is gone: "the service answers, the host cannot be reached", diagnostic with the relay connection established |

`--only e3,e4,y` runs the scenarios whose names start with one of the prefixes (the host and the browsers are still started). The silent-block scenarios shorten the
page's time-outs through `window.__inkboard_test.set_timeouts(connect_ms, probe_ms, probe_delay_ms)` (web-test build only); no check asserts a duration.
e4 needs `openssl` (self-signed certificate; Git for Windows brings one) and Chrome's `--ignore-certificate-errors`.

A phone is emulated by Chrome (`page.emulate`, Pixel 5) in a browser started with `--force-device-scale-factor=2`: the canvas is sized from the REAL pixels of the
screen, so an emulation whose scale factor differs from the real one would draw the interface at the wrong size. Screenshots of every state go to `--out`.
Exit code 1 when a check fails.
