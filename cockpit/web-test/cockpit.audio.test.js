// Playback audio (ADR-0024 §4): pressing play sounds the focused track through a
// WebAudio oscillator synth, in-wasm (no JS). Headless Chromium has no speakers,
// so we spy the WebAudio API — `AudioContext.prototype.createOscillator` and the
// constructor — to prove the Rust audio path actually schedules a note as the
// playhead crosses one, and that nothing is scheduled until the user hits play.
import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { access } from 'node:fs/promises';

import { chromium } from 'playwright';

import { startServer } from './serve.js';
import { LAUNCH_ARGS } from './helpers.js';

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
  // Let the AudioContext start without a real gesture (headless has none), so a
  // blocked-autoplay warning never masquerades as the synth failing.
  browser = await chromium.launch({
    args: [...LAUNCH_ARGS, '--autoplay-policy=no-user-gesture-required'],
  });
});

after(async () => {
  await browser?.close();
  await server?.close();
});

test('pressing play sounds the focused track through WebAudio', async () => {
  const page = await browser.newPage({
    viewport: { width: 1100, height: 520 },
    deviceScaleFactor: 2,
  });
  const errors = [];
  page.on('pageerror', (e) => errors.push(`pageerror: ${e.message}`));
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(`console.error: ${m.text()}`);
  });

  // Spy the WebAudio API *before* the wasm glue runs. Patching the prototype
  // catches every `createOscillator`, however the glue reaches the context; the
  // constructor wrap counts contexts (best-effort, logged only).
  await page.addInitScript(() => {
    window.__audio = { contexts: 0, oscillators: 0 };
    const Real = window.AudioContext;
    if (!Real) return;
    const realCreate = Real.prototype.createOscillator;
    Real.prototype.createOscillator = function createOscillatorSpy(...args) {
      window.__audio.oscillators += 1;
      return realCreate.apply(this, args);
    };
    const Wrapped = function AudioContextSpy(...args) {
      window.__audio.contexts += 1;
      return new Real(...args);
    };
    Wrapped.prototype = Real.prototype;
    window.AudioContext = Wrapped;
  });

  await page.goto(baseURL, { waitUntil: 'load' });
  await page.waitForFunction(() => !document.getElementById('loading'), { timeout: 30000 });
  await page.waitForTimeout(2500); // initial paint
  await page.locator('canvas').click(); // focus the canvas so eframe gets keys

  // A paused cockpit makes no sound: the synth is gated on playback.
  const idle = await page.evaluate(() => window.__audio);
  assert.equal(idle.oscillators, 0, 'no oscillators before play');

  await page.keyboard.press('Space'); // start playback
  await page.waitForTimeout(2500); // the playhead crosses the focused track's notes

  const sounding = await page.evaluate(() => window.__audio);
  console.log(`audio: ${sounding.contexts} context(s), ${sounding.oscillators} oscillator(s)`);
  assert.ok(
    sounding.oscillators > 0,
    `crossing notes should schedule oscillators, saw ${sounding.oscillators}`,
  );

  assert.deepEqual(errors, [], `audio playback must not error:\n${errors.join('\n')}`);
  await page.close();
});
