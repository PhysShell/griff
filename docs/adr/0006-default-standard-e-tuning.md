# ADR 0006: Default to Standard E tuning, not Drop C

Date: 2026-05-19
Status: Accepted

## Context

Will Swan, the genre's namesake guitarist, uses light strings in standard E
(Premier Guitar rig rundown: "typically tuned to standard"). Hail The Sun's
guitar work is similarly standard / Drop D, not low tunings. A Drop C/B
assumption would mismatch the reference corpus and produce wrong chord shapes.

## Decision

`Tuning::StandardE` is the default. `Tuning::DropD` is supported. Drop C and
below are not supported in v1.

## Consequences

- Note ranges and chord shapes match the reference corpus.
- Users wanting Drop C / 7-string need a fork or a superseding ADR. Depends on
  ADR-0005.
