# ADR 0019: Infer fretboard position with a small local DP

Date: 2026-06-04
Status: Proposed

## Context

ADR-0018 made the note fretboard-aware — `position: Option<FretboardPosition>`
under a per-`Track` `Tuning` — and Guitar Pro fills it directly (`Explicit`).
But MIDI import leaves it `None`: the model can *hold* a position, yet nothing
*produces* one for MIDI-sourced material. Without position, `fret_jump_penalty`
(ADR-0013), the playability gate (ADR-0017 §5), and human-plausible fretting
cannot apply to MIDI parts at all.

Pitch ↔ (string, fret) is one-to-many, so assigning positions is a real problem —
the classic **guitar fingering problem**: give each pitch a playable
`(string, fret)` that minimises hand movement between consecutive notes. It is
well-studied, and the open-source prior art converges on one shape — a
shortest-path / DP / Viterbi over fingering states with a movement/stretch cost:
[`jgollub1/guitar_dp`](https://github.com/jgollub1/guitar_dp) (MIT, DP),
[`natecdr/tuttut`](https://github.com/natecdr/tuttut) (MIT, HMM + Viterbi),
[`noahbaculi/guitar-tab-generator`](https://github.com/noahbaculi/guitar-tab-generator)
(GPL-3.0, Dijkstra).

Two scoping facts shape the decision:

- `docs/decisions.log.md` (2026-06-04) parks MIDI *technique* inference
  (virtual-instrument-specific keyswitches) but keeps **position** inference as a
  *distinct, VI-independent fretboard-geometry* problem. This ADR picks it up.
- It does **not** need the S7 graph layer or its Viterbi (ADR-0013): it is a
  *small local* DP over one voice's per-note candidates, the same "small local DP
  is fine, separate from graph traversal" carve-out ADR-0015 §7 already made.

## Decision

We infer fretboard position with a small, local, deterministic DP. We reuse the
*idea* from the MIT-licensed `guitar_dp` / `tuttut`; griff reimplements it
natively (no dependency, per its dep posture), and the GPL `guitar-tab-generator`
is reference-only — griff is MIT.

1. **Candidate mapper.** For each note pitch, enumerate the playable
   `(string, fret)` candidates under the track's `Tuning` — every string whose
   open pitch ≤ target with `fret = target − open` in `[0, max_fret]`. A pitch
   with no candidate is *out of range*: reported as a loss, never fabricated.

2. **DP over candidates.** State is the chosen candidate for the current note
   plus the minimal carried context (the previous hand position). The transition
   cost between consecutive notes is an explicit, inspectable sum of penalties:
   `position_shift + stretch + string_change − open_string_bias`. The minimum-cost
   path is the inferred fingering. Deterministic by construction; ties break by a
   fixed documented rule (e.g. lowest string, then lowest fret) — SPEC §6, no RNG.

3. **Hard gate vs soft cost** (ADR-0017 §5). Out-of-range notes and impossible
   simultaneity (a chord needing one string twice) are *hard rejects*, not soft
   penalties; the cost terms only **rank** among playable candidates.

4. **Weights are data** (ADR-0017 §3). The penalty weights are a named, versioned
   policy, not hardcoded, so they can be tuned later — and the academic
   "path-difference learning" (fitting DP cost weights to real tablatures) is the
   natural future tuner, the same weight surface as S7/S9.

5. **Inferred positions are evidence-marked.** An inferred position is tagged
   `InferredFromMidi` with a confidence (e.g. from the margin between the best and
   runner-up path), so it never masquerades as a source-of-truth `Explicit` GP
   position (glossary §5, ADR-0018). This is the **first real producer** of the
   `InferredFromMidi` path — and it is VI-independent geometry, honouring the
   decisions-log split (technique inference parked, position inference kept). It
   un-defers position evidence from ADR-0018 Slice 2b (which deferred it because
   every position was then a constant `Explicit`).

6. **Placement & supplier role.** A position-inference pass over a `Voice`/`Track`
   in the import/analysis layer, run on MIDI-sourced material; GP keeps its
   `Explicit` positions untouched. It is a *supplier* to playability /
   `fret_jump_penalty` / DP (ADR-0013/0017), never a consumer.

7. **Phasing.** Monophonic `(string, fret)` inference first. Deferred follow-ups:
   chord voicing across strings, **finger assignment** (the 4-finger left-hand
   layer `guitar_dp` / `tuttut` model — stretch is really finger span), and
   `timbre_zone` (neck region for tone). The first pass produces position only.

## Consequences

- MIDI-sourced parts gain plausible fretting, so `fret_jump_penalty`, position
  shifts, and the playability gate finally apply to them — not only GP imports.
- The `InferredFromMidi` evidence path gets its first real producer, and it is
  VI-independent geometry — consistent with the parked MIDI-technique inference.
- The cost-weight surface is the same data shape as ADR-0013/0017, so it unifies
  with their tuning later (including learning weights from real tabs).
- It validates the independently-sketched generation pipeline (phrase → rhythm
  constraint → candidate mapper → playability solver → style scorer → export):
  this ADR is the "candidate mapper + playability solver" stage, the one piece
  griff lacked; the rest already map to S6 / S14 / ADR-0013 / ADR-0017 / export.
- Accepted: position evidence un-defers — if carried on `FretboardPosition`, the
  `f64` confidence drops `Eq` (not `PartialEq`) from `FretboardPosition` and the
  `AtomNote`/`AtomEvent` that hold it, the same trade-off as the span chain in
  ADR-0018 Slice 2b. None is used as a hash key. (Carrying the marker beside the
  position instead is the alternative, settled at implementation.)
- Accepted: finger assignment, chord cross-string voicing, and `timbre_zone` are
  deferred; the first pass is monophonic.
- Accepted: inference is heuristic — a *plausible* fingering, not ground truth;
  the evidence/confidence makes that explicit and the weights are tunable.
- Accepted: prior-art reuse is idea-level for the GPL `guitar-tab-generator`; the
  MIT `guitar_dp` / `tuttut` may be ported, but griff reimplements natively with
  no added dependency.
