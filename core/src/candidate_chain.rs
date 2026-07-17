//! Multi-bar candidate chain over a ranked set (S7 Slice B).
//!
//! The first real client of the layered-path engine ([`crate::layered_path`]).
//! It takes a [`RankedSet`] the S6 pass already produced and asks a different
//! question: not "which candidate is best?" but "which *bar* of which candidate
//! should follow which?".
//!
//! Layer `b` holds bar `b` of every ranked candidate, so a path picks one
//! candidate per output bar and the DP optimises the whole sequence — the fix
//! ADR-0013 names for four locally-fine bars that are globally dull.
//!
//! Nothing is regenerated, reranked or re-seeded here. Every bar is a snapshot
//! of a score already in the set, and every candidate's original S6 axes,
//! rationale, and rerank provenance travel into the result unchanged.

use crate::generate::{GenerationSeed, GenerationStrategy};
use crate::layered_path::PathError;
use crate::rerank::SetCandidate;
use crate::score::Score;
use crate::scoring::{Axes, Provenance, Rationale, Scored};

/// One state of the chain: bar `bar` as supplied by ranked candidate
/// `candidate`.
///
/// The identity is immutable and independent of any UI: it names the bar, the
/// candidate's ordinal in the ranked set, and the candidate's own generation
/// identity (strategy, variant seed, original rank). No title, filename, or
/// application state takes part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChainState {
    /// The output bar this state fills, `0..bars`.
    pub bar: usize,
    /// The supplying candidate's ordinal in the ranked set (`0` is rank 1).
    pub candidate: usize,
    /// The strategy that generated the supplying candidate.
    pub strategy: GenerationStrategy,
    /// The derived variant seed the supplying candidate ran under.
    pub variant_seed: GenerationSeed,
    /// The supplying candidate's original 1-based rank in the ranked set.
    pub rank: usize,
}

/// Why a ranked set could not be planned as a chain.
///
/// Every variant names the first offending fact. The planner never truncates to
/// the shortest candidate, and never borrows meter or tempo from whichever
/// candidate happens to win a layer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChainError {
    /// The ranked set held no candidates.
    EmptySet,
    /// The candidates have no bars to chain.
    NoBars,
    /// A candidate's bar count differs from the first candidate's.
    BarCountMismatch {
        /// The offending candidate's ordinal.
        candidate: usize,
        /// The bar count the first candidate set.
        expected: usize,
        /// The offending candidate's bar count.
        found: usize,
    },
    /// A candidate's tick resolution differs from the first candidate's.
    PpqMismatch {
        /// The offending candidate's ordinal.
        candidate: usize,
        /// The resolution the first candidate set.
        expected: u16,
        /// The offending candidate's resolution.
        found: u16,
    },
    /// A candidate's bar grid — tick range, meter, or tempo — differs from the
    /// first candidate's, so the master timelines cannot be one timeline.
    BarGridMismatch {
        /// The offending candidate's ordinal.
        candidate: usize,
        /// The bar whose grid differs.
        bar: usize,
    },
    /// A candidate's track/voice shape differs from the first candidate's, so
    /// bars from the two cannot be assembled into one part.
    TrackShapeMismatch {
        /// The offending candidate's ordinal.
        candidate: usize,
    },
    /// A candidate carries material crossing a bar line — a note sounding past
    /// its bar's end, or a technique span straddling bars.
    ///
    /// The slicing contract this v1 reuses cuts by onset and clamps spans, so it
    /// cannot concatenate such material losslessly. Rather than clip, shorten,
    /// or silently drop a musical event, the chain refuses the set.
    CrossBarMaterial {
        /// The offending candidate's ordinal.
        candidate: usize,
        /// The bar the material starts in.
        bar: usize,
    },
    /// The layered engine rejected the problem the chain handed it.
    Path(PathError),
}

impl From<PathError> for ChainError {
    fn from(error: PathError) -> Self {
        Self::Path(error)
    }
}

/// The candidate's S6 rerank score, carried through unchanged.
///
/// The chain re-uses S6's verdict; it never reranks. Keeping the six axes, the
/// rationale, and the rerank policy's provenance means a chain step can still
/// answer "why was this candidate good?" in S6's own words.
#[derive(Debug, Clone)]
pub struct S6Quality {
    /// The six rerank axes, in `RERANK_AXIS_LABELS` order.
    pub axes: Axes,
    /// The rerank rationale.
    pub rationale: Rationale,
    /// The rerank policy's provenance (id, version, seed).
    pub provenance: Provenance,
    /// The derived rerank aggregate the local cost is monotonic in.
    pub aggregate: f64,
}

/// The layers of a chain problem: `layers[b][c]` is bar `b` of candidate `c`.
///
/// # Errors
/// [`ChainError`] on the first fact that makes the set unchainable: an empty
/// set, no bars, a mismatched bar count / resolution / bar grid / track shape,
/// or material crossing a bar line.
pub fn chain_layers(ranked: &[Scored<SetCandidate>]) -> Result<Vec<Vec<ChainState>>, ChainError> {
    let first = ranked.first().ok_or(ChainError::EmptySet)?;
    let reference = &first.value.score;
    let bars = reference.master_bars.len();
    if bars == 0 {
        return Err(ChainError::NoBars);
    }

    for (candidate, scored) in ranked.iter().enumerate() {
        check_compatible(candidate, reference, &scored.value.score)?;
        check_self_contained_bars(candidate, &scored.value.score)?;
    }

    Ok((0..bars)
        .map(|bar| {
            ranked
                .iter()
                .enumerate()
                .map(|(candidate, scored)| ChainState {
                    bar,
                    candidate,
                    strategy: scored.value.strategy,
                    variant_seed: scored.value.seed,
                    rank: candidate.saturating_add(1),
                })
                .collect()
        })
        .collect())
}

/// Rejects a candidate whose shape cannot share one timeline with `reference`.
fn check_compatible(candidate: usize, reference: &Score, score: &Score) -> Result<(), ChainError> {
    if score.master_bars.len() != reference.master_bars.len() {
        return Err(ChainError::BarCountMismatch {
            candidate,
            expected: reference.master_bars.len(),
            found: score.master_bars.len(),
        });
    }
    if score.ticks_per_quarter != reference.ticks_per_quarter {
        return Err(ChainError::PpqMismatch {
            candidate,
            expected: reference.ticks_per_quarter,
            found: score.ticks_per_quarter,
        });
    }
    // One master timeline, agreed by every candidate — not borrowed from
    // whichever happens to win a layer.
    for (bar, (want, got)) in reference
        .master_bars
        .iter()
        .zip(score.master_bars.iter())
        .enumerate()
    {
        let same = want.tick_range == got.tick_range
            && want.time_signature == got.time_signature
            && want.tempo.0.to_bits() == got.tempo.0.to_bits();
        if !same {
            return Err(ChainError::BarGridMismatch { candidate, bar });
        }
    }
    let shape = |s: &Score| -> Vec<Vec<u8>> {
        s.tracks
            .iter()
            .map(|t| t.voices.iter().map(|v| v.id).collect())
            .collect()
    };
    if shape(score) != shape(reference) {
        return Err(ChainError::TrackShapeMismatch { candidate });
    }
    Ok(())
}

/// Rejects a candidate carrying material across a bar line.
///
/// A bar may only be lifted out and set beside a bar from another candidate if
/// it is self-contained: every note ends within the bar it starts in, and no
/// technique span straddles a bar line.
fn check_self_contained_bars(candidate: usize, score: &Score) -> Result<(), ChainError> {
    for group in score
        .tracks
        .iter()
        .flat_map(|t| t.voices.iter())
        .flat_map(|v| v.event_groups.iter())
    {
        for atom in &group.atoms {
            let start = atom.absolute_start().0;
            let end = start.saturating_add(atom.duration().0);
            let bar =
                bar_of(score, start).ok_or(ChainError::CrossBarMaterial { candidate, bar: 0 })?;
            let bar_end = score
                .master_bars
                .get(bar)
                .map_or(start, |b| b.tick_range.end.0);
            if end > bar_end {
                return Err(ChainError::CrossBarMaterial { candidate, bar });
            }
        }
        for span in &group.technique_spans {
            let start = span.tick_range.start.0;
            let bar =
                bar_of(score, start).ok_or(ChainError::CrossBarMaterial { candidate, bar: 0 })?;
            let bar_end = score
                .master_bars
                .get(bar)
                .map_or(start, |b| b.tick_range.end.0);
            if span.tick_range.end.0 > bar_end {
                return Err(ChainError::CrossBarMaterial { candidate, bar });
            }
        }
    }
    Ok(())
}

/// The index of the bar containing `tick`, if any.
fn bar_of(score: &Score, tick: u32) -> Option<usize> {
    score
        .master_bars
        .iter()
        .position(|b| tick >= b.tick_range.start.0 && tick < b.tick_range.end.0)
}

#[cfg(test)]
#[allow(
    clippy::missing_assert_message,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::{chain_layers, ChainError};
    use crate::event::{
        NoteMarks, Pitch, SpanTechnique, TechniqueEvidence, Tempo, Ticks, TimeSignature, Tuning,
        Velocity,
    };
    use crate::generate::{GenerationSeed, GenerationStrategy};
    use crate::rerank::SetCandidate;
    use crate::score::{
        AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, RepeatMarker,
        Score, TechniqueSpan, Track, Voice,
    };
    use crate::scoring::{Axes, Axis, Scored, WeightPolicy};
    use crate::slice::TickRange;

    /// Ticks per quarter for every fixture; a 4/4 bar is four of them.
    const PPQ: u16 = 960;
    /// One 4/4 bar at [`PPQ`].
    const BAR: u32 = 3840;

    /// A note as `(offset within its bar, duration, pitch)`.
    type Note = (u32, u32, u8);

    /// Builds a score whose bar `i` holds `bars[i]`'s notes, on one 4/4 track.
    fn score_of(bars: &[&[Note]]) -> Score {
        let mut master_bars = Vec::new();
        let mut event_groups = Vec::new();
        for (index, notes) in bars.iter().enumerate() {
            let start = u32::try_from(index).unwrap() * BAR;
            master_bars.push(MasterBar {
                index,
                tick_range: TickRange::new(Ticks(start), Ticks(start + BAR)).unwrap(),
                time_signature: TimeSignature::new(4, 4).unwrap(),
                tempo: Tempo::new(120.0).unwrap(),
                repeat: RepeatMarker::default(),
            });
            for &(offset, duration, pitch) in *notes {
                event_groups.push(EventGroup {
                    kind: EventGroupKind::Single,
                    atoms: vec![AtomEvent::Note(AtomNote {
                        absolute_start: Ticks(start + offset),
                        duration: Ticks(duration),
                        pitch: Pitch(pitch),
                        velocity: Velocity(96),
                        marks: NoteMarks::empty(),
                        position: None,
                    })],
                    technique_spans: Vec::new(),
                });
            }
        }
        Score {
            ticks_per_quarter: PPQ,
            master_bars,
            tracks: vec![Track {
                name: None,
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

    /// A test rerank policy over one axis, so an aggregate is easy to dial in.
    fn s6_policy() -> WeightPolicy {
        WeightPolicy::new("test_rerank", 1, vec![("quality", 1.0)])
    }

    /// A ranked candidate with the given score and S6 aggregate.
    fn candidate(score: Score, quality: f64, seed: u64) -> Scored<SetCandidate> {
        Scored::new(
            SetCandidate {
                score,
                strategy: GenerationStrategy::ShuffleMotifs,
                seed: GenerationSeed(seed),
                gesture: None,
            },
            Axes::new(vec![Axis {
                label: "quality",
                value: quality,
            }]),
            &s6_policy(),
            Some(seed),
        )
    }

    #[test]
    fn the_layer_count_equals_the_generated_bar_count() {
        let ranked = vec![
            candidate(score_of(&[&[(0, 480, 60)], &[(0, 480, 62)]]), 0.9, 1),
            candidate(score_of(&[&[(0, 480, 64)], &[(0, 480, 65)]]), 0.5, 2),
        ];
        let layers = chain_layers(&ranked).expect("chainable");
        assert_eq!(layers.len(), 2, "one layer per generated bar");
    }

    #[test]
    fn each_layer_holds_one_state_per_ranked_candidate() {
        let ranked = vec![
            candidate(score_of(&[&[(0, 480, 60)]]), 0.9, 1),
            candidate(score_of(&[&[(0, 480, 64)]]), 0.5, 2),
            candidate(score_of(&[&[(0, 480, 67)]]), 0.1, 3),
        ];
        let layers = chain_layers(&ranked).expect("chainable");
        assert_eq!(layers[0].len(), 3, "one state per candidate");
        assert_eq!(
            layers[0].iter().map(|s| s.candidate).collect::<Vec<_>>(),
            vec![0, 1, 2],
            "states keep the ranked order",
        );
    }

    #[test]
    fn each_state_retains_its_candidates_generation_identity() {
        let ranked = vec![
            candidate(score_of(&[&[(0, 480, 60)]]), 0.9, 7),
            candidate(score_of(&[&[(0, 480, 64)]]), 0.5, 11),
        ];
        let layers = chain_layers(&ranked).expect("chainable");
        let second = layers[0][1];
        assert_eq!(second.bar, 0);
        assert_eq!(second.candidate, 1);
        assert_eq!(second.rank, 2, "the original 1-based rank");
        assert_eq!(second.variant_seed, GenerationSeed(11));
        assert_eq!(second.strategy, GenerationStrategy::ShuffleMotifs);
    }

    #[test]
    fn an_empty_ranked_set_is_rejected() {
        assert_eq!(chain_layers(&[]).unwrap_err(), ChainError::EmptySet);
    }

    #[test]
    fn a_barless_candidate_set_is_rejected() {
        let ranked = vec![candidate(score_of(&[]), 0.9, 1)];
        assert_eq!(chain_layers(&ranked).unwrap_err(), ChainError::NoBars);
    }

    #[test]
    fn a_short_candidate_is_rejected_rather_than_truncating_the_chain() {
        let ranked = vec![
            candidate(score_of(&[&[(0, 480, 60)], &[(0, 480, 62)]]), 0.9, 1),
            candidate(score_of(&[&[(0, 480, 64)]]), 0.5, 2),
        ];
        assert_eq!(
            chain_layers(&ranked).unwrap_err(),
            ChainError::BarCountMismatch {
                candidate: 1,
                expected: 2,
                found: 1,
            },
        );
    }

    #[test]
    fn a_mismatched_resolution_is_rejected() {
        let mut odd = score_of(&[&[(0, 480, 64)]]);
        odd.ticks_per_quarter = 480;
        let ranked = vec![
            candidate(score_of(&[&[(0, 480, 60)]]), 0.9, 1),
            candidate(odd, 0.5, 2),
        ];
        assert_eq!(
            chain_layers(&ranked).unwrap_err(),
            ChainError::PpqMismatch {
                candidate: 1,
                expected: 960,
                found: 480,
            },
        );
    }

    #[test]
    fn a_mismatched_bar_grid_is_rejected() {
        let mut odd = score_of(&[&[(0, 480, 60)], &[(0, 480, 62)]]);
        odd.master_bars[1].tempo = Tempo::new(180.0).unwrap();
        let ranked = vec![
            candidate(score_of(&[&[(0, 480, 60)], &[(0, 480, 62)]]), 0.9, 1),
            candidate(odd, 0.5, 2),
        ];
        assert_eq!(
            chain_layers(&ranked).unwrap_err(),
            ChainError::BarGridMismatch {
                candidate: 1,
                bar: 1,
            },
        );
    }

    #[test]
    fn a_mismatched_track_shape_is_rejected() {
        let mut odd = score_of(&[&[(0, 480, 60)]]);
        odd.tracks.push(odd.tracks[0].clone());
        let ranked = vec![
            candidate(score_of(&[&[(0, 480, 60)]]), 0.9, 1),
            candidate(odd, 0.5, 2),
        ];
        assert_eq!(
            chain_layers(&ranked).unwrap_err(),
            ChainError::TrackShapeMismatch { candidate: 1 },
        );
    }

    #[test]
    fn a_note_sounding_past_its_bar_is_rejected_not_clipped() {
        // The note starts in bar 0 and rings 480 ticks into bar 1. The slicing
        // contract cuts by onset and would carry it whole into a bar chosen from
        // another candidate — so the chain refuses instead of clipping it.
        let ranked = vec![
            candidate(score_of(&[&[(0, 480, 60)], &[(0, 480, 62)]]), 0.9, 1),
            candidate(
                score_of(&[&[(BAR - 240, 720, 64)], &[(0, 480, 65)]]),
                0.5,
                2,
            ),
        ];
        assert_eq!(
            chain_layers(&ranked).unwrap_err(),
            ChainError::CrossBarMaterial {
                candidate: 1,
                bar: 0,
            },
        );
    }

    #[test]
    fn a_technique_span_straddling_a_bar_line_is_rejected() {
        let mut odd = score_of(&[&[(0, 480, 60)], &[(0, 480, 62)]]);
        odd.tracks[0].voices[0].event_groups[0]
            .technique_spans
            .push(TechniqueSpan {
                technique: SpanTechnique::PalmMute,
                tick_range: TickRange::new(Ticks(0), Ticks(BAR + 240)).unwrap(),
                evidence: TechniqueEvidence::explicit(),
            });
        let ranked = vec![
            candidate(score_of(&[&[(0, 480, 60)], &[(0, 480, 62)]]), 0.9, 1),
            candidate(odd, 0.5, 2),
        ];
        assert_eq!(
            chain_layers(&ranked).unwrap_err(),
            ChainError::CrossBarMaterial {
                candidate: 1,
                bar: 0,
            },
        );
    }

    #[test]
    fn a_note_ending_exactly_on_the_bar_line_is_accepted() {
        // Half-open bars: a note ending at the bar's end is inside it.
        let ranked = vec![candidate(
            score_of(&[&[(0, BAR, 60)], &[(0, 480, 62)]]),
            0.9,
            1,
        )];
        assert!(
            chain_layers(&ranked).is_ok(),
            "a full-bar note is not cross-bar"
        );
    }
}
