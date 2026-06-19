// Unit tests for the tuning-naming helpers (web/static/tuning.js).
// Pure logic, no DOM/wasm — runs under `node --test` (see web/package.json).
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { describeTuning, noteName } from '../static/tuning.js';

test('noteName maps MIDI numbers to pitch classes', () => {
  assert.equal(noteName(60), 'C');
  assert.equal(noteName(64), 'E');
  assert.equal(noteName(38), 'D');
  assert.equal(noteName(39), 'D#');
});

test('describeTuning recognizes standard E', () => {
  const t = describeTuning([40, 45, 50, 55, 59, 64]); // E2 A2 D3 G3 B3 E4
  assert.equal(t.notes, 'E A D G B E');
  assert.equal(t.label, 'Standard E');
  assert.equal(t.slug, 'standard_e');
});

test('describeTuning recognizes drop D (lowest string E→D)', () => {
  const t = describeTuning([38, 45, 50, 55, 59, 64]);
  assert.equal(t.label, 'Drop D');
  assert.equal(t.slug, 'drop_d');
});

test('describeTuning falls back to a generic label + note-derived slug', () => {
  const t = describeTuning([41, 46, 51, 56, 60, 65]); // F A# D# G# C F (unknown)
  assert.equal(t.notes, 'F A# D# G# C F');
  assert.equal(t.label, 'F (6-string)');
  assert.equal(t.slug, 'f_as_ds_gs_c_f');
});

test('describeTuning handles empty or invalid input', () => {
  assert.deepEqual(describeTuning([]), { label: 'unknown', notes: '', slug: '' });
  assert.deepEqual(describeTuning(null), { label: 'unknown', notes: '', slug: '' });
});
