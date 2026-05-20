# S8: Standalone preview app

Status: done
Depends on: S6
ADRs: —

## Goal

A standalone desktop app to view, listen, compare, and hand-annotate — before
the CLAP plugin, to debug transport/slicing/graph without DAW quirks.

## Inputs / Outputs

- In: `.mid` / corpus chunks / candidates.
- Out: piano-roll view, MIDI playback, boundary overlays, history,
  approve/reject/split/merge/rename/tag actions feeding the corpus.

## Approach

- New workspace member `preview/` using `eframe`/`egui` (immediate-mode,
  native; not Tauri — IPC/HTML overhead for an offline MIDI tool).
- Playback via `midir`. Headless fallback: a `ratatui` TUI.

## Acceptance criteria

- Loads a `.mid`, shows a piano-roll, plays it back.
- Curation actions persist into the S5 corpus schema.

## Open questions

- Playback engine details on each OS.

## See also

- [`../glossary.md`](../glossary.md) §11
