// End-to-end bench of the browser guest: a REAL native host (headless, public n0 relay) and the built web page in an installed Chrome.
//
//   1. builds the page (trunk, feature `web-test` adds `window.__inkboard_test`) unless --skip-build;
//   2. starts the native host (the ignored test `web_bench_host` of inkboard-app: 5 objects, edit and read-only links on its standard output);
//   3. drives the page like a person (mouse, keyboard, finger on an emulated phone), through the real interface of the application:
//      loading screen, old browser / no WebGL messages, join by link (fragment) with name, editing, images (paste, drop), copies,
//      two guests seeing each other's cursors, the MCP agent's cursor, a read-only link, an invalid link, a relay that cannot be reached
//      (enterprise message with its tips and the diagnostic copy), a network cut and its return, the host leaving;
//   4. screenshots of every state (desktop and emulated phone) go to --out.
//
//   node e2e.mjs [--browser chrome|edge|firefox|webkit] [--skip-build] [--dist ../dist-e2e] [--out ./results-e2e] [--headed] [--only <prefix>[,<prefix>…]]   (scenario names)
//
// Needs Internet (the public relay), a browser (see drivers.mjs: CHROME / EDGE / FIREFOX / PLAYWRIGHT_DIR + PLAYWRIGHT_BROWSERS_PATH),
// puppeteer-core (PUPPETEER_DIR), trunk, and for `ring` a clang with the wasm32 target (CC_wasm32_unknown_unknown /
// AR_wasm32_unknown_unknown). RUST_TOOLCHAIN (default 1.99.1) selects the Rust toolchain. No check asserts a duration (waits are on
// conditions, with generous time-outs). Exit code 1 when a check fails; a check an engine cannot make is a SKIP, with its reason.
import http from 'node:http';
import https from 'node:https';
import net from 'node:net';
import { createRequire } from 'node:module';
import { spawn, spawnSync } from 'node:child_process';
import { readFileSync, existsSync, mkdirSync, rmSync, readdirSync, statSync, mkdtempSync } from 'node:fs';
import { join, extname, resolve, dirname } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { createDriver } from './drivers.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const argv = process.argv.slice(2);
const opt = (name, dflt) => { const i = argv.indexOf(`--${name}`); return i < 0 ? dflt : argv[i + 1]; };
const engine = opt('browser', process.env.BROWSER ?? 'chrome');
const dist = resolve(opt('dist', join(here, '../dist-e2e')));
const out = resolve(opt('out', join(here, 'results-e2e')));
const headed = argv.includes('--headed');
const only = opt('only', '');
const toolchain = process.env.RUST_TOOLCHAIN ?? '1.99.1';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const repo = resolve(here, '../../../..');
const downloads = mkdtempSync(join(tmpdir(), 'inkboard-e2e-'));
const drv = await createDriver(engine, { headed, downloads, out });
console.log(`--- browser: ${engine}`);

rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });

let failed = 0;
const check = (name, ok, detail = '') => { console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? `  (${detail})` : ''}`); if (!ok) failed++; };
/** A check this engine cannot make through its automation (not a pass, not a failure). */
const skip = (name, reason) => console.log(`SKIP  ${name}  (${reason})`);
const run = (cmd, args, options) => {
  const r = spawnSync(cmd, args, { stdio: 'inherit', ...options });
  if (r.status !== 0) throw new Error(`${cmd} ${args.join(' ')} failed (${r.status})`);
};
/** A scenario that fails (a check or an exception) does not stop the others. */
async function scenario(name, fn) {
  if (only && !only.split(',').some((o) => name.startsWith(o))) return;
  console.log(`--- ${name}`);
  try { await fn(); } catch (e) { console.error(`ERROR in "${name}": ${e.stack ?? e}`); failed++; }
}

/* ---------------------------------------------------------------- build */
if (!argv.includes('--skip-build')) {
  console.log('--- building the page (trunk, release, web-glow + web-test)');
  run('trunk', ['build', '--release', '--features', 'web-glow,web-test', '--dist', dist], {
    cwd: join(here, '..'),
    env: { ...process.env, RUSTUP_TOOLCHAIN: toolchain },
  });
}
check('the page is built', existsSync(join(dist, 'index.html')), dist);

/* ---------------------------------------------------------------- native host */
console.log('--- building the native host');
const build = spawnSync('cargo', [`+${toolchain}`, 'test', '-p', 'inkboard-app', '--bin', 'inkboard', '--no-run', '--message-format=json'], {
  cwd: repo, encoding: 'utf8', maxBuffer: 1 << 28,
});
const exe = build.stdout.split('\n').filter((l) => l.startsWith('{')).map((l) => JSON.parse(l))
  .find((m) => m.reason === 'compiler-artifact' && m.profile?.test && m.executable && m.target.name === 'inkboard')?.executable;
if (!exe) { console.error(build.stderr); throw new Error('cannot find the test executable of inkboard-app'); }

const host = spawn(exe, ['web_bench_host', '--ignored', '--nocapture'], { stdio: ['pipe', 'pipe', 'pipe'] });
const hostLines = [];
let hostBuffer = '';
host.stdout.on('data', (d) => {
  hostBuffer += d;
  const lines = hostBuffer.split('\n');
  hostBuffer = lines.pop();
  for (const l of lines.map((x) => x.trim()).filter(Boolean)) { hostLines.push(l); console.log(`[host] ${l.length > 100 ? `${l.slice(0, 100)}…` : l}`); }
});
host.stderr.on('data', (d) => process.stderr.write(`[host stderr] ${d}`));
/** The text after `prefix` of the first host line (at or after index `from`) that starts with it. */
const hostSays = async (prefix, from = 0, timeout = 120000) => {
  const t0 = Date.now();
  for (;;) {
    const line = hostLines.slice(from).find((l) => l.startsWith(prefix));
    if (line) return line.slice(prefix.length).trim();
    if (Date.now() - t0 > timeout) throw new Error(`the host did not print ${prefix}`);
    await sleep(100);
  }
};
/** Sends a command to the host and returns the answer line that follows. */
const hostAsk = async (command, prefix) => {
  const from = hostLines.length;
  host.stdin.write(`${command}\n`);
  return hostSays(prefix, from);
};
/** Waits until the host holds `n` objects (the number is asked again and again). */
const hostObjectsUntil = async (what, n, timeout = 90000) => {
  const t0 = Date.now();
  let count = -1;
  while (Date.now() - t0 < timeout) {
    count = Number(await hostAsk('count', 'COUNT '));
    if (count === n) return count;
    await sleep(500);
  }
  throw new Error(`gave up waiting for: ${what} (the host holds ${count} objects, expected ${n})`);
};

/* ---------------------------------------------------------------- static server */
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm', '.css': 'text/css', '.svg': 'image/svg+xml' };
const requests = [];
let slowWasm = false;
const server = http.createServer(async (req, res) => {
  const path = decodeURIComponent(new URL(req.url, 'http://x').pathname).replace(/^\/$/, '/index.html');
  requests.push(req.url);
  const file = join(dist, path);
  if (!file.startsWith(dist) || !existsSync(file)) { res.writeHead(404); return res.end(); }
  const body = readFileSync(file);
  if (path.endsWith('.wasm') && slowWasm) {
    // delivered in pieces with pauses, so that the loading screen can be seen moving
    slowWasm = false;
    res.writeHead(200, { 'content-type': 'application/wasm', 'content-length': body.length });
    for (let i = 0; i < body.length; i += 1 << 20) { res.write(body.subarray(i, i + (1 << 20))); await sleep(150); }
    return res.end();
  }
  res.writeHead(200, { 'content-type': mime[extname(file)] ?? 'application/octet-stream', 'content-length': body.length });
  res.end(body);
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const origin = `http://127.0.0.1:${server.address().port}`;
const url = `${origin}/index.html`;

/* ---------------------------------------------------------------- page helpers */
const logs = [];

/** Counts the `online` / `offline` events the page receives (the bench checks the browser really sends them). */
const countNetEvents = () => {
  window.__netEvents = { online: 0, offline: 0 };
  window.addEventListener('online', () => window.__netEvents.online++);
  window.addEventListener('offline', () => window.__netEvents.offline++);
};
/** Records what the application writes to the browser clipboard (and whether the browser accepted it): readable on every engine. */
const recordClipboard = () => {
  const rec = (window.__clip = { text: [], items: [], errors: [] });
  const c = navigator.clipboard;
  if (!c) return;
  const text = c.writeText?.bind(c);
  if (text) c.writeText = async (t) => { rec.text.push(String(t)); try { return await text(t); } catch (e) { rec.errors.push(`writeText: ${e}`); throw e; } };
  const write = c.write?.bind(c);
  if (write) c.write = async (items) => {
    rec.items.push(items.flatMap((i) => i.types));
    try { return await write(items); } catch (e) { rec.errors.push(`write: ${e}`); throw e; }
  };
};

async function newPage(context, label, { mobile = false, french = false } = {}) {
  const page = await drv.newPage(context);
  if (mobile) await drv.emulatePhone(page); else await page.setViewport({ width: 1360, height: 860 });
  page.on('console', (m) => logs.push({ label, text: `[${m.type()}] ${m.text()}` }));
  page.on('pageerror', (e) => logs.push({ label, text: `[pageerror] ${e.message ?? e}` }));
  await page.evaluateOnNewDocument(countNetEvents);
  await page.evaluateOnNewDocument(recordClipboard);
  if (french) {
    await page.evaluateOnNewDocument(() => {
      Object.defineProperty(navigator, 'language', { get: () => 'fr-FR' });
      Object.defineProperty(navigator, 'languages', { get: () => ['fr-FR', 'fr'] });
    });
  }
  page.label = label;
  page.mobile = mobile;
  return page;
}
const state = async (p) => JSON.parse(await p.evaluate(() => window.__inkboard_test.state()));
const until = async (p, what, cond, { timeout = 90000, tick } = {}) => {
  const t0 = Date.now();
  let s = await state(p);
  while (!cond(s)) {
    if (Date.now() - t0 > timeout) throw new Error(`[${p.label}] gave up waiting for: ${what} (last state: ${JSON.stringify({ ...s, ids: undefined, marks: undefined })})`);
    if (tick) await tick();
    await sleep(250);
    s = await state(p);
  }
  return s;
};
const shot = (p, name) => p.screenshot({ path: join(out, `${name}.png`) });
const started = (p) => p.waitForFunction(() => !document.getElementById('loading') && window.__inkboard_test, { timeout: 120000 });
/** Opens `url#fragment` and waits for the application to start. */
async function open(p, fragment = '') {
  await p.goto(fragment ? `${url}#${fragment}` : url, { waitUntil: 'load' });
  await started(p);
}
/** Clicks (or taps) the center of a widget of the application, found through the marks the interface leaves for the bench. */
async function press(p, name) {
  // the widget must be there and have stopped moving (a window changes size when its content changes): two equal positions in a row
  let last = null;
  for (let i = 0; i < 80; i++) {
    const m = (await state(p)).marks?.[name] ?? null;
    if (m && last && m.every((v, k) => Math.abs(v - last[k]) < 0.5)) {
      const [x, y, w, h] = m;
      if (p.mobile) await p.touchscreen.tap(x + w / 2, y + h / 2); else await p.mouse.click(x + w / 2, y + h / 2);
      return;
    }
    last = m;
    await sleep(300);
  }
  throw new Error(`[${p.label}] the widget ${name} never showed up (or never stopped moving)`);
}
/** Types a name in the field of the join window (the field already holds a name: it is replaced). */
async function typeName(p, name) {
  await press(p, 'join_name');
  await p.keyboard.down('Control');
  await p.keyboard.press('a');
  await p.keyboard.up('Control');
  await p.keyboard.type(name);
}
const isLive = (s) => s.state === 'live' && s.mirror && s.mirror_ready;
/** A drag with pauses: the application handles the pointer once per frame. */
async function drag(p, x0, y0, x1, y1) {
  await p.mouse.move(x0, y0);
  await sleep(60);
  await p.mouse.down();
  await sleep(60);
  for (let i = 1; i <= 12; i++) {
    await p.mouse.move(x0 + ((x1 - x0) * i) / 12, y0 + ((y1 - y0) * i) / 12);
    await sleep(25);
  }
  await p.mouse.up();
}
/** Draws a rectangle with the rectangle tool (key R) on the board. */
async function drawRect(p, x = 700, y = 420) {
  await p.mouse.click(1300, 800);
  await p.keyboard.press('r');
  await drag(p, x, y, x + 180, y + 110);
  await sleep(300);
}
const png = `(async () => {
  const c = document.createElement('canvas'); c.width = 160; c.height = 100;
  const g = c.getContext('2d'); g.fillStyle = '#e11d48'; g.fillRect(0, 0, 160, 100); g.fillStyle = '#fff'; g.fillRect(30, 30, 100, 40);
  return new Promise((r) => c.toBlob(r, 'image/png'));
})()`;
const downloadsSince = (since) => readdirSync(downloads).filter((f) => !f.endsWith('.crdownload') && statSync(join(downloads, f)).mtimeMs >= since);
async function waitDownload(since, ext) {
  for (let i = 0; i < 150; i++) {
    const f = downloadsSince(since).find((n) => n.endsWith(ext));
    if (f) { await sleep(200); return join(downloads, f); }
    await sleep(100);
  }
  return null;
}
const objectsOf = (file) => JSON.parse(readFileSync(file, 'utf8')).document.pages.flatMap((p) => p.objects);

/* ---------------------------------------------------------------- fake relays (silent network blocks, see "e3" and "e4") */
/** A TCP port that accepts connections and never answers: TLS and HTTP requests hang, like packets dropped by a firewall. */
async function startBlackhole() {
  const sockets = new Set();
  const srv = net.createServer((c) => { sockets.add(c); c.on('error', () => {}); c.on('close', () => sockets.delete(c)); });
  await new Promise((r) => srv.listen(0, '127.0.0.1', r));
  return { port: srv.address().port, accepted: () => sockets.size, close: () => { sockets.forEach((c) => c.destroy()); srv.close(); } };
}
/**
 * A relay look-alike that lets plain HTTPS requests through (204, no CORS header) but refuses (`upgrade: 'refuse'`: 403, like a
 * filtering proxy) or never answers (`upgrade: 'hang'`) the WebSocket upgrade. Self-signed certificate (the browser ignores it).
 */
async function startWebSocketBlocker(upgrade) {
  const dir = mkdtempSync(join(tmpdir(), 'inkboard-cert-'));
  const [key, cert] = [join(dir, 'key.pem'), join(dir, 'cert.pem')];
  const r = spawnSync('openssl', ['req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-keyout', key, '-out', cert, '-days', '2', '-subj', '/CN=fake-relay',
    '-addext', 'subjectAltName=DNS:*.relay.n0.iroh.link,DNS:*.iroh.link'], { encoding: 'utf8' });
  if (r.status !== 0) throw new Error(`openssl cannot make a certificate: ${r.stderr}`);
  const seen = { http: [], upgrades: 0 };
  const held = new Set();
  // `/ping` is the request iroh's own browser probe makes (a CORS `fetch`): answered like a real relay does, so that iroh picks this
  // relay as its home relay and then tries the WebSocket, as it does behind a proxy that lets plain HTTPS through
  const srv = https.createServer({ key: readFileSync(key), cert: readFileSync(cert) }, (req, res) => {
    seen.http.push(req.url);
    if (req.url === '/ping') { res.writeHead(200, { 'access-control-allow-origin': '*', 'content-type': 'text/plain' }); res.end('pong'); } else { res.writeHead(204); res.end(); }
  });
  srv.on('upgrade', (req, socket) => {
    seen.upgrades++;
    socket.on('error', () => {});
    if (upgrade === 'refuse') { socket.end('HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n'); } else { held.add(socket); }
  });
  await new Promise((r2) => srv.listen(0, '127.0.0.1', r2));
  return { port: srv.address().port, seen, close: () => { held.forEach((c) => c.destroy()); srv.close(); rmSync(dir, { recursive: true, force: true }); } };
}
/** Chrome rule that sends every relay name (both spellings: with and without the final dot) to `target`. */
const mapRelays = (target) => `--host-resolver-rules=MAP *.iroh.link ${target}, MAP *.iroh.link. ${target}`;
/** Short time-outs for the bench (the page's defaults are 15 s to connect, 5 s per probe): the knob exists only in the `web-test` build. */
const quickNet = (p) => p.evaluate(() => window.__inkboard_test.set_timeouts?.(8000, 3000, 1000));
const extraBrowsers = [];
/** Joins with `ticket` in a new browser started with `args` and returns the failure window's state (screenshot `shotName`). */
async function failedJoin(label, args, ticket, { french = false, shotName, mobile = false } = {}) {
  const b = await puppeteer.launch({ executablePath: chrome, headless: !headed, args: [...(mobile ? phoneArgs : chromeArgs), ...args] });
  extraBrowsers.push(b);
  const ctx = b.defaultBrowserContext();
  await ctx.overridePermissions(origin, ['clipboard-read', 'clipboard-write', 'clipboard-sanitized-write']);
  const P = await newPage(ctx, label, { french, mobile });
  await open(P, ticket);
  await quickNet(P);
  await until(P, 'the join box', (s) => s.window?.from_system);
  await press(P, 'join_go');
  const failed = await until(P, 'the failure', (s) => s.state === 'failed', { timeout: 120000 });
  await sleep(600);
  if (shotName) await shot(P, shotName);
  let diagnostic = failed.issue.diagnostic;
  if (diagnostic === undefined) { // a build without the field in the hook: the text the "Copy the diagnostic" button puts on the clipboard
    await press(P, 'issue_copy');
    await sleep(800);
    diagnostic = await P.evaluate(() => navigator.clipboard.readText().catch((e) => `error: ${e}`));
  }
  console.log(`[${label}] ${failed.issue.class}: ${failed.issue.headline}`);
  console.log(`[${label}] diagnostic:\n${diagnostic.split('\n').map((l) => `    ${l}`).join('\n')}`);
  return { page: P, state: failed, issue: failed.issue, diagnostic };
}

let browser;
let blockedBrowser;
let phoneBrowser;
let blockedPhoneBrowser;
let hostObjects = 0;
const regularPages = [];
try {
  const link = await hostSays('LINK ');
  const readLink = await hostSays('LINK_READ ');
  hostObjects = Number(await hostSays('OBJECTS '));
  const ticket = link.split('#')[1];
  const readTicket = readLink.split('#')[1];
  check('the host shares a board with objects (edit link and read-only link)', hostObjects > 0 && !!ticket && !!readTicket && ticket !== readTicket, `${hostObjects} objects`);

  browser = await drv.launch();
  const ctxA = browser.defaultContext();
  const ctxB = await browser.newContext();
  const ctxC = await browser.newContext();
  const canReadClipboard = (await drv.grant(ctxA, origin)) && drv.caps.clipboardRead;

  /* ---- loading screen, old browser, no WebGL */
  await scenario('loading screen with progress, old browser, no WebGL', async () => {
    const p = await newPage(ctxC, 'loading');
    slowWasm = true;
    await p.goto(url, { waitUntil: 'domcontentloaded' });
    const seen = { bar: 4, text: '' };
    for (let i = 0; i < 200 && (await p.evaluate(() => !!document.getElementById('loading'))); i++) {
      const v = await p.evaluate(() => ({ w: parseFloat(document.getElementById('loading-bar')?.style.width ?? '0'), t: document.getElementById('loading-size')?.textContent ?? '' }));
      if (v.w > seen.bar) seen.bar = v.w;
      if (v.t) seen.text = v.t;
      if (i === 8) await shot(p, 'h1-loading-desktop');
      await sleep(100);
    }
    check('the loading screen shows the download progress', seen.bar > 4 && /\//.test(seen.text), `bar up to ${seen.bar}%, "${seen.text}"`);
    await started(p);
    check('the page starts and the loading screen is gone', await p.evaluate(() => !document.getElementById('loading') && document.getElementById('error').hidden));
    await p.close();

    const old = await newPage(ctxC, 'old-browser');
    await old.evaluateOnNewDocument(() => { delete window.WebAssembly; });
    await old.goto(url, { waitUntil: 'load' });
    await old.waitForFunction(() => !document.getElementById('error').hidden, { timeout: 30000 });
    check('a browser without WebAssembly gets "too old" (not a blank page)', await old.evaluate(() => document.getElementById('error').dataset.case === 'old' && !document.getElementById('loading')));
    await shot(old, 'h2-browser-too-old');
    await old.close();

    const noGl = await newPage(ctxC, 'no-webgl');
    await noGl.evaluateOnNewDocument(() => { HTMLCanvasElement.prototype.getContext = () => null; Object.defineProperty(navigator, 'gpu', { get: () => undefined }); });
    await noGl.goto(url, { waitUntil: 'load' });
    await noGl.waitForFunction(() => !document.getElementById('error').hidden, { timeout: 30000 });
    check('a browser without WebGL nor WebGPU gets the WebGL message', await noGl.evaluate(() => document.getElementById('error').dataset.case === 'webgl'));
    await shot(noGl, 'h3-no-webgl');
    await noGl.close();
  });

  /* ---- a. link with fragment -> name box -> join -> snapshot */
  const A = await newPage(ctxA, 'alice');
  regularPages.push(A);
  await drv.setupDownloads(A);
  await scenario('a. link with the ticket in the fragment: name box, join, snapshot', async () => {
    await A.goto(`${url}#${ticket}`, { waitUntil: 'load' });
    await started(A);
    const boxed = await until(A, 'the join box', (s) => s.window?.from_system);
    check('the page opens the join box with the link of the address (no typing of the link)', boxed.window.from_system && boxed.state === 'none');
    check('the ticket was taken out of the address bar and the history entry', await A.evaluate(() => location.hash === '' && !location.href.includes('#')), await A.evaluate(() => location.href));
    check('the box offers a name, pre-filled', typeof boxed.window.name === 'string' && boxed.window.name.length > 0, boxed.window.name);
    await sleep(300);
    await shot(A, 'a1-join-box-desktop');
    await typeName(A, '  Alice\u0007  ');
    await sleep(300);
    await press(A, 'join_go');
    const joined = await until(A, 'the guest to hold the host board', (s) => (isLive(s) && s.objects > 0) || s.state === 'failed' || s.state === 'ended');
    await shot(A, 'a2-joined-desktop');
    check('the connection is established (live)', joined.state === 'live', JSON.stringify({ ...joined, ids: undefined, marks: undefined }));
    check('the host saw the guest by its (cleaned) name, with edit rights', (await hostSays('MEMBER ')) === 'Alice Edit');
    check('the snapshot arrived: same number of objects as the host', joined.objects === hostObjects, `${joined.objects} vs ${hostObjects}`);
    check('the host name came with the welcome', !!joined.host_name, joined.host_name);
    check('the guest has the edit role', joined.role === 'Edit' && !joined.read_only, joined.role);
    check('the name is remembered in the browser (local storage)', (await A.evaluate(() => localStorage.getItem('inkboard.display_name'))) === 'Alice');
    check('the ticket never reached the static server (URL fragment)', !requests.some((r) => r.includes(ticket.slice(0, 16))));

    // a second tab of the same browser profile: the name is pre-filled
    const again = await newPage(ctxA, 'alice-again');
    await again.goto(`${url}#${ticket}`, { waitUntil: 'load' });
    await started(again);
    const pre = await until(again, 'the join box', (s) => s.window?.from_system);
    check('the name of the last visit is pre-filled', pre.window.name === 'Alice', pre.window.name);
    await again.close();
    await A.bringToFront();
  });

  /* ---- b. the browser guest creates an object -> it appears at the native host */
  await scenario('b. an object created in the browser reaches the host', async () => {
    await drawRect(A);
    await shot(A, 'b1-guest-drew');
    const mine = await state(A);
    check('the new object is on the guest board', mine.objects === hostObjects + 1, `${mine.objects}`);
    await hostObjectsUntil('the new object to reach the host', hostObjects + 1);
    check('the object created in the browser appears at the native host', true);
    const hostCount = hostObjects + 1;
    const after = Number(await hostAsk('add', 'ADDED '));
    const synced = await until(A, 'the host change to reach the browser', (s) => s.objects === hostCount + 1);
    check('a modification of the host reaches the browser', synced.objects === after, `${synced.objects} objects`);
    hostObjects = after;
  });

  /* ---- images: paste, drop, copies */
  await scenario('images: paste and drop reach the host; copies for the guest', async () => {
    const s0 = await state(A);
    const hostImages = async (n) => {
      for (let i = 0; i < 240; i++) { if (Number(await hostAsk('images', 'IMAGES ')) === n) return true; await sleep(500); }
      return false;
    };
    // A synthetic paste event carries its files in Chrome and in Safari's engine; Firefox drops them (the DataTransfer of a clipboard
    // event made by a script is empty). There the page puts the image on the real clipboard (a key press gives it the user activation)
    // and a real Ctrl+V pastes it: the application's listener receives what a person's paste gives.
    let pasted = null;
    try {
      if (drv.caps.syntheticPaste === false) {
        await A.evaluate((code) => {
          window.addEventListener('keydown', async function once(e) {
            if (e.key !== 'F9') return;
            window.removeEventListener('keydown', once, true);
            try { await navigator.clipboard.write([new ClipboardItem({ 'image/png': await eval(code) })]); window.__clipReady = true; } catch (err) { window.__clipReady = String(err); }
          }, true);
        }, png);
        await A.mouse.click(1300, 800);
        await A.keyboard.press('F9');
        await A.waitForFunction(() => window.__clipReady, { timeout: 15000 });
        const ready = await A.evaluate(() => window.__clipReady);
        if (ready !== true) throw new Error(`the page could not put the image on the clipboard: ${ready}`);
        await A.keyboard.down('Control'); await A.keyboard.press('v'); await A.keyboard.up('Control');
      } else {
        await A.evaluate(async (code) => {
          const blob = await eval(code);
          const dt = new DataTransfer();
          dt.items.add(new File([blob], 'image.png', { type: 'image/png' }));
          document.dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true }));
        }, png);
      }
      pasted = await until(A, 'the pasted image on the board', (s) => s.images === s0.images + 1, { timeout: 30000 });
      await sleep(500);
      await shot(A, 'i1-pasted-image');
    } catch (e) { console.log(`  (paste: ${String(e.message ?? e).slice(0, 200)})`); }
    check('a pasted image (Ctrl+V of a clipboard image) is inserted on the guest board', pasted?.images === s0.images + 1);
    check('the pasted image is sent to the host (chunked upload, acknowledged)', pasted !== null && (await hostImages(s0.images + 1)));
    const base = pasted ? s0.images + 1 : s0.images;
    await A.evaluate(async (code) => {
      const blob = await eval(code);
      const dt = new DataTransfer();
      dt.items.add(new File([blob], 'dropped.png', { type: 'image/png' }));
      const target = document.getElementById('inkboard_canvas');
      for (const t of ['dragover', 'drop']) target.dispatchEvent(new DragEvent(t, { dataTransfer: dt, bubbles: true, cancelable: true, clientX: 500, clientY: 500 }));
    }, png);
    await until(A, 'the dropped image on the board', (s) => s.images === base + 1, { timeout: 30000 });
    check('a dropped image file is inserted and sent to the host', await hostImages(base + 1));
    hostObjects = Number(await hostAsk('count', 'COUNT '));

    // "Download a copy": the board leaves as a .inkboard file, with what the host holds
    const since = Date.now() - 20;
    await A.mouse.click(1300, 800);
    await A.keyboard.down('Control');
    await A.keyboard.press('s');
    await A.keyboard.up('Control');
    const file = await waitDownload(since, '.inkboard');
    check('Ctrl+S downloads a copy of the shared board (.inkboard file)', !!file, file && file.split(/[\\/]/).pop());
    if (file) check('the copy holds every object of the shared board', objectsOf(file).length === hostObjects, `${objectsOf(file).length} vs ${hostObjects}`);
    // PNG export from the guest
    const sincePng = Date.now() - 20;
    // asked from a key press, as a person would (Safari's engine refuses a clipboard write that follows no user gesture)
    await A.evaluate(() => window.addEventListener('keydown', function once(e) {
      if (e.key === 'F8') { window.removeEventListener('keydown', once, true); window.__inkboard_test.copy_image(); }
    }, true));
    await A.keyboard.press('F8');
    await sleep(800);
    const written = await A.evaluate(() => window.__clip);
    const accepted = written.items.some((t) => t.includes('image/png')) && written.errors.length === 0;
    // what the engine itself does with the smallest possible image write inside a key press (Playwright's WebKit build refuses even that)
    const minimal = accepted ? true : await (async () => {
      await A.evaluate(() => window.addEventListener('keydown', function once(e) {
        if (e.key !== 'F7') return;
        window.removeEventListener('keydown', once, true);
        const c = document.createElement('canvas'); c.width = c.height = 4;
        const blob = new Promise((r) => c.toBlob(r, 'image/png'));
        navigator.clipboard.write([new ClipboardItem({ 'image/png': blob })]).then(() => { window.__imgProbe = 'ok'; }, (err) => { window.__imgProbe = String(err); });
      }, true));
      await A.keyboard.press('F7');
      await sleep(800);
      return (await A.evaluate(() => window.__imgProbe)) === 'ok';
    })();
    if (!accepted && !minimal) skip('"Copy as image": the browser accepts an image/png', 'this engine refuses ANY image write to the clipboard in automation (a minimal page gets NotAllowedError too): the application cannot be judged');
    else check('"Copy as image": the application hands an image/png to the browser clipboard and the browser accepts it', accepted, JSON.stringify(written));
    if (canReadClipboard) {
      const clip = await A.evaluate(async () => {
        try { const items = await navigator.clipboard.read(); return items.flatMap((i) => i.types); } catch (e) { return `error: ${e}`; }
      });
      check('"Copy as image" puts an image on the browser clipboard (read back)', Array.isArray(clip) && clip.includes('image/png'), JSON.stringify(clip));
    } else skip('"Copy as image" read back from the clipboard', 'this engine does not let automation read the clipboard');
    void sincePng;
  });

  /* ---- c. two browser guests and the host see each other's cursors */
  const B = await newPage(ctxB, 'bob');
  regularPages.push(B);
  await scenario('c. two browser guests: each sees the cursor and name of the other', async () => {
    await B.goto(`${url}#${ticket}`, { waitUntil: 'load' });
    await started(B);
    await until(B, 'the join box', (s) => s.window?.from_system);
    await typeName(B, 'Bob');
    await B.keyboard.press('Enter');
    await until(B, 'Bob to hold the board', (s) => isLive(s) && s.objects > 0);
    await shot(B, 'c1-second-guest');
    const people = (await hostAsk('people', 'PEOPLE ')).split(',').sort();
    check('the host lists both browser guests', people.includes('Alice') && people.includes('Bob'), people.join(','));
    const wiggle = (p, x, y) => async () => { await p.mouse.move(x, y); await sleep(40); await p.mouse.move(x + 60, y + 40); };
    const sees = (who) => (s) => s.people.some((q) => q.name === who && q.visible && q.cursor);
    await until(B, 'Bob to see the cursor of Alice', sees('Alice'), { tick: wiggle(A, 500, 500) });
    await until(A, 'Alice to see the cursor of Bob', sees('Bob'), { tick: wiggle(B, 600, 600) });
    await shot(A, 'c2-sees-the-other-cursor');
    check('Bob sees the cursor and name of Alice', true);
    check('Alice sees the cursor and name of Bob', true);
    // an object of Bob is seen by Alice
    const before = (await state(A)).objects;
    await drawRect(B, 300, 560);
    await until(A, 'the object of Bob to reach Alice', (s) => s.objects === before + 1);
    check('an object created by one guest reaches the other guest (through the host)', true);
    hostObjects = Number(await hostAsk('count', 'COUNT '));
    await A.bringToFront();
  });

  /* ---- d. the MCP agent's cursor */
  await scenario('d. the MCP agent of the host: guests see its cursor', async () => {
    const agentVisible = (s) => s.agent?.visible && s.agent.cursor;
    host.stdin.write('agent\n');
    await hostSays('AGENT');
    const seenA = await until(A, 'Alice to see the agent', agentVisible, { timeout: 30000 });
    await shot(A, 'd1-agent-cursor');
    check('the guest sees the cursor of the agent, with its name', seenA.agent.name === 'Claude (agent)', seenA.agent.name);
    const seenB = await state(B);
    check('the other guest sees it as well', !!agentVisible(seenB) || !!(await until(B, 'Bob to see the agent', agentVisible, { timeout: 30000 }).catch(() => null)));
  });

  /* ---- v. the tab is hidden by another tab, then visible again */
  await scenario('v. tab hidden then visible again (visibilitychange): the guest stays or comes back live', async () => {
    await A.evaluate(() => {
      window.__vis = { events: 0, states: [] };
      document.addEventListener('visibilitychange', () => { window.__vis.events++; window.__vis.states.push(document.visibilityState); });
    });
    const other = await newPage(ctxA, 'other-tab');
    await other.goto('about:blank');
    await other.bringToFront();
    await sleep(1500);
    const hidden = await A.evaluate(() => ({ state: document.visibilityState, ...window.__vis }));
    await A.bringToFront();
    await sleep(1500);
    const shown = await A.evaluate(() => ({ state: document.visibilityState, ...window.__vis }));
    if (hidden.state === 'hidden') check('the browser tells a hidden tab (visibilitychange fired, state hidden)', hidden.events >= 1, JSON.stringify(hidden));
    else skip('the browser tells a hidden tab', `this engine keeps a background tab "${hidden.state}" in automation`);
    check('the tab is visible again and the guest is live with the same board', shown.state === 'visible' && isLive(await state(A)), JSON.stringify(shown));
    await other.close();
    await A.bringToFront();
  });

  /* ---- e2. network cut and return */
  await scenario('e2. network cut: reconnecting, pending changes kept, quick resume when the network is back', async () => {
    await A.bringToFront();
    const cutAt = Date.now();
    await drv.setOffline(A, true);
    const rec = await until(A, 'the guest to notice the cut', (s) => s.state === 'reconnecting', { timeout: 60000 });
    await shot(A, 'e2a-reconnecting-desktop');
    console.log(`  (cut noticed after ${Date.now() - cutAt} ms, not asserted)`);
    check('after the cut the page shows "reconnecting" (the board stays)', rec.state === 'reconnecting' && rec.mirror, `attempt ${rec.attempt}`);
    const offlineEvents = await A.evaluate(() => window.__netEvents.offline);
    if (drv.caps.netEvents === true) check('the browser sent the offline event', offlineEvents >= 1);
    else if (offlineEvents >= 1) check('the browser sent the offline event', true, 'engine without a guaranteed cut event');
    else skip('the browser sent the offline event', "the cut is made through a proxy or this engine's emulation: no browser event");
    const hostBefore = Number(await hostAsk('count', 'COUNT '));
    await drawRect(A, 700, 250);
    const held = await until(A, 'the change to be kept as pending', (s) => s.pending > 0, { timeout: 20000 });
    check('a change made while disconnected is kept, waiting to be sent', held.pending > 0, `${held.pending} pending`);
    check('the host has not seen it yet', Number(await hostAsk('count', 'COUNT ')) === hostBefore);
    await shot(A, 'e2b-pending-desktop');
    const backAt = Date.now();
    await drv.setOffline(A, false);
    const back = await until(A, 'the guest to be live again', (s) => s.state === 'live' && s.pending === 0, { timeout: 90000 });
    console.log(`  (live again ${Date.now() - backAt} ms after the network came back, not asserted)`);
    const onlineEvents = await A.evaluate(() => window.__netEvents.online);
    if (drv.caps.netEvents === true) check('the browser sent the online event (the page listens to it to reconnect at once)', onlineEvents >= 1);
    else if (onlineEvents >= 1) check('the browser sent the online event (the page listens to it to reconnect at once)', true, 'engine without a guaranteed cut event');
    else skip('the browser sent the online event', "no browser event with this cut: the guest resumed through its own retries");
    check('the guest is live again after the network returns, nothing pending', back.state === 'live' && back.pending === 0);
    await hostObjectsUntil('the pending change to reach the host', hostBefore + 1);
    check('the change made during the cut reached the host after the reconnection', true);
    hostObjects = hostBefore + 1;
    await shot(A, 'e2c-resumed-desktop');
  });

  /* ---- f. read-only link */
  await scenario('f. read-only link: the browser guest cannot edit', async () => {
    const C = await newPage(ctxC, 'carol');
    regularPages.push(C);
    await open(C, readTicket);
    await until(C, 'the join box', (s) => s.window?.from_system);
    await typeName(C, 'Carol');
    await press(C, 'join_go');
    const s = await until(C, 'Carol to be live', (st) => isLive(st) && st.objects > 0);
    check('the host accepted Carol with the read-only right only', (await hostSays('MEMBER Carol')).endsWith('View'));
    check('the guest has the view role', s.role === 'View' && s.read_only, s.role);
    const hostBefore = Number(await hostAsk('count', 'COUNT '));
    await drawRect(C, 700, 250);
    await sleep(2500);
    const after = await state(C);
    await shot(C, 'f1-read-only');
    check('drawing is refused on the read-only guest board', after.objects === s.objects, `${after.objects} vs ${s.objects}`);
    check('nothing reached the host from the read-only guest', Number(await hostAsk('count', 'COUNT ')) === hostBefore);
    await C.close();
  });

  /* ---- g. invalid link */
  await scenario('g. invalid link: a clear message, nothing connects', async () => {
    const D = await newPage(ctxC, 'invalid');
    regularPages.push(D);
    await open(D, 'this-is-not-an-invitation');
    const s = await until(D, 'the join window with its error', (st) => st.window && st.join_error);
    check('an invalid link opens the join window with an error and the link editable', !s.window.from_system && !!s.join_error && s.state === 'none', s.join_error);
    await shot(D, 'g1-invalid-link');
    // pasting the right link into the window works
    await D.evaluate((t) => window.__inkboard_test.join(t, 'Dave'), ticket);
    const d = await until(D, 'Dave to be live', (st) => isLive(st) && st.objects > 0);
    check('a valid link pasted afterwards joins', d.state === 'live');
    await D.close();
  });

  /* ---- m. phone: touch */
  await scenario('m. emulated phone: touch targets, virtual keyboard, join with a finger', async () => {
    phoneBrowser = await drv.launch({ mobile: true });
    const M = await newPage(phoneBrowser.defaultContext(), 'phone', { mobile: true });
    regularPages.push(M);
    await open(M, ticket);
    const box = await until(M, 'the join box on the phone', (s) => s.window?.from_system && s.marks?.join_go);
    await sleep(300);
    await shot(M, 'm1-join-box-phone');
    const [vw] = box.screen;
    const fits = Object.entries(box.marks).every(([, [x, , w]]) => x >= 0 && x + w <= vw + 0.5);
    check('the join box fits the width of the phone', fits, `screen ${box.screen.join('x')}`);
    const small = Object.entries(box.marks).filter(([, [, , , h]]) => h < 32);
    check('the buttons and the field of the join box are large enough for a finger (>= 32 px)', small.length === 0, small.map(([n, m]) => `${n}:${m[3]}`).join(' '));
    check('the page does not scroll sideways nor zoom (viewport meta)', await M.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth && /maximum-scale=1/.test(document.querySelector('meta[name=viewport]').content)));
    await press(M, 'join_name');
    await sleep(500);
    const focus = await M.evaluate(() => { const a = document.activeElement; return `${a?.tagName}:${a?.type ?? ''}`; });
    check('tapping the name field focuses the text input that brings up the virtual keyboard', /^INPUT/.test(focus), focus);
    await M.keyboard.down('Control'); await M.keyboard.press('a'); await M.keyboard.up('Control');
    await M.keyboard.type('Phone');
    await shot(M, 'm2-name-typed-phone');
    await press(M, 'join_go');
    const live = await until(M, 'the phone to be live', (s) => isLive(s) && s.objects > 0);
    await sleep(500);
    await shot(M, 'm3-joined-phone');
    check('the phone joins with a finger and holds the board', live.state === 'live' && live.objects > 0, `${live.objects} objects`);
    const bar = Object.entries((await state(M)).marks).filter(([n]) => n.startsWith('bar_'));
    check('the "Leave the session" button is reachable and large enough on the phone', bar.length > 0 && bar.every(([, m]) => m[3] >= 32 && m[0] + m[2] <= vw + 0.5), JSON.stringify(bar));
    check('the toolbar and the board are drawn (not blank) on the phone', await M.evaluate(() => document.getElementById('inkboard_canvas').width > 0));
    // a cut on the phone: the banner sits under the row of tools, fits the width, and its button is large enough
    await drv.setOffline(M, true);
    await until(M, 'the phone to notice the cut', (st) => st.state === 'reconnecting' && st.marks?.bar_retry, { timeout: 60000 });
    await sleep(600);
    const cutM = await state(M);
    await shot(M, 'm4-reconnecting-phone');
    const barM = Object.entries(cutM.marks).filter(([n]) => n.startsWith('bar_'));
    check('the "Reconnecting…" banner and its buttons fit the phone and are large enough', barM.length >= 2 && barM.every(([, m]) => m[3] >= 32 && m[0] >= 0 && m[0] + m[2] <= vw + 0.5), JSON.stringify(barM));
    await drv.setOffline(M, false);
    await until(M, 'the phone to be live again', (st) => st.state === 'live', { timeout: 90000 });
    await M.close();
  });

  /* ---- e1. relay unreachable (blocked): enterprise message ---- */
  await scenario('e1. relay unreachable: enterprise message, 3 tips, copy the diagnostic', async () => {
    blockedBrowser = await drv.launch({ blockRelays: true });
    const ctx = blockedBrowser.defaultContext();
    const canRead = (await drv.grant(ctx, origin)) && drv.caps.clipboardRead;
    const E = await newPage(ctx, 'blocked');
    regularPages.push(E);
    await E.bringToFront();
    await open(E, ticket);
    await until(E, 'the join box', (s) => s.window?.from_system);
    await typeName(E, 'Eve');
    await press(E, 'join_go');
    const failedState = await until(E, 'the failure', (s) => s.state === 'failed', { timeout: 120000 });
    await sleep(600);
    await shot(E, 'e1a-relay-blocked-desktop');
    const issue = failedState.issue;
    check('the failure is the "cannot reach the connection service" one (firewall/gateway), not a host problem', issue.class === 'Unreachable' && /connection service/.test(issue.headline) && /firewall|gateway/i.test(issue.headline), `${issue.class}: ${issue.headline}`);
    check('three actionable tips are shown', issue.tips.length === 3, `${issue.tips.length}`);
    check('a tip names the exact domain to allow', issue.tips.some((t) => /relay\.n0\.iroh\.link/.test(t)), issue.tips[2]);
    await E.bringToFront();
    await press(E, 'issue_copy');
    await sleep(800);
    const copied = await E.evaluate(() => window.__clip);
    const handed = copied.text.at(-1) ?? '';
    check('"Copy the diagnostic" hands the diagnostic to the browser clipboard and the browser accepts it', /Connection service: .*iroh\.link/.test(handed) && /Inkboard/.test(handed) && copied.errors.length === 0, `${handed.split('\n').slice(0, 3).join(' | ')} ${copied.errors.join(' ')}`);
    if (canRead) {
      const clip = await E.evaluate(async () => { try { return await navigator.clipboard.readText(); } catch (e) { return `error: ${e}`; } });
      check('"Copy the diagnostic" (read back from the browser clipboard)', /Connection service: .*iroh\.link/.test(clip), clip.split('\n').slice(0, 2).join(' | '));
    } else skip('"Copy the diagnostic" read back from the clipboard', 'this engine does not let automation read the clipboard');
    check('DNS block: the copied diagnostic states the facts and does not claim the relay is reachable', /Reachable: no/.test(handed) && /Cause retained: relay unreachable/.test(handed) && /HTTPS probe[^
]*failed/i.test(handed) && /Browser: /.test(handed), handed.split('
').slice(0, 14).join(' | '));
    await press(E, 'issue_retry');
    await until(E, 'the new attempt to start', (s) => s.state === 'joining', { timeout: 30000 });
    await until(E, 'the failure again', (s) => s.state === 'failed', { timeout: 120000 });
    check('"Try again" tries again (connecting) and reports the failure again', true);
    await press(E, 'issue_close');
    check('"Close" leaves the failure window', (await until(E, 'the window closed', (s) => s.state === 'none')).state === 'none');
    await E.close();

    const F = await newPage(ctx, 'blocked-fr', { french: true });
    regularPages.push(F);
    await open(F, ticket);
    await until(F, 'the join box', (s) => s.window?.from_system);
    await press(F, 'join_go');
    const fr = await until(F, 'the failure', (s) => s.state === 'failed', { timeout: 120000 });
    await sleep(600);
    await shot(F, 'e1b-relay-blocked-desktop-fr');
    check('the failure message is in French in a French browser', /pare-feu|passerelle/.test(fr.issue.headline) && fr.issue.tips.length === 3, fr.issue.headline);
    await F.close();

    blockedPhoneBrowser = await drv.launch({ mobile: true, blockRelays: true });
    const G = await newPage(blockedPhoneBrowser.defaultContext(), 'blocked-phone', { mobile: true });
    regularPages.push(G);
    await open(G, ticket);
    await until(G, 'the join box', (s) => s.window?.from_system);
    await press(G, 'join_go');
    const ph = await until(G, 'the failure', (s) => s.state === 'failed' && s.marks?.issue_copy, { timeout: 120000 });
    await sleep(600);
    await shot(G, 'e1c-relay-blocked-phone');
    const ms = Object.entries((await state(G)).marks);
    check('the enterprise message and its buttons fit the phone', ms.length > 0 && ms.every(([, [x, , w]]) => x >= 0 && x + w <= ph.screen[0] + 0.5), `screen ${ph.screen.join('x')}`);
    await G.close();
  });

  /* ---- e3. relay blocked SILENTLY (packets dropped): the browser never gets an answer */
  await scenario('e3. relay silently unreachable (time-outs, no immediate error): enterprise message, facts in the diagnostic', async () => {
    const hole = await startBlackhole();
    try {
      const r = await failedJoin('blocked-silent', [mapRelays(`127.0.0.1:${hole.port}`)], ticket, { shotName: 'e3a-relay-silent-desktop' });
      check('the browser really tried the relay (a connection reached the black hole)', hole.accepted() > 0, `${hole.accepted()} connection(s)`);
      check('silent block: "cannot reach the connection service" (firewall/gateway), NOT "the service answers"', r.issue.class === 'Unreachable' && /Cannot reach the connection service/.test(r.issue.headline), `${r.issue.class}: ${r.issue.headline}`);
      check('silent block: three actionable tips naming the domain', r.issue.tips.length === 3 && r.issue.tips.some((t) => /relay\.n0\.iroh\.link/.test(t)), `${r.issue.tips.length}`);
      check('silent block: the diagnostic does not claim the relay is reachable', !/Reachable: yes/.test(r.diagnostic) && /Reachable: no/.test(r.diagnostic), r.diagnostic.split('\n').find((l) => l.startsWith('Reachable')));
      check('silent block: the diagnostic states the facts (probe, relay connection, URLs, browser, time, cause)',
        /HTTPS probe[^\n]*(timed out|failed)/i.test(r.diagnostic) && /Relay connection \(iroh\): not connected/.test(r.diagnostic) && /Relay URL used by the browser: https:\/\/[^\s]+/.test(r.diagnostic)
        && /Browser: .*Chrome/.test(r.diagnostic) && /Gave up after: \d+(\.\d+)? s/.test(r.diagnostic) && /Cause retained: relay unreachable/.test(r.diagnostic), '');
      check('the diagnostic holds no ticket', !r.diagnostic.includes(ticket.slice(0, 16)));
      await r.page.close();
    } finally { hole.close(); }
    // the same with a non-routable address instead of a local black hole (packets go to the network and are dropped, or refused at once)
    const r2 = await failedJoin('blocked-silent-ip', [mapRelays('10.255.255.1')], ticket, { shotName: 'e3b-relay-nonroutable-desktop' });
    check('non-routable relay address: same message as a silent block', r2.issue.class === 'Unreachable' && r2.issue.tips.length === 3 && !/Reachable: yes/.test(r2.diagnostic), `${r2.issue.class}`);
    await r2.page.close();
  });

  /* ---- e4. HTTPS passes but the WebSocket is blocked (a filtering proxy) */
  for (const upgrade of ['refuse', 'hang']) {
    await scenario(`e4-${upgrade}. HTTPS passes but the WebSocket is ${upgrade === 'refuse' ? 'refused (403)' : 'never answered'}: dedicated message`, async () => {
      const fake = await startWebSocketBlocker(upgrade);
      try {
        const args = [mapRelays(`127.0.0.1:${fake.port}`), '--ignore-certificate-errors'];
        const r = await failedJoin(`blocked-ws-${upgrade}`, args, ticket, { shotName: `e4-${upgrade}-websocket-blocked-desktop` });
        check('the browser really tried the WebSocket upgrade', fake.seen.upgrades > 0, `${fake.seen.upgrades} upgrade(s), ${fake.seen.http.length} plain request(s)`);
        check('HTTPS passes but no WebSocket: the dedicated "blocks the connection itself" class', r.issue.class === 'WebSocketBlocked' && /WebSocket/.test(r.issue.headline) && !/Cannot reach the connection service/.test(r.issue.headline), `${r.issue.class}: ${r.issue.headline}`);
        check('three tips, the first asks IT to allow WebSocket connections to *.relay.n0.iroh.link (port 443)', r.issue.tips.length === 3 && /WebSocket/.test(r.issue.tips[0]) && /\*\.relay\.n0\.iroh\.link/.test(r.issue.tips[0]) && /443/.test(r.issue.tips[0]), r.issue.tips[0]);
        check('the message does not claim to know the exact cause', /cause/i.test(r.issue.headline) || /seem|appear|may|might/i.test(r.issue.headline), r.issue.headline);
        check('the diagnostic: probe answered, relay not connected, cause WebSocket blocked', /HTTPS probe[^\n]*answered/i.test(r.diagnostic) && /Relay connection \(iroh\): not connected/.test(r.diagnostic) && /Cause retained: WebSocket blocked/.test(r.diagnostic), '');
        check('the diagnostic does not say the relay is reachable', !/Reachable: yes/.test(r.diagnostic), '');
        await r.page.close();
        if (upgrade === 'refuse') {
          const fr = await failedJoin('blocked-ws-fr', args, ticket, { french: true, shotName: 'e4-refuse-websocket-blocked-desktop-fr' });
          check('in French: WebSocket message with the service-informatique tip', fr.issue.class === 'WebSocketBlocked' && fr.issue.tips.length === 3 && /WebSocket/.test(fr.issue.tips[0]) && /service informatique/.test(fr.issue.tips[0]), fr.issue.tips[0]);
          await fr.page.close();
          const ph = await failedJoin('blocked-ws-phone', args, ticket, { mobile: true, shotName: 'e4-refuse-websocket-blocked-phone' });
          const ms = Object.entries((await state(ph.page)).marks);
          check('the WebSocket message and its buttons fit the phone', ms.length > 0 && ms.every(([, [x, , w]]) => x >= 0 && x + w <= ph.state.screen[0] + 0.5), `screen ${ph.state.screen.join('x')}`);
          await ph.page.close();
        }
      } finally { fake.close(); }
    });
  }

  /* ---- z. the host leaves */
  await scenario('z. the host ends the session: banner with "Save a copy"', async () => {
    await A.bringToFront();
    const finalCount = Number(await hostAsk('count', 'COUNT '));
    host.stdin.write('quit\n');
    const ended = await until(A, 'the session to end', (s) => s.state === 'ended', { timeout: 120000 });
    await sleep(300);
    await shot(A, 'z1-host-left-desktop');
    check('the guest is told that the host ended the session', /HostLeft/.test(JSON.stringify(ended.issue)), JSON.stringify(ended.issue));
    const since = Date.now() - 20;
    await press(A, 'bar_save');
    const file = await waitDownload(since, '.inkboard');
    check('"Save a copy of the board" downloads the board after the host left', !!file);
    if (file) check('the copy holds the whole shared board', objectsOf(file).length === finalCount, `${objectsOf(file).length} vs ${finalCount}`);
  });

  /* ---- y. the relay answers normally but the host is gone (its session is over): the relay connection is established */
  await scenario('y. real relay, host gone: "the service answers, the host cannot be reached" with the relay connected', async () => {
    try { host.stdin.write('quit\n'); } catch { /* already gone (scenario z) */ }
    await sleep(3000);
    const r = await failedJoin('host-gone', [], ticket, { shotName: 'y1-host-unreachable-desktop' });
    check('relay connected, host absent: the "service answers but the host cannot be reached" message, no tips', r.issue.class === 'HostUnreachable' && /service answers/.test(r.issue.headline) && r.issue.tips.length === 0, `${r.issue.class}: ${r.issue.headline}`);
    check('the diagnostic: relay connection established, cause host unreachable', /Relay connection \(iroh\): connected/.test(r.diagnostic) && /Cause retained: host unreachable/.test(r.diagnostic), '');
    await r.page.close();
  });

  const bad = logs.filter((l) => !(l.label.startsWith('blocked') || ['old-browser', 'no-webgl'].includes(l.label)) && /^\[(pageerror|error)\]/.test(l.text) && !/Failed to load resource|ERR_NAME_NOT_RESOLVED|WebSocket connection|Cross-Origin Request Blocked|can.t establish a connection|Load failed|Viewport argument key/.test(l.text));
  check('no unexpected error in the browser consoles', bad.length === 0, bad.slice(0, 3).map((l) => `${l.label}: ${l.text}`).join(' | '));
  console.log('--- console (first lines)');
  for (const l of logs.slice(0, 20)) console.log(`${l.label} ${l.text}`);
} catch (e) {
  console.error(`ERROR ${e.stack ?? e}`);
  failed++;
} finally {
  await browser?.close().catch(() => {});
  await blockedBrowser?.close().catch(() => {});
  await phoneBrowser?.close().catch(() => {});
  await blockedPhoneBrowser?.close().catch(() => {});
  for (const b of extraBrowsers) await b.close().catch(() => {});
  try { host.stdin.write('quit\n'); } catch { /* gone */ }
  await sleep(1500);
  host.kill();
  server.close();
  rmSync(downloads, { recursive: true, force: true });
}
console.log(failed ? `${failed} check(s) failed` : 'all checks passed');
process.exit(failed ? 1 : 0);
