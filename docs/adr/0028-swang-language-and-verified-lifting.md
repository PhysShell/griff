# ADR 0028: Adopt Swang as a deterministic authoring and lifting language

Date: 2026-07-14
Status: Proposed

## Context

Griff already has a canonical symbolic model, deterministic generation, structured
analysis, scoring, Guitar Pro import, MIDI interchange, and growing arrangement
controls. The missing authoring surface is compositional: a user can change CLI or
UI parameters, but cannot yet save the musical construction itself as a compact,
readable, diffable program.

A naive textual format would only serialize every note at the lowest level. That is
useful for debugging and exact patches, but it does not recover motifs, repeats,
transpositions, masks, structural transforms, or the relationship between generated
parts. MIDI and Guitar Pro contain the executed result, not the original program, so
recovering higher-level constructs is a program-synthesis problem rather than a
syntax conversion.

The same language can serve both directions:

```text
Swang program -> Griff execution -> canonical Score
canonical Score -> verified lifting -> optimized Swang program
```

This gives Griff an explainable target for rule-based, evolutionary, and later
neural/LLM generators: generate editable programs, not only opaque event lists.

## Decision

We adopt **Swang** (Swan Language) as a proposed deterministic musical authoring and
lifting language over Griff's canonical `Score`.

### 1. Canonical Score remains the single musical model

Swang is a frontend and orchestration language. It lowers into existing typed Griff
requests, transformations, and canonical score events. `griff-core` must not depend
on Swang syntax, and Swang must not introduce a permanent parallel hierarchy such as
`SwangScore`, `SwangTrack`, and `SwangNote` that competes with `Score`.

The dependency direction is:

```text
griff-core <- griff-swang <- CLI / preview / future plugin clients
```

An execution plan may exist between the AST and `Score`, but it is ephemeral
orchestration, not a second source of musical truth.

### 2. Swang has three explicit abstraction levels

- **L2: recipes and musical intent.** `generate`, `complement`, section scopes,
  named policies, structural objectives, and later graph/DP route requests.
- **L1: pattern algebra.** Motifs, rhythm cells, masks, repeat, rotate, mirror,
  stretch, interleave, quantize, and bounded recursive transforms such as
  `fractalize`.
- **L0: exact escape hatch.** Notes, rests, event groups, techniques, positions,
  and local `replace` / `overlay` / `delete` patches that lower directly into
  canonical score structures.

Raw MIDI bytes are not an escape hatch. MIDI remains a boundary adapter.

### 3. Compilation is deterministic, versioned, bounded, and explainable

A Swang program pins its language/semantic version. Every operation is either pure
or explicitly seeded. Identical source, semantic version, inputs, and seed produce a
byte-stable normalized expansion and a semantically identical canonical `Score`.

Recursive and combinatorial operators declare hard budgets such as:

```text
max_depth
max_cells
max_events
min_duration
max_polyphony
```

The compiler rejects an over-budget expansion or applies an explicitly selected,
deterministic pruning policy. It never truncates silently.

`griff swang expand` and provenance data expose the lowering steps from high-level
constructs to exact events.

### 4. Fractal structure is an independent structural transform

`fractalize` operates on active/empty structural positions, not directly on MIDI
notes, pitches, strings, or techniques. An active parent expands into a scaled copy
of a substitution kernel; an empty parent expands into an equally-sized empty
subtree.

The pipeline remains separated:

```text
structural pattern
  -> traversal / linearization
  -> time mapping
  -> tonal mapping
  -> dynamics and technique mapping
  -> fretboard realization
```

Two-dimensional ASCII patterns have explicit semantics and traversal. Rows are not
silently treated as voices, and columns are not silently treated as time.

### 5. Text may seed structure, not directly dictate notes

A versioned grapheme, phonetic, or stable-hash encoder may turn text into a structural
mask. Text can control occupancy, branching, boundaries, cycles, and accents. Pitch
and guitar realization happen in later typed stages.

This avoids the primitive `letter -> note` mapping while preserving deterministic
text-to-music experiments.

### 6. Lifting reconstructs programs, not dumps

`griff swang lift` imports MIDI or Guitar Pro through the normal adapters and searches
for a compact Swang program that explains the canonical `Score`.

Initial recognizers include:

```text
exact and approximate repeats
motif definitions and references
transposed motifs
shared rhythm with changing pitch material
variation with changed endings
masks / overlays
later: multi-scale self-similarity and fractal candidates
```

Guitar Pro can provide explicit positions, techniques, voices, tuning, and repeat
structure. MIDI lifting is necessarily weaker and must retain the adapter's loss
report; it cannot claim to recover guitar techniques or fretboard positions that the
source did not contain.

### 7. Every lifted program is verified by re-execution

The lifting pipeline is:

```text
source file
  -> canonical source Score
  -> candidate Swang AST
  -> optimize
  -> execute
  -> reconstructed Score
  -> semantic diff / LiftReport
```

A lifted result carries:

```text
structural coverage
exact residual ratio
recognized constructs
source and grammar digests
semantic differences
loss/provenance report
```

Three modes are allowed:

- **lossless:** exact canonical reconstruction, with unmatched detail represented by
  L0 residual patches;
- **structural:** declared normalization tolerances for timing, velocity, duration,
  and other selected facts;
- **generative:** a compact recipe preserving declared structural and musical axes,
  without claiming exact reconstruction.

Mode and tolerances are always explicit.

### 8. Exact residuals are a first-class success path

Real music does not need to fit every high-level construct perfectly. A lifted
program may combine motifs, repeats, transforms, and exact residual patches.

The optimizer prefers programs that explain more events with stable, readable
constructs, but it never fabricates a false pattern merely to avoid residuals.
`residual_ratio = residual_events / total_events` is reported rather than hidden.

### 9. Program selection uses an inspectable description cost

Candidate programs are compared with an explicit, versioned objective shaped like:

```text
cost(program) =
    reconstruction_error
  + AST_size_penalty
  + residual_penalty
  + obscure_construct_penalty
  + instability_penalty
```

The exact weights require fixtures and review. Minimum-description-length is the
organizing principle, not a claim that the shortest possible syntax is always the
most musical or readable.

The first implementation uses bounded recognizer passes and deterministic dynamic
programming where needed. An e-graph optimizer may be evaluated later only after the
rewrite vocabulary and extraction objective are proven useful.

### 10. Generators may produce Swang programs

Rule-based and evolutionary clients may emit or mutate typed Swang ASTs. The normal
compiler remains responsible for validation, execution, scoring, and canonical
output.

Human feedback can then operate on meaningful declared changes such as `depth`,
`traversal`, `density_decay`, contour, rhythm mapping, and articulation policy.
An LLM program writer is a later optional client, not a core dependency or an
acceptance requirement.

### 11. The generic structural core stays domain-neutral; Swang stays music-first

Pure pattern primitives may be reusable for other discrete signals such as LED
sequences, pixel grids, gates, or control timelines. That reuse is permitted only
below the musical mapping layer.

Swang itself remains a music-first Griff language. This ADR does not expand Griff
into a general automation or embedded-control platform.

### 12. Roadmap ownership is S16

Swang authoring, exact score text, verified lifting, optimizer passes, and program
synthesis are owned by the proposed append-only stage S16. S16 consumes stable
contracts from other stages but does not reopen them implicitly.

In particular, TonalContext Phase 1 remains accepted, closed, and frozen. Swang may
represent an explicit tonal context, but automatic scope selection, confidence
thresholds, and generation influence require their own accepted S15 contracts.

## Consequences

- Griff gains a reproducible textual experiment and authoring surface instead of
  relying only on transient UI state and long CLI argument lists.
- MIDI/GP import can produce compact executable explanations rather than only event
  dumps, with verification preventing persuasive but false reconstructions.
- Generation, decompilation, mutation, and human editing converge on one typed AST.
- Exact residuals preserve fidelity while higher-level constructs provide
  readability and compression.
- The pattern algebra can support distinctive deterministic mathcore/swancore
  structure, including bounded recursive self-similarity.
- The implementation is substantial and must ship as vertical slices; attempting
  the full language, optimizer, decompiler, UI, and LLM client together would create
  a compiler-shaped sinkhole.
- Some source material will not admit a useful compact explanation. The system must
  report low structural coverage rather than pretending otherwise.
- General discrete-signal reuse remains possible at the pattern-core boundary but is
  not a product commitment for Griff.

## Related

- Issue #108: Swang roadmap proposal
- ADR-0002: canonical score model
- ADR-0003: master timeline single source of truth
- ADR-0010: fuzz format adapters and core invariants
- ADR-0013: DP/Viterbi traversal
- ADR-0017: explainable scoring contract
- ADR-0018/0019: rich note model and fretboard inference
- S15: tonal context and harmonic control
- S16: Swang language and verified lifting
