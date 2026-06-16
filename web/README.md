# griff web playground (WASM)

A browser front for the complement arranger — built so the engine can be driven
(and *heard*) from a phone, no install. See
[`docs/adr/0024-web-wasm-frontend-for-mobile.md`](../docs/adr/0024-web-wasm-frontend-for-mobile.md).

This is the **MVP** (ADR-0024 §2): a deliberately thin, throwaway front — no
`wasm-bindgen`, no framework. `griff-web` is an *import-free* `cdylib` that
exports a few C-ABI functions; the page (`static/`) loads the `.wasm` with
`WebAssembly.instantiate(bytes, {})` and marshals a small JSON result through
linear memory. The canonical `egui` frontend (ADR-0016) replaces it at M2.

## What it does

Takes a part A — either a built-in sample lead or **a track from a MIDI file you
load** — and generates a complement (part B) entirely in the browser, with live
controls for **mode**, **seed**, **register offset**, and **pitch spread** (the
ADR-0023 `VariationControl`, audible on the grid-locked modes). Deterministic:
the same controls always produce the same result.

## Build & run locally

```sh
./web/build.sh                              # → web/dist/ (wasm + static)
python3 -m http.server -d web/dist 8080     # open http://localhost:8080
```

The crate is wasm32-only and excluded from the root workspace (like `fuzz/`), so
stable `--workspace` builds/clippy/tests never touch it. It depends on
`griff-core` with `default-features = false`, dropping the Guitar Pro importer
(`guitarpro`/`zip`/`time`/`getrandom` → `wasm-bindgen`) — that is what keeps the
module import-free. MIDI import is always available, so loading a `.mid` works in
the lean build (~220 KiB once the parser is reachable code); Guitar Pro file
loading waits on a `getrandom` backend that does not pull in `wasm-bindgen`.

## ABI

| export | signature | meaning |
| --- | --- | --- |
| `arrange` | `(mode:u32, seed:u32, offset:i32, variation:f32, track:i32) -> *const u8` | arrange over part A (`track<0` = built-in sample, `track>=0` = loaded score's track); returns a pointer to JSON |
| `arrange_len` | `() -> usize` | byte length of the last `arrange` result |
| `input_alloc` | `(len:usize) -> *mut u8` | reserve `len` bytes for an uploaded file; JS writes the bytes here, then calls `load_score` |
| `load_score` | `(len:usize) -> *const u8` | parse the input buffer (MIDI), stash the score, return a pointer to a JSON track summary |
| `load_len` | `() -> usize` | byte length of the last `load_score` summary |
| `memory` | — | the linear memory JS reads the JSON from |

`mode`: 0 `rhythm_lock`, 1 `register_contrast`, 2 `call_response`,
3 `support_layer`, 4 `octave_double`, 5 `counter_melody`.

Arrange JSON: `{ppqn, tempo, realized_spread, error, tracks:[{name, role, notes:[{p,s,d,v}]}]}`.
Load summary JSON: `{error, ppqn, tempo, bars, tracks:[{i, name, notes}]}`.

## Deploy

`.github/workflows/web.yml` builds `web/dist` and publishes it to GitHub Pages on
pushes to the default branch (enable Pages → "GitHub Actions" in repo settings).

## Notes / next

- Audio is a placeholder WebAudio synth (sawtooth + envelope, A left / B right).
  A real SoundFont (guitar tone) is a follow-up.
- You can load your own **MIDI** file and arrange over any of its tracks;
  **Guitar Pro** file loading (needs a `wasm-bindgen`-free `getrandom`) and
  drag-drop are follow-ups.
