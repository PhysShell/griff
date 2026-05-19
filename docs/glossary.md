# glossary.md — griff project glossary

> Purpose: single source of truth for terms used by humans and LLM agents
> working on `griff`.
>
> Rule of use: if a term appears in the spec, an ADR, a stage doc, code, or an
> agent task, check this file first. Do not invent competing definitions. If a
> term is missing or ambiguous, extend this file rather than creating synonyms
> in code.

## 0. Project terms

### griff
The guitar riff engine project: a system to analyze, slice, generate, and
regenerate guitar riffs, targeted at swancore / DGD-like clean / prog /
post-hardcore parts. Internally it works on a structured musical model, not on
raw MIDI bytes.

### guitar riff engine
An engine that treats guitar riffs as musical objects — phrases, motifs,
rhythm cells, techniques, fretboard positions, transitions, variations — rather
than emitting random notes.

### swancore-first
Architectural principle: design features for expressive clean / prog /
swancore guitar parts first, not for "music generation in general". Drives the
data model, features, generator, corpus, and UI.

### DGD-like
Use structural traits similar to Dance Gavin Dance (fast clean riffs,
syncopation, register jumps, melodic intervals, tapping / legato-like motion,
unconventional phrasing) to produce original ideas — not to copy songs.

### MVP
Minimum Viable Product. In `griff`: a minimal working vertical slice —
import → internal model → analysis/slicing → generation/export → tests. Not
"a quick throwaway".

### vertical slice
An end-to-end piece of functionality through every layer of the system.

### roadmap stage
A development stage `S0`, `S1`, … Each stage has a goal, tasks, acceptance
criteria, tests, and constraints.

### S0
Baseline stage. Freeze current behavior via characterization tests, golden
snapshots, and smoke tests before refactoring.

### S1
Canonical score model stage. Introduce `Score / MasterBar / Track / Voice /
EventGroup` without breaking existing code in one stroke.

### S2
MIDI transport refactor stage. Move MIDI import/export onto the master
timeline and canonical score model.

### S3
Guitar Pro import stage. Add Guitar Pro as a first-class data source,
especially for techniques and guitar specifics.

### S4
Phrase boundary detection stage. Detect phrase boundaries automatically with
explainable heuristics and manual override.

### S5
Corpus and schema stage. Build a micro-corpus and an annotation schema for
phrases, techniques, tags, and quality.

### S6
Rule-based generator v0 stage. First controlled, non-neural phrase generator.

### S7
Graph layer stage. Add a graph of phrases, motifs, rhythm cells, transitions.

### S8
Preview app stage. Standalone app to view, listen, compare, and hand-annotate.

### S9
Feedback layer stage. Human-in-the-loop: like/dislike, favorites,
preferences, reranking.

### S10
CLAP MVP stage. MIDI-oriented CLAP plugin, once core is stable enough.

### S11
Region regeneration stage. Regenerate selected ranges while preserving
anchors / frozen regions.

### S12
Neural assistance stage. Neural layer for continuation / infilling /
variation, only after corpus and rule-based baseline exist.

## 1. Architecture and data model

### canonical model
Single internal model into which MIDI, Guitar Pro, MusicXML, and other formats
are imported. Not a copy of MIDI or Guitar Pro — a normalized model of its own.

### canonical score model
The target score-level model: `Score -> MasterBar -> Track -> Voice ->
EventGroup -> AtomEvent`. Needed for tempo map, polyphony, chords, Guitar Pro
effects, and generation.

### Score
Top-level musical document: master timeline, tracks, metadata, source info,
tuning, global parameters.

### MasterBar
Score-level bar. Holds bar parameters shared by all tracks: time signature,
tempo changes, repeats, markers, bar index, absolute tick range. Prevents
re-deriving tempo per track.

### Track
An instrument part or logical layer (clean guitar, rhythm guitar, lead guitar,
bass).

### Voice
An independent event stream inside a track. Needed for polyphony, overlapping
notes, ringing notes, chords, multiple rhythmic lines.

### EventGroup
A group of events bound by time or musical role: chord, arpeggio group,
tuplet, grace cluster, sequential fragment. Fixes the "Event::Note/Rest is too
linear" problem.

### AtomEvent
The minimal event in the canonical model: note, rest, tie, control/expression
event, or a service unit. The atom that groups are built from.

### Note
A sounding note. Currently has `pitch`, `duration`, `velocity`,
`articulation`. The target model must also carry source metadata: string,
fret, technique evidence, voice id.

### Rest
A musically significant silence of a given duration. Distinct from "no event".

### Event
Current enum in `griff-core` holding `Note` and `Rest`. In the target
architecture it becomes a compatibility layer or part of a richer model.

### Bar
Current single-bar event container. In the target model it links to
`MasterBar` or becomes a view/projection.

### Phrase
A musical phrase: a sequence of events/bars perceived as a complete unit. The
base unit of analysis, generation, and corpus.

### PhraseChunk
A fragment extracted from a source and stored in the corpus. Has boundaries,
tags, features, source reference, quality flags.

### Motif
A short recognizable musical idea: a rhythmic figure, an interval shape, or a
repeated phrasal cell. The generator must vary motifs, not shuffle notes.

### RhythmCell
A small rhythmic unit: an onset/duration/rest pattern. Used in the graph layer
and rule-based generator.

### PitchMaterial
A set of pitch constraints: scale, mode, pitch set, anchor notes, allowed
intervals, register range.

### Timeline
The time axis of a piece, expressed through ticks, tempo map, and master bars.

### Master timeline
The global time structure of a score. Single source of truth for tempo,
meter, bar positions, and transport-level markers.

### SourceMeta
Metadata from the source format: Guitar Pro effects, string/fret, MIDI CC
evidence, original track name, importer warnings. Must not be dropped silently.

### ImportWarning
Importer warning: data read partially, approximately, or lost.

### LossReport
A report of losses during import/export. Especially important for
MIDI ↔ Guitar Pro, which store different semantics.

### Compatibility layer
A layer letting the old `Phrase/Bar/Event` API live during the transition to
the canonical score model.

### Projection
A representation of the rich model in a simpler form (e.g. polyphonic score →
monophonic phrase view).

### View
A read-only representation for a specific use case: monophonic phrase view,
track summary, CLI summary, debug dump.

## 2. Time, rhythm, transport

### Tick
An abstract unit of musical time. In MIDI it depends on PPQN. Typed as
`Ticks`.

### Ticks
Rust newtype around a tick count, so musical time is not confused with bytes,
indices, or milliseconds.

### PPQN
Pulses Per Quarter Note. Ticks per quarter note (e.g. 480 PPQN = 480 ticks per
quarter).

### Metrical timing
MIDI timing based on ticks per beat/quarter. The path `griff` supports today.

### SMPTE timing
Frame-based MIDI timing. Not supported yet; would need
`TransportTimebase::{Ppqn, Timecode}` or an absolute-time conversion layer.

### Timecode
Time as hours/minutes/seconds/frames. Less convenient than PPQN for a
generator, but appears in MIDI/film/video workflows.

### Tempo
Music speed, usually BPM. Currently `Tempo(pub f64)`.

### BPM
Beats Per Minute (120 BPM = 120 beats per minute).

### Microseconds per beat
MIDI tempo representation: microseconds per beat (500000 µs/beat = 120 BPM).

### Tempo map
A sequence of tempo changes along the timeline. Lives in the master timeline,
not scattered across tracks.

### Time signature
Bar meter (4/4, 7/8, 5/4). Typed as `TimeSignature`.

### Meter
Same meaning as time signature, used more as a musical category.

### Bar duration
Bar length in ticks; depends on PPQN and time signature.

### Half-open range
Range `[start, end)`: start included, end excluded. Important for `TickRange`,
slicing, and boundary logic.

### TickRange
A tick range used for slicing, selection, region regeneration, boundary
detection.

### TimedEvent
An event with absolute start and source indexes; used for analysis, slicing,
regeneration.

### Quantization
Snapping events to a time grid. Useful for MIDI import but dangerous if it
destroys human micro-timing.

### Swing
Rhythmic offset of beats producing groove. Less central for swancore than
syncopation but relevant for groove-aware generation.

### Syncopation
An accent or event on a weak/unexpected beat. A key swancore/prog feel marker.

### Cadence
A sense of phrase completion: harmonic, rhythmic, or melodic.

### Rhythmic reset
A new rhythmic pattern starting after dense/syncopated motion. A useful phrase
boundary signal.

### Density
Event density over time: notes/attacks/changes per region.

### Onset
The start moment of a sounding event. Onset patterns matter for rhythm
analysis.

## 3. MIDI

### MIDI
Musical Instrument Digital Interface. An event format/protocol (note on/off,
velocity, CC, pitch bend, program change). MIDI does not encode "hammer-on" as
full guitar semantics.

### SMF
Standard MIDI File — the `.mid` file format.

### midly
Rust crate for reading/writing MIDI. Currently used by `griff`.

### NoteOn
MIDI note-start event. Velocity > 0 usually means note start.

### NoteOff
MIDI note-end event. `NoteOn velocity=0` is a common NoteOff equivalent.

### Velocity
Note intensity, 0–127. A dedicated type in `griff`.

### Pitch
MIDI pitch number, 0–127. 60 = middle C.

### Channel
MIDI channel 0–15. A track currently gets the dominant channel by majority of
note events.

### TrackEvent
A `midly` MIDI track event: delta time + payload.

### Meta event
A service MIDI event: tempo, time signature, track name, end of track, etc.

### Track name
MIDI meta event with the track name; imported as an optional name.

### EndOfTrack
MIDI meta event ending a track. Required for correct export.

### Control Change / CC
MIDI controller events (pedals, modulation, expression). Not always
unambiguous musically.

### Pitch Bend
MIDI pitch-bend event. Evidence for bend/vibrato, not equal to a symbolic
Guitar Pro technique without context.

### Channel Pressure
Channel-level aftertouch. Possible expression evidence.

### Poly Pressure
Per-note aftertouch. Useful for expression/MPE.

### MPE
MIDI Polyphonic Expression — per-note pitch bend/pressure/timbre. Not MVP, but
potentially useful for a future expression layer.

### MIDI import
Conversion of `.mid` into the canonical score model, preserving as much
evidence as possible.

### MIDI export
Conversion of the canonical score model into `.mid`, built from the master
timeline and tracks — not from a random first track.

### MIDI roundtrip
Import MIDI → export MIDI → re-import → compare normalized result. An
important integration/golden test.

### MIDI evidence
Low-level MIDI events from which articulations or expression can be inferred,
but which cannot be claimed as symbolic technique.

## 4. Guitar Pro, tablature, notation formats

### Guitar Pro
Tablature formats `.gp3`, `.gp4`, `.gp5`, `.gpx`, `.gp`. Important because they
carry guitar semantics: strings, frets, techniques, effects.

### GP3 / GP4 / GP5
Binary Guitar Pro 3/4/5 formats. MVP import candidates.

### GPX
Guitar Pro 6 format — a proprietary container with GPIF/XML-like content.
Harder than GP3–GP5.

### GP7 / GP8 `.gp`
Newer archive/zip-like Guitar Pro formats with `score.gpif` plus extra files.
Mark as experimental.

### GPIF
XML-like internal representation used by Guitar Pro 6+; can be easier to
import than binary parsing.

### PyGuitarPro
Python library for GP3/GP4/GP5; usable as a sidecar adapter if pure Rust is
immature.

### `guitarpro` crate
Rust crate for Guitar Pro; candidate for a Rust-native GP import pipeline.

### alphaTab
Tablature render/parse ecosystem including Guitar Pro; a reference model and
possible adapter inspiration.

### MusicXML
Open notation interchange format. Possible fallback/interchange, but does not
replace Guitar Pro for guitar techniques.

### Tab / tablature
Guitar notation by strings and frets. Matters for playability, string/fret
movement, position shifts.

### String
A guitar string. Stored separately from pitch (one pitch is playable on
several strings).

### Fret
A guitar fret. With string, defines the physical position of a note.

### Tuning
Guitar tuning (standard, drop D, alternate). Required to interpret string/fret
correctly.

### Guitar technique
A playing technique: hammer-on, pull-off, slide, bend, vibrato, palm mute,
tapping, harmonic, etc.

### TechniqueSpan
A time or note range a technique applies to (techniques are not always
single-note).

### Source-of-truth articulation
An articulation from a format that stores it explicitly (e.g. Guitar Pro), as
opposed to inferred from MIDI.

### Inferred articulation
An articulation guessed by heuristics. Must carry confidence/evidence, not
masquerade as fact.

### Lossless import
Import without losing important semantics. Possible for much of Guitar Pro if
the model is rich enough.

### Lossy export
Export losing information (e.g. Guitar Pro techniques → MIDI is almost always
lossy).

## 5. Articulations and guitar techniques

### Articulation
A technique/manner of playing a note. The current enum already holds some
guitar articulations.

### Slide
Sliding between notes/positions. In MIDI only indirectly via pitch bend; in
Guitar Pro an explicit effect.

### Bend
String bend. A symbolic Guitar Pro technique, or MIDI pitch-bend evidence.

### Legato
Connected playing; on guitar often hammer-on/pull-off.

### Hammer-on
Next note sounded by a fretting-hand finger strike, no new pick attack.

### Pull-off
Next note produced by pulling a finger off the string.

### Palm mute
String muting with the palm edge. Important in heavier/prog contexts; usually
not stored explicitly in plain MIDI.

### Vibrato
Pitch oscillation. MIDI: pitch bend/modulation evidence; Guitar Pro: explicit
technique.

### Natural harmonic
A natural harmonic. Explicit in Guitar Pro; usually absent in MIDI.

### Pinch harmonic
Pick/artificial harmonic. Almost impossible to recover reliably from MIDI
without the source.

### Tapping
A note sounded by tapping a fretboard. Can matter for swancore. Not stored
directly in MIDI.

### Tremolo picking
Fast repeated picking. Detectable via dense repeated onsets.

### Let ring
Notes ringing over following events. Needs polyphony/voices.

### Ghost note
A quiet/muted note. May be velocity/technique-driven; interpret carefully.

### Accent
An accented note. May be high velocity in MIDI, but velocity ≠ musical accent.

### Grace note
A short auxiliary note before a main note. Needs its own model or an
EventGroup.

### Tuplet
A rhythmic subdivision off the regular grid (triplets, quintuplets, …).

### Strum
A chord stroke across strings with micro-offsets. Needs an EventGroup, not a
single chord timestamp.

### Arpeggio
A broken chord: chord notes sounded sequentially.

## 6. Phrase boundary detection

### Phrase boundary
A boundary between phrases. May coincide with a pause, cadence, bar boundary,
or a change in rhythmic/melodic behavior.

### Boundary detector
An algorithm proposing likely phrase boundaries.

### Boundary score
A numeric boundary likelihood, e.g. `pause + cadence + rhythm_reset +
motif_boundary + register_jump + density_change`.

### BoundaryReason
An explanation of why a boundary was placed (pause, rhythmic reset, register
jump, harmonic resolution, …).

### Manual override
A user correction of boundaries. Mandatory for the corpus; fully automatic
labelling will confidently err.

### Pause score
Boundary signal from a long pause or rest.

### Cadence score
Completion signal from harmonic/melodic/rhythmic resolution.

### Register jump score
Boundary signal from a sharp register jump.

### Density change score
Boundary signal from a sharp change in event density.

### Motif boundary score
Boundary signal from a motif ending/repeat/change.

### Harmonic resolution
A sense of tension resolving to a stable point; may be heuristic when harmonic
context is poor.

### LBDM
Local Boundary Detection Model. Segmentation by local parameter changes.
Inspiration for an explainable heuristic detector.

### IDyOM
Information Dynamics of Music — an expectation/surprise model. Not MVP;
theoretical reference for predictive boundary cues.

## 7. Features and analysis

### Feature extraction
Extraction of numeric/categorical phrase features: pitch range, density,
intervals, syncopation, articulation ratio, etc.

### PhraseFeatures
Current `griff` struct computing basic phrase features. Must be extended.

### Pitch range
Min and max note pitch in a phrase.

### Velocity range
Min and max velocity.

### Note count
Number of notes.

### Rest count
Number of rests.

### Articulated note count
Number of notes carrying articulation.

### Rest ratio
Fraction of rests in a phrase. Useful for boundary detection and generator
style.

### Interval
Distance between two pitches (melodic or harmonic).

### Interval histogram
Distribution of intervals. Useful for similarity and style constraints.

### Pitch contour
Shape of pitch motion: up, down, wave, leaps, repeats.

### Register
Pitch region (low/mid/high). For guitar, related to fretboard position.

### Register movement
Motion between registers. Often significant in swancore.

### Onset pattern
The rhythmic pattern of note onsets.

### Duration pattern
The pattern of durations.

### Syncopation score
A measure of syncopation strength.

### Technique ratio
Fraction of notes/spans with a given technique.

### Playability
How physically playable a phrase is on guitar.

### Fretboard movement
Motion across the fretboard: position shifts, string changes, fret distance.
A key swancore-specific feature.

### Similarity
Similarity of phrases/motifs; may consider rhythm, contour, intervals,
techniques, register, tags.

### Distance metric
A distance function between objects. Music rarely needs only one.

## 8. Generation

### Generator
A component that creates new phrases, variations, or regions.

### Rule-based generator
A generator driven by rules and heuristics. Must precede the neural layer.

### Deterministic generator
Same seed/input → same result. Required for tests.

### Seed
The RNG seed; needed for reproducibility.

### Candidate
One generated phrase variant.

### Candidate set
A set of variants for the user or reranker to choose from.

### Reranking
Re-sorting candidates by quality score, user preferences, similarity,
constraints.

### Variation
A modified version of a source phrase or motif.

### Continuation
A continuation of a given phrase.

### Infilling
Filling a missing/selected span between two contexts.

### Regeneration
Re-generating a selected span, usually preserving some context.

### Region regeneration
Regenerating a specific `TickRange` or selection.

### Frozen region
A span that must not change during regeneration.

### Anchor
An event/note/boundary that must be preserved (e.g. first and last note of a
region).

### Constraint
A generation constraint: scale, range, rhythm density, max fret distance,
preserve contour, allowed techniques.

### Rhythm-copy
Strategy: take rhythm from a source, replace pitch material.

### Pitch-substitute
Strategy: keep rhythm, replace pitches by rules.

### Motif variation
Strategy: keep a recognizable motif shape, change details.

### Constrained random walk
A random walk over pitch/fretboard space with constraints, to avoid
meaningless output.

### Cadence-aware ending
Generating a phrase ending with a sense of completion.

### Playability filter
A filter discarding physically awkward or impossible phrases.

### Quality score
The overall candidate score: similarity, novelty, playability, density, user
preference, style fit.

### Novelty
How different a candidate is from the source. Balance: too low = copy, too
high = noise.

### Style fit
How well a candidate matches swancore-first constraints.

## 9. Graph layer

### Graph layer
A graph representation of phrases, motifs, rhythm cells, and transitions.

### Node
A graph node: phrase, motif, rhythm cell, or technique pattern.

### Edge
A relation between nodes: similarity, transition, harmonic compatibility, or
user preference.

### Edge weight
The relation weight, used in traversal/generation/reranking.

### Phrase graph
A graph whose nodes are phrases or phrase chunks.

### Motif graph
A graph of motifs and their variations.

### Transition graph
A graph of allowed phrase transitions.

### Graph traversal
Walking the graph to pick a continuation, variation, or candidate chain.

### Recombination
Assembling a new phrase from parts of existing phrases/motifs.

### Knowledge graph
The documentation/semantic graph of concepts, files, decisions, stages, and
dependencies. Not the runtime phrase graph.

## 10. Human-in-the-loop

### Human-in-the-loop
The user rates candidates, corrects boundaries, picks good fragments, and
thereby steers the system.

### Like/dislike
The minimal feedback signal. Used for reranking and the preference profile.

### Favorite
A stronger signal than a like: save a candidate as a good example.

### Preference profile
A model of user preferences: which features, tags, styles are liked more.

### Feedback weight
A feature weight adjusted by user ratings.

### Active curation
The user does not just press "generate" but builds a micro-corpus and marks
the best results.

### Reviewer decision
A human decision on a chunk/candidate: accepted, rejected, needs edit,
favorite, boundary corrected.

## 11. UI, CLI, plugin

### CLI
Command Line Interface. `griff` already has `import`, `inspect`, `export`. The
CLI stays the first debugging surface.

### `griff import`
Reads a file and prints a short summary.

### `griff inspect`
Detailed bar-by-bar view.

### `griff export`
Roundtrip/export.

### Preview app
A standalone app for viewing phrases, candidates, boundaries, graph, and
feedback. Comes before CLAP.

### egui
A Rust immediate-mode GUI library. Candidate for the preview app.

### eframe
A framework around egui for desktop apps.

### CLAP
CLever Audio Plugin API. The target plugin format for `griff` at S10.

### NIH-plug
A Rust framework for VST3/CLAP plugins. Candidate for the CLAP MVP.

### MIDI-out plugin
A plugin that outputs MIDI events into the DAW rather than synthesizing audio.

### DAW
Digital Audio Workstation (Ableton, Reaper, Logic, Bitwig, …).

### Host
The DAW or app loading the plugin.

### Host transport
Tempo, play/stop, timeline position, bar/beat info from the DAW host.

### Drag-and-drop MIDI
Dragging a generated MIDI fragment from the plugin/preview into the DAW.

### Parameter
A configurable plugin parameter: density, variation amount, style mode, seed,
phrase length, etc.

## 12. Rust and workspace

### Rust workspace
A group of crates in one repo. In `griff`: `core`, `cli`, `plugin`.

### crate
A Rust package/library/binary unit.

### `griff-core`
Core crate: musical model, import/export, slicing, features, generation.

### `griff-cli`
CLI crate: user commands and a test debugging surface.

### `griff-plugin`
Plugin crate: empty until S10.

### `Cargo.toml`
Rust crate/workspace manifest: dependencies, package metadata, profiles,
lints.

### `rust-toolchain.toml`
Pins the toolchain and components: stable, rustfmt, clippy.

### MSRV
Minimum Supported Rust Version. `griff` targets Rust 1.74; do not use newer
syntax/features without a decision.

### clippy
The Rust linter, configured strictly in `griff`.

### rustfmt
The Rust code formatter.

### unsafe_code = forbid
Unsafe code is forbidden. A key architectural rule.

### lint policy
The code-quality rule set. `griff` has a strict workspace-wide policy.

### `thiserror`
Rust crate for ergonomic error types.

### `clap`
Rust crate for CLI arguments.

### `Result`
Rust success/error type. All fallible operations return errors, not panic.

### `Option`
Rust optional-value type (e.g. optional articulation or track name).

### newtype
A wrapper around a primitive: `Pitch(u8)`, `Ticks(u32)`. Provides type safety.

## 13. TDD and testing

### TDD
Test-Driven Development: red → green → refactor. Behavior tests first, code
second.

### Red
A test fails because the feature is not implemented or a bug is confirmed.

### Green
A minimal implementation makes the test pass.

### Refactor
Improving structure without changing behavior.

### Characterization test
A test that pins existing/legacy behavior. Required before refactoring.

### Golden test
A test against a reference file/output.

### Snapshot test
A test comparing output to a stored snapshot. Good for CLI output, debug
dumps, normalized score dumps.

### Approval test
Like snapshot/golden: a human approves the reference result.

### Unit test
A test of a small function/module.

### Integration test
A test of several components together: import → model → export, CLI →
output, GP file → score.

### Property-based test
Testing properties over generated inputs (e.g. `duration(a+b) == duration(a) +
duration(b)`, roundtrip invariants).

### proptest
Rust crate for property-based testing.

### insta
Rust crate for snapshot testing.

### cargo-nextest
A fast test runner for a Rust workspace.

### cargo-mutants
Mutation testing for Rust: checks whether tests catch injected bugs.

### Mutation testing
Test-quality check via small code mutations. If tests do not fail, they are
weak.

### cargo-llvm-cov
Code coverage tool for Rust. Coverage ≠ test quality.

### Test fixture
A file or object used by a test (minimal MIDI fixture, GP5 fixture, score JSON
dump).

### Corpus fixture
A minimal corpus slice for tests, not the full corpus.

### Smoke test
A fast "the system starts and does not crash" test.

### Regression test
A test for a previously found bug.

### Roundtrip test
A test of import → export → import, or format A → canonical → format A.

### Invariant
A property that must always hold (e.g. `TickRange.start <= TickRange.end`).

### Test oracle
The source of truth for the expected result: snapshot, manual labelling,
fixture, formal property.

### CI gate
A condition required to accept a PR: tests green, clippy green, format green,
snapshots reviewed.

## 14. Documentation and agent development

### AGENTS.md
Instruction file for AI agents in the repo. Short, concrete, links to docs.

### CLAUDE.md
Agent instruction file for Claude-style tooling. May alias/bridge to
`AGENTS.md`.

### SPEC.md
The main project spec: what `griff` does, does not do, and which architectural
rules are mandatory.

### ADR
Architecture Decision Record: a short doc fixing an important decision —
context, decision, consequences.

### Stage doc
A per-stage doc: goal, tasks, tests, acceptance criteria, risks.

### Architecture doc
A doc about the model, dataflow, boundaries, modules.

### Corpus schema doc
A doc describing the corpus annotation format.

### Agent task
An LLM-agent task with context, files to touch, constraints, tests to run,
definition of done.

### Definition of Done
Task completion criteria. For `griff`: tests green, docs updated, loss report
considered, no hidden behavior changes.

### Prompt rot
When long instructions go stale and start to harm. Cured by short docs, ADRs,
stage files.

### Context window
The limited text an LLM can hold in a request. Hence a glossary, not an
endless scroll.

### Knowledge base
The set of docs, glossary, ADRs, specs, and stage files an agent relies on.

### Knowledge graph documentation
Documentation linking terms, decisions, stages, and modules. Distinct from the
runtime phrase graph.

## 15. Quality, risks, constraints

### Technical debt
Debt in code/architecture that speeds up now and slows down later.

### Architecture risk
Risk that the chosen model will not survive future use cases (e.g. linear
`Event::Note/Rest` vs polyphony/chords).

### Format risk
Risk that an external format stores data differently or richer than the
internal model.

### Semantic loss
Loss of meaning during conversion (e.g. Guitar Pro palm mute → MIDI velocity
is not full preservation).

### Backward compatibility
Preserving old behavior/API while changing the architecture.

### Migration path
A plan to move from old to new model without a total rewrite.

### Feature flag
A switch for experimental functionality (e.g. GP7/GP8 or SMPTE support).

### Experimental support
A feature exists, but API/behavior stability is not promised.

### Stable support
A feature covered by tests, docs, fixtures, and acceptance criteria.

### Fail-fast
Fail quickly and explicitly on unsupported input instead of silently
corrupting data.

### Graceful degradation
Partial support with a warning/loss report when full support is impossible.

### Observability
The ability to understand what happened: debug dumps, summaries, import
warnings, loss reports.

### Debug dump
A normalized text/JSON dump of the internal model for tests and analysis.

### Normalized dump
A dump without unstable details (absolute paths, random ids, map iteration
order). Needed for snapshot tests.

## 16. Quick map: term → where it lives

- `Score`, `MasterBar`, `Track`, `Voice`, `EventGroup` → `griff-core`, future
  canonical model.
- `Pitch`, `Ticks`, `Velocity`, `Tempo`, `TimeSignature` → scalar musical
  types.
- `MIDI`, `SMF`, `PPQN`, `Tempo map` → format adapter + transport.
- `Guitar Pro`, `GP3/4/5`, `GPX`, `GPIF` → format adapter + source metadata.
- `Phrase`, `Motif`, `RhythmCell` → analysis, corpus, generation.
- `Boundary score`, `BoundaryReason` → phrase boundary detection.
- `Feature extraction`, `Similarity`, `Playability` →
  analysis/generator/reranking.
- `Candidate`, `Variation`, `Regeneration`, `Frozen region`, `Anchor` →
  generation workflows.
- `Graph layer`, `Phrase graph`, `Motif graph` → S7+ recombination.
- `Like/dislike`, `Preference profile` → S9 feedback layer.
- `CLAP`, `NIH-plug`, `Host transport` → S10 plugin.
- `TDD`, `Snapshot`, `Property test`, `Mutation testing` → development
  process.
- `ADR`, `SPEC.md`, `AGENTS.md` → documentation/process.

## 17. Rules for the LLM agent

1. Do not use `MIDI` as the internal model. MIDI is an adapter boundary.
2. Do not treat `Event::Note/Rest` as the final guitar model.
3. Do not promise recovery of Guitar Pro articulations from plain MIDI without
   confidence/evidence.
4. Do not add CLAP before the core model and format adapters are stable.
5. Do not add neural generation before corpus, features, and a rule-based
   baseline.
6. Every new format adapter must produce a loss report.
7. Every refactor must start with characterization tests.
8. Every new generation must be deterministic under a fixed seed.
9. Every stage task must have tests, docs, and acceptance criteria.
10. If a term is unclear, extend this glossary rather than spawning synonyms in
    code.

## 18. Preferred naming

Use these as defaults until an ADR decides otherwise:

`Score`, `MasterBar`, `Track`, `Voice`, `EventGroup`, `AtomEvent`,
`TechniqueSpan`, `SourceMeta`, `ImportWarning`, `LossReport`, `PhraseBoundary`,
`BoundaryReason`, `BoundaryScore`, `PhraseChunk`, `FeatureVector`,
`GenerationRequest`, `GenerationCandidate`, `GenerationSeed`,
`RegenerationRegion`, `FrozenRegion`, `AnchorPoint`, `PreferenceProfile`.

## 19. Terms to avoid or use carefully

### AI magic
Not a plan. A neural net does not replace the data model, corpus, tests, and
generator.

### Works fine
Banned as self-soothing. If it "works fine", show tests, fixtures, and
acceptance criteria.

### Just a quick hack
A danger sign. Spikes/prototypes are allowed but must be marked as spikes and
not dragged into production core without cleanup.

### MIDI articulation
Dangerous term. Prefer `MIDI expression evidence` or `inferred articulation
from MIDI evidence`.

### Guitar Pro support
Always qualify: GP3/4/5, GPX/GP6, GP7/GP8, read-only or export, stable or
experimental.

### Polyphony support
Always qualify: simultaneous notes, voices, chords, overlapping notes,
let-ring, strum timing.

## 20. Definition of Done for glossary changes

A `glossary.md` change is done if:

- the new term has a short definition;
- it states how the term is used in `griff`;
- if ambiguous, a warning is added;
- if it replaces an old name, the preferred name is stated;
- agent/stage docs do not use contradicting definitions.
