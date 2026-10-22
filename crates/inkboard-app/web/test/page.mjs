// Page test of the web build: serves ../dist, opens it in an installed Chrome (puppeteer-core) and goes through the main paths:
// start, draw a shape, type text, open the icon library (the 8 MB pack must be fetched only then), insert an icon, download the board,
// open it again through the file picker, paste and drop an image, ids of two separate sessions. Screenshots go to ./results.
//
//   npm install            (once, in this folder)
//   node page.mjs [--dist ../dist] [--out ./results] [--headed] [--pack-delay MS] [--wasm-delay MS]
//
// CHROME=<path to chrome.exe> overrides the browser location. Exit code 1 when a check fails.
import http from 'node:http';
import { createRequire } from 'node:module';
import { readFileSync, existsSync, mkdirSync, readdirSync, rmSync, statSync, copyFileSync } from 'node:fs';
import { join, extname, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url); // honours NODE_PATH as well as ./node_modules
const puppeteer = require('puppeteer-core');

const here = dirname(fileURLToPath(import.meta.url));
const argv = process.argv.slice(2);
const opt = (name, dflt) => { const i = argv.indexOf(`--${name}`); return i < 0 ? dflt : argv[i + 1]; };
const dist = resolve(opt('dist', join(here, '../dist')));
const out = resolve(opt('out', join(here, 'results')));
const packDelay = Number(opt('pack-delay', 1500)); // slows icons.bin down so that the loading indicator can be seen
const wasmDelay = Number(opt('wasm-delay', 1500)); // slows the first .wasm request down, to see the loading indicator
const headed = argv.includes('--headed');
const chrome = process.env.CHROME ?? 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
const downloads = join(out, 'downloads');
mkdirSync(downloads);
mkdirSync(join(out, 'snapshots'));
let snapshots = 0;

let failed = 0;
const check = (name, ok, detail = '') => { console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? `  (${detail})` : ''}`); if (!ok) failed++; };

/* ---------------------------------------------------------------- static server */
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm', '.css': 'text/css', '.svg': 'image/svg+xml' };
const requests = [];
let wasmSlowed = false;
const server = http.createServer(async (req, res) => {
  const path = decodeURIComponent(new URL(req.url, 'http://x').pathname).replace(/^\/$/, '/index.html');
  requests.push({ path, at: Date.now() });
  const file = join(dist, path);
  if (!file.startsWith(dist) || !existsSync(file)) { res.writeHead(404); return res.end(); }
  if (path.endsWith('icons.bin')) await sleep(packDelay);
  if (path.endsWith('.wasm') && !wasmSlowed) { wasmSlowed = true; await sleep(wasmDelay); }
  res.writeHead(200, { 'content-type': mime[extname(file)] ?? 'application/octet-stream' });
  res.end(readFileSync(file));
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const url = `http://127.0.0.1:${server.address().port}/index.html`;

/* ---------------------------------------------------------------- browser */
const browser = await puppeteer.launch({
  executablePath: chrome,
  headless: !headed,
  args: ['--window-size=1360,900', '--ignore-gpu-blocklist', '--enable-unsafe-swiftshader'],
});
const logs = [];
async function newPage() {
  const page = await browser.newPage();
  await page.setViewport({ width: 1360, height: 860 });
  page.on('console', (m) => logs.push(`[${m.type()}] ${m.text()}`));
  page.on('pageerror', (e) => logs.push(`[pageerror] ${e.message}`));
  const cdp = await page.createCDPSession();
  await cdp.send('Browser.setDownloadBehavior', { behavior: 'allow', downloadPath: downloads });
  return page;
}
const shot = (page, name) => page.screenshot({ path: join(out, `${name}.png`) });
/** Downloads finished since `since` (the browser overwrites a file of the same name). */
const downloadsSince = (since) => readdirSync(downloads).filter((f) => !f.endsWith('.crdownload') && statSync(join(downloads, f)).mtimeMs >= since);

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

/** Ctrl+S, then the board file the browser downloaded, parsed. */
async function saveAndRead(page) {
  const since = Date.now() - 20;
  await page.keyboard.down('Control');
  await page.keyboard.press('s');
  await page.keyboard.up('Control');
  for (let i = 0; i < 100 && downloadsSince(since).length === 0; i++) await sleep(100);
  await sleep(150);
  const files = downloadsSince(since);
  if (files.length === 0) return null;
  const name = files[0];
  const path = join(out, 'snapshots', `${++snapshots}-${name}`); // the next download overwrites this one
  copyFileSync(join(downloads, name), path);
  return { name, path, json: JSON.parse(readFileSync(path, 'utf8')) };
}
const objectsOf = (json) => json.document.pages.flatMap((p) => p.objects);

async function startPage() {
  const page = await newPage();
  await page.goto(url, { waitUntil: 'load' });
  await page.waitForFunction(() => !document.getElementById('loading'), { timeout: 120000 });
  await sleep(800);
  return page;
}

/* ---------------------------------------------------------------- scenario */
let page = await newPage();
const t0 = Date.now();
await page.goto(url, { waitUntil: 'domcontentloaded' });
await sleep(500);
check('a loading indicator is shown while the page loads', await page.evaluate(() => !!document.getElementById('loading')));
await shot(page, '00-loading');
await page.waitForFunction(() => !document.getElementById('loading'), { timeout: 120000 }).catch(() => {});
check('the page starts (loading indicator removed)', await page.evaluate(() => !document.getElementById('loading')), `${Date.now() - t0} ms`);
await sleep(800);
await shot(page, '01-start');
check('no icon pack downloaded at startup', !requests.some((r) => r.path.endsWith('icons.bin')));
check('the error panel stays hidden', await page.evaluate(() => document.getElementById('error').hidden));
check('tab title set by the application', (await page.title()).includes('Inkboard'), await page.title());

// rectangle: key R, then a drag on the board
await page.mouse.click(1100, 700);
await page.keyboard.press('r');
await drag(page, 380, 260, 596, 380);
await sleep(500);
await shot(page, '02-rectangle');

// sticky note: key N, a click, then text
await page.keyboard.press('Escape');
await page.keyboard.press('n');
await page.mouse.click(800, 420);
await sleep(500);
await page.keyboard.type('Hello from the web');
await sleep(500);
await shot(page, '03-text');
await page.mouse.click(1100, 700); // validates the text
await sleep(400);
check('unsaved mark in the tab title after editing', (await page.title()).includes('●'), await page.title());

// download the board
const saved = await saveAndRead(page);
check('Ctrl+S downloads the board', !!saved, saved?.name);
if (saved) {
  const objs = objectsOf(saved.json);
  check('the downloaded board holds the shape and the note', objs.length >= 2, `${objs.length} objects`);
  check('the note holds the typed text', JSON.stringify(saved.json).includes('Hello from the web'));
}
await sleep(400);
await shot(page, '04-saved');

// icon library: the pack is fetched on demand
const libraryButton = { x: 940, y: 34 };
const beforeClick = requests.length;
await page.mouse.click(libraryButton.x, libraryButton.y);
await sleep(500);
await shot(page, '05-library-loading');
check('opening the library starts the pack download', requests.slice(beforeClick).some((r) => r.path.endsWith('icons.bin')));
await sleep(packDelay + 2500);
await shot(page, '06-library-loaded');
// search, then insert the first result
await page.keyboard.type('router');
await sleep(2500);
await shot(page, '07-library-search');
await page.mouse.click(1015, 220);
await sleep(800);
await page.mouse.click(1320, 91); // closes the library
await sleep(500);
await shot(page, '08-icon-inserted');
const withIcon = await saveAndRead(page);
check('an icon was inserted', !!withIcon && objectsOf(withIcon.json).length > (saved ? objectsOf(saved.json).length : 0));

// open the downloaded file through the file picker
if (saved) {
  await page.mouse.click(1100, 700);
  const chooser = page.waitForFileChooser({ timeout: 10000 });
  await page.keyboard.down('Control');
  await page.keyboard.press('o');
  await page.keyboard.up('Control');
  try {
    await (await chooser).accept([saved.path]);
    await sleep(1500);
    await shot(page, '09-opened');
    const again = await saveAndRead(page);
    check('opening a file through the picker restores that board', !!again && objectsOf(again.json).length === objectsOf(saved.json).length);
  } catch (e) {
    check('opening a file through the picker', false, String(e));
  }
}

// pasted image (synthetic paste event carrying a PNG, as a browser does for an image on the clipboard)
const pngInPage = `(async () => {
  const c = document.createElement('canvas'); c.width = 160; c.height = 100;
  const g = c.getContext('2d'); g.fillStyle = '#e11d48'; g.fillRect(0, 0, 160, 100); g.fillStyle = '#fff'; g.fillRect(30, 30, 100, 40);
  return new Promise((r) => c.toBlob(r, 'image/png'));
})()`;
const countImages = (json) => objectsOf(json).filter((o) => JSON.stringify(o).includes('"Image"') || JSON.stringify(o.kind ?? {}).toLowerCase().includes('image')).length;
const baseline = (await saveAndRead(page))?.json;
await page.evaluate(async (code) => {
  const blob = await eval(code);
  const dt = new DataTransfer();
  dt.items.add(new File([blob], 'image.png', { type: 'image/png' }));
  document.dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true }));
}, pngInPage);
await sleep(1200);
await shot(page, '10-pasted-image');
const afterPaste = (await saveAndRead(page))?.json;
check('a pasted image is inserted', !!afterPaste && !!baseline && countImages(afterPaste) === countImages(baseline) + 1, `${baseline && countImages(baseline)} -> ${afterPaste && countImages(afterPaste)}`);

// dropped image (bytes, no path)
await page.evaluate(async (code) => {
  const blob = await eval(code);
  const dt = new DataTransfer();
  dt.items.add(new File([blob], 'dropped.png', { type: 'image/png' }));
  const target = document.getElementById('inkboard_canvas');
  for (const t of ['dragover', 'drop']) target.dispatchEvent(new DragEvent(t, { dataTransfer: dt, bubbles: true, cancelable: true, clientX: 500, clientY: 500 }));
}, pngInPage);
await sleep(1200);
await shot(page, '11-dropped-image');
const afterDrop = (await saveAndRead(page))?.json;
check('a dropped image is inserted', !!afterDrop && !!afterPaste && countImages(afterDrop) === countImages(afterPaste) + 1, `${afterPaste && countImages(afterPaste)} -> ${afterDrop && countImages(afterDrop)}`);

// ids: a second, independent session must not generate the same ids
await page.close();
const page2 = await startPage();
await page2.mouse.click(1100, 700);
await page2.keyboard.press('r');
await drag(page2, 380, 260, 596, 380);
await sleep(500);
const second = await saveAndRead(page2);
const idsA = new Set(objectsOf(saved.json).map((o) => o.id));
const idsB = objectsOf(second.json).map((o) => o.id);
check('two sessions draw different ids', idsB.length > 0 && idsB.every((id) => !idsA.has(id)), `${[...idsA].join(',')} vs ${idsB.join(',')}`);
await shot(page2, '12-second-session');
await page2.close();

// the main menu, then an export (PNG) of a note
const page3 = await startPage();
await page3.mouse.click(1100, 700);
await page3.keyboard.press('n');
await page3.mouse.click(700, 300);
await sleep(500);
await page3.keyboard.type('Export me');
await page3.mouse.click(1100, 700);
await sleep(400);
await page3.mouse.click(34, 34);
await sleep(600);
await shot(page3, '13-menu');
const sinceExport = Date.now() - 20;
await page3.mouse.click(109, 273); // "Export as PNG"
for (let i = 0; i < 150 && !downloadsSince(sinceExport).some((f) => f.endsWith('.png')); i++) await sleep(100);
const png = downloadsSince(sinceExport).find((f) => f.endsWith('.png'));
check('the PNG export is downloaded', !!png, png);
if (png) {
  const bytes = readFileSync(join(downloads, png));
  check('the downloaded export is a PNG image', bytes.subarray(1, 4).toString() === 'PNG', `${bytes.length} bytes`);
  copyFileSync(join(downloads, png), join(out, '13b-exported.png'));
}
await page3.close();

// a browser without WebGL: a clear message instead of a blank page
const noGl = await puppeteer.launch({ executablePath: chrome, headless: !headed, args: ['--disable-3d-apis', '--disable-gpu', '--disable-software-rasterizer'] });
const page4 = await noGl.newPage();
await page4.setViewport({ width: 1360, height: 860 });
await page4.goto(url, { waitUntil: 'load' });
await page4.waitForFunction(() => !document.getElementById('error').hidden, { timeout: 60000 }).catch(() => {});
// the WebGL build says so in its own words (case "webgl"); the wgpu build reports whatever its start-up error was (case "other")
check('without WebGL the page explains it', await page4.evaluate(() => !document.getElementById('error').hidden && !document.getElementById('loading')), await page4.evaluate(() => document.getElementById('error').dataset.case));
await shot(page4, '14-no-webgl');
await noGl.close();

/* ---------------------------------------------------------------- report */
const bad = logs.filter((l) => l.startsWith('[pageerror]') || l.startsWith('[error]'));
check('no error in the browser console', bad.length === 0, bad.slice(0, 3).join(' | '));
console.log('--- console');
for (const l of logs.slice(0, 40)) console.log(l);
console.log(`--- requests: ${requests.map((r) => r.path).join(' ')}`);
await browser.close();
server.close();
console.log(failed ? `${failed} check(s) failed` : 'all checks passed');
process.exit(failed ? 1 : 0);
