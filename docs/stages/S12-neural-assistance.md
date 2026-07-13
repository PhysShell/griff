# S12: Neural assistance (far future)

Status: planned (deferred; hard preconditions)
Depends on: S6 baseline + S5 corpus ≥ ~100 phrases + S9 feedback loop
ADRs: ADR-0008 (spirit: heuristics/baseline before ML)

## Goal

A neural layer for continuation / infilling / variation / ornamentation /
articulation suggestion over an existing musical context — never "make a track
from scratch".

## Inputs / Outputs

- In: an existing canonical-model context + constraints.
- Out: suggested continuations/infills as candidates, fed through the same
  S9 reranking.

## Approach (references, not commitments)

- Tokenization: REMI / Compound Words / MIDI-Like.
- Models suited to fixed-region control / infilling (e.g. anticipatory /
  masked-LM style on MIDI tokens).
- Inference in Rust via `candle` or ONNX (`ort`); training offline.
- Harmonic-analysis research such as AugmentedNet is useful for **task
  decomposition** (tonal centre, mode, chord root, quality, inversion) and
  synthetic labelled examples, not as a Phase-1 runtime dependency.
- Symbolic generation projects such as MiniBach remain research references only;
  they do not replace the S6/S7 deterministic baseline or S15 calibrated tonal
  context.

## Preconditions (hard gate)

- Corpus ≥ ~100 phrases; working S6 baseline; S9 feedback integration.
- Not started before all three hold (glossary §17.5).
- Any harmonic neural proposal must also compare against the accepted S15
  symbolic estimator/fixtures and preserve uncertainty/abstention rather than
  emit one unquestioned label.

## Acceptance criteria

- Neural suggestions beat S6/S7 on the quality score in a blind comparison.
- Deterministic given a fixed seed and model checkpoint.
- Harmonic assistance, when attempted, beats or complements the S15 symbolic
  baseline on a labelled test set and reports calibrated uncertainty.

## Non-goals

- No Python/TensorFlow/MusicXML service in the core runtime merely because a
  research implementation uses that stack.
- No rendering symbolic input to audio/chroma and guessing its lost semantics
  back while native symbolic evidence is available.

## See also

- [`../audit/2026-07-symbolic-harmony-and-evolution-research.md`](../audit/2026-07-symbolic-harmony-and-evolution-research.md)
- [`S9-feedback-layer.md`](S9-feedback-layer.md)
- [`S15-tonal-context-and-harmonic-control.md`](S15-tonal-context-and-harmonic-control.md)
- [`../glossary.md`](../glossary.md) §8, §17.5
