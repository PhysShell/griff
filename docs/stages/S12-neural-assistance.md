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

## Preconditions (hard gate)

- Corpus ≥ ~100 phrases; working S6 baseline; S9 feedback integration.
- Not started before all three hold (glossary §17.5).

## Acceptance criteria

- Neural suggestions beat S6/S7 on the quality score in a blind comparison.
- Deterministic given a fixed seed and model checkpoint.

## See also

- [`../glossary.md`](../glossary.md) §8, §17.5
