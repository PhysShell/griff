# S5: Corpus and annotation schema

Status: done
Depends on: S4
ADRs: ADR-0005

## Goal

A managed micro-corpus (20+ hand-curated chunks) plus an annotation schema to
test slicer, boundary detector, similarity, and the first generator.

## Inputs / Outputs

- In: source MIDI/GP (private), S4 boundaries.
- Out: `ChunkMeta` schema, a manifest, tiny test fixtures, curation tooling.

## Approach

- Schema fields: id, source ref/format, title, tempo map, time sig, tuning,
  tags (swancore taxonomy), phrase boundaries, techniques, quality flags,
  reviewer decisions, timestamps.
- Curation via CLI (`griff curate <midi>` — events shown, tags asked). UI is
  S8.
- Corpus content is **not** committed; `corpus/` is git-ignored. Only schema,
  manifest, tooling, and minimal fixtures live in git (ADR-0005 / licensing).

## Acceptance criteria

- 20+ chunks, ≥ 4 bands, each with ≥ 2 tags.
- Schema JSON round-trips losslessly (property test).
- A documented `corpus-sources` note (private), licensing addressed.

## Open questions

- Final swancore tag taxonomy (merge with prior notes, do not replace).
- Reviewer-decision vocabulary.

## See also

- [`../glossary.md`](../glossary.md) §1 (PhraseChunk), §10
