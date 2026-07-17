# ADR 0030: Reduced-state layered DP clients

Date: 2026-07-17
Status: Accepted
Amends: [ADR-0013](0013-dp-viterbi-traversal.md) (DP/Viterbi traversal over the
phrase hypergraph). ADR-0013's decision stands; this narrows three of its claims
and adds the contracts a shipped engine needs.

## Context

ADR-0013 fixed DP/Viterbi as S7's traversal and then, in its own consequences,
made S7 unshippable:

> Accepted: DP depends on a fretboard-aware model (ADR-0014) and a realised
> `EnergyState`; neither exists yet, so S7 cannot ship until they do.

That followed from §1, which specified *one* state — "current phrase/candidate,
fretboard position, last technique, energy level, rhythmic similarity to part
A" — as though every client needed every dimension. It does not. A client that
recombines an existing `RankedSet` across bars needs a candidate ordinal and
nothing else; the fretboard and `EnergyState` are dimensions it would carry
around and never read.

The same paragraph carried a second error, and it is the one worth being exact
about:

> Accepted: exact DP is exponential in the state size.

Exact DP is not exponential in anything. Over an enumerated layered graph the
recurrence visits each edge once: `O(Σᵢ |L[i-1]| × |L[i]|)`, polynomial in the
graph it is handed. What grows combinatorially is the **number of states worth
enumerating** when a state is a product of independent context dimensions — a
layer of `candidates × fret positions × techniques × energy levels` multiplies
out with each dimension added, and the layer is the input the DP is polynomial
*in*. The cost lives in state design, upstream of traversal. Conflating the two
is what made "keep the state small" read as a limitation of DP rather than as a
design rule for its clients, and what made a large state look free until the
graph arrived.

## Decision

**1. The engine is domain-free and the state is the client's.**
`core/src/layered_path.rs` takes ordered layers of caller-supplied `Axes`, a
versioned `WeightPolicy`, and returns one state per layer with its ADR-0017
`Scored` envelope. It has no opinion about what a state *is*. ADR-0013 §1 is
therefore re-read as a description of the *richest imaginable* client, not as a
required state: each client enumerates the smallest state that its own cost
terms actually read.

**2. A client ships with the state it earns.** S7 Slice B's candidate chain
carries `(bar, candidate ordinal)` plus the candidate's own generation identity,
because that is what its four cost terms read. Fretboard position, last
technique, and `EnergyState` are **deferred** — not rejected. They return when a
client needs them, with that client.

**3. No speculative state dimensions.** A dimension may not be added to any DP
state "for later", for symmetry with this ADR's §1, or because a future client
might read it. The bar to clear is a client, in the same PR, whose cost function
reads the dimension. This is the rule ADR-0013's §1 needed and did not have:
every unread dimension multiplies the layer, and a layer nobody can enumerate is
how beam search stops being a fallback and becomes the only option.

**4. Beam search remains the documented fallback**, on the corrected reasoning:
it is needed when a client's *map* is too wide to enumerate, never because exact
DP degrades on a graph it can hold.

**5. The full graph direction is unchanged.** ADR-0013's map/route distinction
stands in full. Nodes (`Phrase`, `Motif`, `RhythmCell`, `ChordMovement`,
`EnergyState`), similarity and transition edges, complement hyperedges, corpus
transition statistics, and a state rich enough to carry musical history remain
S7's direction. Slices A and B are the route arriving before the map, against a
client that needs no map; they are not a decision to stop there. §2 (the cost
function's shape), §3 (determinism), §4 (weights are data, S9 tunes them) and §5
(DP stays out of S13 v0) are untouched.

**6. Determinism has an arithmetic half.** ADR-0013 §3 says a fixed cost
function makes the optimum unique. That is not sufficient: float addition is not
associative, so a path's cost is not defined until the order of the additions
is. One association is normative —
`local(i) + ( edge(i) + cost(i+1) )`, folded from the last layer back — and the
DP, the tie-breaking walk, the reported total, and every client's baseline use
that one. A baseline that adds the same terms up its own way is a second metric
wearing the first one's name (it differed by an ULP in practice), so clients
evaluate baselines *through* the solver, as one-state-per-layer problems.

**7. Finiteness is a property of sums, not of inputs.** Non-finite locals and
transitions are rejected before the walk; every accumulation is then checked as
it forms, because finite costs sum to `±∞` and an optimum compared over
`∞ == ∞` is not an optimum.

**8. Measurements name what they measure.** Two rules the first client made
concrete, stated here because they generalise to every client:

- A *missing address* and an *absent measurement* are different facts. An
  out-of-range bar is a typed error; a bar that exists and is silent is a
  musical observation. Neither may be the other, and an unmeasurable term is
  **absent** from the axes rather than present as a zero.
- Validation covers what the code **copies**, not what it reads. A client
  assembling output from a reference skeleton must validate every field it
  copies off that reference — the fields its cost model never reads are exactly
  the ones that vanish without a sound changing.

## Consequences

- S7 ships. ADR-0013's "cannot ship until" consequence is narrowed to: *a client
  needing fretboard position or `EnergyState` cannot ship until those exist.*
- The engine has one client, and its state is small enough to look
  underwhelming. That is the intent: §3 makes the next dimension arrive with a
  reason attached rather than as scaffolding.
- Accepted: the per-client state rule means two clients may enumerate the same
  musical situation differently. They are different questions; a shared state
  would be a shared answer neither asked for.
- Accepted: the normative association (§6) means totals depend on layer order in
  the last bits. This is the price of a defined answer; the alternative is an
  undefined one.
- The corrected complexity story (polynomial over the enumerated graph,
  combinatorial in state design) is what Slice D must be argued against: a new
  client justifies its state by naming the cost terms that read it, not by
  appealing to this ADR's §1.

## See also

- [`../stages/S7-graph-layer.md`](../stages/S7-graph-layer.md) — the slices and
  their contracts.
- [`0017-explainable-scoring-contract.md`](0017-explainable-scoring-contract.md)
  — `Axes`/`WeightPolicy`/`Scored`, and the anti-scalar rule the engine's
  derived totals obey.
- [`0018-rich-note-model-fretboard-and-techniques.md`](0018-rich-note-model-fretboard-and-techniques.md)
  — the model a future fretboard-carrying state would use.
- [`../decisions.log.md`](../decisions.log.md) 2026-07-17 — the implementation
  record for Slices A and B.
