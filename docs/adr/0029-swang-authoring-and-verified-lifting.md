# ADR 0029: Adopt Swang as a deterministic authoring and verified lifting language

Date: 2026-07-14
Status: Proposed

## Context

Griff has a canonical symbolic score model, deterministic rule generation,
reusable rhythm templates, explainable scoring, complementary-part generation,
and format adapters for MIDI and Guitar Pro. What it lacks is a compact,
editable way to compose these capabilities into reproducible experiments. A
control panel can set scalar values, but it cannot clearly express scoped
transformations, structural relationships, reusable motifs, exact local
repairs, or the provenance of a generated result.

A plain textual score dump would be useful for diagnostics but would not
recover the musical structure lost when a program is rendered into events.
MIDI carries no structural constructs at all; Guitar Pro preserves *notated*
repeats and alternate endings — imported and unfolded per ADR-0022, so they
lift as recorded facts, not guesses — but nothing above them: `motif`,
`transpose`, `fractalize`, and `complement` exist in neither format.
Recovering those higher constructs is program synthesis, not syntax
conversion. A recovered program must be verified by executing it back into
the canonical model and comparing the result with the source.

The same structural operators are useful to generators: instead of emitting
only a final `Score`, a generator can emit an inspectable program which the
user edits, executes, compares, and reuses. This matters most for
mathcore/swancore material, where hierarchical repetition, irregular rhythm,
transformation, and cross-part relationships matter more than isolated note
sampling.

## Prior art surveyed

Per the repository's prior-art rule, four surveys were run before this
decision (pattern DSLs; notation/score-text languages; structure lifting/MDL;
bounded-DSL design and seeded determinism). The findings below are grouped
by theme, not one bullet per survey. All claims are scoped — "no direct
equivalent found in the surveyed systems", not "nothing exists":

- **TidalCycles / Strudel** (pattern algebra for live coding). The algebra —
  `Time = Rational`, an event's `whole`/`part` split, pattern as a
  query-function of a time span — is the strongest design to *reimplement as
  an idea*. Randomness is a pure hash of cycle position; sampling over an
  event's `whole` makes values independent of query slicing — a discipline
  Swang adopts in path-addressed form. No time-scaled self-similar
  substitution operator exists there (its `lindenmayer` rewrites a flat,
  uniform-duration string; the docs' own example hand-bounds it with
  `take 512`); resource budgets are an open bug class (Tidal issue #498).
  Licences: Tidal/tidal-core GPL-3.0, Strudel AGPL-3.0, the sole Rust port
  GPL-3.0 — **read as specification only; no code reuse** into this MIT
  workspace.
- **alphaTab / alphaTex** (MPL-2.0). The closest score-text prior art, in the
  same `(string, fret)`-primary model family as Griff's canonical score. Its
  exporter round-trip attempt (alphaTab issue #1484) enumerates the
  expressibility holes an input-first guitar text language develops — a free
  negative-test checklist for the canonical-text phase. No music notation
  language surveyed has a canonical formatter with an idempotence law.
- **Point-set compression (SIA → SIATEC → COSIATEC, Meredith et al.)**. The
  deterministic half of verified lifting already exists with published
  pseudocode: encode a score as translational pattern classes plus an
  **explicit residual charged at full literal price**, admit a construct only
  if new coverage exceeds its own description cost. Swang adopts both the
  residual design and the admissibility test. The reference implementation
  (OMNISIA) is GPL-3.0 — reimplement from the published pseudocode.
- **Decomposer (arXiv:2607.01849, July 2026)** — MIDI→Strudel decompilation
  via LLM+RL, verified by re-execution. Its central finding is this ADR's
  cost function's failure mode stated empirically: optimizing reconstruction
  alone collapses to unreadable note-by-note transliteration; the
  description-size penalty is load-bearing, not cosmetic. Decomposer has no
  residual mechanism and no determinism, and the two literatures above do not
  cite each other — the intersection (deterministic + executable DSL +
  explicit residual + versioned description cost) is the unoccupied ground
  S16 targets.
- **Bounded/total config languages** (Dhall, Starlark, CUE, Rego; Faust for
  audio). Dhall's semantic-hash lesson is adopted verbatim: **never mix the
  language version into a content hash** (their v6.0.0 release notes record
  the churn that caused). Faust's lesson: totality must hold in the layer
  where users write recursion — Faust bounded its signal graph and left its
  macro layer Turing-complete, and it bites. Swang's budgets therefore live
  in the expansion layer itself.
- **UPIC (Xenakis)** — the contrasting design: a 2-D graphic surface whose
  axes *are* time and pitch. Swang deliberately refuses implicit axis
  meaning; a kernel's axes mean nothing until an explicit traversal and time
  mapping assign meaning.
- **Euclidean rhythms (Toussaint/Bjorklund; closed form per
  Clough–Douthett)** — the one bounded rhythm algorithm with unambiguous
  musical adoption; it enters the **candidate** operator roster as `euclid`,
  with rotation as a first-class parameter when it is specified (the named
  world-music timelines are rotations, and son clave is *not* Euclidean —
  the docs must not claim otherwise).

## Decision

We adopt **Swang** (Swan Language) as a deterministic, versioned authoring
frontend for Griff and assign it roadmap stage S16. Normative semantics live
in [`../swang/spec.md`](../swang/spec.md); the delivery plan in
[`../stages/S16-swang-language-and-verified-lifting.md`](../stages/S16-swang-language-and-verified-lifting.md).

1. **Canonical `Score` remains the only musical model.** Swang parses into a
   surface AST and lowers through a bounded execution plan into the existing
   `Score -> MasterBar -> Track -> Voice -> EventGroup -> AtomEvent`
   hierarchy. `griff-core` never depends on a Swang crate. The execution plan
   is transient orchestration, not a second score model.

2. **Two crates, one dependency direction.** `griff-pattern` is the pure
   structural algebra — **std-only: zero external dependencies, no
   `griff-core`, no MIDI, no serde, no time types** — holding `Pattern`,
   `PatternTree`, traversals, budgets, and deterministic pruning.
   `griff-swang` holds the AST, parser/formatter (later phases), and the
   lowering into `griff-core` types. Serialization of artifacts lives in
   `griff-swang` or an adapter, never in `griff-pattern`.

3. **Swang has three explicit levels.** High-level recipes (generation,
   complement relations, sections, policies); pattern algebra (deterministic
   structural operations, `fractalize` among them); exact blocks — a typed
   escape hatch for canonical notes, rests, groups, techniques, positions,
   and local patches. Raw MIDI bytes are not an escape hatch; MIDI remains a
   boundary format.

4. **Every semantic dependency is explicit and versioned.** A script pins a
   **monotonic integer language level** whose check is lexically trivial (a
   frozen first-line pre-parser that never changes, so any interpreter can
   reject a newer file intelligibly). Language levels are additive-only: a
   level never changes the meaning of existing syntax. The language version
   is **never an input to any content hash** — hashes are taken over
   normalized forms and the version travels beside them. Seeded operations
   name their seed; text encoders (when they arrive) pin their Unicode
   semantics and operate on scalar values, not grapheme clusters. Identical
   source, language level, and seeds produce a byte-stable normalized
   expansion and a semantically identical canonical score.

5. **`fractalize` is a structural operator, not a musical god object.** It
   expands active cells into scaled copies of a kernel and empty cells into
   empty subtrees. Separate stages choose traversal, timing, tonal mapping,
   velocity, articulation, and fretboard realization. A two-dimensional ASCII
   kernel has no implicit meaning: rows are not voices, columns are not time,
   ragged rows are a typed error, and meaning is assigned only by an explicit
   traversal (`row_major`, `snake` in v0.1) and an explicit time unit.

6. **Expansion is bounded before realization, in the right layer.**
   `griff-pattern` enforces the structural limits (`max_depth`, `max_cells`)
   as required arguments — the library ships no defaults; time-domain limits
   (`max_events`, `min_duration`, `max_polyphony`) belong to the lowering in
   `griff-swang`. A breach is a typed error carrying the offending
   `NodePath`. Deterministic pruning is explicit and reported; the compiler
   never silently truncates. Density decay is a **path-addressed hash test**
   (`swang-prune-hash-v1`: a named, documented integer mixer folded over an
   injective canonical path serialization — the 64-bit hash itself may
   collide, and a collision's only consequence is a shared keep/prune
   decision; decay carried in basis points; thresholds computed in integer
   arithmetic) — a removed parent yields an
   entirely empty subtree, and the pruning seed is independent of the
   generation seed so structure and pitch remain separately reproducible
   experiment axes.

7. **Pattern rhythm reaches S6 through an explicit override in the shared
   compiler.** S6 generation strategies and `RhythmTemplate` semantics remain
   unchanged; the shared generation-input compiler (`ranked_candidates`, the
   single entry point every frontend generates through) gains an explicit
   rhythm source with precedence `explicit pattern > corpus > source first
   bar`. Corpus novelty references and gesture remain corpus-based. Swang
   v0.1 speaks onsets and durations only — accent and dynamics have no
   landing place in the seam and are deferred, not smuggled.

8. **Swang supports verified lifting from canonical scores** (later phases).
   Imported scores lift into candidate programs; every candidate is executed
   and semantically compared with its source. Lifting has three modes —
   `lossless` (exact reconstruction through typed residual patches),
   `structural` (declared normalization of timing/velocity noise), and
   `generative` (a compact recipe preserving selected identity axes). The
   exact residual is first-class provenance: a program that is mostly
   residual is reported as low structural coverage, not presented as a
   successful decompilation.

9. **The optimizer prefers compact, readable, stable explanations.**
   Candidates are ranked by a versioned cost combining reconstruction error,
   AST size, exact-residual size, construct complexity, and representation
   stability. Description length is a valid ranking signal only **within**
   one language level and cost-policy version; the API forbids cross-version
   cost comparison. A selected program is the cheapest explanation under a
   documented policy, not a claim about how the musician composed the
   source. The initial implementation uses explicit recognizer passes and
   deterministic DP; equality saturation is deferred until measured rewrite
   pressure justifies it.

10. **Fractal lifting is a deferred research candidate, not a promised
    phase.** Inferring a substitution system from a single terminal string is
    unsolved, and the evidence that real music is self-similar in that sense
    is contested. Hierarchy in the lifter comes from recursive pattern-class
    covers or straight-line grammars, which are deterministic and
    verifiable. A `fractalize` recognizer may be admitted later only with
    synthetic identifiable fixtures, a bounded candidate grammar, a
    demonstrated description-cost win, and negative controls.

11. **The parser is hand-written recursive descent — as an initial
    implementation strategy, not an immutable contract.** What this ADR
    protects is the language: its grammar, its diagnostics (stable typed
    codes with source spans), its canonical formatting, and its deterministic
    semantics. The parsing technique may change without changing the
    language.

12. **TonalContext boundaries remain intact.** Swang may carry explicit tonal
    material and diagnostic estimates, but it must not consume automatic
    scope selection, confidence thresholds, cadence, or generation-facing
    tonal inference until S15 accepts those contracts separately.

## Consequences

- Musical experiments become text-diffable, reproducible, explainable, and
  editable at the level of motifs and transformations instead of hundreds of
  individual events.
- Griff gains a bidirectional contract: programs compile into scores, and
  scores can be lifted into verified candidate programs with explicit
  residuals and honest coverage reports.
- `RhythmTemplate` provides a narrow first integration seam; the first
  vertical slice must prove that a fractal kernel produces a real riff
  through the unchanged S6 generator before any grammar exists.
- The deterministic core forbids ambient randomness, floats in semantics
  (`f64` transcendentals are non-deterministic by Rust std's own
  documentation), platform-sized integers in hashed state, and OS entropy
  (`getrandom` stays out of the dependency tree); wasm builds target a
  module with no imports.
- Generator output can carry program provenance and support AST-aware
  mutation, crossover, comparison, and human correction (S9 pairs over
  declared single-axis deltas).
- Guitar Pro lifting can preserve richer source facts than MIDI lifting;
  MIDI-derived positions and techniques must never masquerade as explicit
  tablature evidence.
- The implementation is a substantial staged effort — parser, formatter,
  diagnostics, bounded evaluator, recognizers, optimizer, semantic diff,
  property tests, and fuzz targets (ADR-0010 applies to the parser and the
  expansion limits) — delivered as the S16 phases, not one compiler PR.
- General-purpose loops, arbitrary recursion, host-code execution, ambient
  randomness, and unbounded expansion remain outside the language. Swang is a
  musical DSL, not a second Rust with worse error messages.
