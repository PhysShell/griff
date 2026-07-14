# S16: Swang language and verified lifting

Status: proposed — architecture and delivery plan only
Depends on: S1 (canonical score), S2/S3 (MIDI and Guitar Pro adapters), S6
(rule generator), S14 (structure controls)
Builds on: ADR-0028
Feeds: S7 graph/DP clients, S8 preview/cockpit, S9 feedback, S11 regeneration,
S13 ComplementArranger; S15 tonal contracts only after their own acceptance

## Goal

Create **Swang** (Swan Language), a deterministic executable language for writing,
transforming, inspecting, and recovering Griff music programs.

Swang must support both directions:

```text
Swang source
  -> parse / type-check / lower
  -> Griff generators and transforms
  -> canonical Score
  -> MIDI / Guitar Pro projections
```

```text
MIDI / Guitar Pro
  -> canonical Score
  -> structural analysis and program synthesis
  -> optimized Swang source
  -> re-execute and verify
```

The stage is successful when Griff can represent a non-trivial guitar fragment as a
compact, editable program, rebuild it deterministically, and prove how closely the
program reconstructs its source.

A note-per-line dump alone does not close S16. That is debugging syntax wearing a
compiler costume.

## Product thesis

Swang is a better experimentation surface than a cockpit containing hundreds of
independent knobs. A script records not only values, but relationships, scopes,
transforms, and intent:

```text
section verse bars 1..8 {
    part guitar_b = complement guitar_a {
        relation rhythm_lock

        bars 1..4 {
            fractal depth 2
            density_decay 0.90
        }

        bars 5..8 {
            fractal depth 3
            density_decay 0.72
            preserve downbeats
        }
    }
}
```

A musical experiment becomes a reviewable diff:

```diff
- depth 2
- traversal snake
+ depth 3
+ traversal morton
```

The same AST becomes an editable output target for rule-based generation,
evolutionary search, and later optional neural/LLM clients.

## Architecture

```text
Swang text
  -> Surface AST
  -> resolved typed AST
  -> bounded execution plan
  -> griff-core requests / transforms
  -> canonical Score
  -> validators / scoring / exports
```

The reverse path is:

```text
canonical Score
  -> normalized facts
  -> recognizer candidates
  -> global program search
  -> optimizer / canonical formatter
  -> execute candidate program
  -> semantic diff + LiftReport
```

### Dependency direction

```text
griff-core <- griff-swang <- griff-cli / UI clients
```

`griff-core` must remain independent of parser syntax. Swang may reference canonical
core types and generation requests, but it must not define a second permanent score
model.

## Language levels

### L2 — recipes and musical intent

Examples:

```text
part riff = generate {
    bars 8
    rhythm from pattern verse_grid
    material explicit E minor
    seed 42
}

part guitar_b = complement guitar_a {
    relation rhythm_lock
    register below 12st
    density 0.65
    contour contrary
}
```

L2 compiles into existing typed requests, policies, and canonical transformations.
It does not bypass validation or directly manufacture MIDI events.

### L1 — pattern algebra

Initial operators:

```text
repeat
rotate
mirror
stretch / compress
interleave
overlay / subtract / mask
thin
accent
quantize
fractalize
linearize
map_rhythm
```

Every operator is pure or explicitly seeded, versioned, bounded, and independently
testable.

### L0 — exact score text and patches

```text
exact guitar_b bar 6 voice 0 {
    replace beat 3..4 with {
        at 3.0 note string 6 fret 7 length 1/16 marks [palm_mute]
        at 3.25 note string 5 fret 9 length 1/16 marks [slide]
        at 3.50 rest length 1/8
    }
}
```

L0 is the lossless residual and local repair surface. It lowers directly into
canonical `EventGroup` / `AtomEvent` data. Raw MIDI bytes remain outside the language.

## Pattern algebra and `fractalize`

A base structural kernel may be written as:

```text
pattern seed = ascii {
    X . X
    X X .
    . X X
}
```

`X` means an active structural position, not a note. `.` means an empty position.
For recursive expansion:

```text
active parent -> transformed/scaled copy of the base kernel
empty parent  -> an equally-sized empty subtree
```

A complete pipeline remains explicit:

```text
pattern seed
  |> fractalize(depth = 3, density_decay = 0.8, budget = 1024)
  |> linearize(snake)
  |> map_rhythm(unit = 1/16, bars = 4)
  |> map_pitch(material = explicit E minor)
  |> articulate(short = palm_mute, downbeat = accent)
  |> solve_fretboard(...)
```

The first implementation stops at `RhythmTemplate`; pitch, articulation, and
fretboard mapping arrive only through accepted later slices.

### Explicit two-dimensional semantics

An ASCII matrix does not silently define time or voices. It must be a substitution
kernel or carry an explicit mapping. Traversal is declared:

```text
row_major
snake
depth_first
morton
```

### Deterministic pruning

`density_decay` does not invoke ambient randomness. Initial valid policies include:

- stable path-hash selection under an explicit seed;
- stable rank-based pruning;
- explicit rule-based pruning.

### Hard budgets

Every recursive expansion declares or inherits limits:

```text
max_depth
max_cells
max_events
min_duration
max_polyphony
```

An over-budget operation returns a typed, source-located error or uses a named,
deterministic budget policy such as `prune_deepest` or `preserve_anchors`.

No silent truncation.

## Text as a structural seed

Text-to-music begins with structure rather than direct letter-to-note mapping:

```text
pattern glyph = text("glass hands")
    |> encode(graphemes, version = 1, seed = 17)
    |> fold(width = 5)
    |> mask(threshold = 0.52)
    |> fractalize(depth = 2)
```

The encoder may influence occupancy, branching, accents, boundaries, or cycle
length. Tonal and physical guitar realization remain separate stages.

Normalization, grapheme segmentation, phonetic dictionaries, and hashing algorithms
must be versioned so an old script does not change meaning after a dependency update.

## Verified lifting

`griff swang lift` is a structural decompiler/program synthesizer, not a pretty dump.
It searches for a Swang program that explains an imported canonical `Score`.

### Initial recognizers

```text
exact repeats
approximate repeats with an exact residual
motif definitions and references
transposed motif occurrences
shared rhythm with changing pitch material
changed endings
masks and overlays
```

Later recognizers may add:

```text
multi-scale self-similarity
fractal substitution candidates
complement relationships
structure-control recipes
```

A recognizer proposes typed candidate fragments. A deterministic global search
selects a non-overlapping explanation across the source timeline.

### Lifting modes

#### Lossless

```text
execute(lift(score)) ~= score
```

All unmatched detail is represented by exact L0 residuals. The equivalence relation
is defined over canonical score semantics, not source-container bytes.

#### Structural

Allows declared tolerances or normalization for selected properties such as MIDI
microtiming, velocity, or duration noise. The report lists every tolerance and
resulting difference.

#### Generative

Finds a compact recipe preserving declared axes such as rhythm identity, contour,
density, tonal material, structure period, and technique distribution. It does not
claim exact reconstruction.

Mode is mandatory and recorded in provenance.

### Minimum-description objective

Candidate programs are ranked by a versioned inspectable objective:

```text
cost(program) =
    reconstruction_error
  + AST_size_penalty
  + residual_penalty
  + obscure_construct_penalty
  + instability_penalty
```

The objective prefers compact, readable, stable explanations, not merely the
shortest token sequence. Weights are data and require fixture-based review.

The first implementation uses explicit recognizer passes and bounded dynamic
programming. E-graphs are deferred until the rewrite vocabulary and extraction cost
have evidence behind them.

### Exact residual

A valid lifted program may be:

```text
high-level motifs / repeats / transforms
+ exact residual patches
= verified reconstruction
```

The report exposes:

```text
source event count
construct-generated event count
exact residual event count
structural coverage
residual ratio
recognized constructs
semantic differences
source loss report
```

Low structural coverage is an honest result. The system must not manufacture a
fractal or motif explanation merely to make a demo look clever.

### MIDI versus Guitar Pro

Guitar Pro lifting may use explicit:

```text
string / fret positions
techniques
tuning
voices
repeat markers
```

MIDI lifting may use only facts actually present or explicitly inferred with
provenance. It cannot claim recovered source-of-truth guitar techniques or positions.

## Program-writing generators

A generator may eventually emit:

```text
part riff = motif "seed_17"
    |> fractalize(depth = 2, density_decay = 0.84)
    |> map_rhythm(unit = 1/16)
    |> map_pitch(material = explicit E minor, contour = rise_then_fall)
    |> articulate(short = palm_mute, downbeat = accent)
```

The ordinary compiler executes and validates the result. Evolutionary search mutates
meaningful AST nodes rather than moving arbitrary MIDI events:

```text
depth 2 -> 3
traversal snake -> morton
density_decay 0.85 -> 0.70
contour parallel -> contrary
```

S9 can later compare single-axis script deltas when candidate provenance proves that
only the declared axis changed.

## CLI target

```text
griff swang check riff.swg
griff swang fmt riff.swg
griff swang expand riff.swg
griff swang build riff.swg --output riff.mid
griff swang explain riff.swg

griff swang dump source.gp5
griff swang lift source.gp5 --mode lossless
griff swang optimize lifted.swg
griff swang verify lifted.swg --against source.gp5
griff swang diff original.swg variation.swg
griff swang roundtrip source.gp5
```

Command names are design targets, not accepted CLI commitments until their slice
lands.

## Delivery slices

### Phase 0 — contract and fixtures

- accept or revise ADR-0028;
- define semantic versioning, units, limits, error spans, and canonical formatting;
- define `Score` semantic equivalence for lifting verification;
- create synthetic/legal fixtures covering repeat, transposition, changed ending,
  irregular rhythm, and no-useful-structure controls;
- freeze baseline normalized score dumps and LiftReport schema.

No parser and no production behavior change.

### Phase 1 — canonical exact score text

- exact textual projection for `Score`, transport, tracks, voices, groups, notes,
  rests, marks, spans, positions, and provenance;
- parser plus canonical formatter;
- `dump`, `check`, and exact `build`;
- semantic round-trip and property tests;
- typed source-located errors;
- parser fuzz target.

Acceptance:

```text
parse(format(score)) ~= score
format(parse(text)) == canonical_text
```

### Phase 2 — pure Pattern Core

Introduce domain-neutral structural types:

```text
Pattern
Cell
PatternTree
NodePath
Traversal
ActivitySequence
FractalSpec
BudgetPolicy
```

Implement `fractalize`, deterministic pruning, and linearization without MIDI,
tonal, fretboard, UI, or filesystem dependencies.

### Phase 3 — Pattern to RhythmTemplate

Map an `ActivitySequence` into placed `(offset, duration)` rhythm templates and
integrate only through the existing S6 `source_rhythms` seam.

Existing pitch strategies, reranking, and canonical output remain unchanged.

### Phase 4 — minimal executable Swang

Restricted grammar:

```text
pattern
ascii
text / encode
fractalize
linearize
map_rhythm
generate
export
```

Add `check`, `fmt`, `expand`, `build`, and `explain` for this subset.

### Phase 5 — L0 exact patches

Add `exact`, `replace`, `overlay`, and `delete` over typed score addresses. Validate
range, overlap, duration, pitch, technique, and position invariants.

### Phase 6 — lifting v0

- exact repeat recognizer;
- motif extraction;
- transposed motif recognizer;
- changed-ending recognizer;
- global non-overlapping program selection;
- exact residual emission;
- execute-and-verify loop;
- `LiftReport` with structural coverage and residual ratio.

No fractal detection is required to close lifting v0.

### Phase 7 — optimizer and structural mode

- canonical rewrite rules such as nested-repeat collapse and transpose fusion;
- versioned description-cost policy;
- structural tolerances and semantic diff;
- adversarial controls proving that the optimizer rejects misleading compact
  explanations.

### Phase 8 — fractal lifting and multi-scale structure

- compare occupancy/self-similarity at multiple rhythmic resolutions;
- propose substitution kernels and traversal candidates;
- accept `fractalize` only when it improves the reviewed objective and passes
  reconstruction verification;
- report rejected fractal candidates and their cost deltas for debugging.

### Phase 9 — recipe layer and script synthesis

- `source`, `section`, `generate`, `complement`, named motifs/patterns/policies;
- rule-based AST generation and deterministic mutation;
- AST-aware crossover;
- program provenance and human edits;
- later optional LLM client behind the same parser/type checker.

### Phase 10 — graph/DP clients

After S7 exposes stable graph and traversal contracts, permit Swang to describe route
objectives and versioned weight vectors. Swang remains a client; it does not implement
a second graph or path engine.

## First killer demo

A narrow end-to-end proof:

```text
short text
  -> versioned grapheme mask
  -> bounded fractalize depth 2
  -> explicit traversal
  -> 1/16 RhythmTemplate
  -> existing S6 pitch strategy under explicit material
  -> canonical Score
  -> MIDI
  -> normalized expansion and provenance
```

Then prove the reverse on a four-bar synthetic/legally safe fixture:

```text
Score
  -> motif/repeat lift
  -> compact Swang
  -> execute
  -> exact semantic verification
```

The demo fails if the “lifted” program merely lists every note or if changing one
high-level parameter produces an unexplained global rewrite.

## Required controls

1. Identical program, inputs, semantic version, and seed produce byte-stable
   normalized expansion and semantically identical `Score`.
2. `griff-core` does not depend on the Swang parser crate.
3. No second canonical score/event hierarchy is introduced.
4. Empty fractal parents produce entirely empty descendants.
5. Expansion never exceeds declared budgets.
6. Pruning and traversal are deterministic and reported.
7. Two-dimensional pattern semantics are explicit.
8. Pattern Core contains no MIDI, tonal, fretboard, UI, or I/O dependency.
9. Pattern-to-generation integration uses approved typed seams such as
   `RhythmTemplate`, not raw MIDI.
10. Parser, expander, and patch boundaries have fuzz targets.
11. Exact patches reject invalid ranges, zero durations, impossible pitches, and
    invalid explicit positions.
12. Lifted lossless programs re-execute to the defined canonical equivalence.
13. Structural/generative modes record every tolerance and non-exact claim.
14. LiftReport exposes structural coverage and residual ratio.
15. MIDI lifting retains loss/provenance and never fabricates source-of-truth guitar
    semantics.
16. A no-structure control remains mostly residual rather than receiving a false
    motif/fractal explanation.
17. A compact but wrong candidate loses to a more exact program under the reviewed
    objective.
18. Existing generation behavior remains unchanged unless a phase explicitly changes
    and validates it.
19. TonalContext generation integration remains frozen until S15 accepts it.
20. Generic discrete-signal reuse stays below the music mapping boundary and does
    not expand Griff's product scope.

## Acceptance for the first S16 milestone

- [ ] ADR-0028 is reviewed and accepted or superseded.
- [ ] Exact score text parses, formats, and round-trips canonical fixtures.
- [ ] Pure bounded Pattern Core implements fractal expansion and traversal.
- [ ] Pattern output maps into placed `RhythmTemplate` values.
- [ ] A minimal `.swg` fixture builds a stable four-bar MIDI result.
- [ ] `expand` and `explain` expose the exact lowering/provenance chain.
- [ ] Lifting v0 recognizes repeat/motif/transposition on committed fixtures.
- [ ] Lifted lossless fixtures re-execute to semantic equivalence with explicit
      residuals.
- [ ] LiftReport reports coverage, residual, constructs, differences, and source
      losses.
- [ ] Property tests and fuzz targets cover parser, pattern expansion, and patches.
- [ ] No production TonalContext, S7, or neural behavior is changed.

## Non-goals

- No general-purpose programming language.
- No arbitrary `while`, unrestricted recursion, host-code execution, or hidden
  filesystem access.
- No raw MIDI-byte blocks.
- No hidden ambient randomness.
- No claim that compactness proves musical intent.
- No claim that all mathcore is fractal.
- No direct string/fret selection inside the structural fractal operator.
- No source-of-truth Guitar Pro technique recovery from plain MIDI.
- No requirement for full Guitar Pro container byte round-trip.
- No e-graph framework in the first optimizer.
- No LLM dependency for parser, compiler, lifting, or first script synthesis.
- No S7 path engine duplicated inside Swang.
- No automatic key/scope/confidence consumption while S15 contracts are frozen.
- No product commitment to Arduino, LED, robotics, or generic automation backends;
  only the low-level pattern algebra may remain reusable.

## See also

- [`../adr/0028-swang-language-and-verified-lifting.md`](../adr/0028-swang-language-and-verified-lifting.md)
- [`S6-rule-generator-v0.md`](S6-rule-generator-v0.md)
- [`S7-graph-layer.md`](S7-graph-layer.md)
- [`S9-feedback-layer.md`](S9-feedback-layer.md)
- [`S11-region-regeneration.md`](S11-region-regeneration.md)
- [`S13-complementary-part-generation.md`](S13-complementary-part-generation.md)
- [`S14-structure-controls-and-metrics.md`](S14-structure-controls-and-metrics.md)
- [`S15-tonal-context-and-harmonic-control.md`](S15-tonal-context-and-harmonic-control.md)
- Issue #108
