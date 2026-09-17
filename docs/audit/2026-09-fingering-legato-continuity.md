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
