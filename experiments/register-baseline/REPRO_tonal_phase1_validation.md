# TonalContext Phase 1 — focused equivalence validation

Throwaway experiment tooling. Focused equivalence check of the new shared
`core::tonal` layer — NOT a full corpus A/B. No production behavior changed here.

## SHAs
- before = `0f8532f` (Phase 0 design note; pre-tonal-module)
- red    = `6f9114d` · green = `184b586`
- after  = `e2c9c7f` (Phase 1: `core/src/tonal.rs`; `complement.rs` delegates)
- validation worktrees: `tonal-phase1-before` @ 0f8532f, `tonal-phase1-after` @
  e2c9c7f.
- **identical harness patch** (`harmony_dump.rs`) applied to both arms —
  `sha256(git diff SHA..arm)` =
  `71dd2cebb4b08a725d703f0297d7e3f753593c2db264b929c4928ff733693e4a` (byte-equal
  on both). `evidence_dump.rs` is after-only (new API); `tonal_scan.rs` is
  refactored to delegate (see §5).

## Inputs (sha256, first 16 hex)
`DGD Robot pt2` 7b33d5c735fb4bc8 · `Wolf & Bear Deleto` d87fbac6afeb3d60 ·
`HTS Eight-Ball` 5b4e09610e27092a · `synth_wide_diatonic` b356febfc5291b22 ·
`synth_wide_pentatonic` b9b827c1d6748904 · `synth_wide_chromatic` 0ae9b26d010bdaa0 ·
`synth_narrow_diatonic` 3e5da3f3d2e732f7 (same set as `tonal_evidence.jsonl`).

## Artifact checksums (sha256, first 16 hex — file-level; see comparison note)
`phase1_harmony_equivalence.csv` b7503cd9e4d2386b ·
`phase1_evidence_equivalence.csv` b93cf89f70918eb3 ·
`phase1_generation_smoke.csv` 378b879f61ab07a9 · `phase1_evidence.jsonl` 0bfdb7be8dd0b2e1.

## Comparison methodology (what was compared, in full)
The 16-hex values above and any 16-hex fields are **display prefixes only**. The
actual equivalence decisions used full data, not truncated hashes:
- **HarmonicContext / structure**: full-file `diff` (every byte).
- **Evidence**: element-wise compare of the full 12-slot `onset_counts` /
  `duration_mass` arrays and exact `winner_correlation` / `confidence_margin`
  float bits (`f64::to_bits`), correlation matched to 4 decimals.
- **Generation smoke**: `phase1_generation_smoke.csv` now carries the **full
  64-hex SHA-256** of each MIDI, and the pass was reconfirmed by a byte-level
  `cmp` (not just the hash): **30/30 full-SHA-256-equal AND 30/30 byte-identical**.
  (An earlier run of this CSV stored 16-hex prefixes and compared on the prefix;
  it has been regenerated with full SHA-256 + `cmp`.)

## 1. HarmonicContext non-regression (production `complement::analyze_part`)
`harmony_dump` on every track, both arms, comparing Some/None,
`tonic_pitch_class`, `mode`, `scale_fit.to_bits()`.
**PASS** — 16 records, **0 changed** (bit-identical, incl. `scale_fit`).
Structure consumer (reuses `estimate_harmony`): `griff structure` output
**byte-identical on 7/7 inputs**.

## 2. Core evidence vs frozen prototype (`tonal_evidence.jsonl`)
`evidence_dump` (new API) on the after arm, mapped scopes
(all_tracks↔WholeScore, track_i↔Track_i, track_i_v0↔Voice_i_0):
- `PitchEvidence.note_count` == old note_count
- `PitchEvidence.pitch_range` == old pitch_lo/pitch_hi
- `PitchEvidence.onset_counts` == old **raw_pc** (NOT old onset_pc — that was
  metric-accent ×2)
- `PitchEvidence.duration_mass` == old **dur_pc**
- `estimate_key` winner (tonic / mode / correlation / confidence_margin) == the
  old duration-only prototype (equal to 4 decimals; exact bits also emitted).

**PASS** — **39/39 mapped scopes, 0 mismatches.** Histogram additivity:
`WholeScore = Σ Tracks` **PASS**, `Track = Σ Voices` **PASS**. **24 finite
candidates** on every non-empty scope. Wolf & Bear scope conflict reproduced —
`Track_0` C#-minor (corr 0.786648, margin 0.155692) vs `all_tracks/WholeScore`
F#-major (corr 0.724206, margin 0.074715). These remain **hypotheses**, not
verified ground truth.

## 3. Generation smoke (tree-drift only; tonal not wired into generation)
`griff generate`, 3 real × seeds 1..5 × gesture on/off, production default
candidate count, full SHA-256 + byte `cmp` before vs after: **30/30
full-SHA-256-equal AND 30/30 byte-identical.**

## 4. Estimator-drift removal (`tonal_scan.rs`)
`tonal_scan` now delegates to `PitchEvidence::measure` + `estimate_key`; its
duplicate KK profiles, Pearson correlation, and 24-key ranking are removed. It
retains only two explicitly named local diagnostics the core omits:
`onset_accent_pc` (metric-accent ×2) and `first_bar_pc`/`last_bar_pc`. It
implements no key estimator.

## Verdict
- HarmonicContext exact equivalence: **PASS**
- structure consumer equivalence: **PASS**
- core evidence vs prototype mapping: **PASS**
- histogram additivity: **PASS**
- generation smoke: **PASS**

**TonalContext Phase 1 closes.** No Phase 2 code, confidence cutoff, automatic
scope selection, cadence, or generation integration.
