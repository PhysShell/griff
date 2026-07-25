# Proposals

Discussion drafts for work that is **not yet decided**: bigger than a
`decisions.log.md` entry, not yet an ADR, and not yet owned by a roadmap
stage. A proposal binds nothing — scope stays governed by
[`../SPEC.md`](../SPEC.md) and stage docs until the proposal is accepted.

Lifecycle: a proposal is either **rejected** (kept for the record with a
status note), or **accepted** — at which point its decisions move to their
canonical homes (an ADR in [`../adr/`](../adr/), a stage doc in
[`../stages/`](../stages/) appended per the glossary §0 rule, and/or a
`decisions.log.md` entry) and the proposal becomes historical context, not a
live contract.

## Index

- [`generator-reachability-lab.md`](generator-reachability-lab.md) — offline
  coverage census and target-relative symbolic comparison for the
  deterministic generators. Status: for discussion.
- [`pattern-operator-inventory.md`](pattern-operator-inventory.md) — sixteen
  candidate pattern operators surveyed against `griff-pattern`, the Swang
  spec, and the generator; classified under a closed four-state taxonomy
  (adopt / adapt / defer / reject) with reasons and later owners.
  Companion artifact to the pattern-processes proposal. Status: for
  discussion.
- [`reproducible-pattern-processes.md`](reproducible-pattern-processes.md) —
  bounded, seed-isolated pattern operators, control curves, and
  boundary-applied edits as a layer above the canonical score, extending the
  S16 pattern core. Status: for discussion.
- [`hard-constraint-contract.md`](hard-constraint-contract.md) — one typed
  rule layer separating legality from preference, plus MiniZinc as a strictly
  offline oracle (Constraint Lab). Status: for discussion.
- [`preference-and-similarity-learning.md`](preference-and-similarity-learning.md)
  — the staged, benchmark-gated order in which ML enters Griff: preference
  evidence, retrieval-only embeddings, a human similarity benchmark, then a
  bounded linear reranker. Feeds S9. Status: for discussion.
- [`transactional-editing-and-candidate-terrain.md`](transactional-editing-and-candidate-terrain.md)
  — technique modules, a deterministic symbolic candidate terrain, and a
  transactional agent-editing contract for the cockpit. Status: for
  discussion.
