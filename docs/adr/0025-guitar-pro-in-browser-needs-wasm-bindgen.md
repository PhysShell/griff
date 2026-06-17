# ADR 0025: Guitar Pro in the browser needs wasm-bindgen (supersede ADR-0024's import-free web build)

Date: 2026-06-17
Status: Accepted

Supersedes ADR-0024 §2–§3 and §6 (the import-free `cdylib`, the `gp`-off wasm
build, and the no-`wasm-bindgen` toolchain). ADR-0024's other decisions — egui as
the M2 canonical web frontend, WebAudio, determinism — stand.

## Context

ADR-0024 shipped the M1 web playground as an *import-free* `cdylib`: `griff-core`
built with `default-features = false` (GP off), exporting C-ABI functions, loaded
with `WebAssembly.instantiate(bytes, {})` — no `wasm-bindgen`, ~90 KiB. That kept
the build trivial but made the browser **MIDI-only**.

The corpus is swancore-first (ADR-0005), and swancore tabs are overwhelmingly
**Guitar Pro**, not MIDI. Phone-side curation — the reason the web front exists —
is dead without GP loading: the maintainer works from a phone and cannot feed the
corpus real material there. Loading GP in the browser is the unblocker.

The Rust GP reader is not import-free-compatible. `guitarpro` → `zip` (a
non-optional dependency; `.gpx` is a zip container) → `time` → `js-sys` →
`wasm-bindgen`, and `zip` → `getrandom`, whose wasm support also routes through
`wasm-bindgen`. A `getrandom` *custom* backend (to dodge that) fails to compile on
`wasm32-unknown-unknown` in getrandom 0.4.2 (a `WEB_CRYPTO` bug), and `time` pulls
`wasm-bindgen` independently regardless. There is no lean shortcut: GP through the
shared Rust parser requires the `wasm-bindgen` toolchain.

The alternative — parse GP in JavaScript (e.g. alphaTab) and feed notes to the
wasm — was rejected: it forks parsing out of `griff-core`, so the browser and the
CLI would disagree on coverage and bugs, and it adds a heavy JS dependency.

## Decision

1. **The web build uses `wasm-bindgen`** (`--target web`), not the import-free
   `cdylib`. `griff-web` exports two `#[wasm_bindgen]` functions returning JSON
   strings (`arrange`, `load_score(bytes)`); the manual linear-memory marshalling
   is gone. The page loads the generated ES module (`<script type="module">`).

2. **The wasm build enables `gp`** (default features on `griff-core`) so Guitar
   Pro (`.gp3/.gp4/.gp5/.gpx`) and MIDI both import through the shared
   `import_score_auto` — the *same* parser as the CLI, so behaviour matches.

3. **`getrandom` uses its `wasm_js` backend** (Web Crypto), enabled by the
   `wasm_js` feature plus `--cfg getrandom_backend="wasm_js"` in `RUSTFLAGS`
   (set in `build.sh`).

4. **`wasm-bindgen-cli` is pinned to the `wasm-bindgen` crate version**
   (`= 0.2.x` in `web/Cargo.toml`); `build.sh` and CI derive and install that
   exact version, and CI caches the built CLI.

## Consequences

- GP tabs load in the browser, parsed by the same Rust code as the CLI. Phone
  curation is unblocked.
- The web build is no longer import-free or lean: the payload grows from ~90 KiB
  to ~830 KiB (gp + zip + wasm-bindgen), and the toolchain now needs
  `wasm-bindgen-cli` (version-matched). Accepted — GP support is worth it.
- `griff-core`'s `gp` feature gate is unchanged; the lean MIDI-only wasm path
  still exists (`default-features = false`), it is just not what the playground
  ships.
- Determinism (SPEC §6) is unaffected: `getrandom` is present only transitively
  (zip never consumes randomness on the read path); the engine's seeded PRNG is
  untouched.
- ADR-0024's M2 plan stands: the egui/eframe frontend remains canonical and
  replaces this playground; this ADR only changes the M1 build mechanics.

## See also

- [`0024-web-wasm-frontend-for-mobile.md`](0024-web-wasm-frontend-for-mobile.md)
- [`0005-swancore-first-scope.md`](0005-swancore-first-scope.md)
- [`0018-rich-note-model-fretboard-and-techniques.md`](0018-rich-note-model-fretboard-and-techniques.md)
