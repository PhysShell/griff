# Swang — language specification

Decision: [ADR-0029](../adr/0029-swang-authoring-and-verified-lifting.md).
Delivery plan: [S16](../stages/S16-swang-language-and-verified-lifting.md).

This document is normative. It is split by stability, and the split is the
point: the **semantic core** below is frozen — changing anything in it
requires a new language level — while the surface grammar is explicitly
unstable until S16 Phase 3 closes, and the transport syntax is a temporary
experiment contract, not early grammar.

Status of each section:

| Section | Stability |
|---|---|
| 1. Stable semantic core | **Frozen.** Changes require a new language level. |
| 2. Experimental transport syntax | Temporary Phase-2 contract; may be replaced by the grammar. |
| 3. Deferred research | No promised names, no promised semantics. |

## 1. Stable semantic core

### 1.1 Language level

Every Swang script pins a **monotonic integer language level** on its first
line. Nothing executes without it.

- The level check is **lexically trivial**: a frozen pre-parser reads only
  the first line and never changes across releases, so any interpreter can
  reject a newer file with an intelligible error even when the rest of the
  grammar is unparseable to it. This first-line syntax is the one piece of
  surface syntax frozen forever.
- An unknown or newer level is a hard typed error before any evaluation.
- Levels are **additive-only**: a new level may add syntax and builtins; it
  never changes the meaning of anything that parsed at an older level. Old
  scripts therefore work by construction, on one evaluator.
- The language level is **never an input to any content hash**. Hashes are
  taken over normalized forms; the level travels beside the hash, not inside
  it. (Prior art: Dhall's v6.0.0 standard-version-in-hash removal.)

### 1.2 Determinism law

Identical source, language level, and declared seeds produce:

1. a **byte-stable normalized expansion artifact**, and
2. a **semantically identical canonical `Score`**.

To make that law implementable rather than aspirational, the deterministic
core forbids:

- floating-point values in semantics (`f64` transcendentals are
  non-deterministic by Rust std's own documentation; durations and thresholds
  are rationals and fixed-point integers);
- platform-sized integers (`usize`/`isize`) in any hashed or serialized
  state — fixed-width types only;
- ambient randomness, OS entropy (`getrandom` must not appear in the
  dependency tree of the compiler), wall-clock time, and host-code execution;
- Unicode-classification functions whose results change with toolchain
  Unicode tables (`char::is_alphabetic` and friends) anywhere semantics can
  observe them.

### 1.3 Units

The closed unit set: `bar`, `beat`, `tick`, `st` (semitone), `string`,
`fret`, and rational note values (`1/16`). Units are typed; mixing them is a
type error, never a coercion.

### 1.4 Budgets

Expansion is bounded **before** realization, in the layer where each limit is
meaningful:

| Layer | Limits |
|---|---|
| `griff-pattern` (structural) | `max_depth`, `max_cells` |
| `griff-swang` lowering (time-domain) | `max_events`, `min_duration`, `max_polyphony` |

- The pattern library ships **no defaults**: `ExpansionBudget` is a required
  argument of `fractalize`; a constructor without it does not exist. Default
  profiles are a property of frontends (the CLI documents its own) and, from
  Phase 3, of a versioned language profile.
- A budget breach is a typed error carrying the offending `NodePath`. There
  is no silent truncation anywhere.

### 1.5 Diagnostics

Every diagnostic carries a stable typed code (`SWG____`), a source location
(a `NodePath` before the grammar exists; a source span after), and a
message. Codes are never reused. The compiler core emits diagnostics as pure
data; rendering happens only at the frontend edge.

### 1.6 Kernel semantics

An ASCII kernel is a **rectangular two-dimensional structural pattern**. Its
axes have no implicit musical meaning.

```text
X . X
X X .
. X X
```

- `X` is an active structural cell; `.` is an inactive structural cell.
  Neither denotes a note, rest, voice, pitch, or time position until an
  explicit lowering assigns that meaning.
- A kernel must be rectangular. Ragged rows are rejected before any
  allocation: `SWG____: ragged kernel: row 1 has 2 cells, expected 3`.
- Only `X` and `.` are cell characters.
- The drum-machine interpretation (columns = time, rows = simultaneous
  voices) is **not a traversal**; it is a separate future lowering with a
  distinct output type (`Pattern2D -> PolyphonicPattern`) and is never the
  default reading of a kernel.

### 1.7 `fractalize`

`fractalize(depth, density_decay, budget)` expands each active cell into a
scaled copy of the kernel and each inactive cell into an equally-sized empty
subtree. It chooses no pitches, strings, techniques, dynamics, or fingerings.

- An expansion node is addressed by its `NodePath` — the sequence of child
  indices from the root.
- A pruned (removed) parent yields an **entirely empty subtree**: no
  descendant of a removed node is ever active.
- `depth = 0` is the kernel itself.

### 1.8 Deterministic pruning — `swang-prune-hash-v1`

`density_decay` is a **path-addressed hash test**, not a stream of random
draws — the survival of a cell is a pure function of the pruning seed and the
cell's path, independent of evaluation order and of every other cell:

```text
keep(path, depth) =
    swang_prune_hash_v1(rhythm_seed, path) < threshold(decay_bps, depth)
```

- **Mixer.** `swang-prune-hash-v1` is the splitmix64 finalizer (Stafford
  Mix13 constants: `0xbf58476d1ce4e5b9`, `0x94d049bb133111eb`, shifts
  30/27/31 — public domain; prior art: Steele–Lea–Flood OOPSLA'14, Vigna's
  reference C), applied as an incremental fold down the tree:
  `key_child = mix(key_parent XOR encode(child_index))`, with
  `key_root = mix(domain_tag XOR rhythm_seed)`. No cryptographic properties
  are promised or required.
- **Path encoding** is injective and fixed-width little-endian: a domain
  separator, the algorithm version, the seed (`u64`), then each child index
  (`u32`) folded in order. `[1, 23]` and `[12, 3]` cannot collide because
  indices are folded stepwise, not concatenated as digits.
- **Decay** is carried as basis points (`DensityBps(u16)`, `0..=10000`),
  never a float. `threshold(decay_bps, depth)` is computed in integer
  arithmetic with `u128` intermediates and documented rounding
  (floor). Edge laws: `10000` keeps every cell at every depth; `0` keeps
  none below the root.
- **Golden vectors.** The implementation ships CI-checked
  `(seed, path) -> u64` vectors and `(seed, kernel, decay, depth) ->
  activity` vectors. Changing the mixer, encoding, or rounding is a new
  algorithm version and a new language level.

### 1.9 Traversals

Swang v0.1 supports exactly two linear traversals; both are explicit — there
is no default.

- `row_major` — rows left-to-right, top-to-bottom.
- `snake` — boustrophedon: alternating rows are visited in opposite
  directions. On a rectangular grid consecutive traversed cells are
  edge-adjacent, avoiding `row_major`'s spatial jump at row boundaries. This
  is a **locality property**, not a guarantee that every active cluster
  becomes one uninterrupted musical motif.

Worked example, the kernel of §1.6:

```text
row_major:  X . X   X X .   . X X   ->  onsets at slots 0 2 3 4 7 8
snake:      X . X   . X X   . X X   ->  onsets at slots 0 2 4 5 7 8
```

Same kernel, two rhythms — which is why the traversal is a mandatory,
explicit parameter. Both traversals carry golden coordinate and activity
vectors.

### 1.10 `linearize` and the time-slot contract

```text
linearize : Pattern2D -> ActivitySequence
```

`linearize` **preserves every cell**. In the subsequent time mapping each
cell occupies exactly one slot: `X` becomes a sounding onset, `.` becomes a
**timed rest**. Inactive cells are never silently removed — the locality
argument for `snake` survives only as long as `.` keeps its slot.

Consequences, all normative:

- `thin : Pattern2D -> Pattern2D` may only flip `X -> .`. It preserves
  dimensions, cell count, coordinates, and post-`linearize` sequence length.
- Compaction (removing cells, shortening the sequence) is a **separate,
  deferred operator with a distinct output type** — returning a plain
  `ActivitySequence` from it would be a type-system lie.
- Two adjacent `X` are two short notes, never one merged longer note.
  Merging is an articulation decision and belongs to deferred operators
  (`merge_adjacent` / `tie_adjacent` / `sustain_runs`), not to `map_rhythm`.

### 1.11 `map_rhythm` and the S6 seam

```text
map_rhythm(unit = 1/16, ...) : ActivitySequence -> Vec<RhythmTemplate>
```

- The time unit is **mandatory** — a sequence has no temporal meaning until
  it is declared. (No default unit exists anywhere, including frontends.)
- The sequence is cut into one-bar `RhythmTemplate` values. `X` at slot `i`
  becomes `TemplateNote { offset: i × unit, duration: unit }` within its
  bar; `.` contributes no note — the existing seam already represents rests
  as gaps between offsets, so the bar keeps its length.
- An incomplete final bar is governed by an explicit **tail policy**:
  `reject` (default — a typed error) or `rest_pad` (pad the tail with timed
  rests). `truncate` and any stretch/fit are deferred.
- **Cycle semantics.** The S6 scheduler rotates the produced palette
  round-robin across bars (`bar_index mod templates`). A request for more
  bars than the palette holds repeats the cycle; structure longer than the
  palette is **not expressible** in v0.1, and nothing stretches or truncates
  the pattern to fit a bar count.

### 1.12 Rhythm override in the shared compiler

S6 generation strategies and `RhythmTemplate` semantics remain unchanged.
The shared generation-input compiler (`ranked_candidates` — the single entry
point every frontend generates through) gains an explicit rhythm source:

```text
explicit pattern rhythms  >  corpus rhythms  >  source first-bar rhythm
```

Corpus novelty references and gesture remain corpus-based when a corpus is
supplied alongside a pattern. Pattern rhythms are never disguised as corpus
material.

### 1.13 Independent seeds

The generation seed (candidate/pitch variation) and the rhythm seed
(structural pruning) are **independent axes**:

- changing the generation seed leaves the expansion artifact byte-identical;
- changing the rhythm seed leaves pitch material and the candidate seed
  sequence untouched.

Wherever `density_decay` is in effect, a rhythm seed is **required** — there
is no implicit default seed.

### 1.14 Expansion artifact

Every expansion can be emitted as a normalized artifact: versioned schema
(`griff.pattern-expansion`, `version: 1`), canonical field order,
byte-stable serialization, complete enough to reproduce the expansion
(kernel, depth, decay bps, rhythm seed, traversal, unit, tail policy,
budgets, activity, per-bar templates), and produced **before** pitch
generation so that structural deltas are visible in isolation. Template
fingerprints are taken from the public `rhythm_diagnostics` (FNV-1a over
`(offset, duration)` pairs) — the hashing is not reimplemented, and the
fingerprint is a within-context check, **not** a durable content-addressed
template identity (it covers neither PPQN, nor bar length, nor schema
version).

## 2. Experimental transport syntax (Phase 2)

A temporary CLI contract that connects the pattern core to the shared
generation compiler **before any Swang grammar exists**. It is not an early
grammar; Phase 3 replaces it.

### 2.1 Kernel literal

`--rhythm-kernel 'X.X/XX./.XX'`

- `/` separates rows; only `X` and `.` are cell characters;
- empty rows and ragged rows are typed errors;
- whitespace inside the literal is a typed error (no silent normalization);
- dimensions and the cell budget are validated before allocation.

### 2.2 Flags

All pattern flags are namespaced under `--rhythm-*` and require
`--rhythm-kernel`:

```text
griff generate seed.gp5 out.mid \
  --bars 8 \
  --seed 42 \
  --rhythm-kernel 'X.X/XX./.XX' \
  --rhythm-fractal-depth 2 \
  --rhythm-density-decay 0.8 \
  --rhythm-seed 17 \
  --rhythm-traversal snake \
  --rhythm-unit 1/16 \
  --rhythm-max-cells 4096 \
  --rhythm-tail rest-pad \
  --emit-rhythm-expansion expansion.json
```

- `--rhythm-unit` is required (per §1.11);
- `--rhythm-density-decay` requires `--rhythm-seed` (per §1.13);
- `--rhythm-tail` defaults to `reject`;
- `--emit-rhythm-expansion` takes a **path** (the artifact never mixes into
  stdout's human-facing summaries);
- `--bars` rotates the palette (per §1.11); it never stretches the pattern.

### 2.3 Acceptance tests

1. Without pattern flags, generation is byte-identical to baseline.
2. `row_major` and `snake` match their golden coordinate/activity vectors.
3. A ragged kernel is rejected before expansion.
4. `.` creates a gap in offsets; it does not disappear.
5. Two adjacent `X` create two short notes.
6. `thin` preserves cell count and sequence duration.
7. `rest-pad` pads only the tail of the final bar.
8. `reject` refuses an incomplete final bar.
9. Pattern rhythm overrides corpus rhythm but not corpus novelty/gesture.
10. `--seed` does not change the expansion artifact.
11. `--rhythm-seed` does not change pitch material or the candidate seed
    sequence.
12. The expansion artifact is byte-stable across runs.
13. Changing only `--rhythm-fractal-depth` produces an explainable artifact
    delta.
14. The cell-budget error fires before exponential allocation.
15. `--bars` neither scales nor truncates the expansion.
16. Artifact fingerprints equal `rhythm_diagnostics` fingerprints for the
    same templates.

## 3. Deferred research

Named without reserved syntax or promised semantics (see the S16 stage doc
for admission bars):

- hierarchical / fractal structure **inference** (lifting);
- generalized-radix Z-order, Peano/Hilbert-family, and tree traversals;
- the polyphonic lowering (`Pattern2D -> PolyphonicPattern`);
- compaction and articulation-merge operators;
- accent/velocity in the pattern seam;
- text-to-structure encoders (Unicode scalar values only; pinned Unicode
  semantics);
- static growth-class prediction via the expansion matrix's spectral radius;
- versioned default budget/limit profiles as language objects.
