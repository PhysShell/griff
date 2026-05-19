# ADR 0010: Fuzz-test format adapters and core invariants

Date: 2026-05-19
Status: Accepted

## Context

`griff` ingests external binary / structural formats — MIDI now, Guitar Pro
(`.gp3/.gp4/.gp5/.gpx/.gp`) later, possibly MusicXML — and then runs
non-trivial transformations (tempo maps, bar grouping, canonical-model
projection, phrase-boundary scoring). These are exactly the inputs that turn
"the file is fine" into "the parser died on one byte".

This is not hypothetical. The pre-canonical baseline already contains a
reachable hang / OOM: `core/src/midi.rs::group_into_bars` advances its
`while bar_start <= end_tick` loop by `bar_ticks(sig, ppqn)`, and
`bar_ticks` integer-divides to **0** for inputs such as PPQN=1 with a `1/8`
time signature. A 49-byte crafted `.mid` makes the loop never advance while
pushing `Bar`s forever. Unit / property / golden tests did not surface it
because nobody thought to write that exact input — which is the point of
fuzzing.

Unit, property (`proptest`), and golden tests stay the primary correctness
tools. Fuzzing is a separate, more modest robustness / security layer: it
proves that arbitrary garbage input does not panic, hang, allocate without
bound, or build an invalid internal model. It does not prove correctness.

Two constraints shape the decision:

- `rust-toolchain.toml` pins **stable** (MSRV 1.74) and the workspace sets
  `unsafe_code = "forbid"` (ADR-0004). `cargo-fuzz` / libFuzzer require
  **nightly** and emit an `extern "C"` harness that cannot satisfy that lint
  policy.
- Stage labels are canonical `S0…S12` only (glossary §0); a "S2.5 fuzzing
  stage" is not allowed. Fuzzing must be distributed across existing stages.

## Decision

1. **Mandatory layer.** Every external format adapter (MIDI, Guitar Pro, …)
   and selected canonical transformations MUST have fuzz targets. The
   required targets, oracles, corpus layout, CI policy, priorities, and
   per-stage mapping are specified in [`../fuzzing.md`](../fuzzing.md) and
   summarized as SPEC hard rule 11.

2. **Tooling.** `cargo-fuzz` + `libfuzzer-sys`, with `arbitrary` for
   structure-aware targets (canonical model, generation requests).
   `proptest` remains the tool for readable invariant specs; `cargo-mutants`
   stays a separate test-strength gate.

3. **Isolation.** `fuzz/` is its own workspace root and is `exclude`d from
   the parent workspace. It carries a local `rust-toolchain.toml` pinning
   nightly. The stable `core`/`cli`/`plugin` workspace, its pinned
   toolchain, and its lint policy are untouched. An optional Nix dev shell
   (`fuzz/flake.nix`) is provided as convenience only.

4. **Oracle enforcement.** "No hang" and "no uncontrolled allocation" are
   enforced operationally by libFuzzer `-timeout`, `-rss_limit_mb`, and
   `-malloc_limit_mb`; "no panic" is a libFuzzer crash; the typed-error
   contract (`Ok(_)` xor a typed error enum) is asserted in the target.

5. **CI policy.** Bounded smoke fuzzing (~60 s/target) plus replay of the
   committed regression corpus is a **blocking** PR gate. Deep fuzzing runs
   scheduled / nightly, **non-blocking**, and files issues. Deep fuzzing is
   non-deterministic and is deliberately kept off the blocking path.

6. **No new stage label.** Targets are scheduled by a P0/P1/P2 priority
   table mapped onto canonical stages (`midi_import`/`midi_roundtrip` at
   S0/S2, `guitar_pro_import` at S3, `score_projection` at S1/S2,
   `phrase_boundary` at S4, generation/region targets at S6/S11). See
   [`../fuzzing.md`](../fuzzing.md).

7. **Known finding tracked, not fixed here.** The `group_into_bars` hang is
   committed unfixed as the first regression seed
   (`fuzz/corpus/midi_import/hang_ppqn1_eighth.mid`) and recorded under
   "Known findings" in [`../fuzzing.md`](../fuzzing.md); the fix is S0/S2
   work, gated by a characterization test first (glossary §17.7).

## Consequences

- The bug class that already bit the baseline is caught early and the
  existing hang becomes a permanent regression seed.
- Accepted: a nightly dependency exists, but only inside `fuzz/`; the main
  workspace stays stable and reproducible.
- Accepted: the `fuzz/` crate is outside `unsafe_code = "forbid"` and the
  workspace lint policy by necessity (libFuzzer harness). The blast radius
  is one excluded crate that ships no library code.
- Accepted: fuzzing proves robustness, not correctness; it complements,
  never replaces, unit / property / golden tests.
- Accepted: deep fuzzing is non-deterministic, so only the bounded smoke +
  regression replay is a blocking gate.
- Migrating to `afl.rs` (stable) later does not invalidate the targets;
  only the harness crate would change.
