// Headless-browser smoke test for the griff-cockpit wasm build (ADR-0027 Slice
// 2). It builds nothing: it serves a prebuilt cockpit/dist (run
// ./cockpit/build-web.sh first), boots the real eframe/egui app in headless
// Chromium — WebGL via SwiftShader, no GPU needed — and asserts it actually
// paints the cockpit: a WebGL2 context is obtained, the canvas is non-blank,
// the cockpit's signature fill colours are present, and nothing throws.
//
// This is the headless pixel-truth `egui_kittest` could not give us: that tool
// rasterises through *native* wgpu, which finds no GPU adapter in CI; a browser
// ships its own software GL, so the canonical web build is verifiable for real.
import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { mkdir, writeFile, access } from 'node:fs/promises';

import { chromium } from 'playwright';
import { PNG } from 'pngjs';

import { startServer } from './serve.js';

const here = dirname(fileURLToPath(import.meta.url));
const dist = join(here, '..', 'dist');
const outDir = join(here, 'output');

// Signature solid fills from cockpit/src/lib.rs (role -> colour). Solid fills
// are flat, so their interiors match near-exactly; anti-aliased edges are noise.
const SIGNATURE = {
  'note lane-0 (orange)': [0xff, 0x7a, 0x45],
  'Riff band (blue)': [0x16, 0x68, 0xdc],
  'Breakdown band (red)': [0xcf, 0x13, 0x22],
  'playhead (yellow)': [0xff, 0xcf, 0x4d],
};
const BG = [0x1b, 0x1b, 0x1f]; // index.html body background

const near = (px, [r, g, b], tol = 12) =>
  Math.abs(px[0] - r) <= tol && Math.abs(px[1] - g) <= tol && Math.abs(px[2] - b) <= tol;

let server;
let baseURL;

before(async () => {
  try {
    await access(join(dist, 'index.html'));
    await access(join(dist, 'griff_cockpit_bg.wasm'));
  } catch {
    throw new Error(`cockpit/dist is not built — run ./cockpit/build-web.sh first (looked in ${dist})`);
  }
  server = await startServer(dist);
  baseURL = `http://127.0.0.1:${server.address().port}/index.html`;
  await mkdir(outDir, { recursive: true });
});

after(async () => {
  await server?.close();
});

test('the wasm cockpit boots in a browser and paints the demo score', async () => {
  const browser = await chromium.launch({
    args: [
      '--ignore-gpu-blocklist',
      '--use-gl=angle',
      '--use-angle=swiftshader',
      '--enable-unsafe-swiftshader',
    ],
  });
  const errors = [];
  try {
    const page = await browser.newPage({ viewport: { width: 1100, height: 520 }, deviceScaleFactor: 2 });
    page.on('pageerror', (e) => errors.push(`pageerror: ${e.message}`));
    page.on('console', (m) => {
      if (m.type() === 'error') errors.push(`console.error: ${m.text()}`);
    });

    await page.goto(baseURL, { waitUntil: 'load' });
    // index.html removes #loading once the wasm init() resolves.
    await page.waitForFunction(() => !document.getElementById('loading'), { timeout: 30000 });

    // eframe must really hold a WebGL2 context (report the renderer it got).
    const renderer = await page.evaluate(() => {
      const c = document.querySelector('canvas');
      const gl = c && c.getContext('webgl2');
      if (!gl) return null;
      const dbg = gl.getExtension('WEBGL_debug_renderer_info');
      return dbg ? gl.getParameter(dbg.UNMASKED_RENDERER_WEBGL) : 'webgl2';
    });
    assert.ok(renderer, 'eframe should obtain a WebGL2 context');
    console.log('WebGL renderer:', renderer);

    await page.waitForTimeout(4000); // let eframe paint a few frames

    const shot = await page.locator('canvas').screenshot();
    await writeFile(join(outDir, 'cockpit.png'), shot);
    const img = PNG.sync.read(shot);

    const counts = Object.fromEntries(Object.keys(SIGNATURE).map((k) => [k, 0]));
    let nonBg = 0;
    const total = img.width * img.height;
    for (let i = 0; i < img.data.length; i += 4) {
      const px = [img.data[i], img.data[i + 1], img.data[i + 2]];
      if (!near(px, BG, 16)) nonBg += 1;
      for (const [name, c] of Object.entries(SIGNATURE)) if (near(px, c)) counts[name] += 1;
    }
    const pct = (n) => `${((100 * n) / total).toFixed(2)}%`;
    console.log(`canvas ${img.width}x${img.height}  non-background ${pct(nonBg)}`);
    for (const [name, n] of Object.entries(counts)) console.log(`  ${name}: ${n} px (${pct(n)})`);

    assert.ok(nonBg / total > 0.05, `canvas looks blank — only ${pct(nonBg)} non-background`);
    for (const [name, c] of Object.entries(SIGNATURE)) {
      const hex = c.map((x) => x.toString(16).padStart(2, '0')).join('');
      assert.ok(counts[name] > 100, `expected the ${name} fill (#${hex}) to be painted, saw ${counts[name]} px`);
    }
    assert.deepEqual(errors, [], `the page must not error:\n${errors.join('\n')}`);
  } finally {
    await browser.close();
  }
});
