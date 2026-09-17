//! Technique-aware fingering objectives — oracle stage.
//!
//! The optimality-gap and tie-break audits measured the `v1` objective as if
//! every note were fretted by the fretting hand, so a note's position was the
//! hand's position. Tapping breaks that: a tapped note is played by the picking
//! hand while the fretting hand stays where it was. Lines with tapped notes were
//! the objective's worst slice (on the whole corpus, no such line's human path
//! was in the model's optimum set).
//!
//! This module asks whether telling the model the truth about which hand plays
//! each note explains that slice. The technique labels are taken from the tab
//! (`TabLine::tapped`), not inferred; inference is a later stage.

use griff_core::event::{FretboardPosition, Pitch, Tuning};
use griff_core::fretboard::FingeringWeights;

use crate::fingering::{v1_unary, TechniqueEdge};
use crate::problems::LabError;
use crate::ties::Chain;

/// Cost of an inadmissible chain transition: far above any real line cost, yet
/// small enough that saturating sums of a line's worth of them stay ordered.
const INADMISSIBLE: i64 = i64::MAX / 4;

/// A chain state: this note's candidate and the other hand's last candidate.
type TapState = (usize, Option<usize>);

/// The `v1` objective with tapped notes attributed to the picking hand:
///
/// - per note, the `v1` unary cost (`fret·fret − [open]·open_string`);
/// - between consecutive notes, `string_change` when the string changes;
/// - fretting-hand travel: each untapped note pays `position_shift ·
///   |Δfret|` from the previous **untapped** note (the anchor carries across
///   taps);
/// - picking-hand travel: each tapped note pays `tap_shift · |Δfret|` from the
///   previous **tapped** note.
///
/// With no tapped notes it equals `fingering::v1_cost`. `None` when `tapped`
/// does not have one flag per position.
#[must_use]
pub fn tap_aware_cost(
    line: &[FretboardPosition],
    tapped: &[bool],
    weights: &FingeringWeights,
    tap_shift: i64,
) -> Option<i64> {
    if line.len() != tapped.len() {
        return None;
    }
    let mut total = 0_i64;
    let mut last_fretted: Option<FretboardPosition> = None;
    let mut last_tapped: Option<FretboardPosition> = None;
    let mut previous: Option<FretboardPosition> = None;
    for (&position, &tap) in line.iter().zip(tapped) {
        total = total.saturating_add(v1_unary(position.fret, weights));
        if previous.is_some_and(|p| p.string != position.string) {
            total = total.saturating_add(weights.string_change);
        }
        let (last, weight) = if tap {
            (&mut last_tapped, tap_shift)
        } else {
            (&mut last_fretted, weights.position_shift)
        };
        if let Some(q) = *last {
            total = total
                .saturating_add(weight.saturating_mul(i64::from(q.fret.abs_diff(position.fret))));
        }
        *last = Some(position);
        previous = Some(position);
    }
    Some(total)
}

/// [`tap_aware_cost`] as a [`Chain`], so the exact optimum-set DPs apply: the
/// [`technique_chain`] of [`TechniqueObjective::tap_aware`] with no legato
/// edges.
///
/// # Errors
///
/// [`LabError::EmptyLine`] for no pitches; [`LabError::UnpositionablePitch`]
/// when a pitch has no candidate at or below `max_fret`;
/// [`LabError::LabelLength`] when `tapped` does not have one flag per pitch.
// The v1 builder's inputs plus the tap weight and labels; a parameter struct
// would only rename them.
#[allow(clippy::too_many_arguments)]
pub fn tap_aware_chain(
    pitches: &[Pitch],
    tuning: &Tuning,
    weights: &FingeringWeights,
    tap_shift: i64,
    tapped: &[bool],
    max_fret: u8,
) -> Result<Chain, LabError> {
    technique_chain(
        pitches,
        tuning,
        tapped,
        &vec![TechniqueEdge::Plain; pitches.len()],
        &TechniqueObjective::tap_aware(*weights, tap_shift),
        max_fret,
    )
}

// ── stage 2: legato continuity ────────────────────────────────────────────────

/// Pitch direction into note `i` — **derived** evidence, not an imported
/// label: the import gives a legato origin without its direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LegatoDirection {
    /// Higher pitch than the previous note (a hammer-on candidate).
    Ascending,
    /// Lower pitch than the previous note (a pull-off candidate).
    Descending,
    /// The same pitch.
    Unison,
}

/// The pitch direction from note `i − 1` to note `i`; `None` for note 0 or
/// out of range.
#[must_use]
pub fn derived_direction(pitches: &[Pitch], i: usize) -> Option<LegatoDirection> {
    let before = pitches.get(i.checked_sub(1)?)?;
    let here = pitches.get(i)?;
    Some(match here.0.cmp(&before.0) {
        std::cmp::Ordering::Greater => LegatoDirection::Ascending,
        std::cmp::Ordering::Less => LegatoDirection::Descending,
        std::cmp::Ordering::Equal => LegatoDirection::Unison,
    })
}

/// Cost of one cross-string legato edge under [`Continuity::Hard`]: far above
/// any real line cost, so an optimum first minimizes such edges, then the rest
/// of the objective; small enough that a line's worth of them cannot overflow.
pub const HARD_VIOLATION: i64 = 1 << 32;

/// How a legato edge binds its two notes to one string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Continuity {
    /// No continuity term.
    Off,
    /// Each cross-string legato edge costs [`HARD_VIOLATION`].
    Hard,
    /// Each cross-string legato edge costs `k · position_shift`.
    Soft {
        /// Penalty in frets of hand travel.
        k: i64,
    },
}

/// The tap-aware objective plus the stage-2 legato terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TechniqueObjective {
    /// The `v1` weights.
    pub weights: FingeringWeights,
    /// Picking-hand travel weight per fret.
    pub tap_shift: i64,
    /// Same-string continuity across legato edges.
    pub continuity: Continuity,
    /// The target of a legato edge with a derived descending direction (a
    /// pull-off candidate) pays no open-string penalty: its open-string term
    /// becomes `min(−open_string, 0)`, so a bonus is untouched.
    pub pull_open_waiver: bool,
}

impl TechniqueObjective {
    /// Stage 1's tap-aware objective: no legato terms.
    #[must_use]
    pub const fn tap_aware(weights: FingeringWeights, tap_shift: i64) -> Self {
        Self {
            weights,
            tap_shift,
            continuity: Continuity::Off,
            pull_open_waiver: false,
        }
    }
}

/// [`tap_aware_cost`] plus the legato terms of `objective` over `edges`.
///
/// `None` when `pitches`, `tapped` or `edges` do not have one entry per
/// position.
#[must_use]
pub fn technique_cost(
    line: &[FretboardPosition],
    pitches: &[Pitch],
    tapped: &[bool],
    edges: &[TechniqueEdge],
    objective: &TechniqueObjective,
) -> Option<i64> {
    if pitches.len() != line.len() || edges.len() != line.len() {
        return None;
    }
    let mut total = tap_aware_cost(line, tapped, &objective.weights, objective.tap_shift)?;
    for (before, (&here, (into, &edge))) in line
        .iter()
        .zip(line.iter().zip(edges.iter().enumerate()).skip(1))
    {
        let descending = derived_direction(pitches, into) == Some(LegatoDirection::Descending);
        total = total.saturating_add(legato_edge_cost(objective, edge, descending, *before, here));
    }
    Some(total)
}

/// What `objective`'s legato terms add on an `edge` from `before` to `here`
/// (`descending` when the pitch falls): the continuity cost when a legato edge
/// changes string, minus the open-string penalty the pull-off waiver lifts
/// from `here`.
fn legato_edge_cost(
    objective: &TechniqueObjective,
    edge: TechniqueEdge,
    descending: bool,
    before: FretboardPosition,
    here: FretboardPosition,
) -> i64 {
    if !edge.is_legato() {
        return 0;
    }
    let continuity = if before.string == here.string {
        0
    } else {
        match objective.continuity {
            Continuity::Off => 0,
            Continuity::Hard => HARD_VIOLATION,
            Continuity::Soft { k } => k.saturating_mul(objective.weights.position_shift),
        }
    };
    let waived = if objective.pull_open_waiver && descending && here.fret == 0 {
        // `v1_unary(0)` is `−open_string`: lift a penalty, keep a bonus.
        v1_unary(0, &objective.weights).max(0)
    } else {
        0
    };
    continuity.saturating_sub(waived)
}

/// [`technique_cost`] as a [`Chain`], so the exact optimum-set DPs apply.
///
/// Per note the chain's states pair the note's candidate with the candidate of
/// the most recent note played by the *other* hand (or none yet); a transition
/// is admissible only when that carried candidate agrees with the previous
/// state, so state paths and position assignments correspond one to one.
/// Inadmissible transitions carry a cost no optimal path can take. The legato
/// terms of an edge depend only on its two notes' candidates, so they are
/// part of the transition cost.
///
/// # Errors
///
/// [`LabError::EmptyLine`] for no pitches; [`LabError::UnpositionablePitch`]
/// when a pitch has no candidate at or below `max_fret`;
/// [`LabError::LabelLength`] when `tapped` or `edges` do not have one entry
/// per pitch.
// The chain builder's inputs plus the technique labels; a parameter struct
// would only rename them.
#[allow(clippy::too_many_arguments)]
pub fn technique_chain(
    pitches: &[Pitch],
    tuning: &Tuning,
    tapped: &[bool],
    edges: &[TechniqueEdge],
    objective: &TechniqueObjective,
    max_fret: u8,
) -> Result<Chain, LabError> {
    if pitches.is_empty() {
        return Err(LabError::EmptyLine);
    }
    for labels in [tapped.len(), edges.len()] {
        if labels != pitches.len() {
            return Err(LabError::LabelLength {
                notes: pitches.len(),
                labels,
            });
        }
    }
    let weights = &objective.weights;
    let tap_shift = objective.tap_shift;
    let candidates = pitches
        .iter()
        .enumerate()
        .map(|(index, &pitch)| {
            let c = tuning.candidates(pitch, max_fret);
            if c.is_empty() {
                Err(LabError::UnpositionablePitch {
                    index,
                    pitch: pitch.0,
                })
            } else {
                Ok(c)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    let n = pitches.len();

    // The latest note before `i` played by the other hand.
    let mut other_note: Vec<Option<usize>> = Vec::with_capacity(n);
    let mut latest = [None, None];
    for (i, &tap) in tapped.iter().enumerate() {
        let hand = usize::from(tap);
        other_note.push(latest[1 - hand]);
        latest[hand] = Some(i);
    }

    // States: (this note's candidate, the other hand's last candidate).
    let states: Vec<Vec<TapState>> = (0..n)
        .map(|i| {
            let others: Vec<Option<usize>> = match other_note[i] {
                None => vec![None],
                Some(o) => (0..candidates[o].len()).map(Some).collect(),
            };
            (0..candidates[i].len())
                .flat_map(|c| others.iter().map(move |&o| (c, o)))
                .collect()
        })
        .collect();

    let positions = states
        .iter()
        .enumerate()
        .map(|(i, layer)| layer.iter().map(|&(c, _)| candidates[i][c]).collect())
        .collect();
    let unary = states
        .iter()
        .enumerate()
        .map(|(i, layer)| {
            layer
                .iter()
                .map(|&(c, _)| v1_unary(candidates[i][c].fret, weights))
                .collect()
        })
        .collect();
    let pairwise = (0..n)
        .map(|i| {
            let Some(previous) = i.checked_sub(1) else {
                return Vec::new();
            };
            let descending = derived_direction(pitches, i) == Some(LegatoDirection::Descending);
            states[previous]
                .iter()
                .map(|&(own_prev, other_prev)| {
                    states[i]
                        .iter()
                        .map(|&(own, other)| {
                            // The same hand keeps the other hand's carried
                            // candidate; a hand switch hands over note i − 1.
                            let (admissible, prev_same) = if tapped[i] == tapped[previous] {
                                (other == other_prev, Some((previous, own_prev)))
                            } else {
                                (
                                    other == Some(own_prev),
                                    other_note[previous].zip(other_prev),
                                )
                            };
                            if !admissible {
                                return INADMISSIBLE;
                            }
                            let here = candidates[i][own];
                            let before = candidates[previous][own_prev];
                            let mut cost = if here.string == before.string {
                                0
                            } else {
                                weights.string_change
                            };
                            if let Some((note, cand)) = prev_same {
                                let weight = if tapped[i] {
                                    tap_shift
                                } else {
                                    weights.position_shift
                                };
                                cost = cost.saturating_add(weight.saturating_mul(i64::from(
                                    candidates[note][cand].fret.abs_diff(here.fret),
                                )));
                            }
                            cost.saturating_add(legato_edge_cost(
                                objective, edges[i], descending, before, here,
                            ))
                        })
                        .collect()
                })
                .collect()
        })
        .collect();
    Ok(Chain::from_parts(positions, unary, pairwise))
}
