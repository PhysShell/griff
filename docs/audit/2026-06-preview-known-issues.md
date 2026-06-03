# Preview known issues — Codex review of PR #18 (2026-06-03)

Three latent **P2** issues surfaced by the Codex reviewer on PR #18 (the
viewport refactor, ADR-0016). None were introduced by that refactor — it is
behaviour-preserving and golden-locked — all three live in the pre-existing S8
slice-2 code (commit `600c939`). They were recorded here so the clean refactor
PR could stay scoped and the fixes could be picked up deliberately as
follow-ups.

Each item is independent. Items 1 and 2 change rendered output, so fixing them
must regenerate the `preview/src/golden/*.txt` characterization frames in the
same commit (a deliberate behaviour change, not a silent one). Item 3 is a
semantic decision in the analysis layer and likely warrants its own discussion.

## 1. Phantom note in the rightmost column (render)

- **Where:** `preview/src/tui.rs`, `render_roll` note loop (the
  `c0.min(last)..=c1.min(last)` clamp).
- **Mechanism:** when a note's onset is entirely to the right of the visible
  tick window, both clamped endpoints collapse to `last`, so an off-screen
  future note is still painted as a one-cell block in the rightmost column.
- **Repro:** zoom/scroll so that later material sits beyond
  `scroll_tick + plot_w * ticks_per_col`; the demo fixture never does, so the
  goldens are unaffected by the *bug* (but see below for the *fix*).
- **Proposed fix:** skip any note whose `onset >= scroll_tick + plot_w *
  ticks_per_col` before clamping (symmetric with the existing
  `note.end <= scroll_tick` left-edge skip).
- **Golden impact:** none expected for the current demo frames; still re-run the
  characterization tests to confirm, and add a fixture/test that scrolls a note
  off the right edge to lock the corrected behaviour.
- **Effort:** small. **Risk:** low.

## 2. `fit` uses floor division, can clip the tail (viewport)

- **Where:** `preview/src/viewport.rs`, `Viewport::fit`
  (`ticks_per_col = (span / cols).max(1)`).
- **Mechanism:** for any span that is not an exact multiple of `cols`, floor
  division yields a zoom that covers `cols * ticks_per_col < span` ticks, so the
  final tail/barline stays off-screen until the user scrolls — yet `fit` is
  documented to fit the *whole* span and drives both the initial TUI and the
  snapshots.
- **Proposed fix:** ceiling division — `ticks_per_col = span.div_ceil(cols).max(1)`.
- **Golden impact:** **changes both goldens** (a different `ticks_per_col`
  re-lays the frame). Regenerate `initial_80x20.txt` and `acted_80x20.txt` in the
  same commit and call out the behaviour change in the message.
- **Note:** this is exactly the math the refactor preserved byte-for-byte from
  the original S8 code, so it is a real (pre-existing) latent bug, not a
  regression.
- **Effort:** small. **Risk:** low, but it is a visible behaviour change.

## 3. Bar classification only inspects the first voice (analysis)

- **Where:** `preview/src/analysis.rs` — `pick_focus_track` sums note counts
  across **all** voices (line ~79), but `sections_for` classifies only
  `track.voices.first()` (line ~89).
- **Mechanism:** for multi-voice imported tracks (e.g. Guitar Pro tracks built
  with one `Voice` per GP voice), material in voice 2+ can drive the chosen
  focus track and the structure metrics while the section bands/inspector are
  classified as if it did not exist — yielding `Unknown`/`Clean` sections for an
  otherwise populated riff.
- **Open question (decision, not just a fix):** how to combine voices for
  per-bar classification — merge all voices' atoms in the bar range before
  `classify_bar`, or classify per voice and reduce? This touches the meaning of
  a "section" for multi-voice material and should be settled deliberately
  (decisions.log entry, or an ADR if it interacts with the S14 metrics).
- **Effort:** medium. **Risk:** medium (semantic). Best handled as its own
  change, not folded into the viewport refactor.

## Status

Resolved in the follow-up branch created from PR #18 review feedback:

- #1 — regression test added for a note that starts beyond the visible plot; the
  scene resolver now skips it before right-edge clamping.
- #2 — `Viewport::fit` now uses ceiling division; both 80×20 terminal goldens
  were re-blessed for the visible final barline / shortened fitted note tail.
- #3 — section classification now aggregates note features across every voice in
  the focus track before assigning a bar class.

Original Codex review thread pointers:

- #1 — https://github.com/PhysShell/griff/pull/18#discussion_r3345343488
- #2 — https://github.com/PhysShell/griff/pull/18#discussion_r3345343491
- #3 — https://github.com/PhysShell/griff/pull/18#discussion_r3345343494
