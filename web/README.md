# griff web playground (WASM)

A browser front for the complement arranger — built so the engine can be driven
(and *heard*) from a phone, no install. See
[`docs/adr/0024-web-wasm-frontend-for-mobile.md`](../docs/adr/0024-web-wasm-frontend-for-mobile.md)
and [`docs/adr/0025-guitar-pro-in-browser-needs-wasm-bindgen.md`](../docs/adr/0025-guitar-pro-in-browser-needs-wasm-bindgen.md).

This is the **MVP**: a deliberately thin, throwaway front — no framework, just two
`wasm-bindgen` functions returning JSON strings. To load Guitar Pro tabs the Rust
GP reader pulls `zip`/`time`/`getrandom`, which need `wasm-bindgen` glue, so the
build is no longer import-free (ADR-0025 supersedes ADR-0024's lean cdylib). The
canonical `egui` frontend (ADR-0016) replaces it at M2.

## What it does

Takes a part A — either a built-in sample lead or **a track from a MIDI or Guitar
Pro file you load** — and generates a complement (part B) entirely in the browser,
with live controls for **mode**, **seed**, **register offset**, and **pitch
spread** (the ADR-0023 `VariationControl`, audible on the grid-locked modes).
Deterministic: the same controls always produce the same result.

## Build & run locally

```sh
# one-time: install the CLI matching the wasm-bindgen crate version in Cargo.toml
cargo install wasm-bindgen-cli --version 0.2.125 --locked

./web/build.sh                              # → web/dist/ (wasm + glue + static)
python3 -m http.server -d web/dist 8080     # open http://localhost:8080
```

The crate is wasm32-only and excluded from the root workspace (like `fuzz/`), so
stable `--workspace` builds/clippy/tests never touch it. It depends on
`griff-core` with the `gp` feature on, so MIDI and Guitar Pro both import through
the shared `import_score_auto` — the same parser as the CLI. `build.sh` sets
`--cfg getrandom_backend="wasm_js"` (Web Crypto) and runs `wasm-bindgen --target
web`; the payload is ~830 KiB (gp + zip + wasm-bindgen).

## API

Two `#[wasm_bindgen]` functions, called from the generated ES module
(`griff_web.js`); both return a JSON string:

| export | signature | meaning |
| --- | --- | --- |
| `arrange` | `(mode, seed, offset, variation, track) -> String` | arrange over part A (`track<0` = built-in sample, `track>=0` = the loaded score's track) |
| `load_score` | `(bytes: &[u8]) -> String` | parse an uploaded MIDI or Guitar Pro file, stash the score, return a track summary |

`mode`: 0 `rhythm_lock`, 1 `register_contrast`, 2 `call_response`,
3 `support_layer`, 4 `octave_double`, 5 `counter_melody`.

Arrange JSON: `{ppqn, tempo, realized_spread, error, tracks:[{name, role, notes:[{p,s,d,v}]}]}`.
Load summary JSON: `{error, ppqn, tempo, bars, tracks:[{i, name, notes}]}`.

## Deploy

`.github/workflows/web.yml` installs the version-matched `wasm-bindgen-cli`, builds
`web/dist`, and publishes it to GitHub Pages on pushes to the default branch
(enable Pages → "GitHub Actions" in repo settings).

## Notes / next

- Audio is a placeholder WebAudio synth (sawtooth + envelope, A left / B right).
  A real SoundFont (guitar tone) is a follow-up.
- You can load your own **MIDI** or **Guitar Pro** (`.gp3/.gp4/.gp5/.gpx`) file and
  arrange over any of its tracks. Drag-drop and in-browser corpus curation /
  download are follow-ups.
