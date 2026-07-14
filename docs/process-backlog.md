# Process backlog

Repository-process follow-ups that belong to no roadmap stage. Keep entries
short; delete them when done.

- [ ] **Codex review sweep script** (requested 2026-06-11): a script that
  walks all PRs and visits those where Codex never left a review (e.g. the
  usage-limit window around PR #40), re-requesting review with
  `@codex review`. Note: the exact trigger phrase matters — free-form
  variants may spawn a cloud task instead of a review.

- [ ] **Kani harnesses for `griff-pattern`** (deferred 2026-07-14, from the
  Swang design review): once the pattern core's semantics are frozen and
  implemented (S16 Phase 1), add a non-blocking CI job — isolated like
  `fuzz/` per ADR-0010 — with bounded harnesses for the budget invariant
  ("expansion never exceeds `max_cells`") and the empty-subtree law
  ("a pruned parent yields no active descendants"). Verus rejected for now:
  its toolchain cost outweighs machine-checked proofs while proptest and the
  golden vectors merely *exercise* those invariants — Kani is the cheaper
  step up when the semantics freeze (see decisions log 2026-07-14).
