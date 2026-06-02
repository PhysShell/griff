# S8: Standalone preview app

Status: in progress — interactive `ratatui` piano-roll landed (slices 1–2)
Depends on: S6
ADRs: —

> Progress: the `preview` workspace member ships:
> - **view-model** (`build_view`: `Score` → `PianoRollView`) — notes on a
>   pitch × tick plane, per-track lanes, bar gridlines. Pure, no I/O.
> - **analysis** (`analyze`: `Score` → `Analysis`) — named sections from
>   `griff_core::classify` (Riff/Breakdown/Solo/Clean/Unknown) plus structure
>   metrics from `griff_core::structure`. Pure, headless-testable.
> - **ASCII rasteriser** (`render_frame`) — view → fixed-size text grid.
> - **interactive TUI** (`tui::App`, `ratatui`) — colored piano-roll with
>   scroll/zoom, a named-section band, a metrics inspector, a playhead, and
>   keyboard navigation. The same render path drives the live crossterm loop and
>   a headless `App::snapshot` (via `TestBackend`), so the UI is CI-verifiable.
>
> The `griff-preview` binary launches the TUI, or prints one headless frame with
> `--snapshot=WxH`.

## UI design reference

`preview/design/` holds self-contained, dependency-free interactive mockups of
the intended native (`egui`) window (design targets only — not wired to the
engine — used to settle layout and interactions before building the real
front-end). Two cross-linked views:

- `index.html` — **piano-roll**: transport bar, left track dock, pitch × time
  grid (keyboard gutter, bar ruler, per-lane notes, playhead, S4 boundary
  overlays, S6 chunk classification bands), and a right curation/inspector dock
  (S14 structure metrics, tags, approve/reject/split/merge).
- `tab.html` — **Guitar Pro–style tablature**: standard notation staff (treble,
  written 8vb) above a TAB staff with fret numbers, rhythm stems/beams, palm-
  mute spans, power chords and lead techniques (bend/hammer/let-ring), a
  multitrack selector strip, and a toggle to hide notation (tab-only, TuxGuitar
  style). Reflects that the engine's `Score`/technique model should drive a
  notation/tab projection alongside the piano-roll.

## Remaining work (follow-up increments)

The pure layers (`view` + `analysis` + `render`) are the foundation; richer
front-ends and audio build on them:

- [x] Interactive `ratatui` front-end: scroll, zoom, named sections, metrics
      inspector, playhead, headless snapshot. (Live terminal-resize is handled by
      `ratatui` redraw; follow-cursor autoscroll is implemented for playback.)
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
