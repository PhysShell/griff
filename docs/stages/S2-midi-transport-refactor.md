# S2: MIDI transport refactor

Status: planned
Depends on: S1
ADRs: ADR-0003, ADR-0010

## Goal

Move MIDI import/export onto the canonical model and the master timeline.

## Inputs / Outputs

- In: canonical score model (S1), frozen MIDI behavior (S0).
- Out: importer populating `Score.master_bars`; exporter building the meta
  track only from the master timeline; a `LossReport`.

## Approach

- Import: build a global tempo map / `MasterBar` set, not per-bar transport.
- Export: derive tempo/time-signature from the master timeline; tracks never
  re-define transport.
- SMPTE staged: (1) fail-fast with a precise reason + `normalize-timing`
  hint, (2) read-only conversion to absolute transport, (3) export support —
  later, behind a flag.
- Emit `LossReport` for anything approximate or dropped.
- Promote `midi_roundtrip` to P0 normalized invariants on the canonical
  model (bar-duration validity, pitch/velocity ∈ 0..=127, no reversed
  ranges, no duration overflow); add `score_projection` (P1) here or at S1.
- Resolve finding F-001 if still open: reject `bar_ticks == 0` with a typed
  `MidiError` instead of looping (characterization test first).

## Acceptance criteria

- S0 roundtrip/characterization tests stay green via the compatibility layer.
- Multi-track export uses master-timeline transport (regression test).
- A real guitar `.mid` imports and re-exports without losing bar alignment.
- `LossReport` is produced and asserted in tests.
- `midi_roundtrip` normalized invariants hold under fuzzing; F-001 seed no
  longer hangs (typed error) and stays in the regression corpus.

## Open questions

- How far to take SMPTE in this stage vs deferring levels 2–3.

## See also

- [`../adr/0003-master-timeline-single-source-of-truth.md`](../adr/0003-master-timeline-single-source-of-truth.md)
- [`../glossary.md`](../glossary.md) §2, §3
- [`../fuzzing.md`](../fuzzing.md) (F-001, `midi_roundtrip`)
