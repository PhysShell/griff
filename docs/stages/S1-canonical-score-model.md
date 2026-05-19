# S1: Canonical score model

Status: planned
Depends on: S0
ADRs: ADR-0002

## Goal

Introduce the canonical score model without breaking public behavior.

## Inputs / Outputs

- In: frozen baseline + S0 characterization tests.
- Out: `Score`, `MasterBar`, `Track`, `Voice`, `EventGroup`, `AtomEvent`,
  `TechniqueSpan`, `SourceMeta`; a compatibility layer keeping
  `Phrase/Bar/Event` usable.

## Approach

- Add new types alongside the old ones; do not delete the linear model yet.
- `MasterBar` owns tempo/meter/repeats/markers/tick range.
- `Voice`/`EventGroup` carry polyphony, chords, arpeggio/strum/tuplet/grace.
- `SourceMeta` holds string/fret, technique evidence, importer warnings.
- Provide `Phrase`/`Bar` as a projection/view over the new model.

## Acceptance criteria

- New types compile, are unit-tested, and have rustdoc.
- All S0 characterization tests stay green (no behavior change).
- A documented mapping old ↔ new exists; nothing forces callers to migrate
  yet.

## Open questions

- `AtomEvent` final name (glossary §18 allows the name to change with an ADR).
- Where ties/let-ring live: `AtomEvent` vs `EventGroup`.

## See also

- [`../adr/0002-canonical-score-model.md`](../adr/0002-canonical-score-model.md)
- [`../glossary.md`](../glossary.md) §1
