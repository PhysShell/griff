# TonalContext diagnostic scan (read-only)

Throwaway experiment tooling. Not a production key detector: no cadence, chord
inference, or model. Production behavior unchanged.

## Harness
`cli/examples/tonal_scan.rs` (corpus-build). Imports a tab/MIDI and, at three
evidence scopes, emits pitch-class evidence + two deterministic tonal baselines.

- **scopes**: `all_tracks` (all tracks combined) · `track_i` (each track) ·
  `track_i_v0` (each track's primary voice).
- **per scope**: note count, sounding ticks, pitch range, distinct pitch-class
  count, and four pitch-class histograms — `raw_pc` (note count), `dur_pc`
  (duration-weighted), `onset_pc` (metric-accent weighted: on-beat notes ×2),
  `first_bar_pc`, `last_bar_pc`.
- **tonal baselines**: Krumhansl-Kessler major & minor key-profile correlation
  (Pearson) over the **duration-weighted** histogram across all 24 keys →
  `key_tonic` / `key_mode` / `key_score`; `confidence_margin` = best − second.
- **top-5**: the five most-confident `(scope,key)` findings per input.

`tonal_scan <input>` → `tonal_evidence.jsonl` (one line/scope + a `top5` line).

## Inputs
3 real (DGD / Wolf & Bear / Hail The Sun) + 4 synthetic fixtures
(`synth_wide_diatonic`, `synth_wide_pentatonic`, `synth_wide_chromatic`,
`synth_narrow_diatonic`). See `tonal_summary.csv` for the top-5 per input.

## Findings
1. **`all_tracks` palette = 12 pitch classes for all three real inputs** — a data
   property, not a tool artifact: a tab combines several instrument tracks
   (guitar, bass, …) plus bends / chromatic passing tones, so their union covers
   every pc. Synthetic fixtures keep their authored palette (diatonic 7,
   pentatonic 5, chromatic 12).
2. **The duration-weighted KS baseline still resolves a key despite the 12-class
   union** — weighting by sounding time surfaces the tonic: DGD E-minor
   (score 0.92, margin 0.29), Wolf & Bear C#-minor (0.79 / 0.16), Hail The Sun
   D-major (0.75 / 0.10).
3. **A single track can be MORE informative than all-tracks.** Wolf & Bear: the
   best finding is **`track_0` C#-minor (margin 0.16)**, while `all_tracks`
   yields a *different, weaker* key (F#-major, margin 0.07, rank 5). DGD /
   HTS: the guitar track dominates, so `all_tracks` ≈ the guitar track. In these
   tabs `track_i` == `track_i_v0` (single-voice tracks), so **voice** splitting
   adds nothing here — **track** selection is what matters.
4. **Confidence margin is a real signal.** Synthetic validation: diatonic and
   pentatonic → C-major (correct) with margins 0.04–0.09; **uniform chromatic →
   margin 0.0003** — the baseline honestly reports "no tonal center" when the
   palette is flat, and a clear key when it is not.

## Takeaway (for a future TonalContext, not built here)
Tonal evidence should be gathered **per instrument track** (duration-weighted),
not from the all-tracks union, and gated on the **confidence margin** — a low
margin (as on a flat/chromatic palette) means "no reliable tonic," which the
generator's pitch-material extraction currently ignores (it takes the full
chromatic union). No cadence / chord / model implemented.

## Artifacts
`tonal_evidence.jsonl` · `tonal_summary.csv` · this file.
