// Unit tests for the per-phrase stats helpers (web/static/phrasestats.js).
// Pure logic, no DOM/wasm — runs under `node --test` (see web/package.json).
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { noteName, phraseStats, formatStats } from '../static/phrasestats.js';

test('noteName renders scientific pitch names', () => {
  assert.equal(noteName(60), 'C4');  // middle C
  assert.equal(noteName(69), 'A4');  // A440
  assert.equal(noteName(40), 'E2');  // low-E string
  assert.equal(noteName(61), 'C#4'); // a sharp
});

test('phraseStats summarises register, density and dynamics', () => {
  const notes = [
    { p: 40, s: 0, d: 240, v: 80 },
    { p: 52, s: 240, d: 240, v: 120 },
    { p: 47, s: 480, d: 240, v: 64 },
    { p: 57, s: 720, d: 240, v: 100 },
  ];
  const st = phraseStats(notes, 2);
  assert.equal(st.count, 4);
  assert.equal(st.low, 40);
  assert.equal(st.high, 57);
  assert.equal(st.span, 17);
  assert.equal(st.perBar, 2); // 4 notes / 2 bars
  assert.equal(st.velLo, 64);
  assert.equal(st.velHi, 120);
  assert.equal(st.flat, false);
});

test('phraseStats flags flat dynamics and rounds density to one decimal', () => {
  const notes = [
    { p: 50, s: 0, d: 100, v: 100 },
    { p: 50, s: 100, d: 100, v: 103 },
    { p: 50, s: 200, d: 100, v: 101 },
  ];
  const st = phraseStats(notes, 4);
  assert.equal(st.span, 0);
  assert.equal(st.perBar, 0.8); // 3 / 4 = 0.75 → 0.8
  assert.equal(st.flat, true); // velocity range 3 ≤ threshold
});

test('phraseStats returns null for an empty or missing phrase', () => {
  assert.equal(phraseStats([], 4), null);
  assert.equal(phraseStats(undefined, 4), null);
});

test('phraseStats guards a zero-bar span (no divide-by-zero)', () => {
  const st = phraseStats([{ p: 60, s: 0, d: 10, v: 90 }], 0);
  assert.equal(st.perBar, null); // unknown, not Infinity
});

test('formatStats renders a compact one-liner', () => {
  const notes = [
    { p: 40, s: 0, d: 240, v: 64 },
    { p: 57, s: 240, d: 240, v: 120 },
  ];
  assert.equal(
    formatStats(phraseStats(notes, 2)),
    'E2–A4 · 17 st · 1 note/bar · vel 64–120',
  );
});

test('formatStats marks flat dynamics and handles empty input', () => {
  const flat = phraseStats(
    [
      { p: 50, s: 0, d: 1, v: 100 },
      { p: 50, s: 1, d: 1, v: 101 },
    ],
    1,
  );
  assert.equal(formatStats(flat), 'D3–D3 · 0 st · 2 notes/bar · vel 100–101 (flat)');
  assert.equal(formatStats(null), '');
});
