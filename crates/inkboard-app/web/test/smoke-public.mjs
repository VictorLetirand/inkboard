// Load smoke of a PUBLISHED page (default: the public Inkboard page) in one engine: does it start, is WebGL 2 there, does the board accept
// a shape? No relay, no host: this is only "does the page run in this browser over https".
//
//   node smoke-public.mjs [--browser chrome|edge|firefox|webkit] [--url https://…/] [--out ./results-public]
//
// Same environment variables as e2e.mjs (see drivers.mjs). Exit code 1 when a check fails.
import { mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import { join, resolve, dirname } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { createDriver } from './drivers.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const argv = process.argv.slice(2);
const opt = (name, dflt) => { const i = argv.indexOf(`--${name}`); return i < 0 ? dflt : argv[i + 1]; };
const engine = opt('browser', process.env.BROWSER ?? 'chrome');
const url = opt('url', 'https://victorletirand.github.io/inkboard/');
const out = resolve(opt('out', join(here, 'results-public')));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
mkdirSync(out, { recursive: true });
const downloads = mkdtempSync(join(tmpdir(), 'inkboard-smoke-'));
let failed = 0;
const check = (name, ok, detail = '') => { console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? `  (${detail})` : ''}`); if (!ok) failed++; };

const drv = await createDriver(engine, { headed: argv.includes('--headed'), downloads, out });
const browser = await drv.launch();
const logs = [];
try {
  const page = await drv.newPage(browser.defaultContext());
  await page.setViewport({ width: 1360, height: 860 });
  page.on('console', (m) => logs.push(`[${m.type()}] ${m.text()}`));
  page.on('pageerror', (e) => logs.push(`[pageerror] ${e.message ?? e}`));
  console.log(`--- ${engine}: ${url}`);
  console.log(`UA ${await page.evaluate(() => navigator.userAgent).catch(() => '?')}`);
  await page.goto(url, { waitUntil: 'load' });
  const gl = await page.evaluate(() => {
    const g = document.createElement('canvas').getContext('webgl2');
    return { webgl2: !!g, renderer: g ? g.getParameter(g.RENDERER) : null, secure: isSecureContext, wasm: typeof WebAssembly };
  });
  check('WebGL 2 and WebAssembly are there', gl.webgl2 && gl.wasm === 'object', JSON.stringify(gl));
  let started = false;
  for (let i = 0; i < 240 && !started; i++) {
    started = await page.evaluate(() => !document.getElementById('loading') && document.getElementById('error').hidden && !!document.getElementById('inkboard_canvas')).catch(() => false);
    if (!started) {
      const err = await page.evaluate(() => { const e = document.getElementById('error'); return e.hidden ? null : `${e.dataset.case}: ${e.innerText.slice(0, 200)}`; }).catch(() => null);
      if (err) { console.log(`  error panel: ${err}`); break; }
      await sleep(500);
    }
  }
  await sleep(1000);
  await page.screenshot({ path: join(out, `public-${engine}-start.png`) });
  check('the page starts (loading screen gone, no error panel, canvas present)', started);
  check('secure context (https)', await page.evaluate(() => isSecureContext));
  const size = await page.evaluate(() => { const c = document.getElementById('inkboard_canvas'); return c ? [c.width, c.height] : [0, 0]; });
  check('the canvas has a size', size[0] > 0 && size[1] > 0, size.join('x'));
  // a rectangle with the rectangle tool (key R) must change what is drawn
  const snap = async (path) => { const r = await page.screenshot({ ...(path ? { path } : {}), encoding: 'base64' }); return typeof r === 'string' ? r : r.toString('base64'); };
  const before = await snap();
  await page.mouse.click(1300, 800);
  await page.keyboard.press('r');
  await page.mouse.move(600, 400); await sleep(60); await page.mouse.down(); await sleep(60);
  for (let i = 1; i <= 12; i++) { await page.mouse.move(600 + i * 15, 400 + i * 9); await sleep(25); }
  await page.mouse.up();
  await sleep(600);
  const after = await snap(join(out, `public-${engine}-rectangle.png`));
  check('drawing a rectangle changes the board', before !== after);
  const bad = logs.filter((l) => /^\[(pageerror|error)\]/.test(l) && !/Viewport argument key|Failed to load resource/.test(l));
  check('no unexpected error in the console', bad.length === 0, bad.slice(0, 3).join(' | '));
} catch (e) {
  console.error(`ERROR ${e.stack ?? e}`);
  failed++;
} finally {
  await browser.close().catch(() => {});
  rmSync(downloads, { recursive: true, force: true });
}
console.log(failed ? `${failed} check(s) failed` : 'all checks passed');
process.exit(failed ? 1 : 0);
