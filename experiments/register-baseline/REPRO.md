# Full-range scale ladder — paired register A/B (reproducibility package)

Throwaway corpus-experiment record. Private corpus + source tabs are **not**
committed (ADR-0005); the synthetic fixtures here contain no corpus material.

## SHAs
- **before-ladder** generator: `53c81aa` (rotation + diagnostics, pre-ladder)
- **after-ladder** generator: `0f3637f` (feat `293843d` full-range scale ladder)
- **harness** (analyze + register_scan): `4f1580b` on branch `corpus-build`
  (both example sources built at each generator tip via a worktree so the
  candidate seam matches the generator under test)

## Config (identical on both arms)
- inputs: 3 real (DGD / Wolf & Bear / Hail The Sun, in the private corpus) + 2
  synthetic (`fixtures/synth_wide_diatonic.mid` span 48 / 7 pitch-classes,
  `fixtures/synth_narrow_diatonic.mid` span 11 / 7 pitch-classes)
- seeds 1..5 · gesture {on, off} · **10 variants per strategy = 50 candidates
  per condition** (5 strategies) · bars 8
- corpus record-list hash (sorted `*.chunk.json` names, no content):
  `213bd8572c95ebb74151a84997ace78067383fc534ed67ef359188777c64f5ed`

## Commands
```
# candidate-level (per arm, register_scan built at that tip):
register_scan "<input>" --corpus <corpus_dir> --seeds 1,2,3,4,5 --variants 10
# winner-level (production rerank, real griff binary per arm):
griff generate "<input>" out.mid --seed S --bars 8 --corpus <corpus_dir> [--no-gesture]
analyze out.mid --input "<input>"
```

## Raw output checksums (sha256, first 16 hex)
- `reg_before.jsonl`  (candidate-level, before)  `4a243f6ab015dd53`
- `reg_after.jsonl`   (candidate-level, after)   `3258f6864a08917d`
- `winners.jsonl`     (winner-level, both arms)  `6ec4c4610f27cb80`

## Fixtures
`make_fixtures.ps1` regenerates the two MIDIs (format-0, PPQ 480, quarter notes,
tempo+time-sig meta required or the MIDI importer rejects them). Wide = C major
C2..C6 (7 classes, ~4 octaves); narrow = C major C4..B4 (same 7 classes, 1
octave).

## Caveat (material seam)
`register_scan`'s corpus **rhythm** (bar-durations → `RhythmTemplate::from_durations`)
and **gesture** (mean of all `GestureControl`) diverge from production's
`load_corpus_material` (placed template; median over rest-bearing chunks). It is
identical on both arms, so the candidate-level DELTA isolates the ladder; but
absolute candidate note-counts are harness-conditioned. The **shared PITCH seam**
(`ScaleLadder`, used by `generate_candidate_set`) IS production-exact, so all
pitch/register metrics are faithful. The winner-level A/B uses the real `griff`
binary (production material compiler) and is fully faithful. A shared
`corpus_material(dir)` seam in `core` would make the candidate-level arm
production-exact too.

## Verdict
`reachability fixed` (union span across variants → full input range, all
strategies; in-class/in-bounds = 1.0) **combined with** `winner reranking
amplifies register defects`: ShuffleMotifs shuffles across the full ladder
(candidate octave-leap-share 0→0.62) and the register-blind rerank policy then
selects it more (winner ShuffleMotifs 8→14, winner octave-leap-share 0→0.137,
10/50 winners with 45–53 semitone leaps, aggregate 0.879→0.894). Fix pairs:
(1) local-window shuffle for ShuffleMotifs; (2) a register-coherence axis in the
rerank policy.
