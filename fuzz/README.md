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
cargo +nightly fuzz run midi_roundtrip
```

Reproduce a saved crash / regression seed:

```sh
cargo +nightly fuzz run midi_import corpus/midi_import/hang_ppqn1_eighth.mid
```

Optional Nix dev shell (convenience only, not required):

```sh
nix develop ./fuzz
```

## Targets

| Target           | Subject                          | Priority |
|------------------|----------------------------------|----------|
| `midi_import`    | `griff_core::midi::import`        | P0       |
| `midi_roundtrip` | import → export → re-import       | P0       |

Future targets (`score_projection`, `phrase_boundary`,
`guitar_pro_import`, …) land with their canonical stages — see
[`../docs/fuzzing.md`](../docs/fuzzing.md).

## Corpus

`corpus/<target>/` holds committed seeds. Notable:

- `corpus/midi_import/hang_ppqn1_eighth.mid` — **known-failing**
  regression seed. A 49-byte PPQN=1 / 1-8 SMF drives
  `group_into_bars` into a non-advancing loop (hang + unbounded `bars`
  growth). Committed unfixed on the planning branch; tracked under
  "Known findings" in [`../docs/fuzzing.md`](../docs/fuzzing.md).
