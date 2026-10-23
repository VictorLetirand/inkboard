// Browser drivers of the e2e bench: one small interface over the engines the bench can drive.
//
//   chrome   puppeteer-core + CDP        (CHROME=<chrome.exe>)
//   edge     puppeteer-core + CDP        (EDGE=<msedge.exe>)        same engine as Chrome, other build
//   firefox  puppeteer-core + WebDriver BiDi, stock Firefox (FIREFOX=<firefox.exe>)
//   webkit   Playwright's WebKit build (PLAYWRIGHT_DIR=<playwright-core>, PLAYWRIGHT_BROWSERS_PATH): an APPROXIMATION of Safari, never Safari itself
//
// The pages handed back look like puppeteer pages (the bench is written against that API); the Playwright ones are wrapped.
// What an engine cannot do through its automation protocol is told by `caps`; the bench then reports SKIP, never a false PASS.
//
// Firefox and WebKit have no CDP: they go through a local proxy ("gate") that can refuse the relay (403, like a company gateway) or cut
// every tunnel (503). That is a network cut seen from inside the page, WITHOUT the browser's `online` / `offline` events.
import http from 'node:http';
import net from 'node:net';
import { createRequire } from 'node:module';
import { join } from 'node:path';

const requireFrom = (dir, fallback) => createRequire(import.meta.url)(dir ?? fallback);

/** A local HTTP proxy (CONNECT only). Modes: `pass`, `block` (the relay domains get 403), `cut` (everything gets 503, tunnels dropped). */
export async function createGate(mode = 'pass') {
  const gate = { mode, port: 0, tunnels: new Set(), refused: 0, connects: 0 };
  const server = http.createServer((_req, res) => { res.writeHead(502); res.end(); });
  server.on('connect', (req, client, head) => {
    const [host, port] = req.url.split(':');
    gate.connects++;
    if (gate.mode === 'cut' || (gate.mode === 'block' && /iroh\.link\.?$/i.test(host))) {
      gate.refused++;
      client.end(`HTTP/1.1 ${gate.mode === 'cut' ? '503 Service Unavailable' : '403 Forbidden'}\r\nConnection: close\r\n\r\n`);
      return;
    }
    const up = net.connect(Number(port) || 443, host, () => {
      client.write('HTTP/1.1 200 Connection Established\r\n\r\n');
      if (head.length) up.write(head);
      up.pipe(client);
      client.pipe(up);
    });
    const done = () => { client.destroy(); up.destroy(); gate.tunnels.delete(client); gate.tunnels.delete(up); };
    gate.tunnels.add(client);
    gate.tunnels.add(up);
    up.on('error', done); client.on('error', done); up.on('close', done); client.on('close', done);
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  gate.port = server.address().port;
  gate.cut = (on) => { gate.mode = on ? 'cut' : 'pass'; if (on) for (const s of gate.tunnels) s.destroy(); };
  gate.close = () => { for (const s of gate.tunnels) s.destroy(); server.close(); };
  return gate;
}

const PHONE_UA = 'Mozilla/5.0 (Linux; Android 11; Pixel 5) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/155.0.0.0 Mobile Safari/537.36';

export async function createDriver(engine, { headed, downloads, out }) {
  const env = process.env;
  const gates = [];
  const lang = 'en-US';
  const base = { engine, gates, caps: {} };

  /* ------------------------------------------------------------ Chromium family: Chrome, Edge */
  if (engine === 'chrome' || engine === 'edge') {
    const puppeteer = requireFrom(env.PUPPETEER_DIR, 'puppeteer-core');
    const executablePath = engine === 'edge'
      ? env.EDGE ?? 'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe'
      : env.CHROME ?? env.BROWSER_PATH ?? 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe';
    const chromeArgs = [`--lang=${lang}`, '--window-size=1360,900', '--ignore-gpu-blocklist', '--enable-unsafe-swiftshader',
      '--disable-background-timer-throttling', '--disable-renderer-backgrounding', '--disable-backgrounding-occluded-windows'];
    const device = puppeteer.KnownDevices['Pixel 5'];
    // Emulating a phone only changes what the page is told: the canvas is sized from the REAL pixels of the screen, so the browser runs
    // with the same real scale factor (2), or the interface would be drawn at the wrong size.
    const phone = { ...device, viewport: { ...device.viewport, deviceScaleFactor: 2 } };
    const perms = ['clipboard-read', 'clipboard-write', 'clipboard-sanitized-write'];
    return {
      ...base,
      caps: { netEvents: true, clipboardRead: true, touch: true, phoneEmulation: true },
      async launch({ mobile = false, blockRelays = false } = {}) {
        const args = [...chromeArgs];
        if (mobile) args.push('--force-device-scale-factor=2');
        // the relay names end with a dot in the links ("….iroh.link."): both spellings are refused, as a DNS that blocks the domain would
        if (blockRelays) args.push('--host-resolver-rules=MAP *.iroh.link ~NOTFOUND, MAP *.iroh.link. ~NOTFOUND');
        const browser = await puppeteer.launch({ executablePath, headless: !headed, args });
        return {
          defaultContext: () => browser.defaultBrowserContext(),
          newContext: () => browser.createBrowserContext(),
          close: () => browser.close(),
        };
      },
      grant: async (ctx, origin) => { await ctx.overridePermissions(origin, perms); return true; },
      async setupDownloads(page) {
        const cdp = await page.createCDPSession();
        await cdp.send('Browser.setDownloadBehavior', { behavior: 'allow', downloadPath: downloads });
      },
      setOffline: (page, on) => page.setOfflineMode(on),
      emulatePhone: (page) => page.emulate(phone),
      newPage: async (ctx) => ctx.newPage(),
    };
  }

  /* ------------------------------------------------------------ Firefox through WebDriver BiDi */
  if (engine === 'firefox') {
    const puppeteer = requireFrom(env.PUPPETEER_DIR, 'puppeteer-core');
    const executablePath = env.FIREFOX ?? 'C:\\Program Files\\Mozilla Firefox\\firefox.exe';
    return {
      ...base,
      caps: { netEvents: false, clipboardRead: true, touch: false, phoneEmulation: false, syntheticPaste: false },
      async launch({ mobile = false, blockRelays = false } = {}) {
        const gate = await createGate(blockRelays ? 'block' : 'pass');
        gates.push(gate);
        const extraPrefsFirefox = {
          'intl.accept_languages': `${lang}, en`,
          'network.proxy.type': 1,
          'network.proxy.http': '127.0.0.1', 'network.proxy.http_port': gate.port,
          'network.proxy.ssl': '127.0.0.1', 'network.proxy.ssl_port': gate.port,
          'network.proxy.no_proxies_on': ['localhost', '127.0.0.1', env.BENCH_LAN].filter(Boolean).join(', '), // BENCH_LAN: a LAN address of this machine (insecure-context probe)
          'browser.download.folderList': 2,
          'browser.download.dir': downloads,
          'browser.download.useDownloadDir': true,
          'browser.helperApps.neverAsk.saveToDisk': 'application/octet-stream,application/json,image/png,application/x-inkboard',
          'browser.download.manager.showWhenStarting': false,
          'dom.events.testing.asyncClipboard': true,
        };
        // a phone is emulated by the page size; the REAL scale factor is 2 as well (the canvas is sized from real pixels), and a coarse pointer
        if (mobile) Object.assign(extraPrefsFirefox, { 'layout.css.devPixelsPerPx': '2', 'ui.primaryPointerCapabilities': 1, 'dom.w3c_touch_events.enabled': 1 });
        const browser = await puppeteer.launch({ browser: 'firefox', executablePath, headless: !headed, extraPrefsFirefox });
        const first = browser.defaultBrowserContext();
        first.gate = gate;
        return {
          defaultContext: () => first,
          newContext: async () => { const c = await browser.createBrowserContext(); c.gate = gate; return c; },
          close: async () => { await browser.close(); gate.close(); },
        };
      },
      grant: async (ctx, origin) => {
        // BiDi permissions may be refused; the testing pref dom.events.testing.asyncClipboard lets the page read the clipboard anyway
        try { await ctx.overridePermissions(origin, ['clipboard-read', 'clipboard-write']); } catch { /* pref */ }
        return true;
      },
      setupDownloads: async () => {},
      setOffline: (page, on) => page.browserContext().gate.cut(on),
      emulatePhone: async (page) => {
        await page.setUserAgent(PHONE_UA).catch(() => {});
        await page.setViewport({ width: 393, height: 851, deviceScaleFactor: 2, isMobile: true, hasTouch: true }).catch(async () => {
          await page.setViewport({ width: 393, height: 851 });
        });
      },
      newPage: async (ctx) => ctx.newPage(),
    };
  }

  /* ------------------------------------------------------------ WebKit (Playwright build) */
  if (engine === 'webkit') {
    if (env.PLAYWRIGHT_BROWSERS_PATH === undefined) throw new Error('set PLAYWRIGHT_BROWSERS_PATH (and PLAYWRIGHT_DIR) for the webkit engine');
    const { webkit, devices } = requireFrom(env.PLAYWRIGHT_DIR, 'playwright-core');
    const phone = devices['iPhone 13'];
    const wrap = (page, gate) => {
      const own = {
        waitForFunction: (fn, opts) => page.waitForFunction(fn, undefined, opts),
        evaluateOnNewDocument: (fn, arg) => page.addInitScript(fn, arg),
        setViewport: (v) => page.setViewportSize({ width: v.width, height: v.height }),
        gate,
      };
      return new Proxy(page, {
        get: (t, k) => (k in own ? own[k] : typeof t[k] === 'function' ? t[k].bind(t) : t[k]),
        set: (t, k, v) => { t[k] = v; return true; },
      });
    };
    const ctxWrap = (context, gate) => ({
      raw: context,
      overridePermissions: (origin, perms) => context.grantPermissions(perms, { origin }),
      newPage: async () => {
        const page = await context.newPage();
        // downloads: saved where the bench looks for them
        page.on('download', (d) => { d.saveAs(join(downloads, d.suggestedFilename())).catch(() => {}); });
        return wrap(page, gate);
      },
    });
    return {
      ...base,
      caps: { netEvents: 'unknown', clipboardRead: false, touch: true, phoneEmulation: true },
      async launch({ mobile = false, blockRelays = false } = {}) {
        const gate = await createGate(blockRelays ? 'block' : 'pass');
        gates.push(gate);
        const browser = await webkit.launch({ headless: !headed, proxy: { server: `http://127.0.0.1:${gate.port}`, bypass: ['localhost', '127.0.0.1', env.BENCH_LAN].filter(Boolean).join(',') } });
        const options = mobile ? { ...phone, locale: lang } : { viewport: { width: 1360, height: 860 }, locale: lang };
        const make = async () => ctxWrap(await browser.newContext(options), gate);
        const first = await make();
        return {
          defaultContext: () => first,
          newContext: make,
          close: async () => { await browser.close(); gate.close(); },
        };
      },
      // Playwright's WebKit knows no clipboard permission ("Unknown permission"): nothing to grant, and no read-back
      grant: async () => false,
      setupDownloads: async () => {},
      // Playwright's offline mode (navigator.onLine, events) does not drop a WebSocket that is already open: the proxy cuts the tunnels as well
      setOffline: async (page, on) => { await page.context().setOffline(on); page.gate.cut(on); },
      emulatePhone: async () => {},
      newPage: async (ctx) => ctx.newPage(),
    };
  }
  throw new Error(`unknown browser "${engine}" (chrome, edge, firefox, webkit)`);
}
