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

use core::cmp::Reverse;

use crate::generate::{GenerationSeed, GenerationStrategy};
use crate::layered_path::PathError;
use crate::rerank::SetCandidate;
use crate::score::{AtomEvent, Score};
use crate::scoring::{Axes, Axis, Provenance, Rationale, Scored, WeightPolicy};

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

/// Local axis: how much this candidate's S6 quality costs the chain.
pub const AXIS_CANDIDATE_QUALITY: &str = "candidate_quality";
/// Transition axis: the boundary pitch jump, in semitones. **Absent** when the
/// boundary is silent — an unmeasurable fact is omitted, never zeroed.
pub const AXIS_BOUNDARY_JUMP: &str = "boundary_jump_semitones";
/// Transition axis: `1.0` when a boundary has no sounding pitch on one side, so
/// the jump could not be measured; `0.0` when it could.
pub const AXIS_SILENT_BOUNDARY: &str = "silent_boundary";
/// Transition axis: `1.0` when adjacent bars share an identical rhythm
/// signature, `0.0` otherwise.
pub const AXIS_RHYTHM_REPEAT: &str = "rhythm_repeat";

/// The `candidate_chain` v1 weights — an **untuned, documented baseline**.
///
/// Not corpus-calibrated and not S9-learned; S7 consumes weights, S9 may one
/// day tune them (ADR-0013 §4). Each term and its unit:
///
/// - [`AXIS_CANDIDATE_QUALITY`] `1.0` — the local preference. Value is
///   `1 − s6_aggregate` (the S6 rerank aggregate is in `[0, 1]` under its
///   uniform policy), so a better S6 candidate always costs less. Weight `1.0`
///   makes it the reference scale every other term is expressed against.
/// - [`AXIS_BOUNDARY_JUMP`] `0.05` — per **semitone** of pitch distance across
///   the bar line, unwrapped. An octave-plus leap (13 st → `0.65`) therefore
///   outweighs a whole S6 quality gap of `0.65`, while a step or two (`0.05`–
///   `0.10`) is cheap enough that quality still decides.
/// - [`AXIS_SILENT_BOUNDARY`] `0.25` — charged when the jump is *unmeasurable*
///   (a silent bar edge). Deliberately not free: "no pitch here" is a real,
///   mildly awkward fact, not perfect continuity.
/// - [`AXIS_RHYTHM_REPEAT`] `0.40` — charged once when adjacent bars share an
///   identical rhythm. Roughly a 8-semitone jump, so the chain will accept a
///   moderate leap to avoid a literal rhythmic copy — the "four identical bars
///   in a row" ADR-0013 complains about.
#[must_use]
pub fn chain_weights_v1() -> WeightPolicy {
    WeightPolicy::new(
        "candidate_chain",
        1,
        vec![
            (AXIS_CANDIDATE_QUALITY, 1.0),
            (AXIS_BOUNDARY_JUMP, 0.05),
            (AXIS_SILENT_BOUNDARY, 0.25),
            (AXIS_RHYTHM_REPEAT, 0.40),
        ],
    )
}

/// One sounding note of a bar, measured relative to that bar's start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BarNote {
    /// Ticks from the bar's start.
    offset: u32,
    /// The note's real duration in ticks.
    duration: u32,
    /// The MIDI pitch.
    pitch: u8,
}

/// The sounding notes of `bar` in `score`, ascending by onset then pitch.
///
/// Rests are not sounding pitches and take no part.
fn bar_notes(score: &Score, bar: usize) -> Vec<BarNote> {
    let Some(range) = score.master_bars.get(bar).map(|b| b.tick_range) else {
        return Vec::new();
    };
    let mut notes: Vec<BarNote> = score
        .tracks
        .iter()
        .flat_map(|t| t.voices.iter())
        .flat_map(|v| v.event_groups.iter())
        .flat_map(|g| g.atoms.iter())
        .filter_map(|atom| match atom {
            AtomEvent::Note(n) => Some(n),
            AtomEvent::Rest(_) => None,
        })
        .filter(|n| n.absolute_start.0 >= range.start.0 && n.absolute_start.0 < range.end.0)
        .map(|n| BarNote {
            offset: n.absolute_start.0.saturating_sub(range.start.0),
            duration: n.duration.0,
            pitch: n.pitch.0,
        })
        .collect();
    notes.sort_unstable_by_key(|n| (n.offset, n.pitch));
    notes
}

/// The bar's rhythm signature: its `(offset, duration)` pairs, ascending.
///
/// Built from **real onsets and durations**, never an equal-step placeholder,
/// and deliberately pitch-free — two bars with the same rhythm and different
/// notes are still a rhythmic repeat. A silent bar's signature is the explicit
/// empty vector, so two silences compare equal rather than being a gap.
fn rhythm_signature(notes: &[BarNote]) -> Vec<(u32, u32)> {
    let mut signature: Vec<(u32, u32)> = notes.iter().map(|n| (n.offset, n.duration)).collect();
    signature.sort_unstable();
    signature
}

/// The bar's last sounding pitch: the latest onset, and among a chord sharing
/// that onset, the highest pitch (the melodic top). `None` for a silent bar.
fn last_pitch(notes: &[BarNote]) -> Option<u8> {
    notes
        .iter()
        .max_by_key(|n| (n.offset, n.pitch))
        .map(|n| n.pitch)
}

/// The bar's first sounding pitch: the earliest onset, and among a chord sharing
/// it, the highest pitch. `None` for a silent bar.
fn first_pitch(notes: &[BarNote]) -> Option<u8> {
    notes
        .iter()
        .min_by_key(|n| (n.offset, Reverse(n.pitch)))
        .map(|n| n.pitch)
}

/// The local cost facts for a candidate whose S6 rerank aggregate is `s6`.
///
/// The transform is `1 − s6`: strictly decreasing, so a better S6 aggregate can
/// never earn a worse local cost. The S6 verdict is reused, never recomputed,
/// and never replaced by the candidate's ordinal.
#[must_use]
pub fn local_facts(s6: f64) -> Axes {
    Axes::new(vec![Axis {
        label: AXIS_CANDIDATE_QUALITY,
        value: 1.0 - s6,
    }])
}

/// The transition cost facts across the line between `from_bar` of `from` and
/// `to_bar` of `to`.
///
/// Measured from canonical note events only. When either side has no sounding
/// pitch the jump is *unmeasurable*: [`AXIS_BOUNDARY_JUMP`] is then **absent**
/// and [`AXIS_SILENT_BOUNDARY`] carries the fact instead — an unavailable
/// measurement is never a zero pretending to be perfect continuity.
#[must_use]
pub fn transition_facts(from: &Score, from_bar: usize, to: &Score, to_bar: usize) -> Axes {
    let left = bar_notes(from, from_bar);
    let right = bar_notes(to, to_bar);
    let mut axes = Vec::with_capacity(3);
    if let (Some(a), Some(b)) = (last_pitch(&left), first_pitch(&right)) {
        // Semitones, unwrapped: a 14-semitone leap stays a 14-semitone leap.
        let jump = f64::from(i16::from(a).saturating_sub(i16::from(b))).abs();
        axes.push(Axis {
            label: AXIS_BOUNDARY_JUMP,
            value: jump,
        });
        axes.push(Axis {
            label: AXIS_SILENT_BOUNDARY,
            value: 0.0,
        });
    } else {
        // Unmeasurable: the jump axis is ABSENT rather than a zero that would
        // read as perfect continuity. The silent fact is charged instead.
        axes.push(Axis {
            label: AXIS_SILENT_BOUNDARY,
            value: 1.0,
        });
    }
    let repeat = if rhythm_signature(&left) == rhythm_signature(&right) {
        1.0
    } else {
        0.0
    };
    axes.push(Axis {
        label: AXIS_RHYTHM_REPEAT,
        value: repeat,
    });
    Axes::new(axes)
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
    use super::{
        chain_layers, chain_weights_v1, local_facts, transition_facts, ChainError,
        AXIS_BOUNDARY_JUMP, AXIS_CANDIDATE_QUALITY, AXIS_RHYTHM_REPEAT, AXIS_SILENT_BOUNDARY,
    };
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

    // ── the v1 cost model ────────────────────────────────────────────────────

    #[test]
    fn the_v1_policy_names_its_id_version_and_documented_weights() {
        let p = chain_weights_v1();
        assert_eq!(p.id, "candidate_chain");
        assert_eq!(p.version, 1);
        assert!((p.weight(AXIS_CANDIDATE_QUALITY) - 1.0).abs() < 1e-12);
        assert!((p.weight(AXIS_BOUNDARY_JUMP) - 0.05).abs() < 1e-12);
        assert!((p.weight(AXIS_SILENT_BOUNDARY) - 0.25).abs() < 1e-12);
        assert!((p.weight(AXIS_RHYTHM_REPEAT) - 0.40).abs() < 1e-12);
    }

    #[test]
    fn the_local_cost_is_monotonic_in_the_s6_aggregate() {
        let cost = |s6: f64| local_facts(s6).get(AXIS_CANDIDATE_QUALITY).unwrap();
        assert!(
            cost(0.9) < cost(0.5),
            "a better S6 aggregate never costs more"
        );
        assert!(cost(0.5) < cost(0.1));
        assert!((cost(1.0) - 0.0).abs() < 1e-12, "the transform is 1 - s6");
    }

    #[test]
    fn the_boundary_jump_separates_a_step_from_an_octave_leap() {
        let a = score_of(&[&[(0, 480, 60)], &[(0, 480, 62)]]);
        let b = score_of(&[&[(0, 480, 60)], &[(0, 480, 74)]]);
        let step = transition_facts(&a, 0, &a, 1)
            .get(AXIS_BOUNDARY_JUMP)
            .unwrap();
        let leap = transition_facts(&b, 0, &b, 1)
            .get(AXIS_BOUNDARY_JUMP)
            .unwrap();
        assert!((step - 2.0).abs() < 1e-12, "two semitones, unwrapped");
        assert!(
            (leap - 14.0).abs() < 1e-12,
            "fourteen semitones, never wrapped to two",
        );
        assert!(leap > step, "a >12-semitone jump is visibly dearer");
    }

    #[test]
    fn a_silent_boundary_omits_the_jump_rather_than_zeroing_it() {
        let s = score_of(&[&[(0, 480, 60)], &[]]);
        let facts = transition_facts(&s, 0, &s, 1);
        assert_eq!(
            facts.get(AXIS_BOUNDARY_JUMP),
            None,
            "an unmeasurable jump is ABSENT, never a zero that reads as continuity",
        );
        assert!((facts.get(AXIS_SILENT_BOUNDARY).unwrap() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn a_measurable_boundary_records_a_zero_silent_fact() {
        let s = score_of(&[&[(0, 480, 60)], &[(0, 480, 62)]]);
        let facts = transition_facts(&s, 0, &s, 1);
        assert!((facts.get(AXIS_SILENT_BOUNDARY).unwrap() - 0.0).abs() < 1e-12);
        assert!(facts.get(AXIS_BOUNDARY_JUMP).is_some());
    }

    #[test]
    fn the_same_rhythm_with_different_pitches_is_still_a_repeat() {
        let s = score_of(&[
            &[(0, 480, 60), (960, 480, 62)],
            &[(0, 480, 71), (960, 480, 69)],
        ]);
        let repeat = transition_facts(&s, 0, &s, 1)
            .get(AXIS_RHYTHM_REPEAT)
            .unwrap();
        assert!(
            (repeat - 1.0).abs() < 1e-12,
            "the signature is pitch-free: same onsets and durations is a repeat",
        );
    }

    #[test]
    fn different_durations_are_not_the_same_rhythm() {
        let s = score_of(&[&[(0, 480, 60)], &[(0, 960, 60)]]);
        let repeat = transition_facts(&s, 0, &s, 1)
            .get(AXIS_RHYTHM_REPEAT)
            .unwrap();
        assert!(
            (repeat - 0.0).abs() < 1e-12,
            "same onset, different duration is a different rhythm",
        );
    }

    #[test]
    fn different_onsets_are_not_the_same_rhythm() {
        let s = score_of(&[&[(0, 480, 60)], &[(480, 480, 60)]]);
        let repeat = transition_facts(&s, 0, &s, 1)
            .get(AXIS_RHYTHM_REPEAT)
            .unwrap();
        assert!(
            (repeat - 0.0).abs() < 1e-12,
            "the signature uses real onsets, not equal-step placeholders",
        );
    }

    #[test]
    fn two_silent_bars_share_the_explicit_empty_signature() {
        let s = score_of(&[&[], &[]]);
        let facts = transition_facts(&s, 0, &s, 1);
        assert!(
            (facts.get(AXIS_RHYTHM_REPEAT).unwrap() - 1.0).abs() < 1e-12,
            "two silences are a rhythmic repeat — an explicit signature, not a gap",
        );
        assert!((facts.get(AXIS_SILENT_BOUNDARY).unwrap() - 1.0).abs() < 1e-12);
    }
}
