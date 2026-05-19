# S10: CLAP MVP

Status: planned
Depends on: S6 (stable core); benefits from S8/S9
ADRs: ADR-0007

## Goal

A MIDI-oriented CLAP plugin: trigger/generate fragments, synced to host
transport. No audio engine.

## Inputs / Outputs

- In: host transport (tempo/bar/position), MIDI input triggers, parameters.
- Out: MIDI events into the DAW; candidate history; drag-and-drop MIDI.

## Approach

- `nih-plug`, `MidiConfig::Basic` input + MIDI output enabled.
- Modes: Continuous (new bar each downbeat), Triggered (a note triggers
  regen), Locked (remembered voice loops).
- Parameters: Density, Syncopation, Tag mix, Seed.
- Bundle via `cargo xtask bundle`.

## Acceptance criteria

- Loads in a CLAP host (Bitwig/Reaper), follows host tempo/bar.
- A trigger produces deterministic MIDI for a fixed seed.
- MIDI-out only; no audio code path.

## Open questions

- History persistence format across host sessions.

## See also

- [`../adr/0007-clap-first-plugin-target.md`](../adr/0007-clap-first-plugin-target.md)
- [`../glossary.md`](../glossary.md) §11
