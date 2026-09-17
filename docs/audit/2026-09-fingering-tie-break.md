# 2026-09 — Fingering tie-break: optimum sets and a learned secondary objective

Follow-up to the optimality-gap audit
([`2026-09-fingering-optimality-gap.md`](2026-09-fingering-optimality-gap.md),
PhysShell/griff#197), whose corrected numbers left one sharp question. Under
the fitted `v1` weights (fret 0, open-string penalty 3, position shift 1,
string change 0) the production DP keeps **44.1%** agreement with tab authors
on holdout songs, while the set of cost-optimal fingerings contains **55.5%**.
That 11.4-point gap needs no change to the primary objective, only a better
choice among its ties. This audit measures the tie sets exactly, asks how much
of the gap a **human-blind learned tie-break** recovers, and locates the
information the tab authors used.

Research tooling only (`lab/`, excluded from the workspace); no production
code changes. Corpus, protocol and holdout are those of the corrected
optimality-gap audit: 410 Guitar Pro files, 1,149 guitar tracks, 9,045
monophonic lines, song-level holdout (1,954 test lines, 70,933 notes). Only
aggregates are recorded; tab content stays out of git (ADR-0005).

## What was built

`lab/src/ties.rs`, red → green per commit:

- **`Chain`** — the `v1` objective per line: candidates in production order,
  unary and pairwise costs.
- **`optimum_set`** — exact chain DPs, no search. Forward and backward cost
  tables carry path counts (saturating `u64` plus an exact natural log). A node
  or edge lies on an optimal path iff `f + g = optimum`. From these come the
  number of optimal paths; the **least and most agreement** with a reference
  among them (min/max DPs over optimal edges); the **expected agreement under a
  uniform draw** from the optimum set (paths through a candidate / total, in
  log space); and a most-agreeing optimal path.
- **`lexicographic_path`** — minimizes `(primary, secondary)`; with zero
  secondary weights it reproduces the production DP path exactly. It supports
  loss augmentation (a margin per reference-matching note).
- **`latent_target`** — among the most-agreeing optimal paths, the cheapest
  under the current secondary weights: the achievable learning target. The
  human path itself is primary-optimal in only 31% of holdout lines.
- **`train_secondary`** — averaged, loss-augmented, latent-target structured
  perceptron over integer weights, inside the primary optimum set.
- **Secondary features** — 20 local fingering-geometry features (fret, open
  string, string one-hot; fret and string distance, string change, same fret,
  spans over 3/5 frets, open transitions, diagonal and box moves, direction),
  plus **`anchor_distance`** (|fret − anchor| per fretted note).
  `TabLine::anchor_fret` gives the anchor: the fret of the latest fretted note
  of the voice before the line, the lowest fret when that onset is a chord.
- **Runner** (`fingering_gap`) — `ties-check` compares the DPs with verified
  CP-SAT records; `tiebreak` trains on train songs, picks the margin on a
  song-level validation bucket (5,111 fit / 1,980 validation lines), and reports
  on holdout songs.

## Verification

- Contract suite against brute force on exhaustive small families:
  - optimum, exact count, min/max/expected agreement;
  - secondary optimality, including anchored chains;
  - the loss-augmented least-agreeing path;
  - the latent target;
  - zero-secondary equality with `infer_positions`;
  - count saturation with an exact log;
  - perceptron convergence on separable synthetic tie-breaks, local and anchored.
- **Exact DPs versus CP-SAT** (`ties-check`), on the verified agreement-pass
  records from the corrected optimality-gap run: for **1,954 / 1,954** holdout
  lines under both `v1` and `v1-fit`, the DP optimum equals the CP-SAT optimum
  and the DP ceiling equals CP-SAT's recounted ceiling. The ceiling now costs
  milliseconds instead of a solver pass. CP-SAT remains the spot-check oracle
  for these DPs.

## Results — the tie-break ladder (holdout songs)

Primary `v1-fit` (the under-discriminative objective):

| rung | agreement | lines changed vs production |
|---|---|---|
| floor — least-agreeing optimal path | 32.4% | |
| uniform draw from the optimum set | 42.9% | |
| production tie-break (lowest candidate index) | 44.1% | — |
| learned tie-break, local features | 44.0% | 89 |
| **learned tie-break, local features + hand anchor** | **47.3%** | 411 |
| ceiling — most-agreeing optimal path | 55.5% | |

The anchored tie-break recovers **3.2 points, 28% of the gap** between the
production tie-break and the ceiling, without touching the primary objective.
The margin (10⁴) was chosen on validation. The validation curve is interior
(47.0 / 46.9 / 47.7 / **48.8** / 48.6 / 48.6 / 48.5% for margins 0 … 10⁷),
over 20 epochs of a non-separable problem (≈57k updates).

Tie structure under `v1-fit` (holdout):

- the optimum is unique in 45.3% of lines;
- ln(#optimal paths): median 0.69 (2 paths), p75 1.79 (6), p90 5.55 (~250),
  p99 28.3;
- the human fingering is itself optimal in 30.9% of lines.

**Control — production `v1` weights.** The optimum is unique in 87.1% of
lines, and the whole ladder spans floor 35.7% → ceiling 36.2%. The learned
tie-break reaches 36.1%, inside that band: the learner finds what little there
is and cannot exceed the ceiling by construction.

## Results — where the tab authors' information lives

**The local features carry none of it.** With local features only, the
learned tie-break converges on production's own choices: the latent target,
margins up to 10⁷ and 20 epochs changed no conclusion (44.0% vs 44.1%). A
diagnostic probe over train songs shows why:

- where a most-agreeing optimal path differs from production, the author plays
  the same pitch **one or two strings lower and higher up the neck** (+1 string
  / +5 frets 50%, +1 / +4 40%, +2 / +9 7% of differing notes);
- yet in about three quarters of lines the author agrees with production's
  "highest string, lowest fret" choice. No global weight on local geometry
  separates the two;
- 42.5% of the differing notes sit in repeated-pitch runs, where hand travel
  inside the line is free.

**Part of it is just outside the line.** Lines are cut at chords and rests,
and the author's choice tracks where the hand was before the cut. On
differing notes the author's fret is closer to the anchor than production's
in 56.7% of notes against 37.8% (5.5% equal); per line, 63.4% against 32.8%.
Adding that one feature produced the only real gain.

## Reading

- **The v1-fit ceiling is not a local-geometry ceiling.** The optimum set does
  contain better fingerings, but a tie-break that sees only the line cannot
  tell them apart. The discriminating information is contextual.
- **Context works as expected.** One boundary feature recovers about a
  quarter of the gap. The rest likely needs richer context than a single fret:
  the anchor string, time since the anchor, the shape of the chord before and
  after, and what follows the line.
- **Lexicographic primary + secondary is the right production shape** for this
  kind of gain. It is exact, cheap, and leaves the primary objective's
  guarantees intact.

## Limitations (recorded, not hidden)

- **The anchor is taken from the human tab.** That is the tab-completion
  scenario. For MIDI-sourced material the preceding event is usually a chord,
  which production leaves unpositioned today (ADR-0019 §7): an anchor there
  needs chord voicing first. This ties the monophonic quality gain to the
  next Lab subject.
- The secondary is linear and hand-featured, and the perceptron is not
  convergent on real data (averaged weights, 20 epochs). A max-margin or
  probabilistic learner may extract more from the same features.
- One primary (`v1-fit`) and its control; the hand-position model is parked
  per the optimality-gap audit.
- Agreement treats one tab as ground truth.

## Follow-ups proposed

1. Richer boundary context for the secondary: anchor string, anchor-to-line
   time, the following event's position, chord span. Measure each as an
   ablation on this ladder.
2. Chord voicing (the next Constraint Lab subject), which also produces
   anchors for MIDI-sourced lines.
3. Once the anchored tie-break is stable, an ADR for lexicographic
   primary + secondary fingering in `core`, with the secondary weights as
   versioned data (ADR-0017 §3).
