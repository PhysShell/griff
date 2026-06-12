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
  reviewer decisions, timestamps, rights info (v7 — see below).
- Curation via CLI (`griff curate <file>` — events shown, tags asked). UI is
  S8. Both MIDI and GP sources must be supported before mass curation begins
  (see pre-curation requirements below).
- Corpus content is **not** committed; `corpus/` is git-ignored. Only schema,
  manifest, tooling, and minimal fixtures live in git (ADR-0005 / licensing).

## Pre-curation requirements (must land before mass curation)

Two schema/tooling items are non-derivable from notes and therefore must be in
place before the first production curation session:

1. **Rights field (`RightsInfo`, schema v7)** — `rights_status` /
   `acquisition` / `redistributable` / `notes`. Cannot be reconstructed from
   note content; backfill cost scales linearly with corpus size. For scraped
   community tabs: `CopyrightedComposition` / `CommunityTabSite` /
   `redistributable: false`. Decision: decisions.log 2026-06-12 (rights entry).

2. **GP ingest in `griff curate`** — the CLI currently dispatches only to
   `midi::import_score` / `SourceFormat::Midi`. If any source files are Guitar
   Pro, curating them via MIDI conversion silently loses string/fret/technique
   data and misrecords `SourceFormat::Midi`. Wire `gp::import_score` with
   extension dispatch before the first GP curation session. Decision:
   decisions.log 2026-06-12 (GP curate wiring entry).

## Acceptance criteria

- 20+ chunks, ≥ 4 bands, each with ≥ 2 tags.
- Schema JSON round-trips losslessly (property test).
- A documented `corpus-sources` note (private), licensing addressed.
- Every chunk has a `RightsInfo` record (no `None` in a production corpus).

## Open questions

- Final swancore tag taxonomy (merge with prior notes, do not replace).
- Reviewer-decision vocabulary.

## See also

- [`../glossary.md`](../glossary.md) §1 (PhraseChunk), §10
