// Unit tests for the arrange-mode helpers (web/static/modes.js).
// Pure logic, no DOM/wasm — runs under `node --test` (see web/package.json).
import { test } from 'node:test';
import assert from 'node:assert/strict';

import {
  MODE_NAMES, SAFE_FALLBACK_MODES, friendlyArrangeError, snapOctaveOffset,
} from '../static/modes.js';

test('friendlyArrangeError maps known engine errors to actionable guidance', () => {
  assert.match(friendlyArrangeError('Arrange(InvalidSpec(OctaveDouble))'), /whole-octave/);
  assert.match(friendlyArrangeError('Arrange(InvalidSpec(RegisterContrast))'), /clear the part/);
  assert.match(friendlyArrangeError('Arrange(NonUniformTimeline)'), /changes meter/);
  assert.match(friendlyArrangeError('Arrange(NoGapsToAnswer)'), /call_response/);
});

test('friendlyArrangeError passes unknown errors through verbatim', () => {
  assert.match(friendlyArrangeError('Arrange(Weird(Thing))'), /Weird\(Thing\)/);
});

test('snapOctaveOffset snaps to the nearest non-zero whole octave', () => {
  assert.equal(snapOctaveOffset(10), 12);
  assert.equal(snapOctaveOffset(5), 12);
  assert.equal(snapOctaveOffset(0), 12); // 0 is invalid → default up an octave
  assert.equal(snapOctaveOffset(-5), -12); // keep the downward direction
  assert.equal(snapOctaveOffset(-10), -12);
});

test('snapOctaveOffset clamps to the slider range', () => {
  assert.equal(snapOctaveOffset(18), 24);
  assert.equal(snapOctaveOffset(100), 24);
  assert.equal(snapOctaveOffset(-100), -24);
});

test('mode metadata matches the UI ordering', () => {
  assert.equal(MODE_NAMES[4], 'octave_double');
  assert.equal(MODE_NAMES[5], 'counter_melody');
  assert.deepEqual(SAFE_FALLBACK_MODES.map((i) => MODE_NAMES[i]), ['rhythm_lock', 'support_layer']);
});
