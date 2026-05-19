# ADR 0007: CLAP is the plugin target; MIDI-out only; nih-plug

Date: 2026-05-19
Status: Accepted

## Context

The user wants a DAW plugin that triggers/generates guitar fragments. CLAP is
an open standard (MIT) with first-class MIDI and clean threading; VST3/AU add
scope without serving the MVP. `nih-plug` is the production-grade Rust
framework with first-class MIDI-out for CLAP/VST3.

## Decision

The plugin target is CLAP, MIDI-out only (no audio synthesis), built with
`nih-plug`. VST3/AU are explicitly out of MVP scope. The plugin is not started
until the core model and format adapters are stable (S10).

## Consequences

- Smaller surface; open toolchain; no audio engine.
- Limited host support today (Bitwig, Reaper, partial others); wider reach
  later needs a new ADR. Depends on a stable core (ADR-0002/0003).
