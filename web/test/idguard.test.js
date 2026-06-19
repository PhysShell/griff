// Unit tests for the chunk-id ↔ source-file guard (web/static/idguard.js).
// Pure logic, no DOM/wasm — runs under `node --test` (see web/package.json).
// Imports a module that does not exist yet, so this fails until the green step.
import { test } from 'node:test';
import assert from 'node:assert/strict';

import {
  slugFromFilename, defaultChunkId, chunkIdInitials, idMatchesFile,
} from '../static/idguard.js';

test('slugFromFilename strips the extension and slugifies the stem', () => {
  assert.equal(slugFromFilename('speeddemon.mid'), 'speeddemon');
  assert.equal(slugFromFilename('dgd.gpx'), 'dgd');
  assert.equal(slugFromFilename('Dance Gavin Dance.gpx'), 'dance_gavin_dance');
  assert.equal(slugFromFilename('foo.bar.mid'), 'foo_bar');
  assert.equal(slugFromFilename(''), '');
});

test('defaultChunkId is the filename slug', () => {
  assert.equal(defaultChunkId('dgd.gpx'), 'dgd');
  assert.equal(defaultChunkId('Speed Demon.mid'), 'speed_demon');
});

test('chunkIdInitials takes the first char of each segment', () => {
  assert.equal(chunkIdInitials('dance_gavin_dance'), 'dgd');
  assert.equal(chunkIdInitials('dgd'), 'd');
  assert.equal(chunkIdInitials(''), '');
});

test('idMatchesFile accepts ids matching the file stem or its initials', () => {
  assert.ok(idMatchesFile('dgd_001', 'dgd.gpx'));               // same stem
  assert.ok(idMatchesFile('dgd_001', 'Dance Gavin Dance.gpx')); // id = file initials
  assert.ok(idMatchesFile('dance_gavin_dance_001', 'dgd.gpx')); // file = id initials
  assert.ok(idMatchesFile('', 'dgd.gpx'));                      // empty id → no opinion
  assert.ok(idMatchesFile('dgd_001', ''));                     // unknown file → no opinion
});

test('idMatchesFile flags a stale id left over from another song', () => {
  assert.equal(idMatchesFile('speeddemon_001', 'dgd.gpx'), false);
  assert.equal(idMatchesFile('speeddemon_001', 'Dance Gavin Dance.gpx'), false);
});
