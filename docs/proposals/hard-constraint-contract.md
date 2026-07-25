# Proposal: Hard constraint contract and offline Constraint Lab

One typed rule layer that separates legality from preference, plus MiniZinc
as an offline oracle that never enters the production path.

Status: proposal for discussion (v1 — distilled from the 2026-07 research
memos on constraint-based composition)
Scope: docs-only until accepted; binds nothing. Production generation stays
deterministic Rust; no external solver becomes a runtime dependency.

## 1. Goal

Make "is this candidate allowed at all?" a single, explainable, versioned
contract — distinct from soft scoring — and give Griff an offline oracle for
proving hard facts (UNSAT, minimal counterexamples) about its own heuristics.

The Strasheela lesson is the organizing principle: never fold requirements
into one aggregate score. Hard constraints decide *admissibility*; soft
objectives rank *among the admissible* (which is exactly what `rerank.rs`
axes and ADR-0017 already do on the soft side).

## 2. The contract

### 2.1 Rule scope (Cluster Engine model)

Every rule declares what it ranges over, explicitly:

```rust
struct RuleScope {
    tracks: TrackSelector,
    voices: VoiceSelector,
    dimension: MusicalDimension, // Pitch | Rhythm | Harmony | Fretboard
                                 // | Technique | Structure | Complementarity
    time_range: MusicalRange,
    relation: TemporalRelation,  // e.g. simultaneous, adjacent, windowed
}
```

### 2.2 Typed result — different failures are different states

Per-rule evaluation returns one of five states:

```rust
enum ConstraintResult {
    Satisfied,
    Violated { code, musical_path, evidence, explanation },
    Unknown { reason },        // e.g. lookahead not yet available
    Unsupported { operation }, // the rule cannot evaluate this input at all
    SolverFailure { diagnostic },
}
```

Two related facts deliberately live *outside* this enum: a low-scoring
candidate is a scoring fact and never a constraint state; and an empty
search space (a whole *problem* being UNSAT) is a problem-level outcome
reported by the Constraint Lab's solve result (§3), distinct from a solver
timeout or crash (`SolverFailure`). Nothing here is ever conflated: each of
these outcomes keeps its own typed diagnostic. (Same discipline as the
reachability lab's "no exact match within N trials ≠ unreachable".)

### 2.3 Rule metadata (Cluster Rules model)

Per rule, recorded in a Constraint Inventory table before implementation:
rule kind, scope, required lookback/lookahead, hard vs soft classification,
failure code, evidence path, human-readable explanation, and whether the
rule supports incremental evaluation (matters for DP clients, ADR-0013/0030).

Starter vocabulary — hard: physical playability, no conflicting notes on a
string, reachable tapping transitions, complement does not double the lead
on strong beats. Soft (stays in scoring, never here): minimize fret jumps,
prefer contrary motion, preserve corpus rhythm identity, register variety.

Harmonic-context membership is deliberately absent from the hard list: the
S15 Phase 2 contract carries tonal context as *optional* evidence with no
pitch restriction and no production behaviour change — chromatic passing
tones, borrowed notes and tensions stay legal by design. Any context-based
hard rejection waits for a later, explicitly calibrated contract on top of
the accepted S15 phases; until then tonal context influences nothing in
this layer.

### 2.4 What is *not* learned

Tablature validity, instrument range, voice crossing, exact timing, MIDI
export, provenance, snapshot immutability, acceptance gates. These are
computed exactly; no ML layer ever gets authority over them.

## 3. Constraint Lab: MiniZinc as offline oracle

Production never shells out to a solver — solver versions and search
strategies would turn determinism into decoration. Offline, a solver is
exactly the right tool:

```text
Griff typed problem → solver-neutral IR → MiniZinc adapter → solution / UNSAT / evidence
```

Legitimate uses: prove a fretboard realization problem UNSAT; find a minimal
counterexample for a bounded-solver implementation; generate hard synthetic
fixtures; compare a Griff heuristic against exact solutions; check
complementary-guitar rule sets for satisfiability. Every run archives exact
inputs, outputs, and solver identity (same manifest discipline as the
reachability lab).

### First increment: oracle spike (research tooling only)

One small fretboard problem and one complementary-guitar problem, modelled
in the neutral IR, run through MiniZinc, with deterministic fixture export
and full input/output archival. Deliverable is a `docs/audit/` report plus
fixtures; zero production dependencies added (`deny.toml` untouched).

## 4. Non-goals

- MiniZinc/Gecode/Clojure in the production path or the workspace tree.
- Rewriting the generator as a CSP; the deterministic generator, DP layer
  (ADR-0013/0030) and provenance stay exactly as they are.
- Merging hard constraints into scoring weights, or vice versa.
- A general-purpose theory language (Strasheela's ambition); Griff rules
  stay swancore-first and typed.

## 5. Prior art surveyed (prior-art-first rule, AGENTS.md)

Strasheela (composition as CSP; hard/soft separation; style-neutral theory
declaration); Cluster Engine (polyphonic rule scoping across voices and
dimensions); Cluster Rules (concrete constraint vocabulary and per-rule
metadata); clojure2minizinc / MiniZinc (solver-neutral modelling, reified
constraints, UNSAT evidence; the shell-out fragility is the argument for
keeping it offline). Licences: idea reuse only; no GPL code enters this MIT
workspace.
