# ADR 0008: Explainable heuristic phrase detection before any ML

Date: 2026-05-19
Status: Accepted

## Context

Phrase boundary detection is needed for slicing, corpus, and generation. Fully
automatic labelling will confidently err. The MIR literature (LBDM, IDyOM)
shows local-discontinuity and predictive cues work and are explainable.
Neural approaches need a corpus that does not yet exist.

## Decision

S4 uses an explainable heuristic detector: a weighted boundary score over
pause / cadence / rhythm-reset / motif-boundary / register-jump /
density-change signals, with hard rules for obvious pauses/resolutions and a
mandatory manual override. No ML for boundary detection before a corpus and a
rule-based baseline exist.

## Consequences

- Debuggable, override-able boundaries; baseline acceptance can be measured
  against hand-labelled phrases.
- Default weights are placeholders to be calibrated on the S5 corpus; not
  "magic coefficients". Depends on ADR-0002. Reconsider after S9.
