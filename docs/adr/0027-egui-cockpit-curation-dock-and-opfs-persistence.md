# ADR 0027: Grow the M1 playground into the egui M2 cockpit — curation dock + OPFS persistence

Date: 2026-06-21
Status: Proposed

Realises the piece ADR-0024 and ADR-0026 explicitly deferred — *"corpus
curation / persistence on web (the `preview/design/` curation dock — later)"*
(ADR-0024 §"out of scope"; ADR-0026 §Consequences). ADR-0016 (shared UI core),
ADR-0024 (egui as the canonical web frontend), and ADR-0025 (the wasm-bindgen
toolchain) stand unchanged; this ADR consumes them.

## Context

The shared UI core is already half-built to plan. ADR-0016 chose one core for
both frontends — view-model (`view::PianoRollView`, `analysis::Analysis`) →
interaction core (`viewport`: a semantic `Intent` enum and the pure reducer
`Viewport::apply(intent, &ViewContext)`) → placement (`scene::resolve(view,
analysis, vp, GridSize) -> Scene`, a grid of `SceneCell`s tagged by a semantic
`CellRole`) → renderers. The `preview` crate implements all four layers with
`ratatui` as renderer #1, and the `Scene` and reducer are asserted headlessly.
`egui` is the planned renderer #2, and ADR-0016 already scoped its first commit:
*"render the existing `Scene` and nothing else."*

The web path is also settled. ADR-0024 made the canonical web frontend the
`eframe`/`egui` app compiled to `wasm32-unknown-unknown` (the M2 target), with a
deliberately throwaway M1 JS playground in front. ADR-0025 fixed the toolchain
(`wasm-bindgen --target web`, `gp` on, `getrandom` `wasm_js`, a version-pinned
`wasm-bindgen-cli`) so the *same* Rust parser loads Guitar Pro and MIDI in the
browser. ADR-0026 added a thin, **download-only** capture path
(`detect_boundaries_json`, `build_chunk_json` → a real `corpus::ChunkMeta`) — but
explicitly deferred the dock, in-browser persistence, edit actions, and manifest
assembly.

That deferred dock is not vapour: `preview/design/index.html` (and `tab.html`)
is a complete, egui-flavoured mockup of it — a menu bar (File / View / **Corpus**),
a transport (play / loop / zoom / position), three docks (tracks │ piano-roll with
chunk-classification bands and a playhead │ a chunk inspector with structure
meters, tag chips, and Approve / Reject / Split / Merge), and a status bar.

The forcing function is the maintainer's phone. ADR-0025/0026 unblocked *loading*
and *capturing* a chunk there, but capture is stateless and download-only: every
chunk is a file hand-shuttled to a desktop `griff manifest`. To actually **hold
and shape a corpus from the web** — browse it, dedup across it, split / merge /
retag, gate by rights, and assemble a manifest — needs the dock and the
persistence ADR-0026 set aside. That is this ADR.

## Decision

1. **The cockpit *is* the ADR-0016/0024 egui app, not a new web app.** One
   `eframe`/`egui` codebase targets native desktop and the browser; it absorbs
   the M1 playground (ADR-0024/0026) and realises `preview/design/`. The
   piano-roll is the ADR-0016 `Scene` renderer #2; the surrounding panels
   (tracks, inspector/curation, corpus browser) are egui drawn over the same
   `PianoRollView` / `Analysis` / `Viewport` plus the corpus types.

2. **Extract `griff-ui-core` now** — ADR-0016's prescribed step "when `egui`
   arrives." Move `view`, `analysis`, `viewport`, `scene`, and `curation` out of
   `preview` into a `griff-ui-core` crate that both the `ratatui` and the new
   `egui` frontend depend on. The **piano-roll grid widget keeps ADR-0016's
   strict boundary**: it sees only `Scene` and `Intent`, never `griff-core`
   domain types, `PianoRollView`, or the layout math, so it *cannot* re-derive
   or diverge — divergence would be a new dependency, visible in review. The
   **cockpit shell is deliberately domain-aware**: curation edits
   `corpus::ChunkMeta`, so the dock calls `griff-ui-core`'s curation/capture API
   and `griff-core` corpus types directly. We keep the strict boundary where
   divergence is the risk (the grid) and do not fake it for the dock, which is
   inherently about domain objects.

3. **Persist to OPFS as a `*.chunk.json` tree, not IndexedDB blobs.** The
   browser corpus is a directory of the *same* `chunk.json` bytes the CLI reads
   — ADR-0026's "serialize the domain type, don't fork a schema" — stored in the
   Origin Private File System, mirroring the CLI corpus layout. Manifest
   assembly (deferred by ADR-0026 §3) runs **in-wasm through the shared core**:
   the `griff manifest` fold over the OPFS tree, emitting a
   `corpus::CorpusManifest { schema_version, chunks }`. Import/export is a file
   copy — a phone-built corpus and a desktop corpus are byte-identical files, so
   they never desync. *(Rejected: IndexedDB records — a browser-specific shape
   that forks the on-disk schema, the failure mode ADR-0025/0026 already rejected
   for parsing and capture.)*

4. **Rights is a first-class gate, not a note.** `RightsInfo.redistributable` is
   already a typed fact — "any future export gate must filter on it without
   scanning prose" (`core/src/corpus.rs`). The cockpit enforces it: capturing or
   approving a chunk requires a `RightsStatus`; the corpus dashboard segments by
   rights; and any export/share path filters on `redistributable`, so
   non-redistributable source stays local in OPFS and never leaves the device.
   The gate lives in `griff-ui-core` (shared with the CLI), not re-implemented in
   the renderer.

5. **The wasm build reuses ADR-0025's toolchain unchanged** — `wasm-bindgen
   --target web`, `gp` on, `getrandom` `wasm_js`, version-pinned
   `wasm-bindgen-cli`, the existing `web/build.sh` + CI. `eframe` adds only its
   web renderer (WebGL via `web-sys`); no Trunk. The per-target seams ADR-0024
   named stay behind traits — audio (`cpal`/`midir` native ↔ WebAudio web) and
   storage (filesystem ↔ OPFS) — and everything above them is one codebase.

6. **Deliver native-first, in vertical slices; retire the JS playground only at
   capture-parity.** Per ADR-0016 ("the first `egui` commit must render the
   existing `Scene` and nothing else"), the first slice is the native egui
   `Scene` renderer. The M1 JS front (`web/index.html`, `app.js`) is deleted only
   once the egui cockpit reaches the ADR-0026 capture flow **and** OPFS manifest
   assembly on a phone; until then they coexist, and `griff-web`'s
   `#[wasm_bindgen]` exports fold into the egui crate.

## Roadmap

Delivered incrementally; each slice is a PR. The boundary work lands before the
renderer, and the renderer before its web target, so every step is shippable and
testable against the existing headless `Scene`/reducer suite.

- **S0 — extract `griff-ui-core`.** Move the four core layers + `curation` out of
  `preview`; switch the `ratatui` front onto the crate. Pure refactor, zero
  behaviour change; pays off immediately by removing the latent
  `render.rs`/`tui.rs` duplication ADR-0016 flagged.
- **S1 — egui renders the `Scene` (native).** Paint `SceneCell`s by `CellRole`,
  map raw input → `Intent` through `Viewport::apply`, drive playback via
  `advance_playback` / `autoscroll`. Renderer #2 proven against the shared core;
  anything it lacks is a signal to extend the core, not the frontend.
- **S2 — egui → wasm.** ADR-0025 toolchain; the canonical web front paints a
  loaded score.
- **S3 — load + capture in egui.** File pick → `import_score_auto`; the ADR-0026
  `detect_boundaries` / `build_chunk` flow as panels producing `ChunkMeta`.
  *(JS-retirement gate, part 1.)*
- **S4 — OPFS persistence.** A `chunk.json` tree plus the in-wasm `manifest` fold
  (`CorpusManifest`). *(JS-retirement gate, part 2 → delete the JS front.)*
- **S5 — corpus dock.** Browse and filter (class / tag / rights / cohort), a
  corpus-aggregate dashboard, and dedup across the corpus
  (`ChunkMeta.duplicate`). The "rule the corpus from the web" surface.
- **S6 — curation actions.** Wire `curation` split / merge / rename / retag /
  approve / reject over the OPFS store, realising the `preview/design` inspector.
- **S7 — onward.** `similarity::find_similar_chunks` ("find like this"); S7 graph
  recombination once the corpus is large enough to motivate it.

## Consequences

- Good: the corpus becomes phone-native — browse, dedup, split/merge, rights-gate,
  and assemble a manifest from a URL, with no desktop shuttle. Closes the gap
  ADR-0026 left open.
- Good: divergence stays impossible where it matters — the piano-roll is the same
  `Scene` the `ratatui` cell-snapshot tests pin (ADR-0016), so a change verified
  in CI constrains `egui` for free.
- Good: OPFS files *are* CLI corpus files; a browser-built corpus drops straight
  into `griff` and back, no bridge, no schema fork.
- Good: the `griff-ui-core` extraction removes the `render.rs`/`tui.rs` ↔ `egui`
  triplication risk *before* `egui` exists; the refactor pays off for `ratatui`
  on day one.
- Bad / cost: `eframe`'s wasm bundle is multiple MB on top of ADR-0025's ~830 KiB
  — a tool, not a landing page; first paint is slower. OPFS support and quotas
  vary on mobile Safari, so a download/export fallback stays.
- Bad / cost: the intent → reduce → resolve → paint indirection, plus the
  ADR-0016 discipline to push any renderer-local need *down* into the core rather
  than hack it locally — now under two renderers' worth of pressure.
- Accepted: native-first means the web target trails native by a slice; audio on
  web stays WebAudio with a placeholder synth (ADR-0024) until a license-checked
  SoundFont lands.
- Accepted: this grows well beyond a playground, so per ADR-0024 §Roadmap it earns
  its own appended stage (append-only, per the stage-label audit); that stage doc
  follows once the slices firm up. Until then it advances S8.

## See also

- [`0016-shared-ui-core-across-frontends.md`](0016-shared-ui-core-across-frontends.md)
- [`0024-web-wasm-frontend-for-mobile.md`](0024-web-wasm-frontend-for-mobile.md)
- [`0025-guitar-pro-in-browser-needs-wasm-bindgen.md`](0025-guitar-pro-in-browser-needs-wasm-bindgen.md)
- [`0026-web-chunk-capture-tool.md`](0026-web-chunk-capture-tool.md)
- [`../stages/S5-corpus-and-schema.md`](../stages/S5-corpus-and-schema.md)
- [`../stages/S8-preview-app.md`](../stages/S8-preview-app.md)
- `../../preview/design/index.html` (and `tab.html`) — the cockpit mockup
