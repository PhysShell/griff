# ADR 0011: Retire the legacy linear model in favour of the canonical model

Date: 2026-05-31
Status: Proposed

## Context

ADR-0002 introduced the canonical score model (`Score → MasterBar → Track →
Voice → EventGroup → AtomEvent`) and kept the legacy linear model
(`Phrase → Bar → Event::{Note,Rest}`, in `core/src/event.rs`) as a transitional
compatibility layer / projection, reachable via `score::project_phrase`.

A dependency audit (2026-05-31) shows the legacy model is **not** confined to
tests; it is on live production paths:

- `cli` commands `import` / `inspect` / `classify` / `curate` run through
  `midi::import` → `MidiSong`/`MidiTrack { phrase: Phrase }` and read
  `track.phrase.bars` — the canonical `midi::import_score` → `Score` exists but
  the CLI does not call it.
- `generate.rs` returns `GenerationCandidate { phrase: Phrase }` — generation is
  entirely legacy.
- `classify.rs` (`bar_features(&Bar)`), `feature.rs` (`phrase_features(&Phrase)`)
  and `slice.rs` operate on the legacy types.

Already canonical-native: `boundary.rs`, `gp.rs` (Guitar Pro imports straight to
`Score`), `score.rs`. Dual-mode: `midi.rs` (both `import` and `import_score`).

The shared value primitives (`Pitch`, `Ticks`, `Velocity`, `TimeSignature`,
`Tempo`, `Articulation`, `ValidationError`) also live in `event.rs` but are used
by both models; deleting the legacy *types* while keeping the *primitives*
breaks nothing in the canonical model.

The trigger is ADR-0012 (ComplementArranger): a complementary-part engine is
inherently multi-track and technique-aware, so it must live in the canonical
model. That forces a directional decision about the legacy model rather than
another ad-hoc bridge.

## Decision

We retire the legacy linear model (`Phrase`, `Bar`, `Event`, `Note`, `Rest`) as
an internal representation. The canonical model is the single internal truth.
Retirement is **staged, not a single cut**, to respect the "characterization
tests first, no silent behaviour change" rule (SPEC §5, glossary §17.7):

1. **Now (with S13).** Port `feature` and `generate` onto the canonical model —
   feature extraction operates on a `Voice`/`Track`; generation emits
   `Track`/`Voice`, not `Phrase`. These two modules have the fewest external
   consumers (the generator is not wired into the CLI at all), so the port is
   low-risk and is what ComplementArranger needs.
2. **Later.** Port `classify`, `slice`, the CLI import/inspect/classify path,
   and the legacy `midi::import`/`MidiSong` branch onto the canonical model,
   each behind characterization tests over current observable behaviour.
3. **Finally.** Remove the legacy types and `project_phrase` once no production
   path depends on them.

The shared primitives stay where they are (or move to a `units` module later);
this ADR does not touch them. Each step is its own red→green change; this ADR
records direction and order, not a single migration commit.

## Consequences

- ComplementArranger (ADR-0012) is built on a model that can express multiple
  technique-aware tracks sharing one master timeline — no legacy bridge needed.
- The dual-model ambiguity (`import` vs `import_score`, `Phrase` vs `Voice`)
  shrinks step by step and eventually disappears.
- Accepted: an interim period where some modules are canonical and others are
  still legacy; `project_phrase` survives until step 3.
- Accepted: porting the CLI import path and `classify`/`slice` is non-trivial
  and is explicitly deferred, not done here.
- Accepted: golden CLI snapshots may need re-blessing when the CLI moves onto
  the canonical model (step 2), gated by characterization tests first.
