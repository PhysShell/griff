//! Candidate-set presentation (S8): the ranked set a frontend browses.
//!
//! Generation itself is **not** implemented here — it is
//! [`griff_core::generation_input::ranked_candidates`], the same entry point
//! `griff generate` uses, so a cockpit set and a CLI set are the same set. This
//! module only turns that result into rows a table can paint: rank, strategy,
//! variant seed, the aggregate and the six rerank axes behind it, and a stable
//! id to hang provenance (and, later, S9 feedback) on.
//!
//! Pure and wasm-safe: the caller hands in an already-imported source score and
//! already-parsed corpus material, and owns its own I/O.

use griff_core::generation_input::{
    ranked_candidates, CorpusMaterial, GenerationAsk, GenerationInputError, RankedSet,
};
use griff_core::rerank::RERANK_AXIS_LABELS;
use griff_core::score::{AtomEvent, Score};

/// One candidate, as a table row.
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateRow {
    /// 1-based rank in the reranked set (1 is the winner `griff generate`
    /// would have written).
    pub rank: usize,
    /// Stable within a set and reproducible across runs of the same ask:
    /// `<strategy>#<variant-seed hex>`. Rerunning the same ask reproduces this
    /// candidate exactly.
    pub id: String,
    /// The strategy that produced it.
    pub strategy: String,
    /// The derived variant seed it ran under — its reproduction key.
    pub variant_seed: u64,
    /// Weighted aggregate over the six rerank axes.
    pub aggregate: f64,
    /// Each rerank axis and its value, in [`RERANK_AXIS_LABELS`] order.
    pub axes: Vec<(&'static str, f64)>,
    /// Notes in the candidate's first track — a coarse density read.
    pub note_count: usize,
    /// Lowest and highest sounding pitch, `None` when silent.
    pub pitch_range: Option<(u8, u8)>,
}

/// What the pass was given — shown above the table so a curator can see whether
/// the corpus actually contributed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetSummary {
    /// Rhythm templates the pass rotated.
    pub templates: usize,
    /// Novelty reference chunks.
    pub references: usize,
    /// The gesture ask, when the pass carved one.
    pub gesture: Option<(usize, String)>,
    /// Distinct scale tones the source supplied (12 means the source's tracks
    /// union to the full chromatic — see S15).
    pub scale_tones: usize,
    /// Corpus records that could not be loaded (never silently dropped).
    pub skipped: Vec<String>,
}

/// A generated, reranked, browsable set.
#[derive(Debug, Clone)]
pub struct CandidateSet {
    /// Rows in rank order.
    pub rows: Vec<CandidateRow>,
    /// The candidate scores, parallel to [`Self::rows`] — index `i` is the
    /// score for `rows[i]`.
    pub scores: Vec<Score>,
    /// What the pass was given.
    pub summary: SetSummary,
}

impl CandidateSet {
    /// Presents an already-generated [`RankedSet`] as browsable rows.
    ///
    /// The one mapping every frontend uses — `generate_set` for the corpus
    /// path, the Swang evaluator's result for the authoring path — so a
    /// candidate table is the same table wherever the set came from. The
    /// `material` supplies only the reference count and skipped-record list,
    /// which live outside the ranked set.
    #[must_use]
    pub fn from_ranked(set: &RankedSet, material: Option<&CorpusMaterial>) -> Self {
        let rows = set
            .ranked
            .iter()
            .enumerate()
            .map(|(i, scored)| {
                let variant_seed = scored.value.seed.0;
                let strategy = format!("{:?}", scored.value.strategy);
                CandidateRow {
                    rank: i.saturating_add(1),
                    id: format!("{strategy}#{variant_seed:016x}"),
                    strategy,
                    variant_seed,
                    aggregate: scored.aggregate(),
                    axes: RERANK_AXIS_LABELS
                        .iter()
                        .map(|&label| (label, scored.axes.get(label).unwrap_or(0.0)))
                        .collect(),
                    note_count: note_count(&scored.value.score),
                    pitch_range: pitch_range(&scored.value.score),
                }
            })
            .collect();
        let scores = set.ranked.iter().map(|s| s.value.score.clone()).collect();

        Self {
            rows,
            scores,
            summary: SetSummary {
                templates: set.source_rhythms.len(),
                references: material.map_or(0, |m| m.references.len()),
                gesture: set
                    .gesture
                    .map(|g| (g.burst_notes, format!("{:.1}q", g.rest_quarters))),
                scale_tones: set.base.pitch_material.intervals.len(),
                skipped: material.map_or_else(Vec::new, |m| m.skipped.clone()),
            },
        }
    }
}

/// Generates and reranks a candidate set, then presents it as rows.
///
/// A thin adapter over [`ranked_candidates`] — it adds no musical decision of
/// its own.
///
/// # Errors
/// Whatever the shared compiler rejects: a source that cannot seed a request,
/// or a candidate-set builder rejection (e.g. a zero variant count).
pub fn generate_set(
    source: &Score,
    material: Option<&CorpusMaterial>,
    ask: &GenerationAsk,
) -> Result<CandidateSet, GenerationInputError> {
    let set = ranked_candidates(source, material, ask, None)?;
    Ok(CandidateSet::from_ranked(&set, material))
}

/// Notes across a score's first track.
fn note_count(score: &Score) -> usize {
    notes(score).count()
}

/// Lowest and highest sounding pitch of a score's first track.
fn pitch_range(score: &Score) -> Option<(u8, u8)> {
    let lo = notes(score).min()?;
    let hi = notes(score).max()?;
    Some((lo, hi))
}

/// Every note pitch in the score's first track, in event order.
fn notes(score: &Score) -> impl Iterator<Item = u8> + '_ {
    score
        .tracks
        .first()
        .into_iter()
        .flat_map(|t| &t.voices)
        .flat_map(|v| &v.event_groups)
        .flat_map(|g| &g.atoms)
        .filter_map(|a| match a {
            AtomEvent::Note(n) => Some(n.pitch.0),
            AtomEvent::Rest(_) => None,
        })
}

#[cfg(test)]
mod tests {
    // Fixture arithmetic runs over a fixed `0..4`, so it cannot overflow.
    #![allow(
        clippy::arithmetic_side_effects,
        clippy::expect_used,
        clippy::indexing_slicing
    )]

    use super::*;
    use griff_core::event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity};
    use griff_core::score::{
        AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, RepeatMarker, Track, Voice,
    };
    use griff_core::slice::TickRange;

    const PPQN: u16 = 480;
    const BAR: u32 = 1920;

    /// One bar of four ascending quarter notes — enough pitch material and
    /// rhythm to seed a request.
    fn source() -> Score {
        let event_groups = (0..4u32)
            .map(|i| EventGroup {
                kind: EventGroupKind::Single,
                atoms: vec![AtomEvent::Note(AtomNote {
                    absolute_start: Ticks(i * u32::from(PPQN)),
                    duration: Ticks(u32::from(PPQN)),
                    pitch: Pitch::new(60 + u8::try_from(i).expect("0..4")).expect("valid pitch"),
                    velocity: Velocity::new(96).expect("valid velocity"),
                    marks: NoteMarks::empty(),
                    position: None,
                })],
                technique_spans: Vec::new(),
            })
            .collect();
        Score {
            ticks_per_quarter: PPQN,
            master_bars: vec![MasterBar {
                index: 0,
                tick_range: TickRange::new(Ticks(0), Ticks(BAR)).expect("ordered"),
                time_signature: TimeSignature::new(4, 4).expect("4/4"),
                tempo: Tempo::new(120.0).expect("120 BPM"),
                repeat: RepeatMarker::default(),
            }],
            tracks: vec![Track {
                name: Some("guitar".to_owned()),
                channel: 0,
                voices: vec![Voice {
                    id: 0,
                    event_groups,
                }],
                tuning: Tuning::standard_e(),
            }],
            source_meta: None,
            loss: LossReport::new(),
        }
    }

    fn ask() -> GenerationAsk {
        GenerationAsk {
            seed: 7,
            bars: 4,
            variants_per_strategy: 2,
            gesture: false,
        }
    }

    #[test]
    fn rows_are_rank_ordered_and_parallel_to_scores() {
        let set = generate_set(&source(), None, &ask()).expect("a four-note bar seeds a request");
        assert_eq!(set.rows.len(), set.scores.len(), "rows index into scores");
        assert!(!set.rows.is_empty(), "five strategies x 2 variants");
        for (i, row) in set.rows.iter().enumerate() {
            assert_eq!(row.rank, i + 1, "rank is 1-based position");
        }
        for pair in set.rows.windows(2) {
            assert!(
                pair[0].aggregate >= pair[1].aggregate,
                "aggregate descends: {} then {}",
                pair[0].aggregate,
                pair[1].aggregate,
            );
        }
    }

    #[test]
    fn the_same_ask_reproduces_the_same_set() {
        let (a, b) = (
            generate_set(&source(), None, &ask()).expect("seeds"),
            generate_set(&source(), None, &ask()).expect("seeds"),
        );
        assert_eq!(a.rows, b.rows, "generation is deterministic under a seed");
    }

    #[test]
    fn a_candidate_id_carries_its_reproduction_key() {
        let set = generate_set(&source(), None, &ask()).expect("seeds");
        let row = &set.rows[0];
        assert!(
            row.id.starts_with(&row.strategy) && row.id.contains('#'),
            "id names the strategy: {}",
            row.id,
        );
        assert!(
            row.id.ends_with(&format!("{:016x}", row.variant_seed)),
            "id carries the variant seed that reproduces it: {}",
            row.id,
        );
    }

    #[test]
    fn every_rerank_axis_is_surfaced() {
        let set = generate_set(&source(), None, &ask()).expect("seeds");
        let labels: Vec<&str> = set.rows[0].axes.iter().map(|&(l, _)| l).collect();
        assert_eq!(
            labels,
            RERANK_AXIS_LABELS.to_vec(),
            "all six axes, in canonical order",
        );
    }

    #[test]
    fn a_zero_variant_ask_is_rejected_not_silently_empty() {
        let ask = GenerationAsk {
            variants_per_strategy: 0,
            ..ask()
        };
        assert!(
            generate_set(&source(), None, &ask).is_err(),
            "an empty set is an error, never an empty table",
        );
    }
}
