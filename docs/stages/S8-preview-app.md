# S8: Standalone preview app

Status: in progress — interactive `ratatui` piano-roll landed (slices 1–3)
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
      **S7 path explanations landed 2026-07-17** — see *Global Chain Audition*
      below. Remaining: register/playability/tonal hypotheses.
- [~] **Global Chain Audition** — **landed 2026-07-17**. The S7 A/B core
      (ADR-0013 as amended by ADR-0030) was headless; this makes it audible.
      After a Generate, the panel offers two audition variants from **one**
      ranked set: **S6 Intact** (ranked candidate 0, the whole candidate S6 put
      first) and **S7 Global Chain** (one candidate per bar, chosen for the
      sequence). Not "original" and "alternative": a user cannot choose between
      two things they cannot tell apart.
      - **S6 stays the default.** Generate still shows the intact winner; the
        chain is an explicit second thing to ask for, never a substitution.
      - **One immutable run.** `griff_ui_core::generate::generate_run` calls
        `ranked_candidates` once and plans the chain from that same live
        `RankedSet`, which then dies inside the function. The cockpit never holds
        a `RankedSet` or the planner, so re-planning on an A/B switch, an export,
        or a history open is unrepresentable rather than merely discouraged.
      - **Refusal is a typed, run-level outcome.** `plan_candidate_chain` can
        refuse a set (a candidate disagreeing about the timeline, material
        crossing a bar line); that is not a failed Generate.
        `GlobalChainOutcome` is not a `Result`, so a `ChainError` cannot become a
        generation error. `SessionHistory::record_chain` keys the outcome to the
        `GenerationRunId` — append-only, first write wins — so the reason a run
        had no chain outlives the run. No fake chain entry holding the intact
        winner's score: a refusal has no score, and an entry needs one.
      - **A/B is the Slice 2 stack, unchanged.** Both variants route through
        `show_score`, inheriting All-Notes-Off, the rebuilt voice, the held
        playhead, the loop remap and the tempo map. Both stand on the master
        timeline every candidate already agreed on — which is why a loop over
        bar 2 is the same bar 2 in either.
      - **Export writes the captured snapshot** through
        `griff_core::midi::export_score`, from the history entry, never the
        active run — and never re-plans. `keep` does not audition: exporting is a
        file action and must not decide what the user is listening to.
      - **Explanations are the core's**, projected by `global_chain_summary`
        from the run's record: both costs, a signed delta, one row per output bar
        naming the supplier's ordinal *and* its distinct rank, strategy and
        variant seed, with the chain-local and S6 rationales. Bars read 1-based;
        the core counts from 0 and the projection is the only place that
        translates. An unmeasured boundary jump shows as **not measured**, never
        `0` — the core omits the axis so it cannot read as perfect continuity.
      - **The sidecar is the cockpit's wire contract** (`KeptChain`), mirrored
        rather than derived onto the backend-neutral history model, which stays
        free of serialisation.
      - Deliberately **not** here: any claim that the chain sounds better (the
        delta says "lower under `candidate_chain` v1", which is a fact about the
        policy), k-best or Slice C, weight tuning or sliders, S9 learning, S15
        harmony, and S17 rendering.
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
