# S16: Swang language and verified lifting

Status: proposed
Depends on: S1 (canonical score), S2/S3 (MIDI and Guitar Pro adapters), S4
(phrase boundaries), S6 (rule generator)
Builds on: S13 relation specs, S14 structure controls, S15 explicit tonal contracts
when accepted
Feeds: S7 graph/DP clients, S8 cockpit, S9 feedback, S11 regeneration, S12
neural assistance, S13 ComplementArranger
Decision: ADR-0028
Tracking: #108

## Goal

Make Griff programmable at the level of musical structure.

Swang (Swan Language) is a deterministic, versioned DSL which can:

1. compile readable musical recipes and structural transforms into canonical
   `Score` values;
2. provide exact typed patches when high-level structure is insufficient;
3. lift imported MIDI/Guitar Pro scores into compact executable Swang programs;
4. verify every lifted program by executing it and comparing it with the source;
5. let generators emit editable programs rather than only opaque final events.

The stage succeeds when a short program explains and reproduces a non-trivial guitar
fragment, exposes its residual exceptions and provenance, and can be edited to produce a
predictable structural variation.

## Position in the architecture

```text
Swang source
  -> parser / formatter
  -> typed surface AST
  -> bounded execution plan
  -> Griff generators and transforms
  -> canonical Score
  -> validation / scoring / export
```

The reverse path is:

```text
MIDI / Guitar Pro
  -> existing adapter + LossReport
  -> canonical Score
  -> analysis / recognizers
  -> candidate Swang programs
  -> costed optimization
  -> execute candidate
  -> semantic diff + lift report
```

The canonical `Score` remains the single musical model. The Swang AST records a program;
the execution plan records transient work. Neither duplicates the score hierarchy.

## Language layers

### L2 — recipes and intent

```text
part guitar_b = complement guitar_a {
    relation rhythm_lock
    register below 12st
    density 0.65
    contour contrary
    seed 42
}
```

L2 compiles relative intent into existing generation, complement, structure, scoring,
and validation contracts.

### L1 — pattern algebra

```text
pattern seed = ascii {
    X . X
    X X .
    . X X
}

rhythm r = seed
    |> fractalize(depth = 2, density_decay = 0.8, budget = 512)
    |> linearize(snake)
    |> map_rhythm(unit = 1/16, bars = 4)
```

Initial operators are deliberately bounded:

```text
repeat
rotate
mirror
stretch / compress
mask / subtract / overlay
interleave
thin
accent
quantize
fractalize
```

### L0 — exact canonical escape hatch

```text
exact guitar_b bar 6 voice 0 {
    replace beat 3..4 with {
        at 3.0 note string 6 fret 7 length 1/16 marks [palm_mute]
        at 3.25 note string 5 fret 9 length 1/16 marks [slide]
        at 3.50 rest length 1/8
    }
}
```

L0 is typed canonical intent, not a raw MIDI byte block.

## Core contracts

### Determinism and semantic versioning

A script pins its Swang semantic version. Every operation is pure or explicitly seeded.
Text encoders pin Unicode normalization, grapheme segmentation, dictionary, and algorithm
versions where applicable. Identical declared inputs produce a byte-stable normalized
expansion and a semantically identical `Score`.

### Bounded evaluation

The evaluator enforces explicit limits before realization:

```text
max_depth
max_cells
max_events
min_duration
max_polyphony
```

A limit breach produces a source-located typed error. Deterministic pruning requires an
explicit policy such as `prune_deepest` or `preserve_anchors`; there is no silent
truncation.

### Structural separation

An active pattern cell is a structural position, not a note. The pipeline keeps these
operations separate:

```text
occupancy / hierarchy
  -> traversal
  -> time mapping
  -> pitch / tonal mapping
  -> dynamics and articulation
  -> fretboard realization
```

`fractalize` therefore cannot choose pitches, strings, techniques, and fingering by
itself.

### Verified lifting

Every lifted result carries:

```text
source digest and format losses
Swang semantic and cost-policy versions
recognized constructs and alternatives
source event count
construct-explained event count
exact residual event count
structural coverage
semantic differences after re-execution
```

A selected program is the best explanation under a documented policy, not a claim about
the historical composition process.

## Lift modes

### Lossless

```text
execute(lift(score, lossless)) ~= score
```

All canonical differences are represented by exact residual patches. Equality is
semantic canonical-model equality; original container bytes are not reconstructed.

### Structural

Permits declared normalization of MIDI noise or performance detail, for example timing
or velocity tolerance, while preserving explicit rhythmic, pitch, grouping, and phrase
invariants. Every normalization appears in the report.

### Generative

Finds a compact recipe that preserves selected identity axes such as rhythm, contour,
density, tonal material, technique distribution, and structure period. It is intended
for controlled regeneration, not archival reconstruction.

## Optimization objective

Initial lifting uses explicit recognizers plus deterministic dynamic programming. The
versioned cost has the shape:

```text
cost(program) =
    reconstruction_error
  + AST_size_penalty
  + exact_residual_penalty
  + obscure_construct_penalty
  + representation_instability_penalty
```

The optimizer must not prefer a clever but unreadable transform chain over a slightly
longer stable program. Equality saturation/e-graphs are deferred until a measured set
of rewrite rules makes them useful.

## Roadmap slices

### Phase 0 — design contract and golden source fixture

- accept or revise ADR-0028;
- define `.swg`, semantic versions, typed units, source spans, diagnostics, and limits;
- select one legally safe 4–8 bar GP fixture plus a MIDI projection;
- define canonical semantic diff and lift-report schemas;
- record exact baseline digests before any implementation.

No generation behavior changes.

### Phase 1 — canonical GriffScore text

Implement the lowest exact textual projection first:

```text
griff swang dump input.gp5 > input.swg
griff swang check input.swg
griff swang fmt input.swg
griff swang build input.swg --output output.mid
griff swang verify input.swg --against input.gp5
```

Required laws:

```text
parse(format(score)) ~= score
format(parse(text)) == canonical_text
```

Acceptance:

- every canonical event/group/track/master-bar fact in scope is representable;
- GP/MIDI adapter losses remain visible;
- diagnostics carry file spans and typed codes;
- parser, formatter, and exact patch boundaries have property/fuzz coverage.

### Phase 2 — pure pattern core

Add domain-light primitives:

```text
Pattern / Cell
PatternTree / NodePath
Mask
Traversal
ActivitySequence
FractalSpec / ExpansionBudget
```

Implement deterministic `fractalize`, traversal, and pruning without MIDI, tonal,
fretboard, UI, or generator dependencies.

Acceptance:

- empty parents produce empty descendants;
- expansion never exceeds the declared budget;
- transforms are deterministic and compositional;
- two-dimensional kernels require explicit semantics/traversal;
- property tests cover depth, shape, pruning, and degenerate inputs.

### Phase 3 — pattern-to-rhythm vertical slice

Map `ActivitySequence` into placed `(offset, duration)` `RhythmTemplate` values and feed
them through the existing S6 `source_rhythms` seam.

Minimal grammar:

```text
pattern
ascii
fractalize
linearize
map_rhythm
generate
export
```

First killer demo:

```text
text("glass hands")
  -> versioned grapheme mask
  -> fractalize depth 2
  -> snake traversal
  -> 1/16 RhythmTemplate
  -> existing S6 pitch strategy
  -> MIDI + normalized expansion + provenance
```

Acceptance:

- changing `depth`, `traversal`, or `density_decay` produces an inspectable,
  deterministic structural delta;
- existing S6 strategies and reranking remain unchanged;
- one checked-in `.swg` fixture produces stable four-bar output.

### Phase 4 — structural recognizers

Lift exact and near-exact source structure into candidate AST fragments:

```text
exact repeat
repeat with final variation
motif definition/reference
transposed motif
shared rhythm with separate pitch mapping
mask / overlay between voices or parts
section/bar scoping
```

Recognizer output is evidence with source ranges and costs, not an immediate rewrite.

Acceptance:

- each recognizer has positive, negative, and adversarial fixtures;
- accepted constructs reduce program cost and residual size;
- false-positive controls prove that repeated pitches alone do not imply a motif;
- GP and MIDI lift reports distinguish available and missing source semantics.

### Phase 5 — optimizing verified lift

Assemble candidate fragments into a global program with deterministic DP, execute it,
and select the lowest-cost verified explanation.

Add:

```text
griff swang lift input.gp5 --mode lossless|structural|generative
griff swang optimize input.swg
griff swang roundtrip input.gp5
griff swang explain input.swg
```

Acceptance:

- lossless mode reconstructs the canonical fixture exactly through residuals;
- structural coverage and residual ratio are reported honestly;
- identical inputs produce byte-stable selected programs and reports;
- at least one source admits two retained alternative explanations;
- changing the cost-policy version never silently rewrites stored programs.

### Phase 6 — fractal and hierarchical lifting

Detect self-similarity across declared rhythmic resolutions and propose
`fractalize` only when it improves the versioned description cost and passes
re-execution verification.

Acceptance:

- synthetic true-fractal fixtures recover the expected kernel/depth;
- non-fractal irregular mathcore fixtures reject the construct;
- approximate candidates expose residuals rather than inventing exactness;
- exponential search is bounded and observable.

### Phase 7 — recipes, complement, and exact patches

Add named sources, sections, policies, transforms, `generate`, and `complement` recipes.
Compile them into existing S6/S13 requests and allow exact local repair.

Tonal mappings use explicit material until the relevant S15 scope/confidence contract is
accepted. No implicit winner from TonalContext enters generation.

### Phase 8 — script-generating composers and human feedback

Allow rule-based/evolutionary clients to emit Swang programs and mutate meaningful AST
nodes:

```text
depth 2 -> 3
traversal snake -> morton
contour parallel -> contrary
rhythm pipeline from parent A + pitch pipeline from parent B
```

S9 may compare declared single-axis deltas and store preference evidence. LLM-generated
scripts are an optional client after the compiler, validator, provenance, and safety
limits are stable.

### Phase 9 — graph/DP recipes

After S7 exists, Swang may expose graph route requests and versioned weight vectors. It
calls the S7 traversal engine; it does not implement a second DP framework.

## Required controls

1. `griff-core` never depends on the Swang parser crate.
2. No permanent score/event hierarchy exists beside canonical `Score`.
3. No ambient randomness or unversioned semantic dependency exists.
4. No expansion silently exceeds depth/cell/event/time/polyphony limits.
5. No ASCII dimension silently means time, pitch, voice, or track.
6. No inferred MIDI technique/position is emitted as explicit Guitar Pro evidence.
7. No lifted high-level construct is accepted without re-execution verification.
8. Exact residuals and low structural coverage remain visible.
9. No automatic TonalContext scope/confidence/generation policy enters through Swang.
10. Parser, evaluator, recognizers, and canonical transforms receive fuzz coverage.
11. Existing import/export/generation behavior remains unchanged until an individual
    phase explicitly changes and validates it.

## Stage acceptance

S16 closes only when all of the following are true:

- a versioned Swang grammar, formatter, evaluator, and diagnostic contract exist;
- one program uses high-level structure plus exact escape hatches and builds into a
  valid canonical score;
- one GP and one MIDI fixture lift into verified programs with format-appropriate loss
  reporting;
- lossless lifting round-trips the canonical GP fixture through exact residuals;
- structural lifting explains a meaningful majority of the selected fixture without
  hiding the remainder;
- the pattern core and `fractalize` are bounded, deterministic, property-tested, and
  fuzzed;
- pattern-generated rhythm reaches S6 only through approved structured inputs;
- a generated or mutated Swang program can be edited, rebuilt, and compared through an
  inspectable AST/score delta;
- no competing score model, hidden tonal policy, or general-purpose execution surface
  has been introduced.

## Non-goals

- No general-purpose programming language, arbitrary recursion, host-code execution, or
  filesystem/network access from scripts.
- No raw MIDI-byte blocks.
- No claim that fractal structure is automatically musical or that every mathcore riff
  is fractal.
- No direct fret/string choice inside `fractalize`.
- No original Guitar Pro container byte reconstruction.
- No proof that a lifted program matches the composer's historical intent.
- No immediate replacement of cockpit controls; the UI may later edit/serialize the
  same AST.
- No neural prerequisite.
- No universal LED/robotics/animation product. A domain-neutral pattern crate may be
  reused later, but S16 is music-first.

## See also

- [`../adr/0028-swang-authoring-and-verified-lifting.md`](../adr/0028-swang-authoring-and-verified-lifting.md)
- [`S6-rule-generator-v0.md`](S6-rule-generator-v0.md)
- [`S7-graph-layer.md`](S7-graph-layer.md)
- [`S9-feedback-layer.md`](S9-feedback-layer.md)
- [`S13-complementary-part-generation.md`](S13-complementary-part-generation.md)
- [`S15-tonal-context-and-harmonic-control.md`](S15-tonal-context-and-harmonic-control.md)
- [Issue #108](https://github.com/PhysShell/griff/issues/108)
