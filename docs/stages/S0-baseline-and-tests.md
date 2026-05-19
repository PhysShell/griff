# S0: Baseline and characterization tests

Status: in-progress
Depends on: —
ADRs: ADR-0001, ADR-0004

## Goal

Turn the current pre-canonical baseline into a measurable system: freeze
observable behavior with characterization tests before any refactor.

## Inputs / Outputs

- In: existing `core/src/{event,feature,generate,midi,slice,classify}.rs`,
  CLI `import/inspect/export/classify`.
- Out: characterization tests, golden snapshots for CLI output, a MIDI
  roundtrip baseline, minimal `.mid` fixtures.

## Approach

- Add `.mid` fixtures and snapshot CLI `import/inspect/export/classify`.
- Pin MIDI roundtrip: import → export → import equivalence on fixtures.
- No observable behavior change. Tests describe what *is*, not what *should
  be*.

## Acceptance criteria

- Snapshot/golden tests cover every CLI command on the fixtures.
- A roundtrip test asserts bar alignment is preserved.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and
  `cargo test --workspace` all green.

## Open questions

- Snapshot tooling: `insta` vs hand-rolled. Pick before writing many.
- Which real guitar `.mid` is licensed for an in-repo fixture vs a synthetic
  minimal fixture.

## See also

- [`../audit/2026-05-stage-label-reconciliation.md`](../audit/2026-05-stage-label-reconciliation.md)
- [`../glossary.md`](../glossary.md) §13
