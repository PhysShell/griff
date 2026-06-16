# ADR 0024: Ship the egui frontend to the browser (WASM) for mobile testing

Date: 2026-06-16
Status: Proposed

## Context

griff today runs only as a desktop CLI and a `ratatui` terminal preview (S8) —
both tied to a computer terminal. The primary author/tester works almost
entirely from a phone, so iterating on generation (tweaking seed / mode /
variation and *hearing* the result) is impractical: every test means getting to
a desktop.

The pieces for a browser build are already in place:

- **`griff-core` is WASM-clean.** It is pure compute — no filesystem, threads,
  wall-clock, or the `rand` crate (the seeded PRNG is a hand-rolled `SplitMix64`
  finalizer); all file I/O lives in the CLI. It compiles to
  `wasm32-unknown-unknown` today (verified 2026-06-16), `serde` and collections
  included.
- **ADR-0016 already chose `egui`** as the GUI renderer over a shared UI core
  (view-model → interaction core → scene → renderers), and **`eframe` builds the
  *same* egui app to native desktop and web/WASM**. A browser build is therefore
  not a new frontend — it is the planned egui renderer targeting the browser.
- **S8 already lists** `eframe/egui window` and `MIDI playback` as its remaining
  items; the pure `PianoRollView` / `Analysis` projections exist.

What the browser changes versus the native plan is narrow: (1) audio — S8 planned
`midir`, which has no web backend; (2) input — no filesystem; (3) build/hosting.

## Decision

1. **The canonical web frontend is the `eframe`/`egui` app compiled to
   `wasm32-unknown-unknown`** — the same renderer ADR-0016 specifies. Native and
   web share one codebase; the browser is a *build target*, not a fork. That is
   the M2 target.

2. **The M1 MVP is a thin, throwaway front, not egui** — an *import-free*
   `cdylib` (`web/`, no `wasm-bindgen`, no framework) that exports two C-ABI
   functions (`arrange`, `arrange_len`) plus the linear `memory`, with a static
   `index.html` + `app.js` that loads the `.wasm` with
   `WebAssembly.instantiate(bytes, {})` and marshals a small JSON result through
   linear memory. This unlocks phone testing now without the egui/Trunk/
   wasm-bindgen toolchain. It is disposable, not a second canonical renderer, so
   it carries no ADR-0016 divergence debt; egui replaces it at M2.

3. **`griff-core` gains a default-on `gp` feature** so the wasm build can drop
   the Guitar Pro importer (`guitarpro`/`zip` → `time`/`getrandom` →
   `wasm-bindgen`/`js-sys`). With `default-features = false` the module is
   genuinely import-free and ~90 KiB; the CLI and tests keep `gp` on and are
   unchanged.

4. **Audio on web is WebAudio**, not the Web MIDI API (absent on iOS Safari,
   patchy on mobile) and not `midir` (no web backend). The MVP uses a placeholder
   oscillator synth fed note events from core; a bundled SoundFont (guitar tone)
   is a follow-up. The playback *driver* is the one per-target seam.

5. **Input is a fixed in-code sample** for the MVP (a file picker / drag-drop
   later); the CLI keeps path-based I/O.

6. **Build and host: `cargo build --target wasm32` → copy the `.wasm` beside the
   static files → GitHub Pages** (`web/build.sh`, `.github/workflows/web.yml`).
   No Trunk or `wasm-bindgen` for the MVP. A URL, no install.

7. **Determinism is unaffected** (SPEC §6): the same controls yield the same
   output in the browser too; the engine's seeded PRNG never touches wall-clock
   or OS randomness.

## Consequences

- The maintainer can run complement — and the `VariationControl` knob — on a
  phone via a URL. That is the actual ask.
- The import-free `cdylib` needs no build tooling beyond the stock wasm target:
  `cargo build --target wasm32-unknown-unknown` then static hosting. Tiny payload
  (~90 KiB, ~35 KiB gzipped).
- The `gp` feature gate also benefits any future wasm/plugin target that only
  needs MIDI; it is a clean, default-on split.
- At M2 the per-target surface becomes: one egui codebase for desktop + web, with
  the playback driver (`midir` native / WebAudio web) and input (fs vs picker)
  behind seams; a SoundFont (license-checked) lands for a real tone.
- Accepted: the MVP synth is a placeholder (sawtooth + envelope), the roll is a
  throwaway canvas painter, and the sample part A is fixed — all replaced as M2/M3
  land.
- Accepted: mobile browsers require a user gesture before audio starts (a tap to
  unlock the `AudioContext`); SoundFont licensing/bundling is a real chore.
- Accepted: the MVP roll is throwaway; the canonical piano-roll still needs the
  ADR-0016 Scene/Viewport work (S8).
- Out of scope for the MVP: offline PWA install, and corpus curation /
  persistence on web (the `preview/design/` curation dock — later).

## Roadmap

Extends ADR-0016 and advances the S8 "egui window + playback" items toward a web
target. If it grows beyond a playground it earns its own appended stage
(append-only, per the stage-label audit).

## See also

- [`0016-shared-ui-core-across-frontends.md`](0016-shared-ui-core-across-frontends.md)
- [`0007-clap-first-plugin-target.md`](0007-clap-first-plugin-target.md)
- [`../stages/S8-preview-app.md`](../stages/S8-preview-app.md)
