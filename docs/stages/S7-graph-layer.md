# S7: Graph layer (late)

Status: in progress — first slice (chunk similarity edge) landed (2026-06-10);
the full graph stays deliberately late
Depends on: S6 acceptance
ADRs: ADR-0013 (DP/Viterbi traversal), ADR-0018 (rich note model: fretboard +
multi-technique with evidence; supersedes ADR-0014)

> Progress: the similarity edge has its first concrete measure —
> `core/src/similarity.rs` computes per-axis agreement (detected pattern
> period, repeatability, loopability, structural complexity, tag sets,
> since v2 the five intensive gesture distributions of corpus schema v3,
> and since v3 the five complexity axes of corpus schema v6 — rhythmic /
> pitch / technical / harmonic / playability; the structural axis stays
> off the edge as a duplicate fact) between `ChunkMeta` records, and
> `find_similar_chunks` ranks a query's neighbours as explainable
> `Scored<ChunkId>` envelopes under the uniform `similarity` v3 policy
> (ADR-0017); *measured* means structure **and** gesture **and**
> complexity, so pre-v6 records sit out until re-curated. Brute-force by design
> at micro-corpus scale (no ANN; decisions.log 2026-06-10 AudioMuse entry,
> idea (a)). Nodes, transition / co-occurrence edges, complement hyperedges,
> and the DP/Viterbi traversal remain gated on S6 acceptance and corpus
> scale.

## Goal

Graph-driven recombination over phrases, motifs, and rhythm cells — only after
stable chunks and features exist. The hypergraph is the *map* (what is
connected / possible); DP/Viterbi is the *route* (which sequence is best).

## Inputs / Outputs

- In: corpus chunks + features (≥ ~100 phrases recommended before this pays
  off).
- Out: a (hyper)graph (nodes + edges) and a DP/Viterbi traversal producing the
  optimal candidate chain.

## Approach

- Node types: `Phrase`, `Motif`, `RhythmCell`, `ChordMovement`,
  `EnergyState`.
- Edges: similarity (cosine on feature vectors), transition probability
  (counted from corpus), tag co-occurrence. Complement relations (§9) are
  multi-feature **hyperedges**, not binary edges.
- Traversal: **DP/Viterbi** as the primary mechanism (ADR-0013) — deterministic
  by construction (SPEC §6), optimising the whole sequence rather than picking
  the locally-best candidate per bar. Beam search is kept only as an
  approximation for graphs too large for exact DP.
- DP state carries running context: current candidate, fretboard position and
  last technique (ADR-0018 — the rich note model makes both expressible),
  `EnergyState`, rhythmic similarity to part A.
- Cost function (inspectable, the same weights S9 later tunes):
  `harmonic_fit + rhythm_complement + style_fit + playability + phrase_continuity
  − mud_penalty − repetition_penalty − fret_jump_penalty`.

## Acceptance criteria

- Recombined chains beat S6 single-strategy output on a defined quality score.
- Deterministic for a fixed cost function (Viterbi optimum is unique; ties break
  by a fixed documented rule).
- Multi-bar output shows a global arc (e.g. no 4 identical-technique bars in a
  row), not a chain of locally-best fragments.

## Open questions

- Minimum corpus size before the graph beats rule-based v0.
- Exact cost-term weights (calibrated on the corpus; later tuned by S9).
- State-size vs exactness trade-off before beam search is needed.

## See also

- [`../glossary.md`](../glossary.md) §9
- [`../adr/0013-dp-viterbi-traversal.md`](../adr/0013-dp-viterbi-traversal.md)
- [`../adr/0018-rich-note-model-fretboard-and-techniques.md`](../adr/0018-rich-note-model-fretboard-and-techniques.md)
  (fretboard position + multi-technique with evidence; supersedes ADR-0014)
- [`S13-complementary-part-generation.md`](S13-complementary-part-generation.md)
  (single-bar retrieval; this stage adds multi-bar sequencing)
