# Proposal: Generator Observatory

Turn the S8 cockpit from a viewer of single Generate runs into a reproducible
experiment surface: **algorithm variant × information regime**, run
headlessly, stored as an immutable bundle, inspected in the cockpit by eye
and ear.

Status: for discussion (design note — inventory and seams, before any code)
Scope: binds nothing. Implementation is gated on review of §7.

## 1. Inventory: the seams main already has (4f6c505)

| Seam | Where | What it gives the Observatory |
|---|---|---|
| One generation entry point | `core/src/generation_input.rs:232` `ranked_candidates` | CLI (`cli/src/main.rs:960`) and cockpit (`ui-core/src/generate.rs:205`) already share it |
| Corpus channels | `generation_input.rs:50` `CorpusMaterial { rhythms, references, gesture, skipped }` | three independent channels; `ranked_candidates` already reads each separately (rhythms fall back to the source's first bar when empty; gesture also needs `ask.gesture`) |
| Actual contribution | `ui-core/src/history.rs:164` `CorpusContribution { templates, references, gesture }` | requested ≠ actual is already modelled; `is_seed_only` |
| Candidate set + scorer | `core/src/rerank.rs:137,200` | S6 strategies × seed variants; `generation_rerank` v1 (six axes) |
| Selection only | `generation_input.rs:299` `select_ranked`; `core/src/candidate_chain.rs:530,733` | S6 Intact, strategy-pinned (S16), S7 Global Chain and its baseline — all read one `RankedSet` |
| One run, two results | `ui-core/src/generate.rs:205` `generate_run` | the `RankedSet` dies inside the function; re-planning is unrepresentable |
| Immutable run + A/B | `cockpit/src/generation.rs:41,65`; `cockpit/src/lib.rs:531,1615` | `ActiveGenerateRun`, `AuditionCandidate`, `show_score` transport |
| Typed provenance | `history.rs:66,206` | generator-split `GeneratorProvenance`, schema v3, run-level `ChainOutcomeRecord` (typed refusal) |
| Holdout boundary | `reachability-lab/src/lib.rs:39,187` (ADR-0032) | song / file / fragment holdout over `LoadedChunk`, before `corpus_material` folds provenance away |
| Content hashing | `core/src/corpus.rs:92` `source_sha256`; census `input_digest_of` | sha2 already a core dependency; a domain-tagged digest precedent |
| Fingering | `core/src/fretboard.rs:103` `infer_positions`; `core/src/midi.rs:405` | v1 DP, applied only at MIDI import |

## 2. Findings that change the brief

- **F1 — There is no TAB projection.** The cockpit renders a piano roll only;
  `preview/design/tab.html` is a mockup. Generated notes carry
  `position: None` (`core/src/generate.rs:469`, `candidate_chain.rs`), and
  fingering runs only on MIDI import. S6 Intact and S7 Global Chain
  therefore have **no realization** to show; a TAB view is new UI work, not
  a reuse.
- **F2 — Two of the obvious metrics depend on the regime.** Novelty is
  measured against the references the pass was given (`rerank.rs:225`; an
  empty set reads fully novel), and the chain's `candidate_quality` is
  `1 − S6 aggregate` (`candidate_chain.rs:398`), so it inherits that.
  Measured per cell, `(B1 − A1) − (B0 − A0)` would partly be the measuring
  stick changing. Cross-regime metrics need an evaluation context fixed by
  the spec (§3.3).
- **F3 — ADR-0032 (Accepted) owns holdout.** `CorpusMode`, `TargetIdentity`
  and all filtering live in the offline Reachability Lab, and the ADR binds
  "no holdout policy in the production CLI or cockpit". The brief's §14 fits
  only as *facts* in the bundle (identity, a leak check), with filtering and
  refusal staying in the lab. The name `CorpusMode` is taken there with a
  different meaning, so the regime type must not reuse it.
- **F4 — The CLI and cockpit loaders already diverge.** The CLI skips a
  source whose bytes do not match the record's `sha256`
  (`cli/src/generation_input.rs:99`); `cockpit/src/generation.rs:177` never
  checks. The same directory can compile two different `CorpusMaterial`s. A
  material-level corpus digest (§3.4) would make this visible. It is a
  pre-existing defect and should be fixed separately.
- **F5 — The Keep sidecar collapses the contribution to `corpus: bool`**
  (`cockpit/src/generation.rs:243`). It is derived from the actual
  contribution, but it is lossy. The bundle does not inherit it.
- **F6 — Nothing on the result path is serialisable.** `Score` has no serde.
  Scoring labels are `&'static str`. History is deliberately unserialisable
  (decisions.log, Global Chain Audition). A bundle needs its own wire types.
- **F7 — The web cockpit never attaches corpus material.** Only native
  `cockpit/src/main.rs:125` does. On the web the Observatory can run
  seed-only or open bundles, nothing more, until OPFS material exists.

## 3. Proposed architecture

**Placement.**
- The contract and the runner go in `griff-core` (`core/src/experiment/`).
  The CLI does not depend on `griff-ui-core`, and the brief requires one
  runner for CLI, cockpit and batch.
- The projection goes in `griff-ui-core` (`observatory.rs`).
- The panel goes in the cockpit.
- `griff experiment` goes in the CLI.
- Nothing from `lab/`, no solver, no plugins.

### 3.1 Information regime

```rust
pub struct InformationRegime { pub rhythms: bool, pub references: bool, pub gesture: bool }
// SEED_ONLY, RHYTHMS_ONLY, REFERENCES_ONLY, GESTURE_ONLY, FULL; all() -> [Self; 8]
```

- **Seam.** `ranked_candidates` becomes a thin wrapper over one
  implementation that takes a borrowed channel view:
  `ranked_candidates_in(score, CorpusView<'_>, ask, rhythm_override)`.
  Masking is then a view, not a clone of thousands of reference scores.
  `CorpusView::of(None)` and `CorpusView::of(Some(m))` reproduce today's
  behaviour exactly.
- **Identity proof.** Characterization tests, plus byte-identical
  `griff generate` runs on fixed seeds with and without a corpus (the
  30/30 precedent in S8).
- **Actual contribution.** It is derived from what the pass consumed, never
  from the mask. `CorpusContribution` moves to core and ui-core re-exports
  it, with `from_pass` kept.
- **Gesture guard.** A spec whose regimes enable gesture while its ask
  disables gesture is refused as typed (`SpecError`), so it cannot silently
  read as a gesture ablation.

### 3.2 Variant = a typed choice per pipeline stage

```rust
pub struct VariantSpec {
    pub label: String,
    pub generator: GeneratorPolicy, // S6CandidateSet        (later: SourcePassthrough for realization-only studies)
    pub scorer: ScorerPolicy,       // GenerationRerankV1    (later: S15 harmonic fit)
    pub selector: SelectorPolicy,   // IntactTop, GlobalChainV1 (later: KBest { k }, StrategyPinned)
    pub realizer: RealizerPolicy,   // None                  (later: FretboardDp { weights }, technique-aware)
    // validator: reserved; the hard-constraint contract is still a proposal
}
```

- **Identity.** Every policy reports `PolicyIdentity { id, version }`.
  Policies are registered statically; research policies sit behind a Cargo
  feature, named at implementation time.
- **Adding an experiment** means a new enum arm plus its adapter in the core
  runner. The cockpit renders variants from the bundle and never matches on
  a variant name.
- **Shared prefix.** Within one regime, variants with equal
  `(generator, scorer)` share **one** `RankedSet`. This generalises S8's
  "one run, two results" law.
- **Declared-axis check.** A test asserts that the candidate-set digest is
  equal across such variants.

### 3.3 Result, metrics, diagnostics

```rust
pub enum CellOutcome { Produced(ExperimentResult), Refused(PolicyRefusal) } // a refusal is never a fake result
pub struct ExperimentResult {
    pub score: Score,
    pub content: ContentDigest,
    pub realization: Option<Realization>, // per note: string, fret, hand?, techniques, chord shape?
    pub metrics: Vec<MetricValue>,
    pub diagnostics: Vec<Diagnostic>,
    pub contribution: CorpusContribution,
    pub trace: PolicyTrace,               // the existing chain trace, verbatim
}
```

- **Two typed metric classes.**
  - `Evaluation` is measured on the output against the spec's fixed
    `EvaluationContext`: the source's pitch material plus evaluation
    references named in the spec. These metrics are comparable across
    cells, so matrix effects and interactions are computed for them only.
  - `PolicyObjective` is the policy's own number under the cell's own
    inputs: S6 aggregate, chain total and baseline. It is comparable within
    a regime only; the matrix reports per-regime effects and never an
    interaction.
- **Slice-1 evaluation axes.** The six `generation_rerank` axes of the
  output's first track. This needs the private `rerank.rs:225` measurement
  exposed as a pub function.
- **No aggregate "quality".**
- **Diagnostics are a closed typed vocabulary.** Chain supplier, boundary
  fact, axis values now. Later: `RealizationDiff { note, a, b, reason }`
  with a typed reason code. Renderers match on the diagnostic kind, never
  on the variant.

### 3.4 Identity and fingerprint contract

- **`SourceIdentity`.** SHA-256 over the canonical encoding of the `Score`
  generation actually consumed, plus the file `sha256` and display label as
  optional facts.
- **`CorpusIdentity`.** Per-channel digests over the compiled material
  (`rhythms`, `references`, `gesture`), plus counts and the skipped names.
  - Digests are order-preserving, because palette order is behaviour.
  - They are computed in core, so any two loaders agree exactly when their
    material agrees.
- **Cell recipe digest.** Schema version, spec fields (ask including tonal,
  evaluation context), source digest, **only the enabled channels'
  digests**, and the policy identities.
  - Consequence: a corpus change confined to references cannot change a
    `SEED_ONLY` or `RHYTHMS_ONLY` cell's identity.
  - Naming follows the candidate-terrain proposal: this is its
    `GenerationRecipeId`, and `ContentDigest` is its `CandidateContentId`.
- **Excluded from every digest.** Paths, run and history ids, timestamps,
  UI state.
- **Encoding.** Canonical JSON of the wire types: fixed field order, no
  maps, `float_roundtrip` already on in the workspace. Floats inside
  identities are hashed via `to_bits`.

### 3.5 Bundle

- **Contents.** `griff.experiment-bundle` v1: spec, source identity, corpus
  identity, evaluation-context identity, cells with outcomes and results.
  - Wire types mirror the core types; `Score` gets a lossless wire form
    with a round-trip property test.
  - Slice 1 carries one source, but the top level is a `cases` list so
    batch runs (M2) need no schema break.
- **Opening.** `ExperimentBundle::from_json` is the only input to the
  projection. No material and no `RankedSet` are in scope, so
  regeneration is unrepresentable.
- **Leakage (M2, field reserved).** `source_in_runtime_corpus` by `sha256` /
  `song_id` takes values Yes / No / Unknown.
  - It needs record identities captured before `corpus_material` folds them.
  - It is a fact, not a policy. A lab-filtered corpus enters with its
    ADR-0032 mode and target recorded.

### 3.6 Cockpit and CLI (milestone 1)

- **Cockpit window.**
  - Pick variant A/B and a regime from the bundle.
  - Audition through the existing `show_score` via a new
    `AuditionCandidate` arm (the A/B key is unchanged).
  - Show the evaluation deltas and the within-regime objectives, labelled
    apart.
  - Show the requested regime next to the actual contribution, plus
    provenance and diagnostics.
  - "Run" executes the default 2×2 (S6 Intact / S7 Global Chain ×
    `SEED_ONLY` / `FULL`) through the core runner. "Open" reads a bundle
    (native first).
  - TAB shows "no realization", which is true for these policies (F1).
- **Existing Generate panel.** Untouched in M1. Folding `generate_run` into
  the runner is a later, separately proven step.
- **History.** Additive `GeneratorProvenance::Experiment` (provenance v4)
  so favourite/reject work on experiment cells.
- **CLI.** `griff experiment run INPUT --corpus DIR --seed --bars
  --candidates --variants intact,chain --regimes seed-only,full --out
  B.json` and `griff experiment show B.json` (reads the bundle only).

## 4. Matrix (milestone 2; the types exist in M1)

- For each `Evaluation` axis: `B0 − A0`, `B1 − A1`, and their difference.
- Channel ablation runs all eight masks from `InformationRegime::all()`
  with no per-experiment branches.
- Explorer queries (source, variant, regime, improvement/regression,
  technique, diagnostic kind) are a typed model before any UI.
- No Independent/Enhanced/Dependent labels until thresholds are
  pre-registered.

## 5. Tests (brief §18)

- **Determinism.** The same spec yields a byte-identical bundle.
- **Declared axis.** Variants differing only by selector have equal
  candidate-set digests.
- **Seed-only.**
  - `SEED_ONLY` is output-identical to `material: None`, with zero
    contribution.
  - An attached but empty or all-skipped corpus leaves `FULL` seed-only.
- **Channel leaks.**
  - `RHYTHMS_ONLY` produces novelty identical to seed-only.
  - `REFERENCES_ONLY` rotates the source's first bar.
  - `GESTURE_ONLY` over a corpus with no resting stats carves nothing.
- **Round trips.** The bundle, `Score` wire form and provenance round-trip.
  Opening a bundle builds the projection with no corpus and no source.
- **Headless = GUI.** The projection shows the headless result's metrics,
  provenance and diagnostics verbatim.
- **Regression.** `cargo test --workspace` and byte-identical
  `griff generate` on fixed seeds after every core step. The existing
  Generate, S6/S7 A/B, history and keep tests stay green.

## 6. Order

A inventory (done) → B this note → review (§7) → C red: experiment
contract → D green: headless runner → E bundle + identity → F cockpit A/B
projection → G `SEED_ONLY` vs `FULL` → H all channel masks → I matrix
tests → J docs → K hostile review. Red and green are separate commits
throughout.

## 7. Decisions needed before step C

1. **Placement.** A core `experiment` module (recommended) or a new
   workspace crate.
2. **`Score` in the bundle.** A wire mirror with a lossless round-trip test
   (recommended) or serde derived on the model (ADR territory).
   - The mirror keeps the canonical model and history serde-free, as the
     Global Chain Audition entry did.
   - It departs from that entry's "the frontend owns the wire format":
     the CLI writes a bundle and the cockpit reads it, so the wire types
     must live in shared core. The ADR records that departure.
3. **Evaluation context.** Novelty is measured against references fixed by
   the spec, defaulting to the full snapshot and labelled as such
   (recommended). Per-cell references are rejected: see F2.
4. **§14 against ADR-0032.** Facts in the bundle, filtering in the lab
   (recommended), or an ADR amendment if the Observatory should own splits.
5. **TAB.** No TAB view in milestone 1, because nothing exists to show for
   S6/S7 (recommended). The alternative is a minimal TAB projection now
   plus a `FretboardDp` v1 realizer.
6. **Loader divergence (F4).** A separate small fix first (recommended: the
   headless-vs-cockpit corpus identity test would otherwise be false on a
   hash-mismatched corpus) or fold it into the Observatory.
7. **Documents.** After acceptance: an ADR for experiment identity and the
   bundle (a new persisted contract), an S8 progress note, and a
   decisions.log entry for placement. This proposal then becomes historical
   context, per the proposals lifecycle.
