// Capture flow (ADR-0027 Slice 3b): toggling the inspector shows the capture
// panel, and the Capture button downloads a real chunk.json for the loaded
// score — built through the shared griff_ui_core::capture::build_chunk, so the
// bytes match what `griff manifest` reads.
import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { access, readFile } from 'node:fs/promises';

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

test('toggling the inspector shows the capture panel', async () => {
  const { page, errors } = await bootPage(browser, baseURL);
  const before = decode(await canvasShot(page));
  await page.keyboard.press('KeyI');
  await page.waitForTimeout(500);
  const after = decode(await canvasShot(page));
  const d = frameDiff(before, after);
  console.log(`inspector frameDiff ${(100 * d).toFixed(1)}%`);
  assert.ok(d > 0.01, `the capture panel should appear, diff was ${(100 * d).toFixed(1)}%`);
  assert.deepEqual(errors, []);
  await page.close();
});

test('Capture downloads a chunk.json for the loaded score', async () => {
  const { page, errors } = await bootPage(browser, baseURL);
  await page.setInputFiles('#file', multiTrack);
  await page.waitForTimeout(1200); // load the score and seed the form

  const [download] = await Promise.all([
    page.waitForEvent('download'),
    page.click('#capture'),
  ]);
  const name = download.suggestedFilename();
  console.log(`downloaded ${name}`);
  assert.match(name, /\.chunk\.json$/, 'downloads a chunk.json');
  assert.match(name, /multi_track/, 'named from the loaded file');

  const chunk = JSON.parse(await readFile(await download.path(), 'utf8'));
  assert.equal(chunk.id, 'multi_track', 'the chunk id is the seeded slug');
  assert.ok(chunk.rights, 'rights are recorded');
  assert.ok(Array.isArray(chunk.boundaries), 'boundaries are measured');
  assert.deepEqual(errors, [], `capture must not error:\n${errors.join('\n')}`);
  await page.close();
});

test('Capture persists the chunk to the OPFS corpus', async () => {
  const { page, errors } = await bootPage(browser, baseURL);
  // Start from a clean corpus so we verify *this* capture wrote the file.
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

  // The OPFS write is async (fire-and-forget); poll the corpus tree for the file.
  let content = null;
  for (let i = 0; i < 30 && !content; i += 1) {
    content = await page.evaluate(async () => {
      try {
        const root = await navigator.storage.getDirectory();
        const corpus = await root.getDirectoryHandle('corpus');
        const fh = await corpus.getFileHandle('multi_track.chunk.json');
        return await (await fh.getFile()).text();
      } catch (_) {
        return null;
      }
    });
    if (!content) await page.waitForTimeout(200);
  }
  assert.ok(content, 'corpus/multi_track.chunk.json should appear in OPFS');
  const chunk = JSON.parse(content);
  assert.equal(chunk.id, 'multi_track', 'the OPFS corpus holds the captured chunk');
  assert.ok(chunk.rights, 'with its rights recorded');
  assert.deepEqual(errors, [], `OPFS persist must not error:\n${errors.join('\n')}`);
  await page.close();
});

test('Manifest folds the OPFS corpus into a CorpusManifest', async () => {
  const { page, errors } = await bootPage(browser, baseURL);
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
  // wait for the async OPFS persist to land before folding
  for (let i = 0; i < 30; i += 1) {
    const ready = await page.evaluate(async () => {
      try {
        const corpus = await (await navigator.storage.getDirectory()).getDirectoryHandle('corpus');
        await corpus.getFileHandle('multi_track.chunk.json');
        return true;
      } catch (_) {
        return false;
      }
    });
    if (ready) break;
    await page.waitForTimeout(200);
  }

  const [manifestDownload] = await Promise.all([
    page.waitForEvent('download'),
    page.click('#manifest'),
  ]);
  assert.equal(manifestDownload.suggestedFilename(), 'manifest.json');
  const manifest = JSON.parse(await readFile(await manifestDownload.path(), 'utf8'));
  assert.equal(manifest.schema_version, 9, 'a schema-v9 manifest');
  assert.ok(
    manifest.chunks.some((c) => c.id === 'multi_track'),
    'the captured chunk is folded into the manifest',
  );
  assert.deepEqual(errors, [], `manifest must not error:\n${errors.join('\n')}`);
  await page.close();
});
