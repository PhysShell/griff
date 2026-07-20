# Proposal: Reproducible Pattern Processes

Sonic Pi-inspired process semantics for deterministic musical variation.

Status: proposal for discussion
Scope: design input for future Swang authoring and cockpit interaction. No production generation behaviour changes; the canonical `Score`, master timeline, frozen S6 strategies, and accepted Swang Phases 0–3 remain untouched.

## 1. Goal

Add a small, bounded vocabulary for describing musical material as several independently cycling parameter streams that are combined on a symbolic beat grid and compiled into the existing canonical `Score`.

The useful lesson from Sonic Pi is not its synthesizers or Ruby host language. It is the separation of:

- cyclic values (`ring`);
- independently advancing cursors (`tick` / named ticks);
- concurrently described parts (`live_loop`);
- named synchronization events (`cue` / `sync`);
- deterministic random streams selected by seed;
- live replacement at explicit musical boundaries.

For Griff these ideas must become finite, pure, inspectable score construction. They must not introduce ambient time, host-code execution, unbounded loops, or a second musical model.

## 2. Why Griff needs this

Griff already has deterministic generation, rhythm templates, a master timeline, playback, history/provenance, and Swang as a bounded authoring language. What remains awkward to express is a common musical construction:

```text
pitch cycle:        E4 G4 B4 D5 B4
rhythm cycle:       1/8 1/8 1/4
articulation cycle: pick hammer pick slide
accent cycle:       strong weak weak medium
```

The cycles may have different lengths. Advancing them independently produces a long, reproducible composite pattern from a small amount of authored material. This is useful for swancore and mathcore because it can model:

- tapping figures whose pitch and attack cycles phase against each other;
- accent masks such as `3+3+2` over a differently sized pitch cell;
- repeated motifs with evolving articulation or fretboard realization;
- complementary parts that react to named section or rest events;
- controlled exploration where one dimension changes and the others stay frozen.

Today the same intent can be encoded by manually expanding events, but the structure and provenance are lost. Swang exists precisely to retain that structure.

## 3. Proposed semantic core

### 3.1 Finite cyclic values

A pattern process owns named finite rings:

```text
ring pitch = [E4, G4, B4, D5, B4]
ring duration = [1/8, 1/8, 1/4]
ring articulation = [pick, hammer, pick, slide]
```

A ring is immutable and indexable at any non-negative logical step by modular arithmetic. Rings contain typed domain values, not arbitrary host objects.

### 3.2 Named cursors

Each consumer advances an explicit named cursor. Cursors are independent even when they read the same ring.

```text
next pitch using pitch_step
next duration using rhythm_step
next articulation using technique_step
```

Cursor state is part of the bounded execution plan and provenance. There is no implicit global counter.

### 3.3 Bounded process expansion

A process is expanded for an explicit musical extent:

```text
for 4 bars
for 32 events
until section_end
```

Every form lowers to a statically or structurally bounded count before realization. General-purpose loops and recursion remain forbidden by ADR-0029. A budget breach is a typed error, never silent truncation.

### 3.4 Musical-time synchronization

Processes communicate only through named symbolic events placed on the master timeline:

```text
emit section chorus
emit guitar_a_rest
await next phrase_boundary
```

`await` resolves against a finite event schedule during compilation. It is not a blocking runtime thread. The master timeline remains the single source of truth under ADR-0003.

### 3.5 Deterministic variation streams

Every stochastic choice names both its stream and seed:

```text
choose pitch_variant from candidates
  stream pitch_variation
  seed 10300
```

A stream is path-addressed or counter-addressed under a versioned integer algorithm. Identical source, language level, stream names, seeds, and inputs must produce byte-stable normalized expansion and a semantically identical `Score`.

Separate stream identities allow one axis to mutate while others remain fixed:

```text
pitch seed:        frozen
rhythm seed:       812 -> 813
articulation seed: frozen
```

Raw seed remains provenance, not a quality feature or ordinal musical control.

### 3.6 Explicit replacement boundaries

The cockpit may stage a parameter or process edit for one of a small set of boundaries:

- next event;
- next beat;
- next bar;
- next phrase or section marker;
- restart from selection start.

This is an interaction and audition contract. The committed result is still a finite `Score` snapshot. No mutable live process becomes canonical state.

## 4. Relationship to existing architecture

### Canonical `Score`

Unchanged. Pattern processes are authoring/orchestration constructs that lower into the existing hierarchy. They do not become a parallel event model.

### Master timeline

Unchanged. All process advancement and synchronization are expressed in rational symbolic time and resolved against the canonical timeline. Wall-clock scheduling belongs only to playback.

### `griff-pattern`

The pure algebra is the likely home for finite rings, cursor addressing, and deterministic stream primitives if Phase 0 finds they are not already representable cleanly. It remains std-only and independent of `griff-core` as required by ADR-0029.

### `griff-swang`

Swang is the likely surface language. The proposal does not define syntax yet. Syntax follows only after semantic inventory and collision analysis against the accepted language levels.

### S6 generation

Frozen strategies stay untouched. A future integration may compile one process projection into the already-defined explicit rhythm override or another separately accepted seam. This proposal does not change precedence or reranking.

### Cockpit

The cockpit may expose rings and streams visually as lanes beside the piano roll or tablature view. Edits reduce to typed intents and produce a new immutable score/history entry. The renderer must not invent separate process semantics.

## 5. Non-goals and rejected shortcuts

- **No Sonic Pi dependency or code transplant.** Sonic Pi is prior art and UX inspiration, not a library boundary for Griff.
- **No embedded Ruby, Lua, JavaScript, or arbitrary host language.** Host execution would destroy boundedness, portability, wasm determinism, and reviewability.
- **No infinite `live_loop`.** Authoring may feel iterative, but every compiled artifact is finite.
- **No wall-clock `sleep` in composition semantics.** Durations are rational musical values; playback maps them to time.
- **No ambient random state.** Every random decision has a named stream, explicit seed, and versioned algorithm.
- **No mutable canonical process graph.** History stores source/provenance and resulting immutable snapshots.
- **No claim that Euclidean rhythms or phased cycles generate swancore by themselves.** They are bounded operators inside a corpus- and structure-aware system, not a genre vending machine.
- **No new roadmap stage in this proposal.** Placement follows an audit and the repository glossary rule.

## 6. Suggested Phase 0: semantic inventory only

No production code. Deliver one audit answering:

1. Which concepts are already expressible by current `griff-pattern` and Swang Phases 0–3?
2. What is genuinely missing: rings, named cursors, independent deterministic streams, event schedules, or only surface syntax?
3. Can named synchronization be represented as ordinary finite dependencies without introducing a scheduler?
4. Which cockpit intents and history/provenance fields would be required for single-axis mutation and boundary-applied edits?
5. What minimum example demonstrates musical value beyond manual event expansion?
6. Where does the work belong: a later S16 phase, an amendment to an existing planned phase, or a separate accepted ADR?

Required negative checks:

- no dependency from `griff-core` to Swang or pattern authoring;
- no OS entropy or platform-sized integers in semantic hashes;
- no floats in deterministic process semantics;
- no unbounded expansion path;
- no production S6 behaviour change;
- no second timeline or score representation.

## 7. Candidate proof fixture

A useful synthetic acceptance fixture should be small enough to inspect by eye:

```text
pitch ring length:        5
rhythm ring length:       3
articulation ring length: 4
extent:                   60 events
```

Because `lcm(5, 3, 4) = 60`, the full composite repeats exactly at the fixture boundary. The proof records:

- the first cycle and final cycle;
- exact cursor positions at every event;
- deterministic replay under identical seeds;
- a one-axis seed mutation changing only the declared axis and downstream derived facts;
- canonical score equality after format/parse or history replay;
- a budget failure at 59 allowed events when 60 are required.

A second fixture should use two finite parts and a named `phrase_boundary` event to prove deterministic synchronization without runtime threads.

## 8. Prior art notes

Sonic Pi provides the motivating vocabulary:

- rings and independent named ticks;
- redefinable live loops with separately managed musical time;
- `cue` / `sync` over a time-state event system;
- deterministic seeded random streams;
- MIDI, OSC, and external-clock integration at the runtime boundary.

Griff adopts only the finite symbolic ideas. Sonic Pi's realtime runtime, synthesizer stack, editor, and Ruby execution model remain outside scope.

Related accepted Griff decisions remain stronger constraints than the inspiration:

- ADR-0002: canonical score model;
- ADR-0003: master timeline as the single source of truth;
- ADR-0010: fuzz core invariants and format boundaries;
- ADR-0027: shared cockpit interaction core and immutable domain-aware actions;
- ADR-0029: bounded deterministic Swang language and pattern algebra.

## 9. Decision requested

Approve only a **Phase 0 semantic inventory** after current owned roadmap work permits it. Do not schedule implementation, allocate a stage number, change production generation, or reopen frozen Swang phases from this proposal alone.
