// Pure, DOM-free helpers for the split-phrase pager (see app.js). Kept apart so
// the jump/compare logic is unit-tested under `node --test` (web/test/pager.test.js)
// without a browser or the wasm engine.

// Options for the jump/compare dropdowns: one per phrase, 1-based labels.
export function phraseOptions(count) {
  return Array.from({ length: Math.max(0, count | 0) },
    (_, i) => ({ value: i, label: `phrase ${i + 1} / ${count}` }));
}

// The role-tagged tracks to render for the current phrase, optionally overlaying
// a second one for comparison: current as A (blue), the compared phrase as B
// (amber). A missing current phrase yields []; a -1, out-of-range, or self
// compareIdx yields just A (no overlay). draw()/play() split notes by role.
export function compareTracks(chunks, idx, compareIdx) {
  const cur = chunks[idx];
  if (!cur) return [];
  const tracks = [{ name: cur.title, role: 'a', notes: cur.notes || [] }];
  const cmp = (compareIdx >= 0 && compareIdx !== idx) ? chunks[compareIdx] : null;
  if (cmp) tracks.push({ name: cmp.title, role: 'b', notes: cmp.notes || [] });
  return tracks;
}
