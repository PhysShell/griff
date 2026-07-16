//! Session history, favorite/reject verdicts, and candidate provenance (S8
//! Slice 3).
//!
//! The append-only record a curator browses, and the honest, typed origin each
//! candidate carries — the ground S9's human-in-the-loop will stand on, without
//! any ranking, learning, or adaptation of its own.
//!
//! Backend-neutral and wasm-safe: pure data and pure transitions, so the
//! cockpit shell and any future frontend share one model. Provenance is a
//! **typed** value, never a pre-baked UI string — a renderer builds its own
//! description from it.

/// A curator's verdict on a candidate.
///
/// Modelled as an `Option<Verdict>` on a history entry: `None` is undecided,
/// and because a single slot holds it, favorite and rejected are mutually
/// exclusive by construction — setting one clears the other (see [`toggle`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The curator likes this candidate.
    Favorite,
    /// The curator rejects this candidate.
    Rejected,
}

/// The next verdict after the curator presses `action` on a slot holding
/// `current`.
///
/// Pressing the same verdict again clears it (an undo); pressing the other
/// switches to it — so favorite and rejected can never both hold. The one place
/// this transition lives, so no UI handler re-implements it.
#[must_use]
pub fn toggle(current: Option<Verdict>, action: Verdict) -> Option<Verdict> {
    if current == Some(action) {
        None
    } else {
        Some(action)
    }
}

/// Which generator produced a candidate — the coarse source a UI badges and a
/// curator filters by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateSource {
    /// The Generate panel's ranked set.
    Generate,
    /// A Swang program's evaluated set.
    Swang,
}

/// The generator-specific origin of a candidate — only the fields the pipeline
/// actually knows for that generator.
///
/// Kept as separate variants precisely so no candidate carries a fabricated
/// field: a Generate candidate has no program text, and a Swang candidate has
/// no seed / bars / gesture ask. Extend a variant (or add one) as generators
/// gain provenance; never invent a value the pass did not produce.
#[derive(Debug, Clone, PartialEq)]
pub enum GeneratorProvenance {
    /// A Generate-panel candidate: the ask it ran under and its rerank result.
    Generate {
        /// The seed the pass was given (a tab name, or the displayed score).
        source: Option<String>,
        /// Whether a corpus supplied templates / references / gesture.
        corpus: bool,
        /// The deterministic ask seed.
        seed: u64,
        /// Bars generated.
        bars: usize,
        /// Seed variants per strategy.
        variants_per_strategy: usize,
        /// Whether the gesture ask was carved.
        gesture: bool,
        /// The candidate's strategy.
        strategy: String,
        /// Its derived variant seed — its reproduction key within the set.
        variant_seed: u64,
        /// Its 1-based rank in the reranked set.
        rank: usize,
        /// Its weighted aggregate over the rerank axes.
        aggregate: f64,
    },
    /// A Swang candidate: the program that made it and its rerank result.
    Swang {
        /// The exact program text that produced the set.
        program: String,
        /// The declared `source` path, when the frontend resolved one.
        source_path: Option<String>,
        /// The candidate's strategy.
        strategy: String,
        /// Its derived variant seed.
        variant_seed: u64,
        /// Its 1-based rank in the reranked set.
        rank: usize,
        /// Its weighted aggregate over the rerank axes.
        aggregate: f64,
    },
}

/// The schema marker every [`Provenance`] carries, so a future reader (an S9
/// sidecar, a diff tool) can tell the shape apart from other records.
pub const PROVENANCE_SCHEMA: &str = "griff.candidate-provenance";

/// The current [`Provenance`] shape version.
pub const PROVENANCE_VERSION: u32 = 1;

/// A candidate's typed, backend-neutral origin — enough to answer where it came
/// from, what made it, and under what request, without a word of UI in it.
///
/// A renderer builds its own description from these fields; nothing here is a
/// pre-formatted string.
#[derive(Debug, Clone, PartialEq)]
pub struct Provenance {
    /// The schema marker ([`PROVENANCE_SCHEMA`]).
    pub schema: &'static str,
    /// The shape version ([`PROVENANCE_VERSION`]).
    pub version: u32,
    /// The reproducible content id of the candidate (`strategy#seed-hex`).
    pub candidate_id: String,
    /// The session-local creation order — a monotonic sequence number.
    pub sequence: u64,
    /// The generator-specific origin.
    pub generator: GeneratorProvenance,
}

impl Provenance {
    /// Stamps a provenance for `generator`, tagging it with the current schema
    /// and version and the candidate's content id and creation `sequence`.
    #[must_use]
    pub const fn new(sequence: u64, candidate_id: String, generator: GeneratorProvenance) -> Self {
        Self {
            schema: PROVENANCE_SCHEMA,
            version: PROVENANCE_VERSION,
            candidate_id,
            sequence,
            generator,
        }
    }

    /// The coarse source — which generator made the candidate.
    #[must_use]
    pub const fn source(&self) -> CandidateSource {
        match self.generator {
            GeneratorProvenance::Generate { .. } => CandidateSource::Generate,
            GeneratorProvenance::Swang { .. } => CandidateSource::Swang,
        }
    }
}

#[cfg(test)]
#[allow(clippy::missing_assert_message, clippy::panic)]
mod tests {
    use super::{
        toggle, CandidateSource, GeneratorProvenance, Provenance, Verdict, PROVENANCE_SCHEMA,
        PROVENANCE_VERSION,
    };

    fn generate_gen() -> GeneratorProvenance {
        GeneratorProvenance::Generate {
            source: Some("riff.mid".to_owned()),
            corpus: false,
            seed: 42,
            bars: 8,
            variants_per_strategy: 2,
            gesture: true,
            strategy: "RepeatVariation".to_owned(),
            variant_seed: 0x00c0_ffee,
            rank: 1,
            aggregate: 0.87,
        }
    }

    fn swang_gen() -> GeneratorProvenance {
        GeneratorProvenance::Swang {
            program: "swang 1\n\npattern p { ... }".to_owned(),
            source_path: Some("riff.mid".to_owned()),
            strategy: "RepeatVariation".to_owned(),
            variant_seed: 0x00c0_ffee,
            rank: 1,
            aggregate: 0.87,
        }
    }

    #[test]
    fn pressing_a_verdict_on_an_undecided_slot_sets_it() {
        assert_eq!(toggle(None, Verdict::Favorite), Some(Verdict::Favorite));
        assert_eq!(toggle(None, Verdict::Rejected), Some(Verdict::Rejected));
    }

    #[test]
    fn pressing_the_same_verdict_again_clears_it() {
        assert_eq!(toggle(Some(Verdict::Favorite), Verdict::Favorite), None);
        assert_eq!(toggle(Some(Verdict::Rejected), Verdict::Rejected), None);
    }

    #[test]
    fn favorite_clears_rejected_and_reject_clears_favorite() {
        assert_eq!(
            toggle(Some(Verdict::Rejected), Verdict::Favorite),
            Some(Verdict::Favorite),
            "favorite supplants rejected — never both",
        );
        assert_eq!(
            toggle(Some(Verdict::Favorite), Verdict::Rejected),
            Some(Verdict::Rejected),
            "reject supplants favorite — never both",
        );
    }

    #[test]
    fn provenance_new_stamps_the_schema_and_version() {
        let p = Provenance::new(
            3,
            "RepeatVariation#000000000c0ffee".to_owned(),
            generate_gen(),
        );
        assert_eq!(p.schema, PROVENANCE_SCHEMA);
        assert_eq!(p.version, PROVENANCE_VERSION);
        assert_eq!(p.sequence, 3);
        assert_eq!(p.candidate_id, "RepeatVariation#000000000c0ffee");
    }

    #[test]
    fn provenance_source_reflects_the_generator() {
        let g = Provenance::new(0, "x".to_owned(), generate_gen());
        let s = Provenance::new(1, "x".to_owned(), swang_gen());
        assert_eq!(g.source(), CandidateSource::Generate);
        assert_eq!(s.source(), CandidateSource::Swang);
    }

    #[test]
    fn provenance_carries_only_the_fields_its_generator_knows() {
        // The honest split: a Generate provenance holds the ask (seed/bars/
        // gesture) and no program; a Swang provenance holds the program and no
        // ask. The types make the other case unrepresentable — assert the shape.
        match Provenance::new(0, "x".to_owned(), generate_gen()).generator {
            GeneratorProvenance::Generate { seed, gesture, .. } => {
                assert_eq!(seed, 42);
                assert!(gesture);
            }
            GeneratorProvenance::Swang { .. } => panic!("a Generate candidate is not Swang"),
        }
        match Provenance::new(0, "x".to_owned(), swang_gen()).generator {
            GeneratorProvenance::Swang {
                program,
                source_path,
                ..
            } => {
                assert!(program.contains("swang"));
                assert_eq!(source_path.as_deref(), Some("riff.mid"));
            }
            GeneratorProvenance::Generate { .. } => panic!("a Swang candidate is not Generate"),
        }
    }
}
