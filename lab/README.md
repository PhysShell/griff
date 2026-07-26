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

## Known spike limits (deliberate)

- The reference solver is leaf-checked backtracking with two sound band
  prunes — adequate for spike-sized fixtures, not a propagation engine.
- Problem A tracks fret distance only (mirroring `max_fret_jump`); string
  choice is free and unrecorded in the witness.
- One shared B domain per problem; per-onset domains are a Lab follow-up.
