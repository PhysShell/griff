//! Exact Lab-only chord-event representation regimes.
//!
//! Target chord positions are deliberately absent from [`ChordEventProblem`].
//! They enter only through [`ObservedChordVoicing`] evaluation.

use std::collections::{BTreeMap, BTreeSet};

use griff_core::event::{FretboardPosition, Pitch, Tuning};
use griff_core::fretboard::FingeringWeights;
use thiserror::Error;

use crate::fingering::v1_unary;

/// Stable identity of one chord onset.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChordEventIdentity {
    /// Repository-external source identity.
    pub source: String,
    /// Normalized song identity used for blocking.
    pub song_key: String,
    /// Track index.
    pub track: usize,
    /// Imported voice id.
    pub voice: u8,
    /// Absolute onset tick.
    pub onset: u32,
}

/// One solver-visible chord atom. It contains no imported target position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChordEventAtom {
    /// Stable imported-voice note id.
    pub note_id: usize,
    /// Imported pitch.
    pub pitch: Pitch,
    /// Imported duration in ticks.
    pub duration: u32,
    /// Whether the atom is tapped.
    pub tapped: bool,
}

/// Latest preceding fretting-hand anchor under #199/#210 semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HandAnchor {
    /// Anchor fret.
    pub fret: u8,
    /// Onset that established it.
    pub onset: u32,
    /// Stable source note id when practical.
    pub source_note_id: Option<usize>,
}

/// Technique kind retained by the representation experiment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TechniqueKind {
    /// Guitar Pro legato origin (direction may be derived separately).
    Legato,
    /// Hammer-on when explicitly known.
    HammerOn,
    /// Pull-off when explicitly known.
    PullOff,
}

/// Earlier observed context targeting one stable chord atom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IncomingTechnique {
    /// Technique kind.
    pub kind: TechniqueKind,
    /// Stable origin id.
    pub origin_note_id: usize,
    /// Origin onset.
    pub origin_onset: u32,
    /// Origin pitch.
    pub origin_pitch: Pitch,
    /// Earlier imported position; this is input context, not the target answer.
    pub origin_position: FretboardPosition,
    /// Stable target atom id inside this chord.
    pub target_atom_id: usize,
}

/// Solver input shared by the four registered information regimes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChordEventProblem {
    identity: ChordEventIdentity,
    tuning: Tuning,
    max_fret: u8,
    atoms: Vec<ChordEventAtom>,
    preceding_hand: Option<HandAnchor>,
    incoming_techniques: Vec<IncomingTechnique>,
}

impl ChordEventProblem {
    /// Builds a validated solver input containing no imported target positions.
    ///
    /// # Errors
    ///
    /// Refuses non-chords, duplicate atom ids, missing targets, and origins
    /// that do not strictly precede the chord.
    #[allow(clippy::too_many_arguments)] // explicit registered input channels
    pub fn new(
        identity: ChordEventIdentity,
        tuning: Tuning,
        max_fret: u8,
        atoms: Vec<ChordEventAtom>,
        preceding_hand: Option<HandAnchor>,
        incoming_techniques: Vec<IncomingTechnique>,
    ) -> Result<Self, ChordEventError> {
        if atoms.len() < 2 {
            return Err(ChordEventError::NotAChord);
        }
        let mut ids = BTreeSet::new();
        for atom in &atoms {
            if !ids.insert(atom.note_id) {
                return Err(ChordEventError::DuplicateAtomId(atom.note_id));
            }
        }
        for incoming in &incoming_techniques {
            if !ids.contains(&incoming.target_atom_id) {
                return Err(ChordEventError::MissingTechniqueTarget(
                    incoming.target_atom_id,
                ));
            }
            if incoming.origin_onset >= identity.onset {
                return Err(ChordEventError::NonPrecedingTechnique {
                    origin: incoming.origin_onset,
                    chord: identity.onset,
                });
            }
        }
        Ok(Self {
            identity,
            tuning,
            max_fret,
            atoms,
            preceding_hand,
            incoming_techniques,
        })
    }

    /// Event identity.
    #[must_use]
    pub const fn identity(&self) -> &ChordEventIdentity {
        &self.identity
    }

    /// Solver-visible atoms.
    #[must_use]
    pub fn atoms(&self) -> &[ChordEventAtom] {
        &self.atoms
    }

    /// Preceding anchor.
    #[must_use]
    pub const fn preceding_hand(&self) -> Option<HandAnchor> {
        self.preceding_hand
    }

    /// Incoming technique inputs.
    #[must_use]
    pub fn incoming_techniques(&self) -> &[IncomingTechnique] {
        &self.incoming_techniques
    }
}

/// One evaluation-only imported position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ObservedAtomPosition {
    /// Stable atom id.
    pub atom_id: usize,
    /// Imported position.
    pub position: FretboardPosition,
}

impl ObservedAtomPosition {
    /// Creates one observation.
    #[must_use]
    pub const fn new(atom_id: usize, position: FretboardPosition) -> Self {
        Self { atom_id, position }
    }
}

/// Evaluation object kept outside the solver input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedChordVoicing {
    positions: Vec<ObservedAtomPosition>,
}

impl ObservedChordVoicing {
    /// Builds an observation with unique stable atom ids.
    ///
    /// # Errors
    ///
    /// Refuses duplicate stable atom ids.
    pub fn new(positions: Vec<ObservedAtomPosition>) -> Result<Self, ChordEventError> {
        let mut ids = BTreeSet::new();
        for position in &positions {
            if !ids.insert(position.atom_id) {
                return Err(ChordEventError::DuplicateObservedAtomId(position.atom_id));
            }
        }
        Ok(Self { positions })
    }

    /// Imported positions.
    #[must_use]
    pub fn positions(&self) -> &[ObservedAtomPosition] {
        &self.positions
    }
}

/// One complete assignment in stable atom order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChordAssignment {
    /// `(stable atom id, position)` pairs.
    pub positions: Vec<ObservedAtomPosition>,
}

impl ChordAssignment {
    /// Finds the assigned position of a stable atom.
    #[must_use]
    pub fn position(&self, atom_id: usize) -> Option<FretboardPosition> {
        self.positions
            .iter()
            .find(|position| position.atom_id == atom_id)
            .map(|position| position.position)
    }
}

/// Saturating count with explicit loss-of-exactness state and log count.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AssignmentCount {
    /// Saturating count.
    pub value: u64,
    /// Whether `value` saturated.
    pub saturated: bool,
    /// Natural logarithm accumulated descriptively.
    pub ln: f64,
}

/// Conflicting technique requirements for one stable target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TechniqueConflict {
    /// Target atom.
    pub target_atom_id: usize,
    /// Distinct required strings.
    pub required_strings: Vec<u8>,
}

/// Evaluation of one exact assignment set.
#[derive(Debug, Clone, PartialEq)]
pub struct HumanSetMetrics {
    /// Whether the complete imported assignment belongs to the set.
    pub exact_membership: bool,
    /// Minimum stable-atom agreement fraction.
    pub floor: f64,
    /// Uniform expected agreement fraction.
    pub uniform: f64,
    /// Agreement of the deterministic first assignment.
    pub chosen: f64,
    /// Maximum agreement fraction.
    pub ceiling: f64,
}

/// Exact feasible-set result (R0 or R2).
#[derive(Debug, Clone, PartialEq)]
pub struct FeasibleRegime {
    /// Every admissible assignment in deterministic order.
    pub assignments: Vec<ChordAssignment>,
    /// Explicit count contract.
    pub admissible_count: AssignmentCount,
    /// Typed conflict, when the technique context is contradictory.
    pub conflict: Option<TechniqueConflict>,
    /// Human-set evaluation when supplied and complete.
    pub human: Option<HumanSetMetrics>,
    /// Frozen B0 optimum over this feasible set.
    pub b0: Option<ExactPreferredSet>,
}

/// Exact preferred subset under one primitive or lexicographic score.
#[derive(Debug, Clone, PartialEq)]
pub struct ExactPreferredSet {
    /// Primary optimum value.
    pub optimum: i64,
    /// Optional secondary optimum value.
    pub secondary: Option<i64>,
    /// Assignments attaining the optimum.
    pub assignments: Vec<ChordAssignment>,
    /// Explicit optimum-set count.
    pub optimum_count: AssignmentCount,
    /// Human-set metrics over the preferred set.
    pub human: Option<HumanSetMetrics>,
}

/// Registered R0/R1/R2/R3 analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct ChordRegimeAnalysis {
    /// Chord-only feasible set.
    pub r0: FeasibleRegime,
    /// Anchor-minimum subset of R0.
    pub r1: Option<ExactPreferredSet>,
    /// Technique-conditioned feasible set.
    pub r2: FeasibleRegime,
    /// Anchor-minimum subset of R2.
    pub r3: Option<ExactPreferredSet>,
}

/// Typed malformed input and evaluation refusals.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ChordEventError {
    /// Fewer than two atoms.
    #[error("event is not a chord")]
    NotAChord,
    /// Duplicate problem atom.
    #[error("duplicate stable chord atom id {0}")]
    DuplicateAtomId(usize),
    /// Duplicate observed atom.
    #[error("duplicate observed chord atom id {0}")]
    DuplicateObservedAtomId(usize),
    /// Technique target is absent.
    #[error("incoming technique targets missing atom {0}")]
    MissingTechniqueTarget(usize),
    /// Technique origin does not precede the chord.
    #[error("incoming technique origin {origin} does not precede chord {chord}")]
    NonPrecedingTechnique {
        /// Origin onset.
        origin: u32,
        /// Chord onset.
        chord: u32,
    },
}

/// Computes all four exact regimes.
///
/// # Errors
///
/// Reserved for typed representation refusals; a validated problem currently
/// evaluates without additional failure modes.
pub fn analyze_regimes(
    problem: &ChordEventProblem,
    observed: Option<&ObservedChordVoicing>,
) -> Result<ChordRegimeAnalysis, ChordEventError> {
    let r0_assignments = enumerate(problem, &BTreeMap::new());
    let requirements = technique_requirements(problem);
    let (r2_assignments, conflict) = match requirements {
        Ok(requirements) => (enumerate(problem, &requirements), None),
        Err(conflict) => (Vec::new(), Some(conflict)),
    };
    let r0 = feasible_regime(&r0_assignments, None, observed);
    let r1 = problem.preceding_hand.and_then(|anchor| {
        preferred(&r0_assignments, observed, |assignment| {
            anchor_cost(assignment, anchor.fret)
        })
    });
    let r2 = feasible_regime(&r2_assignments, conflict, observed);
    let r3 = problem.preceding_hand.and_then(|anchor| {
        preferred(&r2_assignments, observed, |assignment| {
            anchor_cost(assignment, anchor.fret)
        })
    });
    Ok(ChordRegimeAnalysis { r0, r1, r2, r3 })
}

fn enumerate(
    problem: &ChordEventProblem,
    requirements: &BTreeMap<usize, u8>,
) -> Vec<ChordAssignment> {
    let candidates: Vec<Vec<FretboardPosition>> = problem
        .atoms
        .iter()
        .map(|atom| {
            problem
                .tuning
                .candidates(atom.pitch, problem.max_fret)
                .into_iter()
                .filter(|candidate| {
                    requirements
                        .get(&atom.note_id)
                        .is_none_or(|required| candidate.string == *required)
                })
                .collect()
        })
        .collect();
    if candidates.iter().any(Vec::is_empty) {
        return Vec::new();
    }
    let mut output = Vec::new();
    let mut path = Vec::with_capacity(problem.atoms.len());
    visit(
        problem,
        &candidates,
        0,
        &mut [false; 256],
        &mut path,
        &mut output,
    );
    output
}

#[allow(clippy::too_many_arguments)] // transparent exact-search state
fn visit(
    problem: &ChordEventProblem,
    candidates: &[Vec<FretboardPosition>],
    index: usize,
    used: &mut [bool; 256],
    path: &mut Vec<ObservedAtomPosition>,
    output: &mut Vec<ChordAssignment>,
) {
    if index == candidates.len() {
        output.push(ChordAssignment {
            positions: path.clone(),
        });
        return;
    }
    for &candidate in &candidates[index] {
        let slot = usize::from(candidate.string);
        if used[slot] {
            continue;
        }
        used[slot] = true;
        path.push(ObservedAtomPosition::new(
            problem.atoms[index].note_id,
            candidate,
        ));
        visit(problem, candidates, index + 1, used, path, output);
        path.pop();
        used[slot] = false;
    }
}

fn technique_requirements(
    problem: &ChordEventProblem,
) -> Result<BTreeMap<usize, u8>, TechniqueConflict> {
    let mut requirements = BTreeMap::new();
    for relation in &problem.incoming_techniques {
        let required = relation.origin_position.string;
        if let Some(previous) = requirements.insert(relation.target_atom_id, required) {
            if previous != required {
                let mut required_strings = vec![previous, required];
                required_strings.sort_unstable();
                required_strings.dedup();
                return Err(TechniqueConflict {
                    target_atom_id: relation.target_atom_id,
                    required_strings,
                });
            }
        }
    }
    Ok(requirements)
}

fn feasible_regime(
    assignments: &[ChordAssignment],
    conflict: Option<TechniqueConflict>,
    observed: Option<&ObservedChordVoicing>,
) -> FeasibleRegime {
    FeasibleRegime {
        assignments: assignments.to_vec(),
        admissible_count: count(assignments.len()),
        conflict,
        human: observed.map(|human| human_metrics(assignments, human)),
        b0: preferred(assignments, observed, b0_cost),
    }
}

fn preferred<F>(
    assignments: &[ChordAssignment],
    observed: Option<&ObservedChordVoicing>,
    score: F,
) -> Option<ExactPreferredSet>
where
    F: Fn(&ChordAssignment) -> i64,
{
    let optimum = assignments.iter().map(&score).min()?;
    let selected: Vec<ChordAssignment> = assignments
        .iter()
        .filter(|assignment| score(assignment) == optimum)
        .cloned()
        .collect();
    Some(ExactPreferredSet {
        optimum,
        secondary: None,
        optimum_count: count(selected.len()),
        human: observed.map(|human| human_metrics(&selected, human)),
        assignments: selected,
    })
}

fn b0_cost(assignment: &ChordAssignment) -> i64 {
    let weights = FingeringWeights::v1();
    assignment
        .positions
        .iter()
        .map(|position| v1_unary(position.position.fret, &weights))
        .fold(0_i64, i64::saturating_add)
}

fn anchor_cost(assignment: &ChordAssignment, anchor: u8) -> i64 {
    assignment
        .positions
        .iter()
        .filter(|position| position.position.fret > 0)
        .map(|position| i64::from(position.position.fret.abs_diff(anchor)))
        .fold(0_i64, i64::saturating_add)
}

#[allow(clippy::cast_precision_loss)] // descriptive ratios; exact counts are retained
fn human_metrics(
    assignments: &[ChordAssignment],
    observed: &ObservedChordVoicing,
) -> HumanSetMetrics {
    if assignments.is_empty() || observed.positions.is_empty() {
        return HumanSetMetrics {
            exact_membership: false,
            floor: 0.0,
            uniform: 0.0,
            chosen: 0.0,
            ceiling: 0.0,
        };
    }
    let scores: Vec<usize> = assignments
        .iter()
        .map(|assignment| agreement_count(assignment, observed))
        .collect();
    let denominator = observed.positions.len() as f64;
    let total: usize = scores.iter().sum();
    HumanSetMetrics {
        exact_membership: scores.contains(&observed.positions.len()),
        floor: scores.iter().copied().min().unwrap_or(0) as f64 / denominator,
        uniform: total as f64 / (denominator * assignments.len() as f64),
        chosen: scores.first().copied().unwrap_or(0) as f64 / denominator,
        ceiling: scores.iter().copied().max().unwrap_or(0) as f64 / denominator,
    }
}

fn agreement_count(assignment: &ChordAssignment, observed: &ObservedChordVoicing) -> usize {
    observed
        .positions
        .iter()
        .filter(|expected| assignment.position(expected.atom_id) == Some(expected.position))
        .count()
}

#[allow(clippy::cast_precision_loss)] // descriptive log; exact count is retained
fn count(value: usize) -> AssignmentCount {
    AssignmentCount {
        value: u64::try_from(value).unwrap_or(u64::MAX),
        saturated: u64::try_from(value).is_err(),
        ln: if value == 0 {
            f64::NEG_INFINITY
        } else {
            (value as f64).ln()
        },
    }
}

/// Deterministically rotates anchors by one stable event inside each song.
/// Songs with fewer than two events have no control row.
#[must_use]
pub fn rotate_anchors_within_song(
    rows: &[(ChordEventIdentity, HandAnchor)],
) -> Vec<(ChordEventIdentity, HandAnchor)> {
    type AnchorRow<'a> = &'a (ChordEventIdentity, HandAnchor);
    let mut grouped: BTreeMap<&str, Vec<AnchorRow<'_>>> = BTreeMap::new();
    for row in rows {
        grouped.entry(&row.0.song_key).or_default().push(row);
    }
    let mut output = Vec::new();
    for mut song_rows in grouped.into_values() {
        song_rows.sort_by(|left, right| left.0.cmp(&right.0));
        if song_rows.len() < 2 {
            continue;
        }
        for index in 0..song_rows.len() {
            let event = song_rows[index].0.clone();
            let replacement = song_rows[(index + 1) % song_rows.len()].1;
            output.push((event, replacement));
        }
    }
    output.sort_by(|left, right| left.0.cmp(&right.0));
    output
}
