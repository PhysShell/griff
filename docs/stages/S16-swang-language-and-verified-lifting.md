# S16: Swang language and verified lifting

Status: proposed
Depends on: S1 (canonical score), S2/S3 (MIDI and Guitar Pro adapters), S4
(phrase boundaries), S6 (rule generator)
Builds on: S13 relation specs, S14 structure controls, S15 explicit tonal
contracts when accepted
Feeds: S7 graph/DP clients, S8 cockpit, S9 feedback, S11 regeneration, S12
neural assistance, S13 ComplementArranger
Decision: ADR-0029
Normative semantics: [`../swang/spec.md`](../swang/spec.md)
Tracking: #108

## Goal

Make Griff programmable at the level of musical structure.

Swang (Swan Language) is a deterministic, versioned DSL which can:

1. compile readable musical recipes and structural transforms into canonical
   `Score` values;
2. provide exact typed patches when high-level structure is insufficient;
3. lift imported MIDI/Guitar Pro scores into compact executable Swang
   programs;
4. verify every lifted program by executing it and comparing it with the
   source;
5. let generators emit editable programs rather than only opaque final
   events.

The stage succeeds when a short program explains and reproduces a non-trivial
guitar fragment, exposes its residual exceptions and provenance, and can be
edited to produce a predictable structural variation.

The phases are ordered **pattern-first**: the first implementation work
proves the risky hypothesis (a hierarchical pattern can pass through the
existing S6 generator and produce a useful, audible result), not the safe one
(that a parser and formatter can be written). Parsers have been written
before; audibly good deterministic mathcore out of a recursive structure has
not.

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

The canonical `Score` remains the single musical model. The Swang AST records
a program; the execution plan records transient work. Neither duplicates the
score hierarchy.

Crate layout (ADR-0029): `griff-pattern` is the pure structural algebra —
std-only, no external dependencies, no `griff-core`; `griff-swang` owns the
AST, the parser/formatter (from Phase 3), and the lowering into `griff-core`
types. `griff-core` never depends on either.

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

L2 compiles relative intent into existing generation, complement, structure,
scoring, and validation contracts.

### L1 — pattern algebra

```text
pattern seed = ascii {
    X . X
    X X .
    . X X
}

rhythm r = seed
    |> fractalize(depth = 2, max_cells = 512, density_bps = 8000, seed = 17)
    |> linearize(snake)
    |> map_rhythm(unit = 1/16, tail = rest_pad)
```

(The produced palette's bar count is a property of the expansion; how many
bars are *generated* from it is `--bars`, which rotates the palette and
never stretches it — see the spec §1.11.)

The v0.1 operators speak **onsets and durations only** (ADR-0029 §7).
Specified and scheduled, with semantics in the spec:

```text
fractalize
linearize
map_rhythm
```

`thin` sits between the tiers: its **type contract** is fixed (spec §1.10 —
it may only flip `X -> .`, preserving dimensions and sequence length), but
its cell-selection rule is deliberately unspecified and it ships in no phase
until that rule earns its own spec section.

Candidate roster — names under consideration, with **no promised semantics
and no assigned delivery phase** until each earns its spec section:

```text
repeat
rotate
mirror
mask
quantize
euclid
```

`accent` and dynamics are deferred — the S6 seam has no landing place for
them, and widening a load-bearing public struct is its own later phase, not a
rider on the language's first slice.

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

The normative statement of these contracts is
[`../swang/spec.md`](../swang/spec.md); this section is the summary.

### Determinism and language levels

A script pins a monotonic integer language level; levels are additive-only,
and the level check is a frozen first-line pre-parser. Every operation is
pure or explicitly seeded; the pruning seed is independent of the generation
seed. Identical declared inputs produce a byte-stable normalized expansion
and a semantically identical `Score`. No floats in semantics, no
platform-sized integers in hashed state, no OS entropy, no ambient
randomness.

### Bounded evaluation

Structural limits (`max_depth`, `max_cells`) are required arguments in
`griff-pattern` — the library ships no defaults; time-domain limits
(`max_events`, `min_duration`, `max_polyphony`) are enforced by the lowering
in `griff-swang`. A breach is a typed error carrying the offending
`NodePath`. Pruning requires an explicit policy; there is no silent
truncation.

### Structural separation

An active pattern cell is a structural position, not a note. The pipeline
keeps these operations separate:

```text
occupancy / hierarchy
  -> traversal
  -> time mapping
  -> pitch / tonal mapping
  -> dynamics and articulation
  -> fretboard realization
```

A kernel's axes have no implicit meaning: rows are not voices, columns are
not time, ragged kernels are a typed error. v0.1 traversals are `row_major`
and `snake`; `linearize` preserves every cell, and an inactive cell becomes a
timed rest, never silently removed. The drum-machine reading (columns =
time, rows = simultaneous voices) is a separate future lowering with a
distinct output type, never a traversal and never a default.

### Verified lifting

Every lifted result carries:

```text
source digest and format losses
Swang language level and cost-policy versions
recognized constructs and alternatives
source event count
construct-explained event count
exact residual event count
structural coverage
semantic differences after re-execution
```

A selected program is the best explanation under a documented policy, not a
claim about the historical composition process.

## Lift modes

### Lossless

```text
execute(lift(score, lossless)) ~= score
```

All canonical differences are represented by exact residual patches. Equality
is semantic canonical-model equality; original container bytes are not
reconstructed.

### Structural

Permits declared normalization of MIDI noise or performance detail, for
example timing or velocity tolerance, while preserving explicit rhythmic,
pitch, grouping, and phrase invariants. Every normalization appears in the
report.

### Generative

Finds a compact recipe that preserves selected identity axes such as rhythm,
contour, density, tonal material, technique distribution, and structure
period. It is intended for controlled regeneration, not archival
reconstruction.

## Optimization objective

Initial lifting uses explicit recognizers plus deterministic dynamic
programming. The versioned cost has the shape:

```text
cost(program) =
    reconstruction_error
  + AST_size_penalty
  + exact_residual_penalty
  + obscure_construct_penalty
  + representation_instability_penalty
```

The optimizer must not prefer a clever but unreadable transform chain over a
slightly longer stable program. Description length is a valid ranking signal
only within one language level and cost-policy version; the API forbids
cross-version comparison. Equality saturation is deferred until a measured
set of rewrite rules makes it useful.

## Phases

**Status:** Phases 0–3 are **shipped, accepted, and frozen** (PRs
#109–#121). The Swang semantic core (spec §1) and surface grammar (spec §3)
are frozen — further change requires a new language level. `griff swang
check | fmt | expand | build` exist and are held to the seven §3.5 laws;
the parser and pattern core are fuzzed on the blocking CI gate. Phases 4–9
(exact score text, patches, structural recognizers, optimizing verified
lift, recipes, script-generating composers, graph/DP recipes) remain future
work under ADR-0029. The next scope is **S8 Swang Playground** — the
authoring loop that lets a human actually play these four verbs.

### Phase 0 — design contract and golden fixture *(done)*

- accept or revise ADR-0029; land `docs/swang/spec.md` and **freeze its
  semantic core by this phase's acceptance** — until then it is Proposed,
  like the ADR — keeping the other sections explicitly unstable;
- define typed units, diagnostics (the `SWG0001`-style registry the spec
  opens with), limits, and the
  `swang-prune-hash-v1` function with its golden `(seed, path) -> u64`
  vectors;
- select one legally safe 4–8 bar Guitar Pro fixture from the corpus plus a
  MIDI projection; record exact baseline digests;
- define the expansion-artifact schema (versioned, canonical, byte-stable).

No generation behavior changes.

### Phase 1 — pure pattern core (`griff-pattern`) *(done)*

Add domain-neutral primitives, std-only:

```text
Kernel / NodePath
Expansion
Traversal (row_major | snake)
ActivitySequence
PruneSpec / DensityBps / ExpansionBudget
```

There is deliberately **no materialized `PatternTree`**: `fractalize`
answers each cell of the final `Expansion` grid from its coordinate digits
(most-significant first — the digits *are* the implicit tree path), so the
tree exists in the addressing scheme rather than in memory. The normative
pruning semantics are unchanged; the intermediate representation of the
first draft is simply not needed (decisions log 2026-07-14).

Implement deterministic `fractalize`, traversal, and path-addressed pruning
without MIDI, tonal, fretboard, UI, serde, or generator dependencies.

Acceptance:

- empty parents produce empty descendants;
- expansion never exceeds the declared budget, and every breach error —
  depth or cells — carries the offending `NodePath`;
- transforms are deterministic and compositional;
- two-dimensional kernels require explicit traversal; ragged kernels are
  rejected before allocation;
- golden coordinate/activity vectors for both traversals; golden
  `swang-prune-hash-v1` vectors; property tests over depth, shape, pruning,
  and degenerate inputs.

### Phase 2 — pattern-to-rhythm vertical slice *(done)*

Map `ActivitySequence` into placed `(offset, duration)` `RhythmTemplate`
values, cut per bar with an explicit tail policy, and feed them through the
shared generation compiler as an explicit rhythm override with precedence
`explicit pattern > corpus > source first bar` (corpus novelty and gesture
stay corpus-based).

Driven from `griff generate` through a namespaced, temporary transport
syntax — **not** an early Swang grammar:

```text
griff generate seed.gp5 out.mid \
  --bars 8 \
  --seed 42 \
  --rhythm-kernel 'X.X/XX./.XX' \
  --rhythm-fractal-depth 2 \
  --rhythm-density-bps 8000 \
  --rhythm-seed 17 \
  --rhythm-traversal snake \
  --rhythm-unit 1/16 \
  --rhythm-max-cells 4096 \
  --rhythm-tail rest-pad \
  --emit-rhythm-expansion expansion.json
```

Acceptance (the full list lives in the spec):

- without pattern flags, generation is byte-identical to baseline;
- `--rhythm-unit` is required; `--rhythm-density-bps` is an integer
  `0..=10000` and requires `--rhythm-seed`;
- `.` produces a gap in offsets, adjacent `X` stay two short notes, tail
  policy `reject` (default) / `rest-pad` behaves exactly as specified,
  `--bars` rotates the palette and never stretches the pattern;
- `--seed` does not change the expansion artifact; `--rhythm-seed` does not
  change pitch material or the candidate seed sequence;
- the expansion artifact is byte-stable, versioned, and its fingerprints
  come from the public `rhythm_diagnostics` (no duplicated hashing);
- changing only `depth` produces an explainable artifact delta;
- existing S6 strategies and reranking remain unchanged.

This is the stage's killer demo: a fractal kernel audibly realized through
the unchanged generator, reproducible from a one-line delta.

### Phase 3 — minimal Swang parser and canonical formatter *(done)*

The grammar covers only what the Phase 2 killer demo **audibly earned**
(spec §3, the closure verdict on #116): `pattern`, `ascii`,
`fractalize(depth, max_cells, density, seed)` — the cell budget is a
**required word** (the library ships no default and the language invents
none) and density/seed stay a visible pair — `linearize(traversal)`,
`map_rhythm(unit, tail)`,
`generate(source, bars, seed, candidates, strategy, corpus)` — `source`
(the seed score: pitch material, range, PPQN, meter, tempo) and
`candidates` are **required words** too; a program names every semantic
dependency of its run — and `export`, the output's **single owner**
(`griff swang build` takes no output flag). The **strategy policy is
explicit in the AST** — the dense demo proved the audible result is decided
between the expansion and the ear (`repeat_variation` held one template of a
six-template palette), and a language that hides that choice under-tells.
`gesture`, `thin`, pitch/fretboard transforms stay out (spec §3.4).

CLI:

```text
griff swang check riff.swg
griff swang fmt riff.swg
griff swang expand riff.swg
griff swang build riff.swg
```

Acceptance — Phase 3 adds **no musical semantics** (the seven laws of spec
§3.5):

- a Swang program equivalent to a Phase-2 CLI command produces a
  byte-identical expansion JSON (`expand` stops after `map_rhythm`) —
  scoped to the **canonical transport subset** the grammar can express:
  the transport's inert `--rhythm-seed` without density is deliberately
  unexpressible, and no parity is claimed for it;
- `fmt` is canonical and idempotent: `format(parse(text)) ==
  canonical_text` and `fmt(fmt(s)) == fmt(s)`; `parse(format(ast)) == ast`;
- `check` returns the same SWG **codes**, with locations layered per §1.5:
  syntax- and transport-class errors carry a **source span** (the Phase-2
  flag location class retires with the transport), structural errors keep
  their `NodePath`;
- `build` parity is split by strategy policy: under `strategy auto` and
  the same seeds it matches the existing `griff generate`; under a named
  strategy it selects the first ranked candidate of that strategy from
  the unchanged, already-ranked set — selection only, never a
  re-generation;
- hand-written recursive-descent parser (initial strategy per ADR-0029 §11)
  emitting `Vec<Diagnostic>` — pure data with spans and stable codes,
  rendered only at the CLI edge; no defaults invented over the frozen
  semantics — which is why `max_cells`, `source`, and `candidates` are
  required words, not optional ones;
- parser and expansion limits gain fuzz targets (ADR-0010).

### Phase 4 — exact canonical score text and patches

```text
griff swang dump input.gp5 > input.swg
griff swang verify input.swg --against input.gp5
```

Required laws:

```text
parse(format(score)) ~= score        (semantic canonical-model equality)
format(parse(text)) == canonical_text
```

Acceptance:

- every canonical event/group/track/master-bar fact in scope is
  representable; GP/MIDI adapter losses remain visible;
- `exact` / `replace` / `overlay` / `delete` patches address events by
  stable identity, not by fragile positional index alone;
- alphaTab issue #1484's expressibility list is covered by negative tests;
- property/fuzz coverage over parser, formatter, and patch boundaries.

### Phase 5 — structural recognizers

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

Recognizer output is evidence with source ranges and costs, not an immediate
rewrite.

Acceptance:

- each recognizer has positive, negative, and adversarial fixtures;
- accepted constructs reduce program cost and residual size;
- false-positive controls prove that repeated pitches alone do not imply a
  motif;
- GP and MIDI lift reports distinguish available and missing source
  semantics.

### Phase 6 — optimizing verified lift

Assemble candidate fragments into a global program with deterministic DP,
execute it, and select the lowest-cost verified explanation.

```text
griff swang lift input.gp5 --mode lossless|structural|generative
griff swang roundtrip input.gp5
griff swang explain input.swg
```

Acceptance:

- lossless mode reconstructs the canonical fixture exactly through
  residuals;
- structural coverage and residual ratio are reported honestly;
- identical inputs produce byte-stable selected programs and reports;
- at least one source admits two retained alternative explanations;
- changing the cost-policy version never silently rewrites stored programs.

### Phase 7 — recipes, complement, and exact patches

Add named sources, sections, policies, transforms, `generate`, and
`complement` recipes. Compile them into existing S6/S13 requests and allow
exact local repair.

Tonal mappings use explicit material until the relevant S15 scope/confidence
contract is accepted. No implicit winner from TonalContext enters
generation.

### Phase 8 — script-generating composers and human feedback

Allow rule-based/evolutionary clients to emit Swang programs and mutate
meaningful AST nodes:

```text
depth 2 -> 3
traversal snake -> row_major
contour parallel -> contrary
rhythm pipeline from parent A + pitch pipeline from parent B
```

S9 may compare declared single-axis deltas and store preference evidence.
LLM-generated scripts are an optional client after the compiler, validator,
provenance, and safety limits are stable.

### Phase 9 — graph/DP recipes

After S7 exists, Swang may expose graph route requests and versioned weight
vectors. It calls the S7 traversal engine; it does not implement a second DP
framework.

## Deferred research candidates

Named here without reserved syntax or promised semantics:

- **hierarchical / fractal structure inference** (demoted from a promised
  phase by ADR-0029 §10): admitted to the roadmap only with synthetic
  identifiable fixtures, a bounded candidate grammar, a demonstrated
  description-cost win over the recognizer baseline, negative controls, and
  honest residual behavior;
- generalized-radix Z-order / Peano / Hilbert-family traversals (a 3×3
  recursive kernel needs a separately specified radix/child-order contract);
- hierarchical tree traversal of intermediate expansion levels;
- the polyphonic lowering (`Pattern2D -> PolyphonicPattern`, the explicit
  drum-machine reading);
- compaction (`compact` / `remove_rests`) and articulation merges
  (`merge_adjacent` / `tie_adjacent` / `sustain_runs`) — both change the
  time-slot contract and therefore need their own types;
- accent/velocity in the pattern seam (requires widening `TemplateNote`
  behind characterization tests);
- text-to-structure encoders (operate on Unicode scalar values, never
  grapheme clusters; pin Unicode semantics in the script header);
- static growth-class prediction via the expansion matrix's spectral radius
  (report the budget-exhaustion depth before expanding).

## Required controls

1. `griff-core` never depends on `griff-swang` or `griff-pattern`.
2. `griff-pattern` stays std-only; no serde, no `griff-core`, no time types.
3. No permanent score/event hierarchy exists beside canonical `Score`.
4. No ambient randomness or unversioned semantic dependency exists; the
   pruning seed and generation seed are independent axes.
5. No expansion silently exceeds depth/cell/event/time/polyphony limits.
6. No ASCII dimension silently means time, pitch, voice, or track; inactive
   cells are never silently dropped.
7. No inferred MIDI technique/position is emitted as explicit Guitar Pro
   evidence.
8. No lifted high-level construct is accepted without re-execution
   verification.
9. Exact residuals and low structural coverage remain visible.
10. No automatic TonalContext scope/confidence/generation policy enters
    through Swang.
11. Parser, evaluator, recognizers, and canonical transforms receive fuzz
    coverage (ADR-0010).
12. Existing import/export/generation behavior remains unchanged until an
    individual phase explicitly changes and validates it.

## Stage acceptance

S16 closes only when all of the following are true:

- a versioned Swang grammar, formatter, evaluator, and diagnostic contract
  exist;
- one program uses high-level structure plus exact escape hatches and builds
  into a valid canonical score;
- one GP and one MIDI fixture lift into verified programs with
  format-appropriate loss reporting;
- lossless lifting round-trips the canonical GP fixture through exact
  residuals;
- structural lifting explains a meaningful majority of the selected fixture
  without hiding the remainder;
- the pattern core and `fractalize` are bounded, deterministic,
  property-tested, and fuzzed;
- pattern-generated rhythm reaches S6 only through the explicit rhythm
  override;
- a generated or mutated Swang program can be edited, rebuilt, and compared
  through an inspectable AST/score delta;
- no competing score model, hidden tonal policy, or general-purpose
  execution surface has been introduced.

## Non-goals

- No general-purpose programming language, arbitrary recursion, host-code
  execution, or filesystem/network access from scripts.
- No raw MIDI-byte blocks.
- No claim that fractal structure is automatically musical or that every
  mathcore riff is fractal.
- No direct fret/string choice inside `fractalize`.
- No original Guitar Pro container byte reconstruction.
- No proof that a lifted program matches the composer's historical intent.
- No immediate replacement of cockpit controls; the UI may later edit and
  serialize the same AST.
- No neural prerequisite.
- No universal LED/robotics/animation product. `griff-pattern` may be reused
  later, but S16 is music-first.

## See also

- [`../adr/0029-swang-authoring-and-verified-lifting.md`](../adr/0029-swang-authoring-and-verified-lifting.md)
- [`../swang/spec.md`](../swang/spec.md)
- [`S6-rule-generator-v0.md`](S6-rule-generator-v0.md)
- [`S7-graph-layer.md`](S7-graph-layer.md)
- [`S9-feedback-layer.md`](S9-feedback-layer.md)
- [`S13-complementary-part-generation.md`](S13-complementary-part-generation.md)
- [`S15-tonal-context-and-harmonic-control.md`](S15-tonal-context-and-harmonic-control.md)
- [Issue #108](https://github.com/PhysShell/griff/issues/108)
