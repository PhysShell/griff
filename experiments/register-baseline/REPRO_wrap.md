# Wrap-free traversal focused A/B (reproducibility)

Throwaway corpus-experiment record. Private corpus + source tabs are **not**
committed (ADR-0005); the synthetic fixtures here contain no corpus material.

## SHAs
- **before (A)** generator: `0215ecf` (`fix(pitch): remove LadderWindow
  anchor-selection bias`) — unbiased anchor, still full-ladder-modulo wrap.
- **after (B)** generator: `1a3c965` (`fix(generate): reflecting full-ladder
  traversal; remove RhythmCopy/Repeat wrap`).
- A→B changes only `core/src/generate.rs` (RhythmCopy/Repeat) + tests + two
  generate snapshots — no other strategy touched.
- **harness patch** (identical on both arms, verified): `sha256(git diff
  A..exp-wrap-before) == sha256(B..exp-wrap-after)` = `08c84407348f0942…`. Built
  in worktrees `exp-wrap-before` (A) and `exp-wrap-after` (B); harness =
  `cli/examples/{register_scan,analyze}.rs` at corpus-build `10c51c5`.

## Config (identical on both arms)
- inputs: 3 real (DGD / Wolf & Bear / Hail The Sun, private corpus) + 2 synthetic
  (`fixtures/synth_wide_diatonic.mid`, `synth_narrow_diatonic.mid`)
- seeds 1..5 · gesture {on, off} · 10 variants/strategy · 8 bars
- 369 production templates · production `generation_input` seam · production
  reranker · corpus record-list hash
  `213bd8572c95ebb74151a84997ace78067383fc534ed67ef359188777c64f5ed`

## Commands
```
register_scan "<input>" --corpus <corpus_dir> --seeds 1,2,3,4,5 --variants 10
griff generate "<input>" out.mid --seed S --bars 8 --corpus <corpus_dir> [--no-gesture]
analyze out.mid --input "<input>" --corpus <corpus_dir>
# bias sanity (A only): register_scan WIDE --seeds 1..100 --variants 10 --gesture off, Shuffle rows
```

## Raw output checksums (sha256, first 16 hex)
- `candidate_wrap_before.jsonl` `bdaeb29c0271967b`
- `candidate_wrap_after.jsonl`  `4660085684b828cb`
- `winners_wrap_before.jsonl`   `b52c7115a89315d2`
- `winners_wrap_after.jsonl`    `1e9c4db1c6113527`
- `bias_sanity.csv`             `c98b70c1cfe94126`

## Result (see summary_wrap.csv, bias_sanity.csv)
- **RhythmCopy**: candidates with >12 jump 398→**0**/500; max interval 57→**6**;
  largest downward 57→**6**; over-octave 0.018→0.000; mean interval 1.33 (adjacent
  rungs), distinct_pitch 30.1 (ascends the full ladder in steps); boundary
  plateau max 2→**1** (no saturation); union span preserved (DGD 57→56, WIDE
  48→48); in-bounds = in-class = 1.0; deterministic (re-run identical).
- **RepeatVariation**: >12 candidates 16→**0**/500; max interval / variation
  penultimate→last 57→**6**; only corpus grids 4 and 6 occur (no ≥8 template in
  this corpus, so long-grid wrap is untested here but the reflecting traversal is
  grid-agnostic by construction); boundary plateau UNCHANGED before→after (11→11,
  a pre-existing repeat-structure artifact, not introduced by the fix);
  distinct_pitch 5.8 → variation nontrivial.
- **Regression**: Shuffle / MotifTranspose / CRW — 500/500 identical pitch_hash
  A→B. RhythmCopy 498/500 changed, Repeat 38/500 changed (only the wrap ones).
- **Winner level**: winners with interval >12 = 19→**0**/50 (all were RhythmCopy);
  winner max interval 25.4→6.0; over-octave 0.007→0.000; aggregate mean
  0.890→0.895 (median 0.878→0.901, stable); six rerank axes stable; distribution
  stays diverse (4 strategies).
- **Bias sanity (A)**: 1000 Shuffle candidates on WIDE — window span ≤12,
  in-bounds = in-class = 1.0, 23 distinct anchors 36..74 (low/mid/high all
  present), thirds 334/339/327 (low/high ratio 1.02 — **no 2× lower skew**).

## Verdict
**#1 — wrap-free traversal fixes residual register defect; no rerank axis
justified.** RhythmCopy and RepeatVariation wraps are gone (>12 → 0 at candidate
AND winner level), reachability preserved, no boundary saturation introduced,
non-target strategies unchanged, anchor selection unbiased, deterministic. No
generic coherence scoring is needed. TonalCenter, cadence, and reranker policy
remain frozen.
