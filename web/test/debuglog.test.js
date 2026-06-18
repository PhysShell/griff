// Unit tests for the playground's debug-log ring buffer (web/static/debuglog.js).
// Pure logic, no DOM/wasm — runs under `node --test` (see web/package.json).
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { createDebugLog } from '../static/debuglog.js';

// Fixed clock so the "HH:MM:SS" prefix is deterministic. Both the constructor
// and toTimeString() are local time, so this is stable regardless of TZ.
const at = (h, m, s) => () => new Date(2026, 0, 1, h, m, s);

test('push renders a timestamped label with JSON-stringified data', () => {
  const log = createDebugLog({ now: at(9, 8, 7) });
  log.push('loaded', { tracks: 3, bars: 48 });
  assert.equal(log.text(), '09:08:07  loaded {"tracks":3,"bars":48}');
});

test('push without data omits the body', () => {
  const log = createDebugLog({ now: at(1, 2, 3) });
  log.push('engine ready');
  assert.equal(log.text(), '01:02:03  engine ready');
});

test('string data is appended verbatim, not re-quoted', () => {
  const log = createDebugLog({ now: at(0, 0, 0) });
  log.push('detect failed', 'no track selected');
  assert.equal(log.text(), '00:00:00  detect failed no track selected');
});

test('err prefixes the label with the error marker', () => {
  const log = createDebugLog({ now: at(0, 0, 0) });
  log.err('split failed', 'bad JSON');
  assert.equal(log.text(), '00:00:00  ✗ split failed bad JSON');
});

test('the buffer is bounded to max, dropping the oldest lines', () => {
  const log = createDebugLog({ max: 3, now: at(0, 0, 0) });
  for (let i = 0; i < 5; i += 1) log.push(`e${i}`);
  assert.equal(log.length, 3);
  assert.deepEqual(log.lines().map((l) => l.split('  ')[1]), ['e2', 'e3', 'e4']);
});

test('rejects a non-positive max so the ring stays bounded', () => {
  assert.throws(() => createDebugLog({ max: 0 }), RangeError);
  assert.throws(() => createDebugLog({ max: -5 }), RangeError);
});

test('unserializable data is flagged, not thrown', () => {
  const log = createDebugLog({ now: at(0, 0, 0) });
  const circular = {};
  circular.self = circular;
  assert.doesNotThrow(() => log.push('captured', circular));
  assert.equal(log.text(), '00:00:00  captured [unserializable]');
});

test('clear empties the buffer', () => {
  const log = createDebugLog({ now: at(0, 0, 0) });
  log.push('a');
  log.push('b');
  log.clear();
  assert.equal(log.text(), '');
  assert.equal(log.length, 0);
});
