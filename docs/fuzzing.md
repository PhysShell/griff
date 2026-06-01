# fuzzing.md — fuzz-testing policy

Authoritative policy for fuzz testing in `griff`. Decision and rationale:
[`adr/0010-fuzz-format-adapters-and-core-invariants.md`](adr/0010-fuzz-format-adapters-and-core-invariants.md).
Terms: [`glossary.md`](glossary.md) §13. Hard rule: [`SPEC.md`](SPEC.md) #11.

## Why (and why not)

Fuzzing is a **robustness / security layer**, added *after* unit / property
/ golden tests, never instead of them. It does not prove correctness. It
proves the modest-but-vital property:

> No garbage input may panic, hang, allocate without bound, or build an
> invalid internal model. Every input yields `Ok(_)` or a typed error.

It is justified concretely: the pre-canonical baseline already hangs on a
49-byte crafted `.mid` (see [Known findings](#known-findings-open)).

## Tooling

- `cargo-fuzz` + `libfuzzer-sys` — the harness.
- `arbitrary` — structure-aware inputs (canonical model, generation
  requests), not just raw bytes.
- `proptest` — stays the tool for readable invariant specs (not fuzzing).
- `cargo-mutants` — separate test-strength gate, unrelated to fuzzing.

`fuzz/` is its own workspace root, `exclude`d from the parent workspace,
with a local nightly `rust-toolchain.toml`. The stable
`core`/`cli`/`plugin` workspace and `unsafe_code = "forbid"` are untouched
(ADR-0010 §3). Optional Nix dev shell: `nix develop ./fuzz`.

## How the oracle is enforced

| Property                  | Enforcement                                      |
|---------------------------|--------------------------------------------------|
| no panic                  | a panic is a libFuzzer crash                     |
| no hang                   | libFuzzer `-timeout` (default 60 s here)         |
| no uncontrolled allocation| libFuzzer `-rss_limit_mb`, `-malloc_limit_mb`    |
| typed-error contract      | asserted in the target (`Ok` xor typed error)    |
| normalized invariants     | `assert!`/`assert_eq!` in the target body        |

Byte-identical roundtrip is **never** required — MIDI bytes legitimately
differ structurally. Only normalized invariants are checked.

## Required fuzz targets

1. **`midi_import`** — input: arbitrary `&[u8]`; subject:
   `griff_core::midi::import`. Oracle: no panic / hang / unbounded alloc;
   result is `Ok(MidiSong)` xor typed `MidiError`.
2. **`midi_roundtrip`** — input: arbitrary `&[u8]`; flow: `import` →
   `export` → re-`import`. Oracle: re-import succeeds; PPQN preserved;
   note-bearing track count preserved; richer normalized invariants
   (bar-duration validity, pitch/velocity ∈ 0..=127, no reversed ranges,
   no duration overflow) added at S2 with the canonical model + `LossReport`.
3. **`score_projection`** — input: `arbitrary`-generated canonical `Score`;
   subject: projection to phrase view, slicing, feature extraction. Oracle:
   no panic; ranges stay ordered; durations do not overflow silently.
4. **`guitar_pro_import`** — input: arbitrary `&[u8]` + seed corpus of
   minimal `.gp3/.gp4/.gp5/.gpx`; subject: GP adapter. Oracle: no panic;
   no zip-bomb / unbounded-alloc behavior; unsupported constructs produce
   typed warnings / a `LossReport`.
5. **`phrase_boundary`** — input: `arbitrary`-generated phrases / scores;
   subject: boundary detector. Oracle: boundaries sorted; boundary ticks
   within phrase duration; scores finite; `BoundaryReason` consistent with
   non-zero score components.

Targets are introduced incrementally — only `midi_import` and
`midi_roundtrip` are implementable today (the rest depend on types that do
not exist yet) and so are scaffolded in `fuzz/fuzz_targets/` now; the others
land with their stages.

## Priority and stage mapping

No new stage label is created (glossary §0). Targets are scheduled by
priority onto canonical stages:

| Priority | Target              | Lands at | Depends on               |
|----------|---------------------|----------|--------------------------|
| P0       | `midi_import`       | S0       | exists today             |
| P0       | `midi_roundtrip`    | S0 → S2  | richer invariants at S2  |
| P0       | `guitar_pro_import` | S3       | GP adapter               |
| P1       | `score_projection`  | S1 / S2  | canonical model          |
| P1       | `phrase_boundary`   | S4       | boundary detector        |
| P2       | `generation_request`| S6       | rule generator           |
| P2       | `complement_request`| S13      | ComplementArranger       |
| P2       | `region_regeneration`| S11     | regeneration             |

## Corpus

- Layout: `fuzz/corpus/<target>/`, seeds committed to the repo.
- Every confirmed crash/hang is minimized and committed as a permanent
  **regression seed** before (or alongside) its fix.
- Seed corpora for binary formats start from minimal valid files
  (`valid_minimal.mid`, later minimal `.gp3/.gp4/.gp5/.gpx`).

## CI policy

- **Blocking (every PR):** bounded smoke fuzz, ~60 s per implemented
  target, plus a full replay of the committed regression corpus. Fast and
  deterministic.
- **Non-blocking (scheduled / nightly):** deep fuzzing with a large time
  budget; new crashes are minimized and filed as issues + regression seeds.
- Deep fuzzing is non-deterministic and is deliberately kept off the
  blocking path.

## Definition of done for a fuzz target

- Target builds under the nightly `fuzz/` toolchain.
- Oracle encodes the contract above (typed-error xor success + the
  target-specific normalized invariants).
- A minimal valid seed corpus is committed.
- The bounded smoke run is wired into the blocking CI gate.
- Any crash found is committed as a minimized regression seed.

## Known findings

### F-001 — `group_into_bars` zero-`bar_ticks` hang / OOM *(fixed in S2)*

- **Where:** `core/src/midi.rs` — `group_into_bars` (the
  `while bar_start <= end_tick` loop) advances by `bar_ticks(sig, ppqn)`,
  which integer-divides to `0` for e.g. PPQN=1 with a `1/8` time
  signature.
- **Effect:** `bar_start` never advances → infinite loop while pushing
  `Bar`s → hang then OOM. Reachable from a 49-byte well-formed `.mid`.
- **Reproducer / first regression seed:**
  `fuzz/corpus/midi_import/hang_ppqn1_eighth.mid`. The characterization
  test `regression_f001_degenerate_meter_returns_typed_error` in
  `core/src/midi.rs` guards the fix.
- **Status:** **fixed at S2** — `bar_ticks()` now returns
  `MidiError::DegenerateMeter` when the result is 0 instead of looping.

## See also

- [`adr/0010-fuzz-format-adapters-and-core-invariants.md`](adr/0010-fuzz-format-adapters-and-core-invariants.md)
- [`../fuzz/README.md`](../fuzz/README.md)
- [`glossary.md`](glossary.md) §13, [`SPEC.md`](SPEC.md) #11
- Stage docs: [`stages/S0`](stages/S0-baseline-and-tests.md),
  [`S2`](stages/S2-midi-transport-refactor.md),
  [`S3`](stages/S3-guitar-pro-import.md),
  [`S4`](stages/S4-phrase-boundary-detection.md)
