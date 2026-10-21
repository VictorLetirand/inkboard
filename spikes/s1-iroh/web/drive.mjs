// Opens the guest page in headless Chrome with a ticket in the URL fragment and prints the report.
// Usage: node drive.mjs <page-url-without-fragment> <ticket-file> [chrome-path] [pings] [bytes]
import { readFileSync } from "node:fs";
import puppeteer from "puppeteer-core";

const [pageUrl, ticketFile, chrome, pings, bytes] = process.argv.slice(2);
const chromePath =
  chrome || "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
const ticket = readFileSync(ticketFile, "utf8").trim();
const query = `?pings=${pings ?? 1000}&bytes=${bytes ?? 5 * 1024 * 1024}`;

const browser = await puppeteer.launch({
  executablePath: chromePath,
  headless: true,
  args: ["--no-sandbox"],
});
try {
  const page = await browser.newPage();
  page.on("console", (m) => console.error(`[console.${m.type()}] ${m.text()}`));
  page.on("pageerror", (e) => console.error(`[pageerror] ${e.message}`));
  const t0 = Date.now();
  await page.goto(`${pageUrl}${query}#${ticket}`, { waitUntil: "load" });
  await page.waitForSelector("body[data-state='done'], body[data-state='error']", {
    timeout: 240000,
  });
  const state = await page.evaluate(() => document.body.dataset.state);
  const text = await page.evaluate(() => document.getElementById("result").textContent);
  const version = await browser.version();
  console.log(JSON.stringify({ browser: version, state, wall_ms: Date.now() - t0, result: text }));
} finally {
  await browser.close();
}
