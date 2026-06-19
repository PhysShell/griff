// Pure, DOM-free helpers to name a guitar tuning from its open-string MIDI
// numbers (see app.js). Unit-tested under `node --test` (web/test/tuning.test.js)
// without a browser or the wasm engine.

const PITCH_CLASSES = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];

// MIDI note number → pitch-class name (no octave), e.g. 64 → "E", 38 → "D".
export function noteName(midi) {
  return PITCH_CLASSES[((Math.round(Number(midi)) % 12) + 12) % 12];
}

// Common tunings keyed by their low→high pitch-class sequence (sharp spelling,
// matching noteName's output).
const KNOWN = [
  ['E A D G B E', 'Standard E', 'standard_e'],
  ['D A D G B E', 'Drop D', 'drop_d'],
  ['D# G# C# F# A# D#', 'Eb Standard', 'eb_standard'],
  ['D G C F A D', 'D Standard', 'd_standard'],
  ['C# G# C# F# A# D#', 'Drop C#', 'drop_csharp'],
  ['C G C F A D', 'Drop C', 'drop_c'],
  ['D A D G A D', 'DADGAD', 'dadgad'],
  ['B E A D G B E', 'Standard B (7-string)', 'standard_b7'],
];

// Describes a tuning from its open-string MIDI numbers (low→high):
//   { label: human name, notes: "E A D G B E", slug: chunk-friendly id }.
// Unknown tunings get a generic label and a note-derived slug; empty/invalid
// input yields a neutral "unknown".
export function describeTuning(open) {
  const arr = Array.isArray(open) ? open.map(Number).filter(Number.isFinite) : [];
  if (arr.length === 0) return { label: 'unknown', notes: '', slug: '' };
  const notes = arr.map(noteName).join(' ');
  const hit = KNOWN.find(([seq]) => seq === notes);
  if (hit) return { label: hit[1], notes, slug: hit[2] };
  const slug = notes.toLowerCase().replace(/ /g, '_').replace(/#/g, 's');
  return { label: `${noteName(arr[0])} (${arr.length}-string)`, notes, slug };
}
