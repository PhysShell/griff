# ADR 0005: griff is swancore-first, not a general-purpose riff generator

Date: 2026-05-19
Status: Accepted

## Context

A general-purpose riff generator needs either a huge corpus or a rule system
covering many incompatible musical languages (drop-tuned chug vs jazz chord
movement). The user explicitly targets swancore (DGD / Hail The Sun / Sianvar /
Royal Coda / Stolas / Eidola / A Lot Like Birds).

## Decision

`griff` is swancore-first. Defaults: Standard E tuning (see ADR-0006), Drop D
supported; Drop C / Drop B / 7-string out of scope for v1. Chord vocabulary:
maj7, m7, sus2, add9, slash + power chords. Rhythm grid: 1/16 with 1/32 for
tapping passages. Data model, features, generator, corpus, and UI are designed
for this scope first.

## Consequences

- Tighter validation; a much smaller corpus suffices (tens, not thousands); a
  stable glossary.
- Repositioning to "any metal" later requires a retag and a superseding ADR;
  users expecting generic genres are out of scope.
