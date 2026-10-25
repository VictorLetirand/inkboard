// Records the frames of the README demo: two browser guests and the MCP agent editing the board of a native host, through the public relay.
//
//   node demo.mjs [--skip-build] [--dist ../dist-e2e] [--out ./demo-frames] [--browser chrome] [--probe]
//
// The same real parts as the end-to-end bench (e2e.mjs): the host is the ignored test `web_bench_host` of inkboard-app, the guests are real
// browser pages joined by the link, the agent is the host's `agent` command. Alice's page is the one filmed (a JPEG every ~100 ms): it shows Bob's
// and the agent's cursors as any guest sees them. Output: `<out>/frames/NNNNN.jpg` and `<out>/frames.json` (the caption of each frame).
// Encoding (captions, GIF, MP4) is left to the caller (ffmpeg). `--probe` only saves a few screenshots to calibrate the coordinates.
// Needs Internet (the public relay), puppeteer-core (PUPPETEER_DIR), trunk (unless --skip-build) and a clang for wasm32 (see README.md).
import http from 'node:http';
import { createRequire } from 'node:module';
import { spawn, spawnSync } from 'node:child_process';
import { readFileSync, existsSync, mkdirSync, rmSync, writeFileSync, mkdtempSync } from 'node:fs';
import { join, extname, resolve, dirname } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { createDriver } from './drivers.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const argv = process.argv.slice(2);
const opt = (name, dflt) => { const i = argv.indexOf(`--${name}`); return i < 0 ? dflt : argv[i + 1]; };
const dist = resolve(opt('dist', join(here, '../dist-e2e')));
const out = resolve(opt('out', join(here, 'demo-frames')));
const probe = argv.includes('--probe');
const toolchain = process.env.RUST_TOOLCHAIN ?? '1.99.1';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const repo = resolve(here, '../../../..');
const drv = await createDriver(opt('browser', 'chrome'), { headed: false, downloads: mkdtempSync(join(tmpdir(), 'inkboard-demo-')), out });
void createRequire;

rmSync(out, { recursive: true, force: true });
mkdirSync(join(out, 'frames'), { recursive: true });

if (!argv.includes('--skip-build')) {
  const r = spawnSync('trunk', ['build', '--release', '--features', 'web-glow,web-test', '--dist', dist], { cwd: join(here, '..'), stdio: 'inherit', env: { ...process.env, RUSTUP_TOOLCHAIN: toolchain } });
  if (r.status !== 0) throw new Error('trunk build failed');
}

/* ---- native host (same as e2e.mjs) */
const build = spawnSync('cargo', [`+${toolchain}`, 'test', '-p', 'inkboard-app', '--bin', 'inkboard', '--no-run', '--message-format=json'], { cwd: repo, encoding: 'utf8', maxBuffer: 1 << 28 });
const exe = build.stdout.split('\n').filter((l) => l.startsWith('{')).map((l) => JSON.parse(l))
  .find((m) => m.reason === 'compiler-artifact' && m.profile?.test && m.executable && m.target.name === 'inkboard')?.executable;
if (!exe) throw new Error('cannot find the test executable of inkboard-app');
const host = spawn(exe, ['web_bench_host', '--ignored', '--nocapture'], { stdio: ['pipe', 'pipe', 'inherit'] });
const hostLines = [];
let buf = '';
host.stdout.on('data', (d) => {
  buf += d;
  const lines = buf.split('\n');
  buf = lines.pop();
  for (const l of lines.map((x) => x.trim()).filter(Boolean)) hostLines.push(l);
});
const hostSays = async (prefix, timeout = 120000) => {
  const t0 = Date.now();
  for (;;) {
    const line = hostLines.find((l) => l.startsWith(prefix));
    if (line) return line.slice(prefix.length).trim();
    if (Date.now() - t0 > timeout) throw new Error(`the host did not print ${prefix}`);
    await sleep(100);
  }
};

/* ---- static server for the page */
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm', '.css': 'text/css', '.svg': 'image/svg+xml' };
const server = http.createServer((req, res) => {
  const path = decodeURIComponent(new URL(req.url, 'http://x').pathname).replace(/^\/$/, '/index.html');
  const file = join(dist, path);
  if (!file.startsWith(dist) || !existsSync(file)) { res.writeHead(404); return res.end(); }
  const body = readFileSync(file);
  res.writeHead(200, { 'content-type': mime[extname(file)] ?? 'application/octet-stream', 'content-length': body.length });
  res.end(body);
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const url = `http://127.0.0.1:${server.address().port}/index.html`;

const W = 1100;
const H = 700;
const state = async (p) => JSON.parse(await p.evaluate(() => window.__inkboard_test.state()));
async function guest(context, name, ticket) {
  const p = await drv.newPage(context);
  await p.setViewport({ width: W, height: H });
  await p.goto(url, { waitUntil: 'load' });
  await p.waitForFunction(() => !document.getElementById('loading') && window.__inkboard_test, { timeout: 120000 });
  await p.evaluate((t, n) => window.__inkboard_test.join(t, n), ticket, name);
  for (let i = 0; i < 400; i++) {
    const s = await state(p);
    if (s.state === 'live' && s.mirror && s.mirror_ready && s.objects > 0) return p;
    await sleep(250);
  }
  throw new Error(`${name} never went live`);
}
/** A pointer move in small steps, so that the other guests see a moving cursor. */
async function glide(p, pts, pause = 28) {
  for (const [x, y] of pts) { await p.mouse.move(x, y); await sleep(pause); }
}
const lerp = (a, b, n) => Array.from({ length: n }, (_, i) => [a[0] + ((b[0] - a[0]) * (i + 1)) / n, a[1] + ((b[1] - a[1]) * (i + 1)) / n]);
async function drag(p, a, b, n = 14) {
  await p.mouse.move(...a);
  await sleep(80);
  await p.mouse.down();
  await sleep(80);
  await glide(p, lerp(a, b, n));
  await p.mouse.up();
  await sleep(250);
}

let browser;
try {
  const link = await hostSays('LINK ');
  const ticket = link.split('#')[1];
  browser = await drv.launch();
  const A = await guest(browser.defaultContext(), 'Alice', ticket);
  const B = await guest(await browser.newContext(), 'Bob', ticket);
  await sleep(1500);
  if (probe) {
    await A.screenshot({ path: join(out, 'probe-alice.png') });
    await B.screenshot({ path: join(out, 'probe-bob.png') });
    console.log('probe saved in', out);
  } else {
    /* ---- filming */
    const frames = [];
    let caption = '';
    let recording = true;
    const filming = (async () => {
      let n = 0;
      while (recording) {
        const t0 = Date.now();
        const jpg = await A.screenshot({ type: 'jpeg', quality: 82 });
        writeFileSync(join(out, 'frames', `${String(n).padStart(5, '0')}.jpg`), jpg);
        frames.push({ n, t: t0, caption });
        n++;
        await sleep(Math.max(0, 110 - (Date.now() - t0)));
      }
    })();
    const say = (text) => { caption = text; };

    say('Alice and Bob joined from a web page: nothing to install');
    await glide(B, lerp([420, 620], [600, 600], 20));
    await sleep(900);

    say('Everybody sees everybody: names, cursors, edits in real time');
    await A.keyboard.press('r');
    const drawA = drag(A, [400, 470], [560, 560]);
    await B.keyboard.press('o');
    const drawB = (async () => { await sleep(200); await drag(B, [740, 470], [920, 560]); })();
    await Promise.all([drawA, drawB]);
    await A.keyboard.press('Escape');
    await A.keyboard.press('a');
    await drag(A, [565, 515], [735, 515], 18);
    await A.keyboard.press('Escape');
    await B.keyboard.press('p');
    await B.mouse.move(710, 605);
    await B.mouse.down();
    await glide(B, Array.from({ length: 36 }, (_, i) => [620 + 90 * Math.cos(i / 5.7), 605 + 22 * Math.sin(i / 5.7)]), 30);
    await B.mouse.up();
    await sleep(700);

    say('The AI agent edits the same board: its cursor is shown to everybody');
    await A.keyboard.press('Escape');
    await sleep(500);
    host.stdin.write('agent\n');
    await hostSays('AGENT');
    await glide(B, lerp([780, 610], [520, 620], 40), 60);
    await sleep(1800);

    say('End-to-end encrypted. No account. No server to run.');
    await glide(B, lerp([520, 620], [860, 600], 40), 60);
    await sleep(1200);

    recording = false;
    await filming;
    writeFileSync(join(out, 'frames.json'), JSON.stringify(frames.map(({ n, t, caption: c }) => ({ n, ms: t - frames[0].t, caption: c })), null, 1));
    console.log(`recorded ${frames.length} frames in ${out}`);
  }
} finally {
  try { host.stdin.write('quit\n'); } catch { /* gone */ }
  try { await browser?.close(); } catch { /* closed */ }
  server.close();
  setTimeout(() => process.exit(0), 3000);
}
