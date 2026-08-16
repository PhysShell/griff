# Swang foundation backlog

**Status: execution plan, non-normative.** This document schedules work; it
decides nothing. The normative sources stay where they are:

- semantics and grammar — [`spec.md`](spec.md) (§1 and §3 are **frozen**);
- scope, phases, and acceptance —
  [`../stages/S16-swang-language-and-verified-lifting.md`](../stages/S16-swang-language-and-verified-lifting.md);
- the decision itself —
  [`../adr/0029-swang-authoring-and-verified-lifting.md`](../adr/0029-swang-authoring-and-verified-lifting.md).

Where this document and any of those three disagree, they win and this file
is wrong. New decisions that surface while executing a task go to an ADR or
[`../decisions.log.md`](../decisions.log.md) — never into a backlog entry.

## How to read a task

Every entry carries: **Depends on**, **Kind** (docs / code / CLI), and
**Acceptance**. Acceptance is written so that it can fail. A criterion that
cannot fail is not a criterion; it is decoration.

Code tasks obey the mandatory TDD split from
[`../../AGENTS.md`](../../AGENTS.md): failing tests commit first, minimal
implementation commits second, per commit and not per PR. Characterization
tests for existing behaviour are exempt from the red phase but must pass
before commit. Where a task is a pure refactor, "characterization first"
means the existing suite is the characterization and must not be edited to
make the refactor pass.

## Verified baseline (2026-08-16)

Checked against the tree, not against memory:

| Fact | Evidence |
| --- | --- |
| Semantic core and surface grammar frozen | `spec.md` §1, §3 headings |
| `check` / `fmt` / `expand` / `build` exist | `cli/src/main.rs` `SwangCommand` |
| Hand-written lexer + recursive-descent parser + formatter | `swang/src/syntax/` (one 2122-line file until SWG-INF-03 split it) |
| Parser fuzz target on the blocking gate | `fuzz/fuzz_targets/swang_parse.rs` |
| Phase 4-pre A landed — rational `Tempo`, `ConfidenceBps` | `core/src/event.rs`, `core/tests/exact_scalars.rs` (PR #140) |
| Phase 4-pre B landed — `ExactSemanticDiff` | `core/src/semantic_diff.rs::exact_semantic_diff` (PR #142) |
| Phase 4-pre B landed — `NormalizedMusicalDiff` v1 | `core/src/semantic_diff.rs::normalized_musical_diff` (PR #146) |
| Swang Playground Slices 1–3 landed | `cockpit/src/swang.rs`; S8 stage doc |

So the next real boundary is **Phase 4A**, not Phase 3 and not 4-pre.
Nothing below asks for a language, a parser, or an editor to be created a
second time.

Two limits of the current parser are load-bearing for Epic A and are worth
naming precisely rather than approximately:

- `ProgramSpans` (`swang/src/syntax/parser/v1.rs`) carries exactly four
  spans — `kernel`, `unit`, `tail`, `source`. Every other AST field is
  unlocatable.
- `parse_with_spans` returns `Vec<Diagnostic>` but every failure path is
  `Err(vec![one])`. The vector is a shape, not a capability.

## Corrections to the source analysis

This backlog derives from an external review of the repository. Four of its
points do not survive contact with the tree and are corrected here so the
correction is durable:

1. **`stutter` is not first-wave.**
   [`../proposals/pattern-operator-inventory.md`](../proposals/pattern-operator-inventory.md)
   classifies it **defer (timed layer)** — it multiplies note counts in the
   timed domain, which `ActivitySequence` does not own. The first wave is
   `rotate`, `reverse`, `euclidean_mask`, `mask`.
2. **`ping_pong` is not sugar.** The inventory adopts it **standalone** and
   explicitly rejects deriving it from `reverse` + `concat`, because
   `concat` does not exist: "sugar over a nonexistent sugar factory is not
   a definition."
3. **Phase 4D was missing.** The MIDI playback projection is a real
   sub-phase of S16 with a deliberately weaker contract. It is scheduled
   below as SWG-4D-01/02 rather than dropped.
4. **Parser resource limits are not free.** Rejecting an input that level 1
   previously accepted is an observable change to a frozen level. Settled by
   SWG-INF-02: spec §5.11 puts every declared bound at level 2 only.

Confirmed as stated: `repeat` is sugar over ring (**adapt**), `bounded_walk`
stays with the S6 generator (**defer**), `thin` stays unspecified until a
selection rule earns a spec section, and exactly one rotation semantics may
exist across scheduler, traversal, and language.

## Stack verdict

**Keep** the hand-written lexer → recursive-descent parser → typed AST →
evaluator chain. ADR-0029 §11 already names it the initial strategy, and
nothing measured since argues against it.

**No `logos`.** The lexer distinguishes six token categories, ASCII
whitespace, and strings without escapes. A derive-macro generator would
trade ~150 readable lines for a dependency and an unmeasured benefit.

**No `rowan` yet.** It is admitted only through SWG-UI-07's gate, which
requires three demonstrated needs from the lossless-CST list. Until then a
full `SourceMap` (SWG-INF-04) plus a public token stream (SWG-UI-03) cover
the editor's real requirements.

Both refusals are revisitable by evidence, not by preference. Both would be
recorded in `decisions.log.md` if reversed.

## Task index

| ID | Title | Kind | Depends on |
| --- | --- | --- | --- |
| SWG-INF-01 | Sync the S16 status block with reality *(done)* | docs | — |
| SWG-INF-02 | Language level 2 admission contract *(done)* | docs | INF-03 |
| SWG-INF-03 | Split `syntax.rs` without behaviour change *(done)* | code | INF-01 |
| SWG-INF-04 | Replace `ProgramSpans` with a `SourceMap` | code | INF-03 |
| SWG-INF-05 | Deterministic multi-error recovery | code | INF-04 |
| SWG-INF-06 | Parser resource gate and differential harness | code | INF-02, INF-03 |
| SWG-4A-01 | Normative exact-score-text grammar | docs | INF-02 |
| SWG-4A-02 | `ExactScoreDocument` as a transient syntax form | code | 4A-01 |
| SWG-4A-03 | Writer: transport and master timeline | code | 4A-01 |
| SWG-4A-04 | Writer: tracks, voices, groups, atoms | code | 4A-03 |
| SWG-4A-05 | Writer: techniques, positions, evidence, losses | code | 4A-04 |
| SWG-4A-06 | Parser skeleton and level/root dispatch | code | 4A-01, INF-04 |
| SWG-4A-07 | Parser: exact scalar types | code | 4A-06 |
| SWG-4A-08 | Parser: structural tree | code | 4A-07, 4A-02 |
| SWG-4A-09 | Checked `ScoreBuilder` | code | 4A-08 |
| SWG-4A-10 | `griff swang dump` | CLI | 4A-05 |
| SWG-4A-11 | `griff swang verify` | CLI | 4A-09, 4A-10 |
| SWG-4A-12 | The three round-trip laws + mutation matrix | code | 4A-09, 4A-05 |
| SWG-4A-13 | Expressibility negative matrix | code | 4A-12 |
| SWG-4A-14 | Fuzz targets for exact text | code | 4A-09, INF-06 |
| SWG-4B-01 | Closed corpus result classification | code | 4A-11 |
| SWG-4B-02 | Stable report schema | code | 4B-01 |
| SWG-4B-03 | Feature and loss breakdown | code | 4B-02 |
| SWG-4B-04 | Baseline and regression policy | code | 4B-03 |
| SWG-4B-05 | Split blocking and full-corpus gates | CI | 4B-04 |
| SWG-4C-01 | Corpus study of event identity | docs | 4B-03 |
| SWG-4C-02 | Typed selector model | code | 4C-01 |
| SWG-4C-03 | Closed selector-resolution result | code | 4C-02 |
| SWG-4C-04 | Transactional patch engine | code | 4C-03 |
| SWG-4C-05 | Patch syntax and formatter | code | 4C-04 |
| SWG-4C-06 | Semantic evidence for a patch result | code | 4C-04 |
| SWG-4C-07 | Patch fuzzing | code | 4C-05 |
| SWG-4D-01 | MIDI playback-projection contract | docs | 4A-12 |
| SWG-4D-02 | MIDI projection harness | code | 4D-01 |
| SWG-AUTH-01 | Typed intermediate representation | code | 4C-05 |
| SWG-AUTH-02 | Names, definitions, resolver | code | AUTH-01 |
| SWG-AUTH-03 | Typed pipeline checker | code | AUTH-02 |
| SWG-AUTH-04 | Budget hierarchy | code | AUTH-01 |
| SWG-AUTH-05 | Named seed streams | code | AUTH-01 |
| SWG-AUTH-06 | First operator wave | code | AUTH-03, AUTH-04, AUTH-05 |
| SWG-AUTH-07 | Second operator wave | code | AUTH-06 |
| SWG-UI-01 | Debounced live check | code | INF-05 |
| SWG-UI-02 | Underlines and diagnostic navigation | code | UI-01, INF-04 |
| SWG-UI-03 | Highlighting from the shared token stream | code | INF-03 |
| SWG-UI-04 | AST parameter inspector | code | INF-04 |
| SWG-UI-05 | Expansion delta viewer | code | UI-04 |
| SWG-UI-06 | Provenance snapshot v2 | code | UI-05 |
| SWG-UI-07 | Rowan admission gate | docs | UI-02, UI-03, UI-04 |
| SWG-LIFT-01 | Recognizer contract | code | 4C-06 |
| SWG-LIFT-02 | Baseline recognizers | code | LIFT-01 |
| SWG-LIFT-03 | Exact residual generation | code | LIFT-02 |
| SWG-LIFT-04 | Deterministic DP optimizer | code | LIFT-03 |
| SWG-LIFT-05 | Lift CLI and report | CLI | LIFT-04 |

---

## Epic A — parser platform (`SWG-INF-*`)

### SWG-INF-01 — Sync the S16 status block with reality

**Kind:** docs. **Depends on:** —

Documentation currently says Phases 4–9 are future work. Two of them
shipped. Left alone, the next agent proposes implementing `ExactSemanticDiff`
again.

Work:

- update the S16 header `Status:` line (it still reads `proposed` while
  Phases 0–3 are shipped and frozen);
- mark Phase 4-pre A and 4-pre B **done**, with PR evidence (#140, #142,
  #146);
- restate the next boundary as Phase 4A;
- correct the "next scope is S8 Swang Playground" sentence — Playground
  Slices 1–3 landed;
- add the 4A → 4B → 4C → 4D → 5/6 dependency map;
- link this backlog from the stage doc;
- change no frozen spec text.

Acceptance:

- no shipped work is described as future, and no future phase is described
  as landed;
- every "done" claim names a PR or a file;
- `spec.md` is byte-unchanged;
- stage state is derivable from the header plus the phase blocks together,
  per the AGENTS.md routing rule.

### SWG-INF-02 — Language level 2 admission contract *(done)*

**Kind:** docs. **Depends on:** INF-03

Landed as **[`spec.md`](spec.md) §5** (a new section — §4 was already
Deferred research, and nothing was renumbered) plus a decisions-log
Y-statement. No ADR: the decision stays inside Swang, changes no
`griff-core` contract, and ADR-0029 already delegates normative semantics to
the spec. `spec.md` §1 and §3 are byte-unchanged, verified by digest.

What §5 settles, for downstream tasks that need the answer:

- **Law A adopted, strengthened.** A build supporting `1..=N` treats every
  `swang 1` source — *including invalid ones* — exactly as a level-1-only
  build did, compared on all seven observables: verdict, AST, canonical
  bytes, diagnostic code, message, span, and order. A level-2 keyword in a
  `swang 1` script raises the `SWG0401` level 1 already raised, never a
  friendlier "requires language level 2".
- **Law B rejected.** A `swang 2` header over a level-1 body is not valid by
  default. Promotion, if ever wanted, is a migration tool with its own laws,
  not a guarantee attached to editing one digit.
- **Level 2 is allocated to Phase 4A's exact canonical `Score` text and
  nothing else.** One new root form, `score`. Recipes, multiple named
  definitions, pattern operators, patches, tonal constructs, and convenience
  defaults are all excluded; the level-1 `pattern` root is **not** admitted.
- **Allocated, not frozen.** Level 2 freezes when Phase 4A is accepted. Its
  body grammar is SWG-4A-01's job — §5 reserves the number and fences the
  scope so that task designs a score text rather than half a language.
- **Dispatch is per level.** Each released level owns its parser and
  formatter entry point; there is no single grammar with level-conditioned
  branches, because such a grammar cannot prove level 1 survived.
- **Input bounds belong to level 2 only**, declared before its first
  accepted program. Level 1's acceptance set is frozen. This is the space
  SWG-INF-06 works in.
- **Two digests, not one.** `H(format(program))` is a source digest of
  canonical text and includes the header; the level-free semantic hash §1.1
  requires must be taken over the normalized semantic form.

Still open, deliberately: every level-2 diagnostic code (defined by the
section that earns it — no block is reserved in advance), and the level-2
body grammar itself.

### SWG-INF-03 — Split `syntax.rs` without behaviour change *(done)*

**Kind:** code (mechanical refactor). **Depends on:** INF-01

`swang/src/syntax.rs` held the header pre-parser, AST, lexer, parser,
formatter, spans, and tests in one 2122-line file. Landed layout — the
repository denies `clippy::mod_module_files`, so it is the 2018 style
(`syntax.rs` beside `syntax/`), not the `mod.rs` tree this entry first
sketched:

```text
swang/src/syntax.rs        re-exports; the public API is unchanged
swang/src/syntax/
├── header.rs              frozen pre-parser, LANGUAGE_LEVEL
├── diagnostic.rs          Diagnostic
├── span.rs                Span, span_of
├── token.rs               Token, TokenKind
├── lexer.rs
├── tests.rs
├── ast.rs      + ast/v1.rs
├── parser.rs   + parser/v1.rs
└── format.rs   + format/v1.rs
```

Hard condition: mechanical only. No message improvements, no span
adjustments, no "while I'm here".

Acceptance — all met:

- every existing test passes with no edit to an expected value (79 swang,
  824 core, 22 pattern, 194 ui-core, 80 cli — identical counts);
- the spec §3.1 reference program formats byte-identically (sha256
  `b10f82e6…`);
- every `SWG____` code, message, and span is unchanged — verified by a
  differential harness over 168 mutations of the reference program covering
  12 codes, byte-identical output and exit status (sha256 `c3ea6183…`);
- `parse`, `parse_with_spans`, `format`, `header_level` keep their paths and
  signatures; the public item set is identical (70 items);
- the fuzz corpus is unchanged and `swang_parse` still builds;
- `cargo clippy --all-targets -- -D warnings` is clean, and the `cargo doc`
  warning count is unchanged.

Visibility moved only where the split forced it: `span_of`,
`HEADER_WINDOW`, `lex`, `Token` (with its fields), and `TokenKind` became
`pub(crate)`. No item gained `pub`; the level modules are
`pub(crate) mod v1` behind private parents.

**Deliberately not done here**, so that INF-02 stays a real decision rather
than a description of something already coded: no `v2` module, no
`Level2Root`, no `GrammarVersion`, no dispatch stub, no `Parsed<T>`, no
`SourceMap`, no recovery, no limits, no public token API.

### SWG-INF-04 — Replace `ProgramSpans` with a `SourceMap`

**Kind:** code. **Depends on:** INF-03

Four spans cannot locate a diagnostic in an exact score text, cannot anchor
a patch selector, and cannot drive an editor action. Shape:

```text
struct SourceMap {
    nodes:  BTreeMap<AstId, Span>,
    fields: BTreeMap<FieldRef, Span>,
}

enum AstId    { Program(u32), Pattern(u32), PipelineStep(u32), Generate(u32) }
enum FieldRef { Node(AstId), Named { node: AstId, field: FieldKind } }
```

Requirements:

- spans stay out of AST equality — `parse(format(ast)) == ast` still holds;
- the formatter can consume an AST with no source map at all;
- the parser returns `Parsed<T> { value, source_map }`;
- a diagnostic points at the value the user must change, not at the
  statement containing it;
- every semantically significant level-1 field has a span;
- every span lies on a UTF-8 boundary inside the source.

Acceptance:

- the four existing span tests pass through the new model;
- a witness test enumerates every level-1 AST field and asserts a span for
  each — adding a field without a span fails to compile or fails the test;
- reordering words in the source moves the owning span with the value;
- the spec §3.5 formatter laws are unchanged.

### SWG-INF-05 — Deterministic multi-error recovery

**Kind:** code. **Depends on:** INF-04

Today every failure path is `Err(vec![one])`. The vector is a container that
usually holds a single element — a nesting doll with nothing inside.

Add recovery with explicit synchronization points:

- top level synchronizes on `pattern`, `score`, EOF;
- pipelines synchronize on `|>` and `}`;
- block fields synchronize on the next known word;
- a cap (32 is a reasonable first number) on emitted diagnostics;
- one mistake never produces five cascading messages at the same token;
- diagnostics sort by `(span.start, code)`;
- structural errors still produce no executable AST.

Acceptance:

- a fixture with three independent errors returns exactly three expected
  diagnostics;
- re-parsing the same bytes yields the same order;
- every span is in bounds;
- **the first diagnostic of every existing single-error golden is
  unchanged** — recovery adds diagnostics, it never renames the first one;
- accept/reject verdicts are unchanged for every existing fixture;
- adversarial input does not go quadratic (assert a bounded step count, not
  a wall-clock time).

### SWG-INF-06 — Parser resource gate and differential harness

**Kind:** code. **Depends on:** INF-02, INF-03

Add bounded-input laws: maximum source bytes, maximum token count, maximum
nesting depth, maximum diagnostics, no recursion overflow, and a canonical
formatter that only ever writes from a checked AST.

**Constraint from the freeze, settled by INF-02:** rejecting an input that
level 1 previously accepted is an observable semantic change to a frozen
level, so spec §5.11 puts every declared bound at **level 2 only**, to be
declared before level 2's first accepted program. Level 1 keeps its
acceptance set exactly; a level-1 run may still die of exhaustion, but that
is a runtime outcome and never becomes a typed refusal. This task therefore
adds no level-1 limit at all — the earlier "or prove the bound exceeds every
representable level-1 program" branch is closed.

Differential harness: the pre-refactor parser output is compared with the
refactored parser over the fixture set and the fuzz corpus on AST, canonical
text, diagnostic code, and owning span.

Fuzz oracles (extending `swang_parse`):

```text
parse never panics
Err carries at least one diagnostic
every code matches SWG\d{4}
every span lies within the source
format(parse(format(ast))) is a fixed point
a limit breach is a typed error, not an allocation death
```

---

## Epic B — Phase 4A: exact canonical score text (`SWG-4A-*`)

### SWG-4A-01 — Normative exact-score-text grammar

**Kind:** docs. **Depends on:** INF-02

Before any parser code, fix representability for: `Score`, `MasterBar`,
`Track`, `Voice`, `EventGroup`, note and rest atoms, technique spans, note
positions, source metadata, and loss facts.

Decide in writing: the root keyword (`score`); whether exact text is level 2
(it is); field order; required versus optional; the exact rational tempo
spelling; ticks and ranges; tuning; every `EventGroupKind` including payload
variants; marks; technique evidence; fretboard-position evidence;
repeat markers; import warnings; string quoting and escapes; the spelling of
an empty collection; and the unknown-field policy.

Exact text is a projection of the canonical model, not musical poetry. Do
not trade a fact for prettiness.

Acceptance:

- a table maps **every** canonical field to its syntax location;
- no field is left as "obvious";
- every enum variant is listed — no wildcard arms in the table;
- adding a field to the canonical model breaks an exhaustive witness test.

### SWG-4A-02 — `ExactScoreDocument` as a transient syntax form

**Kind:** code. **Depends on:** 4A-01

```text
text -> ExactScoreDocument -> checked ScoreBuilder -> canonical Score
```

`ExactScoreDocument` is a syntax representation with a short life. It holds
author order only until canonical formatting, is never consumed by a
generator, never becomes a persistent domain model, and is never imported
into `griff-core`. This is S16 required control #3 — no permanent score
hierarchy beside canonical `Score` — enforced by construction rather than by
good intentions.

Acceptance:

- `griff-core` does not depend on `griff-swang` (assert in the dependency
  test, not by reading `Cargo.toml` by eye);
- after lowering, the evaluator sees only `Score`;
- no generation entry point accepts an `ExactScoreDocument`;
- the document has no serialization format of its own beyond Swang source.

### SWG-4A-03 — Writer: transport and master timeline

**Kind:** code. **Depends on:** 4A-01

First writer slice only: language header, PPQN, master bars, tick ranges,
time signatures, exact rational tempo, repeat markers.

Laws:

```text
write(score) is deterministic
equal Scores produce equal bytes
no f64 anywhere in the writer
no locale-sensitive formatting
LF only, exactly one trailing newline
```

Characterization fixtures: constant tempo; several exact tempos; a tempo not
representable in MIDI without approximation; odd meters; repeats; an empty
timeline. A canonical score that the writer cannot represent is rejected
before writing, never written approximately.

### SWG-4A-04 — Writer: tracks, voices, groups, atoms

**Kind:** code. **Depends on:** 4A-03

Adds track name / channel / tuning, voices, group kind, note and rest atoms,
absolute start, duration, pitch, velocity, and the ordering rules.

Non-negotiable:

- the writer never quietly sorts where order is exact semantics;
- a note crossing a barline stays one note — the writer synthesizes no ties;
- rests survive;
- `Single`, `Chord`, `Tuplet`, and every other kind never collapse.

Acceptance is measured by `ExactSemanticDiff`, not by "the MIDI sounds
about right".

### SWG-4A-05 — Writer: techniques, positions, evidence, metadata, losses

**Kind:** code. **Depends on:** 4A-04

The remaining facts: note marks, technique spans and their ranges, evidence,
string/fret position, position evidence and confidence (`ConfidenceBps`),
source metadata, import warnings, loss facts.

Acceptance:

- mutating any one of these fields changes the exact text;
- `NormalizedMusicalDiff` may call some of those mutations equal;
  `ExactSemanticDiff` must see every one of them;
- source and loss facts do not disappear merely because they make no sound.

### SWG-4A-06 — Parser skeleton and level/root dispatch

**Kind:** code. **Depends on:** 4A-01, INF-04

```text
header_level -> level dispatch -> root dispatch (pattern | score)
```

The parser may initially accept only a minimal empty score, but the skeleton
ships complete: a typed root enum, the source map, deterministic
diagnostics, resource budgets, and formatter dispatch.

Acceptance:

- level 1 behaviour is bit-for-bit unchanged — Law A's seven observables
  (spec §5.5), asserted over valid *and* invalid `swang 1` sources;
- a level-2 `pattern` root is **rejected** (spec §5.7), with a test naming
  the diagnostic;
- a level-2 `score` reaches the exact parser;
- an unknown newer level is still refused by the frozen pre-parser;
- dispatch routes to one level's entry point and never branches inside a
  shared grammar (spec §5.4).

### SWG-4A-07 — Parser: exact scalar types

**Kind:** code. **Depends on:** 4A-06

One PR for the scalar layer: `u8`/`u16`/`u32`/`u64`, non-zero integers,
rational tempo, ticks, ranges, pitch, velocity, time signature,
`ConfidenceBps`, enums, quoted strings with escapes.

Each type states: its canonical spelling, its overflow diagnostic, its
leading-zero policy, its zero rule, and a span owning exactly the offending
literal — not the enclosing field.

Acceptance: `parse(format(x)) == x` as a property test per type, plus a
negative fixture per diagnostic.

### SWG-4A-08 — Parser: structural tree

**Kind:** code. **Depends on:** 4A-07, 4A-02

Layer by layer: master bars → tracks → voices → groups → atoms → technique
spans → metadata and losses.

No parser function mutates semantic global state. Parsing and executing are
separate passes; the parse-and-do-it-at-the-same-time design is a known
source of unreproducible bugs and is not available here.

### SWG-4A-09 — Checked `ScoreBuilder`

**Kind:** code. **Depends on:** 4A-08

The builder independently re-checks: voice id uniqueness; ordered,
non-overlapping master bars per the canonical contract; valid tick ranges;
positive durations; MIDI pitch bounds; tuning; position/string/fret
consistency; group invariants; technique-span ranges; timeline containment;
and every existing `Score` validation law.

Errors point at the Swang source field through the `SourceMap`.

Acceptance:

```text
syntactically valid text
  -> either a valid Score
  -> or source-located typed diagnostics
```

There is no third outcome, and no partially valid `Score` is ever returned.

### SWG-4A-10 — `griff swang dump`

**Kind:** CLI. **Depends on:** 4A-05

```text
griff swang dump input.gp5
griff swang dump input.mid
```

Contract: import runs through the existing adapters; the loss report stays
in the exact text; the score goes to stdout and only to stdout; diagnostics
and import warnings go to stderr; no hidden normalization; two runs produce
byte-identical output.

### SWG-4A-11 — `griff swang verify`

**Kind:** CLI. **Depends on:** 4A-09, 4A-10

```text
griff swang verify score.swg --against source.gp5
```

Reports parse/build status, the `ExactSemanticDiff`, the
`NormalizedMusicalDiff` on request, typed semantic paths, and import losses.
Exit code is non-zero on exact mismatch. A machine-readable mode may come
later; the human rendering is never the source of truth.

### SWG-4A-12 — The three round-trip laws and the mutation matrix

**Kind:** code. **Depends on:** 4A-09, 4A-05

Over synthetic and real fixtures:

```text
parse(format(score)) ~= score        via ExactSemanticDiff::is_empty()
format(parse(text))  == canonical_text
fmt(fmt(text))       == fmt(text)
```

Law 1 uses the diff rather than derived equality, because the document and
the score sit on either side of a representation boundary.

The mutation matrix applies exactly one controlled mutation per canonical
field and asserts the expected semantic path, the expected text delta, and
forward/backward diff symmetry.

### SWG-4A-13 — Expressibility negative matrix

**Kind:** code. **Depends on:** 4A-12

Turn the alphaTab issue #1484 expressibility list into a table with columns:
feature, canonical representation, Swang representation, positive fixture,
malformed negative fixture, round-trip result.

Minimum coverage: cross-bar notes, tuplets, multiple voices, rests, repeats,
alternate structures where the canonical model stores them, bends and
techniques, fretboard positions, evidence, tempo changes, unusual meters,
loss metadata.

### SWG-4A-14 — Fuzz targets for exact text

**Kind:** code. **Depends on:** 4A-09, INF-06

New targets: `swang_exact_parse`, `swang_exact_roundtrip`,
`swang_score_writer`, `swang_exact_builder`. Oracles: no panic and no OOM;
spans valid; an accepted document produces a valid `Score`; writer output
re-parses; the formatter is a fixed point; the exact diff is empty; a
resource breach is a typed failure rather than a memory event. Bounded smoke
runs join the blocking gate per ADR-0010.

---

## Epic C — Phase 4B: corpus acceptance harness (`SWG-4B-*`)

### SWG-4B-01 — Closed corpus result classification

**Kind:** code. **Depends on:** 4A-11

```text
enum CorpusRoundtripClass { Exact, Normalized, KnownImportLoss, Unsupported, Failed }
```

Exactly one class per corpus item. A parse failure is not "unsupported"; a
loss is not "normalized"; there is no `Other` to sweep the unpleasant cases
into.

### SWG-4B-02 — Stable report schema

**Kind:** code. **Depends on:** 4B-01

Fields: fixture identity and digest, adapter format, import losses, exact
diff count, normalized diff count, unsupported features, failure stage, and
the tool / schema / policy versions. Canonical JSON, stable ordering, a
report hash; the same run produces the same bytes.

### SWG-4B-03 — Feature and loss breakdown

**Kind:** code. **Depends on:** 4B-02

Aggregate separately by meter, tempo, repeats, voices, rests, group kinds,
marks, technique spans, positions, source metadata, and adapter losses.
"92% passed" is not a result if half the bend spans evaporated into an
unnamed bucket.

### SWG-4B-04 — Baseline and regression policy

**Kind:** code. **Depends on:** 4B-03

The baseline stores per-category counts, the report hash, known exceptions,
and the policy version. CI fails on: `Exact` → `Failed`, any increase in
`Unsupported`, any fact disappearing from the report, and a hash change
without an accepted baseline update. `Normalized` → `Exact` is an
improvement and is still recorded as a deliberate delta.

### SWG-4B-05 — Split blocking and full-corpus gates

**Kind:** CI. **Depends on:** 4B-04

Blocking: synthetic exhaustive fixtures, golden score fixtures, property
tests, bounded fuzz smoke. Nightly or manual first: the full corpus, the
stable report artifact, the baseline comparison. Promotion to blocking
happens after runtime and stability are measured, not on the assumption that
CI will cope.

---

## Epic D — Phase 4C: stable selectors and exact patches (`SWG-4C-*`)

### SWG-4C-01 — Corpus study of event identity

**Kind:** docs (measured). **Depends on:** 4B-03

Before any syntax, measure: how often a composite selector is unique; which
combinations suffice (track identity, voice id, bar/tick range, group kind,
onset, pitch, neighbouring fingerprints); where ambiguity concentrates; and
whether persistent IDs in `griff-core` are actually needed. S16 requires
this evidence before the persistent-ID decision. The deliverable is a
measured report, not a preference.

### SWG-4C-02 — Typed selector model

**Kind:** code. **Depends on:** 4C-01

```text
struct EventSelector {
    track:        TrackAnchor,
    voice:        VoiceAnchor,
    time:         TimeAnchor,
    subject:      SubjectAnchor,
    precondition: Option<SemanticFingerprint>,
}
```

A bare positional index is never durable identity.

### SWG-4C-03 — Closed selector-resolution result

**Kind:** code. **Depends on:** 4C-02

```text
enum SelectorResolution<T> { Unique(T), Missing, Ambiguous(Vec<CandidateEvidence>), Stale(PreconditionMismatch) }
```

No first-match fallback anywhere. An ambiguous selector is a typed failure,
not an invitation to pick element zero and rearrange someone's music with a
confident face.

### SWG-4C-04 — Transactional patch engine

**Kind:** code. **Depends on:** 4C-03

Operations: replace, delete, overlay, exact residual insertion. Order:

```text
resolve every selector
  -> validate every precondition
  -> construct a candidate Score
  -> validate the candidate
  -> commit
```

One invalid operation aborts the whole patch. A failed patch leaves the
source score untouched.

### SWG-4C-05 — Patch syntax and formatter

**Kind:** code. **Depends on:** 4C-04

Only after the selector contract exists: the exact syntax, canonical order,
source spans, the stale/missing/ambiguous diagnostics, and the parse/format
laws. No implicit selection at any point.

### SWG-4C-06 — Semantic evidence for a patch result

**Kind:** code. **Depends on:** 4C-04

The result carries patch identity, resolved selectors, before/after semantic
paths, the exact diff, the validation result, provenance, and — on
ambiguity — the rejected alternatives.

### SWG-4C-07 — Patch fuzzing

**Kind:** code. **Depends on:** 4C-05

No arbitrary patch panics; a failed patch never mutates the source; a
successful patch yields a valid score; re-applying with a stale precondition
refuses; ambiguous never becomes success; the formatter stays a fixed point.

---

## Epic D2 — Phase 4D: MIDI playback projection (`SWG-4D-*`)

### SWG-4D-01 — MIDI playback-projection contract

**Kind:** docs. **Depends on:** 4A-12

```text
Score -> MIDI -> Score        playback-equivalent, not exact-semantic
```

Write down what is compared — sounding pitches, onsets, durations,
transport, bends as far as the export carries them — and what is declared
lost: string/fret, grouping, evidence, richer technique semantics,
provenance. MIDI does **not** participate in the 4A exact round-trip gate,
and the contract must say so in a sentence that cannot be misread later.

### SWG-4D-02 — MIDI projection harness

**Kind:** code. **Depends on:** 4D-01

Fixtures and a report over the declared comparison set, reusing the 4B
report schema where it fits. A tempo that MIDI cannot represent exactly
raises the typed approximation loss from Phase 4-pre A — never a silent
rounding.

---

## Epic E — the authoring language (`SWG-AUTH-*`)

Scheduled after exact text and patches. Recipes that reference vaguely
defined objects turn the following quarter into archaeology.

**These constructs are not level 2.** Spec §5.7 allocates level 2 to the
exact canonical `Score` text and nothing else; the typed IR, named
definitions, seed streams, and every operator below land at **level 3 or
later**, each on its own evidence and each freezing with the phase that
delivers it. The `AUTH` prefix replaced an earlier `L2` prefix precisely
because the latter read as a level number it was never going to be.

### SWG-AUTH-01 — Typed intermediate representation

**Kind:** code. **Depends on:** 4C-05

Not one omniscient `enum Expr`. Separate semantic domains with explicit
operator signatures:

```text
Pattern2D  ActivitySequence  Cycle<T>  RhythmPalette
GenerationRecipe  ScoreRecipe  ExactPatch

reverse    : Cycle<T> -> Cycle<T>
rotate     : Cycle<T> × Offset -> Cycle<T>
mask       : Pattern2D × Pattern2D -> Pattern2D
linearize  : Pattern2D × Traversal -> ActivitySequence
map_rhythm : ActivitySequence × TimeMap -> RhythmPalette
generate   : RhythmPalette × GenerationAsk -> CandidateSet
```

The parser builds a surface AST; the type checker decides what may compose.

### SWG-AUTH-02 — Names, definitions, resolver

**Kind:** code. **Depends on:** AUTH-01

Support several declaration kinds (`pattern`, `rhythm`, `source`, `policy`,
later `part`) with lexical scope, duplicate-name and unknown-name
diagnostics, kind mismatch, cycle detection, a deterministic topological
order, and an unused-definition **warning** — a frontend diagnostic, not a
semantic error.

### SWG-AUTH-03 — Typed pipeline checker

**Kind:** code. **Depends on:** AUTH-02

```text
SWG12xx: `rotate` expects Cycle<T>, found Pattern2D
```

A typed operator registry, not a string switch. Type checking precedes
materialization. No coercions unless a spec section says so. Unit types do
not mix. Every lowering step leaves a provenance node.

### SWG-AUTH-04 — Budget hierarchy

**Kind:** code. **Depends on:** AUTH-01

`ExpansionBudget` exists today (spec §1.4). Add `CycleBudget`,
`EventBudget`, and `CandidateBudget`, each charged by its own stage. One
shared `max_items` across unrelated stages is how a limit stops limiting
anything.

### SWG-AUTH-05 — Named seed streams

**Kind:** code. **Depends on:** AUTH-01

```text
seed rhythm 17
seed pitch  42
seed urn    91
```

Axes are independent; the derivation algorithm is versioned; no operator
reads ambient RNG; adding a sibling operator does not shift an existing
sequence; provenance stores the stream id and the derived seed; reordering
independent operators does not change the result where the law allows it.
This extends spec §1.13's two-axis rule rather than replacing it.

### SWG-AUTH-06 — First operator wave

**Kind:** code. **Depends on:** AUTH-03, AUTH-04, AUTH-05

Per the operator inventory's classification: `rotate`, `reverse`,
`euclidean_mask`, `mask`. (`stutter` is **not** here — it is deferred to the
timed layer.) `mask` lands as the first specified `thin` selection rule and
must reconcile with the frozen §1.10 type contract.

Each operator gets its own issue/PR chain: spec section → type signature →
boundedness → diagnostic codes → golden vectors → algebraic laws → parser →
formatter → evaluator → expansion artifact and provenance → cockpit demo →
fuzz and property tests.

Laws to assert:

```text
rotate(a) ∘ rotate(b) = rotate(a + b mod n)
reverse ∘ reverse      = identity
mask never changes dimensions
euclidean_mask(0, n)   = all off
euclidean_mask(n, n)   = all on
```

### SWG-AUTH-07 — Second operator wave

**Kind:** code. **Depends on:** AUTH-06

`ring`/`cycle` (adapt — reconcile with §1.11 palette cycling first, since
`repeat` is sugar over it), `multi_cycle`, `creep`, envelope modulation,
`urn`, and `ping_pong` (standalone, not sugar). These need a cursor model:

```text
struct CursorState { lane: LaneId, cycle: u64, offset: u64 }
```

State must be explicit, serializable into the expansion artifact and
provenance, bounded, reproducible, and independent of wall-clock playback.

---

## Epic F — tooling and the Swang Playground (`SWG-UI-*`)

The Playground already does edit → check/format → run → candidates →
audition → history (S8 Slices 1–3). These tasks extend it; none of them
creates an editor.

### SWG-UI-01 — Debounced live check

**Kind:** code. **Depends on:** INF-05

Parsing runs after a short pause; generation stays behind an explicit Run; a
stale run is invalidated immediately; diagnostics refresh without filesystem
access; the parse budget never blocks the UI thread.

### SWG-UI-02 — Underlines and diagnostic navigation

**Kind:** code. **Depends on:** UI-01, INF-04

Highlight the exact source span; clicking a diagnostic moves the caret;
structural `NodePath` locations render separately from spans; overlapping
diagnostics do not stack into noise; code, message, and location are
available as text and not only as colour.

### SWG-UI-03 — Highlighting from the shared token stream

**Kind:** code. **Depends on:** INF-03

The UI does not get a second lexer. Publish a read-only tokenizer:

```rust
fn highlight_tokens(source: &str) -> Vec<HighlightToken>;
```

Categories: keyword, identifier, number/unit, string, operator,
punctuation, invalid.

### SWG-UI-04 — AST parameter inspector

**Kind:** code. **Depends on:** INF-04

The user edits depth, density, traversal, unit, and strategy; the UI mutates
the AST field and calls the canonical formatter. It never regex-replaces
source text. Regex editing of a language is technical debt wearing a
convenience costume.

### SWG-UI-05 — Expansion delta viewer

**Kind:** code. **Depends on:** UI-04

For two runs, show the canonical source diff, changed AST fields, changed
expansion cells, changed rhythm templates, changed candidate selection,
explicitly unchanged axes (e.g. the pitch seed), and byte-stable artifact
hashes. This is the core Swang promise made visible: move one axis, see an
explainable structural delta.

### SWG-UI-06 — Provenance snapshot v2

**Kind:** code. **Depends on:** UI-05

Per run, store: canonical Swang source, language level, source-score digest,
corpus digest/contribution, operator versions, budget profile, named seed
streams, expansion-artifact digest, selected candidate identity, and exact
export losses. Never a reference to mutable editor text — Slice 3 already
snapshots the evaluated text, and v2 keeps that property.

### SWG-UI-07 — Rowan admission gate

**Kind:** docs. **Depends on:** UI-02, UI-03, UI-04

`rowan` is admitted only if at least **three** of these are demonstrated
needs: comment-preserving formatter/refactor, incremental parsing, rename,
semantic selection over syntax nodes, robust completion, an external LSP.

Before admission: the current parser stays, the `SourceMap` supplies spans,
the token stream supplies highlighting, and the AST formatter supplies
structured edits. After admission:

```text
source -> lossless Rowan CST -> typed AST wrappers -> semantic AST / IR
```

with a mandatory differential gate — the old parser's AST equals the
CST-lowered AST, diagnostics keep their codes, and canonical formatter bytes
do not move. Rowan must not become a second source of semantics.

---

## Epic G — Phases 5–6: verified lifting (`SWG-LIFT-*`)

### SWG-LIFT-01 — Recognizer contract

**Kind:** code. **Depends on:** 4C-06

```text
struct Recognition<T> {
    construct:       T,
    covered_ranges:  Vec<SemanticRange>,
    evidence:        Evidence,
    cost:            Cost,
    alternatives:    Vec<T>,
}
```

A recognizer proposes an explanation. It never rewrites the score.

### SWG-LIFT-02 — Baseline recognizers

**Kind:** code. **Depends on:** LIFT-01

As separate slices: exact repeat; repeat with final variation; transposed
motif; shared rhythm with different pitch; section/bar scope; mask/overlay
relation. Each ships positive fixtures, near-miss negatives, adversarial
fixtures, coverage, residual delta, and description-cost delta. The
false-positive control from S16 applies: repeated pitches alone do not imply
a motif.

### SWG-LIFT-03 — Exact residual generation

**Kind:** code. **Depends on:** LIFT-02

Everything the recognizers do not cover becomes exact patches.

```text
recognized program + residual -> execute -> ExactSemanticDiff is empty
```

Low coverage stays visible. A program where 97% of the music sits in a
literal residual is not a success, and the report must not let it look like
one.

### SWG-LIFT-04 — Deterministic DP optimizer

**Kind:** code. **Depends on:** LIFT-03

```text
min( reconstruction_error + AST_size + residual_size
     + obscure_construct_penalty + instability_penalty )
```

Versioned cost policy; deterministic tie-break; bounded candidate count;
retained alternatives; cross-version cost comparison forbidden by the API
(S16 already requires this); the selected program is always re-executed.

### SWG-LIFT-05 — Lift CLI and report

**Kind:** CLI. **Depends on:** LIFT-04

```text
griff swang lift input.gp5 --mode lossless
griff swang lift input.mid --mode structural
griff swang roundtrip input.gp5
griff swang explain program.swg
```

The report carries the source digest, import loss, recognized constructs,
structural coverage, residual ratio, alternatives, the selected cost policy,
the exact and normalized diffs, and unsupported facts.

---

## Dependency order

```text
INF-01 status sync                    (done)
  -> INF-03 syntax split              (done)
  -> INF-02 level-2 contract          (done)
       │
       ├─→ INF-04 SourceMap ──→ INF-05 recovery ──┐
       │                                          ├─→ 4A-06 parser skeleton
       └─→ 4A-01 exact grammar ───────────────────┘
  -> 4A-02..4A-09 writer / parser / builder
  -> 4A-10..4A-14 dump / verify / laws / fuzz
  -> 4B corpus acceptance
  -> 4C selectors and patches
  -> 4D MIDI projection
  -> AUTH typed IR and first operators (level 3+)
  -> UI richer Playground
  -> LIFT recognizers and verified lift
```

INF-03 comes before INF-02 deliberately. The split is pure cleanup with an
acceptance that can only fail one way, and it moves nothing into the design
space; deciding how level 2 coexists with level 1 is easier once level 1 is
physically separated into `ast/v1`, `parser/v1`, and `format/v1`. The
reverse order would have INF-03 quietly encoding a dispatch shape that
INF-02 then documents after the fact.

Only independent branches may run in parallel:

```text
exact writer (4A-03..05) ─────┐
parser platform (INF-04..06)  ├─→ Phase 4A integration
CLI shell (4A-10 skeleton) ───┘
```

Inventing patch syntax before the selector contract, or adding ten operators
before the typed IR, produces a collection of attractive verbs each holding
its own private theory of time, budget, seed, and reality.

## Milestone M1

M1 is complete when all ten hold:

1. the level-2 admission contract is accepted *(done — spec §5)*;
2. `syntax.rs` is split with no behaviour change *(done)*;
3. a full `SourceMap` replaces `ProgramSpans`;
4. the normative exact-score grammar is written;
5. the canonical writer covers the whole `Score`;
6. the parser and checked builder exist;
7. `griff swang dump` works;
8. `griff swang verify` works;
9. the three round-trip laws are asserted;
10. the fuzz and property gate for exact text is blocking.

At that point Swang is not only a generative recipe language but a
verifiable textual surface over the whole canonical Griff model — the
foundation on which `mask`, `rotate`, recipes, and lifting can land.

## What this backlog will not do

- rewrite the level-1 lexer or parser on a generator for its own sake;
- add `rowan` before the UI-07 gate passes;
- introduce a second score model beside canonical `Score`;
- let `griff-core` depend on `griff-swang` or `griff-pattern`;
- ship `stutter`, `thin`, or `bounded_walk` ahead of their named
  prerequisites;
- allow more than one rotation semantics across scheduler, traversal, and
  language;
- combine a mechanical refactor with a behaviour improvement in one PR.

## See also

- [`spec.md`](spec.md) — normative semantics and grammar
- [`../stages/S16-swang-language-and-verified-lifting.md`](../stages/S16-swang-language-and-verified-lifting.md)
- [`../adr/0029-swang-authoring-and-verified-lifting.md`](../adr/0029-swang-authoring-and-verified-lifting.md)
- [`../proposals/pattern-operator-inventory.md`](../proposals/pattern-operator-inventory.md)
- [`../stages/S8-preview-app.md`](../stages/S8-preview-app.md) — Playground slices
- [`../fuzzing.md`](../fuzzing.md) — ADR-0010 gate policy
