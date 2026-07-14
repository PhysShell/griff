# ADR 0028: Adopt Swang as a deterministic authoring and verified lifting language

Date: 2026-07-14
Status: Proposed

## Context

Griff already has a canonical symbolic score model, deterministic rule generation,
reusable rhythm templates, explainable scoring, complementary-part generation, and
format adapters for MIDI and Guitar Pro. What it lacks is a compact, editable way to
compose these capabilities into reproducible experiments. A large control panel can
set scalar values, but it cannot clearly express scoped transformations, structural
relationships, reusable motifs, exact local repairs, or the provenance of a generated
result.

A plain textual score dump would be useful for diagnostics but would not recover the
musical structure lost when a program is rendered into events. MIDI and Guitar Pro do
not contain source-level constructs such as `repeat`, `motif`, `transpose`,
`fractalize`, or `complement`; recovering those constructs is therefore program
synthesis, not syntax conversion. The recovered program must be verified by executing
it back into the canonical model and comparing the result with the source.

The same structural operators can also be useful to generators: instead of emitting
only a final `Score`, a generator can emit an inspectable program which the user edits,
executes, compares, and reuses. This is especially valuable for mathcore/swancore
material where hierarchical repetition, irregular rhythm, transformation, and
cross-part relationships matter more than isolated note sampling.

## Decision

We adopt **Swang** (Swan Language) as a deterministic, versioned authoring frontend
for Griff and assign it roadmap stage S16.

1. **Canonical Score remains the only musical model.** Swang parses into a surface
   AST and lowers through a bounded execution plan into the existing canonical
   `Score -> MasterBar -> Track -> Voice -> EventGroup -> AtomEvent` hierarchy.
   `griff-core` does not depend on Swang syntax. The execution plan is transient
   orchestration, not a second score model.

2. **Swang has three explicit levels.**
   - High-level recipes express intent such as generation, complement relations,
     sections, policies, and transformations.
   - Pattern algebra expresses deterministic structural operations such as repeat,
     rotate, mirror, mask, overlay, interleave, quantize, and `fractalize`.
   - Exact blocks provide a typed escape hatch for canonical notes, rests, groups,
     techniques, positions, and local patches. Raw MIDI bytes are not an escape hatch;
     MIDI remains a boundary format.

3. **Every semantic dependency is explicit and versioned.** Scripts pin the Swang
   semantic version, seeded operations name their seed, text encoders pin
   normalization/segmentation algorithms, and all resource limits are declared or
   supplied by a versioned default profile. Identical source, semantic version,
   dependencies, and seed produce a byte-stable normalized expansion and a
   semantically identical canonical score.

4. **`fractalize` is a structural operator, not a musical god object.** It expands
   active cells into scaled copies of a kernel and empty cells into empty subtrees.
   Separate stages choose traversal, timing, tonal mapping, velocity, articulation,
   and fretboard realization. Two-dimensional ASCII input has no implicit meaning;
   its kernel semantics and traversal are explicit.

5. **Expansion is bounded before realization.** Fractal depth, cell count, event
   count, minimum duration, and polyphony have hard limits. Deterministic pruning is
   explicit and reported. The compiler never silently truncates an expansion.

6. **Swang supports verified lifting from canonical scores.** MIDI or Guitar Pro is
   first imported through the existing adapter into `Score`. The lifting pipeline then
   proposes higher-level Swang constructs, executes the candidate program, and emits a
   semantic comparison report. Lifting has three modes:
   - `lossless`: exact canonical reconstruction, using typed residual patches where
     high-level constructs do not cover the source;
   - `structural`: controlled normalization of timing/velocity noise while preserving
     declared musical invariants;
   - `generative`: a compact recipe preserving selected structure and features rather
     than every source event.

7. **The optimizer prefers compact, readable, stable explanations.** Candidate
   programs are ranked by a versioned cost function combining reconstruction error,
   AST size, exact-residual size, construct complexity, and representation stability.
   The initial implementation uses explicit recognizer passes and dynamic programming;
   equality-saturation/e-graphs are deferred until measured rewrite pressure justifies
   them.

8. **Exact residual is first-class provenance.** A lifted program may use high-level
   structure plus exact exceptions. Reports expose source event count, events explained
   by constructs, residual event count, structural coverage, reconstruction differences,
   detected constructs, cost policy, and all format losses. A program with a decorative
   high-level wrapper and mostly exact residual is reported as low structural coverage,
   not presented as successful decompilation.

9. **Generators may emit Swang programs.** Rule-based, evolutionary, and later neural
   clients may produce versioned Swang AST/programs which are checked and executed by
   the same compiler. Human feedback may compare meaningful AST deltas, but no hidden
   preference mutation is introduced by this ADR.

10. **The reusable pattern core may be domain-neutral; Swang remains music-first.**
    Generic `Pattern`, `Mask`, `Traversal`, and bounded transform primitives may avoid
    music dependencies. LED, animation, or other discrete-signal backends are research
    possibilities, not S16 deliverables and not a reason to weaken Griff's
    swancore-first scope.

11. **TonalContext boundaries remain intact.** Swang may carry explicit tonal material
    and diagnostic estimates, but it must not consume automatic scope selection,
    confidence thresholds, cadence, or generation-facing tonal inference until S15
    accepts those contracts separately.

## Consequences

- Musical experiments become text-diffable, reproducible, explainable, and editable at
  the level of motifs and transformations instead of hundreds of individual events.
- Griff gains a bidirectional contract: programs compile into scores, and scores can be
  lifted into verified candidate programs with explicit residuals and error reports.
- `RhythmTemplate` provides a narrow first integration seam, allowing pattern-generated
  rhythm to enter S6 without rewriting the generator.
- Generator output can carry meaningful program provenance and support AST-aware
  mutation, crossover, comparison, and human correction.
- Guitar Pro lifting can preserve richer source facts than MIDI lifting; MIDI-derived
  positions and techniques remain absent or inferred and must never masquerade as
  explicit tablature evidence.
- The implementation requires a parser, formatter, diagnostics, bounded evaluator,
  recognizers, optimizer, semantic diff, property tests, and fuzz targets. This is a
  substantial staged effort, not one parser PR.
- Multiple programs can explain the same score. Cost-policy versioning and retained
  alternatives are necessary; the chosen program is an optimized explanation, not
  historical proof of how the musician composed the source.
- General-purpose loops, arbitrary recursion, host-code execution, ambient randomness,
  and unbounded expansion remain outside the language. Swang is a musical DSL, not a
  second Rust with worse error messages.
