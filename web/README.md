# griff web playground (WASM)

A browser front for the complement arranger — built so the engine can be driven
(and *heard*) from a phone, no install. See
[`docs/adr/0024-web-wasm-frontend-for-mobile.md`](../docs/adr/0024-web-wasm-frontend-for-mobile.md).

This is the **MVP** (ADR-0024 §2): a deliberately thin, throwaway front — no
`wasm-bindgen`, no framework. `griff-web` is an *import-free* `cdylib` that
exports two C-ABI functions (`arrange`, `arrange_len`) plus the linear `memory`;
the page (`static/`) loads the `.wasm` with
`WebAssembly.instantiate(bytes, {})` and marshals a small JSON result through
linear memory. The canonical `egui` frontend (ADR-0016) replaces it at M2.

## What it does

Builds a fixed sample lead (part A) and a generated complement (part B) entirely
in the browser, with live controls for **mode**, **seed**, **register offset**,
and **pitch spread** (the ADR-0023 `VariationControl`, audible on the grid-locked
modes). Deterministic: the same controls always produce the same result.

## Build & run locally

```sh
./web/build.sh                              # → web/dist/ (wasm + static)
python3 -m http.server -d web/dist 8080     # open http://localhost:8080
```

The crate is wasm32-only and excluded from the root workspace (like `fuzz/`), so
stable `--workspace` builds/clippy/tests never touch it. It depends on
`griff-core` with `default-features = false`, dropping the Guitar Pro importer
(`guitarpro`/`zip`/`time`/`getrandom` → `wasm-bindgen`) — that is what keeps the
module import-free and ~90 KiB.

## ABI

| export | signature | meaning |
| --- | --- | --- |
| `arrange` | `(mode:u32, seed:u32, offset:i32, variation:f32) -> *const u8` | arrange; returns a pointer to JSON in linear memory |
| `arrange_len` | `() -> usize` | byte length of the last result |
| `memory` | — | the linear memory JS reads the JSON from |

`mode`: 0 `rhythm_lock`, 1 `register_contrast`, 2 `call_response`,
3 `support_layer`, 4 `octave_double`, 5 `counter_melody`.

Result JSON: `{ppqn, tempo, realized_spread, error, tracks:[{name, role, notes:[{p,s,d,v}]}]}`.

## Deploy

`.github/workflows/web.yml` builds `web/dist` and publishes it to GitHub Pages on
pushes to the default branch (enable Pages → "GitHub Actions" in repo settings).

## Notes / next

- Audio is a placeholder WebAudio synth (sawtooth + envelope, A left / B right).
  A real SoundFont (guitar tone) is a follow-up.
- Input is a fixed in-code sample; a file picker / drag-drop comes later.
