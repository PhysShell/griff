# Research: symbolic harmony, global paths, and interactive evolution (2026-07)

Status: accepted as roadmap input; no dependency adoption
Decision: map the useful ideas onto S7/S8/S9/S12/S15 rather than create a
repository-per-stage roadmap

## Executive verdict

The strongest common pattern is:

```text
enumerate feasible local possibilities
→ assign explainable local / transition costs
→ optimise globally
→ return diverse top-k paths
→ let the human choose
→ retain feedback and lineage
```

The responsibilities are intentionally split:

- **S15** defines tonal/harmonic meaning and uncertainty;
- **S7** optimises multi-step paths;
- **S8** displays alternatives and provenance;
- **S9** captures selection and experiments with structural evolution;
- **S12** remains the owner of future neural assistance.

No reviewed repository is adopted as a production dependency by this decision.

## Source assessment

| Source | Value now | Later | What to take | Owner |
|---|---:|---:|---|---|
| [`ekzhang/harmony`](https://github.com/ekzhang/harmony) | 8/10 | 8/10 | layered DP, transition costs, path reconstruction | S7 |
| [`napulen/romanyh`](https://github.com/napulen/romanyh) | 8/10 | 9/10 | alternative harmonisations, k-best global paths, RomanText fixtures | S7 + S15 |
| [`napulen/AugmentedNet`](https://github.com/napulen/AugmentedNet) | 4/10 | 9/10 | decomposed harmonic targets, uncertainty, synthetic labelled examples | S15 + S12 radar |
| [`perfect-shuffle-music`](https://github.com/kroger66/perfect-shuffle-music) | 6/10 concept | 7/10 | human-in-the-loop generations, crossover concept, lineage | S9, surfaced by S8 |
| [`napulen/harmalysis`](https://github.com/napulen/harmalysis) | 4/10 | 7/10 | external harmonic fixture DSL inspiration | S15 |
| [`ekzhang/composing.studio`](https://github.com/ekzhang/composing.studio) | 3/10 | 6/10 | textual playground, live preview/playback, inspectable edits | S8 |
| [`ekzhang/crepe`](https://github.com/ekzhang/crepe) | 5/10 | 6/10 | possible declarative rule engine when rule closure/explanations justify it | S7/S15 radar |

The ratings describe architectural relevance to `griff`, not general project
quality.

## 1. `ekzhang/harmony`: global path shape

The useful lesson is not classical four-part harmony itself. It is the layered
optimisation shape:

1. enumerate valid states for each musical position;
2. calculate local and adjacent-state costs;
3. use dynamic programming to select a globally coherent path;
4. reconstruct the selected path with an inspectable total cost.

This is directly relevant to S7's multi-bar candidate chain. Potential Griff
transition terms include:

- phrase / contour continuity;
- register continuity;
- rhythm continuity or complement;
- repeated-technique penalties;
- playability / fret travel;
- harmonic fit supplied by S15;
- repetition and mud penalties.

Do **not** copy SATB-specific prohibitions or doubling/resolution rules as
swancore policy. Transfer the algorithmic form; make costs Griff-specific and
explainable.

The earlier idea of making a new generic `RegisterPlanner` the first client is
superseded: the register track is already accepted and closed after generator
semantics were repaired. Reopen register planning only when a new measured
multi-bar problem justifies it.

## 2. `napulen/romanyh`: k-best paths

The important extension is returning several ranked global alternatives rather
than one optimum. For Griff this supports:

- globally coherent alternatives without seed-only noise;
- several complementary-guitar trajectories;
- controlled register/harmonic alternatives;
- human selection feeding S9;
- explicit diversity constraints and fixed tie-breaking.

S7 should first extract a concrete layered-path contract from a real multi-bar
client, then add deterministic k-best enumeration. Avoid a universal
`MusicDPGodObject`; specialised state/cost modules share only the small engine.

## 3. Nápoles ecosystem: harmonic decomposition and fixtures

### `AugmentedNet`

Do not introduce its Python/TensorFlow/MusicXML runtime into Griff. Useful ideas:

- decompose harmonic analysis into tonal centre, mode, chord root, quality,
  inversion, and related targets;
- expose distributions/alternatives rather than one unquestioned label;
- create labelled synthetic examples and vary their texture.

S15 should grow incrementally from its accepted tonic/mode estimate. Full Roman
numeral analysis is not the next task.

### `harmalysis` and RomanText

Use a small text language only as an external fixture/debugging surface:

```text
C: I | vi | IV | V
C: I | V/V | V | I
a: i | VI | III | VII
```

Core representation stays typed. The fixture pipeline can generate
transpositions, omissions, inversions, passing tones, pedal textures, and
modulations with known labels for calibration tests.

### Audio key detection

Audio chroma/HMM approaches are deferred. Griff currently receives symbolic
MIDI/Guitar Pro evidence; rendering it to audio/chroma and guessing the lost
symbolic information back would be an avoidable information-loss loop. Revisit
only with a real audio-input stage.

## 4. `perfect-shuffle-music`: concept, not code

The implementation is too note-array-centric for Griff. Do not port its genome
or crossover code.

The useful experiment is structural evolution over Griff objects:

```text
candidate set
→ human selects parents
→ bar/motif/parameter crossover
→ mutation
→ normalisation and validators
→ existing rerank as a safety/quality guard
→ next generation
```

Candidate operators for an S9 experiment:

```rust
pub enum EvolutionOperator {
    AlternateBars,
    AlternateMotifs,
    RhythmFromAContourFromB,
    PrefixSuffixCrossover,
    ParameterBlend,
}
```

Lineage must record parents, operator, mutations, generation, and session. Track
population collapse, strategy diversity, operator survival, and repeated-parent
dominance. This remains an **Evolution Lab** experiment under S9; create a new
stage only if it later becomes a standalone persistent workflow.

## 5. `composing.studio`: S8 playground direction

The transferable UI idea is editable text plus immediate visual/audio feedback.
A Griff playground should expose:

- textual request / constraints / future harmonic fixture input;
- piano roll / tab / playback;
- candidate list and score axes;
- register, novelty, playability, and tonal diagnostics;
- path and transition explanations;
- like/dislike/favourite controls;
- parent/child lineage for the Evolution Lab.

S8 supplies the surface; it does not own tonal inference, path optimisation, or
preference semantics.

## 6. `crepe`: parked implementation option

A Datalog-like rule engine may become useful when Griff has all of:

- many independently maintained rules;
- recursive graph closure;
- a need to explain derivations;
- frequent rule additions that make imperative orchestration brittle.

Until then, typed Rust remains simpler. A pleasant macro is not evidence that the
generator needs an expert-system runtime.

## Roadmap mapping

### S7 — graph layer

- concrete layered-path contract from the first multi-bar client;
- global candidate-chain DP/Viterbi;
- deterministic k-best diverse paths;
- later harmonic/complement/cadence clients using S15 states.

### S8 — preview app / cockpit

- textual playground;
- candidate, tonal, path, and lineage inspectors;
- UI for S9 feedback/evolution experiments.

### S9 — human feedback

- feedback capture and preference reranking first;
- Evolution Lab second;
- diversity/collapse controls before productisation.

### S12 — neural assistance

- AugmentedNet/MiniBach remain decomposition and synthetic-data research inputs;
- no runtime model before the existing corpus, baseline, and feedback gates.

### S15 — tonal context and harmonic control

- Phase 0 evidence audit: accepted/closed;
- Phase 1 shared tonal core: accepted/closed;
- Phase 2 explicit scoped context: next;
- later calibration, fixture DSL, soft harmonic generation, local context and
  cadence.

## Immediate actions

1. Maintain S15 as the owner of tonal meaning and confidence.
2. Implement S15 Phase 2 without changing generated output.
3. Write the S7 layered-path design against a concrete multi-bar candidate-chain
   client before extracting a generic engine.
4. Add harmonic synthetic-fixture planning to S15 Phase 3/4.
5. Add Evolution Lab to the S9 backlog, not the production API.
6. Add the textual/candidate/tonal/lineage playground to the S8 backlog.

## Explicit non-decisions

- no automatic highest-margin track selection;
- no hard inferred-scale pitch whitelist;
- no generic register planner without new evidence;
- no classical harmony rule transplant;
- no neural or Datalog dependency;
- no new Evolution stage yet.
