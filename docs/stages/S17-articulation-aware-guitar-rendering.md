# S17: Articulation-aware guitar rendering

Status: planned
Depends on: S3, S8, ADR-0018
ADRs: future implementation ADR required before embedding a renderer or shipping
third-party assets

## Goal

Turn a canonical symbolic `Score` into a useful guitar audition while preserving
Griff's symbolic-first architecture.

S17 is an optional rendering and audition layer, not a new generation model and
not a second score hierarchy. The canonical score remains the source of truth;
the renderer compiles its notes, positions, techniques, timing, and dynamics
into a target-specific performance plan and then delegates playback to a sampler
and, optionally, an amp/cabinet backend.

The practical target is Guitar-Pro-like feedback: palm mute, harmonics, dead
notes, tapping, slides, bends, vibrato, let-ring, velocity layers, and repeated
notes should sound materially different when the selected instrument supports
them. Unsupported semantics must be reported honestly rather than flattened
silently into generic MIDI notes.

## Architecture

```text
canonical Score
  -> GuitarPerformanceCompiler
  -> GuitarPerformancePlan
  -> versioned ArticulationProfile
  -> SamplerBackend
  -> optional AmpBackend
  -> audition audio
```

### GuitarPerformanceCompiler

A pure, deterministic compiler over the canonical model. It decides *what the
player does*, without knowing any bank's keyswitch layout.

Its input includes:

- master timeline and exact note timing;
- pitch, velocity, note marks, and technique spans;
- string/fret position and tuning where present;
- explicit versus inferred technique evidence;
- renderer capabilities and a selected articulation profile.

Its output is a typed `GuitarPerformancePlan`, not raw MIDI bytes. The plan may
contain note attacks/releases, articulation changes, continuous pitch curves,
controllers, string-selection hints, and explicit losses or approximations.

### ArticulationProfile

A versioned data file mapping Griff semantics to one concrete instrument:

```text
PalmMute        -> keyswitch / CC / channel rule
DeadNote        -> target articulation
NaturalHarmonic -> target articulation
Tap             -> target articulation
Bend            -> pitch curve or target-specific control
LetRing         -> note-release policy
```

Bank-specific note numbers, controllers, channels, and filenames do not belong
in `griff-core` enums. Profiles are adapters and must be replaceable without
changing the canonical model.

### SamplerBackend

A backend consumes the performance plan and an instrument selected by the user.
The first target is an external native/offline SFZ path, because it proves the
semantic mapping before Griff adopts a realtime engine.

Candidate prior art, not dependencies selected by this stage declaration:

- [sfizz](https://github.com/sfztools/sfizz) / its C and C++ library API;
- [sfizz-render](https://github.com/sfztools/sfizz-render) for an early offline
  MIDI-to-WAV experiment;
- the open [SFZ format](https://sfzformat.com/) for sample regions, velocity
  layers, round robins, keyswitches, controllers, and release behaviour.

The sfizz repositories are permissively licensed, but their upstream projects
and maintenance status must be re-evaluated at implementation time. A sampler
engine licence never grants rights to redistribute an instrument's samples.

### AmpBackend

The sampler should preferably produce a clean/DI-like guitar signal. Amp,
cabinet, IR, and effects remain a separate optional backend so performance
semantics and tone are independently replaceable.

[Neural Amp Modeler](https://www.neuralampmodeler.com/the-code) is a candidate
prior-art family for later native amp modelling. S17 does not adopt it by this
roadmap entry, does not introduce neural generation, and does not require model
training.

## Planned slices

### Slice 0 — renderer and asset audit

Before production code:

- evaluate current SFZ/sampler options, supported opcodes, maintenance, build
  targets, realtime safety, and licences;
- evaluate one or more legal user-supplied guitar banks without committing their
  samples to the repository;
- inventory which ADR-0018 techniques each candidate bank can actually render;
- define a small capability matrix and explicit fallback/loss semantics;
- write the implementation ADR and dependency/licence decision.

No Guitar Pro RSE assets, extracted proprietary banks, Kontakt-only libraries,
or ambiguously redistributable samples enter the repository.

### Slice 1 — semantic performance plan

Introduce the pure compiler and a versioned articulation-profile schema.

The first profile subset should cover only semantics supported by the selected
POC instrument, likely:

- sustain/default attack;
- palm mute;
- dead note;
- natural and pinch harmonics;
- tapping;
- accents/velocity;
- let-ring/release policy.

Bends, vibrato, slides, hammer-ons, pull-offs, legato transitions, strumming,
and string-specific sample selection land only when the target protocol can
represent them honestly.

### Slice 2 — external offline SFZ POC

Native-only vertical slice:

```text
Score -> GuitarPerformancePlan -> profile mapping -> temporary MIDI/control
stream -> external renderer -> WAV -> cockpit audition
```

The temporary MIDI/control stream is a renderer adapter, not the internal model.
It must carry a render loss report and be reproducible from the immutable score,
profile version, bank identity, and render settings.

The bank is user-supplied by path. Griff does not redistribute it.

### Slice 3 — cockpit audition integration

Add rendered-audio audition alongside the existing basic native MIDI and Web
Audio backends:

- explicit backend/bank/profile selection;
- fallback when the renderer or bank is unavailable;
- Generate, Swang, history, A/B, seek, loop, and tempo behaviour remain based on
  the same immutable candidate score and master timeline;
- cache keys include score identity, articulation-profile version, bank identity,
  render settings, and tone-chain identity;
- stale UI state must never be used to reconstruct the render request.

This slice does not change candidate ranking, history verdicts, or provenance
semantics owned by S8/S9.

### Slice 4 — embedded realtime sampler

Only after the external POC demonstrates useful articulation-aware audition:

- embed or directly host a compatible sampler backend;
- schedule plan events without an unnecessary MIDI file roundtrip;
- preserve sample-accurate onset/duration and deterministic event ordering;
- support stop/seek/loop with complete voice and articulation reset;
- keep native and wasm capability differences explicit.

A CLAP-facing audio renderer is a later integration choice. S10 remains a
MIDI-oriented plugin stage unless a superseding ADR changes that contract.

### Slice 5 — amp/cabinet chain

Add an optional post-sampler tone chain:

```text
sampled DI -> amp model -> cabinet/IR -> output safety stage
```

The amp model, cabinet, and presets are independent assets with their own
licences and provenance. Renderer tests must remain possible without them.

### Slice 6 — redistributable demo instrument

A bundled instrument is optional and may ship only after a written asset audit
proves that Griff may redistribute every sample, script, preset, impulse
response, and model involved.

If no suitable bank exists, record a small purpose-built DI demo bank rather
than laundering a "free download" into an MIT repository.

## Determinism and provenance

For a fixed:

- canonical score snapshot;
- selected track(s);
- performance-compiler version;
- articulation-profile id and version;
- bank identity/version;
- render settings;
- amp/cabinet identity;

the `GuitarPerformancePlan` must be deterministic and fully inspectable.

The audio file need not be byte-identical across every platform/backend, but the
semantic event plan, asset identities, losses, and requested render parameters
must be reproducible.

Renderer provenance must be captured when the render starts. It must never be
reconstructed later from mutable UI controls or a newly selected bank.

## Loss and fallback rules

- Unsupported articulation is an explicit loss or approximation.
- Missing fret/string position is not silently invented by the renderer; use an
  accepted position-inference result or report the limitation.
- A bank that lacks an articulation may fall back to a documented generic layer,
  but the loss remains visible.
- Continuous/polyphonic bends that the target protocol cannot express are
  reported; MPE or per-note channels are future options, not assumed.
- The existing simple playback remains available when no external assets are
  configured.

## Asset and licence rules

- No Guitar Pro RSE assets or reverse-engineered proprietary sound banks.
- No third-party sample, IR, preset, or amp-model asset is committed merely
  because it can be downloaded without payment.
- Engine code, sample content, profiles, presets, and models are audited as
  separate licensed works.
- The initial POC uses user-supplied paths and stores no copyrighted sample data
  in history, provenance sidecars, tests, or fixtures.
- Tests use synthetic controls and tiny purpose-made fixtures, not commercial
  instrument content.

## Acceptance criteria

### Semantic compiler

- Fixed score + profile produces the same typed performance plan.
- At least five supported techniques produce observably distinct target events.
- Per-note marks and spanning techniques can coexist without one erasing the
  other.
- Timing, velocity, tuning, and explicit fret/string positions survive the
  compilation where the target supports them.
- Unsupported semantics produce a complete loss report.
- No profile-specific keyswitch/controller constants appear in the canonical
  model.

### External rendering POC

- A user-supplied SFZ instrument renders a fixed canonical fixture to playable
  audio through one documented profile.
- The same fixture without a bank falls back cleanly to existing playback.
- No sample-bank asset is copied into the repository or output provenance.
- Renderer failure, missing files, unsupported controls, and malformed profiles
  return typed errors and never corrupt the active score/history candidate.

### Cockpit integration

- A/B compares the same candidate pair under the same captured render context.
- Seek, loop, stop, candidate switches, and backend switches leave no stuck
  voices or stale articulation state.
- A history item re-renders from its immutable candidate snapshot and captured
  render provenance, not current panel state.
- Existing S8 transport tests and symbolic export behaviour remain unchanged.

### Quality gate

- A small blinded audition set demonstrates that articulation-aware rendering is
  materially more useful for judging Griff candidates than the basic oscillator
  or generic General MIDI path.
- The result is reported as an evaluation of audition quality, not evidence that
  generation quality improved.

## Explicit non-goals

- Audio generation from text or waveforms.
- Replacing the canonical symbolic score with MIDI, SFZ, or audio.
- Importing techniques back from arbitrary keyswitch MIDI.
- Recreating or distributing Guitar Pro's proprietary RSE bank.
- Supporting every commercial virtual instrument profile.
- Making a sampler or amp dependency mandatory for core, CLI, or web builds.
- Training amp models or neural guitar generators.
- Changing S6/S7 ranking, S8 feedback semantics, S9 learning, or S10's plugin
  contract without their own scopes.

## Open questions

- Which maintained sampler/backend satisfies the licence, platform, realtime,
  and dependency posture when Slice 0 begins?
- Which user-supplied instrument is the best first profile for Griff's actual
  technique vocabulary?
- Should articulation profiles use TOML, JSON, or a typed compiled format?
- Which expression needs MPE/per-note channels versus ordinary pitch bend/CC?
- What render cache belongs in the cockpit without turning sample assets into
  hidden persistent project state?
- Is a redistributable demo bank worth recording before the S10 plugin, or only
  after articulation-aware audition proves product value?

## See also

- [`S3-guitar-pro-import.md`](S3-guitar-pro-import.md)
- [`S8-preview-app.md`](S8-preview-app.md)
- [`S10-clap-mvp.md`](S10-clap-mvp.md)
- [`../adr/0018-rich-note-model-fretboard-and-techniques.md`](../adr/0018-rich-note-model-fretboard-and-techniques.md)
- [`../decisions.log.md`](../decisions.log.md) — parked technique-aware export
  direction (2026-06-05)
