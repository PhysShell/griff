// Unit tests for the split-pager helpers (web/static/pager.js).
// Pure logic, no DOM/wasm — runs under `node --test` (see web/package.json).
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { compareTracks, phraseOptions } from '../static/pager.js';

const chunks = [
  { title: 'p0', notes: [1, 2, 3] },
  { title: 'p1', notes: [4] },
  { title: 'p2', notes: [] },
];

test('phraseOptions builds one 1-based option per phrase', () => {
  assert.deepEqual(phraseOptions(2), [
    { value: 0, label: 'phrase 1 / 2' },
    { value: 1, label: 'phrase 2 / 2' },
  ]);
  assert.deepEqual(phraseOptions(0), []);
});

test('compareTracks returns only the current phrase (role a) when not comparing', () => {
  const tr = compareTracks(chunks, 0, -1);
  assert.equal(tr.length, 1);
  assert.equal(tr[0].role, 'a');
  assert.deepEqual(tr[0].notes, [1, 2, 3]);
});

test('compareTracks overlays the compared phrase as role b', () => {
  const tr = compareTracks(chunks, 0, 1);
  assert.equal(tr.length, 2);
  assert.deepEqual(tr.map((t) => t.role), ['a', 'b']);
  assert.deepEqual(tr[1].notes, [4]);
});

test('compareTracks ignores a self-compare', () => {
  assert.equal(compareTracks(chunks, 1, 1).length, 1);
});

test('compareTracks tolerates out-of-range / missing indices', () => {
  assert.deepEqual(compareTracks(chunks, 9, 0), []); // no current phrase
  assert.equal(compareTracks(chunks, 0, 9).length, 1); // compare phrase missing → no overlay
});
