# ADR 0012: Complementary part generation (ComplementArranger)

Date: 2026-05-31
Status: Proposed

## Context

The S6 rule generator produces a single part in isolation. The musically
valuable next step is *arrangement*: given an existing guitar part (part A),
generate a second part (part B) that is **related to A on chosen musical axes
and deliberately contrasting on others** — same rhythm but different technique,
same harmony but freed-up rhythm, call-and-response, low-register support, and
so on. This is the difference between "generate another riff" and "write a part
that knows what the first part is doing".

Most of the primitives already exist: the canonical model has `Track`/`Voice`
and shared `MasterBar`s (so two parts are rhythmically locked for free,
ADR-0003); S6 already has `RhythmCopyPitchSubstitute` and a `source_rhythms`
input; the corpus has harmony/technique tags (`SwancoreTag`). What is missing is
(a) a first-class notion of a *typed, multi-axis relation between two parts*, and
(b) a generation mode conditioned on a sibling part rather than on a previous
phrase.

The relation is naturally a **hyperedge**, not a binary edge: A and B are linked
by a *set* of per-axis relations (rhythm / harmony / register / technique /
density / contour) simultaneously. This is the concrete use that justifies the
S7 graph layer being a hypergraph rather than a similarity graph.

Two scoping facts shape the decision:

- Generation and feature extraction currently run on the legacy linear model;
  ADR-0011 ports `feature` and `generate` onto the canonical model first.
- Per glossary §17.5 / SPEC, no neural layer before a rule-based baseline.

## Decision

We add **ComplementArranger**, a rule-based engine that generates a
complementary part from an existing part, on the canonical model.

1. **The complement relation is a first-class, multi-axis object** (a
   hyperedge), and the word "relation" denotes **three distinct types**, kept
   separate in code:
   - *Relation-as-spec (input):* `ComplementSpec` / `RelationMode` — what the
     user asks for ("rhythm_lock, lower register, different technique").
   - *Relation-as-fact (corpus/graph):* a mined hyperedge between real chunks,
     owned by the S7 graph layer. **Out of scope here** (see §3).
   - *Relation-as-provenance (output):* metadata on a produced candidate (which
     mode, which per-axis scores).

2. **ComplementArranger is a constraint compiler, not a new generator.** It
   analyses part A into a part profile, picks a `RelationMode`, *derives* a
   concrete generation request (constraints + `source_rhythms` + pitch material)
   for part B, and delegates to the S6 generator. Part B is appended as a new
   `Track`/`Voice` on the same `Score`. Relative intent ("register = A − octave",
   "density = 0.6·A", "harmony = A", "technique ≠ A") is compiled into the
   existing absolute generation constraints. `rhythm_lock` reuses S6
   `RhythmCopyPitchSubstitute` with A's onset grid as `source_rhythms`.

3. **Generative-first.** The first version derives B from A by rule only; it does
   **not** mine real two-guitar pairs from the corpus. `ChunkMeta` and the
   corpus schema are untouched (`schema_version` stays 1). Learning complement
   relations from real pairs is later work in the S7 graph layer and may require
   a schema bump then.

4. **A pair validator is added** next to the S6 playability filter: a
   harmonic-compatibility check over the (A, B) pair (no dissonant clashes on
   coincident onsets, no register mud). This realises the "harmonic
   compatibility" edge type already named in glossary §9.

5. **Determinism holds:** for a fixed seed and fixed A, ComplementArranger is
   deterministic (SPEC §6).

6. **Naming.** The engine is `ComplementArranger` (not `DualGuitarArranger`): a
   complement is a *relation*, and there may be more than two parts. Modes live
   in a `RelationMode` enum (`rhythm_lock`, `register_contrast`, `call_response`,
   `support_layer`, `octave_double`, `counter_melody`).

7. **Roadmap placement.** Delivered as a new canonical stage **S13**, depending
   on S6. It is appended as the next free stage number rather than renumbering
   S7…S12 (append-only, consistent with the stage-label audit); logically it
   sits between the single-part generator (S6) and the graph layer (S7), which
   later learns complement relations from the corpus. Recorded in glossary §0
   and `docs/audit/`.

## Consequences

- griff gains an arrangement capability — "generate a second part musically
  tied to the first without copying it" — its strongest differentiator.
- The S7 graph layer gets a concrete reason to be a hypergraph: the complement
  relation is the canonical multi-axis edge.
- ComplementArranger reuses the S6 generator unchanged; it adds analysis,
  constraint derivation, and pair validation, not a new generation core.
- The S9 feedback layer can later learn weights over *relation* axes
  (rhythm_similarity, technique_overlap, register_overlap, density_ratio), not
  just single-phrase features.
- Accepted: requires ADR-0011's canonical port of `feature` and `generate`
  first; this stage does not ship until that port lands.
- Accepted: richer per-part features (register band, contour, normalised
  density, technique multiset, harmonic context) must be added to the feature
  layer; today's `PhraseFeatures` is insufficient.
- Accepted: corpus-mined complement relations are deferred; the first version is
  purely generative and cannot yet learn from real two-guitar material.
