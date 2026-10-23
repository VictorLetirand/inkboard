# Browser guest on other engines (lot T1)

*2026-10-23, branch `test/browsers` (from a7b9160). Same bench as lot F3 (`crates/inkboard-app/web/test/e2e.mjs`: a real native host, browser guests, public n0 relay), made engine-independent (`drivers.mjs`) and replayed on Edge, Firefox and a WebKit build. Screenshots and raw logs: `screenshots/t1/<engine>/` (`run.log` = the bench, `smoke-public.log` = the public page, `insecure/` = the page over plain `http://` on a LAN address).*

**What was NOT tested: Safari itself, iOS / iPadOS, any real phone or tablet, a real corporate gateway.** The WebKit column is Playwright's WebKit build for Windows (`webkit-2370`, reports `Version/27.2 Safari/605.1.15`): the same engine family as Safari, not Safari (other graphics back end, no Apple media / clipboard / permission layers, no Safari UI). Treat it as "WebKit does not choke on the page", nothing more.

## Result

| Engine (version, how driven) | Bench `e2e.mjs` | Public page (`https://victorletirand.github.io/inkboard/`) | Page over `http://<LAN>` (insecure context) |
|---|---|---|---|
| Chrome 155, Windows 11, headless, CDP | 69 PASS, 0 FAIL | starts, WebGL 2, draws | starts, draws |
| Edge 155 (Chromium), headless, CDP | 69 PASS, 0 FAIL | starts, WebGL 2, draws | starts, draws |
| Firefox 158.0.1 (stock, Mozilla installer, signature checked), headless, WebDriver BiDi | 67 PASS, 0 FAIL, 2 SKIP | starts, WebGL 2 (ANGLE / D3D11), draws | starts, draws |
| WebKit (Playwright 1.64.1 build 2370; **Safari approximation**), headless | 65 PASS, 0 FAIL, 4 SKIP | starts, WebGL 2, draws | starts, draws |

The bench has 65 checks of lot F3 plus: the "Copy as image" / "Copy the diagnostic" checks now also look at what the application handed to the clipboard (and read it back where the engine allows it), and the new scenario `v` (a tab hidden by another tab, then visible again: `visibilitychange`). `SKIP` = the engine cannot make that check through its automation; it states why and is never counted as a pass.

Engine x function (what the bench exercised; "=" same as Chrome):

| Function | Chrome | Edge | Firefox | WebKit (approx.) |
|---|---|---|---|---|
| Loads: `.wasm` (18 MB), WebGL 2 context, no `SharedArrayBuffer` needed | PASS | = | = | = |
| Loading bar and size while the `.wasm` arrives | PASS | = | = | = (99 %: the bar goes up to the end quickly) |
| "too old" (no WebAssembly) and "no WebGL" messages | PASS | = | = | = |
| Join by link (fragment), ticket removed from the address bar, name cleaned and remembered | PASS | = | = | = |
| Snapshot, edit both ways, 2 guests see each other's cursors, MCP agent cursor | PASS | = | = | = |
| Read-only link, invalid link then valid link | PASS | = | = | = |
| Image drop, `.inkboard` download (Ctrl+S), "Save a copy" after the host left | PASS | = | = | = (the file is saved by Playwright's download handler) |
| Image paste | PASS (synthetic paste event) | = | PASS with a **real** Ctrl+V (a synthetic paste event loses its files in Firefox: bench limit, see below) | PASS (synthetic) |
| "Copy as image" | PASS, read back `image/png` | = | PASS, read back `image/png` | **SKIP**: this build refuses any image write to the clipboard (a minimal page gets `NotAllowedError` too) |
| "Copy the diagnostic" (enterprise message) | PASS, read back | = | PASS, read back | PASS (text handed over and accepted); read-back SKIP (automation cannot read the clipboard) |
| Enterprise message, 3 tips, "Try again", "Close", French | PASS | = | = (relay answered 403 by the bench proxy) | = (same) |
| Network cut, "reconnecting", pending change kept, back live, change reaches the host | PASS | = | PASS through the proxy: **no `online` / `offline` event** (2 SKIP), resumed by the page's own retries | PASS: Playwright's offline mode sent both events and the proxy dropped the tunnels |
| `visibilitychange` (tab hidden, then visible) | PASS (hidden then visible, 2 events) | = | = | **SKIP** for the hidden half: a background tab stays `visible` in automation; the tab is live afterwards |
| Phone profile: fits the width, 40 px targets, joins, banner fits | PASS (Pixel 5 emulation) | = | PASS with a 393x851 viewport + coarse-pointer preference; **no real touch events** | PASS (`iPhone 13` profile, touch) |
| Console without errors | PASS | = | PASS (relay failures logged as `pageerror` by Firefox during cuts / blocked relay are filtered) | PASS (`Viewport argument key "interactive-widget" not recognized` filtered, see below) |

## What differs between engines (observed)

- **Firefox**: `new ClipboardEvent('paste', {clipboardData})` made by a script gives an EMPTY `files` list (the page's listener sees nothing). The bench therefore pastes for real there: the page puts a PNG on the system clipboard from a key press (user activation), then a real Ctrl+V; the application receives the image, inserts it and sends it to the host. So real pastes work; only the synthetic way of testing does not. `navigator.clipboard.read()` works in the bench thanks to the testing preference `dom.events.testing.asyncClipboard`; without it a person gets Firefox's "Paste" prompt (not tested). Firefox logs failed relay requests (`Cross-Origin Request Blocked … /ping`, `Firefox can't establish a connection to the server at wss://…`) as page errors during a cut or a blocked relay: expected noise, not a defect.
- **Firefox (phone)**: no touch emulation through BiDi (`navigator.maxTouchPoints` stays 0); the bench sets `layout.css.devPixelsPerPx=2` and `ui.primaryPointerCapabilities=1` (coarse pointer) so that the touch-sized interface is drawn, and the "tap" is a BiDi pointer action. The pinch gesture, the virtual keyboard and real touch latency are not covered.
- **WebKit**: `navigator.clipboard.write([ClipboardItem(image/png)])` is refused (`NotAllowedError`) by Playwright's Windows build even from a key press and with a Promise-form `ClipboardItem` (eframe already uses the Promise form). Text copies work. The application shows its "Image copied" toast even though the browser refused the write (eframe does not report the rejected promise to the application): to check on real Safari, and possibly to fix in a later lot (not a "simple" patch: it needs the promise result from eframe). `visibilitychange`: not reproduced in headless. Console: Safari's engine logs `Viewport argument key "interactive-widget" not recognized and ignored` (the `interactive-widget=resizes-content` of the viewport meta is for Chrome / Android; harmless, kept).
- **Insecure context** (page opened over `http://<LAN address>`, not localhost): it starts and draws on the four engines (`crypto.getRandomValues`, WebGL 2, WebAssembly do not need a secure context). Not exercised there: joining a session (the relay WebSocket is `wss://` and should not depend on the page being secure) and the clipboard (`navigator.clipboard` is absent in an insecure context: "Copy as image" and "Copy the diagnostic" cannot work; the docs already say HTTPS or localhost only).
- **Download file names / Blob download**: the four engines produced the `.inkboard` file with every object; Firefox needed its download preferences (no prompt), WebKit the Playwright download hook (bench plumbing).
- **`SharedArrayBuffer`**: undefined on all four (the page is not cross-origin isolated) and not needed.

## Defects found

None in the application on these engines, so **no Rust code and no `index.html` message changed** (the "no WebGL" text keeps listing "Chrome, Edge, Firefox or Safari": nothing contradicted it; the "too old" text still suggests Chrome or Edge).
Observations that are not defects of the page but worth a look on real Safari: the "Image copied" toast when the write was refused (above), and the console message about `interactive-widget`.

## Limits

- Safari / iOS not validated (no Mac, no iPhone). Playwright's WebKit is a stand-in; WebGL there is the engine's own back end, so GPU-specific issues of Safari are invisible.
- Headless only; the phone profiles are emulations (Chrome / Edge: CDP; Firefox: viewport + preferences; WebKit: Playwright profile). No real touch keyboard, pinch, pointer pressure.
- Firefox and WebKit cuts go through a local proxy (relay refused with 403; tunnels dropped with 503): that is what a page sees when a gateway or a cut network closes the WebSocket, but it exercises neither the `online` / `offline` events (Firefox) nor a TLS-inspecting gateway. Chrome / Edge keep the F3 methods (`setOfflineMode`, `--host-resolver-rules`).
- No timing is asserted (the logs print them: the cut is noticed after about 2 s, live again 0.5 to 2.4 s after the network is back, on every engine).
- Run on one Windows 11 machine (NVIDIA GPU; Firefox reports ANGLE / D3D11, Chrome / Edge / WebKit mask the renderer as "WebKit WebGL") with the public relay.
- Everything installed for this lot lives outside the repository (`C:\Users\Utilisateur\browsers-test\`: Firefox 158.0.1 extracted from the official installer (SHA-512 from `archive.mozilla.org` and Authenticode signature of Mozilla Corporation checked), `playwright-core` 1.64.1 with its WebKit, about 0.5 GB in total).

## Replaying

```bash
# once, outside the repository: puppeteer-core (Chrome, Edge, Firefox) and playwright-core + its WebKit
mkdir ~/pptr && cd ~/pptr && npm i puppeteer-core
mkdir ~/pw && cd ~/pw && npm i playwright-core && PLAYWRIGHT_BROWSERS_PATH=~/pw-browsers node node_modules/playwright-core/cli.js install webkit
# Firefox: official installer from archive.mozilla.org, extracted with `setup.exe /ExtractDir=<folder>` (no install)
export PUPPETEER_DIR=~/pptr/node_modules/puppeteer-core
export CC_wasm32_unknown_unknown=<llvm>/bin/clang  AR_wasm32_unknown_unknown=<llvm>/bin/llvm-ar
cd crates/inkboard-app/web && trunk build --release --features web-glow,web-test --dist dist-e2e     # once (the bench builds it without --skip-build)
cd test
node e2e.mjs --browser chrome  --skip-build --out ../../../../context/mcp-remote/screenshots/t1/chrome
node e2e.mjs --browser edge    --skip-build --out …/t1/edge
FIREFOX=<firefox.exe> node e2e.mjs --browser firefox --skip-build --out …/t1/firefox
PLAYWRIGHT_DIR=~/pw/node_modules/playwright-core PLAYWRIGHT_BROWSERS_PATH=~/pw-browsers node e2e.mjs --browser webkit --skip-build --out …/t1/webkit
node smoke-public.mjs --browser <engine> [--url https://…/]        # the published page only
```
Each run takes about 4 to 6 minutes (Internet needed) and prints PASS / FAIL / SKIP lines; the exit code is 1 when a check fails. See `crates/inkboard-app/web/test/README.md`.
