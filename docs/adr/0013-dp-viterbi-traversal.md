# ADR 0013: DP/Viterbi traversal over the phrase hypergraph

Date: 2026-05-31
Status: Proposed

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
- Accepted: exact DP is exponential in the state size; the state must be kept
  small, and beam search is the fallback for large graphs.
- Accepted: DP depends on a fretboard-aware model (ADR-0014) and a realised
  `EnergyState`; neither exists yet, so S7 cannot ship until they do.
- Accepted: S13 v0 ships before DP and therefore cannot assemble coherent
  multi-bar parts on its own — that capability arrives with S7.
