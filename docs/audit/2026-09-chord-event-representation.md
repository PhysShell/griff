# Chord-event semantic state — preregistered exact representation experiment

Date: 2026-09-22
Status: preregistered before the general-corpus outcome

## Question and population

This Lab-only experiment asks which semantic inputs reduce exact chord-fingering
ambiguity beyond a chord's pitches alone: the preceding fretting-hand anchor,
incoming imported technique relations, or both.  The population is every
Guitar Pro onset with at least two note atoms in the unchanged 410-file corpus.
An onset is keyed once by source, track, voice, onset and stable atom identity;
`TabLine` fragments are not the sampling unit.

Every onset remains in the census and is classified as `CompleteExplicit`,
`IncompletePosition`, `PitchMismatch`, `DuplicateExplicitString`,
`BeyondMaxFret`, or `OtherUnsupported`.  Only `CompleteExplicit` enters primary
agreement evaluation.  Refused observations are counts, never silently dropped
or repaired.

The 38 relations from #209/#210 are a pinned compatibility cohort, not the
population being optimized.  The runner must fail closed unless their 176 legal
target-string conditions, 174 feasible conditions, B0 ranks `11/24/3`, O rank-1
count 36 and A rank-1 count 30 are reproduced.

## Input/evaluation separation

Solving receives a `ChordEventProblem`: identity, tuning, maximum fret, atoms,
optional preceding anchor and zero or more incoming techniques.  Imported
target-chord positions live only in a separate `ObservedChordVoicing` passed to
explicit evaluation functions.  The problem type has no field through which a
solver can read the target answer.  An incoming relation may carry its earlier
origin position and stable target atom id, but never the target's imported
position.

Stable atom ids, rather than pitch, define both constraints and agreement.
Duplicate pitches therefore remain distinct observations.

## Registered context and regimes

Anchor extraction reuses #199/#210 verbatim: events are strictly earlier than
the chord; tapped and open notes do not establish/move the fretting hand; the
latest qualifying onset wins; its lowest qualifying fret is the anchor; absence
is `None`.

Incoming technique projection reuses corrected #207 semantics: the first
strictly later note in the imported voice on the origin's original string.
Zero or many relations may target a chord. Conflicting requirements on one atom
are a typed infeasible/forensic result.

The four information identities are:

- **R0 chord-only:** pitch-correct candidates, maximum fret and one simultaneous
  atom per physical string.
- **R1 chord+anchor:** R0 feasible set; preference primitive
  `A = sum(abs(fretted_atom.fret - anchor_fret))`, with open atoms contributing
  zero.
- **R2 chord+incoming-technique:** R0 plus hard stable-atom string conditions
  from incoming same-string relations.
- **R3 chord+anchor+incoming-technique:** R2 feasible set plus A.

Hard feasibility and soft/reference ranking stay separate (ADR-0019).  These
regimes are explicit information identities and are compared only through the
same evaluation metrics (ADR-0034).  Constraint-inventory rule 2 remains
`defer`; these are Lab regimes, not production rules.

No scalar `B0 + alpha*A` is fitted.  Frozen #209 unary B0 is reported for R0
and R2 as a reference metric.  A-minimum sets define R1 and R3.  `B0 -> A` and
`A -> B0` may be emitted only as symmetric sensitivity views.

## Exact sets and metrics

Enumeration is exhaustive over the finite candidate product with occupied-
string pruning.  Each regime records admissible assignment count, a saturation
flag and descriptive log count.  B0 and A views record optimum value, optimum-
set count and deterministic first assignment.

For every relevant exact set, evaluation against the imported voicing records:
whole-assignment membership and stable-atom position agreement floor, uniform
expectation, deterministic choice and ceiling.  The imported voicing is one
observation, not unique ground truth.  For R2, constrained-target agreement is
mechanical and cannot validate the technique input; non-target agreement is
reported separately.

Primary comparisons are R0→R1 on anchored chords, R0→R2 on technique-bearing
chords, and R2→R3 where both channels exist.  Reports are stratified by complete
status, anchor/technique availability, chord size, GP3–5/GP6–7 and target tap
state.  Song-key summaries and leave-one-song-out ranges diagnose
concentration; there is no random chord split.

## Registered anchor negative control

Within each song, eligible anchored chord events are sorted by stable event
identity and their anchors are cyclically rotated by one event.  A cohort with
fewer than two eligible events has no control.  No event may receive its own
anchor.  True and rotated anchors are compared using identical A-optimum human
membership, uniform agreement and human A excess.  The rotation is fixed here
and will not be selected after seeing outcomes.

## Technique control domain

For each incoming target, every legal target string is conditioned in turn and
records feasibility, assignment count, B0 optimum and A optimum when available.
The observed origin string is compared with the complete domain.  Its own
same-string target consistency is not independent validation.

## Evidence rules

Anchor is a candidate semantic field only if true anchors improve human-set
metrics more often than they worsen, the sign spans multiple song keys and all
leave-one-song-out removals, true anchors outperform the rotated within-song
control, and the effect is not confined to the legacy 38 cases.  Failure of the
control forbids a local-context claim.

Incoming technique is supported only if it preserves imported whole-chord
feasibility in nearly all valid cases, materially reduces ambiguity across
multiple songs, and conflicts/infeasibility receive typed forensic explanations
rather than repairs.  No post-result percentage threshold is introduced.

Combined state is supported only when R3 adds measurable information beyond R2
in multiple songs.  Otherwise the more minimal representation wins.

## Scope and invariance

This changes no production generator, MIDI fingering, importer, `TabLine`
slicing, Stage 2 objective/weights, `TechniqueEdge`, public chord model,
hard-rule layer, experiment production variants, CLI/cockpit, ADR status or
constraint-inventory classification.  It adds no learned weights, neural model,
MiniZinc or runtime solver.

The final run must retain 29,758 within-line and 46 cross-line relations and
Stage 2 B/C1/D1 `0.9/13.3/16.8%` with gaps `19.1/7.6/4.2` points.  Any drift is
a blocker until explained.

## Corpus result

The registered runner was executed over the unchanged 410-file corpus
(fingerprint `9e53e55a19cddf29`; 3 import failures; 1,149 selected guitar
tracks).  Full licensed-corpus records remain in the disposable output
directory.  The legacy compatibility command ran first and the general runner
refused to proceed until its summary passed.

### Population and typed refusals

The census contains exactly 289,130 chord onsets, independently reproducing
the existing `CutStats.chord_onsets` count:

| status | onsets |
|---|---:|
| `CompleteExplicit` | 281,520 |
| `IncompletePosition` | 134 |
| `PitchMismatch` | 0 |
| `DuplicateExplicitString` | 6,166 |
| `BeyondMaxFret` | 1,310 |
| `OtherUnsupported` | 0 |

No refusal was normalized into the primary population.  Complete observations
span chord sizes 2 / 3 / 4 / 5+ as `98,558 / 124,662 / 40,676 / 17,624` and
GP3–5 / GP6–7 as `183,917 / 97,603`.

Coverage inside the 281,520 primary observations is:

```text
preceding anchor       280,799
incoming technique       1,020
both                     1,020
```

### R0 — chord only and frozen B0

Every complete imported voicing is assignment-feasible (`281,520/281,520`).
R0's admissible-set size has median 16; the largest exact set has 336
assignments.  Frozen unary B0 has median optimum-set size 1 and contains the
imported whole chord in 87,635 cases (31.1%).  This is the general-population
version of #209's conclusion: pitch/string feasibility is broad, while the
context-free unary surrogate explains only a minority of imported voicings.

### R1 — anchor and its registered negative control

Against the R0 B0-optimum set on 280,799 anchored chords, the true anchor's
A-minimum set gives:

```text
uniform agreement: mean delta +0.3750
improved / same / worsened: 173,195 / 100,312 / 7,292
human-membership delta: +117,640
A-minimum median set size: 1
song keys with positive / negative aggregate: 252 / 0
```

Its imported-voicing membership is 204,923 cases (73.0% of anchored chords).
All leave-one-song-out aggregates retain a positive sign against B0: summed
uniform delta `103,365.04 … 105,303.27` and membership delta
`+114,938 … +117,646`.

However, the preregistered within-song rotated-anchor control falsifies the
local-state interpretation.  Among 280,798 control-eligible events:

```text
true better / equal / worse than rotated: 8,489 / 248,464 / 23,845
mean uniform delta (true - rotated): -0.0341
human-membership delta: -13,652
song keys positive / negative: 7 / 245
```

Every leave-one-song-out control aggregate remains negative: summed uniform
delta `-9,568.77 … -9,167.96`, membership delta
`-13,665 … -13,114`.  A fret drawn from the next eligible chord in the same
song predicts the imported A-minimum set better than the actual local anchor.
Therefore the broad B0→A improvement cannot be claimed as evidence for local
temporal hand state; it is consistent with song-level neck/register bias.

### R2 — incoming technique

There are 1,020 technique-bearing complete chords across 61 song keys.  The
observed whole-chord realization remains feasible in `1,020/1,020`; there are
no conflicting incoming requirements and no R2 infeasibilities.

Incoming constraints reduce the complete admissible assignment count by a
mean 65.3% and median 82.4%.  Fifty-five of 61 song keys have positive mean
reduction and six have zero; none has a negative reduction.  The largest song
contributes 113/1,020 cases.  Leaving out any one song keeps the case-weighted
mean reduction between 63.0% and 73.4%, so no repeated phrase family creates
the effect.

For the 405 events with at least one atom not itself technique-constrained,
non-target uniform agreement improves in 277, is unchanged in 127 and worsens
in 1 (mean +0.0674); 35 song keys have positive aggregate and one negative.
This diagnostic is not mechanically guaranteed by the target condition.

The complete legal-string controls contain 6,910 conditions, of which 5,276
are assignment-feasible.  They retain exact counts plus B0 and A minima.  The
observed condition's own target agreement is deliberately not interpreted as
independent validation.

### R3 — incremental anchor primitive after technique

On the 1,020 events carrying both channels, R3 changes uniform agreement over
R2's B0-optimum set as follows:

```text
improved / same / worsened: 212 / 756 / 52
mean uniform delta: +0.0676
human-membership delta: +156
song keys positive / negative: 23 / 4
```

Every leave-one-song-out aggregate remains positive (summed uniform delta
`+54.92 … +75.58`; membership `+124 … +160`).  Thus the A primitive adds
information after technique in a descriptive exact-set comparison.  It does
not rescue the semantic claim for *local* anchor state: the registered rotated
control has already shown that this A signal is better explained by non-local
within-song fret distribution.

### Legacy cohort and exact-search cost

The separate fail-closed compatibility artifact reproduces:

```text
38 cases
176 legal target-string conditions
174 feasible conditions
B0 ranks 11 / 24 / 3
O rank 1: 36 / 38
A rank 1: 30 / 38
```

Exhaustive in-repo enumeration remained sufficient.  The maximum raw candidate
product was 12,960, the maximum admissible set 336, and the full import,
enumeration, controls and output pass took about 8.5 seconds in release mode.
The slowest single event took below one millisecond in the final run.  No
external solver, heuristic pruning or saturation occurred.

## Verdict against the registered evidence rules

1. **Anchor semantic state: not supported.**  It passes the B0 comparison,
   song breadth and leave-one-song-out checks, and is not confined to the
   legacy 38.  It fails the mandatory negative control decisively: the true
   local anchor is worse than the within-song rotated anchor.  Per the
   preregistration, no local-context claim is made.
2. **Incoming technique state: supported as a Lab representation field.**  It
   preserves all 1,020 imported whole chords, produces no conflicts, sharply
   reduces exact ambiguity across 55 positively affected song keys, improves
   non-target agreement where measurable, and survives every song omission.
   This remains conditional on imported technique evidence and does not choose
   a production constraint API.
3. **Combined state: not supported as evidence for both channels.**  R3 adds a
   positive exact-set effect beyond R2 across multiple songs, but its anchor
   channel fails the required locality control.  The more minimal supported
   representation is therefore chord atoms plus incoming technique; anchor is
   retained only as a diagnostic primitive pending a better falsifiable state
   definition.

The answer to the core question is mixed but narrowing: chord atoms alone lose
measurable technique information, while a single preceding fret does not yet
qualify as local semantic state on this population.  `TabLine` may remain a
computational partition, but imported incoming technique relations can cross
that partition and materially reduce chord-assignment ambiguity.

## Limitations and invariance

Agreement uses one imported voicing, not unique ground truth.  Technique
relations are observed labels; target equality is not independent validation.
The anchor control preserves song identity and fret distribution but not every
possible phrase/tuning covariate.  A is a primitive preference set, not a hand
anatomy model.  The experiment does not establish a production API, objective,
chord solver, `TabLine` change or global sequence model.

The final independent reruns retain 29,758 within-line relations and 46
cross-line relations.  Stage 2 remains B/C1/D1 `0.9 / 13.3 / 16.8%`, with
gaps `19.1 / 7.6 / 4.2` points.  No production or Stage 2 behavior changed.
