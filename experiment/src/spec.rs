//! What an experiment asks: the fixed ask, the variant axis, the information
//! axis, and the evaluation context.

use griff_core::candidate_chain::chain_weights_v1;
use griff_core::generate::PitchMaterial;
use griff_core::generation_input::GenerationAsk;
use griff_core::rerank::rerank_weights_v1;
use griff_core::score::Score;
use griff_core::scoring::WeightPolicy;

use crate::bundle::{EvaluationContextV1, ExperimentSpecV1};
use crate::fingerprint::Fingerprint;
use crate::identity;
use crate::regime::InformationRegime;

/// A policy's stable name and version — the identity an experiment records for
/// every stage it ran.
///
/// Ownership follows the semantics: a production policy's identity is read
/// from the crate that implements it, never assigned here. Where the owning
/// crate has no identity yet, the value below is a manual contract pinned by a
/// characterization golden (`experiment/tests/identity_pins.rs`), recorded as
/// debt in ADR-0034.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PolicyIdentity {
    /// Stable identifier.
    pub id: &'static str,
    /// Version, bumped when the policy's behaviour changes.
    pub version: u32,
}

impl PolicyIdentity {
    /// The identity a core weight policy carries itself.
    #[must_use]
    pub const fn of_weights(policy: &WeightPolicy) -> Self {
        Self {
            id: policy.id,
            version: policy.version,
        }
    }
}

/// Which generator fans out the candidate set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GeneratorPolicy {
    /// Every S6 strategy × seed variants (`griff_core::rerank`).
    S6CandidateSet,
}

/// Which scorer ranks the candidate set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScorerPolicy {
    /// The uniform six-axis `generation_rerank` v1 policy.
    GenerationRerankV1,
}

/// Which selector turns a ranked set into one result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SelectorPolicy {
    /// S6 Intact: ranked candidate 0, whole.
    IntactTop,
    /// S7 Global Chain: one candidate per bar under `candidate_chain` v1.
    GlobalChainV1,
}

/// Which realizer, if any, maps the result onto an instrument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RealizerPolicy {
    /// No realization: the result is a score and nothing more.
    None,
}

impl GeneratorPolicy {
    /// The recorded identity.
    ///
    /// **Manual contract (debt):** `griff_core::rerank::generate_candidate_set`
    /// carries no identity of its own yet; this value is pinned to the
    /// candidate set's behaviour by a characterization golden.
    #[must_use]
    pub const fn identity(self) -> PolicyIdentity {
        match self {
            Self::S6CandidateSet => PolicyIdentity {
                id: "s6_candidate_set",
                version: 1,
            },
        }
    }
}

impl ScorerPolicy {
    /// The recorded identity — read from the core weight policy the scorer
    /// runs under.
    #[must_use]
    pub fn identity(self) -> PolicyIdentity {
        match self {
            Self::GenerationRerankV1 => PolicyIdentity::of_weights(&rerank_weights_v1()),
        }
    }
}

impl SelectorPolicy {
    /// The recorded identity.
    ///
    /// The global chain's is read from the core chain policy. **Manual
    /// contract (debt):** the intact selection (`select_ranked` with no
    /// strategy) carries no identity in core yet; its value is pinned by a
    /// characterization golden.
    #[must_use]
    pub fn identity(self) -> PolicyIdentity {
        match self {
            Self::IntactTop => PolicyIdentity {
                id: "intact_top",
                version: 1,
            },
            Self::GlobalChainV1 => PolicyIdentity::of_weights(&chain_weights_v1()),
        }
    }
}

impl RealizerPolicy {
    /// The recorded identity — owned here: "no realization" is this crate's
    /// own policy.
    #[must_use]
    pub const fn identity(self) -> PolicyIdentity {
        match self {
            Self::None => PolicyIdentity {
                id: "no_realization",
                version: 1,
            },
        }
    }
}

/// One algorithm variant: a typed choice at every pipeline stage.
///
/// Adding an experiment is a new policy arm and its adapter in the runner; a
/// frontend reads variants and their identities from the run and never matches
/// on a label.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VariantSpec {
    /// A human label, unique within a spec. Not an identity.
    pub label: String,
    /// The generator stage.
    pub generator: GeneratorPolicy,
    /// The scorer stage.
    pub scorer: ScorerPolicy,
    /// The selector stage.
    pub selector: SelectorPolicy,
    /// The realizer stage.
    pub realizer: RealizerPolicy,
}

impl VariantSpec {
    /// S6 Intact: the reranked winner, whole.
    #[must_use]
    pub fn s6_intact() -> Self {
        Self {
            label: "S6 Intact".to_owned(),
            generator: GeneratorPolicy::S6CandidateSet,
            scorer: ScorerPolicy::GenerationRerankV1,
            selector: SelectorPolicy::IntactTop,
            realizer: RealizerPolicy::None,
        }
    }

    /// S7 Global Chain over the same ranked set.
    #[must_use]
    pub fn s7_global_chain() -> Self {
        Self {
            label: "S7 Global Chain".to_owned(),
            generator: GeneratorPolicy::S6CandidateSet,
            scorer: ScorerPolicy::GenerationRerankV1,
            selector: SelectorPolicy::GlobalChainV1,
            realizer: RealizerPolicy::None,
        }
    }
}

/// The context evaluation metrics are measured in — fixed for a whole
/// experiment, and never defaulted to the runtime corpus.
#[derive(Debug, Clone)]
pub enum EvaluationContext {
    /// No evaluator: a run has policy objectives only, and no cross-regime
    /// comparison exists.
    None,
    /// The six generation axes (closure against `pitch_material`, novelty
    /// against `references`), measured on every result's first track against
    /// this one fixed context.
    GenerationAxes {
        /// The scale closure is measured against.
        pitch_material: PitchMaterial,
        /// The references novelty is measured against, supplied explicitly.
        references: Vec<Score>,
    },
}

impl EvaluationContext {
    /// The context's own fingerprint; `None` for [`EvaluationContext::None`].
    #[must_use]
    pub fn fingerprint(&self) -> Option<Fingerprint> {
        identity::evaluation_fingerprint(&EvaluationContextV1::from(self))
    }
}

/// Why a spec cannot run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecError {
    /// No variant to run.
    NoVariants,
    /// No information regime to run.
    NoRegimes,
    /// A regime listed twice.
    DuplicateRegime(InformationRegime),
    /// A variant label listed twice.
    DuplicateVariantLabel(String),
    /// A regime opens the gesture channel while the ask declines gesture — it
    /// would read as a gesture ablation while carving nothing.
    GestureChannelDeclinedByAsk(InformationRegime),
}

/// One reproducible experiment: everything but the inputs it runs over.
#[derive(Debug, Clone)]
pub struct ExperimentSpec {
    /// The ask every cell shares.
    pub ask: GenerationAsk,
    /// The variant axis.
    pub variants: Vec<VariantSpec>,
    /// The information axis.
    pub regimes: Vec<InformationRegime>,
    /// The fixed evaluation context.
    pub evaluation: EvaluationContext,
}

impl ExperimentSpec {
    /// Checks the spec can run.
    ///
    /// # Errors
    /// The first [`SpecError`] found.
    pub fn validate(&self) -> Result<(), SpecError> {
        if self.variants.is_empty() {
            return Err(SpecError::NoVariants);
        }
        if self.regimes.is_empty() {
            return Err(SpecError::NoRegimes);
        }
        for (i, regime) in self.regimes.iter().enumerate() {
            if self.regimes.iter().take(i).any(|seen| seen == regime) {
                return Err(SpecError::DuplicateRegime(*regime));
            }
        }
        for (i, variant) in self.variants.iter().enumerate() {
            if self
                .variants
                .iter()
                .take(i)
                .any(|seen| seen.label == variant.label)
            {
                return Err(SpecError::DuplicateVariantLabel(variant.label.clone()));
            }
        }
        if let Some(regime) = self
            .regimes
            .iter()
            .find(|regime| regime.gesture && !self.ask.gesture)
        {
            return Err(SpecError::GestureChannelDeclinedByAsk(*regime));
        }
        Ok(())
    }

    /// The spec's fingerprint: ask, variants' stage identities, regimes, and
    /// the evaluation context. Inputs (source, corpus) are identities of their
    /// own. Variant labels are for people and are not hashed; each variant's
    /// stage identities are.
    #[must_use]
    pub fn fingerprint(&self) -> Fingerprint {
        identity::spec_fingerprint(&ExperimentSpecV1::from(self))
    }
}
