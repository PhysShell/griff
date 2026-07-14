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
| [0011](0011-retire-legacy-linear-model.md) | Retire the legacy linear model in favour of the canonical model | Accepted |
| [0012](0012-complementary-part-generation.md) | Complementary part generation (ComplementArranger) | Proposed |
| [0013](0013-dp-viterbi-traversal.md) | DP/Viterbi traversal over the phrase hypergraph | Proposed |
| [0014](0014-fretboard-aware-model.md) | Fretboard-aware canonical model (string/fret positions) | Superseded by ADR-0018 |
| [0015](0015-structure-controls-and-metrics.md) | Separate structure controls and metrics from complexity | Proposed |
| [0016](0016-shared-ui-core-across-frontends.md) | Share one UI core across the ratatui and egui frontends | Proposed |
| [0017](0017-explainable-scoring-contract.md) | Unify scoring into axes, weights, rationale, and a derived aggregate | Proposed |
| [0018](0018-rich-note-model-fretboard-and-techniques.md) | Rich note model — fretboard position and multi-technique with evidence | Proposed |
| [0019](0019-infer-fretboard-position.md) | Infer fretboard position with a small local DP | Proposed |
| [0020](0020-gp-import-validation-harness.md) | Validate Guitar Pro import against a reference oracle | Proposed |
| [0021](0021-property-invariants-over-canonical-score.md) | Property-based invariants over the canonical Score | Proposed |
| [0022](0022-repeat-unfolding-as-projection.md) | Repeat unfolding is a projection, not a model rewrite | Proposed |
| [0023](0023-variation-control-for-complement.md) | Control pitch/contour spread of complementary parts | Proposed |
| [0024](0024-web-wasm-frontend-for-mobile.md) | Ship the egui frontend to the browser (WASM) for mobile testing | Proposed (§2–3,6 superseded by ADR-0025) |
| [0025](0025-guitar-pro-in-browser-needs-wasm-bindgen.md) | Guitar Pro in the browser needs wasm-bindgen (over the import-free web build) | Accepted |
| [0026](0026-web-chunk-capture-tool.md) | A minimal in-browser chunk.json capture tool (carve-out from "web curation — later") | Accepted |
| [0027](0027-egui-cockpit-curation-dock-and-opfs-persistence.md) | Grow the M1 playground into the egui M2 cockpit — curation dock + OPFS persistence | Proposed |
| [0028](0028-shared-theme-tokens-in-the-ui-core.md) | Shared theme tokens in the UI core — one palette, two renderers, asserted contrast | Proposed |
| [0029](0029-swang-authoring-and-verified-lifting.md) | Adopt Swang as a deterministic authoring and verified lifting language | Proposed |

See also: [`../SPEC.md`](../SPEC.md), [`../glossary.md`](../glossary.md),
[`../decisions.log.md`](../decisions.log.md).
