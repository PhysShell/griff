# S7: Graph layer (late)

Status: done
Depends on: S6 acceptance
ADRs: —

## Goal

Graph-driven recombination over phrases, motifs, and rhythm cells — only after
stable chunks and features exist.

## Inputs / Outputs

- In: corpus chunks + features (≥ ~100 phrases recommended before this pays
  off).
- Out: a graph (nodes + weighted edges) and traversal producing candidate
  chains.

## Approach

- Node types: `Phrase`, `Motif`, `RhythmCell`, `ChordMovement`,
  `EnergyState`.
- Edges: similarity (cosine on feature vectors), transition probability
  (counted from corpus), tag co-occurrence.
- Traversal: weighted random walk + beam search.

## Acceptance criteria

- Recombined chains beat S6 single-strategy output on a defined quality score.
- Deterministic for a fixed seed.

## Open questions

- Minimum corpus size before the graph beats rule-based v0.

## See also

- [`../glossary.md`](../glossary.md) §9
