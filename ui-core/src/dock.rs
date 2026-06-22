//! Corpus dock (ADR-0027 Slice 5): pure browse/filter + aggregate over a corpus.
//!
//! The "rule the corpus from the web" surface, over a set of captured
//! [`ChunkMeta`]s. Renderer-agnostic and headless: the egui cockpit's corpus
//! dock draws what these functions compute, and the CLI could reuse them, so the
//! filter and dashboard semantics cannot diverge between frontends (ADR-0016).
//! No I/O — the caller hands in the chunks (read from the OPFS tree on web).

use griff_core::corpus::{ChunkMeta, RightsStatus, StyleCohort, SwancoreTag};

/// A browse filter over the corpus — every field an optional facet.
///
/// `None`, `false`, or an empty query matches everything, so
/// [`CorpusFilter::default`] passes the whole corpus through; facets AND together.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CorpusFilter {
    /// Keep only this style cohort (core ↔ adjacent).
    pub cohort: Option<StyleCohort>,
    /// Keep only chunks whose rights carry this status.
    pub rights: Option<RightsStatus>,
    /// Keep only redistributable chunks — the export gate (ADR-0027 §4).
    pub redistributable_only: bool,
    /// Keep only chunks carrying this tag (the class/tag axis).
    pub tag: Option<SwancoreTag>,
    /// Keep only near-duplicate-flagged chunks ([`ChunkMeta::duplicate`]).
    pub duplicates_only: bool,
    /// Case-insensitive substring over id + title; empty matches all.
    pub query: String,
}

impl CorpusFilter {
    /// Whether `chunk` passes every active facet.
    #[must_use]
    pub fn matches(&self, chunk: &ChunkMeta) -> bool {
        if let Some(cohort) = self.cohort {
            if chunk.style_cohort != Some(cohort) {
                return false;
            }
        }
        if let Some(status) = self.rights {
            if chunk.rights.as_ref().map(|r| r.rights_status) != Some(status) {
                return false;
            }
        }
        if self.redistributable_only && !chunk.rights.as_ref().is_some_and(|r| r.redistributable) {
            return false;
        }
        if let Some(tag) = self.tag {
            if !chunk.tags.contains(&tag) {
                return false;
            }
        }
        if self.duplicates_only && chunk.duplicate.is_none() {
            return false;
        }
        if !self.query.is_empty() {
            let q = self.query.to_lowercase();
            if !chunk.id.0.to_lowercase().contains(&q) && !chunk.title.to_lowercase().contains(&q) {
                return false;
            }
        }
        true
    }
}

/// Filters `chunks` by `filter`, preserving order and borrowing (no clone).
#[must_use]
pub fn filter_chunks<'a>(chunks: &'a [ChunkMeta], filter: &CorpusFilter) -> Vec<&'a ChunkMeta> {
    chunks.iter().filter(|c| filter.matches(c)).collect()
}

/// Corpus-aggregate dashboard: counts segmented along the dock's facets.
///
/// Computed over whatever slice is passed — the full corpus or a filtered view.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CorpusStats {
    /// Total chunks counted.
    pub total: usize,
    /// Redistributable chunks — the export-eligible subset (ADR-0027 §4).
    pub redistributable: usize,
    /// Chunks with no `rights` recorded yet.
    pub rights_unset: usize,
    /// Chunks flagged as near-duplicates ([`ChunkMeta::duplicate`]).
    pub duplicates: usize,
    /// Count per style cohort, indexed by [`cohort_index`] (Core, Adjacent).
    pub by_cohort: [usize; 2],
    /// Count per rights status, indexed by [`rights_index`] (declaration order).
    pub by_rights: [usize; 5],
    /// Count per [`SwancoreTag`], in `all_variants` order. A chunk adds to each
    /// tag it carries, so these sum to ≥ `total`; pairs are kept for stable
    /// display even at zero.
    pub by_tag: Vec<(SwancoreTag, usize)>,
}

impl CorpusStats {
    /// Aggregates `chunks` into the dashboard counts.
    #[must_use]
    pub fn aggregate(chunks: &[ChunkMeta]) -> Self {
        let variants = SwancoreTag::all_variants();
        let mut tag_counts = vec![0usize; variants.len()];
        let mut stats = Self {
            total: chunks.len(),
            ..Self::default()
        };
        for chunk in chunks {
            if let Some(slot) = chunk
                .style_cohort
                .and_then(|c| stats.by_cohort.get_mut(cohort_index(c)))
            {
                *slot = slot.saturating_add(1);
            }
            match &chunk.rights {
                Some(rights) => {
                    if let Some(slot) = stats.by_rights.get_mut(rights_index(rights.rights_status))
                    {
                        *slot = slot.saturating_add(1);
                    }
                    if rights.redistributable {
                        stats.redistributable = stats.redistributable.saturating_add(1);
                    }
                }
                None => stats.rights_unset = stats.rights_unset.saturating_add(1),
            }
            if chunk.duplicate.is_some() {
                stats.duplicates = stats.duplicates.saturating_add(1);
            }
            for (count, tag) in tag_counts.iter_mut().zip(variants) {
                if chunk.tags.contains(tag) {
                    *count = count.saturating_add(1);
                }
            }
        }
        stats.by_tag = variants.iter().copied().zip(tag_counts).collect();
        stats
    }

    /// The tags actually present (non-zero count), most-common first — the
    /// dashboard's "by class/tag" view without the long zero tail.
    #[must_use]
    pub fn present_tags(&self) -> Vec<(SwancoreTag, usize)> {
        let mut present: Vec<_> = self
            .by_tag
            .iter()
            .copied()
            .filter(|&(_, n)| n > 0)
            .collect();
        present.sort_by(|a, b| b.1.cmp(&a.1));
        present
    }
}

/// Stable index of a [`StyleCohort`] in `by_cohort` (declaration order).
#[must_use]
pub const fn cohort_index(cohort: StyleCohort) -> usize {
    match cohort {
        StyleCohort::Core => 0,
        StyleCohort::Adjacent => 1,
    }
}

/// Stable index of a [`RightsStatus`] in `by_rights` (declaration order).
#[must_use]
pub const fn rights_index(status: RightsStatus) -> usize {
    match status {
        RightsStatus::PublicDomain => 0,
        RightsStatus::CcBy => 1,
        RightsStatus::CcBySa => 2,
        RightsStatus::CopyrightedComposition => 3,
        RightsStatus::Unknown => 4,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::indexing_slicing)]
    use super::*;
    use griff_core::corpus::{Acquisition, ChunkId, RightsInfo, SourceFormat, SourceRef};
    use griff_core::novelty::PhraseDuplicate;

    /// A minimal chunk with only the dock-relevant facets set; everything else
    /// takes a neutral default.
    fn chunk(
        id: &str,
        cohort: Option<StyleCohort>,
        rights: Option<(RightsStatus, bool)>,
        tags: &[SwancoreTag],
        duplicate: bool,
    ) -> ChunkMeta {
        ChunkMeta {
            id: ChunkId(id.to_owned()),
            title: format!("Title {id}"),
            source: SourceRef {
                filename: "x.mid".to_owned(),
                format: SourceFormat::Midi,
                bar_range: None,
            },
            tempo_bpm: 120.0,
            ticks_per_quarter: 480,
            time_signature: (4, 4),
            tuning: "standard_e".to_owned(),
            tags: tags.to_vec(),
            boundaries: vec![],
            techniques: vec![],
            quality_flags: vec![],
            reviewer: None,
            structure: None,
            gesture: None,
            complexity: None,
            duplicate: duplicate.then_some(PhraseDuplicate {
                of: 0,
                quote_share: 0.95,
            }),
            style_cohort: cohort,
            ensemble: None,
            rights: rights.map(|(rights_status, redistributable)| RightsInfo {
                rights_status,
                acquisition: Acquisition::CommunityTabSite,
                redistributable,
                notes: String::new(),
            }),
            created_at: "t".to_owned(),
            updated_at: "t".to_owned(),
        }
    }

    fn sample() -> Vec<ChunkMeta> {
        vec![
            chunk(
                "alpha_riff",
                Some(StyleCohort::Core),
                Some((RightsStatus::PublicDomain, true)),
                &[SwancoreTag::CleanRiff, SwancoreTag::PalmMute],
                false,
            ),
            chunk(
                "beta_breakdown",
                Some(StyleCohort::Core),
                Some((RightsStatus::CopyrightedComposition, false)),
                &[SwancoreTag::PalmMute],
                true,
            ),
            chunk(
                "gamma_lead",
                Some(StyleCohort::Adjacent),
                Some((RightsStatus::CcBy, true)),
                &[SwancoreTag::LegatoPassage],
                false,
            ),
            chunk("delta_unset", None, None, &[], false),
        ]
    }

    #[test]
    fn default_filter_passes_everything() {
        let chunks = sample();
        assert_eq!(filter_chunks(&chunks, &CorpusFilter::default()).len(), 4);
    }

    #[test]
    fn facets_combine_with_and() {
        let chunks = sample();
        let core_redist = CorpusFilter {
            cohort: Some(StyleCohort::Core),
            redistributable_only: true,
            ..CorpusFilter::default()
        };
        let kept = filter_chunks(&chunks, &core_redist);
        assert_eq!(kept.len(), 1, "only the public-domain core riff is both");
        assert_eq!(kept[0].id.0, "alpha_riff");
    }

    #[test]
    fn tag_rights_duplicate_and_query_facets() {
        let chunks = sample();
        let by_tag = CorpusFilter {
            tag: Some(SwancoreTag::PalmMute),
            ..Default::default()
        };
        assert_eq!(
            filter_chunks(&chunks, &by_tag).len(),
            2,
            "two carry palm_mute"
        );

        let by_rights = CorpusFilter {
            rights: Some(RightsStatus::CcBy),
            ..Default::default()
        };
        assert_eq!(filter_chunks(&chunks, &by_rights).len(), 1);

        let dups = CorpusFilter {
            duplicates_only: true,
            ..Default::default()
        };
        let kept = filter_chunks(&chunks, &dups);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id.0, "beta_breakdown");

        let q = CorpusFilter {
            query: "LEAD".to_owned(),
            ..Default::default()
        };
        assert_eq!(
            filter_chunks(&chunks, &q)[0].id.0,
            "gamma_lead",
            "query is case-insensitive"
        );
    }

    #[test]
    fn aggregate_segments_the_corpus() {
        let stats = CorpusStats::aggregate(&sample());
        assert_eq!(stats.total, 4);
        assert_eq!(stats.redistributable, 2, "alpha + gamma");
        assert_eq!(stats.rights_unset, 1, "delta has no rights");
        assert_eq!(stats.duplicates, 1, "beta is a near-dup");
        assert_eq!(stats.by_cohort[cohort_index(StyleCohort::Core)], 2);
        assert_eq!(stats.by_cohort[cohort_index(StyleCohort::Adjacent)], 1);
        assert_eq!(stats.by_rights[rights_index(RightsStatus::PublicDomain)], 1);
        assert_eq!(
            stats.by_rights[rights_index(RightsStatus::CopyrightedComposition)],
            1
        );
    }

    #[test]
    fn present_tags_drops_zeros_and_sorts_by_count() {
        let stats = CorpusStats::aggregate(&sample());
        let present = stats.present_tags();
        assert_eq!(
            present[0],
            (SwancoreTag::PalmMute, 2),
            "palm_mute leads with 2"
        );
        assert!(
            present.iter().all(|&(_, n)| n > 0),
            "no zero-count tags in the present view"
        );
        assert!(
            present.iter().any(|&(t, _)| t == SwancoreTag::CleanRiff),
            "clean_riff is present once"
        );
    }

    #[test]
    fn aggregate_of_empty_corpus_is_zeroed() {
        let stats = CorpusStats::aggregate(&[]);
        assert_eq!(stats.total, 0);
        assert!(stats.present_tags().is_empty());
    }
}
