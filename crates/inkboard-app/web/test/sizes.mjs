// Prints the raw / gzip -9 / brotli sizes of every file of the given dist folders.
//   node sizes.mjs ../dist
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { gzipSync, brotliCompressSync, constants } from 'node:zlib';
import { join } from 'node:path';

const mb = (n) => (n / 1048576).toFixed(2).padStart(7) + ' MB';
for (const dir of process.argv.slice(2)) {
  console.log(`== ${dir}`);
  let total = [0, 0, 0];
  for (const f of readdirSync(dir)) {
    const p = join(dir, f);
    if (!statSync(p).isFile()) continue;
    const buf = readFileSync(p);
    const gz = gzipSync(buf, { level: 9 }).length;
    const br = brotliCompressSync(buf, { params: { [constants.BROTLI_PARAM_QUALITY]: 11, [constants.BROTLI_PARAM_SIZE_HINT]: buf.length } }).length;
    console.log(`${f.padEnd(40)} raw ${mb(buf.length)}  gzip ${mb(gz)}  brotli ${mb(br)}`);
    total = [total[0] + buf.length, total[1] + gz, total[2] + br];
  }
  console.log(`${'TOTAL'.padEnd(40)} raw ${mb(total[0])}  gzip ${mb(total[1])}  brotli ${mb(total[2])}`);
}
