# ADR 0022: Repeat unfolding is a projection, not a model rewrite

Date: 2026-06-15
Status: Proposed

## Context

Real Guitar Pro tabs carry transport structure beyond meter and tempo:
repeated sections (`|: … :|×N`). Importing a real GP6 tab made this concrete —
four `|: :|×2` sections, none of which the model represented. The question is
not *whether* to capture repeats (ADR-0003 already names them master-timeline
transport) but *what the canonical model holds*: the **written** timeline or
the **played** one.

Two shapes were on the table:

- **(a) Unfold into the model.** Physically duplicate the repeated `MasterBar`s
  (and their notes) in `Score.master_bars`, so every consumer sees the
  as-played sequence for free.
- **(b) Keep written + markers.** Leave the bars folded, record the repeat
  barlines as metadata, and expand on demand.

Shape (a) is tempting but corrodes invariants the rest of the system leans on:

- The ADR-0020 import-validation harness diffs griff's dump against a reference
  parser (`PyGuitarPro`) that represents the **written** form. Duplicated bars
  would diverge the canonical model from the source file 1:1.
- Curation records anchor a `ChunkId` to bar positions; renumbering bars under
  expansion would silently move those anchors.
- ADR-0002 makes the canonical `Score` the single internal truth and treats
  derived views (the ADR-0020 dump, the retired linear layer) as *projections*.
  An as-played expansion is exactly such a derived view.

## Decision

The canonical `Score` stays **faithful to the written tab**; the as-played
timeline is a **projection** computed on demand.

1. **Markers on the master timeline.** A `RepeatMarker { start, play_count }`
   rides each `MasterBar` (ADR-0003). `start` is the `|:`; `play_count` is the
   total number of times the section closing on that bar is played (`:|×n`),
   `0` when the bar carries no close, a genuine repeat being `>= 2`. Only simple
   `|: … :|×N` repeats are modeled; alternate endings (voltas) and jump
   directions (D.C./D.S., coda/segno) are **not yet** represented — an importer
   that meets them leaves the marker at its default rather than guessing.

2. **Import normalizes the play count.** GP6 (GPIF) stores the raw play count;
   the GP3/4/5 binary reader stores one fewer (the `guitarpro` crate already
   decremented it). The importer bumps the binary path by one so the canonical
   `play_count` means the same thing — total plays — regardless of source
   format.

3. **Unfolding is a pure projection.** `unfold::played_bar_order(&score)`
   returns the played order of `MasterBar` indices after expanding repeats:
   identity (`0..n`) when there are none, a close with no preceding open
   repeating from the song start. It is defensively bounded so a malformed
   repeat map cannot allocate without end (SPEC robustness, ADR-0010).

4. **Written is the default view.** `inspect` shows the written bars; `--unfold`
   opts into the as-played sequence (a source bar then appears more than once).
   The product default is the tab as written; expansion is explicit.

## Consequences

- The canonical model stays 1:1 with the source: the ADR-0020 dump golden,
  curation `ChunkId` anchoring, and bar numbering are all untouched by repeats.
- "What actually plays" is available to every consumer — a future MIDI export
  on the master timeline (ADR-0003), the preview piano-roll, classify-over-
  played — from one shared projection, computed not stored.
- Accepted limitation: voltas and jump directions are not expanded yet and read
  as plain barlines. Because the markers live on the model, a later slice can
  expand them (and re-tick the unfolded timeline for export) without re-importing.
- Accepted: the play-count normalization is format-aware by necessity — a
  documented quirk of the `guitarpro` crate's per-format storage, pinned by a
  unit test on each path.
- Follow-ups, each its own slice: a tick-accurate unfolded timeline (re-ticked
  notes, not just bar order) feeding MIDI export and the preview; voltas and
  D.C./D.S.; and surfacing unmodeled directions as a `LossReport` entry.
