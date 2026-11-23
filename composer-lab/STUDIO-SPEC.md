# Studio: section-level curation tool for composer-lab (specification)

Status: specification for implementation. Owner of the product decision: the
project owner (a guitarist who will use the output as a starting point for his
own tracks and edits MIDI himself). Written 2026-11-24 after a five-model
review concluded that blind rating rounds have stopped producing direction
and that the owner should curate instead of rate.

## 1. Goal

A small local web app in which the owner opens a generated piece, listens to
it, and replaces only the parts he dislikes: re-roll one layer of one section
while everything else stays exactly as it was, switch a few named options,
and export the result as MIDI and MusicXML tab for his DAW and Guitar Pro.

Success is measured by use, not by tests: in one 30-45 minute sitting the
owner curates and exports at least one piece and opens it in his editor. The
tool logs his actions (section 8) so the next development step can be chosen
from what he actually re-rolls and keeps.

## 2. Context you must read first

- `composer-lab/README.md`: how pieces are generated, recipe files.
- `composer-lab/composer.py`: `make_recipe` (stacked version flags, each plan
  layer has its own `random.Random(seed * K + c)` stream), `compose(recipe)`
  (deterministic: same recipe -> same notes), `export` (writes `.mid`,
  `.musicxml`, `.tab.txt`), dose tables `V8`, `V9`.
- `composer-lab/test_composer.py`: invariants every piece must keep (string and
  fret consistency, legato only on one string, taps above the fretting hand,
  no semitone clash on shared accented onsets, MusicXML measures fill their
  meter, no minor ninths between the guitars in v7+), and the `NoTail` scope
  test as the model for "change one thing, nothing else moves".
- `composer-lab/render/`: `audition_midi.py` (forces program 0),
  `render.sh` (fluidsynth + sox), `to_mp3.sh`. Rendering runs inside WSL
  (NixOS) under `nix shell nixpkgs#fluidsynth nixpkgs#sox nixpkgs#ffmpeg`;
  the guitar SoundFont is `../_griff_out/sf/fsbs_clean.sf2` (next to the repository; override with GRIFF_SOUNDFONT).
- `CHANGE-7.md`, `CHANGE-8.md`, `CHANGE-9.md` (Result sections): the owner's
  own words about what he wants to change in pieces.

Base version for the tool: **v8** (`--v8`), the best-received version. Other
flags stay available as options, never as the default.

## 3. Constraints

- Python 3.12 standard library only (as the rest of composer-lab). The web
  page is one static HTML file with inline CSS/JS, no build step, no CDN.
- Do not change the output of any existing version flag. Every existing test
  in `test_composer.py` must still pass unchanged. New behaviour is opt-in
  through new recipe fields that default to the current behaviour.
- Code, comments, commit messages and docs in English. Comments only where a
  non-obvious invariant needs them; no history, task numbers or dates in code.
- Test-first: for each behaviour in section 6, write the failing test, then
  the code. Run `python -m unittest test_composer` (from `composer-lab/`) and
  the new tests; both must be green at the end.
- Work only inside `composer-lab/` of this worktree. Do not push. Commit in
  small steps on the current branch.
- Never read or copy the owner's tab library (`D:\tabs`) or anything under
  `_griff_out/corpus`; the tool does not need them.

## 4. Data model: layers and salts

A recipe gets one new optional field:

```json
"studio": {
  "salts": {"<layer>@<section>": <int>, "...": 0},
  "locks": ["<layer>@<section>", "..."],
  "options": {"b_lead": "augmented|same_speed|rest", "...": "..."}
}
```

A **layer** is one musical decision that can be re-drawn on its own. A
**section** is one of `intro`, `A`, `A2`, `B`, `A3`, `end` (only those present
in the piece's form). Re-rolling increments the salt of one `layer@section`;
the layer's random stream for that section is then
`random.Random(f"{recipe_seed}:{layer}:{section}:{salt}")` (string seeds are
deterministic in CPython). When a recipe has no `studio` field, every layer
behaves exactly as today.

Required layers (v8 base):

| Layer | Scope | What a re-roll changes | Must not change |
|---|---|---|---|
| `comp` | A or A2 | the second guitar's role per bar in that section (chords / chatter / octave chords) and, for chords, its strike rhythm (accents of the grouping bent as in `comp_pattern`, but drawn from the layer stream) | the lead, other sections, harmony |
| `texture` | A or A2 | arpeggio vs tapping for chatter bars, the arpeggio cycle order | lead, roles, other sections |
| `lead_rhythm` | whole piece (section `all`) | the hook's rhythm and contour (new hook drawn as `plan_motifs` would), keeping meter, mode, key, harmony, form | harmony, form, tempo, second-guitar plan (its mask may follow the new lead, as today) |
| `b` | B | the B variant (palm-mute chug / half-time chords / clean arpeggio) and the chug pattern | other sections |
| `intro` | intro | number of intro bars (1, 2 or 4) and the intro texture density | other sections |
| `ending` | end | held chord / short stop / riff-to-hard-end | other sections |
| `tempo` | whole piece | tempo within 0.8-1.1 of the current value | notes (only timing) |

Required options (switches, not random):

- `b_lead`: `augmented` (current behaviour: B lead = hook inverted with
  doubled durations), `same_speed` (inverted hook at its original note
  values), `rest` (no lead in B). The owner heard the augmented B lead as a
  "drunk slowdown" in one piece and as fine in another, so this must be a
  per-piece choice, not a global change.
- `trailing_rest`: on/off (`--notail` behaviour for this piece only).
- The `V8` doses (`octave_piece`, `octave_sixteenths`, `dead_notes`,
  `slide_max`) as per-piece values that apply on the next re-roll of the
  layers that use them.

Locks: a locked `layer@section` is never re-rolled, including by "re-roll the
whole piece except locked parts".

## 5. Web app

`composer-lab/studio/server.py`: `python -m studio.server --port 8765
--out <dir>` (run from `composer-lab/`), `http.server`-based, binds
`127.0.0.1`. All state is files in `<dir>`: one folder per piece with
`recipe.json`, `piece.mid`, `piece.musicxml`, `piece.tab.txt`, `piece.mp3`,
and `history.jsonl`.

JSON API (all POST bodies JSON, all responses JSON unless a file):

- `POST /api/new {"seed": int|null}`: new v8 piece (random seed if null).
- `GET /api/pieces`: list with seed, meter, mode, key, tempo, length, sections.
- `GET /api/piece/<id>`: recipe summary, section list with start/end seconds,
  layers per section with salt and lock state, options.
- `POST /api/piece/<id>/reroll {"layer": str, "section": str}`.
- `POST /api/piece/<id>/lock {"layer": str, "section": str, "locked": bool}`.
- `POST /api/piece/<id>/option {"name": str, "value": ...}`.
- `POST /api/piece/<id>/undo`: restore the previous recipe (history stack).
- `GET /api/piece/<id>/audio.mp3`, `/piece.mid`, `/piece.musicxml`,
  `/piece.tab.txt`.

Every change writes the new recipe, re-runs `compose` + `export`, renders the
MP3 and appends a line to `history.jsonl` (section 8). Rendering: the server
calls a renderer function with a pluggable backend; the default backend runs
`render/audition_midi.py` then `render/render.sh` and `render/to_mp3.sh`
through `wsl -d NixOS -- nix shell ... -c bash ...` when the server runs on
Windows, or directly when it runs inside WSL. A fake backend for tests writes
a placeholder file.

`composer-lab/studio/index.html` (served at `/`), one page, Russian UI
labels (the owner's language), works at desktop and phone width:

- Piece list on the left / top: "Новая пьеса", list of pieces.
- Player for the current piece with a timeline under it: one coloured block
  per section, labelled (Вступление, Куплет A, Куплет A2, Середина B,
  Кульминация, Концовка) with its seconds. Clicking a block seeks the player.
- Per section, buttons for its layers ("Перегенерировать ритм-гитару",
  "Фактура", "Вариант середины", "Вступление", "Концовка") and a lock toggle
  per layer. Whole-piece buttons: "Новый риф" (`lead_rhythm`), "Темп",
  "Перегенерировать всё незакреплённое".
- Options panel: B lead speed (три кнопки), trailing rest on/off, the V8
  doses as sliders with their current values.
- "Отменить", "Скачать MIDI", "Скачать табы (MusicXML)".
- A busy indicator while a change renders; the page stays usable.

## 6. Tests (write first)

In `composer-lab/test_studio.py` (stdlib `unittest`, no network, fake render
backend):

1. A recipe without `studio` composes byte-identical MIDI to today for v8
   seeds 801, 802, 805, 807, 809, 810.
2. For each layer and each section it applies to, on 20 seeds: re-rolling
   changes the target, and every note outside the declared scope (by section
   time span and guitar) is identical before and after. For `lead_rhythm`
   the declared scope is the lead and the second guitar's notes that are
   masked by the lead; harmony, form and tempo are identical.
3. Re-rolling the same `layer@section` with the same salt twice gives the
   same recipe and MIDI (determinism); different salts give a different
   result in at least 15 of 20 seeds.
4. A locked layer is not changed by "re-roll all unlocked".
5. `b_lead`: `same_speed` B lead notes have the hook's durations; `rest`
   leaves no lead notes in B; `augmented` equals today.
6. All invariants of `ComposerInvariants` hold for 20 seeds after 5 random
   re-rolls each.
7. Undo restores the previous recipe exactly.
8. Server: start on a free port in a thread, call `new`, `reroll`, `lock`,
   `option`, `undo`, download MIDI; check status codes, JSON shape and that
   files exist. `history.jsonl` has one line per change.

A negative control for test 2: break the layer isolation on purpose (draw the
`comp` layer from the piece stream), confirm test 2 turns red, restore.

## 7. Out of scope

New musical idioms (fast single-note line, "шмель", rhythm skeletons from
tabs), band renders, blind listening packs, any change to versions v1-v9.

## 8. Usage log

`history.jsonl` per piece, one JSON object per change:
`{"t": ISO time, "action": "new|reroll|lock|option|undo|export",
"layer": ..., "section": ..., "salt": ..., "value": ...}`. Exports are logged
too. A small script `studio/summary.py <dir>` prints, over all pieces: number
of re-rolls per layer, re-rolls before the first export per piece, options
used, pieces exported. This is the data for the next decision.

## 9. Deliverables and acceptance

- `studio/` package (server, page, renderer backends, summary), `test_studio.py`.
- README section "Studio" with the start command for Windows (PowerShell) and
  for WSL, and a one-paragraph Russian how-to for the owner.
- All tests green; the negative control recorded in the final report.
- A final report (in the reply, not a file) listing: what was built, test
  results with the command used, anything not done, and any place where the
  spec could not be followed and why.
