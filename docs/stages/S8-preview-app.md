# S8: Standalone preview app

Status: in progress — landed: the interactive `ratatui` piano-roll
(slices 1–3), the egui cockpit surface (ADR-0027), Generate with
candidate provenance and session history, the accepted Slice 2 playback
stack, Swang Playground Slice 3 (favorite / reject / history / provenance,
PR #126), and Global Chain Audition (PR #129).
Remaining: tracked in the progress notes and checklist below
Depends on: S6
ADRs: ADR-0034 (Generator Observatory contract)

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
- [~] **In-cockpit playback** — the accepted **Swang Playground Slice 2
      transport** (2026-07-16) auditions candidates inside the cockpit
      (`show_score → focus_on_track`; All-Notes-Off, loop remap, tempo, and
      playhead inherited), and Global Chain Audition reuses that stack
      unchanged. Remaining: optional `midir` system-MIDI-out routing (DAW /
      hardware synth); the Generate panel's `open` still hands a kept `.mid`
      to whatever the OS has registered for it.
- [~] **Textual playground** — landed as the **Swang Playground**, Slices 1–3
      (program editing, the Slice 2 transport, Slice 3 favorite / reject /
      history / provenance — PR #126). Remaining: editable generation
      request/constraint text with live parse diagnostics beyond Swang
      programs. Future S15 harmonic fixture scripts may be edited here, but
      typed core structures remain the source of truth.
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
- [~] **Generator Observatory** — **contract accepted 2026-09-17**
      ([ADR-0034](../adr/0034-experiment-axes-and-identities.md)). Global Chain
      Audition generalised into two experiment axes, *algorithm variant* ×
      *information regime*.
      - **In-memory runner landed** (`griff-experiment`): typed variants per
        pipeline stage, regimes that mask the channels of an already
        prepared population, one generation pass per regime shared by
        every variant that can share it, and typed cell refusals.
      - **Identities.** Separate spec, population, pass-information,
        evaluation, requested-cell and effective-recipe identities.
      - **Metrics.** Deltas only between identical metric identities, and
        interactions only over evaluations.
      - **Core seam.** `ranked_candidates` now wraps
        `ranked_candidates_from_view`, pinned output-identical to main.
      - **Unchanged boundary.** Holdout and population selection stay with
        the Reachability Lab (ADR-0032).
      - **Persistent bundle landed** (`ExperimentBundleV1`).
        - **One projection.** The canonical semantic projection V1 is the
          only wire form, and every fingerprint walks it; the pinned v1
          goldens were reproduced unchanged.
        - **Complete record.** The whole spec, the source, the population
          identity, every pass, and both identities of every cell.
        - **Loading.** Parses strictly, validates every projection
          through the model's constructors, and recomputes every identity
          with typed mismatches. It never generates.
        - **Lossless.** `run()` rebuilds the in-memory run exactly.
        - **Characterization, not a contract.** On one repository-corpus
          source, 16 cells wrote a 7.9 MB pretty JSON bundle that loaded
          and verified in about 11 ms in release.
      - **Hardening (C4b).** Writing fails closed. Every displayed fact is
        bound to an identity: `corpus-snapshot.v2`, plus pass, cell and run
        records kept apart from the causal identities. The only sealing
        path is crate-private.
      - **Cockpit Observatory landed** (`o`, "🔬 observatory"). One display
        path: `ExperimentView` is built from a bundle and nothing else.
        - **Run and Open.** A native Run is written down as a bundle before
          anything is shown, and a saved bundle opens through the same
          verification. Their views are equal.
        - **A and B.** Pick A and B by variant and regime. Requested,
          effective and actual stay side by side and apart.
        - **Numbers.** Metric rows and the 2 × 2 interaction are
          `delta()` / `interaction()` verbatim. A comparison without a
          number says why ("not comparable: different measurement
          context").
        - **Audition.** Plays recorded scores through the Slice 2 transport
          (`AuditionCandidate::Experiment`), and `b` swaps them. Nothing
          regenerates or re-plans; a refused cell is shown as its refusal
          and never played.
        - **Web.** Opens and auditions bundles ("🔬 Bundle"); running stays
          native.
        - **Untouched.** The Generate panel, history and TAB.
      - **Next increments.**
        1. Retiring the manual `s6_candidate_set` / `intact_top` identities
           into core.
        2. The full variant × channel matrix and the corpus explorer (typed
           queries first).
- [~] **Feedback/evolution surface** — **Slice 3 landed 2026-07-16 and merged
      as PR #126**: favorite/reject controls (mutually exclusive) and a
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
