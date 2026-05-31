# Audit: canonical roadmap extension — S13 (2026-05)

Status: proposed
Decision: extend the canonical roadmap with **S13 (complementary part
generation)** by appending the next free stage number, not by renumbering
S7…S12 (see [`../glossary.md`](../glossary.md) §0).

## Why this doc exists

The canonical roadmap was `S0…S12` (glossary §0). ComplementArranger (ADR-0012)
is a new vertical slice that did not exist in that list, so the roadmap is being
extended. The glossary preamble requires extending the constitution
deliberately and in the open rather than improvising a label — this doc records
that act.

## What changed

- New stage **S13 — Complementary part generation**, added to glossary §0 and
  [`../stages/S13-complementary-part-generation.md`](../stages/S13-complementary-part-generation.md).
- New ADRs: **0011** (retire legacy linear model) and **0012**
  (ComplementArranger).

## Numbering rationale (append vs renumber)

S13 logically belongs *between* the single-part generator (S6) and the graph
layer (S7): it depends on S6 and the graph (S7) later learns complement
relations from the corpus. We nevertheless give it the next free number, S13,
rather than inserting it as a physical "S7" and shifting S7…S12 up by one,
because:

- Append-only is the project's established posture (the stage-label
  reconciliation kept git history rather than rewriting it; ADRs are immutable
  and append-only).
- Renumbering would rename six stage files (`S7…S12-*.md`) and rewrite their
  cross-references — exactly the churn the project avoids.

Dependency order, not the integer, is authoritative: S13's stage doc carries
`Depends on: S6` and notes the S7 synergy. If a physical re-slot is preferred,
it is a separate, deliberate renumbering change.

## Updated canonical roadmap

`S0` baseline/characterization · `S1` canonical score model · `S2` MIDI
transport refactor · `S3` Guitar Pro import · `S4` phrase boundary detection ·
`S5` corpus + schema · `S6` rule generator v0 · `S7` graph layer · `S8`
preview app · `S9` feedback layer · `S10` CLAP MVP · `S11` region
regeneration · `S12` neural assistance · **`S13` complementary part generation
(depends on S6; precedes graph-mined relations in S7)**.

## See also

- [`2026-05-stage-label-reconciliation.md`](2026-05-stage-label-reconciliation.md)
- [`../glossary.md`](../glossary.md) §0
- [`../adr/0012-complementary-part-generation.md`](../adr/0012-complementary-part-generation.md)
