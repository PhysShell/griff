# Audit: stage-label reconciliation (2026-05)

Status: accepted
Decision: keep git history, reconcile via this doc, use canonical numbering
from now on (see [`../glossary.md`](../glossary.md) §0).

## Why this doc exists

Commits were made before `glossary.md` became the constitution. Their `feat(sN)`
labels were improvised and **do not match** the canonical roadmap. The
glossary preamble forbids papering over this. Git history is **not** rewritten
(consistent with S0: do not change observable history/behavior); instead, the
true mapping is recorded here and canonical numbering is mandatory going
forward.

## Canonical roadmap (authoritative)

`S0` baseline/characterization · `S1` canonical score model · `S2` MIDI
transport refactor · `S3` Guitar Pro import · `S4` phrase boundary detection ·
`S5` corpus + schema · `S6` rule generator v0 · `S7` graph layer · `S8`
preview app · `S9` feedback layer · `S10` CLAP MVP · `S11` region
regeneration · `S12` neural assistance · `S13` complementary part generation ·
`S14` structure controls and metrics · `S15` tonal context and harmonic control.

S13 and later stages are append-only additions. Their logical dependency order
may place them beside or before an earlier-numbered future stage; existing stage
numbers are never repurposed or renumbered to make the list look prettier.

## Mislabel mapping

| Commit | git label | Canonical reality |
|---|---|---|
| `5adc821` | `feat(s0)` bootstrap workspace skeleton | Workspace bootstrap. NOT canonical S0. True S0 (characterization tests/golden snapshots) is **not done**. |
| `66a24d3` | `feat(s4)` generate repeating phrases | A deterministic generation primitive. Generation is canonical **S6**, not S4. S4 is phrase boundary detection. |
| `3367cee` | `feat(s1)` MIDI import/export | Pre-canonical MIDI baseline adapter. Canonical **S1** is the score model; the MIDI **transport refactor** onto it is **S2**. |
| `f846029` | `feat(s2)` bar-level classification | A feature helper loosely supporting S4 analysis. **Not** a standalone canonical stage; canonical S2 is the MIDI transport refactor. |

## True state of the code

All of `core/src/{event,feature,generate,midi,slice,classify}.rs` + the `cli`
commands `import/inspect/export/classify` constitute the **pre-canonical
baseline**. It is the input to:

- **S0** — write characterization tests / golden snapshots / roundtrip
  baseline over this exact behavior, without changing it.
- **S1** — introduce `Score/MasterBar/Track/Voice/EventGroup/AtomEvent/
  SourceMeta` alongside it behind a compatibility layer.

## Rules from here

1. New commits use canonical stage numbers per the glossary and this roadmap.
2. A stage is "closed" only when its acceptance criterion (its stage doc) is
   met, tested, and documented.
3. Earlier mislabeled work is **not** re-closed under its old number; relevant
   pieces are re-credited to their true stage in the stage docs' "See also".
4. New cross-cutting stages take the next free number. Existing S-numbers are
   never reassigned, even when the logical execution order differs.

## See also

- [`../glossary.md`](../glossary.md) §0, §17
- [`../SPEC.md`](../SPEC.md)
- [`../stages/S0-baseline-and-tests.md`](../stages/S0-baseline-and-tests.md)
- [`../stages/S15-tonal-context-and-harmonic-control.md`](../stages/S15-tonal-context-and-harmonic-control.md)
