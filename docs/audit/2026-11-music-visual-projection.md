# Music structure → visuals: pointer to an external research note

Date: 2026-11-24  
Status: research only; no change to the canonical model, stages or dependencies  
Exact base: `d786a99eeb4d7b2d874a3bf0db9b92f9d3d18a94`

The full study lives in the video-autoedit-research repository, branch
`research/music-visual-2026-11-24`, folder `research/music-visual/`
(`DECISION.md`, `REGISTRY.md`, `EXPERIMENTS.md`, `probe/`). This note records
only what concerns griff.

## What was checked against griff

- A visual consumer does not need a second score model. `composer.compose(recipe,
  _trace)` already yields notes with exact sixteenth positions, velocity and
  techniques, the per-bar section label and motif operator (`form[i].prog`) and
  section spans; `ui-core::playback::TempoMap` already maps ticks to seconds.
  A read-only projection of these into an event list was run on four
  composer-lab pieces (7/8, 4/4, 6/8, 5/4 → 4/4) and was byte-identical across
  two runs.
- `midi.rs:build_master_bars` samples the tempo at each bar start, so tempo
  events inside a bar do not reach `MasterBar`; no loss entry for this was seen
  at that call site (code reading only, not tested).
- Audio analysis of griff renders is no substitute for the exact data: on the
  four renders the beat/structure analyser used by the video project found all
  bar lines only in 4/4 (7/8: every second bar; 6/8 and 5/4: mostly off the
  bar lines) and at most two of five section boundaries.
- The control-curve shape in `docs/proposals/reproducible-pattern-processes.md`
  §2.1/§2.4 (pure range query, curves on the normalised phase of a phrase) is
  the same shape a visual consumer needs. That is an observation, not a reason
  to widen S16 beyond music.

## Open question owned by the external study

Whether phrase, section and motif structure adds anything visible over beats
and accents is untested. The study's experiment X1 uses composer-lab recipes as
exact input; griff code is not modified for it.
