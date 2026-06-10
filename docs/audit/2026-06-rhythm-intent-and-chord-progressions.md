# Rhythm intent & chord progressions — discussion synthesis (2026-06)

Status: design note (not an ADR). Captures the outcome of an external design
discussion (hard-constraint JSON/DSL vs soft "vibe" scoring, pattern cells,
chord-per-bar specs) evaluated against the canon, plus the two concrete user
needs it surfaced:

1. **Rhythm intent** — "I place the durations and rests; the engine picks the
   pitches" — without leaving griff for Guitar Pro.
2. **Chord-progression spec** — "Am(add9) | Fmaj7 | C | G, one per bar" as a
   first-class generation input.

Like the other 2026-06 notes, this is a **backlog and a map**, not a ratified
decision. Items marked *→ candidate ADR* graduate into a real ADR before code.

## 0. One-line conclusion

The external discussion independently re-derived ADR-0017 (hard gates vs soft
axes, weights as data, explainable scoring, feature-vector "vibe" before
embeddings) — good external validation, no new architecture. Its genuinely new
deliverables for us are two **input surfaces**, both of which compile into the
*existing* S6 pipeline: a user-authored rhythm as a hard constraint, and a
chord progression as harmonic context. Neither needs references, audio, or a
neural layer; both are symbolic arithmetic plus canon we already have.

## 1. Canon map (what the discussion re-derived — do NOT re-open)

| Discussion point | Already ratified in |
| --- | --- |
| Hard constraints (validity) separate from soft style (ranking); never merged | ADR-0017 §5 |
| Weighted scorer (`0.3·playability + …`) with user-tunable weights | ADR-0013 §4, ADR-0017 §3, S9 |
| "More atmospheric" buttons = weight/preset changes, not regeneration logic | ADR-0017, design note 2026-06 §2.3 (presets) |
| Feature-vector "vibe" (rest ratio, density, register…) before neural embeddings | ADR-0017 §4 (measurability gate), 2026-06 note §2.2 |
| Generate many → gate → rank → top-N | S6 candidate sets + playability filter + rerank |
| Explainability ("the embedding decided" is not an architecture) | ADR-0017 rationale; candidate passport (2026-06 note §2.1) |
| Validation as its own layer with structured errors | playability filter, pair validator, loss reports, fuzz gates |
| Typed internal model, not strings | canonical score model (SPEC §4) |
| Real (neural) embeddings later, gated | S12 hard gate; "style as corpus region" parked behind S5 |

Reaffirmed, not re-opened: the rhythm/pattern DSL stays a **low-level escape
hatch**, not the primary intent surface (2026-06 expressive-control note §3 —
primary is point-at-example, §2.4). §2.1 below is precisely the escape-hatch
use-case: the user *is* thinking in grid terms and wants to dictate the grid.

## 2. Delta we commit to

### 2.1 Rhythm intent: user-authored rhythm, engine-solved pitches

The need: author a simple rhythmic figure (durations + rests, per bar) inside
griff and have the generator fill in pitches — without round-tripping through
Guitar Pro, and without griff growing a tab editor.

This is **not a new generator**. S6's first strategy is
`RhythmCopyPitchSubstitute` (rhythm from `source_rhythms`, pitches substituted
within `PitchMaterial`), and S13 `rhythm_lock` already feeds it part A's onset
grid. The delta is only an **input surface**: compile a user-authored rhythm
into `source_rhythms` instead of corpus material.

- **Contract.** The user rhythm is a **hard gate** (ADR-0017 §5): candidates
  must realise it exactly; soft axes rank only the pitch choices. Determinism
  holds trivially (fixed rhythm + fixed seed).
- **Cheapest surface — pattern string** (the escape hatch): one line per bar
  over a declared grid, e.g. grid `1/16`, `x..- x.-- x..- x...` where `x` =
  onset, `.` = sustain (extends the previous event), `-` = rest. This covers
  durations, not just onsets. A natural extension carries `m` for a muted
  onset, dovetailing with mute-aware `RhythmCell` identity (tab-research note
  §2.6).
- **Second surface — preview step-grid**: a one-row step-sequencer in the
  ratatui preview (toggle cells per grid step). The interaction core
  (ADR-0016 intents) already exists; this is a drum-machine row, not a Guitar
  Pro clone.
- **Already works today**: importing a monotone MIDI rhythm track and
  substituting pitches — but it requires another program, which is the
  irritation this item removes.
- **Lands at**: S6 request surface (a `RhythmIntent` → `source_rhythms`
  compiler), CLI/file input for the pattern string; preview grid later.
  Effort: S (string parser + plumbing). *→ candidate ADR* only if the pattern
  syntax grows beyond onset/sustain/rest/mute.

### 2.2 Chord progressions as a generation input *→ candidate ADR*

The need: "Am(add9) | Fmaj7 | C | G, one chord per bar" as input; the engine
voices and rhythmicises it (or constrains pitch choice by it).

**Identity is arithmetic, not references.** In the symbolic 12-TET model a
chord symbol is a root pitch class (0–11) plus a quality recipe — a set of
semitone offsets from the root: maj `{0,4,7}`, min `{0,3,7}`, maj7
`{0,4,7,11}`, add9 `{0,4,7,14}`, sus2 `{0,2,7}`, … plus an optional slash
bass. `Am(add9)` = root 9, recipe `{0,3,7,14}`. Keys and scales are the same
shape (tonic + interval set), which is exactly what the glossary's
`PitchMaterial` already postulates ("scale, mode, pitch set, anchor notes,
allowed intervals"). Frequency is irrelevant: griff is symbolic end-to-end
(SPEC "not an audio synthesizer"); Hz exists only at synthesis time, outside
our boundary.

Where arithmetic stops and other layers take over:

1. **Voicing** (symbol → concrete fretted notes: octave, inversion, dropped
   notes, strings/frets) is a fretboard-model constraint problem — the
   already-deferred ADR-0019 chord phase, with its cost-term inventory
   pre-collected (tab-research note §2.4). First slice: a small curated
   swancore shape vocabulary (ADR-0005 chord vocabulary — add9/sus2/maj7
   shapes); solver-based voicing later.
2. **Taste** (which extensions/voicings sound swancore) is corpus territory
   (S5 tags, S9 preference) — the style *region*, not the chord *definition*.
   Math gives correctness; the corpus gives taste. No external references are
   required for the spec itself.
3. **Harmonic function** needs no theory engine: a progression supplies the
   harmonic context that existing mechanisms already want — the pair
   validator's coincident-onset checks, the DP `harmonic_fit` cost term
   (ADR-0013), and S13's open item "richer harmonic context (key/scale fit)
   for `PartProfile`".

- **Shape.** A progression is per-span harmonic context on the master
  timeline (ADR-0003): `Vec<(TickRange, ChordSymbol)>`, default one per
  `MasterBar`. Consumers: (a) compile to `PitchMaterial` for S6 ("pitches
  from the current chord ± approach tones"), (b) harmonic context for the
  pair validator / DP, (c) later, voiced chord events via the ADR-0019 chord
  phase. Combined with §2.1 this reconstructs the discussion's
  `progression + pattern` spec entirely from existing parts.
- **Prior art** (per AGENTS.md prior-art-first, to record in the ADR): Harte
  et al. chord syntax (ISMIR 2005, the MIR-standard symbol grammar), music21's
  `harmony` module, the `rust-music-theory` crate — idea references; parsing
  is small enough to implement natively under the lean-tree posture.
- **Lands at**: new `ChordSymbol` / progression types + parser (red→green),
  `PitchMaterial` compiler in S6, harmonic context in S13. Voicing stays
  deferred. Effort: M. Risk: low (pure symbolic layer).

### 2.3 Performance-constraint input spec (smaller, backlog)

The discussion's `performance_constraints` block (articulation default,
picking mode, dynamics policy, allowed durations) maps onto vocabulary that
already exists — glossary §5/§6 techniques, ADR-0018 `TechniqueSpan` /
`NoteMark`, S6 `Constraint` ("allowed techniques") — but is not yet organised
as a *generation input* policy. Record as backlog: a `PerformancePolicy` on
the generation request (default articulation, picking, dynamics), compiled
into constraints + emitted marks. Wait for §2.1/§2.2 to land first; alone it
has no consumer.

## 3. Rejected / parked from the external discussion (with reasons)

| Item | Verdict |
| --- | --- |
| LLM structured planner as a pipeline stage | Parked (2026-06 note §4: "text → axes, boundary-only, lowest priority"). The engine, not an LLM, makes final decisions — which the discussion itself endorses. |
| Audio reference embeddings (CLAP/MERT, rendered-candidate similarity) | Out of scope by SPEC (no audio). Symbolic embeddings are S12-gated. |
| `vibe_similarity` as a scorer axis | Violates the measurability gate (ADR-0017 §4) as stated; a "vibe" must decompose into measurable axes (then it is a preset/region, not an axis). |
| Pattern DSL as the *primary* user interface | Stays rejected (2026-06 note §3); §2.1 uses it strictly as the escape hatch it was kept for. |
| Per-bar YAML event lists (`bars: [{bar: 1, events: […]}]`) | Subsumed: rhythm cells + progression (§2.1/§2.2) express the same content without a per-event authoring format. |

## 4. Suggested sequencing (not a commitment)

1. §2.1 pattern-string `RhythmIntent` → `source_rhythms` (smallest, immediate
   user value, no ADR needed at the onset/sustain/rest/mute level).
2. §2.2 `ChordSymbol` + progression ADR (parser + `PitchMaterial` compiler
   first; voicing stays with ADR-0019's deferred chord phase).
3. Preview step-grid entry for §2.1 (after the request surface exists).
4. §2.3 `PerformancePolicy` once §2.1/§2.2 give it a consumer.

## 5. One-line thesis

Let the user dictate exactly what is mechanical (a rhythm grid, a chord
progression — both pure symbolic arithmetic) as hard constraints into the
existing generator, and keep everything aesthetic in the scoring layer where
ADR-0017 already put it; no tab editor, no references, no new engine.
