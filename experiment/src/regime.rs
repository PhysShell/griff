//! Information regimes: which channels of a prepared corpus one cell may see.

use griff_core::generation_input::{CorpusMaterial, CorpusMaterialView};

/// The corpus channels a cell's generation pass may consume.
///
/// A mask over an **already prepared** runtime population, and nothing else:
/// it never selects songs, never holds anything out, and `FULL` means "every
/// channel of the population the caller bound" — not an evaluation corpus, not
/// a leaky diagnostic, and not a valid holdout. What a pass actually took is
/// reported separately ([`griff_core::generation_input::CorpusContribution`]):
/// requesting a channel the population does not populate contributes nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InformationRegime {
    /// Corpus rhythm templates.
    pub rhythms: bool,
    /// Corpus novelty references.
    pub references: bool,
    /// The corpus gesture.
    pub gesture: bool,
}

impl InformationRegime {
    /// No channel: the pass runs on the seed alone.
    pub const SEED_ONLY: Self = Self {
        rhythms: false,
        references: false,
        gesture: false,
    };
    /// Rhythm templates only.
    pub const RHYTHMS_ONLY: Self = Self {
        rhythms: true,
        references: false,
        gesture: false,
    };
    /// Novelty references only.
    pub const REFERENCES_ONLY: Self = Self {
        rhythms: false,
        references: true,
        gesture: false,
    };
    /// The gesture only.
    pub const GESTURE_ONLY: Self = Self {
        rhythms: false,
        references: false,
        gesture: true,
    };
    /// Every channel of the bound population.
    pub const FULL: Self = Self {
        rhythms: true,
        references: true,
        gesture: true,
    };

    /// All eight channel combinations, `SEED_ONLY` first and `FULL` last.
    #[must_use]
    pub const fn all() -> [Self; 8] {
        [
            Self::SEED_ONLY,
            Self::RHYTHMS_ONLY,
            Self::REFERENCES_ONLY,
            Self {
                rhythms: true,
                references: true,
                gesture: false,
            },
            Self::GESTURE_ONLY,
            Self {
                rhythms: true,
                references: false,
                gesture: true,
            },
            Self {
                rhythms: false,
                references: true,
                gesture: true,
            },
            Self::FULL,
        ]
    }

    /// The view of `corpus` this regime lets a pass consume: each masked
    /// channel empty, each open channel borrowed whole. Without a corpus every
    /// channel is empty, whatever the regime asks for.
    #[must_use]
    pub const fn view(self, corpus: Option<&CorpusMaterial>) -> CorpusMaterialView<'_> {
        let Some(material) = corpus else {
            return CorpusMaterialView::empty();
        };
        CorpusMaterialView {
            rhythms: if self.rhythms {
                material.rhythms.as_slice()
            } else {
                &[]
            },
            references: if self.references {
                material.references.as_slice()
            } else {
                &[]
            },
            gesture: if self.gesture { material.gesture } else { None },
        }
    }
}
