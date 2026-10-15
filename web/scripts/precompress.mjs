// Writes .br and .gz next to compressible build outputs, so Berth can serve
// them without compressing on the fly. Skips files that don't shrink.
import { readdir, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { brotliCompressSync, constants, gzipSync } from 'node:zlib';

const DIST = new URL('../dist/', import.meta.url).pathname;
const COMPRESSIBLE = /\.(js|css|html|svg|json|webmanifest|txt|map)$/;

async function* walk(dir) {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) yield* walk(path);
    else yield path;
  }
}

let saved = 0;
let count = 0;
for await (const file of walk(DIST)) {
  if (!COMPRESSIBLE.test(file) || file.endsWith('.map')) continue;
  const data = await readFile(file);
  if (data.length < 1024) continue;
  const br = brotliCompressSync(data, { params: { [constants.BROTLI_PARAM_QUALITY]: 11, [constants.BROTLI_PARAM_SIZE_HINT]: data.length } });
  const gz = gzipSync(data, { level: 9 });
  if (br.length < data.length * 0.9) {
    await writeFile(`${file}.br`, br);
    saved += data.length - br.length;
    count++;
  }
  if (gz.length < data.length * 0.9) await writeFile(`${file}.gz`, gz);
}
console.log(`precompressed ${count} files (brotli saves ${(saved / 1024).toFixed(0)} KiB)`);
