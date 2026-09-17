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
