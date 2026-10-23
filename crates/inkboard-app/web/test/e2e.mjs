// End-to-end bench of the browser guest: a REAL native host (headless, public n0 relay) and the built web page in an installed Chrome.
//
//   1. builds the page (trunk, feature `web-test` adds `window.__inkboard_test`) unless --skip-build;
//   2. starts the native host (the ignored test `web_bench_host` of inkboard-app: 5 objects, prints its invitation link);
//   3. opens `index.html#<ticket>` in Chrome, joins through the test hook, and checks that the connection opens, the snapshot arrives
//      (same number of objects as the host), and that an object added by the host reaches the browser;
//   4. a garbage link is refused with a message, and the ticket never appears in a request to the static server.
//
//   node e2e.mjs [--skip-build] [--dist ../dist-e2e] [--out ./results-e2e] [--headed]
//
// Needs Internet (the public relay), Chrome (CHROME=<path to chrome.exe>), trunk, and for `ring` a clang with the wasm32 target
// (CC_wasm32_unknown_unknown / AR_wasm32_unknown_unknown). RUST_TOOLCHAIN (default 1.99.1) selects the Rust toolchain.
// Exit code 1 when a check fails.
import http from 'node:http';
import { createRequire } from 'node:module';
import { spawn, spawnSync } from 'node:child_process';
import { readFileSync, existsSync, mkdirSync, rmSync } from 'node:fs';
import { join, extname, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

// PUPPETEER_DIR: folder holding puppeteer-core, to keep node_modules out of the repository tree (see README.md)
const require = createRequire(import.meta.url);
const puppeteer = require(process.env.PUPPETEER_DIR ?? 'puppeteer-core');

const here = dirname(fileURLToPath(import.meta.url));
const argv = process.argv.slice(2);
const opt = (name, dflt) => { const i = argv.indexOf(`--${name}`); return i < 0 ? dflt : argv[i + 1]; };
const dist = resolve(opt('dist', join(here, '../dist-e2e')));
const out = resolve(opt('out', join(here, 'results-e2e')));
const headed = argv.includes('--headed');
const toolchain = process.env.RUST_TOOLCHAIN ?? '1.99.1';
const chrome = process.env.CHROME ?? 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const repo = resolve(here, '../../../..');

rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });

let failed = 0;
const check = (name, ok, detail = '') => { console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? `  (${detail})` : ''}`); if (!ok) failed++; };
const run = (cmd, args, options) => {
  const r = spawnSync(cmd, args, { stdio: 'inherit', ...options });
  if (r.status !== 0) throw new Error(`${cmd} ${args.join(' ')} failed (${r.status})`);
};

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
const waiters = [];
let hostBuffer = '';
host.stdout.on('data', (d) => {
  hostBuffer += d;
  const lines = hostBuffer.split('\n');
  hostBuffer = lines.pop();
  for (const l of lines.map((x) => x.trim()).filter(Boolean)) { hostLines.push(l); console.log(`[host] ${l.length > 100 ? `${l.slice(0, 100)}…` : l}`); }
  for (const w of [...waiters]) w();
});
host.stderr.on('data', (d) => process.stderr.write(`[host stderr] ${d}`));
const hostSays = async (prefix, timeout = 120000) => {
  const t0 = Date.now();
  for (;;) {
    const line = hostLines.find((l) => l.startsWith(prefix));
    if (line) return line.slice(prefix.length).trim();
    if (Date.now() - t0 > timeout) throw new Error(`the host did not print ${prefix}`);
    await new Promise((r) => { waiters.push(r); setTimeout(r, 250); });
    waiters.length = 0;
  }
};

/* ---------------------------------------------------------------- static server */
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm', '.css': 'text/css', '.svg': 'image/svg+xml' };
const requests = [];
const server = http.createServer((req, res) => {
  const path = decodeURIComponent(new URL(req.url, 'http://x').pathname).replace(/^\/$/, '/index.html');
  requests.push(req.url);
  const file = join(dist, path);
  if (!file.startsWith(dist) || !existsSync(file)) { res.writeHead(404); return res.end(); }
  res.writeHead(200, { 'content-type': mime[extname(file)] ?? 'application/octet-stream' });
  res.end(readFileSync(file));
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const url = `http://127.0.0.1:${server.address().port}/index.html`;

let browser;
try {
  const link = await hostSays('LINK ');
  const hostObjects = Number(await hostSays('OBJECTS '));
  const ticket = link.split('#')[1];
  check('the host shares a board with objects', hostObjects > 0 && !!ticket, `${hostObjects} objects`);

  /* ---------------------------------------------------------------- browser */
  browser = await puppeteer.launch({ executablePath: chrome, headless: !headed, args: ['--window-size=1360,900', '--ignore-gpu-blocklist', '--enable-unsafe-swiftshader'] });
  const logs = [];
  const page = await browser.newPage();
  await page.setViewport({ width: 1360, height: 860 });
  page.on('console', (m) => logs.push(`[${m.type()}] ${m.text()}`));
  page.on('pageerror', (e) => logs.push(`[pageerror] ${e.message}`));
  const state = async () => JSON.parse(await page.evaluate(() => window.__inkboard_test.state()));
  const until = async (what, cond, timeout = 90000) => {
    const t0 = Date.now();
    let s = await state();
    while (!cond(s)) {
      if (Date.now() - t0 > timeout) throw new Error(`gave up waiting for: ${what} (last state: ${JSON.stringify({ ...s, ids: undefined })})`);
      await sleep(250);
      s = await state();
    }
    return s;
  };

  await page.goto(`${url}#${ticket}`, { waitUntil: 'load' });
  await page.waitForFunction(() => !document.getElementById('loading') && window.__inkboard_test, { timeout: 120000 });
  check('the page starts with the test hook', true);

  // a garbage link is refused with a message, nothing connects
  await page.evaluate(() => window.__inkboard_test.join('not an invitation', 'Nobody'));
  const refused = await until('the garbage link to be refused', (s) => s.join_error);
  check('a garbage link is refused with a message', !!refused.join_error && refused.state === 'none', refused.join_error);

  // the real link, taken from the fragment of the URL like the page of the join flow will
  await page.evaluate(() => window.__inkboard_test.join(location.hash.slice(1), 'Browser guest'));
  const joined = await until('the guest to hold the host board', (s) => s.state === 'live' && s.mirror && s.objects > 0 || s.state === 'failed' || s.state === 'ended');
  await page.screenshot({ path: join(out, '01-joined.png') });
  check('the connection is established (live)', joined.state === 'live', JSON.stringify({ ...joined, ids: undefined }));
  check('the host saw the guest by name', (await hostSays('MEMBER ')) === 'Browser guest');
  check('the snapshot arrived: same number of objects as the host', joined.objects === hostObjects, `${joined.objects} vs ${hostObjects}`);
  check('the host name came with the welcome', !!joined.host_name, joined.host_name);

  // a change made by the host reaches the browser
  host.stdin.write('add\n');
  const hostAfter = Number((await hostSays('ADDED ')));
  const synced = await until('the new object to reach the browser', (s) => s.objects === hostAfter);
  await page.screenshot({ path: join(out, '02-after-host-change.png') });
  check('a modification of the host reaches the browser', synced.objects === hostAfter && hostAfter === hostObjects + 1, `${synced.objects} objects`);

  // leaving closes the session cleanly
  await page.evaluate(() => window.__inkboard_test.leave());
  const left = await until('the guest to leave', (s) => s.state === 'none');
  check('leaving the session works', left.state === 'none');

  check('the ticket was never sent to the static server (URL fragment)', !requests.some((r) => r.includes(ticket.slice(0, 16))));
  const bad = logs.filter((l) => l.startsWith('[pageerror]') || l.startsWith('[error]'));
  check('no error in the browser console', bad.length === 0, bad.slice(0, 3).join(' | '));
  console.log('--- console');
  for (const l of logs.slice(0, 30)) console.log(l);
} catch (e) {
  console.error(`ERROR ${e.stack ?? e}`);
  failed++;
} finally {
  await browser?.close().catch(() => {});
  host.stdin.write('quit\n');
  await sleep(1500);
  host.kill();
  server.close();
}
console.log(failed ? `${failed} check(s) failed` : 'all checks passed');
process.exit(failed ? 1 : 0);
