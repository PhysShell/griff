// In-browser interaction tests: real DOM key presses drive the eframe app's
// input -> intent -> viewport path, observed through the rendered canvas. The
// native suite (cockpit/src/lib.rs) asserts the same wiring against Viewport
// state directly; this proves it survives the wasm + browser-keyboard round-trip.
import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { access } from 'node:fs/promises';

import { chromium } from 'playwright';

import { startServer } from './serve.js';
import { LAUNCH_ARGS, bootPage, canvasShot, decode, frameDiff, findPlayheadX } from './helpers.js';

const here = dirname(fileURLToPath(import.meta.url));
const dist = join(here, '..', 'dist');

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

const shot = async (page) => decode(await canvasShot(page));

test('Space starts playback (the frame animates) then Space pauses it', async () => {
  const { page, errors } = await bootPage(browser, baseURL);
  const paused = await shot(page);

  await page.keyboard.press('Space');
  await page.waitForTimeout(1500);
  const playing = await shot(page);
  const moved = frameDiff(paused, playing);
  console.log(`play frameDiff ${(100 * moved).toFixed(2)}%`);
  assert.ok(moved > 0.005, `playback should animate the frame, diff was ${(100 * moved).toFixed(2)}%`);

  const x0 = findPlayheadX(paused);
  const x1 = findPlayheadX(playing);
  console.log(`playhead x ${x0} -> ${x1}`);
  assert.ok(x0 >= 0 && x1 > x0, `the playhead should advance rightward (was ${x0}, now ${x1})`);

  await page.keyboard.press('Space'); // pause
  await page.waitForTimeout(400);
  const a = await shot(page);
  await page.waitForTimeout(700);
  const b = await shot(page);
  const drift = frameDiff(a, b);
  console.log(`paused frameDiff ${(100 * drift).toFixed(3)}%`);
  assert.ok(drift < 0.002, `a paused cockpit should hold still, drift was ${(100 * drift).toFixed(3)}%`);

  assert.deepEqual(errors, []);
  await page.close();
});

/** Press `key` `times` and assert the rendered frame changed by > minDiff. */
async function changesFrame(name, key, { times = 1, minDiff = 0.01 } = {}) {
  const { page, errors } = await bootPage(browser, baseURL);
  const before = await shot(page);
  for (let i = 0; i < times; i += 1) await page.keyboard.press(key);
  await page.waitForTimeout(500);
  const after = await shot(page);
  const d = frameDiff(before, after);
  console.log(`${name} (${key} ×${times}) frameDiff ${(100 * d).toFixed(2)}%`);
  assert.ok(d > minDiff, `${name} should change the frame, diff was ${(100 * d).toFixed(2)}%`);
  assert.deepEqual(errors, []);
  await page.close();
}

test('ArrowRight scrolls the view', () => changesFrame('scroll', 'ArrowRight', { times: 4 }));
test('] jumps to the next section', () => changesFrame('next-section', 'BracketRight'));
test('= zooms in', () => changesFrame('zoom-in', 'Equal', { times: 2 }));

test('an unmapped key leaves the frame unchanged', async () => {
  const { page, errors } = await bootPage(browser, baseURL);
  const before = await shot(page);
  await page.keyboard.press('KeyZ'); // not in the cockpit keymap
  await page.waitForTimeout(500);
  const after = await shot(page);
  const d = frameDiff(before, after);
  console.log(`inert (z) frameDiff ${(100 * d).toFixed(3)}%`);
  assert.ok(d < 0.002, `an unmapped key must be inert, diff was ${(100 * d).toFixed(3)}%`);
  assert.deepEqual(errors, []);
  await page.close();
});
