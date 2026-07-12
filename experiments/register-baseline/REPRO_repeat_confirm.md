# RepeatVariation endpoint-local confirmation (focused, ce1c880)

Throwaway corpus-experiment record. Independent focused check of the
endpoint-local RepeatVariation fix — NOT a full corpus A/B.

## SHAs
- behavioral: `2e6d514` (`fix(generate): RepeatVariation variation is
  endpoint-local on dense grids`)
- accepted/docs (built here): `ce1c880` (contains `2e6d514`)
- harness: `register_scan` at corpus-build `c97a652` (identical to the wrap-A/B
  harness plus `bar_metrics` split into three interval kinds); built in a
  detached worktree on `ce1c880`.

## Config
- strategy: **RepeatVariation only** · seeds **0..255** · `--variants 1` ·
  `--gesture off`
- grids (via `--synth-grid`): **4, 6, 8, 16, 32, 64**
- materials (synthetic fixtures, no corpus content):
  `synth_wide_chromatic.mid` (36..84, 12 classes),
  `synth_wide_pentatonic.mid` (36..84, classes {0,2,4,7,9}),
  `synth_two_rung.mid` ({60,67} — 2-rung ladder),
  `synth_single_rung.mid` ({60} — 1-rung ladder)
- regression sanity: the 3 real corpus inputs at their native grids (4/6).

## Interval separation
- `intra_bar_max_interval` — largest step between consecutive notes WITHIN a bar.
- `variation_prev_interval` — in-bar penultimate → varied-last.
- `inter_bar_reset_interval` — last-of-bar → first-of-next (bar-boundary figure
  reset) — **report-only**, a deliberate figure return is allowed.

## Result (6144 synthetic + 768 real-corpus RepeatVariation candidates)
`intra_max` / `var_prev` columns are the max over all grids; `inter_reset` is the
**full candidate-level observed range** (report-only).

| material | intra_max | var_prev | inter_reset (candidate-level range) | in_bounds/in_class | variation |
|---|---|---|---|---|---|
| WIDE_chromatic | 2 | 2 | 1..48 | 1 / 1 | present (2 distinct) |
| WIDE_pentatonic | 5 | 5 | 3..48 | 1 / 1 | present |
| two_rung | 7 | 7 | 7 | 1 / 1 | present |
| single_rung | 0 | 0 | 0 | 1 / 1 | none (1 rung — impossible) |

Acceptance over all 6144: `intra_bar_max > 12` = **0**, `variation_prev > 12` =
**0**, `in_bounds < 1` = 0, `in_class < 1` = 0, variation present where ladder
> 1 = **4608/4608**, single-rung variation correctly absent. Deterministic
(re-run identical). **Real-corpus regression: this raw run exercised grid 6 only**
(`grid_note_count = 6` for all 768 rows across the 3 real inputs) — intra/var max
2, 0 over 12, in-bounds/in-class = 1; **grid 4 is covered by the synthetic
matrix** (the broader corpus historically carries 4/6 templates, but only grid 6
surfaced in this run). `inter_bar_reset` grows with grid (the ascending figure's
loop point) — expected and allowed.

## Verdict
**independent focused confirmation passed; register remains accepted.**
No new register changes, no rerank axis. TonalCenter and cadence remain frozen.
