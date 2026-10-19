// Viewer conformance for PlantGen's glTF bark (see README.md).
//
//   node run.mjs DIR [--out DIR] [--chromium PATH]
//
// DIR holds probe-up.glb and probe-round.glb (`plantc bark --probe`) and
// any bark GLBs (`plantc bark --look`). Every file must pass the Khronos
// glTF validator with no errors. three.js and Babylon.js then draw each
// file in headless Chromium the same way (page.html). Each probe's
// shading must follow the light as its relief says it should: a normal
// map read upside down or mirrored turns the correlation negative. Each
// bark file must look the same in both renderers.
import { createServer } from 'node:http';
import { readFile, readdir, mkdir, writeFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { join, extname, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import validator from 'gltf-validator';
import { chromium } from 'playwright-core';

const here = dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const option = (name, fallback) => {
  const at = args.indexOf(name);
  return at >= 0 ? args[at + 1] : fallback;
};
const dir = resolve(args.find((a, i) => !a.startsWith('--') && !args[i - 1]?.startsWith('--')) ?? '.');
const out = resolve(option('--out', join(here, 'out')));
const chromiumPath = option(
  '--chromium',
  existsSync('/opt/pw-browsers/chromium-1194/chrome-linux/chrome') ? '/opt/pw-browsers/chromium-1194/chrome-linux/chrome' : undefined,
);

const SIZE = 256;
// Metres either side of the view's middle (page.html): 1.2 m square for
// the probes; a trunk fills six tenths of the view whatever its diameter.
const HALF = 0.6;
const halfFor = (bytes) => {
  const view = new DataView(bytes.buffer, bytes.byteOffset);
  const length = view.getUint32(12, true);
  const json = JSON.parse(new TextDecoder().decode(bytes.subarray(20, 20 + length)));
  const diameter = json.scenes?.[0]?.extras?.diameter_m ?? 1;
  return HALF * Math.min(1, diameter);
};
// The probes, as `plantc bark --probe` draws them: a metre of trunk a
// metre across, tiles of 0.25 m, four sine waves a tile sloping at most
// one in two, albedo 0.5 (linear), fully rough.
const PERIOD = 0.25;
const WAVES = 4;
const DIAMETER = 1;
const WRAPS = Math.max(1, Math.round((Math.PI * DIAMETER) / PERIOD));
// Correlation every probe must reach; mean difference in sRGB levels
// between the renderers that bark files must stay under.
const MIN_CORRELATION = 0.95;
// A probe's mean brightness over Lambert's: catches colour read in the
// wrong space or lit in other units.
const SCALE = [0.9, 1.1];
const MAX_MEAN_DIFFERENCE = 6;
// Mean colours may differ by at most this CIE76 ΔE, about the smallest
// difference people notice side by side (2.3); the renderers' specular
// terms differ a little.
const MAX_DELTA_E = 3;

const toLinear = (c) => {
  const v = c / 255;
  return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
};
// sRGB levels to CIELAB (D65).
const lab = (rgb) => {
  const [r, g, b] = rgb.map(toLinear);
  const xyz = [
    (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047,
    0.2126 * r + 0.7152 * g + 0.0722 * b,
    (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883,
  ].map((t) => (t > 216 / 24389 ? Math.cbrt(t) : (24389 / 27 * t + 16) / 116));
  return [116 * xyz[1] - 16, 500 * (xyz[0] - xyz[1]), 200 * (xyz[1] - xyz[2])];
};
const normalize = (v) => {
  const l = Math.hypot(...v);
  return v.map((x) => x / l);
};
const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const frac = (x) => x - Math.floor(x);
// Pixel (column, row) to the place it shows, metres.
const place = (c, r) => [-HALF + ((c + 0.5) / SIZE) * 2 * HALF, 0.5 + HALF - ((r + 0.5) / SIZE) * 2 * HALF];
const luminance = (pixels, c, r) => {
  const i = (r * SIZE + c) * 4;
  return (toLinear(pixels[i]) + toLinear(pixels[i + 1]) + toLinear(pixels[i + 2])) / 3;
};
const correlation = (a, b) => {
  const n = a.length;
  const ma = a.reduce((s, x) => s + x, 0) / n;
  const mb = b.reduce((s, x) => s + x, 0) / n;
  let ab = 0, aa = 0, bb = 0;
  for (let i = 0; i < n; i++) {
    ab += (a[i] - ma) * (b[i] - mb);
    aa += (a[i] - ma) ** 2;
    bb += (b[i] - mb) ** 2;
  }
  return ab / Math.sqrt(aa * bb);
};

// Expected linear radiance of the probes, Lambertian, light of 1.
const PROBES = {
  'probe-up.glb': {
    light: [0, 1, 1],
    // Down the middle column, the trunk's face.
    samples() {
      const list = [];
      const c = SIZE / 2;
      for (let r = 0; r < SIZE; r++) {
        const [, y] = place(c, r);
        if (y < 0.05 || y > 0.95) continue;
        const t = frac(-y / PERIOD);
        // dh/dy: the relief's slope up the stem.
        const slope = -0.5 * Math.cos(2 * Math.PI * WAVES * t);
        const n = normalize([0, -slope, 1]);
        list.push({ c, r, expected: 0.5 * Math.max(0, dot(n, normalize(this.light))) });
      }
      return list;
    },
  },
  'probe-round.glb': {
    light: [1, 0, 1],
    // Across the middle row, round the trunk's front.
    samples() {
      const list = [];
      const r = Math.round(SIZE / 2 + (0.5 - 0.5) * SIZE);
      const radius = DIAMETER / 2;
      for (let c = 0; c < SIZE; c++) {
        const [x] = place(c, r);
        if (Math.abs(x) > 0.8 * radius) continue;
        const phi = Math.asin(x / radius);
        const u = 0.75 * WRAPS + (radius * phi * WRAPS) / (Math.PI * DIAMETER);
        const t = frac(u);
        // dh/ds round the stem, per metre of arc.
        const perMetre = WRAPS / (Math.PI * DIAMETER);
        const slope = (PERIOD / (4 * Math.PI * WAVES)) * 2 * Math.PI * WAVES * Math.cos(2 * Math.PI * WAVES * t) * perMetre;
        const tangent = [Math.cos(phi), 0, -Math.sin(phi)];
        const normal = [Math.sin(phi), 0, Math.cos(phi)];
        const n = normalize([0, 1, 2].map((k) => normal[k] - slope * tangent[k]));
        list.push({ c, r, expected: 0.5 * Math.max(0, dot(n, normalize(this.light))) });
      }
      return list;
    },
  },
};
const BARK_LIGHT = [0.4, 0.8, 1];

const types = { '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.glb': 'model/gltf-binary', '.json': 'application/json' };
const server = createServer(async (request, response) => {
  const path = decodeURIComponent(new URL(request.url, 'http://x').pathname);
  const file = path.startsWith('/glb/') ? join(dir, path.slice(5)) : join(here, path === '/' ? 'page.html' : path);
  try {
    const body = await readFile(file);
    response.writeHead(200, { 'content-type': types[extname(file)] ?? 'application/octet-stream' });
    response.end(body);
  } catch {
    response.writeHead(404);
    response.end();
  }
});
await new Promise((ok) => server.listen(0, '127.0.0.1', ok));
const origin = `http://127.0.0.1:${server.address().port}`;

const files = (await readdir(dir)).filter((f) => f.endsWith('.glb')).sort();
await mkdir(out, { recursive: true });
let failures = 0;
const fail = (message) => {
  failures += 1;
  console.log(`  FAIL ${message}`);
};
const browser = await chromium.launch({
  executablePath: chromiumPath,
  args: ['--use-gl=angle', '--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--ignore-gpu-blocklist'],
});
const page = await browser.newPage();
page.on('pageerror', (e) => console.log(`  page error: ${e.message}`));
await page.goto(origin + '/');
await page.waitForFunction(() => window.ready === true);

const report = [];
for (const file of files) {
  console.log(file);
  const bytes = new Uint8Array(await readFile(join(dir, file)));
  const result = await validator.validateBytes(bytes);
  const { numErrors, numWarnings } = result.issues;
  console.log(`  glTF validator: ${numErrors} errors, ${numWarnings} warnings`);
  for (const m of result.issues.messages.filter((m) => m.severity <= 1)) {
    console.log(`    ${m.severity === 0 ? 'error' : 'warning'} ${m.code} ${m.pointer ?? ''} ${m.message}`);
  }
  if (numErrors > 0) fail(`${file}: the glTF validator found ${numErrors} errors`);
  const probe = PROBES[file];
  const light = probe ? probe.light : BARK_LIGHT;
  const drawn = {};
  for (const engine of ['three', 'babylon']) {
    const base64 = await page.evaluate(([e, f, l, s, h]) => window.draw(e, f, l, s, h), [engine, file, light, SIZE, halfFor(bytes)]);
    drawn[engine] = Uint8Array.from(Buffer.from(base64, 'base64'));
    const url = await page.evaluate(([b, s]) => window.png(b, s), [base64, SIZE]);
    await writeFile(join(out, `${file.replace('.glb', '')}-${engine}.png`), Buffer.from(url.split(',')[1], 'base64'));
  }
  const row = { file, errors: numErrors, warnings: numWarnings };
  if (probe) {
    const samples = probe.samples();
    for (const engine of ['three', 'babylon']) {
      const seen = samples.map(({ c, r }) => luminance(drawn[engine], c, r));
      const expected = samples.map((s) => s.expected);
      const k = correlation(seen, expected);
      const scale = seen.reduce((s, x) => s + x, 0) / expected.reduce((s, x) => s + x, 0);
      console.log(`  ${engine}: shading follows the relief, correlation ${k.toFixed(3)}; brightness ${scale.toFixed(2)} of Lambert's`);
      row[engine] = { correlation: k, scale };
      if (!(k >= MIN_CORRELATION)) fail(`${file} in ${engine}: correlation ${k.toFixed(3)} < ${MIN_CORRELATION}`);
      if (!(scale >= SCALE[0] && scale <= SCALE[1])) fail(`${file} in ${engine}: brightness ${scale.toFixed(2)} of Lambert's`);
    }
  } else {
    // Over the pixels both renderers cover with trunk.
    let n = 0, sum = 0;
    const means = { three: [0, 0, 0], babylon: [0, 0, 0] };
    for (let i = 0; i < SIZE * SIZE * 4; i += 4) {
      const a = drawn.three, b = drawn.babylon;
      if (a[i] + a[i + 1] + a[i + 2] === 0 || b[i] + b[i + 1] + b[i + 2] === 0) continue;
      n += 1;
      for (let k = 0; k < 3; k++) {
        sum += Math.abs(a[i + k] - b[i + k]) / 3;
        means.three[k] += a[i + k];
        means.babylon[k] += b[i + k];
      }
    }
    const mean = sum / n;
    const a = lab(means.three.map((x) => x / n));
    const b = lab(means.babylon.map((x) => x / n));
    const deltaE = Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
    console.log(`  three.js vs Babylon.js: mean difference ${mean.toFixed(1)} sRGB levels a pixel; mean colours ΔE ${deltaE.toFixed(1)}`);
    row.difference = mean;
    row.deltaE = deltaE;
    if (!(mean <= MAX_MEAN_DIFFERENCE)) fail(`${file}: renderers differ by ${mean.toFixed(1)} levels a pixel (> ${MAX_MEAN_DIFFERENCE})`);
    if (!(deltaE <= MAX_DELTA_E)) fail(`${file}: mean colours ΔE ${deltaE.toFixed(1)} (> ${MAX_DELTA_E})`);
  }
  report.push(row);
}
await writeFile(join(out, 'report.json'), JSON.stringify(report, null, 1) + '\n');
await browser.close();
server.close();
console.log(failures ? `${failures} failures` : `all ${files.length} files pass`);
process.exit(failures ? 1 : 0);
