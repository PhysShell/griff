# ADR 0003: Master timeline is the single source of truth for transport

Date: 2026-05-19
Status: Accepted

## Context

The MIDI import already collects tempo / time-signature meta globally, but
export builds the meta track from the first bar of the first track and follows
that track's timeline. That is transport-by-coincidence, not score-level
transport, and breaks once multiple tracks or a real tempo map exist.

## Decision

We make `MasterBar` / the master timeline the single source of truth for
tempo, meter, repeats, and transport markers. Import populates
`Score.master_bars`; export builds the meta track only from the master
timeline; tracks never re-define transport.

## Consequences

- Correct multi-track export; a real tempo map; a clean place for SMPTE work
  later.
- Requires the canonical score model (ADR-0002) and the S2 MIDI transport
  refactor before the simplified path can be removed.
