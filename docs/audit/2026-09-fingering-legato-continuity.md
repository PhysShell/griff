# 2026-09 — Technique-aware fingering, oracle stage 2: does observed legato continuity explain the exactness residual?

Follows [`2026-09-fingering-tap-attribution.md`](2026-09-fingering-tap-attribution.md)
(oracle stage 1) and its re-measurement after #201.

**Terminology.** "The human path lies in the model's optimum set" means that
the tab author's fingering happens to minimize the *model's* cost function.
It says nothing about how well anyone plays.

## Question

> **Does observed legato continuity explain the exactness residual that
> remains after correct tap-hand attribution?**

Stage 1, re-measured on the corrected import, left this state (whole corpus,
242 tapped lines, `v1-fit`). Tap-aware attribution closes most of the excess
gap. Yet the human path lies in the optimum set in 1.7% of tapped lines,
against 20.9% of length-matched untapped lines. The residual concentrates in
tapping figures kept on one string with open-string pull-offs.

- **Primary outcome:** exactness, i.e. whether the human path lies in the
  model's optimum set.
- **Secondary, continuous diagnostic:** excess per note. It separates "one
  cost unit off the optimum" from "a different objective altogether"; both
  count as not exact.

## Protocol (fixed before any stage-2 measurement)

This section is committed before the census and the ablation are run. Results
must be reported against it; deviations are reported as deviations.

### Data and its limits

- **Corpus and lines.** The corpus and tablature lines are those of stage 1
  (`D:\tabs`, 410 files, `LineCut::v1`), imported with #201.
- **Tap slice.** Lines with at least one tapped note: 242.
- **Untapped pool.** Lines with none: 8,803.
- **Formats.** Format families are GP3–5 and GP6/7 (`.gp`, `.gpx`).
- **Splits.** Every number is labelled *whole corpus* or *holdout songs*.
  Holdout numbers are reported but not interpreted.
- **Import limit.** The Guitar Pro import gives an **observed legato origin**:
  the note a hammer-on or pull-off starts from. It does **not** reliably give
  the direction. Every legato origin arrives as `SpanTechnique::HammerOn`, and
  `PullOff` and `Legato` are never emitted. A `HammerOn` label is therefore
  not treated as a true hammer-on.

### `TechniqueEdge` projection

- **Edges.** A technique belongs to the edge from note `i − 1` to note `i`,
  not to a note. `TabLine::edges[i]` records the imported span kind when note
  `i − 1` carries a `HammerOn`, `PullOff` or `Legato` span; `edges[0]` is
  always plain. A legato origin on a line's last note has no edge; these are
  counted as *dangling*.
- **Derived direction.** Direction is computed from pitch:
  - higher pitch into note `i`: ascending (hammer candidate);
  - lower pitch: descending (pull candidate);
  - the same pitch: unison.

  On one string this is the fret order. Reports keep derived direction apart
  from imported labels.

### Phase 1 — legato census (before any objective change)

Every table is given per format family (GP3–5, GP6/7, all), for the whole
corpus and for holdout songs. Populations are all lines and the tap slice.

| law | measure | base rate reported beside it |
|---|---|---|
| L1 | P(same string \| legato edge), per imported kind (with counts) | P(same string \| plain edge) |
| L2 | P(same string \| legato edge), per derived direction | — |
| L3 | P(target open \| legato edge, descending) | P(target open \| plain edge, descending) |
| L4 | tap-adjacent edges, reported separately: edges out of a tapped note (P(legato); P(same string \| legato); derived direction) and edges into a tapped note | — |
| — | counts: legato origins in lines, realized edges, dangling origins | — |

### Phase 2 — ablation

All stages use the same weights, `tap_shift = position_shift` as in stage 1,
and the same lines. The ablation runs one step at a time: each stage differs
from its predecessor by exactly one term.

| stage | objective |
|---|---|
| **A** | tap-blind `v1` |
| **B** | tap-aware (stage 1) |
| **C1** | B, plus hard continuity: each legato edge whose two notes lie on different strings costs `H = 2³²`. The optimum first minimizes cross-string legato edges, then B's cost. |
| **C2(k)** | B, plus soft continuity: `k · position_shift` per cross-string legato edge. `k ∈ {1, 3, 10}` is reported in full; **`k = 3` is primary** and fixed now, not chosen from results. |
| **D1** | C1, plus a pull-off target waiver: the target of a legato edge with derived *descending* direction pays no open-string penalty (its open-string term becomes `min(−open_string, 0)`: a penalty is removed, a bonus is untouched). |
| **D2** | C2(3), plus the same waiver. |

D uses derived direction, so D is **derived evidence**, not imported truth.

**Weights.**

- **Primary:** `v1-fit` = (fret 0, open-string −3, position shift 1, string
  change 0).
- **Secondary:** production `v1`, for which the waiver is a no-op by
  construction.
- No weight is fitted, learned or tuned on these results.

**Metrics per stage** (identical for every stage):

- human path in the optimum set (primary);
- excess per note;
- agreement of the production-order path;
- ceiling;
- agreement on tapped notes.

For C1 and D1, lines where the human path has more cross-string legato edges
than the optimum count as not exact. They are excluded from excess per note
and counted separately, as are lines whose optimum cannot avoid a
cross-string legato edge.

**Subsets:** all 242 tapped lines, GP3–5, GP6/7, holdout songs (labelled,
not interpreted).

**Baselines.** A baseline is the untapped pool under **the same stage's
objective** (legato terms apply to untapped lines too), reweighted to the
subset's line lengths. There are two pools: all untapped lines, and the same
format family. The quantity of interest is the exactness gap, baseline minus
slice, per stage.

**Controls.**

- On lines without legato edges, C1, C2 and D equal B: the same optimum and
  production path. The corpus control count must be 0.
- The chain encodings are checked against brute force.

**Concentration check and stop rule.**

- **Leave one song out.** For each step (A→B, B→C1, B→C2(3), C1→D1, C2→D2),
  compute Δ = change in the slice's exactness, in percentage points. Recompute
  Δ with each song key's lines removed. Report the minimum and maximum Δ, and
  the largest single song's share of the net line gain (lines entering the
  optimum set minus lines leaving it).
- **Stop rule.** An exactness improvement counts as **corpus evidence** only
  if Δ > 0 on the full slice **and** under every leave-one-song-out removal.
  Otherwise it is reported as **concentrated case evidence**. A claim for one
  format family applies the same rule within that family.

### Out of scope

- learning or fitting weights;
- hidden technique inference;
- production weights or code;
- the hand model;
- slides;
- `LeftHandTapped`;
- importing hammer-on/pull-off direction in core.

## Deviations from the protocol

Recorded as they happened; none changed a stage, a weight or a metric.

1. **Implementation preceded the census; measurement did not.** The C1, C2
   and D objectives were implemented and unit-tested (brute force against
   the chain encoding) before the phase-1 census ran. They were not run on
   the corpus. After the census no stage, weight, `k` value or waiver
   definition was changed.
2. **The first census ran on a broken import and is used only as a
   diagnosis.** On `main` before #202, legato edges kept their notes on one
   string only 96.6% of the time (whole corpus). The cross-string cases were
   concentrated: 4 files held 67% of them. Inspecting them showed imported
   onsets running backwards in time within a voice.
   - **Cause.** The Guitar Pro importer applied the tuplet ratio upside down
     (`× enters / times`), making every tuplet 9/4 too long, and it ignored
     double dots. Bars with tuplets overfilled into the next bar, and tab
     lines, which sort notes by onset, interleaved notes of adjacent bars.
   - **Fix.** #202 corrected the importer. Its backward onset steps fell from
     1,051 to 17 (GP6/7) and from 1,517 to 371 (GP3–5).
   - **Use here.** That census is not a result of this stage. The phase-1
     results below come from the corrected import, after the baselines were
     re-measured.
3. **The slice and the pool changed size with #202.** Both are defined by
   rule (lines with or without a tapped note), and the rule stands. After
   #202 the tap slice is **226** lines (not 242) and the untapped pool is
   **8,740** lines (not 8,803).

## Baselines after #202 (impact sweep, before the census)

Each earlier experiment was rebuilt on its own branch head, merged locally
(not pushed) with `main` at `4f6c505` (#202). The commands and weights are
the same as in the original runs.

### #197 — optimality gap (holdout songs)

- **Corpus.** 9,045 → 8,966 lines (holdout 1,954 → 1,945); 326,130 → 329,095
  notes.
- **Oracle.** The CP-SAT oracle was not rerun in full. Of the 1,945 holdout
  problems per model:
  - 1,831 have unchanged fingerprints and keep their verified records;
  - the 114 changed or new problems were solved again, agreement pass
    included.
- **DP exactness.** All 1,945 are proven optimal, and the in-repo DP gap is 0
  on every line for both models.

| model | DP agreement | ceiling at optimum | human path in optimum set |
|---|---|---|---|
| lowest-fret | 33.5% → 33.0% | — | — |
| `v1` | 35.8% → 35.4% | 36.2% → 35.8% | 19.2% → 19.0% |
| `v1-fit` | 44.1% → 44.2% | 55.5% → 55.4% | 30.9% → 30.7% |
| hand-fit (DP only) | 44.3% → 44.7% | — | — |

### #199 — tie-break ladder (holdout songs)

The exact optimum-set DPs still equal CP-SAT on all 1,945 lines, for both the
optimum and the ceiling, under both models.

| weights | features | floor | uniform | production | learned | ceiling | learned − production |
|---|---|---|---|---|---|---|---|
| `v1-fit` | local only | 32.4 → 32.6% | 42.9 → 43.0% | 44.1 → 44.2% | 44.0 → 44.1% | 55.5 → 55.4% | −0.18 → −0.15 pt |
| `v1-fit` | local + anchor | 32.4 → 32.6% | 42.9 → 43.0% | 44.1 → 44.2% | 47.3 → 47.3% | 55.5 → 55.4% | **+3.11 → +3.10 pt** |
| `v1` | local + anchor | 35.7 → 35.3% | 35.9 → 35.5% | 35.8 → 35.4% | 36.1 → 35.7% | 36.2 → 35.8% | +0.33 → +0.32 pt |

The validation-chosen margins are unchanged. The anchor's contribution
survives the corrected timeline, even though it depends on the preceding
context.

### #200 — tap slice (whole corpus, `v1-fit`)

| | after #201 | after #202 |
|---|---|---|
| tapped lines (notes, tapped notes) | 242 (23,522, 4,972) | **226** (25,571, 5,260) |
| untapped pool | 8,803 | 8,740 |
| tap-blind: in optimum set / excess per note / agreement | 0.0% / 3.17 / 38.3% | 0.0% / 3.20 / 38.5% |
| tap-aware (`tap_shift` = 1) | 1.7% / 1.60 / 43.8% | **0.9% / 1.63 / 43.1%** |
| length-matched untapped | 20.9% / 1.17 / 45.3% | **20.0% / 1.16 / 44.9%** |
| control: untapped lines where the objectives differ | 0 | 0 |

On the whole corpus the slice has fewer but longer lines. Lines no longer
break where adjacent bars used to interleave. Holdout songs: 30 lines,
reported, not interpreted.

### Frozen stage-2 baseline

- **Slice.** 226 tapped lines against an 8,740-line untapped pool.
- **Stage B exactness.** 0.9% of tapped lines against 20.0% (`v1-fit`, whole
  corpus).
- **Per format.** Per-format rows come with stages A and B in phase 2, from
  the same command.
- **Earlier conclusions.** None of the conclusions of #197, #199 or #200
  changes.

## Phase 1 — legato census (corrected import)

`fingering_gap legato-census`, on `main` at `4f6c505` merged into this branch.

- **Imported kinds.** All 29,778 legato edges arrive as `HammerOn` on the
  whole corpus; `PullOff` and `Legato` edges number 0, and direction is
  derived.
- **Dangling origins.** 26 legato origins end a kept line (89 before #202).

**L1 — one string across the edge** (whole corpus). Share of edges whose two
notes lie on the same string, with the edge count in parentheses:

| family | population | legato edges | plain edges (base rate) |
|---|---|---|---|
| GP3–5 | all lines | 99.4% (18,093) | 59.9% (192,551) |
| GP3–5 | tap slice | 99.1% (5,965) | 51.3% (9,372) |
| GP6/7 | all lines | 99.8% (11,685) | 56.0% (97,800) |
| GP6/7 | tap slice | 99.9% (4,046) | 58.4% (5,962) |
| all | all lines | **99.6%** (29,778) | 58.6% (290,351) |
| all | tap slice | **99.4%** (10,011) | 54.0% (15,334) |

Holdout songs: 99.8% of 5,889 legato edges (all lines) and 99.4% of 1,270
(tap slice).

**L2 — per derived direction** (whole corpus, all formats):

| population | ascending | descending | unison |
|---|---|---|---|
| all lines | 99.4% (12,879) | 99.7% (16,878) | 71.4% (21) |
| tap slice | 99.3% (3,489) | 99.6% (6,516) | 0.0% (6) |

**L3 — open-string target of a descending edge** (whole corpus):

| family | population | legato edges | plain edges (base rate) |
|---|---|---|---|
| GP3–5 | all lines | 30.5% (10,315) | 11.5% (61,296) |
| GP3–5 | tap slice | 19.7% (3,880) | 8.2% (2,841) |
| GP6/7 | all lines | 33.6% (6,563) | 11.1% (32,545) |
| GP6/7 | tap slice | 34.6% (2,636) | 9.6% (1,714) |
| all | all lines | **31.7%** (16,878) | 11.3% (93,841) |
| all | tap slice | **25.7%** (6,516) | 8.7% (4,555) |

Holdout songs:

- all lines: 14.2% (3,047) against 9.5% (20,377);
- tap slice: **5.1% (844) against 6.3% (648)**, the opposite direction.

**L4 — tap-adjacent edges** (whole corpus; every tapped note lies in the tap
slice). "Legato" columns count only the legato edges among them.

| family | edges out of a tapped note | legato share | legato: one string | legato: ascending / descending / unison | legato: open target | edges into a tapped note | legato share | legato: one string |
|---|---|---|---|---|---|---|---|---|
| GP3–5 | 3,148 | 74.0% | 99.3% | 4.3 / 95.5 / 0.2% | 12.7% | 3,121 | 8.3% | 96.2% |
| GP6/7 | 2,059 | 75.2% | 100.0% | 1.2 / 98.8 / 0.0% | 17.9% | 2,032 | 8.6% | 98.9% |
| all | 5,207 | 74.5% | 99.6% | 3.1 / 96.8 / 0.1% | 14.8% | 5,153 | 8.4% | 97.2% |

Holdout songs: 777 edges out of a tapped note, 65.8% legato; all of those
stay on one string and descend.

**Reading** (descriptive; the tests are in phase 2):

- **L1: an observed legato origin keeps its target on its string.** This
  holds in 99.6% of edges on the whole corpus, in both format families (99.4%
  and 99.8%) and in the tap slice (99.4%), against 58.6% for plain edges. At
  that level a hard constraint is plausible, so C1 is tested beside C2, as
  registered.
- **L2: the law does not depend on the derived direction.** Ascending
  99.4%, descending 99.7%. The 21 unison edges are too few to read.
- **L3: a descending legato edge lands on an open string about 2.8 times as
  often as a descending plain edge.** 31.7% against 11.3%; the lift appears in
  both families and in the tap slice (25.7% against 8.7%). This is the
  interaction D encodes.
  - The tap-slice holdout shows no lift (5.1% against 6.3%, 844 edges). This
    is reported, not interpreted, and it is one more reason D must pass the
    leave-one-song-out rule.
- **L4: tapping figures are legato figures.**
  - Three quarters of the edges out of a tapped note are legato.
  - Of those legato edges, 96.8% descend (tap, then pull-off) and 99.6% stay
    on the string.
  - 14.8% land on an open string.

## Phase 2 — ablation (corrected import)

`fingering_gap legato`. Primary weights `v1-fit`, whole corpus: 226 tapped
lines, 25,571 notes, 5,260 of them tapped.

**Controls:**

- lines without legato edges: 0 of 12,814 (lines × weight sets) differ
  between B and any legato stage;
- untapped lines: 0 of 17,480 differ between A and B.

### Results (`v1-fit`, whole corpus)

The baseline is the untapped pool under **the same stage objective**,
reweighted to the slice's line lengths; its value is in parentheses.

| stage | human path in optimum set | exactness gap to baseline | excess per note | agreement | on tapped notes | ceiling | unique optimum |
|---|---|---|---|---|---|---|---|
| A tap-blind | 0.0% | 20.0 pt (20.0%) | 3.20 (1.16) | 38.5% (44.9%) | 29.0% | 44.1% | 14.2% |
| B tap-aware | 0.9% | 19.1 pt (20.0%) | 1.63 (1.16) | 43.1% (44.9%) | 45.3% | 52.3% | 3.1% |
| **C1 hard continuity** | **11.5%** | **9.3 pt** (20.8%) | 0.69\* (0.97) | 45.7% (44.9%) | 50.7% | 51.4% | 28.8% |
| C2 soft, k = 1 | 3.1% | 17.2 pt (20.2%) | 1.46 (1.14) | 46.7% (44.9%) | 52.5% | 52.9% | 14.6% |
| **C2 soft, k = 3** | **8.4%** | **12.0 pt** (20.4%) | 1.25 (1.10) | 47.6% (45.0%) | 51.9% | 53.3% | 28.8% |
| C2 soft, k = 10 | 10.6% | 10.1 pt (20.7%) | 0.94 (1.04) | 46.5% (45.0%) | 50.5% | 52.0% | 37.6% |
| **D1 = C1 + waiver** | **15.0%** | **6.0 pt** (21.0%) | 0.52\* (0.94) | 52.1% (46.1%) | 53.5% | 59.7% | 27.9% |
| **D2 = C2(3) + waiver** | **8.8%** | **11.7 pt** (20.5%) | 1.08 (1.07) | 45.7% (45.0%) | 48.4% | 53.2% | 27.0% |

\* Over 207 lines. The other 19 tapped lines have more cross-string legato
edges in the tab than the hard optimum; they are not exact and have no
defined excess.

Where the optimum put a legato edge across strings (lines):

| stage | lines |
|---|---|
| A | 209 of 226 |
| B | 212 |
| C2(3) | 163 |
| C1 | 2 (edges the constraint cannot avoid) |

**Per format family** (exactness, then the gap to the same-format baseline):

| stage | GP3–5 (142 lines) | GP6/7 (84 lines) |
|---|---|---|
| B | 1.4%, gap 17.0 pt | 0.0%, gap 19.8 pt |
| C1 | 12.0%, gap 7.0 pt | 10.7%, gap 10.1 pt |
| C2(3) | 7.7%, gap 10.9 pt | 9.5%, gap 11.0 pt |
| D1 | 15.5%, gap 3.7 pt | 14.3%, gap 6.7 pt |
| D2 | 9.9%, gap 8.8 pt | 7.1%, gap 13.4 pt |

### Leave one song out (`v1-fit`, whole corpus)

- **Δ** is the change in the slice's exactness, in points.
- **Min / max** give Δ recomputed with each song key's lines removed.
- **Share** is the largest single song's share of the net line gain. It can
  exceed 100% when other songs lose lines.
- **Evidence** applies the registered rule: Δ > 0 on the full subset and
  under every removal.

All tapped lines (226 lines, 58 songs):

| step | Δ | min / max | share | evidence |
|---|---|---|---|---|
| A → B | +0.9 | +0.0 / +1.1 | 100% | no |
| **B → C1** | +10.6 | +7.3 / +13.3 | 33% | **yes** |
| **B → C2(3)** | +7.5 | +5.6 / +9.4 | 35% | **yes** |
| **C1 → D1** | +3.5 | +1.8 / +4.4 | 50% | **yes** |
| C2(3) → D2 | +0.4 | −0.9 / +1.4 | 300% | no |

GP3–5 (142 lines, 36 songs):

| step | Δ | min / max | share | evidence |
|---|---|---|---|---|
| A → B | +1.4 | +0.0 / +1.8 | 100% | no |
| **B → C1** | +10.6 | +5.2 / +12.6 | 53% | **yes** |
| **B → C2(3)** | +6.3 | +2.6 / +7.6 | 67% | **yes** |
| **C1 → D1** | +3.5 | +1.5 / +4.4 | 60% | **yes** |
| C2(3) → D2 | +2.1 | +0.0 / +2.6 | 100% | no |

GP6/7 (84 lines, 27 songs):

| step | Δ | min / max | share | evidence |
|---|---|---|---|---|
| A → B | +0.0 | +0.0 / +0.0 | — | no |
| **B → C1** | +10.7 | +7.7 / +14.8 | 33% | **yes** |
| **B → C2(3)** | +9.5 | +6.4 / +13.1 | 38% | **yes** |
| C1 → D1 | +3.6 | −1.3 / +5.1 | 133% | no: concentrated case evidence |
| C2(3) → D2 | −2.4 | −3.3 / +0.0 | — | no |

The B → C1 gain spreads over 8 songs; none loses a line, and the largest
contributes 8 of the 24 lines. The C1 → D1 gain comes from 3 songs, all by
one band; one other song loses a line. It passes the registered rule on the
whole slice and in GP3–5 but rests on few songs.

### Production `v1` (secondary)

- **Hard continuity.** Exactness goes from B 0.9% to C1 **17.7%**, above its
  baseline (14.7%, gap −3.0 pt). B → C1 is corpus evidence in all three
  subsets (all +16.8 pt, leave-one-song-out +9.6 / +18.9).
- **Soft continuity.** C2(3) reaches 2.2% (+1.3 pt, not corpus evidence).
  The soft penalty `3 · position_shift` is small against `v1`'s other terms.
- **Waiver.** D equals C line by line: the waiver lifts only penalties, and
  `v1` has an open-string bonus.

### Holdout songs

30 tapped lines, reported, not interpreted. `v1-fit` exactness: B 6.7%,
C1 16.7%, D1 26.7%, C2(3) 13.3%, D2 13.3%.

## Reading

1. **Observed legato continuity explains a large, robust part of the
   exactness residual.** Hard same-string continuity across observed legato
   edges halves the gap to comparable untapped lines (19.1 → 9.3 pt, `v1-fit`,
   whole corpus). It is corpus evidence by the registered rule in both format
   families and under both weight sets. Under B, the optimum put some legato
   edge across strings in 212 of 226 tapped lines. H1 (tapping figures kept on
   one string) was a real blind spot of the objective, not a hunch.
2. **Hard fits better than soft at this law level (99.6%).** Soft continuity
   approaches hard as `k` grows (3.1% → 8.4% → 10.6%, against 11.5%). The
   registered `k = 3` closes 37% of the gap, and under production `v1` almost
   nothing. The hard constraint's cost is 19 tapped lines (8.4%) that it can
   never reach: their tabs cross strings on 61 legato edges, spread over 8
   songs.
3. **The pull-off → open-string interaction (derived direction) adds to hard
   continuity, not to soft.**
   - **On top of C1,** it closes another third of the remaining gap
     (9.3 → 6.0 pt), raising agreement from 45.7% to 52.1% and the ceiling
     from 51.4% to 59.7%. By the registered rule it is corpus evidence on the
     whole slice and in GP3–5, but its gain comes from 3 songs of one band,
     and in GP6/7 it is concentrated case evidence.
   - **On top of soft continuity,** it adds nothing (+0.4 pt) and lowers
     agreement (47.6% → 45.7%). A plausible reading, not tested here: under
     soft continuity a waived open string can be reached by crossing strings,
     which the hard constraint forbids.
4. **What remains is closeness without exactness.** Under D1 the slice sits
   6.0 pt below its baseline in exactness (15.0% against 21.0%). Its excess
   per note is below the baseline's (0.52 against 0.94, over lines with a
   defined excess), and its agreement is above (52.1% against 46.1%).
5. **Tap attribution alone never moved exactness.** A → B changes 2 lines
   from one song and is not corpus evidence, consistent with stage 1.

## Limitations

- **Oracle labels.** Tapping and legato labels come from the tab.
  MIDI-sourced lines carry neither, so these gains assume the labels.
- **Import limits.** Legato is imported as an origin only, and D's direction
  is derived from pitch.
- **Weights.** `v1-fit` was fitted on all lines and reused unchanged. `k` and
  the waiver were fixed before the results.
- **Hard constraint.** 19 tapped lines are unreachable under it.
- **Concentration.** The C1 → D1 gain rests on 3 songs.
- **Holdout.** The holdout slice (30 lines) is too small to interpret.

## Follow-ups proposed (not done)

1. **Hidden technique inference must predict legato edges, not only taps.**
   Continuity carries half of the exactness effect, so a MIDI-side model
   without legato labels would lose it.
2. **Inspect the 19 hard-constraint violators** before choosing between hard
   continuity with an exception budget and soft continuity with a large `k`.
   Guitar Pro stores legato "to the next note on this string", which need not
   be the next onset.
3. **Hammer-on / pull-off direction.** D's derived direction argues for the
   separate core decision on importing or deriving direction for all formats.
