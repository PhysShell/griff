// Pure, DOM-free helpers for the per-phrase fact-sheet in the split pager
// (see app.js). Kept apart so the stats logic is unit-tested under `node --test`
// (web/test/phrasestats.test.js) without a browser or the wasm engine.
//
// The fact-sheet reports a phrase's intrinsic, transferable characteristics —
// register, density, dynamics — rather than a song-form label (verse/chorus),
// which is bound to the source song and lost once a phrase enters the corpus
// (#75). Everything is derived from the split's `{p,s,d,v}` notes.

const PITCH_CLASSES = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];

// Velocity range at or below which a phrase reads as flat (a hand for the
// `flat_dynamics` quality flag). Coarse on purpose, like the capture triage.
const FLAT_VELOCITY_RANGE = 8;

// Scientific pitch name for a MIDI note, e.g. 40 → "E2", 69 → "A4", 61 → "C#4".
// MIDI 69 is A4 (A440), so octave = floor(midi / 12) - 1.
export function noteName(midi) {
  const m = Math.round(midi);
  const pc = ((m % 12) + 12) % 12;
  const octave = Math.floor(m / 12) - 1;
  return `${PITCH_CLASSES[pc]}${octave}`;
}

// Summarises a phrase's `{p,s,d,v}` notes over `bars` bars: note count, register
// (low/high MIDI + semitone span), notes-per-bar density (one decimal, null when
// the bar span is unknown), and velocity range + a flat-dynamics flag. Returns
// null for an empty or missing phrase.
export function phraseStats(notes, bars) {
  if (!Array.isArray(notes) || notes.length === 0) return null;
  let low = Infinity;
  let high = -Infinity;
  let velLo = Infinity;
  let velHi = -Infinity;
  for (const n of notes) {
    if (n.p < low) low = n.p;
    if (n.p > high) high = n.p;
    if (n.v < velLo) velLo = n.v;
    if (n.v > velHi) velHi = n.v;
  }
  const perBar = bars > 0 ? Math.round((notes.length / bars) * 10) / 10 : null;
  return {
    count: notes.length,
    low,
    high,
    span: high - low,
    perBar,
    velLo,
    velHi,
    flat: velHi - velLo <= FLAT_VELOCITY_RANGE,
  };
}

// Renders a phraseStats object as a compact one-liner for the pager, e.g.
// "E2–A4 · 17 st · 6.4 notes/bar · vel 64–120". Empty string for null input.
export function formatStats(st) {
  if (!st) return '';
  const parts = [`${noteName(st.low)}–${noteName(st.high)}`, `${st.span} st`];
  if (st.perBar != null) {
    parts.push(`${st.perBar} note${st.perBar === 1 ? '' : 's'}/bar`);
  }
  parts.push(`vel ${st.velLo}–${st.velHi}${st.flat ? ' (flat)' : ''}`);
  return parts.join(' · ');
}
