# S8: Standalone preview app

Status: in progress — piano-roll slice 1 landed (view-model + ASCII renderer)
Depends on: S6
ADRs: —

> Progress: the `preview` workspace member ships the first slice — a pure,
> headless-testable **view-model** (`build_view`: `Score` → `PianoRollView`,
> notes laid out on a pitch × tick plane, per-track lanes, bar gridlines) and an
> **ASCII rasteriser** (`render_frame`: view → fixed-size grid of rows). The
> `griff-preview` binary imports a `.mid` via the core importer and prints one
> rendered frame. Dependency-free (only `griff-core`), so it builds and is
> verifiable in headless CI.

## Remaining work (follow-up increments)

The two pure layers (`view` + `render`) are the foundation; the interactive
front-end and audio build on them:

- [ ] Interactive `ratatui`/`crossterm` front-end: scroll, zoom, follow-cursor,
      live terminal resize (the doc's headless-friendly path).
- [ ] `eframe`/`egui` native window — the canonical desktop target (piano-roll
      canvas, pan/zoom), reusing the same `PianoRollView`.
- [ ] MIDI playback via `midir`, with a playhead overlay.
- [ ] Curation actions (approve/reject/split/merge/rename/tag) feeding the S5
      corpus schema.
- [ ] Boundary overlays (S4) and candidate history.

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
