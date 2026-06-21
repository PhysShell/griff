// Boot + paint smoke test for the griff-cockpit wasm build (ADR-0027 Slice 2).
// Serves the prebuilt cockpit/dist (run ./cockpit/build-web.sh first) and boots
// the real eframe/egui app in headless Chromium — WebGL via SwiftShader, no GPU
// — then asserts it paints the cockpit's actual content. This is the headless
// pixel-truth egui_kittest can't give: that tool rasterises through native
// wgpu, which finds no adapter in CI; a browser ships its own software GL.
import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { mkdir, writeFile, readFile, access } from 'node:fs/promises';

import { chromium } from 'playwright';

import { startServer } from './serve.js';
import {
  LAUNCH_ARGS, SIGNATURE, bootPage, canvasShot, decode, analyze, coarseMatch,
} from './helpers.js';

const here = dirname(fileURLToPath(import.meta.url));
const dist = join(here, '..', 'dist');
const outDir = join(here, 'output');

let server;
let browser;
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
  browser = await chromium.launch({ args: LAUNCH_ARGS });
  await mkdir(outDir, { recursive: true });
});

after(async () => {
  await browser?.close();
  await server?.close();
});

test('the wasm cockpit boots in a browser and paints the demo score', async () => {
  const { page, errors } = await bootPage(browser, baseURL);

  const renderer = await page.evaluate(() => {
    const c = document.querySelector('canvas');
    const gl = c && c.getContext('webgl2');
    if (!gl) return null;
    const dbg = gl.getExtension('WEBGL_debug_renderer_info');
    return dbg ? gl.getParameter(dbg.UNMASKED_RENDERER_WEBGL) : 'webgl2';
  });
  assert.ok(renderer, 'eframe should obtain a WebGL2 context');
  console.log('WebGL renderer:', renderer);

  const buf = await canvasShot(page);
  await writeFile(join(outDir, 'cockpit.png'), buf);
  const { counts, nonBg, total } = analyze(decode(buf));

  const pct = (n) => `${((100 * n) / total).toFixed(2)}%`;
  console.log(`non-background ${pct(nonBg)}`);
  for (const [name, n] of Object.entries(counts)) console.log(`  ${name}: ${n} px (${pct(n)})`);

  assert.ok(nonBg / total > 0.05, `canvas looks blank — only ${pct(nonBg)} non-background`);
  for (const [name, c] of Object.entries(SIGNATURE)) {
    const hex = c.map((x) => x.toString(16).padStart(2, '0')).join('');
    assert.ok(counts[name] > 100, `expected the ${name} fill (#${hex}) painted, saw ${counts[name]} px`);
  }
  assert.deepEqual(errors, [], `the page must not error:\n${errors.join('\n')}`);
  await page.close();
});

test('the cockpit re-fits and keeps painting after a resize', async () => {
  const { page, errors } = await bootPage(browser, baseURL, { width: 1100, height: 520 });
  await page.setViewportSize({ width: 640, height: 360 });
  await page.waitForTimeout(1500); // the resize observer re-fits and repaints
  const { nonBg, total } = analyze(decode(await canvasShot(page)));
  assert.ok(nonBg / total > 0.05, 'the cockpit should still paint after shrinking the viewport');
  assert.deepEqual(errors, [], `resize must not error:\n${errors.join('\n')}`);
  await page.close();
});

test('the first frame matches the committed reference', async () => {
  // The default-view render is deterministic (the font is baked into the wasm
  // and SwiftShader is software), so a coarse block-average compare locks the
  // layout against cockpit-reference.png without flaking on AA noise.
  const { page, errors } = await bootPage(browser, baseURL);
  const live = decode(await canvasShot(page));
  const reference = decode(await readFile(join(here, 'cockpit-reference.png')));
  const match = coarseMatch(reference, live);
  console.log(`reference block match ${(100 * match).toFixed(1)}%`);
  assert.ok(match > 0.95, `the render drifted from cockpit-reference.png — ${(100 * match).toFixed(1)}% of blocks match`);
  assert.deepEqual(errors, [], `reference run must not error:\n${errors.join('\n')}`);
  await page.close();
});
