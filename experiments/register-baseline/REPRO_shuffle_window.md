# Shuffle-window focused A/B (reproducibility)

Throwaway corpus-experiment record. Private corpus + source tabs are **not**
committed (ADR-0005); the synthetic fixtures here contain no corpus material.

## SHAs
- **before** generator: `2374b31` (full ladder, seam, NO Shuffle window)
- **after** generator: `acd85b9` (feat: Shuffle-only `LadderWindow`)
- **harness** (analyze + register_scan, production-faithful via
  `griff_cli::generation_input`): `242cf87` on branch `corpus-build`; the same
  example sources are built at each generator tip (a worktree for the `before`
  arm) so the candidate seam matches the generator under test.

## Config (identical on both arms)
- inputs: 3 real (DGD / Wolf & Bear / Hail The Sun, private corpus) + 2 synthetic
  (`fixtures/synth_wide_diatonic.mid` span 48, `synth_narrow_diatonic.mid` span 11)
- seeds 1..5 · gesture {on, off} · **10 variants per strategy** · bars 8
- production `generation_input` seam (placed templates + median gesture) and
  production reranker (real `griff generate`)
- corpus record-list hash:
  `213bd8572c95ebb74151a84997ace78067383fc534ed67ef359188777c64f5ed`

## Commands
```
register_scan "<input>" --corpus <corpus_dir> --seeds 1,2,3,4,5 --variants 10
griff generate "<input>" out.mid --seed S --bars 8 --corpus <corpus_dir> [--no-gesture]
analyze out.mid --input "<input>" --corpus <corpus_dir>
```

## Raw output checksums (sha256, first 16 hex)
- `candidate_before.jsonl` `6043efdc7f534cfb`
- `candidate_after.jsonl`  `8ec0f2b263205d49`
- `winners_before.jsonl`   `ae834b667feb99e4`
- `winners_after.jsonl`    `b8e59eaa51613f72`

## Metric note
`octave_leap_share` counts `abs(interval) >= 12` (== `at_least_octave_share`).
Added for this pass (harness only, no production change): `exact_octave_share`
(`==12`), `over_octave_share` (`>12`), `alternation_rate` (sign-reversal share, a
low↔high bounce detector), and `pitch_hash` (FNV-1a of the ordered pitch bytes,
for before/after regression comparison).

## Result (see summary_shuffle_window.csv)
- **Shuffle fixed**: candidate span median 52→12, max interval 57→12,
  over-octave 0.48→0.00, exact-octave 0.026→0.015 (no bounce takeover),
  alternation 0.62→0.56, union reachability preserved (real 57→57, WIDE 48→47),
  invariants after: span ≤ 12, in-bounds = in-class = 1.0, 0 candidates span>12.
- **Regression check**: RhythmCopy / MotifTranspose / ConstrainedRandomWalk /
  RepeatVariation — 500/500 identical `pitch_hash` before→after (Shuffle-only).
- **Winner level**: Shuffle winners 14→4; winner over-octave 0.130→0.008;
  aggregate 0.894→0.882. BUT 21/50 after-winners still incoherent — all
  RhythmCopy (and RepeatVariation): they ascend the full ladder then wrap with a
  large **downward** jump (largest_downward mean 44.5, max 57). Unchanged by the
  Shuffle fix (a full-ladder property), reranker does not penalize it.

## Verdict
**#2 — Shuffle fixed, but other incoherent winners remain; measure a generic
coherence axis next.** The Shuffle window fully fixes ShuffleMotifs; the residual
winner incoherence is the RhythmCopy/RepeatVariation ascending-then-wrap jump
(present in both arms), which the register-blind reranker does not penalize. Next
increment: a generic coherence axis (or an octave-fold on the ascending walk).
The reranker **does not penalize register incoherence** (it does not "prefer wide
register"). TonalCenter/cadence remain frozen.
