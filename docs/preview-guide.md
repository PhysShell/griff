# `griff-preview` — interactive piano-roll guide

`griff-preview` (the `preview/` crate) is a headless-testable, `ratatui`
terminal piano-roll for a single `.mid` file: it draws the score as a
pitch × time grid, classifies sections (Riff / Solo / Breakdown / Clean),
measures structure and complexity, and — with a chunk record attached —
lets you **re-curate** that record interactively. There is **no audio**;
griff is a symbolic model (MIDI in → MIDI out).

Press **`?`** inside the app for the keybinding cheatsheet at any time.

## Launch

```text
griff-preview <file.mid>                                  # interactive TUI
griff-preview <file.mid> --snapshot=120x40                # one headless frame to stdout, then exit
griff-preview <file.mid> --record=<chunk.json>            # re-curation: a/x, t/T, r, s persist into the record
griff-preview <file.mid> --record=<chunk.json> --merge=<next.json>  # also unlocks m (merge with <next>)
griff-preview -h | --help                                 # usage line
```

From a checkout:

```bash
cargo run -p griff-preview -- path/to/riff.mid
```

The curation keys edit the **`--record` file** (a corpus `ChunkMeta` JSON),
**not** the MIDI; the MIDI is shown only for context. Without `--record`,
tagging / rename / split / merge are inert (see *Gotchas*).

## Keybindings

| Key(s)            | Action                                                  |
| ----------------- | ------------------------------------------------------- |
| `?`               | Toggle the help overlay (any key closes it)             |
| `q` / `Esc`       | Quit (writes pending curation to disk)                  |
| `Space`           | Play / pause (from the end, replays from the start)     |
| `←` / `h` · `→` / `l` | Scroll left / right in time                         |
| `↑` / `k` · `↓` / `j` | Move the visible pitch band up / down               |
| `+` / `=` · `-` / `_` | Zoom in / out (fewer / more ticks per column)       |
| `[` · `]` / `Tab` | Previous / next named section (jumps to it)             |
| `0` / `Home`      | Jump to the start of the score                          |
| `i`               | Show / hide the inspector dock                          |
| `PageUp` / `PageDown` | Scroll the inspector up / down                      |
| `a`               | Approve the chunk (press again to clear)                |
| `x`               | Reject the chunk (press again to clear)                 |
| `t`               | Move the tag cursor to the next palette entry (wraps)   |
| `T` (Shift+`t`)   | Toggle the tag under the cursor                         |
| `r`               | Rename the record (enter rename mode)                   |
| `s`               | Mark / unmark the split point at the playhead           |
| `m`               | Toggle the merge with the `--merge` partner             |

**Rename mode** (after `r`): type to edit, `Enter` commits (trimmed,
non-empty), `Esc` cancels, `Backspace` deletes. Inside this mode `q` is a
literal character, not quit.

## Navigation & playback

- **Sections** (`[` / `]` / `Tab`) jump between auto-classified spans. The
  coloured band under the header is the section map; the selected one is
  underlined. Colours: Riff = blue, Breakdown = red, Solo = orange,
  Clean = green, Unknown = grey.
- **Playback** (`Space`) advances a yellow playhead; the view autoscrolls to
  follow it while playing. Scroll manually when paused.
- **Zoom / pan** are independent. On launch the zoom is chosen so the whole
  piece fits the plot width.
- The header reads: file · `♩=` tempo · bar count · `bar:beat` position.

## The inspector (`i`)

A 32-column dock on the right (auto-hidden when the window is too narrow;
bring it back with `i`). Top to bottom:

- `track` — the focus track's name.
- The current section's class badge and `bars X–Y · N bar(s)`.
- `curation` — the decision pending **this session** (`a` / `x`).
- `transport` — tempo and `▶ playing` / `⏸ paused`, plus `pos bar:beat`.
- **Record digest** (only with `--record`): `record <title>` (or
  `name▸ <buffer>█` while renaming), `review <prior decision>`,
  `tags <active>`, the tag cursor `tag▸ <name> [x]/[ ]`, and any pending
  `split▸ at bar N` / `merge▸ + <partner>`.
- `structure (S14)` — `loopability`, `repeatability`, `variation`,
  `complexity`, and the detected `pattern` period.
- `complexity (S14)` — the six-axis profile (below).

Note the two decision lines: `curation` is what you are about to write this
session; `review` is what the loaded record already held. On a short
terminal the metric tail clips — reach it with `PageDown`.

## Complexity is measured automatically

You do **not** type complexity in. It is computed from the notes at import
time (`core::structure::measure_complexity`) and is read-only in the TUI.
The inspector shows it in two places:

1. The `complexity` line inside the **`structure (S14)`** block — a single
   scalar: the distinct-bar-signature ratio (`distinct / total`). It is the
   same value as the `str` axis below.
2. The **`complexity (S14)`** block — a six-axis vector (a fact for
   reranking, not a verdict; ADR-0015). Each axis is in `[0, 1]`, shown as a
   percentage:

| Axis  | Meaning              | How it is computed                                                                 |
| ----- | -------------------- | ---------------------------------------------------------------------------------- |
| `rhy` | rhythmic variety     | variety of inter-onset intervals over distinct onsets: `(distinct − 1)/(count − 1)`|
| `pit` | melodic variety      | the same variety over absolute melodic intervals of the highest-pitch-per-onset line |
| `tec` | technicality         | share of notes carrying a mark or inside a technique span of the same voice        |
| `har` | chromaticism         | `1 − scale_fit` of the estimated key (Krumhansl–Schmuckler, duration-weighted)     |
| `ply` | playability          | `max_fret_jump / 12` on the optimal fingering path; `1.0` if a note is unreachable  |
| `str` | structural           | distinct-bar-signature ratio (same as the `structure` block's `complexity`)        |

Whenever an extent changes (after a split or merge), the record's
`structure` / `gesture` / `complexity` fields are **reset**: each result is
a new record to review and re-measure.

## Curation: `curate` vs. re-curation

There are two entry points:

- **`griff curate <file.mid>`** (the CLI, `cli/`) **creates a new**
  `ChunkMeta` record from a MIDI file. It prompts for id, title, tuning
  (default `standard_e`), cohort, tags, quality flags, and reviewer
  decision, and **auto-measures** structure / gesture / complexity. It
  writes `<file>.chunk.json` (or `--output`; `--ensemble` writes one chunk
  per note-bearing track plus a group record with measured pair relations).
- **`griff-preview … --record=<chunk.json>`** (this TUI) **re-curates an
  existing** record. Edits accumulate during the session and are written
  **on quit** (`q` / `Esc`) — there is no autosave mid-session.

### Approve / reject — `a` / `x`

Marks the record `accepted` / `rejected`. The same key again clears it; the
other key overwrites. Persisted into `reviewer` on quit.

### Tag — `t` / `T`

`t` cycles the cursor through the 27-tag palette; `T` toggles the tag under
the cursor. On quit a changed set rewrites the record's `tags`. The palette
(`SwancoreTag`, in order):

> **style:** `clean_riff` · `syncopated_riff` · `tapping_passage` ·
> `legato_passage`
> **harmony:** `maj7` · `min7` · `sus2` · `add9` · `slash_chord` ·
> `power_chord`
> **technique:** `hammer_on` · `pull_off` · `slide` · `bend` · `vibrato` ·
> `palm_mute` · `natural_harmonic` · `artificial_harmonic`
> **rhythm:** `syncopated` · `triplet_feel` · `polyrhythm`
> **structure:** `intro` · `verse` · `chorus` · `bridge` · `outro` ·
> `interlude`

### Rename — `r`

Enters rename mode seeded with the current title. `Enter` commits (trimmed,
non-empty), `Esc` cancels. Persisted into `title` on quit.

### Split — `s` (one record → two)

Place the playhead and press `s` to mark the split point (the same spot
again clears it). The mark floors to the containing bar. **Gates**
(`viewport.rs`): the first bar cannot be split (the point must be on the
second barline or later), and a point in a note's ringing tail past the
final barline is refused. Mutually exclusive with merge.

On quit (`split_record_at_tick`): the first half `[start, at_bar − 1]`
**replaces** the record file (id suffix `.1`, title `… (1/2)`); the second
half `[at_bar, end]` lands in the first free sibling slot `chunk.2.json`,
`chunk.3.json`, … (id `.N`, title `… (2/2)`). Both halves reset their
decision, measurements, and ensemble link.

### Merge — `m` (two adjacent records → one)

Requires **both** `--record` and `--merge=<partner.json>`. `m` arms /
disarms it; mutually exclusive with split. **Conditions** (`merge_records`):
both records share a source (filename, format, ticks-per-quarter, time
signature, tuning — else `MergeMismatch`), their bar ranges are
**consecutive** (`a_end + 1 == b_start` — else `NotAdjacent`), and both
carry a `source.bar_range` (else `MissingBarRange`).

The first record's identity wins; tags / techniques / quality flags union;
cohort and ensemble survive only on agreement; decision and measurements
reset. On success the partner **file is absorbed** (deleted).

## Gotchas

- **Without `--record`**, `a` / `x` only update the inspector indicator and
  persist nothing; `t` / `T`, `r`, `s` are no-ops; `m` also needs `--merge`.
- **No autosave** — everything is applied on quit. If a rewrite cannot apply
  (e.g. split / merge on a record with no `bar_range`), it fails loudly to
  stderr and the files are rolled back, not corrupted.
- **Split and merge are mutually exclusive** — one record rewrite per pass.
- On a narrow terminal the inspector auto-hides (`i` restores it) and the
  complexity tail scrolls into view with `PageDown`.

## Source map

- TUI render + key map + help overlay: `preview/src/tui.rs`
- Interaction core (intents, viewport state, gates): `preview/src/viewport.rs`
- Curation persistence (tag / rename / split / merge): `preview/src/curation.rs`
- Launch, flags, on-quit persistence: `preview/src/main.rs`
- Complexity / structure measurement: `core/src/structure.rs`
- `griff curate` CLI: `cli/src/main.rs`
- Design: ADR-0016 (shared UI core), ADR-0015 (structure controls & metrics),
  stage `docs/stages/S8-preview-app.md`.
