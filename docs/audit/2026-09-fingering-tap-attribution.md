# 2026-09 — Technique-aware fingering, oracle stage: does hand attribution explain the tapping slice?

A follow-up to the optimality-gap and tie-break audits
([`2026-09-fingering-optimality-gap.md`](2026-09-fingering-optimality-gap.md),
[`2026-09-fingering-tie-break.md`](2026-09-fingering-tie-break.md)).

**Terminology.** "The human path lies in the model's optimum set" means that
the tab author's fingering happens to minimize the *model's* cost function.
It says nothing about how well anyone plays. When the human path falls
outside the set, the model is missing something the player took into account.

> **Re-measured after the GPIF import fix (#201).** GP6/7 tapping now reaches
> the lab, so the slice grows from 155 to 242 lines. The length-matched
> baseline below also contained that unlabelled tapping. Against the corrected
> baseline, hand attribution closes **most, not all**, of the slice's excess.
> The exactness finding stands. The sections from "Question" to "Conclusion"
> are the original measurement, kept as run; see
> [Re-measurement after #201](#re-measurement-after-201).
>
> **Re-measured again after the tuplet import fix (#202).** The slice is now
> 226 lines, fewer but longer. All three readings (partial excess closure,
> exactness out of reach, and the whole-slice share closed: 77%) still hold. See
> [Re-measurement after #202](#re-measurement-after-202).

## Question

The `v1` objective reads every note's position as the fretting hand's
position. Tapping breaks that assumption. A tapped note is played by the
picking hand while the fretting hand stays where it was. On the whole corpus,
no line with tapped notes had its human path in the `v1-fit` optimum set.

> **If the model is told which hand plays each note, how much of that failure
> goes away?**

This is the **oracle stage**: the technique labels come from the tab
(`NoteMark::Tap`), not from inference. Changing the physiology model and the
technique recognition at once would leave one number and two suspects.

## What was built

`lab/`, red → green per commit:

- **`TabLine::tapped`** — one tap flag per note.
- **`technique::tap_aware_cost`** — `v1` per-note costs and string changes
  between neighbours, plus two travel terms:
  - **fretting hand**: travel from the previous *untapped* note, so its anchor
    carries across taps;
  - **picking hand**: travel from the previous *tapped* note, at `tap_shift`.

  Without taps it equals `v1_cost`.
- **`technique::tap_aware_chain`** — the same objective as a `ties::Chain`,
  so the exact optimum-set DPs of the tie-break audit apply. Each state pairs
  a note's candidate with the other hand's last candidate. Transitions that
  contradict that carried candidate are inadmissible, which keeps state paths
  and position assignments in one-to-one correspondence.
- **`fingering_gap taps`** — tap-blind and tap-aware models under **the same
  weights**. With `tap_shift = position_shift`, costs and excesses are in the
  same units, so they compare directly. Also reported: an untapped baseline
  reweighted to the tapped slice's line lengths.

## Verification

- Contract suite: the tap-aware cost is hand-computed on the 5 → 8 → tap 12
  → 8 → 5 figure (14 tap-blind, 6 tap-aware) and equals `v1_cost` without
  taps. The chain is checked by brute force over position assignments and all
  tap masks: same optimum, same optimal-assignment count, and every
  admissible state path scores its assignment exactly. Without taps it is the
  `v1` chain.
- **Control on the corpus:** on **0 of 8,890** untapped lines do the
  tap-blind and tap-aware objectives disagree on the optimum or the
  production-order path.

## The slice

- **Whole corpus:** 155 lines with at least one tapped note (1.7% of 9,045
  lines), 14,480 notes (4.4% of 326,130), of which 3,132 are tapped. Tapped
  lines are long: 93 notes on average.
- **Holdout songs:** only 15 of these lines. Those numbers are in
  `taps.json`, but no conclusion rests on them.
- **Tap labels undercount — and one cause is the importer.** Tab authors
  sometimes leave tapping unmarked. More importantly, the `guitarpro` 0.4.2
  GPIF import never reads the `Tapped` note property: it leaves the beat's
  tap effect as a placeholder. As a result, **no GP6/GP7 tapping reaches
  griff**, although 31 of the corpus's 145 GPIF files contain it: 1,006
  `Tapped` note definitions, plus 45 `LeftHandTapped`. The 155-line slice is
  therefore GP3–5 material only, and GP6/7 tapping sits unlabelled inside the
  "untapped" lines and the length-matched baseline. (GPIF deduplicates
  repeated notes, so 1,006 is a lower bound on played tapped notes.)

## Results (whole corpus)

`v1-fit` weights (fret 0, open-string penalty 3, position shift 1, string
change 0):

| model | human path in optimum set | excess per note | agreement | on tapped notes | ceiling |
|---|---|---|---|---|---|
| tap-blind | 0.0% | 2.94 | 39.4% | 30.7% | 45.7% |
| **tap-aware, `tap_shift` = 1** | 1.3% | **1.27** | **44.3%** | **44.6%** | **52.9%** |
| tap-aware, `tap_shift` = 0 | 3.9% | 1.12 | 39.6% | 23.1% | 62.5% |
| *length-matched untapped lines* | *21.0%* | *1.26* | *44.9%* | — | *54.1%* |

Production `v1` weights:

| model | human path in optimum set | excess per note | agreement | ceiling |
|---|---|---|---|---|
| tap-blind | 0.0% | 7.76 | 32.9% | 33.9% |
| tap-aware, `tap_shift` = 2 | 1.3% | 4.87 | 34.9% | 35.7% |
| *length-matched untapped lines* | *13.7%* | *5.03* | *36.6%* | *37.1%* |

## Reading

*(Revised after #201: the excess half of this reading does not survive the
corrected baseline; see [Re-measurement after #201](#re-measurement-after-201).)*

**Hand attribution explains the slice's *excess*, not its *exactness*.** With
the tap labels, the tapped slice reaches the untapped baseline of the same
line lengths on every continuous measure, under both weight sets:

- excess per note 1.27 against 1.26 (`v1-fit`);
- agreement 44.3% against 44.9%;
- ceiling 52.9% against 54.1%.

Agreement on the tapped notes themselves rises 14 points. Yet the human path
lands in the optimum set in only 1.3% of tapped lines, against 21.0% of
comparable untapped lines. The human paths are now *about as close* to the
optimum as elsewhere, but almost never *on* it: 153 of 155 lines keep a
positive residual.

`tap_shift = 0` is the wrong model. A free picking hand flattens the
objective (no line has a unique optimum), raises the ceiling and makes the
tie-break choice on tapped notes worse (23.1%). Right-hand travel is real.

## Where the residual is

Human cost minus the tap-aware optimum's cost on the slice (`v1-fit`,
`tap_shift` = 1), by component:

| component | human | tap-aware optimum | human − optimum |
|---|---|---|---|
| fretting-hand travel | 26,475 | 12,684 | **+13,791 (75%)** |
| picking-hand travel | 4,659 | 2,357 | +2,302 (12.5%) |
| open-string penalty | 2,769 | 450 | +2,319 (12.5%) |

This is a **decomposition under the current objective, not a causal
attribution**. The terms interact: adding a cost for, say, string continuity
would move the optimum path and redistribute the residual across all three
components. Read the table as "75% of the residual cost under the current
objective falls on the fretting-hand travel term", not as "75% of the problem
is the fretting hand".

What the decomposition suggests, as hypotheses for the next stage:

- **H1, string continuity.** Tapping figures appear to live **on one string**:
  tap, pull off to a fretted note on the same string, often on to an open
  string. The tab authors put a tap on the string of the preceding fretted
  note 1,023 times; the model's optimum does so 525 times. With string
  changes free under `v1-fit`, the optimum can scatter a figure across
  strings to save fret travel, and a player keeping it on one string would
  pay exactly this kind of travel.
- **H2, conditional open strings.** An open string looks like a natural
  pull-off target in these figures, while `v1-fit` penalizes open strings
  everywhere (it was fitted mostly on untapped material). This points to an
  interaction (open target under a pull-off), not to a different global open
  weight.

## Conclusion

Attributing tapped notes to the picking hand is necessary. On this slice it
is sufficient to remove the slice-specific *excess*, but not to put the human
paths into the optimum set. The residual has a systematic structure that the
objective does not model; H1 and H2 point to technique continuity (legato
figures binding notes to one string) as the candidate, to be tested as an
observed-label oracle before any hidden technique inference. Otherwise an
inference stage could learn to paper over a continuity cost it cannot see.

*(Revised after #201: "sufficient to remove the slice-specific excess" does
not hold against the corrected baseline; see below.)*

## Re-measurement after #201

**Setup.** #201 (merged into `main`) imports GPIF `Tapped` and keeps the
hammer-on flag on origin notes only. This PR's lab code was rebuilt on a
local, unpushed merge of `main` (`c028609`) and `fingering_gap taps` rerun
with the same weights. A local per-line dump (not committed) splits the
slice. Rerun on this PR's head, the command reproduces the report above byte
for byte.

**What changed in the data (whole corpus).**

- **Lines unchanged.** Line cuts are the same (9,045 lines, same ids). The
  155 original tapped lines (all GP3–5) are unchanged line by line. The
  hammer-on fix does not touch this experiment, which has no legato term.
- **87 GP6/7 lines gain tap labels.** They come from 30 files and hold 9,042
  notes, 1,840 of them tapped; 18 of the lines are on holdout songs. The
  slice grows to **242 lines, 23,522 notes, 4,972 tapped**.
- **The baseline pool shrinks.** The same 87 lines leave the untapped pool
  (8,890 → 8,803 lines). They had been scored there tap-blind, which inflated
  the baseline. For the original 155 lines, the `v1-fit` length-matched
  baseline moves from 1.26 to **1.17** excess per note, and agreement from
  44.9% to 45.3%.

**Results (whole corpus, `v1-fit`, tap-aware at `tap_shift` = 1).** Baselines
are untapped lines reweighted to each subset's line lengths, drawn from three
pools: all untapped lines, the same format family, and the same files.

| subset | model or baseline | human path in optimum set | excess per note | agreement | on tapped notes | ceiling |
|---|---|---|---|---|---|---|
| all 242 | tap-blind | 0.0% | 3.17 | 38.3% | 31.4% | 43.8% |
| all 242 | **tap-aware** | 1.7% | **1.60** | 43.8% | 46.0% | 52.2% |
| all 242 | *baseline: all untapped* | *20.9%* | *1.17* | *45.3%* | — | *54.6%* |
| original 155 (GP3–5) | tap-aware | 1.3% | 1.27 | 44.3% | 44.6% | 52.9% |
| original 155 (GP3–5) | *baseline: all / GP3–5 / same files* | *21.2 / 17.9 / 14.7%* | *1.17 / 1.08 / 0.98* | *45.3 / 45.2 / 48.3%* | — | *54.6 / 54.1 / 56.5%* |
| added 87 (GP6/7) | tap-blind | 0.0% | 3.52 | 36.5% | 32.6% | 40.8% |
| added 87 (GP6/7) | tap-aware | 2.3% | 2.13 | 42.9% | 48.4% | 51.1% |
| added 87 (GP6/7) | *baseline: all / GP6/7 / same files* | *20.4 / 23.7 / 20.7%* | *1.16 / 1.41 / 1.22* | *45.3 / 45.2 / 44.8%* | — | *54.7 / 55.4 / 56.4%* |

Share of the gap between tap-blind excess and the baseline that tap
attribution closes:

| subset | baseline pool | share closed |
|---|---|---|
| original 155 | pre-#201 pool, as reported above | 99% |
| original 155 | all untapped / GP3–5 | 94% / 90% |
| added 87 | all untapped / GP6/7 | 59% / 66% |
| all 242 | all untapped | 78% |

Other results:

- **Production `v1` weights (all 242).** Tap-blind 7.89, tap-aware
  (`tap_shift` = 2) 5.21, baseline 4.92 excess per note. The human path is in
  the optimum set for 0.8% of lines against 13.7% of baseline lines.
- **`tap_shift = 0` is still the wrong model.** Agreement on tapped notes
  falls to 15.7%, and to 3.2% on the added lines.
- **Holdout songs.** Now 33 lines; they are in `taps.json` but not
  interpreted.

**Concentration.** Two transcriptions of one song (the same song key, both
on training songs) supply three lines of 461–541 notes. Those lines carry
46% of the added lines' residual and 23% of the whole slice's residual. In
them the tab keeps a repeated tapping figure on one string: tap, pull-off to
the open string, two fretted notes. The lead-in is played on an open string
as well. The tap-aware `v1-fit` optimum spreads the same figure over four
strings at higher frets, which avoids the open-string penalty. Without these
two files, the added lines sit at their format's baseline on the continuous
measures:

- excess per note 1.38 against 1.38;
- agreement 47.8% against 45.4%.

They are still not on the optimum: 2.4% against 24.4%.

**Where the residual is (all 242).** Human cost minus the tap-aware optimum's
cost, by component, checked per line against the dump. As above, this is a
decomposition under the current objective, not a causal attribution.

| component | human | tap-aware optimum | human − optimum |
|---|---|---|---|
| fretting-hand travel | 48,229 | 20,143 | **+28,086 (75%)** |
| picking-hand travel | 7,803 | 3,711 | +4,092 (11%) |
| open-string penalty | 6,624 | 1,158 | +5,466 (15%) |

Fretting-hand travel keeps its 75% share. Within the added lines the
open-string term takes a larger share: 16%, against 12.5% on the original
lines. A tap on the string of the immediately preceding untapped note occurs
as follows:

| subset | human | optimum |
|---|---|---|
| all 242 | 1,770 | 832 |
| original 155 | 1,023 | 525 |
| added 87 | 747 | 307 |

**Revised reading.**

- **Stands.** Attributing tapped notes to the picking hand is necessary, and
  picking-hand travel is real. Exactness remains out of reach. The human path
  lies in the optimum set in 1–2% of tapped lines, against 15–24% for every
  length-matched untapped baseline. This holds in both format families.
- **Revised.** Hand attribution does **not** fully explain the slice's
  excess. It closes most of the gap: 94% on the original lines against the
  corrected pool, 66% on the added lines against their own format, 78% on
  the whole slice. The earlier near-exact match (1.27 against 1.26) came
  partly from unlabelled GP6/7 tapping inside the baseline pool.
- **Consistent with H1/H2.** Where the remaining excess concentrates, it has
  the shape H1 and H2 anticipated: one-string tapping figures with
  open-string pull-offs. This is an observation on a concentrated subset,
  not a test.

**Consequences for stage 2 (legato continuity).**

- **Baseline.** The baseline is this re-measurement, not the 155-line
  slice: 242 lines and a pool of 8,803 untapped lines.
- **Format.** Report per format family, with format-matched baselines next
  to the pooled one.
- **Concentration.** Report a concentration check by song key, for example
  leave one song out. One song supplies about a quarter of the residual.
- **Primary target.** Exactness (human path in the optimum set) is the
  primary target, ahead of excess.

## Re-measurement after #202

**Setup.** #202 (merged into `main`) corrects the importer's tuplet durations.
Before it, bars with tuplets overflowed into the next bar, and tab lines,
which order notes by onset, interleaved notes from neighbouring bars. For
this section, this PR was merged with `main` at `e871a44` and
`fingering_gap taps` was rerun with the same weights. The slice rule (lines
containing a tapped note) is unchanged.

**What changed in the data (whole corpus).**

- **The slice.** 242 → **226** lines, 23,522 → 25,571 notes, 4,972 → 5,260
  tapped notes. Lines no longer break where neighbouring bars used to
  interleave, so the slice has fewer but longer lines.
- **The untapped pool.** 8,803 → 8,740 lines.
- **Holdout songs.** 33 → 30 lines. Reported in `taps.json`, not interpreted.
- **Control.** The tap-blind and tap-aware objectives still agree on every
  untapped line (0 of 8,740 differ).

**Results (whole corpus).**

`v1-fit`. Tap-aware uses `tap_shift` = 1; the baseline is untapped lines
reweighted to the slice's line lengths.

| model or baseline | human path in optimum set | excess per note | agreement | on tapped notes | ceiling |
|---|---|---|---|---|---|
| tap-blind | 0.0% → 0.0% | 3.17 → 3.20 | 38.3% → 38.5% | 31.4% → 29.0% | 43.8% → 44.1% |
| **tap-aware** | 1.7% → **0.9%** | 1.60 → **1.63** | 43.8% → 43.1% | 46.0% → 45.3% | 52.2% → 52.3% |
| *baseline: all untapped* | *20.9% → 20.0%* | *1.17 → 1.16* | *45.3% → 44.9%* | — | *54.6% → 54.5%* |

Other results:

- **Share of the excess gap closed** (tap-blind excess against the pooled
  baseline, whole slice): 78% → **77%**.
- **Production `v1` weights.**
  - Excess per note: tap-blind 8.10, tap-aware (`tap_shift` = 2) 5.41,
    baseline 4.91.
  - The human path is in the optimum set for 0.9% of lines, against 13.1% of
    baseline lines.
- **`tap_shift = 0` is still the wrong model.** Agreement on tapped notes is
  15.4%.

**Not re-run.**

- the split into the original 155 and the added 87 lines;
- the format-matched and same-file baselines;
- the concentration check;
- the residual decomposition by cost component.

These came from a local per-line dump keyed to the pre-#202 lines, and #202
changes those lines. They stay as measured after #201. Stage 2 reports its
per-format and concentration checks on the 226-line slice.

**Reading.**

- **The #201 revision stands.** Hand attribution closes most of the slice's
  excess (77%), not all of it.
- **Exactness remains out of reach.** The human path is in the optimum set in
  0.9% of tapped lines, against 20.0% for the length-matched baseline.
- **Stage 2's baseline is this measurement:** 226 lines against an 8,740-line
  pool.

## Limitations

- Oracle labels from the tab; tap marks undercount. The original measurement
  also missed all GP6/7 tapping, which the importer dropped until #201 (see
  the re-measurement).
- Hand attribution is one binary label per note. There are no simultaneous
  two-hand notes, no multi-finger picking-hand tapping, and no per-finger
  model.
- One tab is taken as ground truth; `v1-fit` weights are reused unchanged
  (fitted on all lines); the holdout slice is too small to report on.

## Follow-ups proposed

1. **Legato continuity:** carry Guitar Pro's hammer-on, pull-off and legato
   spans (`TechniqueSpan`) onto tablature lines and add a same-string
   continuity term (or constraint) for notes joined by legato or tap. Measure
   the tap slice's exact-optimum rate against the length-matched baseline
   again, starting from the re-measured slice after #201.
2. Only then **hidden technique inference** for MIDI-sourced lines: tap,
   hammer-on, pull-off and slide as latent per-note labels, with these
   tab-labelled lines as the supervised check.
3. Decide between chord voicing (anchors for MIDI lines, per the tie-break
   audit) and full technique-aware fingering as the next production-facing
   step.
