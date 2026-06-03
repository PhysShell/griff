# Expressive control & explainable scoring — discussion synthesis (2026-06)

Status: design note (not an ADR). Captures the outcome of a design discussion on
(1) an explainable, multi-axis scoring/provenance layer and (2) how to formalise
"the sound I want" (swancore and guitar idiom) for the generator.

This note is a **backlog and a map**, not a ratified decision. Its job is to
record what we commit to, what we rejected and why, and what is promising but
unproven — and, crucially, to show how much of it is **already ratified** in
ADR-0012…0015 so we do not re-litigate settled ground. Items marked
*→ ADR-00xx* are candidates to graduate into a real ADR before code.

## 0. Where this came from

Three external references were evaluated as mental models (not dependencies):
neuro-symbolic reasoning (Scallop), learning-to-rank relevance (Kaggle
CrowdFlower), and "algorithmic empathy / human fallibility". Independently, all
three pointed at the same missing shape:

> deterministic generator → **multi-axis, explainable scoring with provenance**
> → ranking → (later) learning from feedback.

That shape is largely **already in the canon**. The value below is the *delta*
the discussion added on top of it, plus an honest reject/park split.

## 1. Already decided (canon map — do NOT re-open)

| Discussion point | Already ratified in |
| --- | --- |
| Orthogonal feature/complexity **vector**, never a single scalar | ADR-0015 (`ComplexityProfile`) |
| spec / fact / provenance split for relations & structure | ADR-0012 §1, ADR-0015 §1–3 |
| Multi-axis relation between parts (rhythm/harmony/register/technique/density/contour) | ADR-0012, glossary §9 |
| "measure before target" (metrics land before they become budgets) | ADR-0015 §8 |
| Weights are **data**, not hardcoded; S9 tunes, DP/rerank consume | ADR-0013 §4, S9 |
| Deterministic selection with a fixed, documented tie-break | ADR-0013 §3, SPEC §6 |
| Part profile = per-part features derived from material (A) | ADR-0012 §2, glossary "Part profile" |
| Fretboard model (string/fret, playability, `fret_jump_penalty`) | ADR-0014 |
| Technique vocabulary (tapping, palm mute, slide, bend, arpeggio, strum) | glossary §6 |
| Region/selection as `TickRange`; region regeneration | S11, glossary §8 |
| No neural layer before a rule-based baseline | ADR-0005/0008, glossary §17.5; S12 deferred |
| Explainable rerank by features/tags; like/dislike → preference profile | S9, glossary §10 |

Takeaway: the architecture already *is* "neuro-symbolic without the neuro". The
discussion did not discover a new layer so much as a **unification** of layers
that currently each describe scoring/provenance on their own.

## 2. To-do — the delta we commit to

These are additive and do **not** touch the canonical model (ADR-0011) or the S6
strategies. Each says where it lives and what it extends.

### 2.1 Unify scoring + provenance into one cross-cutting representation → ADR-0017

Today three subsystems describe per-axis scores/provenance independently:
ComplementArranger relation scores (ADR-0012), `StructureMetrics` (ADR-0015), and
the DP cost terms (ADR-0013) — plus the flat `Quality score` glossary entry.
They are the same pattern. Commit to a **single representation**:

- A `Scored<T>` envelope: `value`, the per-axis scores (**facts**), the total
  under a named weight policy, and a list of per-axis **rationale** entries.
- **Axes (facts) are separate from weights (policy).** Reaffirms ADR-0015's
  anti-scalar stance and ADR-0013's weights-as-data; makes it one type, not
  three. The flat `Quality score` becomes an explicit axis vector + policy.
- **Hard vs soft constraints are distinct:** hard = validity/playability gates
  that *reject* (`Playability filter`, `Pair validator`); soft = axes that
  *rank*. Never merged into one function.
- **Determinism under feedback:** a feedback-tuned ranking must stay
  reproducible. Pin/version the weight policy so `(seed, weight_policy_id)`
  fully determines the order (extends ADR-0013 §3 to the tuned case).
- **Renders in the preview inspector** — the per-candidate "passport" (why
  generated, which mode, which axis scores, which violations, why ranked).
  The inspector substrate already exists from the S8 scene slice (ADR-0016);
  provenance is renderer-local text, exactly where the scene boundary put it.

Naming constraint: **`Evidence` is already taken** in the glossary (MIDI /
inferred-articulation confidence). The scoring concept needs a distinct name —
proposed `Rationale` / `ScoreRationale` — to avoid collision.

### 2.2 Bidirectional axis invariant (anti-snake-oil gate)

Adopt as an explicit rule (sharper than ADR-0015's "measure before target"):

> Every axis that can be **targeted** in a request must be **measurable** from
> material, and vice versa. If an axis cannot be measured on a real phrase, it
> cannot be a generation target.

This is the falsifiability gate that keeps affect/style honest: "nervous" is not
an axis (unmeasurable) — it is a *preset* over measurable axes (syncopation,
delayed-resolution, weak-beat accent bias).

### 2.3 Affect / style / gesture = named presets (regions) over the same axes

Generalise the existing `Relation mode` pattern ("a named complementarity preset
over axes", glossary §9) to all three:

- A **style** (e.g. swancore) is a *region* — centroid + spread — over the
  idiom axes, not a label. Labels (`riff/solo/clean/breakdown`) overlap because
  they collapse orthogonal axes into one word; regions keep them separate.
- An **affect** ("nervous", "bright", "urgent") is a named bundle of target
  ranges + soft-constraint weights. No standalone "EmotionEngine".
- A **gesture** ("tapped cascade", "перебор on the top strings", "palm-muted
  gallop") is a named bundle of idiom-axis values + fretboard constraints.

All three reuse 2.1's scoring and 2.4's extractor; none is a new engine.

### 2.4 One profile-extractor primitive (material → axes), four consumers

`Part profile` (ADR-0012) and `StructureMetrics` (ADR-0015) already extract axes
from material. Commit to recognising these as **one reusable primitive** — the
inverse of generation — with four consumers:

1. Part A profile (ComplementArranger).
2. `StructureMetrics` over any span.
3. The soft-scorer's feature extractor (2.1).
4. **Input as intent**: a profile extracted from a user-**selected region** (or
   a reference phrase) *is* a generation intent. "Formalisation by example."

Consumer 4 is the new synthesis and the cheapest expressivity win: it sidesteps
the formalisation problem entirely (point at material instead of describing it),
and it reuses the S8 selection substrate (ADR-0016) and S11 region machinery.
"Develop my idea" = extract the seed profile, then constrained walk within the
region (2.3).

### 2.5 Idiom-axis vocabulary (the "missing middle") — glossary expansion

Between the coarse labels and the note-level rhythm DSL sits the layer that
actually carries guitar idiom. Add an **orthogonal** axis vocabulary; much of the
technique terminology already exists (glossary §6) but is not organised as axes:

- **Texture** — monophonic line / arpeggio (перебор) / strummed chordal (бой) /
  dyad / power-chord block / tapped.
- **Right-hand actuation** — alternate / economy / sweep / tapping / fingerstyle
  / hybrid. (перебор vs бой live here.)
- **Left-hand articulation** — legato (hammer/pull) / staccato / palm-mute
  (глушение) / slide / bend / vibrato.
- **Fretboard locus** — position, string-set (low/mid/high), string-skip vs
  adjacent. (Needs ADR-0014.)
- **Contour** — ascending run / descending cascade / zigzag / pedal / ostinato.
- **Harmonic device** — pentatonic / diatonic / modal (lydian brightness) /
  chromatic passing / arpeggiated / extended chords.
- **Rhythmic device** — straight / swung / gallop / syncopation-displacement /
  ghost notes / anticipation (push).

Worked example to keep us honest: a **swancore clean lead** decomposes as high
locus + legato/tapping + arpeggio/monophonic texture + syncopated funk-16th +
lydian-bright-plus-chromatic harmony + wide cascading contour. A "качёвый лик"
is *not* "16ths with rests" (a symptom) but *syncopation + ghost notes +
anticipation + medium density* (the cause).

## 3. Rejected (with reason)

| Rejected | Reason |
| --- | --- |
| **Scallop / external rule-engine as a dependency** | Python/PyTorch/Datalog ecosystem; wrong fit for a Rust core targeting CLAP/realtime. It re-implements what ADR-0013/0015 already do natively. Keep a small "Scallop-at-home": typed facts + rules in Rust. |
| **Neural / transformer-first generation now** | Violates the no-neural-before-baseline rule (canon). S12 stays deferred until corpus + rule baseline exist. |
| **A single `quality: f64` as the primary control** | Already designed out by ADR-0015 (length ≠ period ≠ complexity); reaffirmed. Conflates orthogonal axes. |
| **Ensemble of N independent scorers (CrowdFlower style)** | Kaggle leaderboard tactic; premature with no data. One rule-based scorer that grows by axis. |
| **Rhythm DSL (`meter/max_sixteenth/tuplet`) as the *primary* user interface** | Too low-level; describes any music, not idiom, and not how a human expresses intent. Keep it only as a low-level escape hatch, not the main surface (region-select/example is primary — 2.4). |
| **A standalone "EmotionEngine" module** | Collapses into presets-over-axes (2.3); a separate "theory of emotion" module is where snake-oil lives. |
| **A trained relevance reranker now** | Design the `Scorer` trait seam (rule-based today, trained later), but build no ML until corpus + labels exist (S12). The seam is cheap; the model is gated by data. |

## 4. Parking lot — uncertain but promising

Not rejected, not committed. Each notes what would unblock it.

- **Affect → axes mapping** (valence/arousal, tension arcs → target ranges).
  Strong differentiator; unfalsifiable without listening tests / corpus. Park
  behind 2.3 presets + blind-listening validation.
- **Human-fallibility as a composition parameter** (hesitation, anticipation,
  delayed resolution, ghost-note bias) — "humanize as intent, not random
  jitter". Concretely promising; must be expressed as *deterministic*
  axis-targets/constraints, never RNG. Park as a future axis cluster on 2.1.
- **A general unified `Intent` object** (target region + constraints + gesture
  priors) generalising `ComplementSpec` + `StructureControl` + affect presets.
  Possibly premature generalisation; revisit after 2.1/ADR-0017 lands. (Note:
  `Intent` is already used in the UI core, ADR-0016 — a different concept; a
  generation-intent type would need a non-colliding name.)
- **Style as a corpus-derived region** (centroid + spread over idiom axes).
  Needs the S5 corpus; park until it exists.
- **Text → axes natural-language parsing** (possibly LLM-assisted at the
  boundary, since the target is just numbers/enums). Lowest priority, most
  mutable; park.
- **Disagreement as a second-order signal** (model-vs-human divergence as a
  learning signal, beyond S9's EMA). Interesting; park with S9/S12.
- **Ordinal feedback labels** (👎 / meh / 👍 / ⭐ → ordinal target) feeding a
  future trained reranker. Cheap to *capture* early even while tuning is
  deferred; worth wiring into S9's logging.

## 5. Suggested next steps (sequencing, not a commitment)

1. **ADR-0017** "Explainable candidate scoring (axes + rationale + ranking
   policy)" — ratifies 2.1/2.2, unifying ADR-0012/0013/0015 provenance and
   resolving the `Evidence` naming collision. Cross-cutting, so ADR before code.
2. Narrow slice: a `score` module — generalise the existing complement
   `AxisScores`/`PairValidation` onto the unified `Scored<T>` + `Rationale`,
   axes-vs-weights split, stable tie-break, versioned weight policy. Red→green;
   no golden changes (pure extension).
3. Wire the candidate "passport" into the preview inspector (substrate exists
   from the S8 scene slice).
4. Glossary expansion for the idiom-axis vocabulary (2.5); map the Russian
   practice terms (перебор/бой/глушение) onto axes.
5. Start **capturing** feedback labels in S9 (even just logging) to grow the
   dataset that gates everything trained — independent of building any model.

Affect (4-park) and the trained reranker (3-reject/seam-only) stay out until
there are axes, rationale, and at least some corpus.

## 6. One-line thesis

You do not formalise swancore by describing it — you formalise the **axes** so
swancore becomes a measurable *region*, let the user **point at examples**
(profile extraction), and give every candidate an **explainable passport** over
those axes. Affect and gesture are presets over the same axes; the fretboard
model (ADR-0014) makes idiom and playability computable; nothing in the
canonical core changes.
