# TonalContext diagnostic scan (read-only)

Throwaway experiment tooling. Not a production key detector: no cadence, chord
inference, or model. Production behavior unchanged.

## Harness
`cli/examples/tonal_scan.rs` (corpus-build). Imports a tab/MIDI and, at three
evidence scopes, emits pitch-class evidence + two deterministic tonal baselines.

- **scopes**: `all_tracks` (all tracks combined) · `track_i` (each track) ·
  `track_i_v0` (each track's primary voice).
- **per scope**: note count, `sounding_ticks`, pitch range, distinct pitch-class
  count, and five pitch-class histograms — `raw_pc` (note count), `dur_pc`
  (duration-weighted), `onset_pc`, `first_bar_pc`, `last_bar_pc`.
- **`onset_pc` = metric-accent-weighted evidence** (on-beat notes receive ×2) —
  it is NOT a raw onset count.
- **`sounding_ticks` = summed note-duration mass**; it may double-count
  overlapping polyphonic notes. A clearer name is `duration_mass_ticks` — future
  output should use that; the current column keeps its historical name (documented
  here).
- **tonal baselines**: Krumhansl-Kessler major & minor key-profile correlation
  (Pearson) over the **duration-weighted** histogram across all 24 keys →
  `key_tonic` / `key_mode` / `key_score`; `confidence_margin` = best − second.
- **top-5**: the five most-confident `(scope,key)` findings per input.

`tonal_scan <input>` → `tonal_evidence.jsonl` (one line/scope + a `top5` line).

## Inputs
3 real (DGD / Wolf & Bear / Hail The Sun) + 4 synthetic fixtures
(`synth_wide_diatonic`, `synth_wide_pentatonic`, `synth_wide_chromatic`,
`synth_narrow_diatonic`). See `tonal_summary.csv` for the top-5 per input.

**No ground-truth key annotations exist for the three real tabs in this
experiment.** All key statements below are the KS baseline's *strongest tonal
hypothesis for that evidence scope*, not verified keys.

## Findings
1. **`all_tracks` palette = 12 pitch classes for all three real inputs** — a data
   property, not a tool artifact: a tab combines several instrument tracks
   (guitar, bass, …) plus bends / chromatic passing tones, so their union covers
   every pc. Synthetic fixtures keep their authored palette (diatonic 7,
   pentatonic 5, chromatic 12).
2. **The duration-weighted KS baseline returns a strongest tonal hypothesis even
   with the 12-class union** — weighting by sounding time surfaces a candidate
   tonic: DGD E-minor (score 0.92, margin 0.29), Wolf & Bear C#-minor
   (0.79 / 0.16), Hail The Sun D-major (0.75 / 0.10).
3. **Scope changes the strongest hypothesis.** Wolf & Bear:
   - `track_0` strongest hypothesis: **C#-minor, score 0.7866, margin 0.1557**;
   - `all_tracks` conflicting weaker hypothesis: **F#-major, score 0.7242,
     margin 0.0747** (rank 5).

   These are two different hypotheses; the larger margin does **not** make
   `track_0` "correct" (no ground truth). DGD / HTS: the guitar track dominates,
   so `all_tracks` ≈ the guitar track. In these tabs `track_i` == `track_i_v0`
   (single-voice tracks), so **voice** splitting adds nothing here — **track**
   selection is what changes the evidence.
4. **The confidence margin varies with evidence and can expose a near-tie.**
   Synthetic: `synth_wide_diatonic` → C-major margin **0.0757**;
   `synth_wide_pentatonic` → C-major margin **0.0854**;
   `synth_wide_chromatic` (**near-balanced** — see note) → C-major margin
   **0.0003** (an essentially flat tie). No ordering between diatonic and
   pentatonic material is claimed — the numeric confidence classes remain
   **uncalibrated**; only the near-tie on a flat palette is meaningful.

### Fixture note (chromatic is near-balanced, not uniform)
`synth_wide_chromatic.mid` is C2..C6 (36..84), so pitch-class **C occurs 5 times
(5×480 = 2400 ticks)** while every other class occurs 4 times (1920 ticks). It is
therefore **near-balanced chromatic spanning all 12 pitch classes**, not exactly
uniform. Its ~0 margin still supports the ambiguity finding. An **exactly
balanced** chromatic control should be constructed programmatically in the
Phase-1 core tests.

## Takeaway (for a future TonalContext, not built here)
Tonal evidence is better gathered **per instrument track** (duration-weighted)
than from the all-tracks union, and should be **gated on the confidence margin** —
a near-zero margin (as on a flat palette) means "no reliable tonic." The
generator's pitch-material extraction currently ignores this (it takes the full
chromatic union). No cadence / chord / model implemented; confidence thresholds
are NOT calibrated and automatic scope selection is NOT approved.

## Artifacts
`tonal_evidence.jsonl` · `tonal_summary.csv` · this file.
