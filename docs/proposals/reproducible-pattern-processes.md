# Proposal: Reproducible pattern processes

Bounded, typed, seed-isolated pattern operators and control curves as a
compilation layer above the canonical score — never a second score model.

Status: proposal for discussion (v1 — distilled from the 2026-07 research
memos on live-coding/pattern ecosystems; supersedes the reverted placeholder
of the same name)
Scope: docs-only until accepted. Binds nothing; S16 (`griff-pattern`, Swang,
ADR-0029) remains the canonical pattern work. This proposal extends that
line, it does not fork it.

## 1. Goal

Give Griff a reproducible *process* vocabulary — the thing live-coding
systems (Sonic Pi, Isobar, FoxDot, Sardine) are actually good at — without
importing their runtimes, their mutable iterator models, or their wall-clock
scheduling. A pattern process is a finite, serializable, hashable value that
compiles into the existing canonical `Score` through the same verified-lifting
discipline as Swang (ADR-0029).

## 2. Core decisions proposed

### 2.1 Bounded query semantics, no ambient state

Prior-art pattern libraries expose `next()` on shared mutable iterators; the
result then depends on call order and hidden global state. The Griff form is
a pure query:

```
pattern.query(range, cursor_state, seed_streams)
  → events + next_cursor_state + provenance
```

Hard requirements (all already house law elsewhere, restated for this layer):
finite extent, rational musical time on the master timeline, exact
serialization, stable hashing, bounded expansion (the `ExpansionBudget`
precedent from `griff-pattern`), typed refusal instead of silent truncation.

### 2.2 Named seed streams

`griff-pattern` already states the law: the rhythm prune seed is
"independent of any generation seed by law" (`PruneSpec`). This proposal
generalizes it: every stochastic operator draws from a *named* stream with
its own identity —

```
operator_kind + operator_path + stream_version + seed + local_index
```

so that (a) freezing one axis (pitch) while regenerating another (rhythm) is
a first-class operation, (b) changing one operator's implementation bumps its
`stream_version` without shifting any other stream, and (c) provenance names
the exact draw that produced each event. Prior art (Total Serialism) shows
the failure mode this prevents: a module-global RNG where one operator's
draw silently shifts every other operator's output.

### 2.3 Independent cyclic axes

Pitch, rhythm, articulation, velocity and register may cycle with different
periods (Sonic Pi's `ring`/`tick`); the full period is the LCM of the axis
periods. This yields long controlled development from small material and is
trivially finite and hashable. Cursors are explicit state in the query
contract, never hidden counters.

### 2.4 Control curves (envelopes) instead of scalar sliders

Fenv-style curves on the normalized `0..1` extent of a phrase or section:

```
density: 0.0 → 0.25, 0.5 → 0.85, 1.0 → 0.40
```

Candidate targets: complexity, note density, register target, variation
strength, technique likelihood, harmonic tension, complement activity,
rhythmic displacement. Curves are finite piecewise data — serializable,
diffable, and a natural fit for the S14 `ComplexityProfile` measurement side
(a curve is a *request*; S14 metrics measure what was *achieved*).

### 2.5 Hierarchical commitments

The HWFC lesson, restated for music:

```
section structure → phrase roles → motif families → events → fretboard realization
```

A lower level only fills windows the level above allowed; local generation
may not break global form; every commitment carries provenance. This is an
architectural constraint on future generator work (S14 structure controls,
S7 DP realization), not a WFC implementation proposal.

### 2.6 Synchronization by musical events, application at boundaries

Named musical events (`phrase_started`, `cadence_reached`, `guitar_a_rest`,
`section_changed`, `candidate_committed`) instead of shared mutable state;
edits take effect at a chosen musical boundary (next note / beat / bar /
phrase / section). The Glicol rule is adopted verbatim: an invalid edit never
destroys the currently accepted snapshot — parse, check, expand bounded,
and only then swap at the boundary.

## 3. First increment (docs-only): Pattern Operator Inventory

One table, one row per operator, before any code:

| column | content |
| --- | --- |
| operator | ring, rotate, reverse, ping-pong, stutter, creep, pad-to-multiple, Euclidean mask, urn, bounded walk, multi-cycle, reset-on-event, envelope modulation |
| prior art | Isobar / Sonic Pi / Total Serialism / TOT reference |
| semantics | input/output contract in canonical-score terms |
| boundedness | proof sketch of finiteness |
| state | cursor requirements |
| randomness | named streams consumed |
| existing equivalent | what `griff-pattern` / the generator already does |
| decision | adopt / adapt / reject |

Acceptance of this proposal means: the inventory lands under `docs/`, and
each *adopted* operator later enters `griff-pattern` through the normal S16
red-green path with golden vectors. Nothing lands in production from this
document directly.

## 4. Non-goals

- Embedding Ruby/Lisp/Clojure/Max runtimes or any live-coding engine.
- Infinite patterns, wall-clock `sleep`, user threads, or a second timeline.
- A second score model (Mutwo's tree is prior art for *transform* shapes
  only; the canonical model stays the single internal model, ADR-0011).
- A text DSL beyond Swang; text syntax only ever sits on top of the typed IR.
- Handing canonical timing to a browser audio engine (Tone.js is an audition
  backend at most; SCAMP's lesson stands — canonical symbolic timing,
  performance timing, and notation projection are three different things,
  and humanization never changes `Score` identity).

## 5. Prior art surveyed (prior-art-first rule, AGENTS.md)

Sonic Pi (rings/ticks, cue/sync, deterministic seeds, boundary application);
Isobar (operator catalogue; the mutable-iterator model is the anti-pattern);
Total Serialism (operator ideas; global-RNG anti-pattern); TOT (Opusmodus,
GPL — reference only, never code); Fenv (envelopes as functions on `0..1`);
HWFC (hierarchical commitments); Glicol (diff-based live swap, old graph
survives bad edits); Mutwo (event-tree transform vocabulary); SCAMP
(performance/notation separation); Sardine/FoxDot (UX reference only). GPL
sources are idea-only per the licence rule.
