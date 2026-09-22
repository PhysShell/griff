//! Exact, Lab-only chord string-assignment oracle.
//!
//! This is the preregistered assignment layer for observed legato targets that
//! land inside chord onsets. It deliberately models no fingers, barre, reach,
//! or sequence state: every atom must receive a pitch-correct candidate and
//! simultaneous atoms must use distinct physical strings.

use std::collections::BTreeSet;

use griff_core::{
    event::{FretboardPosition, Pitch, Tuning},
    fretboard::FingeringWeights,
};
use thiserror::Error;

use crate::fingering::v1_unary;

/// One stable imported chord atom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChordAtom {
    /// Stable note id within the imported voice.
    pub note_id: usize,
    /// Imported pitch.
    pub pitch: Pitch,
    /// Explicit imported position, when usable for the human-realization check.
    pub imported_position: Option<FretboardPosition>,
    /// Whether the imported atom is marked tapped.
    pub tapped: bool,
}

/// The identified target atom and the observed legato-origin string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TargetStringConstraint {
    /// Stable identity of the target atom.
    pub atom_id: usize,
    /// Required physical string in the imported tuning orientation.
    pub string: u8,
}

/// Transparent chord ranking policy. Feasibility never depends on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChordCostPolicy {
    weights: FingeringWeights,
}

impl ChordCostPolicy {
    /// Sum of the accepted ADR-0019 `v1` per-note fret/open term.
    #[must_use]
    pub const fn v1_unary() -> Self {
        Self {
            weights: FingeringWeights::v1(),
        }
    }

    fn position_cost(self, position: FretboardPosition) -> i64 {
        v1_unary(position.fret, &self.weights)
    }
}

/// Exact optimum and cardinalities for one chord condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChordOptimum {
    /// Minimum registered surrogate cost.
    pub optimum: i64,
    /// Number of admissible assignments at the optimum (saturating).
    pub optimum_count: u64,
    /// Total admissible assignments (saturating).
    pub admissible_count: u64,
    /// First optimum in atom-order × ascending-string candidate order.
    pub chosen: Vec<FretboardPosition>,
}

/// Exact result for one legal target-string condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetStringResult {
    /// Candidate string for the identified target pitch.
    pub string: u8,
    /// Exact result; `None` means the condition is infeasible.
    pub result: Option<ChordOptimum>,
}

/// Assessment of the complete imported realization, when every atom has one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanChordAssessment {
    /// Pitch/range/string-exclusivity feasibility under the registered model.
    pub feasible: bool,
    /// Whether the identified target uses the observed origin string.
    pub satisfies_observed_constraint: bool,
    /// Registered surrogate cost, only when feasible.
    pub cost: Option<i64>,
    /// Membership in U's exact optimum set.
    pub in_unconstrained_optimum: bool,
    /// Membership in C's exact optimum set.
    pub in_observed_optimum: bool,
    /// Cost above U, only when feasible.
    pub excess_unconstrained: Option<i64>,
    /// Cost above C, only when feasible and the constraint is satisfied.
    pub excess_observed: Option<i64>,
}

/// Complete U/C/control analysis for one chord.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChordAnalysis {
    /// Unconstrained optimum U.
    pub unconstrained: ChordOptimum,
    /// Observed-string constrained optimum C; absent when infeasible.
    pub observed: Option<ChordOptimum>,
    /// Complete legal target-string domain, ordered by string number.
    pub target_strings: Vec<TargetStringResult>,
    /// Dense cost rank of the observed condition among feasible conditions.
    pub observed_dense_rank: Option<usize>,
    /// Number of legal strings tied at the observed condition's cost.
    pub observed_best_tie_size: Option<usize>,
    /// Imported realization assessment; absent when any atom lacks a position.
    pub human: Option<HumanChordAssessment>,
}

/// Typed malformed-input refusals for the chord oracle.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ChordError {
    /// A zero-atom chord has no assignment question.
    #[error("chord has no atoms")]
    Empty,
    /// Stable atom identities must be unique.
    #[error("duplicate stable chord atom id {0}")]
    DuplicateAtomId(usize),
    /// The constraint names no atom.
    #[error("target atom {0} is not in the chord")]
    MissingTarget(usize),
    /// U is infeasible, so U/C comparison is undefined.
    #[error("unconstrained chord is infeasible")]
    UnconstrainedInfeasible,
}

/// Enumerates every admissible assignment exactly.
///
/// # Errors
///
/// Refuses empty chords, duplicate stable identities, or a constraint naming
/// no atom. `Ok(None)` is an exact infeasibility result.
pub fn solve_chord(
    atoms: &[ChordAtom],
    tuning: &Tuning,
    max_fret: u8,
    constraint: Option<TargetStringConstraint>,
    policy: &ChordCostPolicy,
) -> Result<Option<ChordOptimum>, ChordError> {
    validate(atoms, constraint)?;
    let candidates: Vec<Vec<FretboardPosition>> = atoms
        .iter()
        .map(|atom| {
            tuning
                .candidates(atom.pitch, max_fret)
                .into_iter()
                .filter(|candidate| {
                    constraint.is_none_or(|required| {
                        atom.note_id != required.atom_id || candidate.string == required.string
                    })
                })
                .collect()
        })
        .collect();
    if candidates.iter().any(Vec::is_empty) {
        return Ok(None);
    }

    let mut search = Search {
        policy: *policy,
        candidates: &candidates,
        used: [false; 256],
        path: Vec::with_capacity(atoms.len()),
        best: None,
        admissible_count: 0,
    };
    search.visit(0, 0);
    Ok(search.best.map(|mut best| {
        best.admissible_count = search.admissible_count;
        best
    }))
}

/// Solves U, C, every legal target-string condition, and the imported voicing.
///
/// # Errors
///
/// See [`solve_chord`]; additionally refuses an infeasible U because ranks and
/// excesses would be undefined.
pub fn analyze_chord(
    atoms: &[ChordAtom],
    tuning: &Tuning,
    max_fret: u8,
    observed: TargetStringConstraint,
    policy: &ChordCostPolicy,
) -> Result<ChordAnalysis, ChordError> {
    let target = atoms
        .iter()
        .find(|atom| atom.note_id == observed.atom_id)
        .ok_or(ChordError::MissingTarget(observed.atom_id))?;
    let unconstrained = solve_chord(atoms, tuning, max_fret, None, policy)?
        .ok_or(ChordError::UnconstrainedInfeasible)?;
    let mut legal_strings: Vec<u8> = tuning
        .candidates(target.pitch, max_fret)
        .into_iter()
        .map(|candidate| candidate.string)
        .collect();
    legal_strings.sort_unstable();
    legal_strings.dedup();
    let target_strings: Vec<TargetStringResult> = legal_strings
        .into_iter()
        .map(|string| {
            let condition = TargetStringConstraint {
                atom_id: observed.atom_id,
                string,
            };
            solve_chord(atoms, tuning, max_fret, Some(condition), policy)
                .map(|result| TargetStringResult { string, result })
        })
        .collect::<Result<_, _>>()?;
    let observed_result = target_strings
        .iter()
        .find(|entry| entry.string == observed.string)
        .and_then(|entry| entry.result.clone());
    let observed_cost = observed_result.as_ref().map(|result| result.optimum);
    let observed_dense_rank = observed_cost.map(|cost| {
        let mut cheaper: Vec<i64> = target_strings
            .iter()
            .filter_map(|entry| entry.result.as_ref().map(|result| result.optimum))
            .filter(|candidate| *candidate < cost)
            .collect();
        cheaper.sort_unstable();
        cheaper.dedup();
        cheaper.len().saturating_add(1)
    });
    let observed_best_tie_size = observed_cost.map(|cost| {
        target_strings
            .iter()
            .filter(|entry| {
                entry
                    .result
                    .as_ref()
                    .is_some_and(|result| result.optimum == cost)
            })
            .count()
    });
    let human = assess_human(
        atoms,
        tuning,
        max_fret,
        observed,
        policy,
        &unconstrained,
        observed_result.as_ref(),
    );
    Ok(ChordAnalysis {
        unconstrained,
        observed: observed_result,
        target_strings,
        observed_dense_rank,
        observed_best_tie_size,
        human,
    })
}

fn validate(
    atoms: &[ChordAtom],
    constraint: Option<TargetStringConstraint>,
) -> Result<(), ChordError> {
    if atoms.is_empty() {
        return Err(ChordError::Empty);
    }
    let mut ids = BTreeSet::new();
    for atom in atoms {
        if !ids.insert(atom.note_id) {
            return Err(ChordError::DuplicateAtomId(atom.note_id));
        }
    }
    if let Some(required) = constraint {
        if !ids.contains(&required.atom_id) {
            return Err(ChordError::MissingTarget(required.atom_id));
        }
    }
    Ok(())
}

fn assess_human(
    atoms: &[ChordAtom],
    tuning: &Tuning,
    max_fret: u8,
    observed: TargetStringConstraint,
    policy: &ChordCostPolicy,
    unconstrained: &ChordOptimum,
    constrained: Option<&ChordOptimum>,
) -> Option<HumanChordAssessment> {
    let positions: Vec<FretboardPosition> = atoms
        .iter()
        .map(|atom| atom.imported_position)
        .collect::<Option<_>>()?;
    let mut used = [false; 256];
    let feasible = atoms.iter().zip(&positions).all(|(atom, position)| {
        let index = usize::from(position.string);
        let valid = position.fret <= max_fret
            && tuning.pitch_at(*position) == Some(atom.pitch)
            && !used[index];
        used[index] = true;
        valid
    });
    let satisfies = atoms
        .iter()
        .zip(&positions)
        .find(|(atom, _)| atom.note_id == observed.atom_id)
        .is_some_and(|(_, position)| position.string == observed.string);
    let cost = feasible.then(|| {
        positions
            .iter()
            .map(|position| policy.position_cost(*position))
            .fold(0_i64, i64::saturating_add)
    });
    Some(HumanChordAssessment {
        feasible,
        satisfies_observed_constraint: satisfies,
        cost,
        in_unconstrained_optimum: cost == Some(unconstrained.optimum),
        in_observed_optimum: satisfies
            && cost.is_some_and(|cost| constrained.is_some_and(|c| cost == c.optimum)),
        excess_unconstrained: cost.map(|cost| cost.saturating_sub(unconstrained.optimum)),
        excess_observed: if satisfies {
            cost.zip(constrained.map(|result| result.optimum))
                .map(|(cost, optimum)| cost.saturating_sub(optimum))
        } else {
            None
        },
    })
}

struct Search<'a> {
    policy: ChordCostPolicy,
    candidates: &'a [Vec<FretboardPosition>],
    used: [bool; 256],
    path: Vec<FretboardPosition>,
    best: Option<ChordOptimum>,
    admissible_count: u64,
}

impl Search<'_> {
    fn visit(&mut self, atom: usize, cost: i64) {
        if atom == self.candidates.len() {
            self.admissible_count = self.admissible_count.saturating_add(1);
            match &mut self.best {
                Some(best) if cost == best.optimum => {
                    best.optimum_count = best.optimum_count.saturating_add(1);
                }
                Some(best) if cost > best.optimum => {}
                _ => {
                    self.best = Some(ChordOptimum {
                        optimum: cost,
                        optimum_count: 1,
                        admissible_count: 0,
                        chosen: self.path.clone(),
                    });
                }
            }
            return;
        }
        for &candidate in &self.candidates[atom] {
            let string = usize::from(candidate.string);
            if self.used[string] {
                continue;
            }
            self.used[string] = true;
            self.path.push(candidate);
            self.visit(
                atom.saturating_add(1),
                cost.saturating_add(self.policy.position_cost(candidate)),
            );
            self.path.pop();
            self.used[string] = false;
        }
    }
}
