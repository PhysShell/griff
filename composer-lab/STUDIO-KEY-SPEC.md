# Studio: change the key of a piece (specification)

Base: `main` at 552831d (after #222 and #223). Both front ends
(`studio/index.html`, Studio Web) share `studio/engine.py`, `studio/web_api.py`
and `composer.py`.

## Problem (from the owner)

The owner wants to move a finished piece to another key and mode, e.g. from A major
to D minor, keeping everything else: the same notes in the same places, each
note moved to the same scale degree of the new scale.

Today a recipe can be recomposed with another `intent.tonic` / `intent.mode`,
but that is not "everything else unchanged". On seed 802000, A aeolian to
E aeolian keeps the onsets but changes durations (4 -> 2, 8 -> 2) and legato,
and the register jumps unevenly (77 -> 84 but 76 -> 71) because octave choice
and fingering are re-derived per note. In 19 of 24 probes the note set
changed by 0-12 notes per piece.

## Required behaviour

New whole-piece option `key`, stored in `studio.options`, default unset
(today's output byte for byte). Value: `{"tonic": one of TONICS,
"mode": one of ionian, dorian, phrygian, lydian, mixolydian, aeolian,
locrian}`. It applies at once, is undoable/redoable and logged like other
options; locks are unaffected (same rule as the other piece parameters).

1. Every note keeps its onset, duration, guitar, velocity and articulation
   intent (dead, palm mute, accent). Its pitch moves to the same scale degree
   in the new scale. Chromatic notes (not in the old scale) keep their
   offset from the nearest lower scale degree.
2. Register: the whole piece shifts by one common octave offset chosen so that
   all pitches fit the guitar range (40-88); within a phrase the melodic
   contour (up/down/same between consecutive notes of one guitar) is
   preserved. No per-note octave folding.
3. Fingering is recomputed for the new pitches with the existing fretboard
   pass; legato/slide/tap are kept where they remain playable under the
   existing rules (same string, slide_max, taps above the fretting hand) and
   dropped otherwise.
4. After mapping, the existing minor-ninth rule between the guitars holds; if
   a mapped note creates a minor ninth, apply the existing resolution and
   report how often that happens in the tests.
5. UI: "Тональность" in "Параметры пьесы" — a tonic choice (E, F#, G, A, B,
   C#, D, as available in TONICS; add C and F only if trivially supported)
   and a mode choice with Russian names (мажор, натуральный минор,
   дорийский, фригийский, лидийский, миксолидийский, локрийский), plus
   "Как в пьесе" to reset. Hint: "Переводит все ноты в выбранную тональность
   по ступеням; ритм и форма не меняются. Применяется сразу." The piece header
   shows the current key in Russian.

## Tests (write first)

- Unset key: byte-identical MIDI for the frozen seeds and 20 random seeds.
- For 30 seeds and several target keys: onsets, durations, guitar assignment
  identical to the source; each pitch is the mapped scale degree; contour
  preserved per guitar; all pitches in 40-88; ComposerInvariants hold;
  MusicXML measures fill their meter.
- Mapping to the piece's own key is the identity.
- A -> D minor -> back to the original key restores the original pitches
  (round trip), when no note was clipped by range.
- Undo/redo and web_api parity as for the other options.
