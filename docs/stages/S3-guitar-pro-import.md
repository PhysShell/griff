# S3: Guitar Pro import

Status: planned
Depends on: S1, S2
ADRs: ADR-0002, ADR-0003

## Goal

Add Guitar Pro as a first-class input, preserving guitar semantics.

## Inputs / Outputs

- In: canonical score model; GP files.
- Out: GP → canonical importer + `LossReport`; `SourceMeta` carrying
  string/fret/technique as source-of-truth.

## Approach

- MVP: GP3/GP4/GP5 read-import (binary).
- Next: GP6 `.gpx` read-import.
- Experimental, flag-gated: GP7/GP8 `.gp`.
- Candidate routes: `guitarpro` crate (Rust-native), PyGuitarPro sidecar
  (GP3–5), GPIF-based for GP6–8. MusicXML as an open interchange fallback.
- Articulations from GP are source-of-truth (not inferred).

## Acceptance criteria

- GP3/4/5 fixtures import into the canonical model with string/fret/technique
  preserved.
- Every import emits a `LossReport`.
- Support matrix documented (stable vs experimental, read-only).

## Open questions

- GP6–8 route: pure Rust vs sidecar vs hybrid.
- MusicXML fallback: in scope for S3 or deferred.

## See also

- [`../glossary.md`](../glossary.md) §4, §5
