# SPEC.md — griff

One page: what `griff` is, what it is not, and the rules that do not bend.
Term definitions live in [`glossary.md`](glossary.md). Decisions live in
[`adr/`](adr/). Stage detail lives in [`stages/`](stages/).

## What griff is

`griff` is a **swancore-first guitar riff engine**. It analyzes, slices,
generates, and regenerates guitar parts as a **structured symbolic model**,
not as audio. Input is symbolic (MIDI now; Guitar Pro later); output is
symbolic (MIDI now). The long-term delivery target is a **MIDI-oriented CLAP
plugin** with human-in-the-loop curation.

## What griff is not

- Not an audio synthesizer or audio-generation tool.
- Not a general-purpose "any genre" riff generator. Swancore-first by
  decision (ADR-0005).
- Not a neural music generator — at least not until a corpus and a working
  rule-based baseline exist (glossary §17.5).
- Not a Guitar-Pro-articulation oracle reconstructed from plain MIDI
  (glossary §17.3).

## Delivery shape

Strict staged delivery, `S0 … S13`, defined canonically in
[`glossary.md`](glossary.md) §0 and detailed in [`stages/`](stages/). Each
stage is a vertical slice with a measurable acceptance criterion. Stages are
implemented in order; library groundwork may land earlier but does not "close"
a later stage until its acceptance criterion is met and documented. The
roadmap is extended by appending the next free stage number (see
[`audit/2026-05-s13-complementary-arranger.md`](audit/2026-05-s13-complementary-arranger.md)),
never by renumbering existing stages.

## Hard rules (do not violate without a superseding ADR)

1. **MIDI is a boundary, not the model.** Raw MIDI bytes live only in the
   import/export adapter. Everything else uses the structured model
   (glossary §17.1).
2. **`unsafe_code = "forbid"`** workspace-wide (ADR-0004).
3. **Master timeline is the single source of truth** for tempo / meter / bar
   positions. Export builds from it, not from a track (ADR-0003).
4. **Canonical score model is the target** (`Score → MasterBar → Track →
   Voice → EventGroup → AtomEvent`); the linear `Phrase/Bar/Event` becomes a
   compatibility layer / projection (ADR-0002).
5. **Refactors start with characterization tests.** No observable behavior
   change without a red test first (glossary §17.7).
6. **Generation is deterministic under a fixed seed** (glossary §17.8).
7. **Format adapters emit a loss report** (glossary §17.6).
8. **Swancore-first defaults**: Standard E tuning (ADR-0006), swancore chord
   vocabulary and rhythm grid. Generic-metal features need an ADR-0005 update.
9. **Strict lint policy.** `cargo fmt`, `cargo clippy --all-targets -D
   warnings`, and the full test suite must be green at every stage boundary.
10. **Repository text is English.** Glossary, specs, ADRs, stage docs, code,
    comments, commit messages — English only.
11. **Fuzzing is a mandatory robustness layer.** External format adapters
    (MIDI, Guitar Pro, …) and selected canonical transformations must have
    fuzz targets per ADR-0010 and [`fuzzing.md`](fuzzing.md). Bounded smoke
    fuzzing plus the regression corpus is a blocking CI gate; deep fuzzing
    runs scheduled and non-blocking.

## Current state (pre-canonical baseline)

The workspace (`core`, `cli`, `plugin`) exists with a simple linear model
(`Event::{Note,Rest}`, `Bar`, `Phrase`), a MIDI import/export adapter, and
simple `feature` / `generate` / `slice` / `classify` helpers. This is the
**pre-canonical baseline** — the input to S0 (freeze it) and S1 (introduce the
canonical model alongside it). Earlier commit stage labels predate this spec
and are reconciled in
[`audit/2026-05-stage-label-reconciliation.md`](audit/2026-05-stage-label-reconciliation.md).

## See also

- [`glossary.md`](glossary.md) — the constitution.
- [`adr/README.md`](adr/README.md) — decision index.
- [`fuzzing.md`](fuzzing.md) — fuzz-testing policy (ADR-0010).
- [`stages/`](stages/) — S0 … S13.
- [`decisions.log.md`](decisions.log.md) — small decisions, append-only.
