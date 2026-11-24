# Studio: mark "sounds wrong here" on the timeline (specification)

Base: branch `claude/studio-key` (PR #225). Both front ends (`studio/index.html`, Studio Web) share `studio/engine.py`, `studio/web_api.py` and `composer.py`.

## Problem (from the owner)

Some pieces have places where 2–3 notes sound like no guitarist would play them: an odd rest, or a note that falls out of the rhythm. The owner cannot always explain it. Writing down seconds by hand is tedious. The owner wants to mark the place on the timeline while listening. We then read the marks, inspect what the generator did there, and fix the rules. Later the same marks feed a preference model and bar-level editing.

## Required behaviour

1. **Making a mark.** While a piece is playing or paused, the owner can add a mark in three ways:
   - the button «Тут не так»;
   - the key `M`, when focus is not in a text field;
   - a click on the timeline with a modifier: Shift+click, or a long press on touch.

   A plain click on the timeline keeps today's meaning: seek. The mark takes the current playback position, or the clicked position.
2. **Mark contents.** Stored with the piece:
   - an `id`;
   - the time in seconds;
   - the 1-based bar number and the sixteenth inside the bar, computed from the piece's tempo and meters;
   - the section label;
   - the guitar: `lead` / `rhythm` / `both`, default `both`;
   - an optional short text, up to 80 characters;
   - the `notes` of both guitars sounding or starting within ±1 sixteenth: pitch, start, duration, string, fret, tech;
   - the recipe revision it was made on.
3. **Mark editor.** After a mark is added, a small inline editor (not a modal) lets the owner set the guitar and the text, or delete the mark. Enter or "Готово" closes it; Esc deletes a mark that was just added.
4. **Display.**
   - Marks appear as small flags on the timeline at their time.
   - Clicking a flag seeks to 1 bar before it and opens its editor.
   - A list «Отметки (N)» under the timeline shows bar:sixteenth, the guitar and the text. Clicking a row seeks the same way.
   - Marks made on an older revision show «(до изменения)» and are greyed.
5. **Auditioning a mark.** "Послушать место" on a flag or row plays from 1 bar before to 1 bar after the mark, then stops.
6. **Persistence.** Marks are stored in the piece state:
   - server: `marks.json` in the piece folder, path-checked like the other files;
   - Web: the piece record in IndexedDB, included in export and import.

   Adding, editing or deleting a mark does not change the music. It does not push undo or redo and does not clear redo. Each such action is logged to `history.jsonl` as `mark_add` / `mark_edit` / `mark_delete` with the full mark. `studio.summary` lists the mark count per piece.
7. **Export for analysis.** A button «Скачать отметки» downloads `marks.json` for the current piece, or for all pieces from the sidebar. Each file has the seed, the recipe and the marks.
8. **Russian UI text.** The hint under the list reads: «Отметьте место, где звучит не по-человечески: кнопка «Тут не так», клавиша M или Shift+клик по таймлайну. Музыку это не меняет.»

## Tests (write first)

- **Bar and sixteenth.** Bar and sixteenth computed from time are correct for 4/4, 7/8, and 7/8 → 5/4 → 7/8, including the intro.
- **Notes snapshot.** The captured notes for a mark equal the composed notes within ±1 sixteenth.
- **Music unchanged.** Adding, editing and deleting marks leaves the MIDI byte-identical, does not change the undo or redo stacks, and logs the three actions.
- **Round trip.** Server save/load and Web export/import round-trip the marks. Legacy pieces without marks load fine.
- **Stale marks.** A mark made before a reroll is flagged stale after it.
- **Path safety.** The server path check covers `marks.json`.
- **Parity.** Server and web behave the same.
- **Page tests.**
  - `M` adds a mark at the playback position.
  - Shift+click adds a mark; a plain click only seeks.
  - Esc removes a new mark.
  - "Послушать место" sets the start and stop correctly.
