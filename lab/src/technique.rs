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

/// A chain state: this note's candidate, the other hand's last candidate, and
/// the inferred strings chosen for legato origins whose targets are later.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TechniqueState {
    own: usize,
    other: Option<usize>,
    active_strings: Vec<u8>,
}

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
        &[],
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
    direction_between(pitches, i.checked_sub(1)?, i)
}

/// Pitch direction from `from` to `to`; `None` for an invalid or non-forward
/// relation.
#[must_use]
pub fn direction_between(pitches: &[Pitch], from: usize, to: usize) -> Option<LegatoDirection> {
    if from >= to {
        return None;
    }
    let before = pitches.get(from)?;
    let here = pitches.get(to)?;
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
/// `None` when `pitches` or `tapped` do not have one entry per position, or an
/// edge does not name a forward pair inside the line.
#[must_use]
pub fn technique_cost(
    line: &[FretboardPosition],
    pitches: &[Pitch],
    tapped: &[bool],
    edges: &[TechniqueEdge],
    objective: &TechniqueObjective,
) -> Option<i64> {
    if pitches.len() != line.len()
        || tapped.len() != line.len()
        || edges
            .iter()
            .any(|edge| edge.from >= edge.to || edge.to >= line.len())
    {
        return None;
    }
    let mut total = tap_aware_cost(line, tapped, &objective.weights, objective.tap_shift)?;
    for edge in edges {
        let descending =
            direction_between(pitches, edge.from, edge.to) == Some(LegatoDirection::Descending);
        total = total.saturating_add(legato_edge_cost(
            objective,
            descending,
            line[edge.from],
            line[edge.to],
        ));
    }
    Some(total)
}

/// What `objective`'s legato terms add on an `edge` from `before` to `here`
/// (`descending` when the pitch falls): the continuity cost when a legato edge
/// changes string, minus the open-string penalty the pull-off waiver lifts
/// from `here`.
fn legato_edge_cost(
    objective: &TechniqueObjective,
    descending: bool,
    before: FretboardPosition,
    here: FretboardPosition,
) -> i64 {
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
/// Per note the chain's states carry the note's candidate, the candidate of
/// the most recent note played by the *other* hand, and the candidate string
/// of every still-open legato origin. The latter is the minimal frontier state
/// needed for a relation that may skip intervening voice onsets.
///
/// # Errors
///
/// [`LabError::EmptyLine`] for no pitches; [`LabError::UnpositionablePitch`]
/// when a pitch has no candidate at or below `max_fret`;
/// [`LabError::LabelLength`] when `tapped` does not have one entry per pitch;
/// [`LabError::InvalidTechniqueEdge`] for an edge outside the line or not
/// directed forward.
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
    if tapped.len() != pitches.len() {
        return Err(LabError::LabelLength {
            notes: pitches.len(),
            labels: tapped.len(),
        });
    }
    if let Some(edge) = edges
        .iter()
        .find(|edge| edge.from >= edge.to || edge.to >= pitches.len())
    {
        return Err(LabError::InvalidTechniqueEdge {
            notes: pitches.len(),
            from: edge.from,
            to: edge.to,
        });
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

    // Legato frontier after each note: edges already opened but not yet closed.
    let active: Vec<Vec<usize>> = (0..n)
        .map(|i| {
            edges
                .iter()
                .enumerate()
                .filter_map(|(edge_index, edge)| {
                    (edge.from <= i && i < edge.to).then_some(edge_index)
                })
                .collect()
        })
        .collect();

    // States: this note's candidate, the other hand's last candidate, and one
    // inferred string per active legato origin (in `active[i]` order).
    let states: Vec<Vec<TechniqueState>> = (0..n)
        .map(|i| {
            let others: Vec<Option<usize>> = match other_note[i] {
                None => vec![None],
                Some(o) => (0..candidates[o].len()).map(Some).collect(),
            };
            (0..candidates[i].len())
                .flat_map(|own| {
                    let domains: Vec<Vec<u8>> = active[i]
                        .iter()
                        .map(|&edge_index| {
                            let edge = edges[edge_index];
                            if edge.from == i {
                                vec![candidates[i][own].string]
                            } else {
                                let mut strings: Vec<u8> = candidates[edge.from]
                                    .iter()
                                    .map(|position| position.string)
                                    .collect();
                                strings.sort_unstable();
                                strings.dedup();
                                strings
                            }
                        })
                        .collect();
                    string_products(&domains).into_iter().flat_map({
                        let others = others.clone();
                        move |active_strings| {
                            others.clone().into_iter().map(move |other| TechniqueState {
                                own,
                                other,
                                active_strings: active_strings.clone(),
                            })
                        }
                    })
                })
                .collect()
        })
        .collect();

    let positions = states
        .iter()
        .enumerate()
        .map(|(i, layer)| layer.iter().map(|state| candidates[i][state.own]).collect())
        .collect();
    let unary = states
        .iter()
        .enumerate()
        .map(|(i, layer)| {
            layer
                .iter()
                .map(|state| v1_unary(candidates[i][state.own].fret, weights))
                .collect()
        })
        .collect();
    let pairwise = (0..n)
        .map(|i| {
            let Some(previous) = i.checked_sub(1) else {
                return Vec::new();
            };
            states[previous]
                .iter()
                .map(|previous_state| {
                    states[i]
                        .iter()
                        .map(|state| {
                            // The same hand keeps the other hand's carried
                            // candidate; a hand switch hands over note i - 1.
                            let (admissible, prev_same) = if tapped[i] == tapped[previous] {
                                (
                                    state.other == previous_state.other,
                                    Some((previous, previous_state.own)),
                                )
                            } else {
                                (
                                    state.other == Some(previous_state.own),
                                    other_note[previous].zip(previous_state.other),
                                )
                            };
                            if !admissible {
                                return INADMISSIBLE;
                            }
                            // Every still-open origin must carry the same
                            // inferred string through this transition.
                            for (slot, &edge_index) in active[i].iter().enumerate() {
                                let edge = edges[edge_index];
                                if edge.from == i {
                                    if state.active_strings[slot] != candidates[i][state.own].string
                                    {
                                        return INADMISSIBLE;
                                    }
                                } else {
                                    let Some(previous_slot) = active[previous]
                                        .iter()
                                        .position(|&candidate| candidate == edge_index)
                                    else {
                                        return INADMISSIBLE;
                                    };
                                    if state.active_strings[slot]
                                        != previous_state.active_strings[previous_slot]
                                    {
                                        return INADMISSIBLE;
                                    }
                                }
                            }

                            let here = candidates[i][state.own];
                            let before = candidates[previous][previous_state.own];
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
                            for (edge_index, edge) in edges.iter().enumerate() {
                                if edge.to != i {
                                    continue;
                                }
                                let Some(previous_slot) = active[previous]
                                    .iter()
                                    .position(|&candidate| candidate == edge_index)
                                else {
                                    return INADMISSIBLE;
                                };
                                let origin = FretboardPosition {
                                    string: previous_state.active_strings[previous_slot],
                                    fret: 0,
                                };
                                let descending = direction_between(pitches, edge.from, edge.to)
                                    == Some(LegatoDirection::Descending);
                                cost = cost.saturating_add(legato_edge_cost(
                                    objective, descending, origin, here,
                                ));
                            }
                            cost
                        })
                        .collect()
                })
                .collect()
        })
        .collect();
    Ok(Chain::from_parts(positions, unary, pairwise))
}

fn string_products(domains: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let mut products = vec![Vec::new()];
    for domain in domains {
        let mut next = Vec::new();
        for prefix in &products {
            for &value in domain {
                let mut product = prefix.clone();
                product.push(value);
                next.push(product);
            }
        }
        products = next;
    }
    products
}
