# griff fuzzing crate

Isolated [`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz) /
libFuzzer harness. Policy and rationale live in
[`../docs/fuzzing.md`](../docs/fuzzing.md) and
[`../docs/adr/0010-fuzz-format-adapters-and-core-invariants.md`](../docs/adr/0010-fuzz-format-adapters-and-core-invariants.md).

This crate is **excluded** from the parent workspace and is its own
workspace root: cargo-fuzz needs **nightly** and emits an `extern "C"`
harness that cannot satisfy the workspace `unsafe_code = "forbid"` lint
policy. That divergence stays contained here.

## Run

```sh
# nightly is auto-selected by fuzz/rust-toolchain.toml
cargo install cargo-fuzz          # once
cargo +nightly fuzz run midi_import
cargo +nightly fuzz run swang_parse
cargo +nightly fuzz run pattern_expansion
```

Reproduce a saved crash / regression seed:

```sh
cargo +nightly fuzz run midi_import corpus/midi_import/hang_ppqn1_eighth.mid
```

Optional Nix dev shell (convenience only, not required):

```sh
nix develop ./fuzz
```

## CI gate

The `fuzz` job in
[`.github/workflows/ci.yml`](../.github/workflows/ci.yml) is **blocking**
on every PR: per target (an eleven-way matrix) it runs `cargo check
--bins` under this crate's nightly pin, then ~60 s of bounded libFuzzer
smoke with the committed corpus replayed
(`-timeout=60 -rss_limit_mb=4096 -malloc_limit_mb=2048`). Policy:
[`../docs/fuzzing.md`](../docs/fuzzing.md).

## Targets

| Target               | Subject                                          | Priority |
|----------------------|--------------------------------------------------|----------|
| `midi_import`        | `griff_core::midi::import`                       | P0       |
| `midi_roundtrip`     | import → export → re-import                      | P0       |
| `guitar_pro_import`  | Guitar Pro adapter                               | P0       |
| `score_projection`   | canonical `Score` projection / slicing           | P1       |
| `phrase_boundary`    | S4 boundary detector                             | P1       |
| `swang_parse`        | S16 header pre-parser + parser + formatter laws  | P1       |
| `pattern_expansion`  | S16 `Kernel → fractalize → linearize → map_rhythm` | P1     |
| `generation_request` | S6 rule generator (structure-aware)              | P2       |
| `complement_request` | S13 ComplementArranger                           | P2       |
| `structure_metrics`  | S14 structure metrics                            | P2       |
| `gesture_request`    | S6 gesture compiler                              | P2       |

## Corpus

`corpus/<target>/` holds committed seeds. Notable:

- `corpus/midi_import/hang_ppqn1_eighth.mid` — regression seed for
  **F-001** (a 49-byte PPQN=1 / 1-8 SMF that drove `group_into_bars`
  into a non-advancing loop). **Fixed at S2**; the seed stays as a
  permanent regression guard — see "Known findings" in
  [`../docs/fuzzing.md`](../docs/fuzzing.md).
- `corpus/swang_parse/reference.swg` — the spec §3.1 reference program,
  the grammar's minimal valid seed.
