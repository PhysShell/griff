# ADR 0014: Fretboard-aware canonical model (string/fret positions)

Date: 2026-05-31
Status: Superseded by ADR-0018

> Superseded by [ADR-0018](0018-rich-note-model-fretboard-and-techniques.md),
> which carries this string/fret decision forward and merges it with the
> multi-technique + evidence model, because both live on the same note/group and
> must migrate together (one core-model change, one golden re-bless).

## Context

The canonical `AtomNote` carries only `pitch` (a MIDI number). The glossary
already defines `String`, `Fret`, and `Tuning` (§4) and notes that one pitch is
playable on several strings — but these live as *source metadata*, not in the
working model. Feature extraction, generation, and playability therefore reason
about pitch only.

That is a real gap for a guitar engine. What separates a guitar part from "MIDI
that happens to be in range" is *where the hand is*: position shifts, string
choice, fret distance, reachable spans. Fret-awareness is what humanises the
instrument — "don't jump from fret 3 to fret 17 without reason" cannot even be
expressed without string/fret on the note.

This becomes load-bearing at S7: the DP/Viterbi cost function (ADR-0013)
includes a `fret_jump_penalty` and the DP state includes fretboard position.
Neither is expressible while a note is just a pitch.

## Decision

We make the canonical model fretboard-aware.

1. **Note position.** `AtomNote` gains an optional fretboard position
   (`string` + `fret`), under the declared `Tuning` of the score/track. It is
   optional because MIDI import often cannot recover it (glossary §17.3); Guitar
   Pro import populates it directly.

2. **Tuning on the score.** The score/track carries its `Tuning` (default
   Standard E, ADR-0006), the reference needed to map pitch ↔ (string, fret).

3. **Position inference is explicit and lossy.** When position is absent (MIDI
   source), a separate, documented inference step may assign a plausible
   (string, fret) under playability rules; it never fabricates certainty —
   inferred positions are marked as such, consistent with the source-of-truth
   vs inferred-articulation distinction (glossary §5).

4. **Playability uses position, not pitch intervals.** The playability
   filter/score and the DP `fret_jump_penalty` operate on fretboard distance and
   reachable spans, not on semitone deltas.

5. **Staging.** Introduced as the data-model prerequisite for S7. It is **not**
   required for S13 v0 (single-bar pitch-level retrieval) and must not block it.
   Delivered alongside S7, gated by characterization tests so existing
   pitch-only behaviour does not change silently (SPEC §5).

## Consequences

- The engine can reason about hand position: string choice, position shifts,
  fret distance — the basis for human-plausible guitar parts and the DP
  `fret_jump_penalty`.
- Guitar Pro imports preserve their native string/fret instead of flattening to
  pitch; the GP→model path gets richer, not lossier.
- Accepted: `AtomNote` grows a field; all constructors and the MIDI/GP adapters
  must set it (MIDI → `None`/inferred, GP → actual). Characterization tests pin
  that pitch-only behaviour is unchanged until inference is enabled.
- Accepted: pitch↔fret is one-to-many; position inference is a genuine
  sub-problem (a small search/DP of its own) and is deferred to its
  implementation, not solved in this ADR.
- Accepted: this is a prerequisite for S7 and adds scope there; S13 v0 stays
  pitch-only and ships independently.
