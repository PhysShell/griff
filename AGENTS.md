# griff — agent guide

## Why

`griff` generates swancore-style guitar riffs as a structured symbolic model
(MIDI in → MIDI out, no audio synthesis). Swancore = post-hardcore subgenre
after Will Swan / Dance Gavin Dance / Hail The Sun.

## What (project map)

- `core/`    — library: event model, MIDI I/O, slicing, features, generator
- `cli/`     — binary `griff` (`import` / `inspect` / `export` / `classify` /
  `curate`)
- `preview/` — headless-testable ratatui preview: view-model, analysis
  (sections + structure metrics), interaction core (ADR-0016)
- `plugin/`  — CLAP plugin via nih-plug (S10+, not yet)
- `fuzz/`    — isolated nightly cargo-fuzz crate (ADR-0010; not a workspace
  member); policy in [`docs/fuzzing.md`](docs/fuzzing.md)
- `docs/`    — knowledge base; start at [`docs/SPEC.md`](docs/SPEC.md)

## Constitution

[`docs/glossary.md`](docs/glossary.md) is authoritative. On any term conflict,
defer to it; extend it rather than inventing synonyms in code. The **canonical
score model is now the single internal model** — the legacy linear
`Event/Bar/Phrase` layer has been removed (ADR-0011). For the stage-label
history see
[`docs/audit/2026-05-stage-label-reconciliation.md`](docs/audit/2026-05-stage-label-reconciliation.md).

## How (commands)

- Test:  `cargo test --workspace`
- Lint:  `cargo clippy --all-targets -- -D warnings`
- Format: `cargo fmt --all` (`--check` in CI)
- Docs:  `cargo doc --no-deps --workspace`
- Fuzz:  `cargo +nightly fuzz run midi_import` (from repo root; see
  [`docs/fuzzing.md`](docs/fuzzing.md))

## Routing

- A roadmap stage? → `docs/stages/SN-*.md` (canonical S0…S14)
- A fuzzing question? → [`docs/fuzzing.md`](docs/fuzzing.md) (policy) /
  ADR-0010
- An architectural decision? → new ADR in `docs/adr/` (Nygard, ADR-0009)
- A small decision? → append to `docs/decisions.log.md`
- A term? → `docs/glossary.md`
- Scope question? → `docs/SPEC.md`

## TDD workflow (mandatory)

Every new module or non-trivial change follows the red-green cycle strictly:

1. **Red** — write the tests first. Run `cargo test --workspace`; the new tests
   must appear and fail. Commit the failing tests before touching implementation.
2. **Green** — write the minimal implementation to make them pass. Run
   `cargo test --workspace`; all tests must be green. Commit.
3. **Refactor** — tidy while keeping tests green. Commit if anything changed.

Hard rules:
- Never commit new `pub fn` / `pub struct` implementation in the same commit as
  the tests that cover it, for any new functionality.
- A subagent or agent step that is told to "implement module X" must split the
  work into two sequential tasks: (a) write and commit failing tests, (b) write
  and commit implementation.
- Characterization tests for existing behaviour (no new public API) are exempt
  from the red phase but must still pass before commit.

## Hard constraints (full list in docs/SPEC.md)

- `unsafe_code = "forbid"` workspace-wide; no exceptions without an ADR.
- MIDI is a boundary, not the internal model.
- Master timeline is the single source of truth for tempo/meter.
- Refactors start with characterization tests; no silent behavior change.
- Generation is deterministic under a fixed seed.
- Format adapters emit a loss report.
- Fuzzing is mandatory for format adapters and selected core transforms;
  bounded smoke + regression fuzz is a CI gate (ADR-0010).
- Default tuning Standard E; swancore-first scope (ADR-0005/0006).
- All repository text is English.
- Stage numbering follows `docs/glossary.md` §0 only — never improvise labels.
