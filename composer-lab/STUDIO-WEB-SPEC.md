# Studio Web: the Studio tool running fully in the browser (specification)

Status: specification for implementation. Base: branch `claude/griff-studio`
(Studio with a local Python server, commits e84a719 and 31f0448, reviewed).
Goal: the same Studio, served as static files (GitHub Pages), with the
generator running in the browser through Pyodide, so the owner can use it from
a phone without a computer running a server.

## 1. Scope

In scope: a static web app in `composer-lab/web/` that offers everything the
current Studio page offers (new piece, section timeline, per-layer re-roll and
lock, whole-piece re-roll, options, undo, MIDI and MusicXML download, usage
log), with no server code at run time.

Out of scope: changing the generator's music, the Rust Griff code, the
existing `cockpit` Pages site, and deploying. Deployment is a separate,
owner-approved step (section 9).

## 2. Read first

- `composer-lab/STUDIO-SPEC.md` (behaviour that must be preserved).
- `composer-lab/studio/engine.py` (pure recipe functions: `new_recipe`,
  `reroll`, `set_lock`, `set_option`, `available_layers`, `piece_sections`,
  `piece_detail`, plus the file-backed `StudioStore`), `studio/index.html`
  (current UI), `studio/server.py` (API shapes the page uses).
- `composer-lab/composer.py`, `fretboard.py`, `midiio.py`, `musicxml.py`
  (the generator; standard library only).
- `composer-lab/render/render.sh` (current sound: FluidSynth with the
  FreePats "EGuitarFSBS clean" SoundFont, CC0, then sox reverb).

## 3. Architecture

1. **Python in the browser.** Load Pyodide from
   `https://cdn.jsdelivr.net/pyodide/<pinned version>/full/` (pin an exact
   version released at least two weeks ago; no other CDN). Fetch the Python
   sources from the site (`py/composer.py`, `py/fretboard.py`, `py/midiio.py`,
   `py/musicxml.py`, `py/studio/engine.py`, `py/web_api.py`), write them into
   Pyodide's virtual file system and import them. No Python package installs.
2. **`composer-lab/studio/web_api.py`** (new, pure Python, also runs under
   CPython): an in-memory API over the engine's pure functions, JSON in, JSON
   out, no file system or subprocess use:
   - `new_piece(seed|None) -> state`
   - `reroll(state, layer, section) -> state`, `lock(state, layer, section,
     locked) -> state`, `option(state, name, value) -> state`, `undo(state) ->
     state`
   - `detail(state) -> dict` (what the page shows: sections with start/end
     seconds and labels, layers with salt and lock, options, can_undo)
   - `events(state) -> list` of notes for playback: `{"t": seconds, "dur":
     seconds, "pitch": int, "vel": int, "guitar": 1|2, "dead": bool}`, with
     ring durations as realized, sorted by time.
   - `export_midi(state) -> base64 str`, `export_musicxml(state) -> str`,
     `export_tab(state) -> str`.
   - `state` is a JSON-serializable dict holding the recipe and the undo stack
     (bounded to 50 entries). The JS side stores it; Python keeps no globals
     between calls.
   It must reuse the engine's functions; do not duplicate their logic. If the
   engine's pure functions need a small refactor to be importable without
   `StudioStore`'s dependencies, do it without changing behaviour (existing
   tests stay green).
3. **Sound: a sampler, not a SoundFont player.** The SoundFont is 446 MB and
   cannot be shipped. Add `composer-lab/web/build_samples.sh` (runs in WSL
   under `nix shell nixpkgs#fluidsynth nixpkgs#sox nixpkgs#ffmpeg`) that
   renders, with the same SoundFont path the renderer uses
   (`../_griff_out/sf/fsbs_clean.sf2` (next to the repository; override with GRIFF_SOUNDFONT)),
   one sample per MIDI pitch 40-88 at two velocities (72 and 100), 2.5 s
   each, mono, 44.1 kHz, encoded as `.mp3` at 96 kbit/s into
   `composer-lab/web/samples/` with a `manifest.json`. Target total size under
   8 MB. Commit the samples and the SoundFont's licence text
   (`_griff_out/sf/fsbs_LICENSE.txt`) next to them.
   In the page, a small Web Audio sampler plays `events(state)`: pick the
   nearer velocity layer, scale gain by velocity, stop each note at its
   duration with a 30 ms release, dead notes as 40 ms clicks at low gain,
   guitar 1 panned left and guitar 2 right (as in the MIDI export), a single
   convolution-free reverb is optional. Transport: play/pause, seek by
   clicking a section block or a progress bar, current-time display.
4. **Persistence.** Pieces (their `state`) and the usage log live in
   IndexedDB (fall back to memory if unavailable, wrapped in try/catch). A
   "Скачать журнал" button downloads the log as JSONL in the same format as
   `history.jsonl` (STUDIO-SPEC section 8), so `studio/summary.py` can read
   it. A "Экспорт/импорт пьес" option saves and loads all pieces as one JSON
   file.
5. **Files.** `composer-lab/web/index.html` (page, Russian UI labels as in
   the current Studio page, inline CSS/JS, no build step), `web/app.js` (may
   be inline instead), `web/samples/`, `web/build.py` (copies the Python
   sources listed in 3.1 into `web/py/` so the site is self-contained; the
   copies are generated and git-ignored, or the page fetches them from a
   path configured in one constant — choose one and document it).
6. **Paths.** Every URL in the page is relative, so the site works under a
   sub-path such as `/griff/studio/`.

## 4. Behaviour to preserve

Everything in STUDIO-SPEC sections 4 and 5 as implemented and reviewed on
`claude/griff-studio`: layer scopes, locks (including the ending riff and
frozen comp decisions), options, undo, dose snapshots. Same seed and same
action sequence must give the same notes in the browser as `engine` under
CPython (Pyodide runs the same CPython code).

## 5. Performance and UX

- First load: show a progress message while Pyodide and samples load; the
  page must say clearly what is happening. Later loads use the browser cache.
- A re-roll must update the timeline and be playable within 2 s on a desktop
  after Pyodide is loaded (compose is about 20-100 ms under CPython; measure
  under Pyodide and report it).
- Run Python in a Web Worker so the UI does not freeze; the page stays usable
  while a change is computed.
- Works at 360 px phone width; touch targets at least 40 px; no horizontal
  scroll.

## 6. Constraints

- Python changes: standard library only; existing tests
  (`python -m unittest test_composer test_studio` from `composer-lab`) stay
  green; `test_composer.py` is not edited; outputs of all existing version
  flags stay byte-identical.
- No secrets, no tracking, no network calls other than the pinned Pyodide CDN
  and the site's own files.
- English for code, comments, docs; Russian for UI labels. Minimal comments.
- Work only inside `composer-lab/` of the worktree. Do not commit or push.
- Do not read `D:\tabs` or `_griff_out/corpus`.

## 7. Tests (write first)

- `composer-lab/test_web_api.py` (CPython, stdlib `unittest`):
  1. `new_piece(801000)` then `events` equals the notes of the engine piece
     for the same seed (start, pitch, duration, velocity, guitar).
  2. For a fixed action sequence (re-roll comp@A, lock b@B, option b_lead
     same_speed, re-roll all, undo), `web_api` and `StudioStore` (fake
     render) end in identical recipes and identical MIDI bytes.
  3. `state` survives `json.dumps`/`json.loads` between every call.
  4. Undo stack bound: after 60 changes, 50 undos are possible and the 51st
     reports no undo.
  5. `export_midi` decodes to the same bytes as `composer.export`.
- `composer-lab/web/check_site.py`: verifies the site is self-contained
  (every relative URL referenced by `index.html`/`app.js` exists after
  `build.py`; samples manifest lists existing files; total size of `web/`
  under 12 MB excluding Pyodide).
- Browser check (report, not a unit test): if a headless Chromium or Edge is
  available (`msedge --headless`), load the page from a local static server
  (`python -m http.server` in `web/`), wait for Pyodide, create a piece,
  re-roll a layer, and capture a screenshot and the console log; report
  timings and any console errors.

## 8. Deliverables

- `studio/web_api.py`, `test_web_api.py`, `web/` (page, sampler, samples,
  build and check scripts), README section "Studio Web" with: how to rebuild
  samples, how to run locally (`python -m http.server` in `web/`), and a
  short Russian how-to for the owner.
- A final report: what was built, test commands and results, Pyodide version
  and load time, re-roll time under Pyodide, total site size, anything not
  done, and any deviation from this spec with the reason.

## 9. Deployment (not part of this task)

The repository already deploys `cockpit/dist` to GitHub Pages
(`.github/workflows/cockpit-pages.yml`). The proposed deployment, to be done
only after the owner approves publishing, is one step in that workflow that
copies `composer-lab/web/` (after `build.py`) to `cockpit/dist/studio/`, so
Studio appears at `<pages-url>/studio/` without replacing the cockpit. Do not
edit the workflow in this task.
