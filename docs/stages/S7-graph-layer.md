# S7: Graph layer (late)

Status: in progress — landed: chunk similarity edge (2026-06-10), Slices A/B
(2026-07-17: the layered-path engine and the multi-bar candidate chain).
Remaining: Slice C; the corpus-scale full graph and any Slice D specialised
clients stay deliberately late
Depends on: S6 acceptance
ADRs: ADR-0013 (DP/Viterbi traversal) as amended by ADR-0030 (reduced-state
layered DP clients), ADR-0018 (rich note model: fretboard + multi-technique with
evidence; supersedes ADR-0014)

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
>
> Research update (2026-07): `ekzhang/harmony` and `napulen/romanyh` reinforce
> the stage's existing architecture: enumerate feasible states per layer,
> calculate explainable local/transition costs, optimise globally, reconstruct
> the path, and later return deterministic k-best alternatives. The algorithmic
> form is adopted as roadmap input; their classical SATB rules and runtime code
> are not.

## Goal

Graph-driven recombination over phrases, motifs, and rhythm cells — only after
stable chunks and features exist. The hypergraph is the *map* (what is
connected / possible); DP/Viterbi is the *route* (which sequence is best).

## Inputs / Outputs

- In: corpus chunks + features (≥ ~100 phrases recommended before this pays
  off).
- Out: a (hyper)graph (nodes + edges) and a DP/Viterbi traversal producing the
  optimal candidate chain and, after the first path is validated, ranked k-best
  alternatives.

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
  `EnergyState`, rhythmic similarity to part A, and optional S15 harmonic state
  once that contract is calibrated. **Per ADR-0030 this is the richest
  imaginable state, not a required one:** each client enumerates the smallest
  state its own cost terms read, and a dimension arrives only with a client that
  reads it. Slice B's chain carries `(bar, candidate)` and nothing else.
- Cost function (inspectable, the same weights S9 later tunes):
  `harmonic_fit + rhythm_complement + style_fit + playability + phrase_continuity
  − mud_penalty − repetition_penalty − fret_jump_penalty`.

## Planned slices

### Slice A — concrete layered-path contract — **landed 2026-07-17**

`core/src/layered_path.rs`. A domain-free layered DAG: the caller hands ordered
layers of `Axes` (local per state, transition per adjacent pair) plus a
versioned `WeightPolicy`; `solve()` returns one state per layer, each with its
ADR-0017 `Scored` envelope, the selected edges, the derived total, and policy
provenance. It knows nothing of notes, bars, strategies, S6, S8, or S9 — the
chain below is a client, not a special case.

- Recurrence `suffix[i][s] = local(i,s) + min_t( trans(i,s,t) + suffix[i+1][t] )`,
  backward pass then forward walk, `O(Σᵢ |L[i-1]| × |L[i]|)`. Exact DP; no beam,
  no RNG, no seed.
- **Tie-break:** the lexicographically smallest vector of state ordinals, decided
  front-to-back. All-equal costs therefore select ordinal `0` everywhere.
- `NaN`/`±∞` are rejected before the walk, naming the state or edge, so the
  comparisons run on a total order — and every *accumulation* is checked as it
  is formed (`NonFiniteAccumulation`), because a sum of finite costs can still
  reach `±∞` and an optimum over `∞ == ∞` is not an optimum.
- The shape is validated exactly: `n` layers require exactly `n − 1` transition
  tables, checked before any scoring. Empty problems, empty layers, mismatched
  table counts and mismatched table shapes all return typed errors.
- **One cost association** (`PATH_COST_ASSOCIATION`, ADR-0030 §6): float
  addition is not associative, so a path's cost is undefined until the order of
  the additions is. `local + (edge + suffix)`, folded from the last layer back,
  is normative — the DP, the walk, the reported total and every client baseline
  use it, and the reported total is therefore the number the search minimised.
- Verified against a brute-force oracle over every tiny problem shape (1–4
  layers × 1–3 states), on both total and exact path.
- 24 tests.

The already-accepted register track was **not** reopened to manufacture a
client.

### Slice B — multi-bar global candidate chain — **landed 2026-07-17**

`core/src/candidate_chain.rs`. `plan_candidate_chain(&RankedSet)`: layer `b`
holds bar `b` of every ranked candidate, so the DP picks one candidate per
output bar and optimises the whole sequence. Nothing is regenerated, reranked,
or re-seeded; each bar is a snapshot of a score already in the set, and each
candidate's six S6 axes, rationale, and rerank provenance travel into the result
untouched.

- **Cost model `candidate_chain` v1** (untuned, documented baseline):
  `candidate_quality` 1.0 (value `1 − s6_aggregate`, monotonic in S6's verdict),
  `boundary_jump_semitones` 0.05 per unwrapped semitone, `silent_boundary` 0.25,
  `rhythm_repeat` 0.40 (signature of real `(onset, duration)` pairs, pitch-free).
- **The boundary pitch is the last note still sounding**, selected by note end
  (`offset + duration`, widened to `u64` so the sum cannot wrap) with ties to
  the highest pitch — not the last onset, which would hear a stopped note over a
  held one.
- **Silent-bar semantics:** when a bar edge has no sounding pitch the jump is
  unmeasurable, so `boundary_jump_semitones` is **absent** and `silent_boundary`
  carries the fact. An unavailable measurement is never a zero pretending to be
  perfect continuity. Two silent bars share the explicit empty rhythm signature.
  An **invalid** bar index is not silence: it is a typed error naming the
  offending side, because a bar that does not exist and a bar that is quiet are
  different facts.
- **Compatibility covers every fact assembly copies from ranked candidate 0**,
  not just the three the cost model reads. The set is refused — never truncated
  — on the first offending fact, which the error names: empty set, no bars,
  differing bar count or PPQ; any master-bar field (index, tick range, meter,
  tempo, repeat barlines); any track field (name, channel, tuning, voice count,
  voice ids); the source format; the import **loss report**, which is then
  carried into the assembled score rather than replaced with a clean one — an
  empty report must mean nothing was lost, not that nobody carried it. The
  master timeline is the one every candidate already agrees on, never borrowed
  from a layer winner.
- **Cross-bar material is rejected, not clipped.** `slice::extract_bars` filters
  atoms by onset without shortening durations and clamps technique spans, so it
  is not a lossless concatenation contract. The unit of rejection is the **event
  group**, because the group is what assembly lifts: its bar is decided once,
  from its first atom, and every atom and technique span it holds must start and
  end in that bar. A note ringing past its bar, a span straddling or sitting
  outside the group's bar, an atom in another bar, and a group with no atoms at
  all are each refused; material outside the timeline is named by its tick.
- **Assembly** copies the selected bars' event groups verbatim onto the shared
  timeline — group kind, rests, velocity, per-note marks, fretboard position and
  its ADR-0019 evidence, technique spans with their evidence and exact unclamped
  ranges — with no rebasing, no re-quantising, and no MIDI round-trip. A missing
  bar, track, or voice is a typed error, never an empty result that would leave
  the part quietly short.
- **Baseline:** ranked candidate 0, intact, evaluated *through the solver* as a
  one-state-per-layer problem under the *same* policy — not by a second
  summation of its own, which is a second metric wearing the first one's name
  (the two disagreed by an ULP). On the
  synthetic non-greedy fixture (candidate 0 is locally cheapest everywhere but
  its bar 1 dives to pitch 50 and must climb 34 semitones back) the intact
  winner costs **3.3**, the planned chain **2.4** via `[0, 1, 0]`. A real
  fixed-seed `ranked_candidates` pass plans deterministically with complete
  explanations. No corpus-level musical superiority is claimed from one fixture.
- 58 tests, plus 6 in `core/tests/s6_chain_baseline.rs` pinning S6's ranked
  output for a fixed seed against **literals recorded at base `a02355a`**, in a
  detached worktree, before S7 existed: the ten candidates as `(strategy,
  variant seed, aggregate bits)` in rank order — ties included, so the ADR-0017
  §7 tie-break is recorded as it fell — the six axis labels, and the winner's
  actual notes. A characterization test that recomputes its expectations from
  the code under test proves only that the code equals itself.

**Audible since 2026-07-17.** Slices A and B are core-only: they answer the
question in numbers. The **Global Chain Audition** milestone (S8) surfaces both
results in the cockpit — the intact S6 winner and the assembled chain, from one
ranked set, A/B through the existing playback stack, with the per-bar suppliers
and this stage's own cost explanations on screen, and the assembled score
exportable to MIDI. The chain is planned once inside the generation pass; the
cockpit never holds a `RankedSet` or the planner. See
[`S8-preview-app.md`](S8-preview-app.md).

Deliberately **not** in these slices: k-best, harmonic fit, style fit,
playability, fret travel, `EnergyState`, corpus transition statistics,
persistent nodes, and complement hyperedges.

Both slices were built as RED→GREEN pairs, in order: the layered path's optimum
and tie-break; the chain's layers from a `RankedSet`; the v1 cost model; lossless
assembly; the S6 characterization; then, from review, the exact transition-table
count, non-finite DP accumulations, the boundary facts' valid bars and last
sounding note, the compatibility contract over the shared skeleton, and
group-local lossless assembly.

### Slice C — deterministic k-best alternatives

Return several ranked global paths with:

- fixed tie-breaking;
- complete total/local/transition explanations;
- an explicit diversity rule so alternatives are not path clones;
- stable provenance for S8 display and S9 feedback.

### Slice D — specialised clients

After the engine and first client are accepted, consider complementary-guitar,
harmonic, and cadence planners. Reuse existing fretboard DP rather than rewrite
it only for abstraction symmetry. Register planning requires new measured
counterevidence before reopening.

## Acceptance criteria

- Recombined chains beat S6 single-strategy output on a defined quality score.
- Deterministic for a fixed cost function (Viterbi optimum is unique; ties break
  by a fixed documented rule).
- Multi-bar output shows a global arc (e.g. no 4 identical-technique bars in a
  row), not a chain of locally-best fragments.
- Every selected path exposes local and transition-cost explanations.
- k-best alternatives are deterministic and measurably distinct under the
  documented diversity rule.

## Open questions

- Minimum corpus size before the graph beats rule-based v0.
- Exact cost-term weights (calibrated on the corpus; later tuned by S9).
- State-size vs exactness trade-off before beam search is needed.
- Which multi-bar client produces enough value to justify the first reusable
  path contract.

## See also

- [`../audit/2026-07-symbolic-harmony-and-evolution-research.md`](../audit/2026-07-symbolic-harmony-and-evolution-research.md)
- [`S15-tonal-context-and-harmonic-control.md`](S15-tonal-context-and-harmonic-control.md)
- [`../glossary.md`](../glossary.md) §9
- [`../adr/0013-dp-viterbi-traversal.md`](../adr/0013-dp-viterbi-traversal.md)
  (the 2026-05-31 decision) and
  [`../adr/0030-reduced-state-layered-dp-clients.md`](../adr/0030-reduced-state-layered-dp-clients.md)
  (what amends it: per-client state, the cost association, and why exact DP is
  polynomial over the graph it is handed)
- [`../adr/0018-rich-note-model-fretboard-and-techniques.md`](../adr/0018-rich-note-model-fretboard-and-techniques.md)
  (fretboard position + multi-technique with evidence; supersedes ADR-0014)
- [`S13-complementary-part-generation.md`](S13-complementary-part-generation.md)
  (single-bar retrieval; this stage adds multi-bar sequencing)
