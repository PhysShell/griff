# S3: Guitar Pro import

Status: done
Depends on: S1, S2
ADRs: ADR-0002, ADR-0003, ADR-0010

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
- Add the P0 `guitar_pro_import` fuzz target (ADR-0010) from day one: GP
  parsers are the highest-risk adapter. Oracle: no panic / hang / zip-bomb
  / unbounded alloc; unsupported constructs → typed warnings + `LossReport`.

## Acceptance criteria

- GP3/4/5 fixtures import into the canonical model with string/fret/technique
  preserved.
- Every import emits a `LossReport`.
- Support matrix documented (stable vs experimental, read-only).
- `guitar_pro_import` fuzz target builds, has a minimal `.gp3/.gp4/.gp5/
  .gpx` seed corpus, and runs in the blocking smoke gate.

## Open questions

- GP6–8 route: pure Rust vs sidecar vs hybrid.
- MusicXML fallback: in scope for S3 or deferred.

## See also

- [`../glossary.md`](../glossary.md) §4, §5
- [`../fuzzing.md`](../fuzzing.md) (`guitar_pro_import`, P0)
