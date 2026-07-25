# Pattern Operator Inventory (S16 discussion)

Working research artifact for the reproducible-pattern-processes proposal:
sixteen candidate operators, each surveyed against what Griff already has,
classified adopt / adapt / reject with a reason and a later owner.

Status: for discussion (v1). Companion to
[`reproducible-pattern-processes.md`](reproducible-pattern-processes.md);
that proposal stays the thesis, this file is the inventory.
Scope: docs-only; binds nothing.

**Acceptance gate (normative for this document).** The inventory
*classifies ideas*; it promises no production semantics, no API, and no
delivery phase. Every `adopt` still requires its own spec section, golden
vectors, and a red→green slice through the normal S16 path. An `adopt`
here is a licence to *specify*, never a decision already taken.

Licence discipline: all prior art is idea-only reuse; GPL sources
(TidalCycles, TOT) are reference systems whose code never enters this MIT
workspace (AGENTS.md rule).

## Survey baseline — what already exists

- `griff-pattern`: `Kernel`, `fractalize` (seeded density pruning via
  `PruneSpec` — the *proven* thinning operation, per spec §3.4),
  `linearize` (`row_major | snake`), `ActivitySequence`, `ExpansionBudget`,
  `prune_hash_v1`.
- Swang spec: `map_rhythm` §1.11 with **cycle semantics** (the S6 scheduler
  rotates the template palette `bar_index mod templates`), tail policies
  (`reject` / `rest_pad`), `thin`'s frozen type contract §1.10
  (`Pattern2D -> Pattern2D`, may only flip `X -> .`, cell-selection rule
  deliberately unspecified, ships in no phase until that rule earns its own
  spec section), compaction and articulation-merge deferred as *separate
  operators with distinct output types*.
- Generator (`core/src/generate.rs`): strategies
  `ConstrainedRandomWalk` (leap-penalised walk), `ShuffleMotifs`
  (per-bar shuffle), `RepeatVariation` (repeat + last-note variation),
  `MotifTransposeVariation`; per-bar grid rotation by bar index; seeded
  `Xorshift64`.

Duplication risk is real: rotate-like and repeat-like semantics already
exist in two places. The point of this table is to catch the third
almost-identical `rotate`, each with its own philosophy of rotating the
universe, *before* it is written.

## Summary

| Operator | Musical purpose | Existing overlap | Decision | Later owner |
| --- | --- | --- | --- | --- |
| ring / cycle | repetition backbone, finite cyclic lanes | `map_rhythm` §1.11 palette cycling | **adapt** | S16 IR |
| rotate | phase-shift a cycle (displacement riffs) | §1.11 bar-index rotation | **adopt** (single semantics) | S16 IR |
| reverse | retrograde | none | **adopt** | S16 IR |
| ping-pong | forward-back oscillation | none | **adapt** (derived form) | S16 IR (sugar) |
| stutter | per-cell repetition, rhythmic insistence | none | **adopt** | S16 IR |
| creep | sliding-window walk, evolving loop | none | **adopt** | S16 IR + cockpit module |
| pad-to-multiple | fit material to bar multiples | `map_rhythm` tail policy `rest_pad` | **adapt** | S16 IR |
| euclidean-mask | even onset distribution | none (deterministic) | **adopt** | S16 IR |
| urn | no-repeat randomness | `ShuffleMotifs` strategy | **adopt** | S16 IR |
| bounded-walk | constrained wandering line | `ConstrainedRandomWalk` strategy | **adapt** (defer) | S6 generator stays |
| multi-cycle | polymeter-style long development | none | **adopt** | S16 IR |
| reset-on-event | re-sync axes at musical events | none (no events contract yet) | **adapt** (blocked) | S16/S8 |
| envelope-modulation | parameter development over extent | none | **adopt** | S16 IR + S14 |
| mask | explicit thinning by a given mask | `thin` §1.10 type contract | **adopt** (as first specified `thin` rule) | S16 spec |
| repeat | plain finite repetition | ring; `RepeatVariation`; §1.11 cycling | **adapt** (sugar over ring) | S16 IR |
| thin | density reduction | frozen §1.10 contract; `fractalize density/seed` | **defer to spec** | Swang spec |

## Per-operator detail

Field key: *Prior art* names idea sources only. *Identity effect* uses the
recipe / content / lineage split from the transactional-editing proposal.
*Boundedness* must hold under `ExpansionBudget`/`CycleBudget`.

### ring / cycle — adapt

- **Musical purpose**: the repetition backbone — a finite sequence read
  cyclically, per axis (pitch, rhythm, articulation…).
- **Prior art**: Sonic Pi `ring`/`tick`; Isobar loop patterns; TidalCycles
  cycles (idea only, GPL).
- **Input / output**: finite lane `[T; n]` + cursor → windowed events;
  cursor advances explicitly via the query contract.
- **Existing overlap**: `map_rhythm` cycle semantics already rotate the
  template palette round-robin. The IR-level ring must be specified as the
  *generalization* that §1.11 becomes a client of — not a second cycling
  notion beside it.
- **Boundedness**: window queries only; full-period materialization
  forbidden (CycleBudget).
- **State**: one cursor per lane, explicit in `cursor_state`.
- **Randomness**: none.
- **Identity effect**: recipe (program hash); content unchanged by identity
  of the ring itself.
- **Composition laws**: length-preserving per period; commutes with
  per-element maps.
- **Decision / reason**: **adapt** — semantics must be reconciled with
  §1.11 rather than invented fresh.
- **Later owner**: S16 pattern IR.

### rotate — adopt (exactly one rotation semantics)

- **Musical purpose**: phase displacement of a cycle — the classic
  displaced-riff device.
- **Prior art**: Isobar rotate; Total Serialism rotation.
- **Input / output**: finite lane + offset `k` → same-length lane,
  cyclically shifted; direction and origin fixed by spec.
- **Existing overlap**: §1.11 rotates the palette by bar index — that is a
  *client* of rotation, not a competing definition. Any traversal-level
  rotation is likewise a client.
- **Boundedness**: length-preserving, trivially bounded.
- **State**: none (pure); rotating-per-cycle is `rotate ∘ ring`, not new
  state.
- **Randomness**: none.
- **Identity effect**: recipe only.
- **Composition laws**: `rotate(a) ∘ rotate(b) = rotate(a+b mod n)`;
  preserves multiset and length; commutes with `reverse` up to sign.
- **Decision / reason**: **adopt**, with the explicit constraint that the
  repository gets *one* rotate: a cyclic index shift on a finite sequence.
  Everything else composes from it.
- **Later owner**: S16 pattern IR.

### reverse — adopt

- **Musical purpose**: retrograde of a lane.
- **Prior art**: universal (Isobar, Total Serialism, classical retrograde).
- **Input / output**: finite lane → same-length lane, order reversed.
- **Existing overlap**: none.
- **Boundedness**: length-preserving.
- **State**: none. **Randomness**: none.
- **Identity effect**: recipe only.
- **Composition laws**: involution (`reverse ∘ reverse = id`); preserves
  multiset and length.
- **Decision / reason**: **adopt** — cheap, lawful, obviously useful.
- **Later owner**: S16 pattern IR.

### ping-pong — adapt (derived form)

- **Musical purpose**: forward-then-back oscillation without doubling the
  endpoints.
- **Prior art**: Isobar ping-pong; ubiquitous in hardware sequencers.
- **Input / output**: finite lane → lane of length `2n−2` (endpoints not
  repeated), then typically ring-ed.
- **Existing overlap**: none directly.
- **Boundedness**: bounded (≤ 2n).
- **State**: none beyond the ring cursor it usually feeds.
- **Randomness**: none.
- **Identity effect**: recipe only.
- **Composition laws**: expressible as
  `concat(p, reverse(interior(p)))` — a composition of adopted primitives.
- **Decision / reason**: **adapt** — enters as *derived* sugar over
  concat/reverse/ring; a primitive only if the derived form measurably
  fails (it should not).
- **Later owner**: S16 pattern IR (sugar tier).

### stutter — adopt

- **Musical purpose**: per-cell repetition (`n` short notes where one was)
  — rhythmic insistence, subdivision emphasis.
- **Prior art**: Isobar stutter; live-coding idiom.
- **Input / output**: lane + count `n` → lane of length `n·len`.
- **Existing overlap**: none. Note the §1.10 law: two adjacent `X` are two
  short notes, never a merged sustain — stutter output obeys it by
  construction; merging stays with the deferred articulation operators.
- **Boundedness**: multiplies length ×n — must charge the
  `ExpansionBudget`/`CycleBudget`; exceeding is a typed refusal.
- **State**: none. **Randomness**: none.
- **Identity effect**: recipe only.
- **Composition laws**: `stutter(1) = id`;
  `stutter(a) ∘ stutter(b) = stutter(a·b)`.
- **Decision / reason**: **adopt** — simple, lawful, budget-aware.
- **Later owner**: S16 pattern IR.

### creep — adopt

- **Musical purpose**: a sliding window over material, advanced by a small
  shift each repetition — the evolving-loop sound; the flagship
  technique-module example in the transactional-editing proposal.
- **Prior art**: Isobar creep.
- **Input / output**: lane + (window, shift, repetitions) → bounded event
  stream of overlapping windows.
- **Existing overlap**: none.
- **Boundedness**: `repetitions` is finite and budget-charged.
- **State**: window-position cursor, explicit.
- **Randomness**: none.
- **Identity effect**: recipe only.
- **Composition laws**: windows preserve source order; output is a
  subsequence-of-concatenation fact usable by provenance.
- **Decision / reason**: **adopt** — high musical value, clean semantics,
  and the natural first cockpit technique module.
- **Later owner**: S16 pattern IR; cockpit module later (S8).

### pad-to-multiple — adapt

- **Musical purpose**: extend material with timed rests to a multiple of a
  declared unit, so it tiles bars cleanly.
- **Prior art**: Isobar pad; TOT form tools (idea only, GPL).
- **Input / output**: `ActivitySequence` + unit → padded sequence.
- **Existing overlap**: `map_rhythm`'s tail policy `rest_pad` already pads
  an incomplete final bar with timed rests. One padding notion must
  survive, not two: pad-to-multiple should be specified as the
  generalization the §1.11 tail policy instantiates (or rejected in favour
  of extending tail policy).
- **Boundedness**: adds < one unit of rests; bounded.
- **State**: none. **Randomness**: none.
- **Identity effect**: recipe only; changes content (rests are content).
- **Composition laws**: idempotent at the same unit.
- **Decision / reason**: **adapt** — reconcile with tail policy first.
- **Later owner**: S16 pattern IR / Swang spec.

### euclidean-mask — adopt

- **Musical purpose**: maximally even distribution of `k` onsets over `n`
  slots (Bjorklund) — the canonical groove-mask family.
- **Prior art**: Toussaint's Euclidean-rhythms result; Sonic Pi `spread`;
  Total Serialism euclid; widespread.
- **Input / output**: `(k, n, rotation)` → `ActivitySequence` of length
  `n` (as a *generator*); applied to a pattern it is a fully specified
  thinning rule (see `mask` / `thin`).
- **Existing overlap**: none; deterministic, so it does not overlap the
  seeded density pruning of `fractalize`.
- **Boundedness**: length `n`, trivially bounded.
- **State**: none. **Randomness**: none.
- **Identity effect**: recipe only.
- **Composition laws**: rotation composes with `rotate`;
  `E(n, n) = all-on`, `E(0, n) = all-off`.
- **Decision / reason**: **adopt** as an activity-sequence generator; as a
  thinning rule it routes through the `thin` spec path below.
- **Later owner**: S16 pattern IR.

### urn — adopt

- **Musical purpose**: randomness without immediate repetition — draw
  without replacement, refill when empty.
- **Prior art**: Total Serialism urn; serial-composition practice.
- **Input / output**: lane + named seed stream → permuted draw sequence;
  refills per exhausted cycle.
- **Existing overlap**: `ShuffleMotifs` shuffles degrees per bar — an
  urn-shaped idea inside one strategy. The IR operator must not silently
  re-specify that strategy; if adopted, the strategy is later re-expressed
  or the two are documented as distinct.
- **Boundedness**: one permutation per cycle; bounded.
- **State**: remaining-items cursor, explicit.
- **Randomness**: one named stream (`urn` kind, stable `operator_id`).
- **Identity effect**: recipe (stream identity + seed); content varies with
  seed by design.
- **Composition laws**: each cycle emits each element exactly once
  (permutation invariant — a golden-vector property).
- **Decision / reason**: **adopt** — the permutation invariant is exactly
  the kind of law the proptest/golden-vector discipline can pin.
- **Later owner**: S16 pattern IR.

### bounded-walk — adapt (defer)

- **Musical purpose**: constrained wandering line — melodic motion without
  large leaps.
- **Prior art**: Total Serialism random walk; `ConstrainedRandomWalk`.
- **Input / output**: start + step distribution + bounds + named stream →
  value lane.
- **Existing overlap**: **direct** — the generator's
  `ConstrainedRandomWalk` strategy (leap-penalised walk on scale degrees)
  is this operator living at the strategy tier.
- **Boundedness**: finite length; bounded range by contract.
- **State**: current-position cursor. **Randomness**: one named stream.
- **Identity effect**: recipe (stream + seed).
- **Composition laws**: all outputs within declared bounds (typed refusal
  otherwise).
- **Decision / reason**: **adapt, deferred** — a pattern-tier walk must
  either share semantics with the strategy or not exist; extracting it is
  generator-unification work, not inventory work. Recording the overlap
  here is the deliverable.
- **Later owner**: S6 generator (status quo); S16 IR only with a
  unification decision.

### multi-cycle — adopt

- **Musical purpose**: several axes cycling with coprime-ish periods —
  long non-repeating development from small material (full period = LCM).
- **Prior art**: Sonic Pi multi-ring idiom; isorhythm (talea/color).
- **Input / output**: set of rings with independent periods → combined
  windowed query.
- **Existing overlap**: none at the combined-axes level.
- **Boundedness**: **the** CycleBudget case: LCM is finite but can be
  astronomical; full-period materialization is forbidden, windows are
  evaluated lazily, budget breach is a typed refusal.
- **State**: one cursor per axis. **Randomness**: none.
- **Identity effect**: recipe only.
- **Composition laws**: combined period divides LCM of axis periods;
  per-axis projection recovers each ring.
- **Decision / reason**: **adopt** — the core payoff of the proposal's
  §2.3, with its budget hazard named.
- **Later owner**: S16 pattern IR.

### reset-on-event — adapt (blocked)

- **Musical purpose**: re-synchronize cursors at musical events
  (`phrase_started`, `section_changed`…) — alignment without shared
  mutable state.
- **Prior art**: Sonic Pi `cue`/`sync`; Isobar reset-on-trigger.
- **Input / output**: cursor set + event name → cursor set (reset), at a
  declared boundary.
- **Existing overlap**: none — and the synchronization-events contract it
  needs (proposal §2.6) does not exist yet.
- **Boundedness**: state-only; bounded.
- **State**: rewrites cursors — the one operator here whose *effect* is
  state. **Randomness**: none.
- **Identity effect**: recipe; changes subsequent content deterministically.
- **Composition laws**: reset is idempotent per event instance.
- **Decision / reason**: **adapt, blocked** — cannot be specified before
  the events contract; adopting it now would invent that contract by
  side effect.
- **Later owner**: S16 IR + S8 (events surface).

### envelope-modulation — adopt

- **Musical purpose**: a parameter developing over a phrase/section extent
  (density rising to a climax and falling) instead of a scalar.
- **Prior art**: Fenv; TOT envelopes (idea only, GPL).
- **Input / output**: curve (rational breakpoints + versioned interpolation
  law, per the proposal's §2.4 no-floats amendment) + target parameter →
  modulated operator parameter over the extent.
- **Existing overlap**: none as a request-side object; S14
  `ComplexityProfile` is the *measurement* counterpart (request vs
  achieved).
- **Boundedness**: finite breakpoints; evaluation per queried window.
- **State**: none. **Randomness**: none.
- **Identity effect**: recipe (curve is part of the program hash).
- **Composition laws**: evaluation is exact rational arithmetic; identical
  breakpoints + law ⇒ identical values, cross-implementation.
- **Decision / reason**: **adopt** — the semantics question (rational
  representation, interpolation law) is already answered in the parent
  proposal.
- **Later owner**: S16 pattern IR; S14 for the measurement side.

### mask — adopt (as the first fully specified `thin` rule)

- **Musical purpose**: thin a pattern by an explicit, given mask —
  deterministic sculpting of density.
- **Prior art**: Isobar mask; TidalCycles boolean patterns (idea only,
  GPL).
- **Input / output**: `Pattern2D` + explicit mask → `Pattern2D`; exactly
  the frozen §1.10 `thin` type contract (may only flip `X -> .`, preserves
  dimensions, cell count, coordinates, post-`linearize` length).
- **Existing overlap**: `thin` §1.10 (type fixed, selection rule
  unspecified); `fractalize density/seed` (seeded selection rule, already
  shipped).
- **Boundedness**: length-preserving.
- **State**: none. **Randomness**: none (the mask is explicit data).
- **Identity effect**: recipe (mask is program data).
- **Composition laws**: `mask(m1) ∘ mask(m2) = mask(m1 ∧ m2)`;
  monotone (never adds activity).
- **Decision / reason**: **adopt** — an explicit mask argument *is* a
  fully specified cell-selection rule, which is precisely what §1.10 says
  `thin` waits for; this lands as `thin`'s first spec section, not as a
  new vaguer abstraction (§3.4's warning).
- **Later owner**: Swang spec section + S16 pattern core.

### repeat — adapt (sugar over ring)

- **Musical purpose**: plain finite repetition (`n` times, then stop).
- **Prior art**: universal.
- **Input / output**: lane + `n` → lane of length `n·len`.
- **Existing overlap**: triple — `ring` (unbounded cyclic form), §1.11
  palette cycling, and the `RepeatVariation` strategy.
- **Boundedness**: ×n, budget-charged.
- **State**: none. **Randomness**: none.
- **Identity effect**: recipe only.
- **Composition laws**: `repeat(1) = id`; finite unrolling of `ring`.
- **Decision / reason**: **adapt** — sugar over ring with an explicit
  budget; rejected as an independent primitive (that is how the third
  repeat is born).
- **Later owner**: S16 pattern IR (sugar tier).

### thin — defer to spec

- **Musical purpose**: density reduction of a pattern.
- **Prior art**: live-coding `degrade`-family; `fractalize density/seed`.
- **Input / output**: frozen — §1.10.
- **Existing overlap**: this *is* the spec's operator; the proven concrete
  rule is seeded density pruning, already named by
  `fractalize density/seed` (§3.4).
- **Decision / reason**: **defer to spec** — this inventory adds no new
  decision; it only enumerates the candidate selection-rule families for
  the future spec section: explicit mask (above), euclidean selection,
  seeded density (exists). Per §3.4 the language must not pre-create a
  vaguer abstraction, and this row exists to make that impossible to do by
  accident.
- **Later owner**: Swang spec.

## What the inventory deliberately does not contain

Compaction, articulation merge (`merge_adjacent` / `tie_adjacent` /
`sustain_runs`), polyphonic lowering, accent/velocity lanes, pitch/harmony
transforms and fretboard operators: all already named as deferred in the
Swang spec (§1.10, §3.4, §4) with their own admission bars. They are not
re-litigated here.
