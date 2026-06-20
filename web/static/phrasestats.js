// Pure, DOM-free helpers for the per-phrase fact-sheet in the split pager
// (see app.js). Kept apart so the stats logic is unit-tested under `node --test`
// (web/test/phrasestats.test.js) without a browser or the wasm engine.

// RED stub — see web/test/phrasestats.test.js. Implemented in the green commit.
export function noteName() {
  return '';
}

export function phraseStats() {
  return null;
}

export function formatStats() {
  return '';
}
