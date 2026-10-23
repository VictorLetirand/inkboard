# Web page tests (Chrome + puppeteer-core)

Run by hand, never by `cargo test`. Needs an installed Chrome (`CHROME=<path to chrome.exe>` if it is not in its default Windows location).
Keep `node_modules` out of the repository tree (the French-text test of the repo walks every folder): install `puppeteer-core` elsewhere and
point `PUPPETEER_DIR` (`e2e.mjs`) or `NODE_PATH` (`page.mjs`) at it.

| Script | What it checks |
|---|---|
| `page.mjs` | the built page (`../dist`): start, shape, text, icon library, download/open, paste/drop, ids, no-WebGL message |
| `e2e.mjs` | **a real native host and the page as a guest**, through the public n0 relay (Internet needed) |
| `sizes.mjs` | raw / gzip / brotli sizes of `../dist` |

## `e2e.mjs` (lot F2: browser guest over iroh)

```bash
# once: puppeteer-core somewhere outside the repo, e.g.  mkdir ~/pptr && cd ~/pptr && npm i puppeteer-core
export PUPPETEER_DIR=~/pptr/node_modules/puppeteer-core
# `ring` (iroh) compiles C for wasm32: a clang with the WebAssembly backend (any LLVM >= 15; the Android NDK one works)
export CC_wasm32_unknown_unknown=<llvm>/bin/clang  AR_wasm32_unknown_unknown=<llvm>/bin/llvm-ar
node e2e.mjs                 # builds ../dist-e2e with trunk (release, features web-glow + web-test), then runs the scenario
node e2e.mjs --skip-build    # reuse ../dist-e2e;  --headed to watch;  RUST_TOOLCHAIN=1.99.1 (default) picks the Rust toolchain
```

Scenario: builds the page with the `web-test` feature (adds `window.__inkboard_test`), starts the native host (the `#[ignore]`d test
`web_bench_host` of `inkboard-app`: a board of 5 objects shared through the public relay, every request to join accepted), opens
`index.html#<ticket>` in Chrome, joins, checks that the session goes live, that the guest board has as many objects as the host's, that an object
added by the host reaches the browser, that a garbage link is refused, and that the ticket never reaches the static server. Screenshots go to
`results-e2e/`. Exit code 1 when a check fails.
