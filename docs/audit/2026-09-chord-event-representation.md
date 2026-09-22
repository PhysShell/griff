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

