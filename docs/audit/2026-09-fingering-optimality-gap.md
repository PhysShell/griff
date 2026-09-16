# 2026-09 — Fingering optimality gap (Constraint Lab optimization phase)

The July oracle spike ([`2026-07-constraint-oracle-spike.md`](2026-07-constraint-oracle-spike.md))
gave the Constraint Lab a SAT/UNSAT phase: *does an admissible realization
exist?* This increment adds the next question, borrowed from SLOTHY
(assembly superoptimization with CP-SAT, <https://eprint.iacr.org/2022/1303>):
*what does the **best** admissible realization cost, and how far from it is
what Griff produces?* — measured on monophonic fretboard fingering, against
two references: an external proven optimum and the tab authors of a real
Guitar Pro corpus.

Research tooling only. `lab/` stays outside the workspace; no production
code, dependency, or `deny.toml` changed; OR-Tools lives in a local venv.

## Correction (2026-09-17) — GP7 pitches were wrong in the data

Chasing this audit's side finding exposed a larger import defect, fixed in
PhysShell/griff#198: GP7 (`.gp`) tracks imported a fallback Standard E
tuning while their notes stayed numbered from the low string, so **nearly
every GP7 note in the corpus had the wrong pitch** (0.2% matched the GPIF
`Midi` property). Positions stayed consistent with the wrong tuning, which is
why no check in this audit caught it. Every number in Results 1–4 below was
measured *before* that fix.

Re-measured on the fixed importer (same corpus and protocol; 1,149 guitar
tracks, 9,045 lines, 326,130 notes; 7,091 train / 1,954 holdout lines; weights
refitted on train songs):

| model | agreement, holdout (before → after) | human fingering optimal, holdout (after) |
|---|---|---|
| lowest fret | 31.8% → 33.5% | — |
| v1 production | 34.0% → 35.8% | 19.2% |
| v1-fit (after: fret 0, open +3 penalty, shift 1, string 0) | 40.1% → 44.1% | 30.9% |
| hand-fit (after: height 0, open 1, stretch 2, shift 0, shift_distance 2, string_distance 2) | 40.6% → 44.3% | 37.7% |

CP-SAT on the holdout lines, fixed importer (every record verified):

| model | proven | DP gap = 0 | DP agreement | ceiling at the optimum |
|---|---|---|---|---|
| v1 | 1,954 / 1,954 | 1,954 | 35.8% | **36.2%** |
| v1-fit | 1,954 / 1,954 | 1,954 | 44.1% | **55.5%** |

What changes and what does not:

- **Unchanged:** the solver gap is zero; the production weights barely beat
  a lowest-fret heuristic; v1's optimal set caps agreement near its DP value
  (36.2% vs 35.8%), so the objective is the limit.
- **Stronger:** the flat-objective reading of v1-fit — its optimal set
  contains 55.5% agreement against the DP's 44.1%, an 11.4-point tie-break
  loss.
- **Shifted:** fitted weights and absolute agreement (+2 to +4 points on
  every model). The fitted hand model no longer beats fitted v1 on holdout
  agreement (44.3% vs 44.1%); its advantage remains the human-optimal rate.
- **Not re-run:** the repeat-consistency experiment (Result 3) and the
  hand-model oracle (Results 1 and 4). Neither conclusion depends on pitch
  correctness in an obvious way, but their numbers are pre-fix.

## What was built

`lab/` (`griff-constraint-lab`), TDD red → green per commit:

- **Optimization IR** (`src/optir.rs`) — finite integer variables, binary
  hard tables, and an integer objective of unary/pair cost tables and
  weighted `|a − b|` / `[a ≠ b]` terms; canonical tables and fingerprints.
  The external solver is untrusted: `verify_record` accepts an optimum only
  when the solver proved it, the witness is admissible, the in-repo re-score
  equals the claim, and the bound equals the objective; `verify_agreement`
  recounts the agreement pass and pins it to the optimum.
- **Fingering subject** (`src/fingering.rs`):
  - `tab_lines` — monophonic runs of a GP track with the tab author's
    `(string, fret)`; every refused note counted by cause; tracks always
    emitted with string 1 = highest (see *Side finding*);
  - `v1_cost` / `v1_problem` — the production `infer_positions` objective,
    re-implemented independently and pinned against the production DP by
    exhaustive brute force;
  - `HandModel` — the finger-span layer ADR-0019 §7 defers: a hidden
    index-finger position with a four-fret box, one-fret stretches, shift
    event + shift distance, string distance, neck height, open strings.
    `best_hands` scores fixed positions (a human tab) exactly; `solve_hand`
    is the exact joint DP with a factored `O(K²H + KH²)` step; both pinned
    against brute force;
  - `repeat_pairs` / `with_repeat_consistency` / `with_string_tiebreak` —
    the global constraint experiment below.
- **Runner** (`src/bin/fingering_gap.rs`): `fit`, `export`, `report`,
  `repeat-export`, `repeat-report`. `report` rebuilds every problem from the
  tabs and verifies every solver record before counting it.
- **CP-SAT adapter** (`cpsat/solve_opt.py`, OR-Tools 9.15.6755): proven
  optimum, bound, witness; a lexicographic agreement pass
  (`min scale·cost − matches`); two-tier solving (pool of single-worker
  solves with a 5 s limit, then a 16-worker portfolio with 300 s for anything
  unproven — the tier is recorded in the solver identity).

## Corpus and protocol

- 410 Guitar Pro files (GP3/4/5/6/7, swancore-first), 407 imported, 1,177
  guitar tracks (`select_ingest_tracks`). Content fingerprint
  `9e53e55a19cddf29`; tabs are licensed material and never enter git —
  problems, solver records and per-line data stay in the git-ignored
  `lab/out/` (ADR-0005). Only aggregates are recorded here.
- Line cut (`LineCut::v1`): single-note onsets with explicit positions,
  ≥ 4 notes; chords, unpositioned notes, positions above fret 24,
  pitch/position mismatches and rests ≥ 4 quarters end a line. Of 1,216,035
  note atoms, **331,670 (27%) sit in 9,150 kept lines**; the rest are chord
  onsets (292,992 onsets), frets above 24 (11,453 notes, mostly 35/36/99
  placeholders in non-guitar parts), short runs (17,376 notes), unpositioned
  (64). Pitch/position mismatches: 0. 156 tracks were mirrored.
- **Song-level holdout**: `holdout_bucket(song_key, 5) == 0` is test —
  208 train / 51 test songs, 7,196 / 1,954 lines, 260,737 / 70,933 notes;
  arrangements of one song (`(ver 2 by …)`) share a key.
- Machine-generated tabs would inflate agreement (prior art warns about
  DadaGP): files with ≥ 50 kept notes that agree ≥ 99% with a lowest-fret
  baseline — **1 of 385**.

## Result 1 — the production DP has no optimality gap

`infer_positions` is an exact Viterbi for its own objective, so the
SLOTHY-style gap was expected to be zero; the Lab now proves it at corpus
scale instead of assuming it.

| model | lines | proven by CP-SAT | verified | gap = 0 | gap > 0 | gap < 0 | escalated |
|---|---|---|---|---|---|---|---|
| v1 (production weights) | 9,150 | 9,150 | 9,150 | **9,150** | 0 | 0 | 519 |
| v1-fit | 9,150 | 9,150 | 9,150 | **9,150** | 0 | 0 | 748 |
| hand-fit (in-repo `solve_hand`) | 1,954 (holdout) | 1,930 | 1,930 | **1,930** | 0 | 0 | 350 |

For the hand model, 24 holdout lines (1.2%) stayed unproven after the
16-worker, 60 s escalation; no claim is made for them.

`gap < 0` would mean the IR encoding and the in-repo evaluator disagree; it
is a defect detector and stayed empty. Every optimum above was re-scored in
the repo before it counted. For the hand model the oracle comparison is a
differential test of the new DP against an independent declarative model;
the DP is additionally pinned against brute force in the contract suite.

## Result 2 — the model gap to human tablature is large

Per-note agreement with the tab author (string and fret), holdout songs:

| model | weights | agreement (test) | all | exact lines (test) | human fingering optimal (test) | human excess p50 / p90 (test) |
|---|---|---|---|---|---|---|
| lowest fret (baseline) | — | 31.8% | 35.5% | 20.7% | — | — |
| v1 production | fret 1, open bonus 1, shift 2, string 1 | 34.0% | 36.7% | 18.9% | 19.0% | 45 / 489 |
| v1-fit | fret 0, open +4 penalty, shift 1, string 0 | 40.1% | 42.4% | 11.5% | 22.1% | 10 / 76 |
| hand-fit | height 0, open 0, stretch 0, shift 2, shift_distance 1, string_distance 3 | **40.6%** | 46.2% | 22.3% | 35.4% | 6 / 69 |

(v1 fields: cost = `fret·w − [open]·open_string`; `open_string = −4` is a
penalty of 4 per open string. v1-fit: exhaustive grid of 2,205 integer
weight sets on train songs; hand-fit: coordinate descent from three starts,
366 weight sets evaluated.)

**Reading the last two columns across models.** Only the agreement columns
compare models on a common scale.

- *Human excess* is in each model's own cost units and is **not comparable
  across rows**: v1 charges every note its fret number, so a tab author
  playing at the 12th fret pays 12 per note before any movement, while the
  fitted models have no per-note term at all. The drop from 45 to 6 mostly
  reflects weight scale, not a better account of human choices.
- *Human fingering optimal* is scale-free but inflated by flat objectives:
  the more fingerings tie at the optimum, the easier it is for the human one
  to be among them.
- Neither is a training target: all-zero weights make every fingering
  optimal (excess 0, 100% optimal). A scale-free diagnostic — the human
  path's rank among candidates, or `(human − optimum) / (baseline −
  optimum)` — is follow-up work; agreement on holdout songs remains the only
  unbiased target used here.

- The production weights barely beat "always the lowest fret" (34.0% vs
  31.8%) and make the human fingering optimal in only 19% of lines.
- Fitted on train songs only, both families gain ~6 points on unseen songs.
  The fit is a local optimum over small integer grids, not a claim about the
  best achievable weights.
- The hand model makes the human fingering optimal in 35% of test lines
  (v1: 19%, v1-fit: 22%) while agreement moves only to 40.6%. Part of that
  rise is the flatness caveat above, so it is a hint that the hand terms
  describe human choices better, not a measurement of how much better.
- Tab authors avoid open strings: every fit turned the open-string bonus
  into a penalty or zero. The hand model charges a crossed string three
  times a fret of hand travel; the v1 family, which sees only whether the
  string *changed*, set that weight to 0 — the distance, not the change,
  carries the signal.

**Tie-breaking is not the problem.** The agreement pass maximizes agreement
over *all* cost-optimal fingerings, so it is the ceiling any tie-break could
reach:

| model | DP agreement (all lines) | ceiling at the optimum |
|---|---|---|
| v1 | 36.7% | 37.2% |
| v1-fit | 42.4% | **52.3%** |

(The ceiling uses the human tab to break ties, so it is an upper bound, not
a predictor.)

- **v1: the objective is the limit.** Even the most human-like of all
  v1-optimal fingerings agrees on only 37.2% of notes; no tie-break or
  search improvement can recover the rest. The weights have to change.
- **v1-fit: the objective is too flat.** With zero fret and string-change
  weights, many fingerings tie, and the DP's tie-break loses 10 points
  against what the optimal set contains. The fitted weights moved toward
  humans but stopped distinguishing choices a guitarist does distinguish —
  the missing terms, not the search, are the next gain.
- The hand model's agreement pass is the same lexicographic problem shape
  and was not tractable in the session's budget (Result 4).

## Result 3 — a global constraint the chain DP cannot hold

Tab authors finger a repeated 6-note figure identically in **98.5%** of
23,635 repeat pairs (2,468 lines; single-pitch ostinati excluded); the
chain DPs do so in 84.8% (v1-fit) and 88.7% (hand-fit), because entry and
exit context pull repeats apart. Equality between distant notes is out of
reach of a first-order DP state, but it is a handful of equal-value tables in
the IR. Both variants use a deterministic string tie-break
(`with_string_tiebreak`) so the witnesses are comparable; the tie-broken
solver reproduced the DP's choices exactly (identical agreement and
consistency counts), so the constrained column differs from the DP by the
constraint alone.

v1-fit, holdout songs, lines with a repeated figure (510 lines, 56,455 notes,
5,433 repeat pairs; every record verified):

| | tab author | chain DP | solver (tie-break) | solver + repeat consistency |
|---|---|---|---|---|
| repeat pairs fingered identically | 98.2% | 82.2% | 82.2% | **100%** (enforced) |
| per-note agreement with the tab author | — | 41.3% | 41.3% | **40.2%** |

- The constraint is cheap: it raised the model cost in 177 of 510 lines,
  by a median of 0 and p90 of 3 cost units (max 108).
- **It does not move the model toward the tab author** — agreement drops by
  1.1 points. The human fingering satisfies the constraint in 460 of 510
  lines, but the cheapest *consistent* fingering under v1-fit is usually
  not the human one. Consistency is a property humans have, not a cause of
  their choices; with an objective this far from human preference, pinning
  a human-true invariant does not import the preference.
- This is the SLOTHY lesson in its honest form: the solver can hold global
  structure a DP cannot, but it optimizes whatever objective it is given.
  The experiment isolates the objective — not the search, and not the
  constraint vocabulary — as the component that limits Griff's fingering.

The same experiment on the hand model was stopped: under the tie-break
scaling, 354 of 510 holdout lines were still unproven after the 5 s
first tier (Result 4).

## Result 4 — what the oracle costs

- In-repo DPs over all 331,670 notes: v1 in single-digit milliseconds, the
  hand model in ~120 ms (16 threads).
- CP-SAT per line (recorded wall time of the optimality solve, model build
  included, agreement pass excluded; for escalated lines the accepted
  multi-worker re-solve):

  | problem | lines | first tier unproven (5 s, 1 worker) | p50 | p99 | max | total |
  |---|---|---|---|---|---|---|
  | v1 | 9,150 | 519 (5.7%) | 19 ms | 2.8 s | 19 s | 1,507 s |
  | v1-fit | 9,150 | 748 (8.2%) | 10 ms | 1.4 s | 142 s | 939 s |
  | v1-fit + tie-break (repeat lines) | 510 | 60 (12%) | | | | 400 s |
  | v1-fit + tie-break + repeat consistency | 510 | 33 (6.5%) | | | | 342 s |
  | hand-fit + tie-break (repeat lines) | 510 | **354 (69%)** | | | | stopped |
  | hand-fit (holdout, no agreement pass) | 1,954 | 350 (18%) | 61 ms | 60 s (limit) | 61 s | 3,676 s |

- With one worker and a 120 s limit, ~2% of v1 lines (80–357 notes) stayed
  unproven; a 16-worker portfolio proved the same lines in 0.2–1.1 s, while
  a tighter local-marginal encoding with one worker still timed out on one
  of five — search strategy, not formulation, was the bottleneck.
- The hidden hand position (21 values per note) and any lexicographic
  scaling (tie-break, agreement pass) push CP-SAT from milliseconds to
  tens of seconds per line; the in-repo DP is unaffected by either. The
  repeat constraint, by contrast, made proofs *easier* (it prunes).
- The first full-corpus hand-model run with the agreement pass was stopped
  after 1.5 h in its escalation tier; the holdout run without it took about
  an hour of wall time for what `solve_hand` does over the whole corpus in
  ~120 ms, and still left 24 lines unproven.

The contract's standing decision holds with evidence: an external solver in
the production path would cost four to five orders of magnitude of latency
for problems a DP solves exactly, and its cost is sensitive to modelling
details a DP does not see. As an offline oracle over a sample it is cheap
enough and it found what it was asked to find.

## Side finding — GPIF imports mirror string numbering (and GP7 pitches)

156 guitar tracks — nearly all `.gpx` (GP6/GPIF) — import with a strictly
ascending tuning: string 1 is the *lowest* string, against the glossary's
string 1 = highest. Pitches stay consistent (tuning and positions are
mirrored together, which is why 9073de1's index fix did not surface it), but
anything orientation-sensitive — the DP's "lowest string first" tie-break,
tab rendering, `Tuning` equality — flips with the file format. The Lab
normalizes lines (`CutStats::mirrored_tracks`).

Following it up against the GPIF `Midi` note property found the GP7 half:
the `guitarpro` crate never reads a staff-level tuning, so GP7 tracks got a
high-first Standard E and wrong pitches (see *Correction* above). Both are
fixed at the import boundary in PhysShell/griff#198.

## Limitations (recorded, not hidden)

- Monophonic lines only: 73% of corpus notes are in chords or cut away.
  Chord voicing (inventory rule 2) remains the next oracle target.
- Agreement treats one human tab as ground truth; real alternatives exist,
  and GP authoring (copy-paste) inflates repeat consistency.
- Techniques are ignored: tapping, slides, legato and harmonics change what
  "hand position" means, and swancore uses them heavily.
- Weights were fitted by agreement with small integer grids and coordinate
  descent; a structured-perceptron / path-difference learner is the obvious
  next step.
- CP-SAT witnesses are not deterministic across worker counts; every claim
  above rests on verified optima and recounts, not on witness identity.
- The hand model's `h` domain spans frets 1–21 with a fixed four-fret box
  and no per-finger assignment (ADR-0019 §7 remains open).

## Prior art (surveyed before the experiment)

- Human-tab agreement: MIDI-to-Tab (ISMIR 2024) reports Guitar Pro 8 at
  62.3%, MuseScore 62.5%, a transformer 73.6% string agreement on 8,451
  held-out jazz notes (<https://arxiv.org/html/2408.05024v1>);
  Fretting-Transformer (2025) reports a lowest-fret heuristic at 58.1% on
  Leduc and 79.2% on DadaGP (<https://arxiv.org/html/2506.14223>). Neither
  evaluates a DP of Griff's shape; swancore position playing is harder for
  every heuristic.
- Hand-position costs: Hori & Sagayama (ISMIR 2016, index-finger state),
  Radicioni & Lombardo (2005, neck height, string crossing, shift at phrase
  boundaries), Heijink & Meulenbroek (2002, motion capture: shifts and
  spans are costly).
- Learned weights: path difference learning (ICMC 2004).
- Solvers for fingering: TablaZinc (MiniZinc/Gecode, MPL-2.0), CPLEX in
  Bontempi et al. (2024) and Tahon (2017); no CP-SAT use found. Idea reuse
  only; no code copied.

## Follow-ups proposed

1. A `FingeringWeights` v2 decision (ADR or decisions log): the production
   weights are the weakest calibrated component measured here — they barely
   beat a lowest-fret heuristic and their optimal set caps agreement at 37%.
   The fitted v1 weights are a better starting point but too flat; the hand
   model's terms (shift event vs distance, string distance) belong in the
   candidate v2 cost.
2. Replace grid/descent fitting with path-difference learning (a structured
   perceptron over the same features), still scored on holdout songs, with
   the agreement ceiling as the target to close.
3. Keep global constraints (repeat consistency) as Lab instruments, not
   production rules: they are cheap for the solver and true of humans, but
   they do not substitute for a better objective. Revisit once the
   objective's ceiling rises.
4. Fix GP6 string orientation in `core/src/gp.rs` (separate change).
5. Chord voicing feasibility and optimization as the next Lab subject —
   73% of the corpus notes are outside monophonic lines.
