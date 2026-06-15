# ADR 0021: Property-based invariants over the canonical Score

Date: 2026-06-15
Status: Proposed

## Context

griff's automated checks cluster at two ends of a spectrum and thin out in the
middle:

- **Fuzzing (ADR-0010)** drives nine targets that feed *adversarial bytes*
  through the format adapters and core transforms. It proves robustness — no
  panic, hang, or unbounded allocation — and checks a few invariants, but only
  over Scores reachable through an importer. `score_projection.rs` says so
  itself: *"A future upgrade will add an `arbitrary`-generated canonical Score
  path for richer structure coverage."*
- **Example tests** (golden snapshots, hand-built unit cases) pin specific,
  hand-chosen inputs.

Between them sits the gap: *semantic invariants over arbitrary valid Scores*.
`proptest` is already a dev-dependency but is used in exactly one file
(`corpus_schema.rs`, JSON round-trip only). Nothing generates random valid
`Score`s and asserts the canonical model's structural laws.

The gap is not theoretical. Two ordering bugs in the ADR-0020 normalized dump —
notes keyed by `pitch` before `string`, and voices echoing import order —
shipped behind passing example tests and were caught only in review. Both are
*relational ordering* invariants that one property over many generated chords
would have failed immediately. ADR-0020 itself designs a "tier C — properties
(no oracle)" layer (onsets monotonic, fret in range, string within tuning,
half-open ranges) that has not been built.

Prior art informs the shape (AGENTS.md prior-art rule): the QuickCheck /
Hypothesis lineage, applied as **valid-by-construction** generators plus
**independent-oracle** invariants plus **anti-vacuity coverage gates** — a
suite that recomputes its expectation independently of the system under test and
fails if a run never exercised the interesting shapes ("never passes
vacuously").

## Decision

Introduce a **property-testing layer** over the canonical `Score`, run under
ordinary `cargo test` (deterministic, fast, CI-friendly), complementary to — not
a replacement for — fuzzing (ADR-0010) and the type-level invariants the
newtypes (`Pitch`, `Velocity`, `TickRange`) already enforce at construction.

1. **Valid-by-construction `Score` strategy.** A `proptest` generator builds
   Scores valid by construction: tiled master bars, in-range strings / frets /
   pitches, onsets inside their bar. It is the reusable primitive the
   `score_projection` fuzzer asked for; the same shape can later seed that
   fuzzer.

2. **Independent-oracle invariants.** Properties recompute their expectation
   *independently* of the code under test rather than trusting it. The first
   consumer is the ADR-0020 `normalize`:
   - notes within a voice are ordered by the canonical key
     `(onset, string, fret, pitch)` (would have caught the pitch-vs-string bug);
   - voices within a bar are ordered by `id` (would have caught the import-order
     bug);
   - every note's `string` lies within the track tuning, `pitch ≤ 127`, and its
     onset inside the bar's half-open range;
   - **preservation**: the multiset of `(onset, string, fret, pitch)` out of
     `normalize` equals the multiset recomputed from the input — `normalize`
     neither drops, adds, nor mutates a note.

3. **Anti-vacuity gates.** Each property asserts the generated input actually
   exercised the shape it is about (a same-onset chord, a multi-voice bar); a
   run that did not is a test failure, not a silent pass.

4. **Scope and growth.** This ADR lands the layer and the `normalize` consumer.
   Natural follow-ups, each its own slice: generator determinism
   (`generate(seed)` reproducible — a SPEC hard rule), import loss-completeness
   (ADR-0020 tier B: every input note represented or reported), and invariants
   over the analysis transforms (structure metrics, features, closure).

## Consequences

- The ordering / relational bug class that example tests miss becomes
  systematically covered; the two ADR-0020 review escapes gain standing
  regression guards.
- The `Score` strategy is shared infrastructure: property tests today, a richer
  `score_projection` fuzz path tomorrow, with no new dependency (`proptest` is
  already vendored and license-clean).
- Accepted: properties target *semantic / relational* invariants, not what the
  type system already guarantees, to avoid hollow tests.
- Accepted: anti-vacuity gates couple a property to the generator's coverage; a
  generator change that stops producing chords fails loudly rather than passing
  vacuously.
- Accepted: this is additive — fuzzing (ADR-0010, adversarial bytes / robustness)
  and golden snapshots stay; the layer sits beside them.
- The first slice characterizes existing, already-fixed behaviour, so it is
  exempt from the red phase (AGENTS.md) but ships with anti-vacuity gates and is
  verified to fail against the pre-fix sort key.
