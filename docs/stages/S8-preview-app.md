# S8: Standalone preview app

Status: in progress — interactive `ratatui` piano-roll landed (slices 1–2)
Depends on: S6
ADRs: —

> Progress: the `preview` workspace member ships:
> - **view-model** (`build_view`: `Score` → `PianoRollView`) — notes on a
>   pitch × tick plane, per-track lanes, bar gridlines. Pure, no I/O.
> - **analysis** (`analyze`: `Score` → `Analysis`) — named sections from
>   `griff_core::classify` (Riff/Breakdown/Solo/Clean/Unknown) plus structure
>   metrics and the per-axis S14 `ComplexityProfile` from
>   `griff_core::structure`. Pure, headless-testable.
> - **ASCII rasteriser** (`render_frame`) — view → fixed-size text grid.
> - **interactive TUI** (`tui::App`, `ratatui`) — colored piano-roll with
>   scroll/zoom, a named-section band, a metrics inspector (structure plus the
>   compact complexity-vector block since 2026-06-11), a playhead, and
>   keyboard navigation. The same render path drives the live crossterm loop and
>   a headless `App::snapshot` (via `TestBackend`), so the UI is CI-verifiable.
>
> The `griff-preview` binary launches the TUI, or prints one headless frame with
> `--snapshot=WxH`.
>
> Research update (2026-07): the future cockpit/playground should borrow the
> **editable text + immediate visual/audio feedback** shape from symbolic music
> editors, without adopting their notation format as Griff's domain model. S8
> owns the surface for S7 path alternatives, S9 feedback/evolution lineage, and
> S15 tonal/harmonic provenance; those stages retain their own semantics.

## UI design reference

`preview/design/` holds self-contained, dependency-free interactive mockups of
the intended native (`egui`) window (design targets only — not wired to the
engine — used to settle layout and interactions before building the real
front-end). Two cross-linked views:

- `index.html` — **piano-roll**: transport bar, left track dock, pitch × time
  grid (keyboard gutter, bar ruler, per-lane notes, playhead, S4 boundary
  overlays, S6 chunk classification bands), and a right curation/inspector dock
  (S14 structure metrics, tags, approve/reject/split/merge).
- `tab.html` — **Guitar Pro–style tablature**: standard notation staff (treble,
  written 8vb) above a TAB staff with fret numbers, rhythm stems/beams, palm-
  mute spans, power chords and lead techniques (bend/hammer/let-ring), a
  multitrack selector strip, and a toggle to hide notation (tab-only, TuxGuitar
  style). Reflects that the engine's `Score`/technique model should drive a
  notation/tab projection alongside the piano-roll.

## Remaining work (follow-up increments)

The pure layers (`view` + `analysis` + `render`) are the foundation; richer
front-ends and audio build on them:

- [x] Interactive `ratatui` front-end: scroll, zoom, named sections, metrics
      inspector, playhead, headless snapshot. (Live terminal-resize is handled by
      `ratatui` redraw; follow-cursor autoscroll is implemented for playback.)
- [x] `eframe`/`egui` native window — the canonical desktop target (piano-roll
      canvas, pan/zoom), reusing the same `PianoRollView` (the cockpit, ADR-0027).
- [ ] MIDI playback via `midir`, with a playhead overlay. Until then the cockpit
      **does not synthesise audio**: the Generate panel's `open` hands a kept
      `.mid` to whatever the OS has registered for it (a notation editor, a DAW).
      The playhead is visual only.
- [ ] **Griff textual playground** — editable request/constraint text with live
      parse diagnostics and immediate candidate refresh. Future S15 harmonic
      fixture scripts may be edited here, but typed core structures remain the
      source of truth.
- [~] **Candidate/provenance inspector** — **generation slice landed 2026-07-13**:
      the cockpit's **Generate panel** (`g`) asks for a candidate set over the
      loaded corpus (seed tab, seed, bars, variants/strategy, gesture) and browses
      the **reranked set** — rank, strategy, aggregate, the six rerank axes on
      hover, note count — with a click painting that candidate into the roll and
      **keep** writing its `.mid` plus a provenance sidecar (source, ask,
      strategy, variant seed, rank, aggregate, axes) that reproduces it exactly.
      Rank 1 is the candidate `griff generate` writes: the panel enters the same
      `griff_core::generation_input::ranked_candidates` the CLI does — the
      compiler moved into `griff-core` for exactly this, and the move is proven
      output-identical (30/30 byte-identical `griff generate` runs).
      Remaining: register/playability/tonal hypotheses, S7 path explanations.
- [~] **Feedback/evolution surface** — **Slice 3 landed 2026-07-16** (PR
      pending review): favorite/reject controls (mutually exclusive) and a
      session **history** of every auditioned candidate with typed provenance,
      in `griff_ui_core::history` (append-only `SessionHistory`, stable
      `HistoryId`, `Verdict` toggle, generator-split `Provenance`); the cockpit
      records on show and replays a history snapshot through the Slice 2
      transport (`AuditionCandidate::History`). Session-local + in-memory; no
      ranking/learning/persistence. Remaining for S9: parent/child lineage and
      preference/evolution semantics. S8 displays and edits; S9 owns evolution.
- [ ] Curation actions feeding the S5 corpus schema — **first slice landed
      2026-06-11**: approve/reject intents in the interaction core
      (`Viewport::decision`, ADR-0016 — repeat to undo), 'a'/'x' keys and a
      pending-decision line in the TUI inspector, and
      `griff-preview --record=<chunk.json>` persisting the decision into the
      record's `reviewer` field on quit (`curation::decide_record`).
      **Second slice landed 2026-06-11**: the inspector surfaces the loaded
      record's current state — title, prior reviewer decision, tags — via
      `curation::summarize_record` (schema wire names, UI-level strings).
      **Third slice landed 2026-06-12 (tag)**: 't' cycles the palette
      (`curation::tag_palette`, mirrors `SwancoreTag::all_variants`), 'T'
      toggles the cursor's tag, the record block shows the live set, and
      quit persists the changed set via `curation::set_tags` alongside the
      decision. Tag state crosses the interaction core as plain integers
      (cursor + bitmask). **Fourth slice landed 2026-06-12 (rename)**: 'r'
      opens a buffer seeded with the live title (text stays
      frontend-local; the core keeps only the renaming flag), Enter
      commits, Esc cancels, quit persists via `curation::rename_record`
      (trimmed, never blank). **Fifth slice landed 2026-06-12
      (split/merge)** — the curation action set is complete: 's' pins a
      split to the playhead (the shell floors it to the containing source
      bar; the record file keeps the first half, the first vacant `.N`
      sibling takes the second — never over an existing record), 'm' arms
      a merge with the `--merge=PARTNER_JSON` record
      (same source, consecutive bar ranges; the absorbed partner file is
      removed). Both reset the reviewer and the whole-extent measurements
      — see the 2026-06-12 split/merge decision.
- [~] Boundary overlays (S4) and candidate history — **overlays landed
      2026-06-11**: `Analysis.boundaries` carries the S4 start ticks under a
      PPQN-scaled default config, the scene places `BoundaryMark` columns
      (sections keep precedence on shared columns), the TUI styles them.
      **Candidate history landed 2026-07-16** (S8 Slice 3): the cockpit's
      session `SessionHistory` window (`y`) — a newest-first feed with
      provenance and favorite/reject.
- [x] Scrollable inspector dock (2026-06-11): `Viewport.inspector_scroll`
      steps via `InspectorScrollUp/Down` (PgUp/PgDn in the TUI), hiding the
      dock resets it, renderers clamp to their own content overflow — the
      follow-up the PR #38 liveness decision deferred.

## Goal

A standalone desktop app to view, listen, compare, and hand-annotate — before
the CLAP plugin, to debug transport/slicing/graph without DAW quirks.

## Inputs / Outputs

- In: `.mid` / corpus chunks / candidates.
- Out: piano-roll/tab view, MIDI playback, boundary overlays, candidate history,
  score/provenance inspectors, and approve/reject/split/merge/rename/tag actions
  feeding the corpus.

## Approach

- New workspace member `preview/` using `eframe`/`egui` (immediate-mode,
  native; not Tauri — IPC/HTML overhead for an offline MIDI tool).
- Playback via `midir`. Headless fallback: a `ratatui` TUI.
- Text is an editable request/fixture surface, not a replacement for the typed
  canonical model.

## Acceptance criteria

- Loads a `.mid`, shows a piano-roll, plays it back.
- Curation actions persist into the S5 corpus schema.
- Candidate and provenance views use stable ids and headless-testable view
  models.
- S7/S9/S15 data is displayed without reimplementing their inference or policy
  in the UI.

## Open questions

- Playback engine details on each OS.
- Minimal textual request syntax before the S15 fixture DSL exists.

## See also

- [`../audit/2026-07-symbolic-harmony-and-evolution-research.md`](../audit/2026-07-symbolic-harmony-and-evolution-research.md)
- [`S7-graph-layer.md`](S7-graph-layer.md)
- [`S9-feedback-layer.md`](S9-feedback-layer.md)
- [`S15-tonal-context-and-harmonic-control.md`](S15-tonal-context-and-harmonic-control.md)
- [`../glossary.md`](../glossary.md) §11
