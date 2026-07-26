# 2026-07 — Generator Reachability Lab, Phase 0 metric & expressivity inventory

The Phase 0 deliverable of the Generator Reachability Lab proposal
([`../proposals/generator-reachability-lab.md`](../proposals/generator-reachability-lab.md)
§5): an audit of the existing metric layer, a per-axis triage, a holdout
feasibility check, the target-eligibility contract, a cost-benchmark protocol,
and a roadmap-placement recommendation — so Phase 1 formalizes reality instead
of inventing a parallel one.

Scope: docs-only research artifact; binds nothing. Phase 0 adds **no production
code** (proposal §5). Every code claim below was verified against the tree at
the time of writing and is cited by `file:line`; a reviewer should re-check
them. The proposal itself binds nothing until accepted
([`../proposals/README.md`](../proposals/README.md)); this audit is evidence
for that discussion, not a decision.

## 0. Headline findings

- **The metric logic Phase 1 wants mostly exists; the debt is *privacy*, not
  absence.** The reusable primitives (`ratio_similarity`, `scalar_similarity`,
  `set_jaccard`, `longest_common_run`, `top_line`, `transitions`,
  `displaced_beats`) are private helpers inside their modules. Phase 1's work is
  disciplined *extraction*, not green-field measurement.
- **"`ExactVoice`" does not exist.** The proposal's §5 item 4 names a monophonic
  `ExactVoice` projection as the eligibility unit, but there is no such type in
  the tree (verified: no `ExactVoice` anywhere outside `target/`). The
  monophonic projection is realized today by *private* `top_line` helpers. The
  eligibility contract must therefore define its own projection type/primitive;
  it cannot reference an existing one. (Recorded here so Phase 1 does not chase a
  phantom.)
- **Three different "the melodic line" extractors already exist.** Adding a
  fourth is the standing hazard the proposal warns about (§3, "No third
  slightly-different definition of the melodic line").
- **Holdout is feasible** and enforceable *before* corpus material is compiled,
  **conditional on fail-closed handling of records that lack a content
  identity**: `sha256` is `Option` (pre-v9), and the loader falls back to the
  basename ("a filename is not an identity") — so holdout must reject or migrate
  `sha256`-less records rather than trust a filename. Source identity is also
  **structurally dropped** the moment `CorpusMaterial` is built — a blocker for
  *post-hoc leak attribution*, not for holdout.
- **No benchmark infrastructure exists** (no `criterion`, no `benches/`). The
  cost benchmark is specified below with a zero-production-code harness; the
  numbers are `TBD (unmeasured)` because the Rust toolchain was unavailable in
  the authoring environment — the harness is given so the measurement is a
  mechanical follow-up. **Phase 0 is therefore not yet complete as an evidence
  gate**: item 5 (the cost number that gates any future directed-search
  discussion) does not exist until those trials are run and recorded. Items
  1–4 and 6 are answered; item 5 is drafted-but-unmeasured.

## 1. Inventory — the existing metric layer

Enforcement of the proposal's "reuse the existing scoring architecture"
decision (§2) requires knowing exactly what that architecture is. Per module:
what it produces, whether it reads **note content** or persisted **metadata**,
its versioned policy, and the private helpers Phase 1 would reuse.

### `core/src/scoring.rs` — the shared vocabulary (ADR-0017)

Domain-neutral; reads nothing (pure algebra over caller-supplied facts).
`Axis { label: &'static str, value: f64 }` (`scoring.rs:33`), `Axes`
(`:45`, with `get`/`iter`/`len`), `WeightPolicy` (`:96`; `weights` field
private; `uniform(id, version, labels)` `:120`; `weight()` returns `0.0` for an
unknown axis `:136`), `Rationale`/`RationaleEntry` (`:147`/`:160`),
`Provenance { policy_id, policy_version, seed: Option<u64> }` (`:176`),
`Scored<T>` (`:192`; `aggregate()` is derived `Σ contribution`, never stored,
`:237`), and `rank_indices` (`:253`; descending aggregate, ties by ascending
index). This is the vocabulary the Lab's comparison facts must be expressed in
(proposal §3: "Comparison facts are plain `Axes`").

### `core/src/similarity.rs` — chunk similarity v3 (reads METADATA only)

The one pure-metadata consumer: its module doc states "No note content is
read" (`similarity.rs:1-9`); it reads persisted `ChunkMeta`
(structure/gesture/complexity/tags). `SIMILARITY_AXIS_LABELS: [&str; …]`
(`:88`), `similarity_axes(a, b: &ChunkMeta) -> Option<Axes>` (`:123`, symmetric,
`None` if either side unmeasured), `similarity_weights_v3()` =
`WeightPolicy::uniform("similarity", 3, …)` (`:202`), `find_similar_chunks`
(`:214`). Private helpers Phase 1 would reuse: `scalar_similarity` (`1 − |Δ|`,
`:264`), `ratio_similarity` (min/max, `:272`), `tag_similarity` (Jaccard,
`:285`). **Policy: `similarity` v3.**

### `core/src/novelty.rs` — novelty guard v1 (reads NOTE CONTENT)

Compares **transition sequences** `(pitch interval i16, normalised IOI u32)` on
a common grid. `NOVELTY_AXIS_LABELS` (`:42`), `NoveltyReport` (`:66`),
`measure_novelty(score, track_index, references)` (`:91`), `novelty_axes`
(`:164`), `novelty_weights_v1()` = `WeightPolicy::uniform("novelty", 1, …)`
(`:186`), `PHRASE_DUPLICATE_SHARE: f64 = 0.8` (`:195`), `flag_phrase_duplicates`
(`:221`). Key private constants `NGRAM_TRANSITIONS = 4` (`:45`) and
`IOI_GRID_PER_QUARTER = 480` (`:49`); private helpers `top_line` →
`Vec<(u32, u8)>` (`:287`), `transitions` (`:320`), `longest_common_run`
(O(n·m·len), `:342`). **Policy: `novelty` v1.**

### `core/src/syncopation.rs` — displacement → boolean tag (reads NOTE CONTENT)

Public surface is only `derive_syncopated(score, track_index) -> Vec<SwancoreTag>`
(`:33`), which returns `[Syncopated]` at threshold `SYNCOPATION_THRESHOLD = 0.25`
(`:25`). **The raw fact is private**: `displaced_beats(bars, onsets) -> (u32, u32)`
(`:77`) computes `(displaced, total)` and is immediately collapsed to the tag
(`:48`); per-beat positions are discarded. There is **no `DisplacementProfile`
type**. This is precisely the "extract the raw fact, keep the tag as a policy
over it" case the proposal names (§3, `syncopation.rs` row).

### `core/src/feature.rs` — voice features (reads NOTE CONTENT; silence NOT stored)

`PitchRange { lowest, highest }` (`:11`), `VelocityRange` (`:29`),
`VoiceFeatures { event_count, note_count, rest_count, articulated_note_count,
total_duration, pitch_range, velocity_range }` (`:62`), `voice_features(&Voice)`
(`:81`). **Silence is derived, never stored** (load-bearing doc `:53-60`):
`total_duration` is summed atom duration (not bar span), `rest_count` counts
only explicit `AtomEvent::Rest`; silence-aware metrics "should be derived from
`MasterBar` context". So a silence-occupancy axis is a **new projection**, not a
reuse.

### `core/src/rerank.rs` — production candidate set + 6 axes (reads NOTE CONTENT)

`RERANK_AXIS_LABELS: [&str; 6]` = `internal_continuity, ending_stability,
final_lengthening, gap_fill` (from `closure.rs`) + `quote_novelty, ngram_novelty`
(from `novelty.rs`) (`:44`). `rerank_weights_v1()` =
`WeightPolicy::uniform("generation_rerank", 1, …)` (`:60`).
`generate_candidate_set(&SetRequest)` (`:137`) iterates
`SET_STRATEGIES: [GenerationStrategy; 5]` (`:65`) × `variants_per_strategy`,
`rerank_candidates(candidates, material, references, policy)` (`:200`).
**The Lab measures; it must not touch this** (proposal §3, `rerank.rs` row: "Do
not touch"). **Policy: `generation_rerank` v1.**

### `core/src/structure.rs` + `core/src/gesture.rs` + `core/src/closure.rs`

- `StructureMetrics` (`structure.rs:84`): `bar_count`,
  `detected_pattern_period_bars`/`…_ticks`, `detected_subbar_period_ticks`,
  `repeatability_score`, `variation_score`, `loopability_score`,
  `structural_complexity`; `measure_structure` (`:147`). `ComplexityProfile`
  (`:458`): `rhythmic, pitch, technical, harmonic, playability, structural`;
  `measure_complexity` (`:484`). Private `set_jaccard` (`:352`). Phase-2 axes:
  `structure_weights_v1()` = `WeightPolicy::uniform("structure", 1, …)` (`:988`).
  **Policy: `structure` v1.**
- `GestureStats` (`gesture.rs:58`): `mean_burst_notes, max_burst_notes,
  mean_rest_quarters, rest_on_grid_share, modal_landing_share,
  mean_final_lengthening`, …; `measure_gesture` (`:99`). Carries its **own**
  private `top_line -> Vec<LineNote>` where `LineNote = (u32, u32, u8)` =
  `(onset, duration, pitch)` (`:169`).
- `closure.rs`: `CLOSURE_AXIS_LABELS: [&str; 4]` (`:39`), `closure_weights_v1()`
  (`:110`). **Policy: `closure` v1.**

**Versioned policies present (all `WeightPolicy::uniform`, all untuned
baselines):** `similarity` v3, `novelty` v1, `generation_rerank` v1,
`structure` v1, `closure` v1.

**Line-extraction fragmentation (hazard).** Three private "melodic line"
extractors, on different shapes, already exist:
`novelty::top_line` → `(onset, pitch)` (`novelty.rs:287`);
`gesture::top_line` → `(onset, duration, pitch)` (`gesture.rs:169`);
`structure::line_pitches` → `pitch` only (`structure.rs:535`). Phase 1 must
extract **one** shared primitive before adding contour/IOI/pitch axes, not a
fourth (proposal §3).

## 2. Per-axis triage

Every proposed Phase-1 fact, its best existing source, and a decision under the
closed vocabulary **reuse / extend / new**. "extend" means an existing private
helper is lifted to a shared primitive; "new" means no equivalent logic exists.

| Proposed fact | Best existing source | Decision | Reason (grounded) |
| --- | --- | --- | --- |
| Exact onset set | `syncopation::track_onsets` (`syncopation.rs:59`, private `HashSet<u32>`); onsets also in `novelty::top_line` (`novelty.rs:287`) | **extend** | Consistent with the `extend` definition: `track_onsets` already computes the exact onset set privately — lift the shared line/onset primitive (canonicalising the ordering the set drops), do not author a new projection. |
| Exact pitch on paired onsets | `novelty::top_line` (`novelty.rs:287`); also gesture / structure variants | **extend** | Reuse *one* line extractor by lifting a shared `(onset, pitch)` primitive — never add a fourth. |
| Interval contour | interval half of `novelty::transitions` (`novelty.rs:320,330`) | **extend** | Transposition-invariant `i16` intervals already computed; extract the interval component. |
| IOI sequence | `novelty::transitions` + `IOI_GRID_PER_QUARTER = 480` (`novelty.rs:49,320`) | **extend** | Resolution-invariant normalised-IOI already implemented in a private helper. |
| Silence occupancy / IoU | `feature.rs` semantics (`:53-60`) + `MasterBar` timeline | **new** | Silence is derived-never-stored; occupancy/IoU must be authored as a projection. |
| Syncopation (raw displacement profile) | `syncopation::displaced_beats` (`syncopation.rs:77`, private `(u32,u32)`) | **extend** | Only the boolean tag is public; extract `DisplacementProfile`, keep the tag as a policy over it. |
| Pitch range | `feature::PitchRange` + `VoiceFeatures.pitch_range` (`feature.rs:11,74`) | **reuse** | Public type over the canonical model; directly reusable. |
| Structure / repeatability | `StructureMetrics` (`structure.rs:84`); `measure_structure` (`:147`) | **reuse** (where bar-level semantics apply) | Fully implemented, serde-persisted; already an S7 similarity input. |
| Duration similarity | `similarity::ratio_similarity`/`scalar_similarity` (private); gesture `LineNote` duration | **new** (thin, reusing extracted helpers) | No per-note duration comparison axis exists; build it over an extracted ratio helper. |
| N-gram / contiguous-run leakage | `novelty::longest_common_run` (`:342`), n-gram match in `measure_novelty` (`:134-142`) | **reuse / extend** | Transposition- and resolution-aware quote/n-gram machinery is exactly the leakage check needed. |

Cross-cutting: the `reuse/extend` rows dominate — the Lab is a measurement
*re-projection* over an existing layer, and its main engineering risk is
metric-vocabulary drift, which the proposal's own measurement-policy contract
(§4, `TargetMeasurementPolicy`, a versioned identity distinct from
`WeightPolicy`) is designed to contain.

## 3. Holdout feasibility (proposal §5 item 3, §6 holdout discipline)

**Verdict: PASS, conditional on fail-closed handling of records without a
content identity — holdout is enforceable before corpus material is compiled;
one content-identity condition, one range caveat, and one attribution blocker
are recorded.**

Provenance carried by a chunk (`core/src/corpus.rs`, `SCHEMA_VERSION = 9`
`:58`): `ChunkMeta { id: ChunkId, title, source: SourceRef, … }` (`:369`);
`SourceRef { filename, format, bar_range: Option<(u32, u32)>, track_index:
Option<u32>, sha256: Option<String>, … }` (`:98`; `track_index` + `sha256` added
in v9 so "a second-guitar chunk must not reload as the first"). Chunk id and
song identity (`title`, `filename`, `sha256`) are **required**; the source range
`bar_range` is **`Option`**.

Path (verified end-to-end):

- **Loader** (`cli/src/generation_input.rs`): `load_chunk` (`:75`) deserializes
  the full `ChunkMeta` intact (`:81`) and *verifies* source identity by `sha256`
  → `filename` (`:87`), a hash mismatch being a hard failure (`:98`). Each record
  becomes a `LoadedChunk { meta, sliced, track }` (`core/src/generation_input.rs:65`),
  **provenance still attached**.
- **Clean holdout insertion point:** the `Vec<LoadedChunk>` handed to
  `corpus_material(loaded, skipped)` (`core/src/generation_input.rs:137`). Every
  element still exposes `.meta.id`, `.meta.title`, and `.meta.source`
  (`filename` / `sha256` / `bar_range` / `track_index`). Both holdout predicates
  the proposal names are satisfiable here: "every chunk of the target song"
  (match `filename`/`sha256`/`title`/`ensemble.group_id`) and "every chunk whose
  source range overlaps or contains the target" (match `bar_range`).
- **Identity-drop point (recorded):** `CorpusMaterial` is built at
  `core/src/generation_input.rs:152-157` with fields `rhythms:
  Vec<RhythmTemplate>`, `references: Vec<Score>`, `gesture`, `skipped` — **no
  `ChunkId`, no `SourceRef`**. Rhythm templates (`bar_rhythms`) and novelty
  references (`references.push(chunk.sliced)`, `:148`) carry no provenance
  downstream; the `metas` vector is used only for gesture control (`:155`) and
  then dropped. Confirmed downstream: `rerank_candidates` takes `references:
  &[Score]` (`rerank.rs:200`), and a novelty match records only a `usize` index
  into the anonymous `Vec<Score>` (`novelty.rs`), never a `ChunkId`.

Three Phase-1 items follow, each named honestly rather than waved off:

- **Condition (fail-closed content identity) — the load-bearing one.** Source
  identity is only reliable where a record carries `sha256`. `SourceRef.sha256`
  is `Option` (`corpus.rs:120`, added v9), and the loader keys the source on
  `sha256.unwrap_or_else(|| filename)` (`cli/src/generation_input.rs:87-91`),
  verifying the hash only `if let Some(expected)` (`:99-101`) — its own comment
  states "a filename is not an identity". So a corpus containing pre-v9
  (`sha256`-less) records **cannot reliably implement `HoldoutTargetSong` by
  content identity**, and a naive PASS would let a leaky run wear the costume of
  a valid holdout. The proposal's own §6 law resolves this: holdout modes **fail
  closed** — records lacking `sha256` must be **rejected or migrated** in
  holdout mode, never trusted by basename. Absent that, the missing content
  identity is a Phase-1 blocker, not a footnote. (Records *with* `sha256` are
  fully reliable — the content check runs and a mismatch is a hard failure.)
- **Caveat (holdout correctness):** because `bar_range` is `Option`, a
  range-overlap holdout must treat `bar_range == None` (whole-source) records as
  overlapping *any* range of the same file/`sha256`; ignoring `None` would
  silently leak whole-file records. This is a holdout-implementation
  requirement, not a data-model blocker.
- **Blocker (attribution only, not holdout):** provenance-tagged *leak
  attribution* ("which held-out chunk did this candidate quote?") is **not**
  possible today, because identity is dropped at `CorpusMaterial` and the
  novelty path returns only a positional index. Threading a `ChunkId` through
  `CorpusMaterial.references` → `rerank.rs` → the novelty match is a Phase-1
  prerequisite *only if* the Lab wants attribution; source-identity holdout
  itself runs upstream of the drop and needs no such change.

This confirms the proposal's own §6 statement that holdout must be enforced "by
source identity before material construction" — the plumbing supports it.

## 4. Target eligibility contract (proposal §5 item 4)

**Correction to the proposal's language:** there is no `ExactVoice` type. The
eligibility unit must be defined by Phase 1, not referenced.

- **Monophonic projection exists only as private helpers.** The
  `(onset, duration, pitch)` projection Phase 1 needs is realized by
  `gesture::top_line -> Vec<LineNote>` (`gesture.rs:169`, `LineNote =
  (u32, u32, u8)`), a private `fn` on `&Track`. It (and `novelty::top_line`)
  fold a chord to its highest pitch. Phase 1 must lift a shared projection
  primitive (see §2, "Exact pitch on paired onsets" / the fragmentation hazard).
- **The generator's output space is provably monophonic**, so the eligibility
  contract's restriction to monophonic targets is grounded, not conservative
  guesswork. Every strategy emits one `GenNote` per grid slot; `bars_to_score`
  (`generate.rs:434`) wraps each in its own `EventGroup { kind: Single, atoms:
  [Note] }` (`:461`) with `position: None`, `technique_spans: []`,
  `marks: NoteMarks::empty()`, producing a single `Track`/`Voice` (`:484`).
  Therefore **no chords, no techniques, no fret positions are ever emitted** —
  polyphonic / chordal / technique-bearing fragments are outside the reachable
  output space and are correctly ineligible for Phase 1. Onsets/durations are
  further constrained to the rhythm-template grid and pitches to the in-range
  `ScaleLadder` (`generate.rs:395`).

**Eligibility record (recommended shape for Phase 1):** every benchmark target
carries an explicit projection/eligibility record naming: the source (song id +
`bar_range` + `track_index`), the projection applied (top-line monophonic), and
an eligibility verdict (`Eligible` monophonic, or a typed `Ineligible`-reason —
polyphonic / technique-bearing / empty). The verdict is a *fact about the
projection*, mirroring the constraint-lab's "input precondition" discipline.

## 5. Cost benchmark (proposal §5 item 5)

The proposal wants measured generation cost at 1k / 10k / 100k trials across
three tiers — the number that gates any future directed-search discussion.

**Status: protocol specified, numbers `TBD (unmeasured)`.** No benchmark
infrastructure exists — no `criterion` in any `Cargo.toml` dev-deps or in
`Cargo.lock`, no `benches/` directory, no `[[bench]]` target (verified). The
Rust toolchain was unavailable in the environment that authored this audit, so
the numbers below are placeholders; the harness makes measuring them a
mechanical follow-up requiring **no production code**.

**Entry points a benchmark calls (cheapest → most representative):**

- `generate::generate(&RuleGenerationRequest)` (`core/src/generate.rs:353`) —
  single-candidate, pure, deterministic, no I/O. The cheapest honest unit of
  "generation cost".
- `rerank::generate_candidate_set(&SetRequest)` (`core/src/rerank.rs:137`) —
  `5 × variants_per_strategy` candidates per call (set-level cost).
- `generation_input::ranked_candidates(…)` (`core/src/generation_input.rs:215`)
  — the full generate + rerank + novelty pipeline (most representative; the CLI
  path).

**Three tiers (proposal §5):** generation only; generation + fingerprint
(a stable hash of the canonical `(onset, duration, pitch)` signature — the Lab's
Phase-1 fingerprint does not exist yet, so a `DefaultHasher` over the tuples is
a stand-in lower bound); generation + full metrics (`measure_structure` +
`measure_gesture` + `measure_complexity` + `measure_novelty` against a small
reference set — the last being the O(n·m·len) `longest_common_run`, the
expensive term).

| Tier | Entry points measured | 1k | 10k | 100k |
| --- | --- | --- | --- | --- |
| generation only | `generate` | TBD | TBD | TBD |
| generation + fingerprint | `generate` + signature hash | TBD | TBD | TBD |
| generation + full metrics | `generate` + structure/gesture/complexity/novelty | TBD | TBD | TBD |

**Reproducible harness (dev-only, adds no production code).** A throwaway
integration test under `core/tests/` — never shipped, `eprintln!` allowed by the
workspace lint (`print_stderr = "allow"`) — reusing the fixture pattern already
present at `core/tests/rule_generator.rs:49`. Sketch:

```rust
// core/tests/phase0_cost.rs  (dev-only; not part of Phase 0's committed output)
use std::time::Instant;
use griff_core::{generate::*, event::*, /* structure/gesture/novelty measures */};

fn request(seed: u64, bars: usize, strategy: GenerationStrategy) -> RuleGenerationRequest {
    RuleGenerationRequest {
        seed: GenerationSeed(seed),
        pitch_material: PitchMaterial { root: Pitch(40), intervals: vec![0, 3, 5, 7, 10] },
        constraints: GenerationConstraints {
            bar_count: bars,
            time_signature: TimeSignature { numerator: 4, denominator: 4 },
            tempo: Tempo::from_bpm_integer(120).unwrap(),
            ticks_per_quarter: Ticks(480),
            pitch_lo: Pitch(36), pitch_hi: Pitch(72),
        },
        explicit_rhythms: None,
        source_rhythms: vec![RhythmTemplate::from_durations(&[Ticks(240); 8])],
        strategy,
    }
}

#[test]
fn phase0_cost() {
    let (bars, strat) = (4, GenerationStrategy::ConstrainedRandomWalk);
    for &n in &[1_000usize, 10_000, 100_000] {
        let t = Instant::now();
        let mut sink = 0u64;
        for i in 0..n {
            let s = generate(&request(i as u64, bars, strat)).unwrap().score; // vary seed
            sink += s.tracks.len() as u64;
        }
        eprintln!("gen only  N={n}  per-trial={} ns  (sink={sink})",
                  t.elapsed().as_nanos() as f64 / n as f64);
    }
    // repeat with a fingerprint hash, and with structure/gesture/complexity/novelty.
}
```

Run: `cargo test --release -p griff-core --test phase0_cost -- --nocapture`.
Vary `seed` per iteration (real seed sweeps, defeats memoization). This test is
**not** committed as part of Phase 0 (which adds no production code); it is the
measurement instrument, run and its numbers folded back into the table above
before any directed-search discussion opens.

## 6. Roadmap placement recommendation (proposal §5 item 6)

Per the glossary §0 rule, **no stage number is assigned or invented here** — the
placement decision is a human one, and this audit only recommends a direction.

**Recommended direction: an ADR for the measurement/holdout boundary, plus an
isolated offline `lab/`-style crate, and *not* a new roadmap stage yet.**
Grounds:

- The Lab is an **offline research instrument**, structurally like the
  Constraint Lab (`lab/`, excluded from the workspace, `griff-core` read-only,
  never a CI gate — ADR-0010 / `fuzz/` precedent). The same isolation fits: it
  measures, it must not touch production scoring, and its outputs are archived
  fixtures/manifests, not a runtime.
- It **reuses** the existing metric layer (§1–§2), so it does not warrant a new
  stage of its own; what it genuinely adds is (a) a small number of *extracted*
  shared primitives in `core` (each a normal red→green slice with
  characterization tests, SPEC hard rule 5), and (b) a versioned
  **measurement-policy** contract + JSONL storage that live in the lab crate,
  not `core` (proposal §4: the Lab owns its wire DTOs; `core::scoring` gains no
  serde).
- The holdout plumbing already supports Phase 1 (§3); the only `core` change
  holdout strictly needs is *none* (it runs on `Vec<LoadedChunk>`), and the
  optional attribution change is a clearly bounded threading of `ChunkId`.

Concretely, the smallest honest path is: (1) an ADR fixing the
measurement-policy / holdout boundary and the lab-crate isolation; (2)
red→green extraction slices for the private primitives §2 marks `extend`; (3)
the Phase-1 census in the isolated crate. Whether any of this later earns a
canonical `SN` stage is deferred to the humans, exactly as the proposal's §2
decision states ("no stage number is assigned or invented now").

## 7. What this audit deliberately does not do

It measures no numbers (§5 is a protocol, not a result — toolchain unavailable);
it extracts no helpers and changes no `core` code (Phase 0 is no-production-code);
it does not decide the roadmap placement (a human call, §6); and it does not
re-litigate the proposal's architecture (§2–§4 there), only verifies that the
existing layer supports it. Each `extend` row in §2 is a *candidate* extraction,
not a committed one — Phase 1 owns those red→green slices.
