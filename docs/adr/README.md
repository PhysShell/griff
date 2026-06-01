# Architecture Decision Records

Nygard format, in-repo, append-only (ADR-0009). After `Accepted`, an ADR is
immutable; supersede it with a new one. New ADRs: copy
[`0000-template.md`](0000-template.md), use the next id, link it here.

| ADR | Title | Status |
|---|---|---|
| [0001](0001-use-rust-workspace.md) | Use a Rust workspace with core/cli/plugin crates | Accepted |
| [0002](0002-canonical-score-model.md) | Adopt a canonical score model as the internal representation | Accepted |
| [0003](0003-master-timeline-single-source-of-truth.md) | Master timeline is the single source of truth for transport | Accepted |
| [0004](0004-forbid-unsafe-code.md) | Forbid unsafe_code at the workspace level | Accepted |
| [0005](0005-swancore-first-scope.md) | griff is swancore-first, not a general-purpose riff generator | Accepted |
| [0006](0006-default-standard-e-tuning.md) | Default to Standard E tuning, not Drop C | Accepted |
| [0007](0007-clap-first-plugin-target.md) | CLAP is the plugin target; MIDI-out only; nih-plug | Accepted |
| [0008](0008-heuristic-phrase-detection-before-ml.md) | Explainable heuristic phrase detection before any ML | Accepted |
| [0009](0009-use-nygard-adr-format.md) | Use the Nygard ADR format, stored in-repo, append-only | Accepted |
| [0010](0010-fuzz-format-adapters-and-core-invariants.md) | Fuzz-test format adapters and core invariants | Accepted |
| [0011](0011-retire-legacy-linear-model.md) | Retire the legacy linear model in favour of the canonical model | Proposed |
| [0012](0012-complementary-part-generation.md) | Complementary part generation (ComplementArranger) | Proposed |
| [0013](0013-dp-viterbi-traversal.md) | DP/Viterbi traversal over the phrase hypergraph | Proposed |
| [0014](0014-fretboard-aware-model.md) | Fretboard-aware canonical model (string/fret positions) | Proposed |

See also: [`../SPEC.md`](../SPEC.md), [`../glossary.md`](../glossary.md),
[`../decisions.log.md`](../decisions.log.md).
