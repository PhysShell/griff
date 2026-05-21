# S8: Standalone preview app

Status: partial — library layer done; UI binary deferred
Depends on: S6
ADRs: —

## Goal

A standalone desktop app to view, listen, compare, and hand-annotate — before
the CLAP plugin, to debug transport/slicing/graph without DAW quirks.

## What was delivered (library layer)

- `preview::piano_roll` — parses MIDI bytes into `PianoRollView` (notes,
  pitch range, tick positions). See `preview/src/piano_roll.rs`.
- `preview::curation` — `CurationAction` enum (`Approve`, `Reject`, `AddTag`,
  `RemoveTag`, `AddQualityFlag`, `SetTitle`), `apply_curation`, and JSON
  persistence (`save_chunk_meta` / `load_chunk_meta`). See
  `preview/src/curation.rs`.
- 23 integration tests, clippy-clean.

## What is deferred

- `preview/src/main.rs` binary — no runnable app yet.
- GUI rendering: `eframe`/`egui` or `ratatui` TUI.
- MIDI playback via `midir`.
- Boundary overlays, history, split/merge/rename actions.

## Inputs / Outputs

- In: `.mid` / corpus chunks / candidates.
- Out: piano-roll view, MIDI playback, boundary overlays, history,
  approve/reject/split/merge/rename/tag actions feeding the corpus.

## Approach (when resumed)

- Add a `[[bin]]` to `preview/Cargo.toml`.
- Gate `eframe`/`egui` behind `--features gui`; ship a `ratatui` TUI as the
  default so the binary works in headless environments.
- Playback via `midir` (OS audio stack required).

## Acceptance criteria

- Loads a `.mid`, shows a piano-roll, plays it back.
- Curation actions persist into the S5 corpus schema.

## Open questions

- Playback engine details on each OS.
- Whether ratatui TUI is sufficient long-term or egui is mandatory.

## See also

- [`../glossary.md`](../glossary.md) §11
