# Studio controls that do what their labels say (specification)

Base: `main` (Studio and Studio Web as published). Applies to both front ends:
the server page (`studio/index.html`) and Studio Web (`web/index.html`,
`web/app.js`), which share `studio/engine.py`, `studio/web_api.py` and
`composer.py`.

## Problem (from the owner)

- He set "Глухие ноты (dead notes)" to 1 and expected the rhythm guitar to be
  muted; nothing changed without a re-roll and little changed with one.
  Today the value is the probability that a section whose rhythm guitar plays
  octave chords gets a dead stroke before each accent, applied only at the next
  re-roll of `comp` in A/A2, only if octave chords were drawn, and never in the
  sixteenth-note octave grid. At 1 a piece has 0-8 % dead notes.
- "Доля октавных секций" has the same indirect behaviour.
- The intro is often 1.5 s (one bar of 7/8 at a fast tempo); its length changes
  only when the intro layer is re-rolled. Verses are one 4-bar square.

## Required behaviour

All four controls act immediately on the current piece (recompose and render
on change, no re-roll needed), are stored in `studio.options`, are undoable,
and are logged like other options. Their defaults reproduce today's output
byte for byte (existing recipes and tests unchanged).

1. **Глухие ноты (доля ударов ритм-гитары)**, option `mute_share`, 0-1,
   default 0. After composition, each onset of the accompanying (non-lead)
   guitar in sections A, A2, A3 and B is muted with probability `mute_share`,
   chosen deterministically from the recipe seed and the onset time (so the
   same value gives the same result and undo restores it). A muted onset keeps
   its string and fret, gets technique `dead`, duration one sixteenth and
   velocity 40; all notes struck together at that onset are muted together.
   At 1 every accompaniment onset in those sections is dead; the lead is never
   touched. The existing octave dead strokes stay as they are.
2. **Октавные аккорды (доля тактов)**, option `octave_share`, 0-1 or unset.
   Unset (default) keeps today's behaviour. When set, each comping bar of A and
   A2 plays octave chords if a deterministic draw from the seed and the bar
   index is below the value, otherwise its current role. 1 = every comping bar
   of A and A2 in octave chords.
3. **Длина вступления (тактов)**, option `intro_bars`: "auto" (default, today's
   behaviour including the intro layer) or 1, 2, 4, 8. Extra bars cycle the A
   chords as the existing intro does.
4. **Повтор квадрата в куплетах**, option `square_repeat`: 1 (default) or 2.
   With 2, sections A and A2 are played twice in a row (the same four bars
   again), so a verse lasts eight bars.

Keep the old dose options working for existing saved pieces but hide them in
the UI behind "Дополнительно" with labels that state what they really do:
"Вероятность октав при перегенерации ритма", "Октавы шестнадцатыми (при
перегенерации)", "Глухие перед акцентами в октавах (при перегенерации)".

Each new control shows a one-line hint under it in Russian saying what it does
and that it applies at once.

## Constraints

- Python standard library; existing tests stay green; `test_composer.py` not
  edited; all existing version flags stay byte-identical (the version-hash
  script in the reviewer's notes).
- Section isolation rules of STUDIO-SPEC stay true: the four options are
  whole-piece settings, not layers; locks are unaffected by them.
- English code and comments, Russian UI text, minimal comments. Work only in
  `composer-lab/`. Do not commit or push.

## Tests (write first)

- Defaults: a piece with none of the new options set composes byte-identical
  MIDI to before for seeds 801, 802, 805, 807, 809, 810 and 20 random seeds.
- `mute_share=1`: every accompaniment onset in A, A2, A3, B is dead and the lead
  is unchanged; `mute_share=0.3`: share of muted onsets within 0.2-0.4 over 20
  pieces; same value twice gives identical MIDI; undo restores the previous MIDI.
- `octave_share=1`: every comping bar of A and A2 is an octave-chord bar;
  `octave_share=0`: none.
- `intro_bars=8`: the intro has 8 bars; `auto` equals today.
- `square_repeat=2`: A and A2 each have twice as many bars and the second half
  repeats the first note for note; total length grows accordingly.
- Studio Web: the same option sequence gives the same MIDI through `web_api`
  as through the engine; the Node harness covers that the controls send the
  option and the page refreshes without a re-roll.
- All `ComposerInvariants` hold for 20 seeds with random combinations of the
  four options.

## Deliverables

Code, tests, updated README sections, and a short report: what was built, test
commands and results, anything not done.
