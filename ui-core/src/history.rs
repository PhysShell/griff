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

use griff_core::score::Score;

use crate::generate::SetSummary;

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

/// What a corpus **actually** contributed to a generation — not merely whether
/// one was attached.
///
/// An attached corpus that is empty, or whose records were all skipped, gives
/// no templates, references, or gesture; provenance must say so rather than
/// claim "corpus". Built from the pass's real result — the corpus's own rhythm
/// count plus the set summary — via [`CorpusContribution::from_pass`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorpusContribution {
    /// Rhythm templates the corpus supplied (its own, not the source's).
    pub templates: usize,
    /// Novelty reference chunks the corpus supplied.
    pub references: usize,
    /// Whether a corpus gesture was **actually carved** (the ask enabled it and
    /// the corpus had one) — never merely whether the ask requested gesture.
    pub gesture: bool,
}

impl CorpusContribution {
    /// Derives the contribution from the corpus's own rhythm-template count and
    /// the pass's [`SetSummary`] — `references` and the actually-carved
    /// `gesture` come from the summary; `templates` is the corpus's rhythm
    /// count (0 without a corpus), since the summary's template count also folds
    /// in the source's own rhythms.
    #[must_use]
    pub fn from_pass(corpus_templates: usize, summary: &SetSummary) -> Self {
        let _ = (corpus_templates, summary);
        unimplemented!("CorpusContribution::from_pass")
    }

    /// Whether the corpus contributed nothing — the pass ran on the seed alone.
    #[must_use]
    pub fn is_seed_only(&self) -> bool {
        unimplemented!("CorpusContribution::is_seed_only")
    }
}

/// The schema marker every [`Provenance`] carries, so a future reader (an S9
/// sidecar, a diff tool) can tell the shape apart from other records.
pub const PROVENANCE_SCHEMA: &str = "griff.candidate-provenance";

/// The current [`Provenance`] shape version. v2 adds the generation-run
/// identity and distinguishes it from the history sequence.
pub const PROVENANCE_VERSION: u32 = 2;

/// A candidate's typed, backend-neutral origin — enough to answer where it came
/// from, what made it, and under what request, without a word of UI in it.
///
/// A renderer builds its own description from these fields; nothing here is a
/// pre-formatted string. The `run` and `sequence` are distinct identities: the
/// former groups a generation's candidates, the latter orders history entries.
#[derive(Debug, Clone, PartialEq)]
pub struct Provenance {
    /// The schema marker ([`PROVENANCE_SCHEMA`]).
    pub schema: &'static str,
    /// The shape version ([`PROVENANCE_VERSION`]).
    pub version: u32,
    /// The generation run that produced the candidate — a session-local id
    /// shared by every candidate of one Generate/Swang set.
    pub run: GenerationRunId,
    /// The candidate key **within its run** (`strategy#variant_seed`). Not a
    /// content hash — it does not reproduce a score without its request/inputs.
    pub candidate_id: String,
    /// The session-local history order — a monotonic sequence number, distinct
    /// from `run`.
    pub sequence: u64,
    /// The generator-specific origin.
    pub generator: GeneratorProvenance,
}

impl Provenance {
    /// Stamps a provenance for `generator`, tagging it with the current schema
    /// and version, the generation `run`, the within-run candidate key, and the
    /// history `sequence`.
    #[must_use]
    pub const fn new(
        run: GenerationRunId,
        sequence: u64,
        candidate_id: String,
        generator: GeneratorProvenance,
    ) -> Self {
        Self {
            schema: PROVENANCE_SCHEMA,
            version: PROVENANCE_VERSION,
            run,
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

/// A stable, session-local candidate identity.
///
/// A monotonic id, **never an index into the list**, so it survives appends,
/// verdicts, re-selection, and A/B without ever pointing at the wrong entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HistoryId(pub u64);

/// A session-local identity for one **generation run** — a single successful
/// Generate or Swang candidate set.
///
/// Every candidate of a set shares one id; a fresh generation mints a new one
/// (see [`SessionHistory::begin_run`]). This is what scopes de-duplication: a
/// candidate key like `strategy#variant_seed` is stable only *within* a run —
/// it is not a content hash and does not reproduce a score without its request
/// and inputs — so two runs that happen to share a key are still distinct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GenerationRunId(pub u64);

/// One recorded candidate: an immutable snapshot plus a mutable verdict.
///
/// The snapshot (`score`, `provenance`, `candidate_id`, `title`, `run`) is
/// fixed at record time; only `verdict` changes afterwards.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    /// The stable identity.
    pub id: HistoryId,
    /// Creation order — a monotonic sequence number (equals `id.0`).
    pub sequence: u64,
    /// The generation run this candidate belongs to — the other half of its
    /// de-dupe key.
    pub run: GenerationRunId,
    /// Which generator produced it.
    pub source: CandidateSource,
    /// The candidate key **within its run** (`strategy#variant_seed`). Not a
    /// content hash: unique per row inside one run, but two runs may repeat it.
    pub candidate_id: String,
    /// The display label the roll showed it under.
    pub title: String,
    /// The candidate's score — an immutable snapshot, owned by the entry.
    pub score: Score,
    /// The curator's verdict, or `None` while undecided.
    pub verdict: Option<Verdict>,
    /// The candidate's typed origin.
    pub provenance: Provenance,
}

/// The session's append-only candidate history.
///
/// A new generation **adds** to it; it never destroys prior entries. Entries
/// are keyed by a stable [`HistoryId`], and selection is a separate pointer, so
/// the record a curator built does not depend on the current UI selection.
#[derive(Debug, Clone, Default)]
pub struct SessionHistory {
    /// Entries in creation order.
    entries: Vec<HistoryEntry>,
    /// The next id to hand out — monotonic, never reused.
    next_id: u64,
    /// The next generation-run id to hand out — monotonic, never reused.
    next_run_id: u64,
    /// The currently selected entry, if any (a view pointer, not identity).
    selected: Option<HistoryId>,
}

impl SessionHistory {
    /// An empty history.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Mints a fresh [`GenerationRunId`] for one successful candidate set.
    ///
    /// The caller assigns it once per Generate or Swang run and passes it to
    /// [`Self::record`] for every candidate of that set, so a new run never
    /// collides with an earlier one — even when the ask and the resulting music
    /// deterministically match.
    pub const fn begin_run(&mut self) -> GenerationRunId {
        let id = GenerationRunId(self.next_run_id);
        self.next_run_id = self.next_run_id.saturating_add(1);
        id
    }

    /// Records a shown candidate of run `run` and returns its stable id.
    ///
    /// Append-only and **de-duplicated within the run**: a candidate already
    /// present (same `run` and candidate key) returns its existing id unchanged
    /// — no duplicate row, and its verdict and snapshot are left intact. A
    /// candidate of a different run is always appended, even if its key repeats
    /// an earlier run's; prior entries never move or mutate.
    #[allow(clippy::too_many_arguments)] // run + key + title + snapshot + generator are irreducible
    pub fn record(
        &mut self,
        run: GenerationRunId,
        candidate_id: String,
        title: String,
        score: Score,
        generator: GeneratorProvenance,
    ) -> HistoryId {
        // De-dupe **within the run**: the same row of the same generation keeps
        // its id, snapshot, and verdict. A different run is always a new entry,
        // even if its key repeats — the key is not a content hash.
        if let Some(existing) = self
            .entries
            .iter()
            .find(|e| e.run == run && e.candidate_id == candidate_id)
        {
            return existing.id;
        }
        let id = HistoryId(self.next_id);
        let provenance = Provenance::new(run, self.next_id, candidate_id.clone(), generator);
        self.entries.push(HistoryEntry {
            id,
            sequence: self.next_id,
            run,
            source: provenance.source(),
            candidate_id,
            title,
            score,
            verdict: None,
            provenance,
        });
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// The entries, in creation order.
    #[must_use]
    pub fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }

    /// The entry with `id`, if it exists.
    #[must_use]
    pub fn get(&self, id: HistoryId) -> Option<&HistoryEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Selects entry `id` (a no-op if it is not present).
    pub fn select(&mut self, id: HistoryId) {
        if self.entries.iter().any(|e| e.id == id) {
            self.selected = Some(id);
        }
    }

    /// Clears the selection without touching the entries or their verdicts —
    /// the fresh-load lifecycle seam: a new file has no active history row, so
    /// nothing should read as selected or playing, but the record is preserved.
    pub const fn clear_selection(&mut self) {
        self.selected = None;
    }

    /// The selected entry's id, if any.
    #[must_use]
    pub const fn selected(&self) -> Option<HistoryId> {
        self.selected
    }

    /// Applies verdict `action` to entry `id` via [`toggle`] (a no-op if the
    /// entry is gone). Re-pressing the same verdict clears it.
    pub fn set_verdict(&mut self, id: HistoryId, action: Verdict) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
            entry.verdict = toggle(entry.verdict, action);
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::missing_assert_message,
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used
)]
mod tests {
    use super::{
        toggle, CandidateSource, CorpusContribution, GenerationRunId, GeneratorProvenance,
        Provenance, SessionHistory, Verdict, PROVENANCE_SCHEMA, PROVENANCE_VERSION,
    };
    use crate::generate::SetSummary;
    use griff_core::score::{LossReport, Score};

    fn summary(references: usize, gesture: bool) -> SetSummary {
        SetSummary {
            templates: 4, // the source's own rhythms — must not be read as corpus
            references,
            gesture: gesture.then(|| (3usize, "1.0q".to_owned())),
            scale_tones: 7,
            skipped: Vec::new(),
        }
    }

    fn score() -> Score {
        Score {
            ticks_per_quarter: 960,
            master_bars: Vec::new(),
            tracks: Vec::new(),
            source_meta: None,
            loss: LossReport::new(),
        }
    }

    fn generate_gen() -> GeneratorProvenance {
        generate_gen_with(false)
    }

    fn generate_gen_with(corpus: bool) -> GeneratorProvenance {
        GeneratorProvenance::Generate {
            source: Some("riff.mid".to_owned()),
            corpus,
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
    fn provenance_new_stamps_schema_version_and_run() {
        let p = Provenance::new(
            GenerationRunId(5),
            3,
            "RepeatVariation#000000000c0ffee".to_owned(),
            generate_gen(),
        );
        assert_eq!(p.schema, PROVENANCE_SCHEMA);
        assert_eq!(p.version, PROVENANCE_VERSION);
        assert_eq!(p.run, GenerationRunId(5), "the generation run is recorded");
        assert_eq!(p.sequence, 3, "distinct from the run");
        assert_eq!(p.candidate_id, "RepeatVariation#000000000c0ffee");
    }

    #[test]
    fn provenance_source_reflects_the_generator() {
        let g = Provenance::new(GenerationRunId(0), 0, "x".to_owned(), generate_gen());
        let s = Provenance::new(GenerationRunId(0), 1, "x".to_owned(), swang_gen());
        assert_eq!(g.source(), CandidateSource::Generate);
        assert_eq!(s.source(), CandidateSource::Swang);
    }

    #[test]
    fn provenance_carries_only_the_fields_its_generator_knows() {
        // The honest split: a Generate provenance holds the ask and no program;
        // a Swang provenance holds the program and no ask. The types make the
        // other case unrepresentable — assert the shape.
        let g = Provenance::new(GenerationRunId(0), 0, "x".to_owned(), generate_gen());
        match g.generator {
            GeneratorProvenance::Generate { seed, .. } => assert_eq!(seed, 42),
            GeneratorProvenance::Swang { .. } => panic!("a Generate candidate is not Swang"),
        }
        let s = Provenance::new(GenerationRunId(0), 0, "x".to_owned(), swang_gen());
        match s.generator {
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

    #[test]
    fn record_appends_an_entry_with_a_stable_id() {
        let mut h = SessionHistory::new();
        let run = h.begin_run();
        let id = h.record(
            run,
            "a#1".to_owned(),
            "A".to_owned(),
            score(),
            generate_gen(),
        );
        assert_eq!(h.entries().len(), 1);
        let entry = h.get(id).expect("the entry is retrievable by its id");
        assert_eq!(entry.candidate_id, "a#1");
        assert_eq!(entry.run, run, "the entry carries its run");
        assert_eq!(entry.source, CandidateSource::Generate);
        assert_eq!(entry.verdict, None);
        assert_eq!(entry.provenance.schema, PROVENANCE_SCHEMA);
        assert_eq!(entry.provenance.version, PROVENANCE_VERSION);
        assert_eq!(entry.provenance.run, run, "provenance names the run");
    }

    #[test]
    fn begin_run_hands_out_distinct_ids() {
        let mut h = SessionHistory::new();
        assert_ne!(h.begin_run(), h.begin_run(), "each run is a fresh identity");
    }

    #[test]
    fn re_recording_the_same_row_within_a_run_dedupes_and_keeps_the_verdict() {
        // Law 7 + 8: re-showing the same row of the current run returns the same
        // HistoryId and preserves its verdict.
        let mut h = SessionHistory::new();
        let run = h.begin_run();
        let a = h.record(
            run,
            "a#1".to_owned(),
            "A".to_owned(),
            score(),
            generate_gen(),
        );
        h.set_verdict(a, Verdict::Favorite);
        let again = h.record(
            run,
            "a#1".to_owned(),
            "A".to_owned(),
            score(),
            generate_gen(),
        );
        assert_eq!(again, a, "same run + key returns the existing id");
        assert_eq!(h.entries().len(), 1, "no duplicate row");
        assert_eq!(
            h.get(a).expect("still there").verdict,
            Some(Verdict::Favorite),
            "the verdict survives the re-show",
        );
    }

    #[test]
    fn the_same_key_in_two_generate_runs_makes_two_entries() {
        // Law 1: identical candidate_id across two Generate runs → two entries.
        let mut h = SessionHistory::new();
        let r1 = h.begin_run();
        let r2 = h.begin_run();
        let a = h.record(
            r1,
            "auto#1".to_owned(),
            "A".to_owned(),
            score(),
            generate_gen(),
        );
        let b = h.record(
            r2,
            "auto#1".to_owned(),
            "B".to_owned(),
            score(),
            generate_gen(),
        );
        assert_ne!(a, b, "a new run never collapses onto an earlier one");
        assert_eq!(h.entries().len(), 2);
    }

    #[test]
    fn different_inputs_same_key_do_not_collapse() {
        // Law 2/3/4: different inputs (source score, corpus, gesture) are
        // different runs, so the same key stays two entries — the run scopes it.
        let mut h = SessionHistory::new();
        let r_plain = h.begin_run();
        let r_corpus = h.begin_run();
        let a = h.record(
            r_plain,
            "auto#1".to_owned(),
            "A".to_owned(),
            score(),
            generate_gen_with(false),
        );
        let b = h.record(
            r_corpus,
            "auto#1".to_owned(),
            "B".to_owned(),
            score(),
            generate_gen_with(true),
        );
        assert_ne!(a, b);
        assert_eq!(
            h.entries().len(),
            2,
            "corpus vs no-corpus are separate runs"
        );
    }

    #[test]
    fn different_swang_programs_same_key_do_not_collapse() {
        // Law 5/6: two Swang executions are two runs; the same key stays two
        // entries even if the displayed source or program differ.
        let mut h = SessionHistory::new();
        let r1 = h.begin_run();
        let r2 = h.begin_run();
        let a = h.record(
            r1,
            "auto#1".to_owned(),
            "P1".to_owned(),
            score(),
            swang_gen(),
        );
        let b = h.record(
            r2,
            "auto#1".to_owned(),
            "P2".to_owned(),
            score(),
            swang_gen(),
        );
        assert_ne!(a, b);
        assert_eq!(h.entries().len(), 2, "each Swang execution is its own run");
    }

    #[test]
    fn a_new_run_never_destroys_or_mutates_a_prior_entry() {
        // Law 9/10: a later run keeps every earlier snapshot, provenance, and
        // verdict — the old entry is not overwritten by a same-key candidate.
        let mut h = SessionHistory::new();
        let r1 = h.begin_run();
        let a = h.record(
            r1,
            "auto#1".to_owned(),
            "A".to_owned(),
            score(),
            generate_gen(),
        );
        h.set_verdict(a, Verdict::Favorite);
        let r2 = h.begin_run();
        let _b = h.record(
            r2,
            "auto#1".to_owned(),
            "B".to_owned(),
            score(),
            swang_gen(),
        );
        let first = h.get(a).expect("the first entry survives");
        assert_eq!(first.candidate_id, "auto#1");
        assert_eq!(first.title, "A", "its snapshot title is untouched");
        assert_eq!(first.run, r1, "its run is untouched");
        assert_eq!(
            first.verdict,
            Some(Verdict::Favorite),
            "its verdict is kept"
        );
        assert_eq!(first.sequence, 0, "and its order is fixed");
        assert_eq!(first.provenance.source(), CandidateSource::Generate);
    }

    #[test]
    fn the_same_key_from_two_sources_is_two_entries() {
        // Generate and Swang each mint their own run, so a shared key is two
        // entries.
        let mut h = SessionHistory::new();
        let rg = h.begin_run();
        let rs = h.begin_run();
        let g = h.record(
            rg,
            "x#1".to_owned(),
            "G".to_owned(),
            score(),
            generate_gen(),
        );
        let s = h.record(rs, "x#1".to_owned(), "S".to_owned(), score(), swang_gen());
        assert_ne!(g, s);
        assert_eq!(h.entries().len(), 2);
    }

    #[test]
    fn set_verdict_toggles_and_stays_exclusive() {
        let mut h = SessionHistory::new();
        let run = h.begin_run();
        let a = h.record(
            run,
            "a#1".to_owned(),
            "A".to_owned(),
            score(),
            generate_gen(),
        );
        h.set_verdict(a, Verdict::Favorite);
        assert_eq!(h.get(a).unwrap().verdict, Some(Verdict::Favorite));
        h.set_verdict(a, Verdict::Rejected);
        assert_eq!(
            h.get(a).unwrap().verdict,
            Some(Verdict::Rejected),
            "reject supplants favorite",
        );
        h.set_verdict(a, Verdict::Rejected);
        assert_eq!(h.get(a).unwrap().verdict, None, "re-press clears it");
    }

    #[test]
    fn selection_is_a_separate_pointer_from_the_entries() {
        let mut h = SessionHistory::new();
        let run = h.begin_run();
        let a = h.record(
            run,
            "a#1".to_owned(),
            "A".to_owned(),
            score(),
            generate_gen(),
        );
        let b = h.record(
            run,
            "b#2".to_owned(),
            "B".to_owned(),
            score(),
            generate_gen(),
        );
        assert_eq!(h.selected(), None, "nothing selected until asked");
        h.select(b);
        assert_eq!(h.selected(), Some(b));
        // Selecting does not touch the entries, and both remain retrievable.
        assert!(h.get(a).is_some() && h.get(b).is_some());
        h.select(a);
        assert_eq!(h.selected(), Some(a), "selection moves freely");
    }

    #[test]
    fn corpus_contribution_no_corpus_is_seed_only() {
        // Law 1: no corpus → no templates, references, or gesture.
        let c = CorpusContribution::from_pass(0, &summary(0, false));
        assert!(c.is_seed_only(), "no corpus contributes nothing");
        assert_eq!(c.templates, 0);
        assert_eq!(c.references, 0);
        assert!(!c.gesture);
    }

    #[test]
    fn corpus_contribution_attached_but_empty_is_seed_only() {
        // Law 2: an attached-but-empty corpus (0 rhythms, 0 references, no
        // carved gesture) still reads as seed-only — attachment is not use.
        let c = CorpusContribution::from_pass(0, &summary(0, false));
        assert!(c.is_seed_only());
    }

    #[test]
    fn corpus_contribution_reflects_references_and_templates() {
        // Law 3/4: references come from the summary; the corpus rhythm count is
        // reported as templates (never the summary's source-inclusive count).
        let refs_only = CorpusContribution::from_pass(0, &summary(5, false));
        assert_eq!(refs_only.references, 5);
        assert_eq!(
            refs_only.templates, 0,
            "no corpus rhythms → 0, not the source's 4"
        );
        assert!(!refs_only.is_seed_only());

        let with_templates = CorpusContribution::from_pass(3, &summary(0, false));
        assert_eq!(with_templates.templates, 3, "the corpus's own rhythm count");
        assert!(!with_templates.is_seed_only());
    }

    #[test]
    fn corpus_contribution_gesture_tracks_actual_carving() {
        // Law 5/6: gesture is true only when one was actually carved (the
        // summary carries it), false when the ask disabled it.
        assert!(!CorpusContribution::from_pass(0, &summary(0, false)).gesture);
        assert!(CorpusContribution::from_pass(0, &summary(0, true)).gesture);
    }

    #[test]
    fn clear_selection_drops_the_pointer_but_keeps_the_record() {
        let mut h = SessionHistory::new();
        let run = h.begin_run();
        let a = h.record(
            run,
            "a#1".to_owned(),
            "A".to_owned(),
            score(),
            generate_gen(),
        );
        h.set_verdict(a, Verdict::Favorite);
        h.select(a);
        h.clear_selection();
        assert_eq!(h.selected(), None, "nothing is selected after a clear");
        assert_eq!(h.entries().len(), 1, "the entry survives");
        assert_eq!(
            h.get(a).unwrap().verdict,
            Some(Verdict::Favorite),
            "and its verdict is kept",
        );
    }
}
