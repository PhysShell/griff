# griff-constraint-lab — offline Constraint Lab spike

The first increment of the hard-constraint-contract proposal
([`../docs/proposals/hard-constraint-contract.md`](../docs/proposals/hard-constraint-contract.md)),
modelling the two problems selected by the Constraint Inventory
([`../docs/proposals/constraint-inventory.md`](../docs/proposals/constraint-inventory.md)).
Results are archived in
[`../docs/audit/2026-07-constraint-oracle-spike.md`](../docs/audit/2026-07-constraint-oracle-spike.md).

**Isolation.** Deliberately not a member of the parent workspace (root
`Cargo.toml` `exclude`, the `fuzz/` precedent from ADR-0010): research
tooling only. CI, production builds, and `deny.toml` never see this crate;
its only path dependency is `griff-core`, read-only.

## What it does

```text
typed problem ──► solver-neutral IR ──► MiniZinc .mzn (emitted, archived)
                        │
                        └────► exact reference solver (deterministic backtracking)
                                        │
                       manifests: runs/*.json (solver identity, outcome, witness)
```

- **Problem A — bounded-travel fretboard realization** (*experimental*):
  fret domains from `Tuning::candidates`, `|fret_i − fret_{i+1}| ≤ bound`.
  Explicitly not a check on `infer_positions` and not the production
  reachability rule — it measures where a hypothetical hard travel bound
  would bite (calibration evidence only).
- **Problem B — complement pair cleanliness** (existing rule set): part-A
  playability as a precondition, playable-pitch domain filter, the
  production dissonance classes `{1, 6, 11}` on coincident onsets, and the
  exact `band_overlap > 1/2` register-mud law with its degenerate
  single-pitch rule.

## Running

```sh
cd lab
cargo test                 # the contract suite
cargo run --bin oracle     # reference solver only
MINIZINC_BIN=/path/to/minizinc cargo run --bin oracle   # + external cross-check
```

The runner emits `fixtures/*.mzn`, archives `runs/*.json`, and exits
non-zero if the external solver disagrees with the reference solver on any
fixture. The external solver is optional, but the archive never lies about
it: a reference-only or failed external run deletes the corresponding
`*.minizinc-chuffed.json` manifest and makes no agreement claim — external
evidence on disk always belongs to the run that produced it. The committed
manifests record both `griff-lab-exact` and `minizinc/chuffed` runs
(frontend and backend versions separately).

## Optimization phase — fingering gap

The SAT/UNSAT IR answers "does an admissible realization exist?". The
optimization IR (`src/optir.rs`) adds an objective — binary hard tables plus
unary/pair cost tables and weighted `|a − b|` / `[a ≠ b]` terms — so an
external solver can report the *best* admissible realization. The solver is
untrusted: `optir::verify_record` accepts an optimum only when the solver
proved it, the witness is admissible, and the in-repo re-score equals the
claim.

First subject (`src/fingering.rs`, `src/bin/fingering_gap.rs`): monophonic
fingering, measured two ways —

- **against an external optimum**: the production objective (`v1`, mirrored
  independently of `infer_positions`) and an experimental hand-position
  model are exported as IR and solved by OR-Tools CP-SAT
  (`cpsat/solve_opt.py`); the in-repo DPs are compared with the verified
  optima, and a lexicographic agreement pass gives the tie-insensitive
  ceiling of each model's agreement with the tab author;
- **against human tablature**: per-note agreement with Guitar Pro tabs,
  how often the human fingering is itself optimal under a model, and by how
  much it is not — with song-level holdout for fitted weights.

```sh
cd lab
cargo build --release --bin fingering_gap
T=path/to/gp/tabs; O=out            # out/ is git-ignored (ADR-0005)
./target/release/fingering_gap fit    --tabs $T --out $O
./target/release/fingering_gap export --tabs $T --out $O --v1 v1=1,1,2,1 --hand hand-fit=0,0,0,2,1,3
python -m pip install ortools        # any venv
python cpsat/solve_opt.py $O/v1.problems.jsonl $O/v1.cpsat.jsonl --agreement
./target/release/fingering_gap report --tabs $T --out $O --v1 v1=1,1,2,1 --hand hand-fit=0,0,0,2,1,3
```

Everything written to `--out` is corpus-derived and stays local; `report`
archives aggregates only (`report.json`). Results:
[`../docs/audit/2026-09-fingering-optimality-gap.md`](../docs/audit/2026-09-fingering-optimality-gap.md).

## Optimum sets and the learned tie-break

`src/ties.rs` measures a fingering objective's optimum set exactly with chain
DPs — how many optimal paths, and the least, most and expected (uniform draw)
agreement with the tab author among them — and learns a human-blind secondary
objective that breaks ties lexicographically after the primary cost
(averaged, loss-augmented, latent-target perceptron). `ties-check` compares
the DPs with verified CP-SAT records; `tiebreak` reports the ladder
floor → uniform → production tie-break → learned → ceiling on holdout songs,
with and without the line's hand anchor.

```sh
./target/release/fingering_gap ties-check --tabs $T --out $O --v1 v1-fit=0,-3,1,0
./target/release/fingering_gap tiebreak   --tabs $T --out $O --v1 v1-fit=0,-3,1,0
```

Results: [`../docs/audit/2026-09-fingering-tie-break.md`](../docs/audit/2026-09-fingering-tie-break.md).

## Known spike limits (deliberate)

- The reference solver is leaf-checked backtracking with two sound band
  prunes — adequate for spike-sized fixtures, not a propagation engine.
- Problem A tracks fret distance only (mirroring `max_fret_jump`); string
  choice is free and unrecorded in the witness.
- One shared B domain per problem; per-onset domains are a Lab follow-up.
