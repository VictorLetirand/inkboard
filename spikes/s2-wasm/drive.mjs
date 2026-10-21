// Drives a built variant in an installed browser (Chrome / Edge) with puppeteer-core: serves dist-*/, loads the page, measures the
// time to "running", takes screenshots, optionally draws shapes by synthetic input and waits for the in-app icon bench report.
//
//   node drive.mjs --dist dist-glow --query "?demo=shapes" --shot results/glow.png
//   node drive.mjs --dist dist-wgpu --query "?renderer=webgl" --browser edge --interact
//   node drive.mjs --dist dist-glow-embedded --query "?iconboard=2000" --bench
//
// Flags: --browser chrome|edge (default chrome), --headed, --interact, --bench, --timeout SECONDS (default 60), --shot FILE,
//        --gpu-flags "<extra chromium flags>" (default: WebGPU enabled)
import http from 'node:http';
import { readFileSync, existsSync, mkdirSync } from 'node:fs';
import { join, extname, dirname, resolve } from 'node:path';
import puppeteer from 'puppeteer-core';

const argv = process.argv.slice(2);
const opt = (name, dflt) => { const i = argv.indexOf(`--${name}`); return i < 0 ? dflt : (argv[i + 1]?.startsWith('--') || argv[i + 1] === undefined ? true : argv[i + 1]); };
const dist = resolve(opt('dist', 'dist-glow'));
const query = opt('query', '');
const browserName = opt('browser', 'chrome');
const timeout = Number(opt('timeout', 60)) * 1000;
const shot = opt('shot', null);
const interact = argv.includes('--interact');
const bench = argv.includes('--bench');
const headed = argv.includes('--headed');
const extra = (opt('gpu-flags', '--enable-unsafe-webgpu --enable-features=Vulkan') || '').toString().split(' ').filter(Boolean);

const exe = browserName === 'edge' ? 'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe' : 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe';
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm', '.bin': 'application/octet-stream', '.css': 'text/css' };

const server = http.createServer((req, res) => {
  const p = join(dist, decodeURIComponent(new URL(req.url, 'http://x').pathname).replace(/^\/$/, '/index.html'));
  if (!p.startsWith(dist) || !existsSync(p)) { res.writeHead(404); return res.end(); }
  res.writeHead(200, { 'content-type': mime[extname(p)] ?? 'application/octet-stream' });
  res.end(readFileSync(p));
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const url = `http://127.0.0.1:${server.address().port}/index.html${query}`;

const browser = await puppeteer.launch({ executablePath: exe, headless: !headed, args: ['--window-size=1360,900', ...extra] });
const page = await browser.newPage();
await page.setViewport({ width: 1360, height: 860 });
const logs = [];
page.on('console', (m) => logs.push(`[${m.type()}] ${m.text()}`));
page.on('pageerror', (e) => logs.push(`[pageerror] ${e.message}`));
await page.evaluateOnNewDocument(() => {
  new MutationObserver(() => {
    const s = document.getElementById('status');
    if (s && s.textContent === 'running' && !window.__running_at) window.__running_at = performance.now();
  }).observe(document, { subtree: true, childList: true, characterData: true });
});
const shotTo = async (name) => { if (!shot) return; mkdirSync(dirname(resolve(shot)), { recursive: true }); await page.screenshot({ path: shot.replace(/\.png$/, `${name}.png`) }); };

console.log(`browser: ${await browser.version()}   url: ${url}`);
const t0 = Date.now();
await page.goto(url, { waitUntil: 'load' });
try {
  await page.waitForFunction('window.__running_at', { timeout });
} catch { console.log('TIMEOUT waiting for "running"'); }
const runningAt = await page.evaluate(() => window.__running_at ?? null);
console.log(`startup: status "running" ${runningAt === null ? 'never reached' : `${runningAt.toFixed(0)} ms after navigation start`} (wall ${Date.now() - t0} ms incl. goto)`);
console.log('webgpu in navigator:', await page.evaluate(() => 'gpu' in navigator));
await new Promise((r) => setTimeout(r, 1500));
await shotTo('-1-initial');

if (interact) {
  const click = async (x, y) => { await page.mouse.move(x, y); await page.mouse.down(); await page.mouse.up(); };
  await click(1100, 700); // empty spot: gives the canvas the keyboard focus
  await page.keyboard.press('r');
  await page.mouse.move(700, 150); await page.mouse.down();
  for (let i = 1; i <= 10; i++) await page.mouse.move(700 + i * 20, 150 + i * 10);
  await page.mouse.up();
  await new Promise((r) => setTimeout(r, 600));
  await shotTo('-2-after-rect-drag');
  await page.keyboard.press('n');
  await click(1000, 400); // sticky note: a plain click creates one
  await new Promise((r) => setTimeout(r, 600));
  await page.keyboard.type('hello');
  await new Promise((r) => setTimeout(r, 600));
  await shotTo('-3-after-sticky-click');
}

if (argv.includes('--touch')) {
  // touch tap (sticky note tool) then a pen stroke (pen tool; CDP pointerType "pen" with a force) at the bottom right
  await page.mouse.click(1250, 760); // focus
  await page.keyboard.press('n');
  await page.touchscreen.tap(1100, 300);
  await new Promise((r) => setTimeout(r, 800));
  await page.keyboard.press('Escape');
  await page.mouse.click(1250, 760); // leaves the text editing, keyboard focus on the canvas
  await page.keyboard.press('p');
  await new Promise((r) => setTimeout(r, 300));
  const cdp = await page.createCDPSession();
  const ev = (type, x, y, buttons) => cdp.send('Input.dispatchMouseEvent', { type, x, y, button: 'left', buttons, clickCount: 1, pointerType: 'pen', force: 0.8 });
  await ev('mouseMoved', 1000, 650, 0);
  await ev('mousePressed', 1000, 650, 1);
  for (let i = 1; i <= 30; i++) await ev('mouseMoved', 1000 + i * 8, 650 + Math.sin(i / 3) * 40, 1);
  await ev('mouseReleased', 1240, 650, 0);
  await new Promise((r) => setTimeout(r, 800));
  await shotTo('-6-after-touch-and-pen');
}

if (argv.includes('--drop')) {
  // synthetic file drop: a File with bytes and a name, no path (what a browser gives)
  await page.evaluate(async () => {
    const c = document.createElement('canvas'); c.width = 120; c.height = 80;
    const g = c.getContext('2d'); g.fillStyle = '#e11d48'; g.fillRect(0, 0, 120, 80); g.fillStyle = '#fff'; g.fillRect(20, 20, 80, 40);
    const blob = await new Promise((r) => c.toBlob(r, 'image/png'));
    const dt = new DataTransfer(); dt.items.add(new File([blob], 'dropped.png', { type: 'image/png' }));
    const target = document.getElementById('inkboard_canvas');
    for (const t of ['dragover', 'drop']) target.dispatchEvent(new DragEvent(t, { dataTransfer: dt, bubbles: true, cancelable: true, clientX: 600, clientY: 400 }));
  });
  await new Promise((r) => setTimeout(r, 1000));
  await shotTo('-5-after-drop');
}

if (bench) {
  const t1 = Date.now();
  while (!logs.some((l) => l.includes('textures held at the end')) && Date.now() - t1 < timeout) await new Promise((r) => setTimeout(r, 500));
  await shotTo('-4-after-bench');
}
console.log('--- console');
for (const l of logs) console.log(l);
await browser.close();
server.close();
