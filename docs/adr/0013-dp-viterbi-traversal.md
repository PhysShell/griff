# ADR 0013: DP/Viterbi traversal over the phrase hypergraph

Date: 2026-05-31
Status: Accepted (2026-07-17) — Slices A and B shipped; see "What shipped" below.
Supersedes nothing; Slice C (deterministic k-best) remains future work.

## Context

The graph layer (S7) connects phrases, motifs, rhythm cells, chord movements,
and energy states, and produces candidate chains. The stage doc originally
specified traversal as *weighted random walk + beam search*.

Two things make that the weaker choice once the graph carries real musical
edges:

- **Determinism.** SPEC §6 requires generation to be deterministic under a
  fixed seed. A random walk only satisfies this by threading a seed and still
  yields "a random good path". Dynamic programming / Viterbi over a fixed cost
  function yields *the* optimal path — deterministic by construction, no seed
  needed for the core selection.
- **Sequence quality.** Picking the locally best candidate per bar produces
  globally bad parts (four bars of tapping in a row — locally fine, globally a
  "MIDI spider in a coffee grinder"). The fix is to optimise the *sequence*,
  carrying history (last technique, energy, fretboard position, rhythmic
  similarity to part A) in the DP state.

This separates two concerns that must not be conflated (the "map vs route"
distinction):

- The **hypergraph is the map** — it answers *what is connected / possible*. A
  hyperedge binds many features at once (part A phrase ↔ chord ↔ rhythm ↔
  technique ↔ fret position ↔ part B candidate), because a musical decision
  rarely depends on one factor.
- **DP/Viterbi is the route** — it answers *which sequence is best* across bars.

For a single bar ("good part-B candidates for this bar") DP is unnecessary —
that is retrieval/ranking, which is S13 v0. DP is needed only to assemble a
coherent multi-bar part (8/16/32 bars).

## Decision

We adopt **DP/Viterbi as the primary S7 traversal mechanism**, replacing
weighted random walk as the default. Beam search is retained only as an
*approximation* for graphs too large for exact DP.

1. **State.** A DP state carries the running context needed for musical
   coherence: current phrase/candidate, fretboard position (ADR-0014), last
   technique, energy level (the `EnergyState` node already named in S7),
   and rhythmic similarity to part A.

2. **Cost function.** Transition cost is an explicit, inspectable sum:
   `harmonic_fit + rhythm_complement + style_fit + playability + phrase_continuity
   − mud_penalty − repetition_penalty − fret_jump_penalty`. The exact terms are
   calibrated later; the shape is fixed here.

3. **Determinism.** With a fixed cost function the Viterbi optimum is unique and
   reproducible; ties break by a fixed, documented rule (e.g. lowest candidate
   index). This satisfies SPEC §6 without an RNG in the core selection.

4. **The cost function is the S9 surface.** The same weights DP consumes are
   what the feedback layer (S9) learns from like/dislike. DP *consumes* weights;
   S9 *tunes* them. DP must therefore expose its weights as data, not hardcode
   them, so S9 can adjust them without touching traversal code.

5. **Scope boundary.** DP lives in S7. It is explicitly **not** part of
   ComplementArranger v0 (S13), which stays single-bar retrieval. Pulling DP
   into S13 v0 would couple it to the unimplemented `EnergyState` and fretboard
   model and is forbidden by this ADR.

## Consequences

- S7 traversal is deterministic by construction, aligning the graph layer with
  SPEC §6 instead of working around it with a seed.
- Multi-bar parts get a global arc (contrast → answer → release → setup) rather
  than a chain of locally-best fragments.
- The DP cost function becomes the natural, explainable target for S9 feedback;
  S7 and S9 share one weight vector.
- Accepted: the cost is in the *state design*, not in the DP. Over an
  enumerated layered graph the recurrence is polynomial —
  `O(Σᵢ |L[i-1]| × |L[i]|)`, one visit per edge. What can explode is the number
  of states worth enumerating: a state that is the product of independent
  context dimensions (candidate × fret position × technique × energy × …) has a
  layer size multiplying out with each dimension. So the state must be kept
  small, and beam search is the fallback once a client needs a layer too wide
  to enumerate — not because exact DP degrades, but because the map does.
- Accepted: DP depends on a fretboard-aware model (ADR-0014) and a realised
  `EnergyState`; neither exists yet, so S7 cannot ship until they do.
- Accepted: S13 v0 ships before DP and therefore cannot assemble coherent
  multi-bar parts on its own — that capability arrives with S7.

## What shipped (2026-07-17)

Slices A and B, against the first *real* client rather than a speculative
framework. The 2026-05-31 decision above stands; this records what exists.

**Slice A — `core/src/layered_path.rs`.** A domain-free layered-DAG engine:
ordered layers of caller-supplied [`Axes`], a versioned `WeightPolicy`, and
`solve()` returning one selected state per layer with its full ADR-0017
`Scored` envelope. It knows nothing of notes, bars, strategies, S6, S8, or S9.

- Recurrence: `suffix[n-1][s] = local(n-1,s)`;
  `suffix[i][s] = local(i,s) + min_t( trans(i,s,t) + suffix[i+1][t] )`.
  A backward pass fills `suffix`; a forward pass walks it. Complexity
  `O(Σᵢ |L[i-1]| × |L[i]|)` — exact DP, no beam, no RNG, no seed.
- **Tie-breaking:** exact ties resolve to the **lexicographically smallest
  vector of state ordinals**. Deciding front-to-back over the optimal set is
  what delivers that without storing prefixes, so the complexity claim holds.
  No epsilon "approximately equal" rule; no hash-map iteration order.
- Non-finite costs (`NaN`, `±∞`) are rejected **before** the walk, naming the
  exact state or edge, so the comparisons run on a genuine total order. Finite
  inputs are not enough: a sum of finite costs can still reach `±∞`, so every
  accumulation — each suffix step and each step of the final total — is checked
  as it is formed and reported as `NonFiniteAccumulation` naming the state it
  overflowed at. An optimum computed over `∞ == ∞` is not an optimum.
- The problem's shape is validated exactly, not permissively: `n` layers require
  exactly `n − 1` transition tables. An extra table is a caller whose layers and
  transitions disagree, and silently ignoring it would let a real cost go
  unapplied.
- The state carried is deliberately *smaller* than §1 of this ADR imagined:
  fretboard position, last technique, and `EnergyState` are **not** in it.
  They remain unbuilt, and the ADR's own §"Consequences" note that DP could
  not ship until they existed is hereby narrowed: DP ships for a client that
  does not need them. Slice D may widen the state when a client earns it.

**Slice B — `core/src/candidate_chain.rs`.** The first client: recombining an
existing `RankedSet` across bars. Layer `b` holds bar `b` of every ranked
candidate; a path picks one candidate per output bar. Nothing is regenerated,
reranked, or re-seeded.

- **Cost function `candidate_chain` v1** — an untuned, documented baseline, and
  a much smaller thing than the §2 sketch (`harmonic_fit + rhythm_complement +
  style_fit + playability + phrase_continuity − mud − repetition − fret_jump`).
  Only what is measurable today from canonical events ships:
  `candidate_quality` (1.0, value `1 − s6_aggregate`), `boundary_jump_semitones`
  (0.05/semitone, unwrapped), `silent_boundary` (0.25), `rhythm_repeat` (0.40).
  Harmonic fit, style fit, playability and fret travel are **absent**, not
  present as zeroes — an unmeasured term is a lie with a weight attached.
- §4 holds: the weights are data (`WeightPolicy`), so S9 can tune them without
  touching traversal. S7 consumes; S9 does not exist here.
- **The boundary pitch is the last note still *sounding*, not the last onset.**
  It is selected by note end (`offset + duration`, compared in `u64` so the sum
  cannot wrap), ties going to the highest pitch — the chord top, the same
  convention the next bar's first pitch uses. A note held across the bar is what
  the ear carries over the line; a short note struck later and already stopped
  is not.
- **An invalid bar index is not silence.** `silent_boundary` is a musical
  observation — this edge has no sounding pitch — and it must never be produced
  by an out-of-range address. Boundary facts return a typed error naming the
  offending side; a bar that does not exist and a bar that exists and is quiet
  are different facts and are reported differently. The same rule holds one
  layer down: assembly asking for a track, voice, or bar that is not there is an
  invariant violation, not an empty bar.
- **Compatibility covers everything assembly copies**, not everything the cost
  model reads. The output is built on ranked candidate 0's skeleton — master
  bars (index, tick range, meter, tempo, repeat barlines), track name, channel,
  tuning, voice ids, source format — and then filled with bars from any
  candidate, so every one of those facts must be one all candidates already
  agree on. The errors name the field. The fields the cost model does *not* read
  are exactly the ones that would otherwise go missing without a sound changing.
- Cross-bar material is **rejected**, not clipped: `slice::extract_bars` cuts
  atoms by onset without shortening durations and clamps technique spans, so it
  offers no lossless concatenation contract for a note ringing past its bar.
  The unit of rejection is the **event group**, because the group is the unit
  assembly lifts: the group's bar is decided once, from its first atom, and
  every atom and every technique span it holds must start and end inside that
  bar. Material outside the timeline entirely is named by its tick, since there
  is no bar to report it against.
- Baseline: ranked candidate 0, kept intact, weighed under the *same* policy.
  On the synthetic non-greedy fixture the intact winner costs 3.3 and the
  planned chain 2.4.
