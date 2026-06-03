# ADR 0016: Share one UI core across the ratatui and egui frontends

Date: 2026-06-03
Status: Proposed

## Context

`griff` will grow two user-facing frontends that must show the *same* thing: the
`ratatui` terminal preview (S8, exists today) and an `egui` preview/plugin GUI
(S10+, ADR-0007). A terminal UI cannot be a plugin GUI — a DAW hosts a native
window, not a terminal — so `egui` is unavoidable for the CLAP GUI, while
`ratatui` remains the headless/CI-verifiable, zero-GPU developer face
(`App::snapshot` over a `TestBackend`).

The risk is divergence: two frontends independently re-implementing the same
interaction logic (scroll, zoom, selection, playback) and the same layout math
(tick↔column, pitch↔row), so a change verified in the terminal does not match
what `egui` paints. This is already latent — `preview/src/render.rs` and
`preview/src/tui.rs` each compute the piano-roll layout independently, and the
viewport state lives welded inside `tui::App`. A third copy in `egui` would make
the preview unmaintainable.

The codebase already has the right seam half-built: `view::PianoRollView` and
`analysis::Analysis` are pure, renderer-agnostic projections of a `Score`. What
is missing is a shared home for *interaction state* and *resolved placement*,
plus a rule that stops a renderer from re-deriving either.

## Decision

We keep **one UI core** that both frontends consume, and make frontends thin
adapters that only map data to cells/pixels and raw input to intents. The core
has four layers, top to bottom:

1. **View-model** (`PianoRollView`, `Analysis`) — pure projection of a `Score`.
   Already exists; renderer-agnostic by construction.
2. **Interaction core** (`viewport`) — `Viewport` state (scroll, zoom, pitch
   offset, selection, playback, inspector toggle), a semantic `Intent` enum
   (device-independent: `TogglePlay`, `ScrollRight`, `NextSection`, …), and a
   pure reducer `Viewport::apply(intent, &ViewContext)`. Playback advance and
   autoscroll are pure functions of elapsed time and plot width. This is the
   *behavioural* half of "both frontends agree."
3. **Scene** (`scene`, follows in a later slice) — a `resolve(view, analysis,
   viewport, GridSize) -> Scene` that lifts all layout math into one place and
   emits a *placed* grid (note cells, bar/section lines, playhead column,
   status segments) in abstract cells, with **semantic** style (lane index,
   emphasis enum, section class) rather than concrete colours. This is the
   *visual* half.
4. **Renderers** — `ratatui` and `egui`. Each maps `Scene` → its toolkit and its
   raw input → `Intent`. The piano-roll is fundamentally a grid, so `egui`
   draws the same integer cell grid as `ratatui`, scaled by a pixel cell size;
   the only per-renderer freedom is one small style-mapping table (glyphs vs
   colours).

The enforcement mechanism is a **compile-time boundary**: when `egui` arrives
the UI core moves to its own crate (`griff-ui-core`) and the `egui` frontend
depends only on its `Scene` and `Intent`, never on `griff-core` domain types,
`PianoRollView`, or the layout math. A renderer that cannot *see* the domain or
the placement math cannot re-derive or diverge from it; divergence becomes a new
dependency, visible in review, not a silent drift.

We do **not** share input handling (keys vs mouse/wheel differ — only `Intent`s
cross), and we do **not** build a portable widget framework on top of two
toolkits. The core is data + reducer + a grid resolver; nothing more.

Tests assert the `Scene` (and the reducer) headlessly; the `ratatui`
cell-snapshot stays as a human-readable witness of the same scene.

## Consequences

- Good: a change verified in the `ratatui` snapshot constrains `egui`, because
  both consume the same `Viewport`/`Scene` and the renderer holds no logic.
- Good: removes the existing `render.rs`/`tui.rs` layout duplication — the
  refactor pays off before `egui` exists.
- Good: `egui` standalone (eframe) and `egui`-in-plugin (baseview) are the same
  widget tree with a different host window.
- Bad / cost: an extra indirection (intent → reduce → resolve → paint) and the
  discipline to push any renderer-local need *down* into the core instead of
  hacking it locally.
- The abstraction is only fully proven by the second renderer, so the first
  `egui` commit must render the existing `Scene` and nothing else; anything it
  lacks is a signal to extend the core, not the frontend.
- Delivered incrementally: the interaction core (`viewport`) lands first; the
  `Scene`/`resolve` placement layer and the `griff-ui-core` crate extraction
  follow as their own slices.
