# ADR 0002: Adopt a canonical score model as the internal representation

Date: 2026-05-19
Status: Accepted

## Context

The current model is linear: `Phrase -> Bar -> Vec<Event>` with
`Event::{Note,Rest}`. It has no explicit layer for voices, chords,
simultaneity, or event groups, so polyphony and Guitar Pro semantics cannot be
represented. Mature notation models (alphaTab, MusicXML) are two-dimensional
(time × parts) with a score-level bar.

## Decision

We adopt a canonical score model `Score -> MasterBar -> Track -> Voice ->
EventGroup -> AtomEvent`, plus `SourceMeta` for format-specific data
(string/fret, technique evidence, importer warnings). Existing
`Phrase/Bar/Event` survives during transition as a compatibility layer /
projection, not as the final model.

## Consequences

- Enables polyphony, chords, tempo map, Guitar Pro effects, and group-aware
  generation.
- Larger model and migration cost; monophonic APIs become projections, not the
  truth. Introduced in S1; MIDI moves onto it in S2.
