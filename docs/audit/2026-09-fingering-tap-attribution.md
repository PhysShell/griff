# 2026-09 — Technique-aware fingering, oracle stage: does hand attribution explain the tapping slice?

A follow-up to the optimality-gap and tie-break audits
([`2026-09-fingering-optimality-gap.md`](2026-09-fingering-optimality-gap.md),
[`2026-09-fingering-tie-break.md`](2026-09-fingering-tie-break.md)).

**Terminology.** "The human path lies in the model's optimum set" means that
the tab author's fingering happens to minimize the *model's* cost function.
It says nothing about how well anyone plays. When the human path falls
outside the set, the model is missing something the player took into account.

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

## Limitations

- Oracle labels from the tab; tap marks undercount, including all GP6/7
  tapping, which the importer drops (above).
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
   again.
2. Only then **hidden technique inference** for MIDI-sourced lines: tap,
   hammer-on, pull-off and slide as latent per-note labels, with these
   tab-labelled lines as the supervised check.
3. Decide between chord voicing (anchors for MIDI lines, per the tie-break
   audit) and full technique-aware fingering as the next production-facing
   step.
