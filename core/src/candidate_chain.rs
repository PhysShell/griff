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
use crate::generation_input::RankedSet;
use crate::layered_path::{self, EdgeId, PathError};
use crate::rerank::SetCandidate;
use crate::score::{AtomEvent, EventGroup, LossReport, MasterBar, Score, Track, Voice};
use crate::scoring::{Axes, Axis, Provenance, Rationale, Scored, WeightPolicy};
use crate::slice::TickRange;

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
    /// A candidate's master bar differs from the first candidate's in the named
    /// field, so the master timelines cannot be one timeline.
    ///
    /// The field is named because assembly copies the whole master bar from
    /// ranked candidate 0: any field that differs is a field a selected bar
    /// would silently lose.
    MasterBarMismatch {
        /// The offending candidate's ordinal.
        candidate: usize,
        /// The bar whose fact differs.
        bar: usize,
        /// Which fact differs.
        field: MasterBarField,
    },
    /// A candidate has a different number of tracks than the first candidate,
    /// so bars from the two cannot be assembled into one part.
    TrackCountMismatch {
        /// The offending candidate's ordinal.
        candidate: usize,
        /// The track count the first candidate set.
        expected: usize,
        /// The offending candidate's track count.
        found: usize,
    },
    /// A candidate's track differs from the first candidate's in the named
    /// field, so the assembled part would misdescribe the borrowed bars.
    TrackMetadataMismatch {
        /// The offending candidate's ordinal.
        candidate: usize,
        /// The track whose fact differs.
        track: usize,
        /// Which fact differs.
        field: TrackField,
    },
    /// A candidate names a different source format than the first candidate's,
    /// which assembly would otherwise claim for every borrowed bar.
    SourceMetaMismatch {
        /// The offending candidate's ordinal.
        candidate: usize,
    },
    /// A candidate carries material that does not fit inside the bar its event
    /// group belongs to — a note sounding past the bar's end, an atom in another
    /// bar than its group's, or a technique span outside the group's bar.
    ///
    /// The whole group is lifted into one output bar, so everything it holds
    /// must live in that bar. The slicing contract this v1 reuses cuts by onset
    /// and clamps spans, so it cannot concatenate such material losslessly.
    /// Rather than clip, shorten, or silently drop a musical event, the chain
    /// refuses the set.
    CrossBarMaterial {
        /// The offending candidate's ordinal.
        candidate: usize,
        /// The bar the offending group belongs to.
        bar: usize,
    },
    /// A candidate holds an event group with no atoms.
    ///
    /// A group is the unit assembly lifts, and it is attributed to a bar by the
    /// atoms it holds. A group with none belongs to no bar, so it would be
    /// dropped by every bar — silently, and exactly once per assembled score.
    EmptyEventGroup {
        /// The offending candidate's ordinal.
        candidate: usize,
    },
    /// A candidate holds material at a tick outside its own master timeline.
    ///
    /// There is no bar to name, so this cannot be reported as cross-bar
    /// material without inventing one.
    MaterialOutsideTimeline {
        /// The offending candidate's ordinal.
        candidate: usize,
        /// The offending tick.
        tick: u32,
    },
    /// Assembly asked a candidate for material it does not have.
    ///
    /// After compatibility validation this is an invariant violation, not an
    /// empty bar: a missing track, voice, or bar is a different fact from a
    /// silent one.
    MissingMaterial {
        /// The candidate the bar was selected from.
        candidate: usize,
        /// The track asked for.
        track: usize,
        /// The voice asked for.
        voice: usize,
        /// The bar asked for.
        bar: usize,
    },
    /// A boundary fact could not be measured because a bar address was invalid.
    ///
    /// After compatibility validation this is an invariant violation, not a
    /// musical observation — it never surfaces as silence.
    BoundaryFact(TransitionFactError),
    /// The layered engine rejected the problem the chain handed it.
    Path(PathError),
}

/// A master-bar fact the assembled score copies from ranked candidate 0.
///
/// Every variant is a field `assemble` clones off candidate 0's master bar and
/// then applies to a bar that may have come from any candidate. Validating the
/// whole struct rather than the three fields the cost model happens to read is
/// the point: the ones it does not read are exactly the ones that would go
/// missing quietly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MasterBarField {
    /// [`MasterBar::index`](crate::score::MasterBar::index).
    Index,
    /// [`MasterBar::tick_range`](crate::score::MasterBar::tick_range).
    TickRange,
    /// [`MasterBar::time_signature`](crate::score::MasterBar::time_signature).
    TimeSignature,
    /// [`MasterBar::tempo`](crate::score::MasterBar::tempo), compared bitwise.
    Tempo,
    /// [`MasterBar::repeat`](crate::score::MasterBar::repeat) — the repeat
    /// barlines. A borrowed bar under a foreign repeat structure is played a
    /// different number of times than its candidate meant.
    Repeat,
}

/// A track fact the assembled score copies from ranked candidate 0.
///
/// As with [`MasterBarField`], each variant is something `assemble` takes from
/// candidate 0 and applies to bars sourced from elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackField {
    /// [`Track::name`](crate::score::Track::name).
    Name,
    /// [`Track::channel`](crate::score::Track::channel).
    Channel,
    /// [`Track::tuning`](crate::score::Track::tuning) — a bar's fretboard
    /// positions mean nothing under another tuning (ADR-0018).
    Tuning,
    /// The track's number of voices.
    VoiceCount,
    /// [`Voice::id`](crate::score::Voice::id).
    VoiceId,
}

impl From<PathError> for ChainError {
    fn from(error: PathError) -> Self {
        Self::Path(error)
    }
}

impl From<TransitionFactError> for ChainError {
    fn from(error: TransitionFactError) -> Self {
        Self::BoundaryFact(error)
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
/// Rests are not sounding pitches and take no part. `None` — distinct from an
/// empty vector — when `bar` is not a bar of `score` at all: a bar that does not
/// exist has no notes to report, which is not the same fact as a bar that exists
/// and is silent.
fn bar_notes(score: &Score, bar: usize) -> Option<Vec<BarNote>> {
    let range = score.master_bars.get(bar).map(|b| b.tick_range)?;
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
    Some(notes)
}

/// When the note stops sounding, in ticks from its bar's start.
///
/// Widened to `u64` on purpose: both terms are `u32`, so the sum is exact and
/// overflow is *unrepresentable* rather than merely unlikely. Saturating or
/// wrapping arithmetic would silently reorder notes at the top of the tick
/// range — a measurement function must not hide its own overflow.
#[allow(clippy::arithmetic_side_effects)]
fn sounding_end(note: &BarNote) -> u64 {
    u64::from(note.offset) + u64::from(note.duration)
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

/// The bar's last **sounding** pitch: the note still ringing latest into the bar
/// line, measured by its end (`offset + duration`), not by its onset. `None` for
/// a silent bar.
///
/// A held note that opened the bar and rings through it is what the ear carries
/// across the line; a short note struck later and already stopped is not. Among
/// notes ending together the tie-break is the **highest** pitch — the chord top,
/// matching [`first_pitch`]'s convention on the other side of the line.
fn last_pitch(notes: &[BarNote]) -> Option<u8> {
    notes
        .iter()
        .max_by_key(|n| (sounding_end(n), n.pitch))
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

/// Why boundary facts could not be measured.
///
/// An invalid bar address and a genuinely silent bar are different states: the
/// first is a caller error, the second a musical observation. Conflating them
/// would let a typo read as a legitimate `silent_boundary`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionFactError {
    /// `from_bar` is not a bar of the `from` score.
    MissingFromBar {
        /// The requested bar.
        bar: usize,
        /// How many bars the score actually has.
        bars: usize,
    },
    /// `to_bar` is not a bar of the `to` score.
    MissingToBar {
        /// The requested bar.
        bar: usize,
        /// How many bars the score actually has.
        bars: usize,
    },
}

/// The transition cost facts across the line between `from_bar` of `from` and
/// `to_bar` of `to`.
///
/// Measured from canonical note events only. When either side has no sounding
/// pitch the jump is *unmeasurable*: [`AXIS_BOUNDARY_JUMP`] is then **absent**
/// and [`AXIS_SILENT_BOUNDARY`] carries the fact instead — an unavailable
/// measurement is never a zero pretending to be perfect continuity.
///
/// # Errors
/// [`TransitionFactError`] when either bar index is out of range. An invalid
/// address is not silence.
pub fn transition_facts(
    from: &Score,
    from_bar: usize,
    to: &Score,
    to_bar: usize,
) -> Result<Axes, TransitionFactError> {
    let left = bar_notes(from, from_bar).ok_or(TransitionFactError::MissingFromBar {
        bar: from_bar,
        bars: from.master_bars.len(),
    })?;
    let right = bar_notes(to, to_bar).ok_or(TransitionFactError::MissingToBar {
        bar: to_bar,
        bars: to.master_bars.len(),
    })?;
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
    Ok(Axes::new(axes))
}

/// One selected bar of the planned chain, with everything needed to say where
/// it came from and why it was chosen.
#[derive(Debug, Clone)]
pub struct ChainStep {
    /// Which output bar, from which candidate, with that candidate's own
    /// generation identity (strategy, variant seed, original rank).
    pub state: ChainState,
    /// The chain-policy local cost: its facts, rationale, and provenance.
    pub local: Scored<layered_path::StateId>,
    /// The supplying candidate's untouched S6 rerank score.
    pub s6: S6Quality,
}

/// One selected bar line of the planned chain, with its full explanation.
#[derive(Debug, Clone)]
pub struct ChainTransition {
    /// The step this transition leaves.
    pub from: ChainState,
    /// The step this transition enters.
    pub to: ChainState,
    /// The chain-policy transition cost: its facts, rationale, and provenance.
    pub cost: Scored<EdgeId>,
}

/// A planned multi-bar chain: the assembled score and the whole trace behind it.
#[derive(Debug, Clone)]
pub struct PlannedCandidateChain {
    /// The assembled score — the selected bars over the shared master timeline.
    pub score: Score,
    /// The selected bar of every layer, in bar order.
    pub steps: Vec<ChainStep>,
    /// The selected bar line between each adjacent pair, in bar order.
    pub transitions: Vec<ChainTransition>,
    /// `Σ local + Σ transition`, derived from the retained rationales.
    pub total_cost: f64,
    /// The chain policy the costs were weighed under.
    pub provenance: Provenance,
}

/// Plans the best multi-bar chain over `set` under the `candidate_chain` v1
/// policy.
///
/// Nothing is regenerated, reranked, or re-seeded: every bar is a snapshot of a
/// score already in the set, and the ranked set is only read.
///
/// # Errors
/// [`ChainError`] when the set is not chain-compatible, or the layered engine
/// rejects the problem.
pub fn plan_candidate_chain(set: &RankedSet) -> Result<PlannedCandidateChain, ChainError> {
    plan_ranked_with(&set.ranked, &chain_weights_v1())
}

/// Plans a chain over `ranked` under an explicit policy — the seam the v1
/// entry point and the tests share.
fn plan_ranked_with(
    ranked: &[Scored<SetCandidate>],
    policy: &WeightPolicy,
) -> Result<PlannedCandidateChain, ChainError> {
    let layers = chain_layers(ranked)?; // also validates chain-compatibility
    let bars = layers.len();

    // The facts. The engine computes none of them; it only weighs them.
    let locals: Vec<Vec<Axes>> = layers
        .iter()
        .map(|layer| {
            layer
                .iter()
                .map(|state| {
                    let s6 = ranked.get(state.candidate).map_or(0.0, Scored::aggregate);
                    local_facts(s6)
                })
                .collect()
        })
        .collect();
    let mut transitions: Vec<Vec<Vec<Axes>>> = Vec::with_capacity(bars.saturating_sub(1));
    for bar in 0..bars.saturating_sub(1) {
        let mut table: Vec<Vec<Axes>> = Vec::with_capacity(ranked.len());
        for from in ranked {
            let mut row: Vec<Axes> = Vec::with_capacity(ranked.len());
            for to in ranked {
                row.push(transition_facts(
                    &from.value.score,
                    bar,
                    &to.value.score,
                    bar.saturating_add(1),
                )?);
            }
            table.push(row);
        }
        transitions.push(table);
    }

    let solution = layered_path::solve(&layered_path::LayeredProblem {
        locals: &locals,
        transitions: &transitions,
        policy,
    })?;

    let state_at = |id: layered_path::StateId| -> Option<ChainState> {
        layers.get(id.layer)?.get(id.ordinal).copied()
    };
    let steps: Vec<ChainStep> = solution
        .steps
        .iter()
        .filter_map(|scored| {
            let state = state_at(scored.value)?;
            let source = ranked.get(state.candidate)?;
            Some(ChainStep {
                state,
                local: scored.clone(),
                // S6's verdict, carried through untouched.
                s6: S6Quality {
                    axes: source.axes.clone(),
                    rationale: source.rationale.clone(),
                    provenance: source.provenance,
                    aggregate: source.aggregate(),
                },
            })
        })
        .collect();
    let chain_transitions: Vec<ChainTransition> = solution
        .edges
        .iter()
        .filter_map(|edge| {
            Some(ChainTransition {
                from: state_at(edge.value.from)?,
                to: state_at(edge.value.to)?,
                cost: edge.clone(),
            })
        })
        .collect();

    let chosen: Vec<usize> = steps.iter().map(|s| s.state.candidate).collect();
    let score = assemble(ranked, &chosen)?;

    Ok(PlannedCandidateChain {
        score,
        steps,
        transitions: chain_transitions,
        total_cost: solution.total_cost,
        provenance: solution.provenance,
    })
}

/// Assembles the selected bars into one canonical score.
///
/// Every candidate already agreed on the master timeline (validated), so bar
/// `b` occupies the same ticks in each of them: the selected bar's event groups
/// are copied **verbatim**, with no rebasing, no re-quantising, and no MIDI
/// round-trip. MIDI is a boundary, not an editing tool.
fn assemble(ranked: &[Scored<SetCandidate>], chosen: &[usize]) -> Result<Score, ChainError> {
    let reference = &ranked.first().ok_or(ChainError::EmptySet)?.value.score;
    // Written as loops, not as a `map` chain: every lookup here can fail, and
    // the only way to write this as an iterator is to answer a failed lookup
    // with an empty vector — which is how a missing bar becomes a short bar.
    let mut tracks: Vec<Track> = Vec::with_capacity(reference.tracks.len());
    for (track, source_track) in reference.tracks.iter().enumerate() {
        let mut voices: Vec<Voice> = Vec::with_capacity(source_track.voices.len());
        for (voice, source_voice) in source_track.voices.iter().enumerate() {
            let mut event_groups: Vec<EventGroup> = Vec::new();
            for (bar, &candidate) in chosen.iter().enumerate() {
                let supplier = ranked.get(candidate).ok_or(ChainError::MissingMaterial {
                    candidate,
                    track,
                    voice,
                    bar,
                })?;
                event_groups.extend(groups_in_bar(
                    candidate,
                    &supplier.value.score,
                    track,
                    voice,
                    bar,
                )?);
            }
            voices.push(Voice {
                id: source_voice.id,
                event_groups,
            });
        }
        tracks.push(Track {
            name: source_track.name.clone(),
            channel: source_track.channel,
            tuning: source_track.tuning.clone(),
            voices,
        });
    }
    Ok(Score {
        ticks_per_quarter: reference.ticks_per_quarter,
        // The one timeline every candidate shares — never a layer winner's.
        master_bars: reference.master_bars.clone(),
        tracks,
        source_meta: reference.source_meta.clone(),
        loss: LossReport::new(),
    })
}

/// The event groups of `track`/`voice` that belong to `bar`, cloned verbatim.
///
/// A group is attributed by [`group_bar`]; validation has already guaranteed
/// every atom and span of a group lives in that one bar, so taking the group is
/// taking all of it.
///
/// # Errors
/// [`ChainError::MissingMaterial`] when the bar, track, or voice does not
/// exist. That is not an empty bar: after compatibility validation it cannot
/// happen, and if it ever does, answering "no groups" would turn an invariant
/// violation into a silently shorter part.
fn groups_in_bar(
    candidate: usize,
    score: &Score,
    track: usize,
    voice: usize,
    bar: usize,
) -> Result<Vec<EventGroup>, ChainError> {
    let missing = ChainError::MissingMaterial {
        candidate,
        track,
        voice,
        bar,
    };
    if bar >= score.master_bars.len() {
        return Err(missing);
    }
    let source = score
        .tracks
        .get(track)
        .and_then(|t| t.voices.get(voice))
        .ok_or(missing)?;
    let mut kept = Vec::new();
    for group in &source.event_groups {
        if group_bar(candidate, score, group)?.0 == bar {
            kept.push(group.clone());
        }
    }
    Ok(kept)
}

/// The S6 baseline cost: ranked candidate 0 kept **intact** as one multi-bar
/// score, weighed under the very same `candidate_chain` v1 policy.
///
/// This is what the planned chain must beat — the same metric on both sides.
///
/// # Errors
/// [`ChainError`] when the set is not chain-compatible.
pub fn intact_s6_cost(set: &RankedSet) -> Result<f64, ChainError> {
    intact_cost_with(&set.ranked, &chain_weights_v1())
}

/// The intact-candidate-0 cost under an explicit policy.
fn intact_cost_with(
    ranked: &[Scored<SetCandidate>],
    policy: &WeightPolicy,
) -> Result<f64, ChainError> {
    let bars = chain_layers(ranked)?.len();
    let winner = ranked.first().ok_or(ChainError::EmptySet)?;
    let score = &winner.value.score;
    let s6 = winner.aggregate();

    // The same facts and the same policy the planned chain is weighed under —
    // one metric on both sides of the comparison.
    let local: f64 = (0..bars)
        .map(|_| Scored::new((), local_facts(s6), policy, None).aggregate())
        .sum();
    let mut transition = 0.0_f64;
    for bar in 0..bars.saturating_sub(1) {
        let facts = transition_facts(score, bar, score, bar.saturating_add(1))?;
        transition += Scored::new((), facts, policy, None).aggregate();
    }
    Ok(local + transition)
}

/// The layers of a chain problem: `layers[b][c]` is bar `b` of candidate `c`.
///
/// # Errors
/// [`ChainError`] on the first fact that makes the set unchainable: an empty
/// set, no bars, a mismatched bar count or resolution, a disagreement about any
/// master-bar or track fact assembly copies from ranked candidate 0
/// ([`MasterBarField`], [`TrackField`], the source format), or material
/// crossing a bar line.
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
        if let Some(field) = master_bar_disagreement(want, got) {
            return Err(ChainError::MasterBarMismatch {
                candidate,
                bar,
                field,
            });
        }
    }
    if score.tracks.len() != reference.tracks.len() {
        return Err(ChainError::TrackCountMismatch {
            candidate,
            expected: reference.tracks.len(),
            found: score.tracks.len(),
        });
    }
    for (track, (want, got)) in reference.tracks.iter().zip(score.tracks.iter()).enumerate() {
        if let Some(field) = track_disagreement(want, got) {
            return Err(ChainError::TrackMetadataMismatch {
                candidate,
                track,
                field,
            });
        }
    }
    // Assembly claims the reference's origin for every bar it borrows.
    let format = |s: &Score| s.source_meta.as_ref().and_then(|m| m.format.clone());
    if format(score) != format(reference) {
        return Err(ChainError::SourceMetaMismatch { candidate });
    }
    Ok(())
}

/// The first master-bar fact `got` disagrees with `want` about, if any.
///
/// Exhaustive over what `assemble` copies from ranked candidate 0 — the cost
/// model reads three of these five, and the other two are the ones a borrowed
/// bar would lose without a sound changing anywhere the tests look.
fn master_bar_disagreement(want: &MasterBar, got: &MasterBar) -> Option<MasterBarField> {
    if want.index != got.index {
        return Some(MasterBarField::Index);
    }
    if want.tick_range != got.tick_range {
        return Some(MasterBarField::TickRange);
    }
    if want.time_signature != got.time_signature {
        return Some(MasterBarField::TimeSignature);
    }
    // Bitwise: two tempos are the same tempo or they are not, and `Tempo` is an
    // f64 whose equality would otherwise be an approximation question.
    if want.tempo.0.to_bits() != got.tempo.0.to_bits() {
        return Some(MasterBarField::Tempo);
    }
    if want.repeat != got.repeat {
        return Some(MasterBarField::Repeat);
    }
    None
}

/// The first track fact `got` disagrees with `want` about, if any.
///
/// Event groups are deliberately absent: they are what the candidates are
/// *supposed* to differ in. Everything else in a `Track` is skeleton.
fn track_disagreement(want: &Track, got: &Track) -> Option<TrackField> {
    if want.name != got.name {
        return Some(TrackField::Name);
    }
    if want.channel != got.channel {
        return Some(TrackField::Channel);
    }
    if want.tuning != got.tuning {
        return Some(TrackField::Tuning);
    }
    if want.voices.len() != got.voices.len() {
        return Some(TrackField::VoiceCount);
    }
    if want
        .voices
        .iter()
        .zip(got.voices.iter())
        .any(|(a, b)| a.id != b.id)
    {
        return Some(TrackField::VoiceId);
    }
    None
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
        // The group's own bar, decided once, from its first atom. Everything the
        // group holds is then measured against *that* bar rather than against
        // whichever bar each piece happens to land in — the group is lifted
        // whole, so a piece elsewhere is a piece in the wrong place.
        let (bar, range) = group_bar(candidate, score, group)?;
        for atom in &group.atoms {
            let start = atom.absolute_start().0;
            if start < range.start.0 || start >= range.end.0 {
                return Err(ChainError::CrossBarMaterial { candidate, bar });
            }
            if atom_end(atom) > u64::from(range.end.0) {
                return Err(ChainError::CrossBarMaterial { candidate, bar });
            }
        }
        for span in &group.technique_spans {
            let start = span.tick_range.start.0;
            if start < range.start.0 || start >= range.end.0 {
                return Err(ChainError::CrossBarMaterial { candidate, bar });
            }
            if span.tick_range.end.0 > range.end.0 {
                return Err(ChainError::CrossBarMaterial { candidate, bar });
            }
        }
    }
    Ok(())
}

/// The bar an event group belongs to — its index and its ticks — decided by the
/// group's first atom.
///
/// # Errors
/// [`ChainError::EmptyEventGroup`] when the group has no atoms to place it by,
/// and [`ChainError::MaterialOutsideTimeline`] when its first atom sits at a
/// tick no bar covers — neither has a bar to be reported against.
fn group_bar(
    candidate: usize,
    score: &Score,
    group: &EventGroup,
) -> Result<(usize, TickRange), ChainError> {
    let first = group
        .atoms
        .first()
        .ok_or(ChainError::EmptyEventGroup { candidate })?;
    let tick = first.absolute_start().0;
    bar_of(score, tick).ok_or(ChainError::MaterialOutsideTimeline { candidate, tick })
}

/// When an atom stops sounding, in absolute ticks.
///
/// Widened to `u64` for the same reason as [`sounding_end`]: a `u32` sum could
/// wrap and let a note that runs past the end of the timeline validate as one
/// that ends early.
#[allow(clippy::arithmetic_side_effects)]
fn atom_end(atom: &AtomEvent) -> u64 {
    u64::from(atom.absolute_start().0) + u64::from(atom.duration().0)
}

/// The bar containing `tick` — its index and its ticks — if any.
fn bar_of(score: &Score, tick: u32) -> Option<(usize, TickRange)> {
    score
        .master_bars
        .iter()
        .enumerate()
        .find(|(_, b)| tick >= b.tick_range.start.0 && tick < b.tick_range.end.0)
        .map(|(index, b)| (index, b.tick_range))
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
        chain_layers, chain_weights_v1, intact_s6_cost, local_facts, plan_candidate_chain,
        plan_ranked_with, transition_facts, ChainError, MasterBarField, TrackField,
        TransitionFactError, AXIS_BOUNDARY_JUMP, AXIS_CANDIDATE_QUALITY, AXIS_RHYTHM_REPEAT,
        AXIS_SILENT_BOUNDARY,
    };
    use crate::event::{
        FretboardPosition, NoteMark, NoteMarks, NotePosition, Pitch, SpanTechnique,
        TechniqueEvidence, Tempo, Ticks, TimeSignature, Tuning, Velocity,
    };
    use crate::generate::{
        GenerationConstraints, GenerationSeed, GenerationStrategy, PitchMaterial,
        RuleGenerationRequest,
    };
    use crate::generation_input::{ranked_candidates, GenerationAsk, RankedSet};
    use crate::layered_path::{PathError, StateId};
    use crate::rerank::SetCandidate;
    use crate::score::{
        AtomEvent, AtomNote, AtomRest, EventGroup, EventGroupKind, LossReport, MasterBar,
        RepeatMarker, Score, SourceMeta, TechniqueSpan, Track, Voice,
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

    /// Two identical two-bar candidates; `mutate` breaks the second one's
    /// agreement with the first in exactly one place.
    fn rejected_for(mutate: impl FnOnce(&mut Score)) -> ChainError {
        let bars: [&[Note]; 2] = [&[(0, 480, 60)], &[(0, 480, 62)]];
        let mut odd = score_of(&bars);
        mutate(&mut odd);
        let ranked = vec![candidate(score_of(&bars), 0.9, 1), candidate(odd, 0.5, 2)];
        chain_layers(&ranked).unwrap_err()
    }

    // Below: one law per fact `assemble` copies off ranked candidate 0. The cost
    // model reads three of them; assembly copies all of them, and the ones it
    // copies without reading are the ones that would vanish quietly.

    #[test]
    fn a_mismatched_bar_index_is_rejected() {
        assert_eq!(
            rejected_for(|s| s.master_bars[1].index = 7),
            ChainError::MasterBarMismatch {
                candidate: 1,
                bar: 1,
                field: MasterBarField::Index,
            },
        );
    }

    #[test]
    fn a_mismatched_bar_tick_range_is_rejected() {
        assert_eq!(
            rejected_for(|s| {
                s.master_bars[1].tick_range = TickRange::new(Ticks(BAR), Ticks(BAR * 3)).unwrap();
            }),
            ChainError::MasterBarMismatch {
                candidate: 1,
                bar: 1,
                field: MasterBarField::TickRange,
            },
        );
    }

    #[test]
    fn a_mismatched_time_signature_is_rejected() {
        assert_eq!(
            rejected_for(|s| s.master_bars[1].time_signature = TimeSignature::new(7, 8).unwrap()),
            ChainError::MasterBarMismatch {
                candidate: 1,
                bar: 1,
                field: MasterBarField::TimeSignature,
            },
        );
    }

    #[test]
    fn a_mismatched_tempo_is_rejected() {
        assert_eq!(
            rejected_for(|s| s.master_bars[1].tempo = Tempo::new(180.0).unwrap()),
            ChainError::MasterBarMismatch {
                candidate: 1,
                bar: 1,
                field: MasterBarField::Tempo,
            },
        );
    }

    #[test]
    fn a_mismatched_repeat_marker_is_rejected() {
        // Nothing in the cost model reads repeats, which is exactly why an
        // unvalidated one would survive to the assembled score and change how
        // many times the borrowed bar is played.
        assert_eq!(
            rejected_for(|s| {
                s.master_bars[1].repeat = RepeatMarker {
                    start: false,
                    play_count: 2,
                };
            }),
            ChainError::MasterBarMismatch {
                candidate: 1,
                bar: 1,
                field: MasterBarField::Repeat,
            },
        );
    }

    #[test]
    fn a_mismatched_track_count_is_rejected() {
        assert_eq!(
            rejected_for(|s| {
                let extra = s.tracks[0].clone();
                s.tracks.push(extra);
            }),
            ChainError::TrackCountMismatch {
                candidate: 1,
                expected: 1,
                found: 2,
            },
        );
    }

    #[test]
    fn a_mismatched_track_name_is_rejected() {
        assert_eq!(
            rejected_for(|s| s.tracks[0].name = Some("lead".to_owned())),
            ChainError::TrackMetadataMismatch {
                candidate: 1,
                track: 0,
                field: TrackField::Name,
            },
        );
    }

    #[test]
    fn a_mismatched_channel_is_rejected() {
        assert_eq!(
            rejected_for(|s| s.tracks[0].channel = 4),
            ChainError::TrackMetadataMismatch {
                candidate: 1,
                track: 0,
                field: TrackField::Channel,
            },
        );
    }

    #[test]
    fn a_mismatched_tuning_is_rejected() {
        // Drop D. The bars would look identical and mean different notes under
        // the fretboard model (ADR-0018).
        assert_eq!(
            rejected_for(|s| {
                s.tracks[0].tuning = Tuning::new(vec![
                    Pitch(64),
                    Pitch(59),
                    Pitch(55),
                    Pitch(50),
                    Pitch(45),
                    Pitch(38),
                ]);
            }),
            ChainError::TrackMetadataMismatch {
                candidate: 1,
                track: 0,
                field: TrackField::Tuning,
            },
        );
    }

    #[test]
    fn a_mismatched_voice_count_is_rejected() {
        assert_eq!(
            rejected_for(|s| {
                s.tracks[0].voices.push(Voice {
                    id: 1,
                    event_groups: Vec::new(),
                });
            }),
            ChainError::TrackMetadataMismatch {
                candidate: 1,
                track: 0,
                field: TrackField::VoiceCount,
            },
        );
    }

    #[test]
    fn a_mismatched_voice_id_is_rejected() {
        assert_eq!(
            rejected_for(|s| s.tracks[0].voices[0].id = 3),
            ChainError::TrackMetadataMismatch {
                candidate: 1,
                track: 0,
                field: TrackField::VoiceId,
            },
        );
    }

    #[test]
    fn a_mismatched_source_format_is_rejected() {
        // Assembly claims candidate 0's source for every bar it borrows.
        assert_eq!(
            rejected_for(|s| {
                s.source_meta = Some(SourceMeta {
                    format: Some("GP5".to_owned()),
                });
            }),
            ChainError::SourceMetaMismatch { candidate: 1 },
        );
    }

    #[test]
    fn the_assembled_score_carries_the_metadata_every_candidate_agreed_on() {
        // The other side of the rejection laws: when the set is accepted, the
        // metadata copied off candidate 0 is not a substitution, because every
        // supplying candidate carried the identical fact. Proven against the
        // non-greedy fixture, whose chain really does borrow bar 1 elsewhere.
        let set = non_greedy_set();
        let planned = plan_candidate_chain(&set).expect("plannable");
        assert_eq!(
            planned
                .steps
                .iter()
                .map(|s| s.state.candidate)
                .collect::<Vec<_>>(),
            vec![0, 1, 0],
            "the fixture's chain does borrow from another candidate",
        );

        let assembled = &planned.score;
        for supplier in &set.ranked {
            let source = &supplier.value.score;
            assert_eq!(assembled.ticks_per_quarter, source.ticks_per_quarter);
            assert_eq!(
                assembled
                    .master_bars
                    .iter()
                    .map(bar_facts)
                    .collect::<Vec<_>>(),
                source.master_bars.iter().map(bar_facts).collect::<Vec<_>>(),
                "every master-bar fact of the assembled score is one this supplier also holds",
            );
            assert_eq!(
                assembled.tracks.iter().map(track_facts).collect::<Vec<_>>(),
                source.tracks.iter().map(track_facts).collect::<Vec<_>>(),
                "every track fact of the assembled score is one this supplier also holds",
            );
            assert_eq!(
                assembled
                    .source_meta
                    .as_ref()
                    .and_then(|m| m.format.clone()),
                source.source_meta.as_ref().and_then(|m| m.format.clone()),
            );
        }
    }

    /// Every fact of a master bar, in a comparable form (`MasterBar` is not
    /// `PartialEq`, and `Tempo` holds an `f64`).
    fn bar_facts(bar: &MasterBar) -> (usize, TickRange, TimeSignature, u64, RepeatMarker) {
        (
            bar.index,
            bar.tick_range,
            bar.time_signature,
            bar.tempo.0.to_bits(),
            bar.repeat,
        )
    }

    /// Every track fact assembly copies, minus the event groups themselves.
    fn track_facts(track: &Track) -> (Option<String>, u8, Tuning, Vec<u8>) {
        (
            track.name.clone(),
            track.channel,
            track.tuning.clone(),
            track.voices.iter().map(|v| v.id).collect(),
        )
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

    // ── the group is the unit that is lifted ─────────────────────────────────

    #[test]
    fn an_event_group_with_no_atoms_is_rejected_rather_than_dropped() {
        // A group with no atoms belongs to no bar, so every bar's filter passes
        // over it and it disappears from the assembled score without a word.
        assert_eq!(
            rejected_for(|s| {
                s.tracks[0].voices[0].event_groups.push(EventGroup {
                    kind: EventGroupKind::Single,
                    atoms: Vec::new(),
                    technique_spans: Vec::new(),
                });
            }),
            ChainError::EmptyEventGroup { candidate: 1 },
        );
    }

    #[test]
    fn material_outside_the_master_timeline_is_rejected_by_its_tick() {
        // Past the last bar there is no bar to name — reporting this as bar 0
        // would be a measurement invented to fit the error type.
        assert_eq!(
            rejected_for(|s| {
                s.tracks[0].voices[0].event_groups.push(EventGroup {
                    kind: EventGroupKind::Single,
                    atoms: vec![AtomEvent::Note(AtomNote {
                        absolute_start: Ticks(BAR * 5),
                        duration: Ticks(480),
                        pitch: Pitch(60),
                        velocity: Velocity(96),
                        marks: NoteMarks::empty(),
                        position: None,
                    })],
                    technique_spans: Vec::new(),
                });
            }),
            ChainError::MaterialOutsideTimeline {
                candidate: 1,
                tick: BAR * 5,
            },
        );
    }

    #[test]
    fn a_group_whose_atoms_sit_in_different_bars_is_rejected() {
        // Bar 0's group gains a note in bar 1. The group is lifted whole, so
        // whichever bar takes it also takes the other bar's note.
        assert_eq!(
            rejected_for(|s| {
                s.tracks[0].voices[0].event_groups[0]
                    .atoms
                    .push(AtomEvent::Note(AtomNote {
                        absolute_start: Ticks(BAR),
                        duration: Ticks(480),
                        pitch: Pitch(62),
                        velocity: Velocity(96),
                        marks: NoteMarks::empty(),
                        position: None,
                    }));
            }),
            ChainError::CrossBarMaterial {
                candidate: 1,
                bar: 0,
            },
        );
    }

    #[test]
    fn a_technique_span_living_in_another_bar_than_its_group_is_rejected() {
        // The span is entirely inside bar 1 and fits it, but it hangs off bar
        // 0's group: it travels wherever bar 0 goes, and vanishes when bar 0 is
        // taken from someone else.
        assert_eq!(
            rejected_for(|s| {
                s.tracks[0].voices[0].event_groups[0]
                    .technique_spans
                    .push(TechniqueSpan {
                        technique: SpanTechnique::PalmMute,
                        tick_range: TickRange::new(Ticks(BAR), Ticks(BAR + 480)).unwrap(),
                        evidence: TechniqueEvidence::explicit(),
                    });
            }),
            ChainError::CrossBarMaterial {
                candidate: 1,
                bar: 0,
            },
        );
    }

    #[test]
    fn a_technique_span_starting_before_its_groups_bar_is_rejected() {
        assert_eq!(
            rejected_for(|s| {
                s.tracks[0].voices[0].event_groups[1]
                    .technique_spans
                    .push(TechniqueSpan {
                        technique: SpanTechnique::Slide,
                        tick_range: TickRange::new(Ticks(BAR - 240), Ticks(BAR + 480)).unwrap(),
                        evidence: TechniqueEvidence::explicit(),
                    });
            }),
            ChainError::CrossBarMaterial {
                candidate: 1,
                bar: 1,
            },
        );
    }

    // ── what a borrowed bar keeps ────────────────────────────────────────────

    /// The non-greedy fixture, with candidate 1's bar 1 — the one bar the chain
    /// borrows — carrying every fact assembly could lose: a group kind, a rest,
    /// a velocity, per-note marks, a fretboard position with its evidence, and
    /// a technique span with its own evidence and exact range.
    fn rich_borrowed_bar_set() -> RankedSet {
        let mut rich = score_of(&[&[(0, 480, 60)], &[(0, 480, 70)], &[(0, 480, 84)]]);
        let group = &mut rich.tracks[0].voices[0].event_groups[1];
        group.kind = EventGroupKind::Chord;
        let mut marks = NoteMarks::empty();
        marks.insert(NoteMark::Accent);
        marks.insert(NoteMark::Staccato);
        if let AtomEvent::Note(note) = &mut group.atoms[0] {
            note.velocity = Velocity(37);
            note.marks = marks;
            note.position = Some(NotePosition::explicit(FretboardPosition {
                string: 3,
                fret: 11,
            }));
        } else {
            panic!("the fixture's first atom is a note");
        }
        group.atoms.push(AtomEvent::Rest(AtomRest {
            absolute_start: Ticks(BAR + 480),
            duration: Ticks(480),
        }));
        group.technique_spans.push(TechniqueSpan {
            technique: SpanTechnique::PalmMute,
            tick_range: TickRange::new(Ticks(BAR), Ticks(BAR + 960)).unwrap(),
            evidence: TechniqueEvidence::inferred(0.42),
        });
        ranked_set(vec![
            candidate(
                score_of(&[&[(0, 480, 60)], &[(0, 480, 50)], &[(0, 480, 84)]]),
                0.9,
                1,
            ),
            candidate(rich, 0.8, 2),
            candidate(
                score_of(&[&[(0, 480, 60)], &[(0, 480, 50)], &[(0, 480, 84)]]),
                0.7,
                3,
            ),
        ])
    }

    /// The borrowed bar as it left candidate 1, and as it arrived in the plan.
    fn borrowed_bar(set: &RankedSet) -> (Vec<EventGroup>, Vec<EventGroup>) {
        let planned = plan_candidate_chain(set).expect("plannable");
        assert_eq!(
            planned
                .steps
                .iter()
                .map(|s| s.state.candidate)
                .collect::<Vec<_>>(),
            vec![0, 1, 0],
            "bar 1 is borrowed from candidate 1 — rests and spans are pitch-free \
             and must not have moved the plan",
        );
        let in_bar = |score: &Score| -> Vec<EventGroup> {
            score.tracks[0].voices[0]
                .event_groups
                .iter()
                .filter(|g| (BAR..BAR * 2).contains(&g.atoms[0].absolute_start().0))
                .cloned()
                .collect()
        };
        let source = in_bar(&set.ranked[1].value.score);
        let assembled = in_bar(&planned.score);
        (source, assembled)
    }

    #[test]
    fn a_borrowed_bar_arrives_as_the_exact_event_groups_it_left_as() {
        // The whole contract in one line: assembly copies, it does not edit.
        let set = rich_borrowed_bar_set();
        let (source, assembled) = borrowed_bar(&set);
        assert_eq!(
            assembled, source,
            "every group of the borrowed bar, verbatim and in order",
        );
    }

    #[test]
    fn a_borrowed_group_keeps_its_kind() {
        let set = rich_borrowed_bar_set();
        let (_, assembled) = borrowed_bar(&set);
        assert_eq!(
            assembled[0].kind,
            EventGroupKind::Chord,
            "a chord does not arrive as a single note",
        );
    }

    #[test]
    fn a_borrowed_group_keeps_its_rests() {
        // Rests carry no pitch, so nothing in the cost model would notice their
        // loss; the assembled bar would simply be shorter.
        let set = rich_borrowed_bar_set();
        let (_, assembled) = borrowed_bar(&set);
        assert_eq!(
            assembled[0].atoms.clone(),
            vec![
                assembled[0].atoms[0],
                AtomEvent::Rest(AtomRest {
                    absolute_start: Ticks(BAR + 480),
                    duration: Ticks(480),
                }),
            ],
            "the rest is an atom of the group, not decoration",
        );
    }

    #[test]
    fn a_borrowed_note_keeps_its_velocity_and_marks() {
        let set = rich_borrowed_bar_set();
        let (_, assembled) = borrowed_bar(&set);
        let AtomEvent::Note(note) = assembled[0].atoms[0] else {
            panic!("the first atom is a note");
        };
        assert_eq!(
            note.velocity,
            Velocity(37),
            "dynamics are not re-normalised"
        );
        assert!(note.marks.contains(NoteMark::Accent));
        assert!(note.marks.contains(NoteMark::Staccato));
    }

    #[test]
    fn a_borrowed_note_keeps_its_fretboard_position_and_evidence() {
        let set = rich_borrowed_bar_set();
        let (_, assembled) = borrowed_bar(&set);
        let AtomEvent::Note(note) = assembled[0].atoms[0] else {
            panic!("the first atom is a note");
        };
        let position = note.position.expect("the fixture's note is fretted");
        assert_eq!(position.position.string, 3);
        assert_eq!(position.position.fret, 11);
        assert_eq!(
            position.evidence,
            TechniqueEvidence::explicit(),
            "the position's evidence travels with it (ADR-0019)",
        );
    }

    #[test]
    fn a_borrowed_group_keeps_its_technique_spans_with_their_evidence() {
        let set = rich_borrowed_bar_set();
        let (_, assembled) = borrowed_bar(&set);
        let spans = &assembled[0].technique_spans;
        assert_eq!(spans.len(), 1, "the span is not dropped");
        assert_eq!(spans[0].technique, SpanTechnique::PalmMute);
        assert_eq!(
            spans[0].evidence,
            TechniqueEvidence::inferred(0.42),
            "an inferred span does not arrive as fact",
        );
    }

    #[test]
    fn a_borrowed_span_keeps_its_exact_tick_range() {
        // Not clamped to the bar, not re-based, not rounded to the grid.
        let set = rich_borrowed_bar_set();
        let (_, assembled) = borrowed_bar(&set);
        assert_eq!(
            assembled[0].technique_spans[0].tick_range,
            TickRange::new(Ticks(BAR), Ticks(BAR + 960)).unwrap(),
        );
    }

    #[test]
    fn every_group_of_every_selected_bar_is_assembled() {
        // Counted end to end: the assembled voice holds exactly the groups of
        // the selected bars — none dropped, none duplicated.
        let set = rich_borrowed_bar_set();
        let planned = plan_candidate_chain(&set).expect("plannable");
        let expected: usize = planned
            .steps
            .iter()
            .map(|step| {
                let source = &set.ranked[step.state.candidate].value.score;
                source.tracks[0].voices[0]
                    .event_groups
                    .iter()
                    .filter(|g| {
                        let onset = g.atoms[0].absolute_start().0;
                        let bar = u32::try_from(step.state.bar).unwrap();
                        onset >= bar * BAR && onset < (bar + 1) * BAR
                    })
                    .count()
            })
            .sum();
        assert_eq!(
            planned.score.tracks[0].voices[0].event_groups.len(),
            expected,
            "one assembled group per selected-bar group",
        );
        assert_eq!(expected, 3, "the fixture has one group per bar");
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
            .expect("valid bars")
            .get(AXIS_BOUNDARY_JUMP)
            .unwrap();
        let leap = transition_facts(&b, 0, &b, 1)
            .expect("valid bars")
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
        let facts = transition_facts(&s, 0, &s, 1).expect("valid bars");
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
        let facts = transition_facts(&s, 0, &s, 1).expect("valid bars");
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
            .expect("valid bars")
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
            .expect("valid bars")
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
            .expect("valid bars")
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
        let facts = transition_facts(&s, 0, &s, 1).expect("valid bars");
        assert!(
            (facts.get(AXIS_RHYTHM_REPEAT).unwrap() - 1.0).abs() < 1e-12,
            "two silences are a rhythmic repeat — an explicit signature, not a gap",
        );
        assert!((facts.get(AXIS_SILENT_BOUNDARY).unwrap() - 1.0).abs() < 1e-12);
    }

    // ── boundary facts: valid bars, and the last SOUNDING note ───────────────

    #[test]
    fn the_boundary_pitch_is_the_last_note_still_sounding_not_the_last_onset() {
        // Bar 0: pitch 60 rings the whole bar; pitch 48 starts later but stops
        // before the line. The note still sounding at the boundary is 60, so the
        // jump into bar 1's 60 is 0 — not the 12 a latest-onset rule would claim.
        let s = score_of(&[&[(0, BAR, 60), (BAR - 480, 240, 48)], &[(0, 480, 60)]]);
        let jump = transition_facts(&s, 0, &s, 1)
            .expect("valid bars")
            .get(AXIS_BOUNDARY_JUMP)
            .unwrap();
        assert!(
            (jump - 0.0).abs() < 1e-12,
            "the sustained note is what reaches the bar line, got {jump}",
        );
    }

    #[test]
    fn notes_ending_together_break_the_tie_on_the_highest_pitch() {
        // Both stop at the same tick; the chord top is the documented winner.
        let s = score_of(&[&[(0, 960, 60), (480, 480, 67)], &[(0, 480, 67)]]);
        let jump = transition_facts(&s, 0, &s, 1)
            .expect("valid bars")
            .get(AXIS_BOUNDARY_JUMP)
            .unwrap();
        assert!(
            (jump - 0.0).abs() < 1e-12,
            "the highest of the equally-last notes (67) reaches the line, got {jump}",
        );
    }

    #[test]
    fn an_invalid_from_bar_is_rejected_not_reported_as_silence() {
        let s = score_of(&[&[(0, 480, 60)], &[(0, 480, 62)]]);
        assert_eq!(
            transition_facts(&s, 7, &s, 1).unwrap_err(),
            TransitionFactError::MissingFromBar { bar: 7, bars: 2 },
        );
    }

    #[test]
    fn an_invalid_to_bar_is_rejected_not_reported_as_silence() {
        let s = score_of(&[&[(0, 480, 60)], &[(0, 480, 62)]]);
        assert_eq!(
            transition_facts(&s, 0, &s, 9).unwrap_err(),
            TransitionFactError::MissingToBar { bar: 9, bars: 2 },
        );
    }

    // ── planning, assembly, and the S6 baseline ──────────────────────────────

    /// Wraps ranked candidates into a `RankedSet` with a minimal base request.
    fn ranked_set(ranked: Vec<Scored<SetCandidate>>) -> RankedSet {
        RankedSet {
            ranked,
            base: RuleGenerationRequest {
                seed: GenerationSeed(1),
                pitch_material: PitchMaterial {
                    root: Pitch(60),
                    intervals: vec![0, 2, 4, 5, 7, 9, 11],
                },
                constraints: GenerationConstraints {
                    bar_count: 3,
                    time_signature: TimeSignature::new(4, 4).unwrap(),
                    tempo: Tempo::new(120.0).unwrap(),
                    ticks_per_quarter: Ticks(u32::from(PPQ)),
                    pitch_lo: Pitch(40),
                    pitch_hi: Pitch(90),
                },
                source_rhythms: Vec::new(),
                explicit_rhythms: None,
                strategy: GenerationStrategy::ShuffleMotifs,
            },
            source_rhythms: Vec::new(),
            rhythm_explicit: false,
            gesture: None,
            policy: s6_policy(),
        }
    }

    /// The non-greedy fixture.
    ///
    /// Every bar of every candidate is one note at offset 0 lasting 480 ticks,
    /// so the rhythm signature is identical everywhere and the repeat term is a
    /// constant that cannot sway the choice. Pitch alone decides:
    ///
    /// ```text
    ///            bar0   bar1   bar2    S6 aggregate → local cost
    ///   cand 0    60     50     84       0.9 → 0.1
    ///   cand 1    60     70     84       0.8 → 0.2
    ///   cand 2    60     50     84       0.7 → 0.3
    /// ```
    ///
    /// Candidate 0 is locally cheapest at every layer, so a greedy pass takes
    /// it throughout — but its bar 1 dives to 50 and must then climb 34
    /// semitones back to 84. Candidate 1's bar 1 costs 0.1 more locally and
    /// sits at 70, on the way, for a far cheaper climb. Only downstream
    /// transition cost reveals this, which is exactly what DP sees and greedy
    /// does not.
    fn non_greedy_set() -> RankedSet {
        ranked_set(vec![
            candidate(
                score_of(&[&[(0, 480, 60)], &[(0, 480, 50)], &[(0, 480, 84)]]),
                0.9,
                1,
            ),
            candidate(
                score_of(&[&[(0, 480, 60)], &[(0, 480, 70)], &[(0, 480, 84)]]),
                0.8,
                2,
            ),
            candidate(
                score_of(&[&[(0, 480, 60)], &[(0, 480, 50)], &[(0, 480, 84)]]),
                0.7,
                3,
            ),
        ])
    }

    #[test]
    fn the_planned_chain_beats_the_intact_s6_winner_on_the_non_greedy_fixture() {
        let set = non_greedy_set();
        let planned = plan_candidate_chain(&set).expect("plans");
        let baseline = intact_s6_cost(&set).expect("baseline");

        assert_eq!(
            planned
                .steps
                .iter()
                .map(|s| s.state.candidate)
                .collect::<Vec<_>>(),
            vec![0, 1, 0],
            "DP leaves the locally-best candidate at bar 1 for the cheaper climb",
        );
        // Intact:  locals 0.3 + jumps 0.05*(10 + 34) = 2.2 + repeats 0.8 = 3.3
        // Planned: locals 0.4 + jumps 0.05*(10 + 14) = 1.2 + repeats 0.8 = 2.4
        assert!((baseline - 3.3).abs() < 1e-9, "baseline was {baseline}");
        assert!(
            (planned.total_cost - 2.4).abs() < 1e-9,
            "planned was {}",
            planned.total_cost,
        );
        assert!(
            planned.total_cost < baseline,
            "the global path is strictly cheaper under the same policy",
        );
    }

    // ── the baseline is an evaluation by the same engine ─────────────────────

    #[test]
    fn a_finite_baseline_keeps_the_total_it_always_had() {
        // Two bars of one candidate: locals 2 x (1 - 0.9) = 0.2, a 2-semitone
        // step at 0.05 = 0.1, an identical rhythm either side at 0.40. Routing
        // the baseline through the solver must not move this number.
        let set = ranked_set(vec![candidate(
            score_of(&[&[(0, 480, 60)], &[(0, 480, 62)]]),
            0.9,
            1,
        )]);
        let baseline = intact_s6_cost(&set).expect("baseline");
        assert!((baseline - 0.7).abs() < 1e-9, "baseline was {baseline}");
    }

    #[test]
    fn a_non_finite_baseline_local_is_a_typed_path_error() {
        // An infinite S6 aggregate makes `1 - s6` infinite. The planned chain
        // has always named this; the baseline used to sum it into an f64 and
        // hand back Ok(-inf) — a "cost" that compares less than every real one,
        // so the S6 winner would look unbeatable.
        let set = ranked_set(vec![candidate(
            score_of(&[&[(0, 480, 60)], &[(0, 480, 62)]]),
            f64::INFINITY,
            1,
        )]);
        assert_eq!(
            intact_s6_cost(&set).unwrap_err(),
            ChainError::Path(PathError::NonFiniteLocal {
                state: StateId {
                    layer: 0,
                    ordinal: 0,
                },
                cost: f64::NEG_INFINITY,
            }),
        );
    }

    #[test]
    fn an_overflowing_baseline_accumulation_is_a_typed_path_error() {
        // Every local is finite (1e308), their sum is not. The baseline's own
        // arithmetic checked nothing and returned Ok(inf).
        let set = ranked_set(vec![candidate(
            score_of(&[&[(0, 480, 60)], &[(0, 480, 62)]]),
            -1e308,
            1,
        )]);
        assert_eq!(
            intact_s6_cost(&set).unwrap_err(),
            ChainError::Path(PathError::NonFiniteAccumulation {
                state: StateId {
                    layer: 0,
                    ordinal: 0,
                },
                cost: f64::INFINITY,
            }),
        );
    }

    #[test]
    fn the_baseline_is_the_planned_chain_of_the_candidate_0_set() {
        // The comparison's whole meaning is that both sides are one metric. The
        // baseline is a one-state-per-layer path, so it must be *that path's*
        // cost as this engine computes it — bit for bit, not merely close.
        //
        // With one candidate every local is the same number, so no fixture can
        // make the two associations disagree on a value here; this law is
        // structural on purpose, and the two typed-error laws above are what
        // prove the baseline really goes through the solver.
        let set = non_greedy_set();
        let alone = ranked_set(vec![set.ranked[0].clone()]);
        let baseline = intact_s6_cost(&set).expect("baseline");
        let planned = plan_candidate_chain(&alone).expect("plans");
        assert_eq!(
            baseline.to_bits(),
            planned.total_cost.to_bits(),
            "the baseline is the intact candidate's path cost, folded once",
        );
    }

    #[test]
    fn the_locally_best_state_at_a_layer_is_not_the_one_dp_takes() {
        // The non-greedy property, stated directly: candidate 0 has the lowest
        // local cost at bar 1, yet the optimum does not take it there.
        let set = non_greedy_set();
        let bar1_locals: Vec<f64> = set
            .ranked
            .iter()
            .map(|c| {
                local_facts(c.aggregate())
                    .get(AXIS_CANDIDATE_QUALITY)
                    .unwrap()
            })
            .collect();
        let cheapest_local = bar1_locals
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i)
            .unwrap();
        assert_eq!(cheapest_local, 0, "candidate 0 is the locally best state");
        let planned = plan_candidate_chain(&set).expect("plans");
        assert_ne!(
            planned.steps[1].state.candidate, cheapest_local,
            "DP rejects the locally best state because of downstream cost",
        );
    }

    #[test]
    fn zero_transition_weights_reproduce_the_intact_s6_winner() {
        let set = non_greedy_set();
        let only_local =
            WeightPolicy::new("chain_local_only", 1, vec![(AXIS_CANDIDATE_QUALITY, 1.0)]);
        let planned = plan_ranked_with(&set.ranked, &only_local).expect("plans");
        assert_eq!(
            planned
                .steps
                .iter()
                .map(|s| s.state.candidate)
                .collect::<Vec<_>>(),
            vec![0, 0, 0],
            "with no transition terms the chain is S6's winner, intact",
        );
    }

    #[test]
    fn each_step_keeps_its_candidates_identity_and_s6_score() {
        let set = non_greedy_set();
        let planned = plan_candidate_chain(&set).expect("plans");
        let step = &planned.steps[1];
        assert_eq!(step.state.bar, 1);
        assert_eq!(step.state.candidate, 1);
        assert_eq!(step.state.rank, 2, "the original 1-based rank");
        assert_eq!(step.state.variant_seed, GenerationSeed(2));
        assert!(
            (step.s6.aggregate - 0.8).abs() < 1e-12,
            "the untouched S6 aggregate travels with the step",
        );
        assert_eq!(step.s6.provenance.policy_id, "test_rerank");
        assert!(
            !step.s6.rationale.entries().is_empty(),
            "S6's rationale is kept"
        );
    }

    #[test]
    fn the_contributions_sum_to_the_reported_total() {
        let planned = plan_candidate_chain(&non_greedy_set()).expect("plans");
        let summed: f64 = planned
            .steps
            .iter()
            .map(|s| s.local.aggregate())
            .sum::<f64>()
            + planned
                .transitions
                .iter()
                .map(|t| t.cost.aggregate())
                .sum::<f64>();
        assert!((summed - planned.total_cost).abs() < 1e-12);
    }

    #[test]
    fn planning_never_reorders_or_changes_the_ranked_set() {
        let set = non_greedy_set();
        let before: Vec<(u64, usize)> = set
            .ranked
            .iter()
            .map(|c| (c.value.seed.0, c.value.score.master_bars.len()))
            .collect();
        drop(plan_candidate_chain(&set).expect("plans"));
        let after: Vec<(u64, usize)> = set
            .ranked
            .iter()
            .map(|c| (c.value.seed.0, c.value.score.master_bars.len()))
            .collect();
        assert_eq!(before, after, "the set is read, never rewritten");
    }

    #[test]
    fn the_output_keeps_the_bar_count_duration_and_master_timeline() {
        let set = non_greedy_set();
        let planned = plan_candidate_chain(&set).expect("plans");
        let reference = &set.ranked[0].value.score;
        assert_eq!(
            planned.score.master_bars.len(),
            3,
            "the requested bar count"
        );
        assert_eq!(planned.score.ticks_per_quarter, PPQ);
        assert_eq!(
            planned
                .score
                .master_bars
                .iter()
                .map(|b| b.tick_range)
                .collect::<Vec<_>>(),
            reference
                .master_bars
                .iter()
                .map(|b| b.tick_range)
                .collect::<Vec<_>>(),
            "the master timeline is the one every candidate already agreed on",
        );
        for bar in &planned.score.master_bars {
            assert!(
                (bar.tempo.0 - 120.0).abs() < 1e-12,
                "tempo is the timeline's"
            );
            assert_eq!(bar.time_signature, TimeSignature::new(4, 4).unwrap());
        }
    }

    #[test]
    fn every_selected_bar_is_an_exact_snapshot_of_its_source_candidate() {
        let set = non_greedy_set();
        let planned = plan_candidate_chain(&set).expect("plans");
        // The planned path is [0, 1, 0] → pitches 60, 70, 84 with no note lost.
        let notes: Vec<(u32, u32, u8)> = planned
            .score
            .tracks
            .iter()
            .flat_map(|t| t.voices.iter())
            .flat_map(|v| v.event_groups.iter())
            .flat_map(|g| g.atoms.iter())
            .filter_map(|a| match a {
                AtomEvent::Note(n) => Some((n.absolute_start.0, n.duration.0, n.pitch.0)),
                AtomEvent::Rest(_) => None,
            })
            .collect();
        assert_eq!(
            notes,
            vec![(0, 480, 60), (BAR, 480, 70), (2 * BAR, 480, 84)],
            "onsets, durations and pitches survive assembly exactly",
        );
    }

    #[test]
    fn a_real_ranked_candidates_fixture_plans_deterministically() {
        // A real fixed-seed S6 pass, not a hand-built set.
        let source = score_of(&[
            &[
                (0, 480, 60),
                (960, 480, 62),
                (1920, 480, 64),
                (2880, 480, 65),
            ],
            &[
                (0, 480, 67),
                (960, 480, 65),
                (1920, 480, 64),
                (2880, 480, 62),
            ],
        ]);
        let ask = GenerationAsk {
            seed: 42,
            bars: 4,
            variants_per_strategy: 2,
            gesture: false,
        };
        let set = ranked_candidates(&source, None, &ask, None).expect("the pass runs");
        let a = plan_candidate_chain(&set).expect("plans");
        let b = plan_candidate_chain(&set).expect("plans again");

        assert_eq!(a.score.master_bars.len(), 4, "the requested bars");
        assert_eq!(
            a.steps
                .iter()
                .map(|s| s.state.candidate)
                .collect::<Vec<_>>(),
            b.steps
                .iter()
                .map(|s| s.state.candidate)
                .collect::<Vec<_>>(),
            "the same input plans the same path, every time",
        );
        assert!((a.total_cost - b.total_cost).abs() < 1e-12);
        assert_eq!(a.steps.len(), 4, "one step per bar");
        assert_eq!(a.transitions.len(), 3, "one transition per bar line");
        for step in &a.steps {
            assert!(
                !step.local.rationale.entries().is_empty(),
                "local explained"
            );
            assert!(!step.s6.rationale.entries().is_empty(), "S6 kept");
        }
        for t in &a.transitions {
            assert!(
                !t.cost.rationale.entries().is_empty(),
                "transition explained"
            );
        }
        assert_eq!(a.provenance.policy_id, "candidate_chain");
        assert_eq!(a.provenance.policy_version, 1);
    }
}
