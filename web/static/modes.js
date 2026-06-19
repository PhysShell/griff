// Pure, DOM-free helpers for the arrange-mode controls (see app.js). Kept apart
// so the error-message mapping and offset math are unit-tested under `node --test`
// (web/test/modes.test.js) without a browser or the wasm engine.

// Mode indices, matching the <select id="mode"> options in index.html.
export const MODE_NAMES = ['rhythm_lock', 'register_contrast', 'call_response',
  'support_layer', 'octave_double', 'counter_melody'];

export const OCTAVE_DOUBLE = 4;

// Modes that need only notes to apply — used as load-time fallbacks when the
// selected mode can't (e.g. counter_melody on a meter-changing song).
export const SAFE_FALLBACK_MODES = [0, 3]; // rhythm_lock, support_layer

// Maps a raw engine error (e.g. "Arrange(InvalidSpec(OctaveDouble))") to a short,
// actionable message for the status line. Unknown errors fall through verbatim so
// nothing is hidden.
export function friendlyArrangeError(raw) {
  const e = String(raw);
  if (e.includes('OctaveDouble')) {
    return 'octave_double needs a whole-octave Register offset (±12 or ±24).';
  }
  if (e.includes('RegisterContrast')) {
    return 'register_contrast needs a Register offset big enough to clear the part’s range — try ±12 or more.';
  }
  if (e.includes('NonUniformTimeline')) {
    return 'counter_melody needs one steady time signature; this song changes meter — try rhythm_lock or support_layer.';
  }
  if (e.includes('NoGapsToAnswer')) {
    return 'call_response needs a rest of at least a quarter note to answer into; this part has none.';
  }
  if (e.includes('NoNotes') || e.includes('Empty')) {
    return 'this track has no notes to arrange — pick another track.';
  }
  return `couldn’t generate with this mode/offset (${e}).`;
}

// octave_double only accepts a non-zero whole-octave offset; snap an arbitrary
// slider value to the nearest valid one, keeping the user's direction, clamped to
// the ±max slider range.
export function snapOctaveOffset(value, max = 24) {
  let oct = Math.round(Number(value) / 12) * 12;
  if (oct === 0) oct = Number(value) < 0 ? -12 : 12;
  return Math.max(-max, Math.min(max, oct));
}
