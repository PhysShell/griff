# S0: Baseline and characterization tests

Status: done
Depends on: —
ADRs: ADR-0001, ADR-0004, ADR-0010

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
- Stand up the isolated `fuzz/` crate (ADR-0010) with the `midi_import` and
  `midi_roundtrip` targets; wire bounded smoke + regression-corpus fuzz into
  CI as a blocking gate.
- Commit finding F-001 (`group_into_bars` zero-`bar_ticks` hang) as the
  first regression seed; fix it here or defer to S2, but only behind a
  characterization test first (no silent behavior change).
- No observable behavior change. Tests describe what *is*, not what *should
  be*.

## Acceptance criteria

- Snapshot/golden tests cover every CLI command on the fixtures.
- A roundtrip test asserts bar alignment is preserved.
- `midi_import` and `midi_roundtrip` fuzz targets build under the nightly
  `fuzz/` toolchain; the regression corpus (incl. F-001) replays in CI.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and
  `cargo test --workspace` all green (the `fuzz/` crate is excluded).

## Resolved questions

- Snapshot tooling: **hand-rolled** plain golden text, re-blessed via
  `GRIFF_BLESS=1` (see `docs/decisions.log.md`, 2026-05-19). No new deps.
- Fixtures: **synthetic minimal** `.mid` built with `midly`, committed under
  `cli/tests/fixtures/` and byte-pinned by `fixtures_in_sync`. No licensed
  real MIDI is used.

## Outcome

- Fixtures: `simple_4_4`, `seven_eight`, `multi_track`, `tempo_change`.
- CLI goldens: `cli/tests/cli.rs` snapshots `import`/`inspect`/`export`/
  `classify` (plus the missing-file error) for every fixture.
- Library characterization: `core/tests/characterization.rs` pins
  `midi`/`feature`/`slice`/`classify` and deterministic `generate`.
- Roundtrip: `core/tests/roundtrip.rs` asserts bar alignment is preserved
  and is idempotent; the pre-canonical losses it *does* have (track names
  dropped, tempo map collapsed to the first tempo) are pinned in the
  `roundtrip__*` goldens, not silently accepted — S0 freezes, never fixes.

## See also

- [`../audit/2026-05-stage-label-reconciliation.md`](../audit/2026-05-stage-label-reconciliation.md)
- [`../glossary.md`](../glossary.md) §13
- [`../fuzzing.md`](../fuzzing.md) (F-001),
  [`../adr/0010-fuzz-format-adapters-and-core-invariants.md`](../adr/0010-fuzz-format-adapters-and-core-invariants.md)
