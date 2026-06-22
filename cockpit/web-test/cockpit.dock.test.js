// Corpus dock (ADR-0027 Slice 5): the 📚 Corpus button reads the OPFS corpus and
// opens the in-canvas dock (browse / filter / dashboard over the captured chunks,
// computed by the shared griff_ui_core::dock). We capture a chunk first so the
// corpus is non-empty, then assert the dock window paints over the roll.
import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { access } from 'node:fs/promises';

import { chromium } from 'playwright';

import { startServer } from './serve.js';
import { LAUNCH_ARGS, bootPage, canvasShot, decode, frameDiff } from './helpers.js';

const here = dirname(fileURLToPath(import.meta.url));
const dist = join(here, '..', 'dist');
const multiTrack = join(here, '..', 'assets', 'multi_track.mid');

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

test('Corpus opens the dock over the roll', async () => {
  const { page, errors } = await bootPage(browser, baseURL);

  // Start clean, then capture one chunk so the OPFS corpus is non-empty.
  await page.evaluate(async () => {
    try {
      const root = await navigator.storage.getDirectory();
      await root.removeEntry('corpus', { recursive: true });
    } catch (_) {
      /* no corpus yet */
    }
  });
  await page.setInputFiles('#file', multiTrack);
  await page.waitForTimeout(1200);
  await Promise.all([page.waitForEvent('download'), page.click('#capture')]);

  // Wait for the async OPFS persist to land before reading the corpus back.
  let persisted = false;
  for (let i = 0; i < 30; i += 1) {
    persisted = await page.evaluate(async () => {
      try {
        const corpus = await (await navigator.storage.getDirectory()).getDirectoryHandle('corpus');
        await corpus.getFileHandle('multi_track.chunk.json');
        return true;
      } catch (_) {
        return false;
      }
    });
    if (persisted) break;
    await page.waitForTimeout(200);
  }
  assert.ok(persisted, 'the captured chunk should persist to OPFS before opening the dock');

  const before = decode(await canvasShot(page));
  await page.click('#corpus'); // read OPFS → load_corpus → dock opens next frame
  await page.waitForTimeout(800);
  const after = decode(await canvasShot(page));

  const d = frameDiff(before, after);
  console.log(`dock frameDiff ${(100 * d).toFixed(1)}%`);
  assert.ok(d > 0.01, `the corpus dock should appear, diff was ${(100 * d).toFixed(1)}%`);
  assert.deepEqual(errors, [], `opening the dock must not error:\n${errors.join('\n')}`);
  await page.close();
});
