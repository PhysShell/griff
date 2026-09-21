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

- **Edges.** Guitar Pro attaches a legato flag to the origin note. Its target
  is the first strictly later note in the same imported voice on the same
  original string. Projection is performed over the whole imported voice,
  before `TabLine` slicing; simultaneous notes never target each other.
  `TabLine::edges` stores sparse `(from, to, kind)` relations whose two notes
  survive in the same line, so an edge may skip onsets on other strings.
- **Loss accounting.** An origin with no later note on its original string is
  *unresolved*. A resolved target outside the origin's kept `TabLine` is
  counted separately as *cross-line* and is not silently rebound to another
  note.
- **Derived direction.** Direction is computed from pitch:
  - higher target pitch: ascending (hammer candidate);
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
| — | counts: legato origins in lines, realized edges, unresolved origins, cross-line targets | — |

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
4. **The original stage-2 edge projection was falsified and repaired.** It
   bound every origin flag to the immediate next onset in the voice. The
   forensic manifest added in #206 showed that all 61 apparent cross-string
   edges in 19 tapped lines had a later note on the origin string. They are
   therefore projection regressions, not exceptions to guitar physics. The
   census and ablation below were rerun after projecting to that same-string
   target; no objective, weight, stage or metric changed.

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

## Phase 1 — legato census (corrected import and projection)

`fingering_gap legato-census`, rerun after the same-string projection fix.

- **Imported kinds.** All 29,758 within-line legato edges arrive as
  `HammerOn`; `PullOff` and `Legato` number 0, and direction is derived.
- **Projection accounting.** There are **0 unresolved origins** and **46
  cross-line targets**. Including those cross-line relations gives 29,804
  resolved origins, the same total population as before the repair.
- **Regression result.** All 29,758 within-line edges are same-string. The 61
  old cross-string edges are gone, and `legato-violators.jsonl` is empty.

**L1 — one string across the edge** (whole corpus). Share of edges whose two
notes lie on the same string, with the edge count in parentheses:

| family | population | legato edges | plain edges (base rate) |
|---|---|---|---|
| GP3–5 | all lines | 100.0% (18,078) | 59.8% (192,656) |
| GP3–5 | tap slice | 100.0% (5,953) | 51.0% (9,427) |
| GP6/7 | all lines | 100.0% (11,680) | 56.0% (97,825) |
| GP6/7 | tap slice | 100.0% (4,046) | 58.4% (5,968) |
| all | all lines | **100.0%** (29,758) | 58.5% (290,481) |
| all | tap slice | **100.0%** (9,999) | 53.8% (15,395) |

Holdout songs: 100.0% of 5,887 legato edges (all lines) and 100.0% of 1,270
(tap slice).

**L2 — per derived direction** (whole corpus, all formats):

| population | ascending | descending | unison |
|---|---|---|---|
| all lines | 100.0% (12,856) | 100.0% (16,860) | 100.0% (42) |
| tap slice | 100.0% (3,502) | 100.0% (6,491) | 100.0% (6) |

**L3 — open-string target of a descending edge** (whole corpus):

| family | population | legato edges | plain edges (base rate) |
|---|---|---|---|
| GP3–5 | all lines | 30.7% (10,299) | 11.5% (61,341) |
| GP3–5 | tap slice | 19.8% (3,855) | 8.1% (2,870) |
| GP6/7 | all lines | 33.6% (6,561) | 11.1% (32,551) |
| GP6/7 | tap slice | 34.6% (2,636) | 9.6% (1,714) |
| all | all lines | **31.8%** (16,860) | 11.3% (93,892) |
| all | tap slice | **25.8%** (6,491) | 8.7% (4,584) |

Holdout songs:

- all lines: 14.2% (3,037) against 9.5% (20,388);
- tap slice: **5.1% (836) against 6.2% (656)**, the opposite direction.

**L4 — tap-adjacent edges** (whole corpus; every tapped note lies in the tap
slice). "Legato" columns count only the legato edges among them.

| family | edges out of a tapped note | legato share | legato: one string | legato: ascending / descending / unison | legato: open target | edges into a tapped note | legato share | legato: one string |
|---|---|---|---|---|---|---|---|---|
| GP3–5 | 3,148 | 73.8% | 100.0% | 4.0 / 95.7 / 0.3% | 12.7% | 3,121 | 9.2% | 100.0% |
| GP6/7 | 2,059 | 75.2% | 100.0% | 1.2 / 98.8 / 0.0% | 17.9% | 2,032 | 8.6% | 100.0% |
| all | 5,207 | 74.4% | 100.0% | 2.9 / 97.0 / 0.2% | 14.8% | 5,153 | 9.0% | 100.0% |

Holdout songs: 777 edges out of a tapped note, 65.8% legato; all of those
stay on one string and descend.

**Reading** (descriptive; the tests are in phase 2):

- **L1: an observed legato origin resolves to a later note on its imported
  string.** This is now a property of the repaired projection, verified for
  every retained edge rather than an empirical 99.6% tendency. The useful
  data question becomes how strongly the inferred fingering should preserve
  the observed imported string.
- **L2: direction remains derived evidence.** The repair changes which pitch
  is the target, producing 12,856 ascending, 16,860 descending and 42 unison
  within-line edges.
- **L3: a descending legato edge lands on an open string about 2.8 times as
  often as a descending plain edge.** 31.8% against 11.3%; the lift appears in
  both families and in the tap slice (25.8% against 8.7%). This is the
  interaction D encodes.
  - The tap-slice holdout shows no lift (5.1% against 6.2%, 836 edges). This
    is reported, not interpreted, and it is one more reason D must pass the
    leave-one-song-out rule.
- **L4: tapping figures are legato figures.**
  - Three quarters of the source opportunities out of a tapped note have a
    projected legato relation.
  - Of those relations, 97.0% descend (tap, then pull-off); all preserve the
    imported string by construction and census verification.
  - 14.8% land on an open string.

## Phase 2 — ablation (corrected import and projection)

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
| **C1 hard continuity** | **13.3%** | **7.6 pt** (20.8%) | 0.78 (0.98) | 46.3% (45.0%) | 51.6% | 52.0% | 27.9% |
| C2 soft, k = 1 | 3.1% | 17.2 pt (20.2%) | 1.46 (1.14) | 46.7% (44.9%) | 52.7% | 52.9% | 17.3% |
| **C2 soft, k = 3** | **8.4%** | **12.0 pt** (20.4%) | 1.24 (1.10) | 47.7% (45.0%) | 52.2% | 53.4% | 29.2% |
| C2 soft, k = 10 | 12.4% | 8.3 pt (20.7%) | 0.92 (1.04) | 47.0% (45.1%) | 51.2% | 52.4% | 36.7% |
| **D1 = C1 + waiver** | **16.8%** | **4.2 pt** (21.0%) | 0.62 (0.95) | 52.2% (46.2%) | 53.3% | 60.0% | 27.0% |
| **D2 = C2(3) + waiver** | **8.8%** | **11.7 pt** (20.5%) | 1.07 (1.07) | 45.6% (45.0%) | 48.4% | 53.2% | 27.4% |

Every human path is feasible under the hard stage after the projection fix;
excess is defined over all 226 tapped lines.

Where the optimum put a legato edge across strings (lines):

| stage | lines |
|---|---|
| A | 209 of 226 |
| B | 212 |
| C2(3) | 159 |
| C1 | 0 |

**Per format family** (exactness, then the gap to the same-format baseline):

| stage | GP3–5 (142 lines) | GP6/7 (84 lines) |
|---|---|---|
| B | 1.4%, gap 17.0 pt | 0.0%, gap 19.8 pt |
| C1 | 14.8%, gap 4.3 pt | 10.7%, gap 10.1 pt |
| C2(3) | 7.7%, gap 10.9 pt | 9.5%, gap 11.0 pt |
| D1 | 18.3%, gap 1.0 pt | 14.3%, gap 6.7 pt |
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
| **B → C1** | +12.4 | +9.2 / +15.6 | 29% | **yes** |
| **B → C2(3)** | +7.5 | +5.6 / +9.4 | 35% | **yes** |
| **C1 → D1** | +3.5 | +1.8 / +4.4 | 50% | **yes** |
| C2(3) → D2 | +0.4 | −0.9 / +1.4 | 300% | no |

GP3–5 (142 lines, 36 songs):

| step | Δ | min / max | share | evidence |
|---|---|---|---|---|
| A → B | +1.4 | +0.0 / +1.8 | 100% | no |
| **B → C1** | +13.4 | +8.2 / +16.0 | 42% | **yes** |
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

The B → C1 net gain is now 28 lines; the largest song contributes 8. The
C1 → D1 gain still comes from 3 songs, all by one band; one other song loses
a line. It passes the registered rule on the whole slice and in GP3–5 but
rests on few songs.

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

### Projection forensic tail (post-fix)

`legato-census` now writes `legato-projection-forensics.jsonl`, sorted by
normalized onset gap. It contains source and target identity, timing, pitch,
position, tap state, line context, and the number of intervening voice notes
for every cross-line relation and every non-adjacent within-line relation.
The manifest is diagnostic-only and is written under `--out`; licensed corpus
content is not committed. The census fails closed if its manifest count does
not equal the importer's cross-line count.

Full-corpus rerun (410 files):

- **46 / 46 cross-line relations** were emitted, from 44 lines and 15 songs.
  31 target gaps are at most one quarter note; 22 targets are the immediate
  next voice note, while 24 skip notes on other strings. The origin is the
  last kept-line note in 26 cases; the other 20 have 1–19 later line notes on
  other strings.
- **11 cross-line gaps exceed 8 quarters.** Four are exact repetitions of one
  11-quarter figure in `Nothing Shameful`; four isolated relations exceed 32
  quarters (64.375, 75, 123.5, and 182.496875). These are suspicious stale or
  overextended source flags, but they remain outside the line-local objective,
  as cross-line relations did before this audit.
- **110 non-adjacent within-line relations** were emitted, from 25 lines and
  15 songs. 94 are at most 4 quarters. Fourteen exceed the audit threshold of
  8 quarters and three exceed 32 quarters; the maximum is 70.67 quarters.
  The 14-case tail is confined to four songs: `Say Hi` (8),
  `There's No Dust in the City` (3), `Frozen One` (2), and
  `Missed Injections` (1).
- Ten of those 14 long within-line relations occur on tapped lines. Eight are
  repeated figures in `Say Hi`; the other two are the paired tapped-string
  relations in `Frozen One`. This is a real transcription/import caveat, but
  not an explanation of the C1 result: B → C1 remains positive when either
  song is removed, and under every other leave-one-song-out removal.

No threshold from this audit enters projection or optimization. The long tail
is retained as a compact regression/forensic corpus rather than converted into
another objective exception.

## Reading

1. **Observed legato continuity explains a large, robust part of the
   exactness residual.** Hard same-string continuity across observed legato
   edges more than halves the gap to comparable untapped lines (19.1 → 7.6 pt, `v1-fit`,
   whole corpus). It is corpus evidence by the registered rule in both format
   families and under both weight sets. Under B, the optimum put some legato
   edge across strings in 212 of 226 tapped lines. H1 (tapping figures kept on
   one string) was a real blind spot of the objective, not a hunch.
2. **Hard fits better than soft after the semantic repair.** Soft continuity
   approaches hard as `k` grows (3.1% → 8.4% → 12.4%, against 13.3%). The
   registered `k = 3` closes 37% of the gap, and under production `v1` almost
   nothing. Unlike the old immediate-onset projection, the repaired edges make
   every observed human path feasible under hard continuity.
3. **The pull-off → open-string interaction (derived direction) adds to hard
   continuity, not to soft.**
   - **On top of C1,** it closes another third of the remaining gap
     (7.6 → 4.2 pt), raising agreement from 46.3% to 52.2% and the ceiling
     from 52.0% to 60.0%. By the registered rule it is corpus evidence on the
     whole slice and in GP3–5, but its gain comes from 3 songs of one band,
     and in GP6/7 it is concentrated case evidence.
   - **On top of soft continuity,** it adds nothing (+0.4 pt) and lowers
     agreement (47.7% → 45.6%). A plausible reading, not tested here: under
     soft continuity a waived open string can be reached by crossing strings,
     which the hard constraint forbids.
4. **What remains is closeness without exactness.** Under D1 the slice sits
   4.2 pt below its baseline in exactness (16.8% against 21.0%). Its excess
   per note is below the baseline's (0.62 against 0.95), and its agreement is
   above (52.2% against 46.2%).
5. **Tap attribution alone never moved exactness.** A → B changes 2 lines
   from one song and is not corpus evidence, consistent with stage 1.

## Limitations

- **Oracle labels.** Tapping and legato labels come from the tab.
  MIDI-sourced lines carry neither, so these gains assume the labels.
- **Import limits.** Legato is imported as an origin only, and D's direction
  is derived from pitch.
- **Line slicing.** 46 resolved targets lie outside their origin's kept line.
  They are counted and emitted explicitly but cannot enter a line-local
  objective. Eleven have gaps longer than the audit-only 8-quarter review
  threshold.
- **Weights.** `v1-fit` was fitted on all lines and reused unchanged. `k` and
  the waiver were fixed before the results.
- **Concentration.** The C1 → D1 gain rests on 3 songs.
- **Holdout.** The holdout slice (30 lines) is too small to interpret.

## Stage status and next targets

Stage 2 is frozen after the semantic repair, independent remeasurement, and
the projection forensic tail:

- **C1:** robust corpus evidence for hard continuity when an observed legato
  relation is available.
- **D1:** promising conditional evidence, not a canonical objective rule; its
  gain is concentrated and direction is derived from pitch.
- **No C3/C4 tuning:** the remaining tail does not justify another ladder of
  penalties or exceptions.

The next large Constraint Lab target should be **chord voicing**, extending
the guitar-specific structural work from monophonic paths to hand shapes.
**Hidden technique inference** remains the next bridge to MIDI: predict taps
and legato relations from pitches, timing, and context, then score both label
quality and downstream fingering regret against this supervised oracle.
Importing or deriving hammer-on / pull-off direction remains a separate core
decision.
