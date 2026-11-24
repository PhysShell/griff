# GUITAR-RENDER-01 probe

Date: 2026-11-24. Branch `research/guitar-render-01` (from origin/main 552831d). Local commits only.
Audio and third-party files: `_griff_out/guitar-render-01/` (outside git).

## 1. Input piece and stems

- Generated with `python composer.py --seeds 1-8 --v8 --out _griff_out/guitar-render-01/piece --prefix v8`.
  `v8_002` is the only one of the 8 with exactly three guitar tracks: `Guitar 1`, `Guitar 2`,
  `Guitar 2 (palm mute)` (7/8, B mixolydian, 153 BPM, one tempo, PPQ 480, 33.7 s with a 3 s tail).
- Stems are the three MIDI tracks. Roles by content (`midi_checks.json`): guitar1 = lead line
  (116 notes, slides, pull-offs, max polyphony 4); guitar2 = arpeggios and chords (197 notes, 45 chord
  onsets, up to 6 voices); guitar3 = the palm-muted low riff of guitar 2 in the B section (52 notes, 16.2-22.1 s).
- Articulations: MIDI gives notes, PM (track name, composer-lab exports PM notes to their own track);
  MusicXML gives string/fret, hammer-on, pull-off, slide, tap, dead (`notehead x`). 359 of 365 MIDI notes
  were matched to a MusicXML note by (part, onset, pitch); the 6 unmatched are the final chords of both guitars at tick 30240,
  which were not found in the MusicXML (not investigated further). MusicXML marks only the start of a P.M. run, so PM is taken from MIDI.
- Separate-file input: splitting v8_002 into three MIDI files and running `prepare.py` with three `--input`
  gives byte-identical stem MIDI to the single type-1 file.
- Griff reuse: Griff's MIDI import (`core/src/midi.rs`) is Rust and keeps one tempo per bar (#224), and
  there is no Python MusicXML reader in Griff; `grender/smf.py` keeps the full tempo map (checked by the
  smoke test's mid-bar tempo change) and `grender/mxl.py` reads only composer-lab's own MusicXML.

## 2. Backends

### A: Unirock HumStrat (P0 only, not executed)

Facts:
- `Pro2Max_Unirock_HumStrat_1.0.0_Portable.zip` 1,716,916,418 B, SHA-256
  `36611f8b4fcd19b5d0d82486094dd891b0d3a0e72122e559892c7ae18d845253`; zip test passes; 339 entries:
  the VST3 bundle, `SampleLibrary/` (327 WAV, 1.70 GB), README.md, README_PORTABLE.txt, LICENSE,
  interface_screenshot.png. No installer, no standalone exe in the portable zip.
- `Unirock_Humstrat-1.0.0.zip` 547,759 B, SHA-256 `6ca5f9b775102c7ff9127dba30f616767d341fd4aeb2705b5e9447bc2caa215a`
  is the GitHub source-tag zip (README, LICENSE, screenshot); README and LICENSE are byte-identical to the portable ones.
- Both carry Zone.Identifier ZoneId=3 with referrer `https://github.com/Pro2Max/Unirock_Humstrat/releases/tag/1.0.0`.
- Plugin binary 14,860,288 B, SHA-256 `713e506928c673f3df3955cf60919bd76ead7353912541ae812bcede1139c769`,
  Authenticode: NotSigned. Version info Pro2Max / 1.0.0. Built with JUCE v8.0.10 (string), VST SDK 3.7.12
  (moduleinfo.json), class IDs with JUCE's `ABCDEF01` prefix. Imports include WININET and WS2_32 (JUCE's
  URL classes import them; no hard-coded URL other than generic schema strings was found).
- Sample path rule: SampleLibrary beside the .vst3 bundle, or `C:\ProgramData\Pro2Max\Unirock HumStrat\SampleLibraryPath.txt` (installer).
- Keyswitch table in the downloaded README matches the task: pickups 12-16, PickScrape 18/19, articulations
  24-32; Slide Down (30) is momentary on a ringing note; Legato Slide (32) is overlap-driven; playable from
  MIDI 33; pitch bend default 2 semitones (1-12 in the settings panel); README says "physically modelled"
  but ships a sample library.
- `instruments/unirock.json` + `prepare.py --instrument-all unirock` produce the keyswitch MIDI that a
  Unirock render would use (`_griff_out/.../prep/unirock_mapping_{strict,expressive}`); EXPRESSIVE extends
  the note before each Legato Slide arrival so the notes overlap (19 + 3 notes). Not rendered.

### B: sfizz

- sfizz 1.2.3 from nixpkgs 26ef669 (the auditbox's pin), binary `sfizz_render` (cache.nixos.org build).
  CLI (`--help`): `--sfz --midi --wav -b -s -q -p -v --log --use-eot`; there is no `--version`.
  Output is 16-bit PCM stereo WAV (not configurable from the CLI).
- Standard Guitar (Unreal Instruments): the Google Drive download returned "quota exceeded" for every try
  in this run (no account involved). The owner downloaded `UI_Standard_Guitar.rar` (750,502,921 B, SHA-256
  `0777b8cbbfe2fe79e843fa6c2c24ab300c2d5a538d46f4d970f057979f87fee0`) and the manual RAR. Both were tested
  and unpacked inside the offline auditbox with unar 1.10.8 (`jobs/sg_extract.sh`; 3,714 entries passed:
  2,953 FLAC, 612 SFZ). Licence "Custom", no terms found: private use only, nothing committed.
  Program `01-Standard Guitar KSOP.sfz`; keyswitches from `Standard_Guitar_KSMap.txt` and the `sw_last`
  values (c1 = 24): 20 natural harmonics, 21 pinch harmonics, 23 fret mute (dead), 24/25/26 sustain
  down/up/alternate, 27/28/29 palm mute down/up/alternate, 30 hammer/pull, 31 slide down, 32 slide up,
  33 slide in, 34 slide out; playable 35-86 (7-string range). Mapping in `instruments/sg_ksop.json`:
  sustain 26, palm mute 29, dead 23, hammer/pull/tap 30, slides 31/32 by direction from the MusicXML slide start.
  Samples are stereo doubled takes (left and right differ); the DI takes the left channel.
  Attack: 23-30 ms after note-on in the first smoke test; after a 25 ms DI advance most clear notes sit
  within -2..+11 ms (the detector is crude on repeated notes).
- Also used (and used first, while Standard Guitar was blocked): Karoryfer Black And Green Guitars v1.000 (CC0, GitHub release of sfzinstruments, listed in the
  owner's task as an alternative): a green Gretsch Anniversary and a black Hofner Club, mono 44.1 kHz 24-bit
  DI samples. Keyswitches: green 36 twang / 37 staccato / 38 hammer-on; black 36 twang / 37 staccato /
  38 behind-the-bridge. No palm-mute, slide, pull-off, dead or tap samples; bend ranges are set per region
  (from about 20 to 320 cents), so pitch-bend slides are not used.

## 3. Amp, cabinet, effects

- NAudio.Nam 3.4.0-preview.1 (NuGet, 2026-10-24, MIT, net9.0; repo commit 1190984f) with NAudio.Core
  3.4.0-preview.1 and System.Numerics.Tensors 9.0.0. Released as a prerelease package, not only in the branch.
  Run by `namrender` (own 100-line C# console app) on the .NET 9.0.15 runtime from nixpkgs.
- Friedman Smallbox 50 (tone3000 tone 75761, user music2234): two captures, Ch1 and Ch2 "Trans-DI",
  NAM file version 0.7.0, `SlimmableContainer` holding A2-Lite (3 channels, 1,871 weights) and A2-Full
  (8 channels, 12,146 weights), 48 kHz, amp only (no cab), no input-level calibration in the metadata
  (`input_level_dbu` null), loudness -10.1 / -8.5 dB, trainer gain 0.41 / 0.78. NAudio.Nam loads both.
- Cabinet: guitar-on-demand free pack 01, `P_GOD_2xV30_SM57_Edge_30deg_02cm-48k_f.wav`
  (Harley Benton 2x12 Celestion V30, SM57), 2048 taps, 48 kHz; alternative Laney Ironheart 2x12.
  Convolution: scipy `fftconvolve`, IR scaled to unit energy.
- EQ, compression, chorus, phaser, distortion, delay, reverb: Spotify pedalboard 0.9.26.
- Gain staging: one gain per DI track to -9 dBFS peak before the amp (never per note), then the preset's
  `input_db`; after the chain one gain per stem to -20 LUFS; the mix is scaled down as a whole only if its
  peak passes -1 dBFS. There is no capture calibration to match, so -9 dBFS is an assumption.

## 4. Measurements (offline auditbox guest, 4 vCPU, 8 GB; results in `_griff_out/guitar-render-01/out/`)

Speed, 33.7 s piece, three stems (`out/bg/v8_002_strict`, `out/repro`):
- Whole render (3 x sfizz, 3 x NAM, IR, EQ, mix, MP3): 11-14 s wall, RTF 0.33-0.42.
- sfizz_render per stem: 1.8-2.4 s warm, 3.3-4.1 s for the first stem of a VM (cold page cache);
  RTF 0.05-0.12; peak RSS about 600 MB (sample preload).
- NAudio.Nam A2-Full: 0.9 s for 33.7 s (RTF 0.027); A2-Lite 0.45 s (RTF 0.0135); model load 0.03 s;
  process start of `dotnet namrender` included: 1.1 s per stem.
- Fixed costs per VM job: about 15-20 s boot, 17 s guest setup (unpack payload, pip install from wheels),
  plus host-side staging of the 560 MB library (hard links on WSL ext4).
- Dependencies: sfizz 8.5 MB (one store path; everything else is already in the guest image), .NET 9
  runtime 79 MB, Python wheels 63 MB, namrender + NAudio 0.9 MB, B&G library 560 MB, NAM 0.6 MB, IRs 62 KB.

Levels (`render-manifest.json` per run): no NaN/Inf, no samples at or above full scale in any WAV;
DC offset below 4e-4. DI peaks -12.8 to -19.6 dBFS (sfizz writes 16-bit). Mix -15.7 LUFS, peak -3.4 dBFS
(STRICT); SWAN-TEXTURE mix was scaled -2.7 dB to keep -1 dBFS.

Timing and notes (`smoke.py`, `out/bg/smoke_*`, `analyze.py onsets`):
- sfizz adds no measurable delay: with the built-in sine, the steepest rise is within 1 ms of the MIDI time.
- B&G samples: the attack peaks 21-22 ms after note-on for every note (green); after the 21 ms DI advance
  (`di_advance_ms`, applied to the whole stem) the lag is -1 to +2 ms for most notes, both guitars.
- Tempo change in the middle of a bar (120 -> 90 BPM at beat 3 of bar 2) lands where expected.
- Chords, fast repeats, same-pitch overlap, pitch bend, staccato -> sustain and legato -> sustain keyswitch
  round trips render; last signal 9.05-9.07 s against the last note-off at 9.00 s (release tail), so no hung notes.
- Bug found and fixed: a keyswitch sent as note-on and note-off at the same tick left a hanging voice when
  the keyswitch key also maps to a sounding region (sine test with all keys mapped: signal to the end of the
  file; with a one-tick keyswitch: silence after 9.03 s). B&G has no regions on 36-38, so the first mix was
  not affected, but keyswitches are now one tick long.
- The detector is crude: soft repeated 16ths on the same pitch are sometimes not seen as onsets.

Reproducibility (`out/repro`, 3 fresh processes, plus run 1 of `out/bg` in another VM):
- `guitars.mix.wav` (PCM24) and `guitars.mix.mp3`: byte-identical in all 4 renders (STRICT and EXPRESSIVE).
- float32 DI and processed stems: numerically identical (max difference 0.0) but not byte-identical:
  bytes 60-61 differ, which is the timestamp in the PEAK chunk libsndfile writes into float WAVs.
- So: byte-level for the mix and MP3, numeric for the stems, musical trivially.

Articulation mapping (`plan.json` -> `mapping_logs`):
- STRICT: notes, ticks and velocities unchanged; 30 keyswitches on guitar1 (slides and pull-offs ->
  hammer-on samples), 1 on guitar3 (palm mute -> staccato samples); guitar2's hammer-ons and slide fall back
  to picked notes (black guitar has no legato samples).
- EXPRESSIVE: same keyswitches, legato arrivals at 80 % velocity (19 + 3 notes), 4-tick (about 3 ms)
  down/up strum spread on chords of 3+ notes (129 note starts moved on guitar2). No random numbers.
- Every technique on B&G is an approximation (see `instruments/*.json`). Nothing claims a real palm mute or slide.

## 5. Isolation for Unirock (owner's decision)

- The auditbox is a Linux KVM guest; it cannot load a Windows VST3 without Wine, and Wine plus a JUCE 8
  Direct2D plug-in is a separate project. Not attempted.
- Windows Sandbox is present on this machine (`C:\Windows\System32\WindowsSandbox.exe` and the `wsb` CLI
  exist; the optional-feature state needs elevation to query, so it was not read). Nothing was enabled.
- Proposal: `unirock/unirock-sandbox.wsb.template` + `unirock/render_unirock.py` (drafts, not run):
  networking, vGPU, clipboard, printer, audio/video input off, ProtectedClient on, one read-only mapped
  folder (portable Unirock, an embeddable Python with the Windows pedalboard wheel, stem MIDI, the script)
  and one new empty writable folder for results; LogonCommand runs the render and shuts the sandbox down.
  Results come back as WAV and JSON only.
- Open points for the owner: (1) go/no-go for running an unsigned binary from an unverified publisher in
  Windows Sandbox; (2) the writable mapped folder is the one path back to the host; (3) the pedalboard
  Windows wheel and an embeddable Python would be fetched through the auditbox fetch VM first;
  (4) whether Defender should scan the extracted portable folder before mapping (it scans on access anyway).

## 6. What could go into Griff (assessment, nothing integrated)

- Worth keeping close to Griff: the articulation JSON (instrument-independent, built from Score/MusicXML
  data Griff already has), the per-instrument mapping files and the STRICT/EXPRESSIVE rules. These are small,
  testable and have no audio dependency.
- Keep external: sfizz, sample libraries, NAM captures, IRs, .NET runtime, pedalboard. They are large,
  have separate licences (T3K captures may not be redistributed; Unirock samples may not be extracted)
  and need the isolated render job. An offline "render this candidate" job that Griff calls and that writes
  WAV/MP3 plus a manifest fits #218 (offline analysis) better than any cockpit playback path.
- A C# Griff tool could call NAudio.Nam directly (managed, no native code); it would still need the DI
  from an external sampler.
- Gaps found on the way: the final chords at tick 30240 are not in composer-lab's MusicXML, the writer skips
  notes with fractional onsets (by its code), and it writes only the start of a P.M. run; Griff's MIDI import drops mid-bar tempo changes (#224).

## 7. Standard Guitar results and the owner's pieces

- `out/sg`: STRICT, EXPRESSIVE, no-FX, four SWAN presets, palm-mute solo; whole render 18-20 s for 33.7 s
  (RTF 0.53-0.60); sfizz 4-6.5 s per stem, peak RSS about 1 GB (more samples than B&G).
- `out/repro_sg`: 3 fresh processes plus the batch run in another VM: mix WAV and MP3 byte-identical;
  float stems numerically identical (PEAK-chunk timestamp only).
- One float stem (preset_sg_SWAN-CLEAN guitar3.processed.wav, the palm-mute riff through the clean preset after
  the -20 LUFS trim) peaks at +1.1 dBFS; it is float, the mix is not clipped.
- Owner's pieces (`out/owner/`, copies of `Downloads/piece_9285936.mid` and `piece_1619764.mid`, originals untouched):
  - piece_9285936: type 1, PPQ 480, one tempo 135 BPM, 7/8; Guitar 1 (88 notes), Guitar 2 (189),
    Guitar 2 (palm mute) (53). No MusicXML or recipe; only PM is known as a technique.
  - piece_1619764: type 1, PPQ 480, one tempo 144 BPM, 7/8 -> 5/4 -> 7/8; Guitar 1 (96), Guitar 2 (296).
    94 Guitar 2 notes have velocity 40 and one-sixteenth length, which is how composer.py writes dead strokes
    (`"vel": 40` with `preset_tech ["dead"]`); `--dead-velocity 40` maps them to the fret-mute keyswitch. Inference
    from the generator code, not from a recipe.
  - Variants: best (per-track presets), heavy (SWAN-HEAVY on all), clean (SWAN-CLEAN on all); STRICT only.
- Blind clips (`out/clips_v2`): one loudness per group, lowered for the whole group if any clip would pass
  -1 dBFS (the first SG DI clip set clipped at +2.6 dBFS and was regenerated).
