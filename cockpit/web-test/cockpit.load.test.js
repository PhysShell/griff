// Load flow (ADR-0027 Slice 3a): the page's file input hands bytes to the wasm
// `load_score` export, which the running eframe app drains and re-imports
// through the shared parser. Picking a multi-track file both repaints a new
// score and, since it has several tracks, exercises lane colours the
// single-track demo never shows in-browser.
import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { access } from 'node:fs/promises';

import { chromium } from 'playwright';

import { startServer } from './serve.js';
import { LAUNCH_ARGS, bootPage, canvasShot, decode, frameDiff, countColor } from './helpers.js';

const here = dirname(fileURLToPath(import.meta.url));
const dist = join(here, '..', 'dist');
const multiTrack = join(here, '..', 'assets', 'multi_track.mid');
const LANE1_TEAL = [0x36, 0xcf, 0xc9]; // lane_color(1), absent from the single-track demo

let server;
let browser;
let baseURL;

before(async () => {
  try {
    await access(join(dist, 'griff_cockpit_bg.wasm'));
  } catch {
    throw new Error('cockpit/dist is not built — run ./cockpit/build-web.sh first');
  }
  server = await startServer(dist);
  baseURL = `http://127.0.0.1:${server.address().port}/index.html`;
  browser = await chromium.launch({ args: LAUNCH_ARGS });
});

after(async () => {
  await browser?.close();
  await server?.close();
});

test('picking a file loads and paints the chosen score', async () => {
  const { page, errors } = await bootPage(browser, baseURL);
  const before = decode(await canvasShot(page));
  assert.equal(countColor(before, LANE1_TEAL), 0, 'the single-track demo has no lane-1 teal');

  await page.setInputFiles('#file', multiTrack);
  await page.waitForTimeout(1500); // the app drains the inbox and re-fits

  const after = decode(await canvasShot(page));
  const d = frameDiff(before, after);
  const teal = countColor(after, LANE1_TEAL);
  console.log(`load frameDiff ${(100 * d).toFixed(1)}%  lane-1 teal ${teal}px`);
  assert.ok(d > 0.02, `loading a new score should change the frame, diff was ${(100 * d).toFixed(1)}%`);
  assert.ok(teal > 100, `a multi-track load should paint lane-1 teal, saw ${teal}px`);
  assert.deepEqual(errors, [], `loading must not error:\n${errors.join('\n')}`);
  await page.close();
});
