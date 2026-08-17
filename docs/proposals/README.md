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
- [`constraint-inventory.md`](constraint-inventory.md) — fifteen rules
  catalogued against what the code already enforces, classified under a
  closed taxonomy (hard / soft / opt-in / defer) with scope, lookback,
  evidence, and incremental-evaluation notes; selects the two real
  problems for the MiniZinc oracle spike. Companion artifact to the
  hard-constraint-contract proposal. Status: for discussion.
- [`hard-constraint-contract.md`](hard-constraint-contract.md) — one typed
  rule layer separating legality from preference, plus MiniZinc as a strictly
  offline oracle (Constraint Lab). Status: for discussion.
- [`preference-and-similarity-learning.md`](preference-and-similarity-learning.md)
  — the staged, benchmark-gated order in which ML enters Griff: preference
  evidence, retrieval-only embeddings, a human similarity benchmark, then a
  bounded linear reranker. Feeds S9. Status: for discussion.
- [`human-similarity-benchmark.md`](human-similarity-benchmark.md) — the
  evidence contract, sampling protocol, session procedure, presentation
  contract, and paired-clustered-difference gate for the four separate
  benchmark tasks (similarity / variation / complementarity / copy-detection),
  each gated against a named handcrafted baseline or, where none exists
  (complementarity), an explicitly declared floor. Companion artifact to the
  preference-and-similarity-learning proposal. Status: for discussion.
- [`transactional-editing-and-candidate-terrain.md`](transactional-editing-and-candidate-terrain.md)
  — technique modules, a deterministic symbolic candidate terrain, and a
  transactional agent-editing contract for the cockpit. Status: for
  discussion.
- [`song-curation-slice-2-transactional-apply.md`](song-curation-slice-2-transactional-apply.md)
  — the acceptance contract for ADR-0033 Slice 2 (transactional Apply): exact
  inputs/outputs, application-index and report v1 schemas, verification
  ordering, chain equations, filesystem/transaction semantics, preservation
  law, closed refusal taxonomy, and the preregistered RED→GREEN matrix.
  Status: proposed — awaiting independent acceptance; implementation
  prohibited until accepted (ADR-0033 Decision 10).
- [`song-id-curation-workflow.md`](song-id-curation-workflow.md) — an offline,
  human-confirmed workflow (inventory → suggest → confirm → plan → apply →
  manifest → validate) for assigning `SongId` (ADR-0031) to corpus sources by
  `sha256`, with suggestions held strictly non-authoritative and a typed
  refusal taxonomy; recommends a standalone `song-curation/` tool and a
  four-slice implementation gated behind a new ADR. Status: historical context; durable decisions accepted in ADR-0033.
