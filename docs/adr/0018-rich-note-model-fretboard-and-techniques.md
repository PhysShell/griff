# ADR 0018: Rich note model — fretboard position and multi-technique with evidence

Date: 2026-06-03
Status: Proposed
Supersedes: ADR-0014

## Context

`griff` "thinks like a guitar engine but plays like a MIDI robot with a cardboard
pick": the canonical note is still pianoroll-shaped. Two gaps, on the same note,
keep it there — so they are one migration, not two (hence this ADR subsumes the
fretboard-only ADR-0014).

**1. Technique is a single hat.** `AtomNote` carries `articulation:
Option<Articulation>`, and the *same* flat `Articulation` enum is reused for
`TechniqueSpan.technique`. One note cannot be, at once, a hammer-on target, under
a palm-mute, accented, and a pinch harmonic — yet on a guitar that is ordinary.
`gp.rs` already pays the price: it keeps "only the highest-priority articulation"
and drops the rest. Two distinct problems are conflated under one `Option`:
*per-note* attributes that co-occur (accent, ghost, harmonic) and *spanning*
techniques that relate notes or cover a range (slide, legato, palm-mute).

**2. No hand position.** A note is only `pitch`; there is no string/fret. What
separates a guitar part from "MIDI in range" is *where the hand is* — position
shifts, string choice, fret distance, reachable spans. `fret_jump_penalty`
(ADR-0013) and the playability gate cannot even be *expressed* on a pitch. The
information already exists at the boundary — `gp.rs` computes pitch *from*
`(string, open_string_midi)` and then throws the position away — and `Tuning`
exists only as a corpus string, not in the model.

**3. Inference must not masquerade as fact.** The glossary (§5, §17.3) requires
inferred articulations to carry confidence/evidence. Today a technique is a bare
enum with no provenance: a palm-mute *read from Guitar Pro* and one *guessed from
MIDI* are indistinguishable.

This is load-bearing for S7 (DP fretboard state + `fret_jump_penalty`), S3
(Guitar Pro import that stops flattening), and the complement `technique_overlap`
scoring axis (ADR-0012/0017). It is a cross-cutting core-model change, so it is
recorded before code.

## Decision

We make the note **fretboard-aware and multi-technique, with provenance** — one
migration on the note/group. (Carries ADR-0014's string/fret decision forward
verbatim in spirit; adds the technique model.)

> **Amendment (2026-06-04, during implementation).** §3–§4 are refined from the
> original "each mark carries evidence": per-note **marks** are an evidence-free
> `Copy` bitset, and `TechniqueEvidence` lives on **spans and positions only**.
> Rationale: note-level marks (accent/ghost/harmonic/…) are near-boolean facts
> where per-mark confidence is degenerate, while the inferred-with-confidence
> cases are genuinely the spans (MIDI cues) and positions (string/fret guesses);
> keeping marks a bitset preserves `AtomNote: Copy` (zero ripple across the
> codebase). This is the ADR's own "reshape, don't fork" loop in action.

1. **Fretboard position on the note (from ADR-0014).** `AtomNote` gains
   `position: Option<FretboardPosition>` (`string` + `fret`), interpreted under a
   `Tuning`. Optional: MIDI import usually cannot recover it (`None`/inferred),
   Guitar Pro supplies it directly. Pitch↔(string,fret) is one-to-many; position
   *inference* stays a separate, documented, lossy sub-problem (a small search of
   its own), deferred — never fabricating certainty.

2. **`Tuning` is a first-class model type, per `Track`** (default Standard E,
   ADR-0006), promoting today's corpus string and `gp.rs`'s per-string array into
   the reference that maps pitch ↔ (string, fret). Per-track because instruments
   differ (a bass is not a guitar).

3. **Two technique scopes, split by domain — kill the single `Option` and the
   shared enum.**
   - *Per-note marks* (`NoteMark`): single-note attributes that **co-occur** —
     accent, ghost, staccato, dead/muted, natural/pinch harmonic, tap. The note
     carries a **set** (`marks`), replacing `articulation: Option<Articulation>`.
     Stored as a `Copy` bitset (`NoteMarks`) and **evidence-free** (see §4).
   - *Spanning techniques* (`SpanTechnique` on `TechniqueSpan`): techniques that
     relate notes or cover a range — slide, hammer-on, pull-off (legato), bend,
     vibrato, palm-mute, let-ring, tremolo, sweep. They stay on the group/voice
     with a range.

   The cut is principled: a mark is intrinsic to one note and stackable; a span
   is relational or time-extended. This makes illegal states unrepresentable (a
   slide is not a note mark; an accent is not a span). Genuinely dual techniques
   (vibrato, bend) get a canonical home documented in the glossary — a
   single-note span of length one rather than a second enum membership.

4. **Evidence on spans and positions (the honesty rule).** Each `TechniqueSpan`
   and `FretboardPosition` carries `TechniqueEvidence { source: TechniqueSource,
   confidence }`; per-note `NoteMark`s are evidence-free flags (see the
   amendment above):
   - `Explicit` — a source-of-truth format stored it (Guitar Pro); confidence 1.0.
   - `InferredFromMidi { cue }` — a heuristic from MIDI evidence (pitch-bend →
     bend/vibrato, dense repeats → tremolo); confidence `< 1.0`.

   This realises glossary §5/§17.3 and reuses **"evidence"** for *import-side
   provenance* exactly as ADR-0017 reserved it (evidence ≠ scoring rationale).

5. **`Articulation` survives only as a compatibility projection**, not as note
   storage — the ADR-0002 projection pattern. A `dominant()` projection over a
   note's marks (and overlapping spans) lets summary/view code and the corpus's
   coarse technique tags keep a single label while call sites migrate behind
   characterization tests. The rich marks+spans are the truth.

6. **Note identity for span endpoints (`NoteId`), phased.** Spans reference by
   `tick_range` today; a lightweight stable `NoteId` lets a span pin specific
   endpoints (a legato pair, which chord note bends) and survive region
   regeneration. Included in the target model, **deferred** in sequencing — phase
   1 keeps `tick_range`.

7. **Feeds scoring, does not build it.** Position makes the playability **gate**
   (a hard constraint that *rejects* unreachable spans, ADR-0017 §5) and the
   `fret_jump_penalty` **soft axis** (DP cost, ADR-0013) computable; the
   complement `technique_overlap` axis reads the multi-technique set instead of
   one `Option`. The model is a *supplier* to scoring/DP, never a consumer.

8. **Phasing — this ADR ships no code.**
   - **P1 (model + projection):** add `FretboardPosition`, `Tuning` per track,
     `marks` set on `AtomNote` (replacing `Option<Articulation>`),
     `TechniqueEvidence`, enriched `TechniqueSpan`; keep the `Articulation`
     projection. Importers/generator set empty marks / `None` position.
     Characterization-gated; re-bless the `artic=` goldens; **no** behavioural
     change beyond representation.
   - **P2 (populate):** `gp.rs` stops flattening — emits multiple techniques +
     string/fret + `Explicit` evidence; MIDI import emits inferred marks/spans
     with `InferredFromMidi` evidence + confidence, else absent (S3).
   - **P3 (consume):** playability gate + `fret_jump_penalty` + DP state use
     position; the generator/ComplementArranger emit techniques (S6/S7).

9. **Roadmap.** A data-model prerequisite for S7, as ADR-0014 was; **not**
   required for S13 v0 and must not block it. No new stage — it is cross-cutting
   canon (S3 GP import, S6 generation, S7 DP/playability, S13 complement, corpus).
   A corpus `schema_version` bump is deferred to when positions/techniques are
   persisted (as ADR-0015/0017).

## Consequences

- The note can carry what a guitar note carries: hand position plus co-occurring
  techniques with honest provenance. The Guitar Pro path gets **richer, not
  lossier** — the `gp.rs` "highest-priority only" flattening is designed out.
- `fret_jump_penalty`, position shifts, and the playability gate become
  expressible — the basis for human-plausible parts and the DP cost function.
- Inferred and source-of-truth techniques are distinguishable by evidence +
  confidence; nothing masquerades as fact.
- Cost: `AtomNote` changes shape (gains `position`, `marks`; loses
  `Option<Articulation>`); every constructor and the MIDI/GP/preview/complement
  call sites are touched, and the `artic=` / GP goldens re-bless — characterization-
  gated, phased, no big-bang.
- Accepted: position inference (pitch→string/fret) and `NoteId` endpoint pinning
  are genuine sub-problems, deferred to their phases, not solved here.
- Accepted: a corpus-schema bump is needed when the richer note is persisted;
  deferred until then.
- Accepted: proven only when P2 actually populates the model from Guitar Pro;
  P1 is pure representation and must reshape to the new model and nothing else.
