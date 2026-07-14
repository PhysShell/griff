# Swang — language specification

Decision: [ADR-0029](../adr/0029-swang-authoring-and-verified-lifting.md).
Delivery plan: [S16](../stages/S16-swang-language-and-verified-lifting.md).

This document is normative. It is split by stability, and the split is the
point: the **semantic core** below freezes when S16 Phase 0 is accepted —
from that point, changing anything in it requires a new language level —
while the surface grammar stays explicitly unstable until S16 Phase 3
closes, and the transport syntax is a temporary experiment contract, not
early grammar.

Status of each section:

| Section | Stability |
|---|---|
| 1. Proposed semantic core | **Proposed; freezes at Phase 0 acceptance.** After that, changes require a new language level. |
| 2. Experimental transport syntax | Temporary Phase-2 contract; may be replaced by the grammar. |
| 3. Deferred research | No promised names, no promised semantics. |

## 1. Proposed semantic core

### 1.1 Language level and the header line

Every Swang script begins with a header line pinning a **monotonic integer
language level**. Nothing executes without it.

The header syntax is the one piece of surface syntax that freezes forever
(at Phase 0 acceptance), byte-exactly:

```text
header = "swang" SP level EOL
SP     = exactly one U+0020 space
level  = a nonzero decimal digit followed by up to eight decimal digits
         (no sign, no leading zeros, no separators)
EOL    = LF, optionally preceded by exactly one CR
```

- The file is UTF-8. A byte-order mark is a typed error (`SWG0003`), never
  silently skipped.
- The header is the very first line: no leading blank lines, no leading
  whitespace, no comments before it; `swang` is lowercase.
- The pre-parser reads at most 64 bytes of the first line; a longer first
  line, a missing header, or a malformed header is `SWG0002`.
- Only the first line is the header. Later lines beginning with `swang` are
  ordinary content for the grammar to judge.
- The pre-parser implementing exactly these rules never changes across
  releases, so any interpreter can reject a newer file with an intelligible
  error (`SWG0001`, reporting its supported range) even when the rest of the
  grammar is unparseable to it.
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

Every diagnostic carries a stable typed code, a source location (a
`NodePath` before the grammar exists; a source span after), and a message.
Codes are never reused; the registry is append-only. The compiler core emits
diagnostics as pure data; rendering happens only at the frontend edge.

Initial registry:

| Code | Meaning |
|---|---|
| `SWG0001` | unsupported language level (newer than this build supports) |
| `SWG0002` | missing or malformed header line |
| `SWG0003` | byte-order mark before the header |
| `SWG0101` | ragged kernel: rows of unequal length |
| `SWG0102` | invalid kernel character (only `X` and `.` are cells) |
| `SWG0103` | whitespace inside a kernel literal |
| `SWG0201` | expansion exceeds `max_cells` (carries the offending `NodePath`) |
| `SWG0202` | expansion exceeds `max_depth` |
| `SWG0301` | rhythm unit does not divide the bar exactly |
| `SWG0302` | incomplete final bar under tail policy `reject` |
| `SWG0303` | density decay given without a rhythm seed |
| `SWG0304` | meter change inside the mapped span (v0.1 requires a constant meter) |
| `SWG0305` | the mapped span's bar duration is zero or its meter is unrepresentable |
| `SWG0306` | the expansion produced no onsets — nothing to generate (a fully silent kernel or a pruned-to-silence expansion is a deliberate typed error, not an empty candidate set) |
| `SWG0307` | empty kernel literal (no rows, or a row with no cells) |

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
  allocation: `SWG0101: ragged kernel: row 1 has 2 cells, expected 3`.
- Only `X` and `.` are cell characters.
- The drum-machine interpretation (columns = time, rows = simultaneous
  voices) is **not a traversal**; it is a separate future lowering with a
  distinct output type (`Pattern2D -> PolyphonicPattern`) and is never the
  default reading of a kernel.

### 1.7 `fractalize`

`fractalize(depth, density_bps, budget)` expands each active cell into a
scaled copy of the kernel and each inactive cell into an equally-sized empty
subtree. It chooses no pitches, strings, techniques, dynamics, or fingerings.

- An expansion node is addressed by its `NodePath` — the sequence of child
  indices from the root.
- A pruned (removed) parent yields an **entirely empty subtree**: no
  descendant of a removed node is ever active.
- `depth = 0` is the kernel itself.

### 1.8 Deterministic pruning — `swang-prune-hash-v1`

`density_bps` prunes by a **path-addressed hash test**, not a stream of
random draws: whether a cell survives is a pure function of the pruning seed
and the cell's path, independent of evaluation order and of every other
cell.

**Serialization and hash are two layers, honestly separated.** A node's
canonical *path* is the sequence of its child indices from the root, each a
`u32` in **structural order** — `row × kernel_width + column` within the
parent's kernel copy — fixed by the kernel's own geometry and independent of
any traversal. That serialization is injective. The 64-bit key fold over it
is a *hash*, not an encoding: distinct paths may collide, and the only
consequence of a collision is that two cells share one keep/prune decision —
a correlated coin, never a crash, never an out-of-budget expansion.

**The algorithm.** `mix64` is the splitmix64 finalizer (Stafford Mix13;
public domain — Steele–Lea–Flood OOPSLA '14; Vigna's reference C):

```text
mix64(z):
    z = (z XOR (z >> 30)) × 0xbf58476d1ce4e5b9    (wrapping, mod 2^64)
    z = (z XOR (z >> 27)) × 0x94d049bb133111eb    (wrapping, mod 2^64)
    return z XOR (z >> 31)

DOMAIN = u64::from_le_bytes(*b"swangpr1") = 0x3172_7067_6e61_7773
GAMMA  = 0x9e37_79b9_7f4a_7c15

key(root)     = mix64(DOMAIN XOR rhythm_seed)
key(node · c) = mix64(key(node) XOR (u64(c) + GAMMA))    (wrapping add)
hash(path)    = the key at the end of the fold
```

All arithmetic is wrapping `u64`; child indices are folded one at a time,
never concatenated as digits, so `[1, 23]` and `[12, 3]` cannot merge. No
cryptographic properties are promised or required.

**The test.** Pruning applies to expansion levels `1..=depth`; the kernel's
own cells (level 0) are given, not tested. Every node is tested once,
against a **constant per-node threshold**:

```text
threshold = floor(decay_bps × 2^64 / 10000)      (exact, computed in u128)
keep(node) = hash(path(node)) < threshold
```

Cumulative decay is emergent, not encoded. The normative statement is
exact: **a level-`d` cell is active iff the hash test passes for the cell
itself and for every tested ancestor** (a pruned parent already yields an
entirely empty subtree, §1.7). Under the *nominal model* — treating the
hash values as independent uniform draws, which the deterministic fold
approximates but does not prove — expected survival is
`(decay_bps / 10000)^d`; that is a design intuition for choosing `decay`,
not a guarantee of the algorithm.

Edge laws: `decay_bps = 10000` keeps every cell unconditionally (the test is
skipped — `2^64` is not representable as a `u64` threshold);
`decay_bps = 0` keeps nothing below level 0. Decay is carried as
`DensityBps(u16)`, range `0..=10000`; floats never appear anywhere in the
computation or its transport.

**Golden vectors** — CI-checked `(seed, path) -> u64` and
`(seed, kernel, decay_bps, depth) -> activity` fixtures — *illustrate* the
algorithm; the algorithm above, not the vectors, is normative. Changing the
mixer, the fold, the serialization order, or the threshold arithmetic is a
new algorithm version and a new language level.

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
  Only this type contract is fixed: the cell-selection rule is deliberately
  unspecified, and `thin` ships in no phase until that rule earns its own
  spec section.
- Compaction (removing cells, shortening the sequence) is a **separate,
  deferred operator with a distinct output type** — returning a plain
  `ActivitySequence` from it would be a type-system lie.
- Two adjacent `X` are two short notes, never one merged longer note.
  Merging is an articulation decision and belongs to deferred operators
  (`merge_adjacent` / `tie_adjacent` / `sustain_runs`), not to `map_rhythm`.

### 1.11 `map_rhythm` and the S6 seam

```text
map_rhythm(unit = 1/16) : ActivitySequence -> Vec<RhythmTemplate>
```

- The time unit is **mandatory** — a sequence has no temporal meaning until
  it is declared. (No default unit exists anywhere, including frontends.)
- **Bar geometry comes from the canonical master timeline** of the score the
  generation pass was seeded with: the score's PPQN and the time signature
  in effect at the start of the mapped span define `bar_duration` in ticks.
  v0.1 requires a **constant meter** across the mapped span; a meter change
  inside it is a typed error (`SWG0304`).
- `slots_per_bar = bar_duration / unit_ticks` must divide **exactly**;
  a unit that does not divide the bar is a typed error (`SWG0301`). A unit
  that is not representable in whole ticks at the score's PPQN is the same
  incompatibility and carries the same code, with a message naming the PPQN.
  A slot therefore never crosses a bar boundary, and the one-bar template cut
  is unambiguous.
- The sequence is cut into one-bar `RhythmTemplate` values. `X` at slot `i`
  becomes `TemplateNote { offset: (i mod slots_per_bar) × unit_ticks,
  duration: unit_ticks }` in bar `i div slots_per_bar`; `.` contributes no
  note — the existing seam already represents rests as gaps between offsets,
  so the bar keeps its length.
- An incomplete final bar is governed by an explicit **tail policy**:
  `reject` (default — a typed error, `SWG0302`) or `rest_pad` (pad the tail
  with timed rests). `truncate` and any stretch/fit are deferred.
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

Wherever `density_bps` is in effect, a rhythm seed is **required** — there
is no implicit default seed.

### 1.14 Expansion artifact

Every expansion can be emitted as a normalized artifact: versioned schema
(`griff.pattern-expansion`, `version: 1`), canonical field order,
byte-stable serialization, complete enough to reproduce the expansion
(kernel, depth, decay bps, rhythm seed, traversal, unit, tail policy,
budgets, **bar geometry — PPQN, meter, `bar_duration` in ticks,
`slots_per_bar`** — activity, per-bar templates), and produced **before**
pitch generation so that structural deltas are visible in isolation. Template
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
  --rhythm-density-bps 8000 \
  --rhythm-seed 17 \
  --rhythm-traversal snake \
  --rhythm-unit 1/16 \
  --rhythm-max-cells 4096 \
  --rhythm-tail rest-pad \
  --emit-rhythm-expansion expansion.json
```

- `--rhythm-unit` and `--rhythm-traversal` are required (per §1.9/§1.11);
- `--rhythm-fractal-depth` is required alongside the kernel — the requested
  depth is exact, so it doubles as the structural `max_depth`;
- `--rhythm-density-bps` takes an **integer** `0..=10000` (basis points, per
  §1.8 — no decimal transport, no float-to-bps conversion exists) and
  requires `--rhythm-seed` (per §1.13);
- `--rhythm-max-cells` is optional with the CLI's documented default of
  4096 — a *frontend* default (the pattern library still has none, per
  §1.4);
- the time-domain limits need no Phase-2 flags because the lowering
  satisfies them by construction: the sequence is monophonic
  (`max_polyphony = 1`), every slot lasts exactly one unit
  (`min_duration = unit`), and `max_events ≤ max_cells`. They become flags
  only when an operator can violate them;
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
6. The expansion artifact records the bar geometry (PPQN, meter,
   `bar_duration`, `slots_per_bar`).
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
