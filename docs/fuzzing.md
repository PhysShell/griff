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
6. **`swang_parse`** — input: arbitrary UTF-8; subject: the S16 Phase 3
   header pre-parser, parser, and canonical formatter. Oracle: no panic;
   `Ok(Program)` xor a non-empty `Vec<Diagnostic>` with `SWG` codes and
   in-bounds spans; every accepted program satisfies the formatter laws
   (`parse(format(ast)) == ast`, `format` a fixed point).
7. **`pattern_expansion`** — input: `arbitrary`-built kernel, budget,
   pruning, and bar/unit geometry; subject:
   `Kernel -> fractalize -> linearize -> map_rhythm`. Oracle: no panic; an
   accepted expansion never exceeds `max_cells` (budget capped at
   `u16::MAX` in the target — a huge grid under a huge explicit budget is
   not a finding); a budget breach reports `needed > max_cells`; both
   traversals cover every cell; every lowered note is one unit long, on a
   slot boundary, inside its bar.

All eleven registered targets are implemented in `fuzz/fuzz_targets/`;
each landed with its stage (see the priority map below). New subsystems
keep the pattern: the target lands in the same stage as the code it
fuzzes.

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
| P2       | `structure_metrics` | S14      | structure metrics        |
| P2       | `gesture_request`   | S6       | gesture compiler         |
| P1       | `swang_parse`       | S16      | Phase 3 parser/formatter |
| P1       | `pattern_expansion` | S16      | Phases 1–2 pattern core  |
| P2       | `region_regeneration`| S11     | regeneration             |

## Corpus

- Layout: `fuzz/corpus/<target>/`, seeds committed to the repo.
- Every confirmed crash/hang is minimized and committed as a permanent
  **regression seed** before (or alongside) its fix.
- Seed corpora for binary formats start from minimal valid files
  (`valid_minimal.mid`, later minimal `.gp3/.gp4/.gp5/.gpx`).

## CI policy

- **Blocking (every PR):** the `fuzz` job in
  [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) — an
  eleven-way matrix, one job per implemented target: a `cargo check
  --bins` under the crate's nightly pin (signature rot is what actually
  bit first — the crate sat silently broken from S16 Phase 2 until Phase
  3 tried to compile it), then ~60 s of bounded libFuzzer smoke with the
  committed corpus replayed, under `-timeout=60 -rss_limit_mb=4096
  -malloc_limit_mb=2048`.
- **Non-blocking (scheduled / nightly):** deep fuzzing with a large time
  budget; new crashes are minimized and filed as issues + regression seeds.
  Not yet wired — the standing prescription.
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

### F-002 — midly SMPTE fps negate overflow *(fixed at S16)*

- **Where:** `midly 0.5.3`, `Timing::read` (`primitive.rs:495`): the SMPTE
  fps byte is negated **before** validation — `-(byte as i8)` overflows on
  fps `-128` (division high byte `0x80`).
- **Effect:** a debug-profile panic (fuzz builds carry debug assertions)
  reachable from a 14-byte header. Found by the blocking gate's **first
  ever smoke run**, felling four MIDI targets at once.
- **Reproducer / regression seeds:** `panic_smpte_fps_min.mid` (corpora of
  `midi_import`, `midi_roundtrip`, `score_projection`, `phrase_boundary`)
  and `panic_smpte_second_header.mid` (`midi_import`, `midi_roundtrip`) —
  the gate's **second** run found that midly parses *every* `MThd` chunk as
  a header, so a guard on the first header alone guards nothing.
  Characterization tests: `regression_f002_*` in `core/src/midi.rs`.
- **Status:** **fixed at S16** — `smpte_division_reachable` in
  `core/src/midi.rs` walks the chunk boundaries exactly the way midly does
  and rejects any header chunk with a timecode division
  (`SmpteTimingUnsupported`, the rule that already existed in
  `extract_ppqn`) before midly reads it.

### F-004 — tiny-PPQN unbounded bar materialization *(fixed at S16)*

- **Where:** `core/src/midi.rs` — `build_score_track` and
  `build_master_bars` walk one bar per loop iteration; a metrical PPQN of 1
  plus a ~2-billion-tick varlen delta implies ~500 million bars.
- **Effect:** out-of-memory (not a hang — the loops terminate, but only
  after allocating billions of `EventGroup`/`MasterBar` entries).
  griff's own bug, found by the gate's **third** smoke run after F-002 was
  fixed — the same F-001 family (a degenerate timing value driving an
  unbounded bar loop), a different trigger.
- **Reproducer / regression seed:** `oom_tiny_ppqn_huge_delta.mid` (49 bytes,
  corpora of `midi_import` and `midi_roundtrip`). Characterization test:
  `regression_f004_tiny_ppqn_huge_delta_is_typed_error_not_oom`.
- **Status:** **fixed at S16** — both per-bar loops stop at
  `MAX_MASTER_BARS` (1,000,000) with `MidiError::TooManyBars`, bounding
  memory to tens of megabytes.

### F-005 — quadratic phrase-boundary cadence scan *(fixed at S8)*

- **Where:** `core/src/boundary.rs` — `signal_cadence` scanned all master bars
  linearly, and the detector scores one candidate per bar, so the cost was
  `O(bars²)`.
- **Effect:** a 24 s slow unit (libFuzzer timeout) from a 50-byte MIDI whose
  one huge varlen delta at PPQN 480 imports tens of thousands of bars (under
  F-004's `MAX_MASTER_BARS`, so import succeeds). Not a hang — quadratic. Found
  by the `phrase_boundary` gate on an unrelated PR's run, the fuzz gate earning
  its keep again.
- **Reproducer / regression seed:**
  `fuzz/corpus/phrase_boundary/slow_quadratic_cadence.mid`. Characterization
  test: `regression_f005_many_bars_do_not_go_quadratic` in `core/src/boundary.rs`.
- **Status:** **fixed at S8** — `signal_cadence` binary-searches the ascending
  master bars (`O(log bars)`), so the detector is `O(bars · log bars)`.

### F-006 — silent VLQ delta truncation on export *(fixed at S8)*

- **Where:** `core/src/midi.rs` — `abs_to_delta` converted an unchecked `u32`
  event gap with `u28::from_int_lossy`. An SMF delta is a 4-byte
  variable-length quantity, capped at `0x0FFF_FFFF`; `from_int_lossy` wraps
  anything larger mod 2^28 and reports no error. Every export — meta track and
  note track — went through that one helper.
- **Effect:** **silent corruption reported as success**, the one outcome the
  gate's typed-error-xor-success contract forbids. The finding's meta track
  jumps from tick 1,920 to a 5/64 time-signature change at 268,536,960; the
  gap of 268,535,040 wrapped to 99,584, placing the change at 101,504 —
  ~268 million ticks early. Re-import then derives ~1,791,473 bars from the
  resulting 150-tick bar and rejects the file with `TooManyBars`, so griff's
  own importer refused griff's own export.
- **The condition is a gap, not a position:** a score may legally run past
  `0x0FFF_FFFF` provided every individual delta stays representable. Sparse
  meta tracks are the exposed case, since time-signature, tempo and
  end-of-track events can sit hundreds of millions of ticks apart.
- **Reproducer / regression seed:**
  `fuzz/corpus/midi_roundtrip/roundtrip_delta_exceeds_vlq.mid` (274 bytes, the
  gate's own artifact, unminimized). Characterization tests:
  `regression_f006_export_refuses_unrepresentable_delta` and
  `export_never_returns_bytes_its_own_importer_rejects` in `core/src/midi.rs`,
  both sharing the corpus file via `include_bytes!` so seed and contract cannot
  drift.
- **Found by:** the `midi_roundtrip` gate on PR #129 (run 29572853555), whose
  diff touches no `core/` file. Replaying the artifact reproduced identically
  on `main`'s tip (`2612bd8`), so the defect was live on `main` and merely
  discovered by that PR. The gate had been green for the 11 preceding runs:
  libFuzzer mutated a committed seed and found this inside its 60-second
  budget. Committing the seed converts that dice roll into a deterministic
  check.
- **Status:** **fixed at S8** — `abs_to_delta` returns `Result` and refuses an
  unrepresentable gap with `MidiError::DeltaExceedsVlq { previous_tick,
  absolute_tick, delta }`. Not `TooManyBars`: that is an importer resource
  bound, this is an exporter representation failure.

### F-003 — guitarpro unvalidated direction index *(open, quarantined)*

- **Where:** `guitarpro 0.4.2`, `model/legacy/headers/io.rs:114`:
  `measure_headers[index - 1]` indexes by a direction measure index the
  format data supplies, unvalidated against the header count.
- **Effect:** an index-out-of-bounds panic inside the upstream crate; no
  adapter-side pre-validation can reach it (the index is deep in the
  format), and `catch_unwind` cannot help under libFuzzer's abort-on-panic
  hook.
- **Status:** **open** — `guitar_pro_import` is quarantined to
  signature-check only in the CI matrix (visibly, in the workflow file).
  **Exit criteria:** an upstream fix or a vendored patch re-enables the
  smoke; the crashing input from any future run is preserved by the
  gate's artifact upload.

## See also

- [`adr/0010-fuzz-format-adapters-and-core-invariants.md`](adr/0010-fuzz-format-adapters-and-core-invariants.md)
- [`../fuzz/README.md`](../fuzz/README.md)
- [`glossary.md`](glossary.md) §13, [`SPEC.md`](SPEC.md) #11
- Stage docs: [`stages/S0`](stages/S0-baseline-and-tests.md),
  [`S2`](stages/S2-midi-transport-refactor.md),
  [`S3`](stages/S3-guitar-pro-import.md),
  [`S4`](stages/S4-phrase-boundary-detection.md)
