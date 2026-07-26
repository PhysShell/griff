//! The two Constraint Inventory problems, built from production types.
//!
//! Problem A (bounded-travel realization) takes its fret domains from
//! `Tuning::candidates` — the same enumeration the production fingering DP
//! consumes. Problem B (pair cleanliness) pins the `PairValidation` laws:
//! part-A playability is a *precondition*, the B domain is filtered to
//! playable pitches (per-note existence — exactly `is_playable`'s law for a
//! monophonic line), coincident dissonance uses the production classes
//! `{1, 6, 11}`, and the band constraint carries the exact
//! `band_overlap > 1/2` register-mud law.

use griff_core::event::{Pitch, Tuning};
use griff_core::fretboard::{measure_playability, FingeringWeights};
use thiserror::Error;

use crate::ir::{Constraint, IntVar, OracleProblem, VarId};

/// Production dissonant interval classes (m2 / tritone / M7), mod 12.
pub const DISSONANT_CLASSES: [i64; 3] = [1, 6, 11];

/// Typed refusals for problem construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum LabError {
    /// A line pitch has no playable `(string, fret)` under the tuning.
    #[error("pitch {pitch} at index {index} has no playable position")]
    UnpositionablePitch {
        /// Index of the offending note in the line.
        index: usize,
        /// The MIDI pitch.
        pitch: u8,
    },
    /// Part A fails its playability precondition.
    #[error("part A is not playable: {unpositionable} unpositionable note(s)")]
    PartAUnplayable {
        /// Count of unpositionable part-A notes.
        unpositionable: usize,
    },
    /// Filtering the B domain to playable pitches left nothing.
    #[error("no playable pitch remains in the B domain ({removed} removed)")]
    EmptyPlayableDomain {
        /// How many domain pitches the playability filter removed.
        removed: usize,
    },
    /// A problem needs at least one note.
    #[error("the line is empty")]
    EmptyLine,
}

/// Problem A: assign one fret per line note so consecutive travel stays
/// within `bound`. Domains are the playable frets from `Tuning::candidates`
/// (any string — the travel law measures fret distance only, mirroring
/// `max_fret_jump`).
///
/// # Errors
///
/// [`LabError::EmptyLine`] for an empty line; [`LabError::UnpositionablePitch`]
/// when a pitch has no playable position under the tuning.
pub fn bounded_travel_problem(
    pitches: &[Pitch],
    tuning: &Tuning,
    max_fret: u8,
    bound: u8,
) -> Result<OracleProblem, LabError> {
    if pitches.is_empty() {
        return Err(LabError::EmptyLine);
    }
    let mut vars = Vec::with_capacity(pitches.len());
    for (index, &pitch) in pitches.iter().enumerate() {
        let frets: Vec<i64> = tuning
            .candidates(pitch, max_fret)
            .iter()
            .map(|p| i64::from(p.fret))
            .collect();
        if frets.is_empty() {
            return Err(LabError::UnpositionablePitch {
                index,
                pitch: pitch.0,
            });
        }
        vars.push(IntVar::new(format!("f{index}"), frets));
    }
    let constraints = (1..pitches.len())
        .map(|i| Constraint::AbsDiffLe {
            a: VarId(i - 1),
            b: VarId(i),
            bound: i64::from(bound),
        })
        .collect();
    Ok(OracleProblem::new("bounded-travel", vars, constraints))
}

/// Specification for Problem B: part A fixed, one pitch variable per B onset
/// over a shared finite domain.
#[derive(Debug, Clone)]
pub struct PairSpec {
    /// Part A's monophonic line as `(onset, pitch)`.
    pub a_line: Vec<(u32, Pitch)>,
    /// B onsets (one pitch variable each).
    pub b_onsets: Vec<u32>,
    /// The finite pitch domain B may draw from (relation-mode domain).
    pub b_domain: Vec<Pitch>,
    /// The tuning both playability checks run under.
    pub tuning: Tuning,
    /// Highest fret considered.
    pub max_fret: u8,
}

/// Problem B: is there any B pitch assignment that the production
/// `PairValidation` laws call clean?
///
/// # Errors
///
/// [`LabError::EmptyLine`] when either side has no notes;
/// [`LabError::PartAUnplayable`] when part A fails its playability
/// precondition; [`LabError::EmptyPlayableDomain`] when no playable pitch
/// remains after filtering the B domain.
pub fn pair_cleanliness_problem(spec: &PairSpec) -> Result<OracleProblem, LabError> {
    if spec.a_line.is_empty() || spec.b_onsets.is_empty() {
        return Err(LabError::EmptyLine);
    }

    // Precondition: part A itself must be playable (`is_clean` demands it of
    // both parts; for the oracle, A is an input, not a variable).
    let a_pitches: Vec<Pitch> = spec.a_line.iter().map(|&(_, p)| p).collect();
    let a_report = measure_playability(
        &a_pitches,
        &spec.tuning,
        &FingeringWeights::v1(),
        spec.max_fret,
    );
    if !a_report.is_playable() {
        return Err(LabError::PartAUnplayable {
            unpositionable: a_report.unpositionable,
        });
    }

    // B playability as a domain filter: for a monophonic line, `is_playable`
    // is exactly per-note position existence.
    let playable: Vec<i64> = spec
        .b_domain
        .iter()
        .filter(|&&p| !spec.tuning.candidates(p, spec.max_fret).is_empty())
        .map(|&p| i64::from(p.0))
        .collect();
    let removed = spec.b_domain.len().saturating_sub(playable.len());
    if playable.is_empty() {
        return Err(LabError::EmptyPlayableDomain { removed });
    }

    let vars: Vec<IntVar> = spec
        .b_onsets
        .iter()
        .enumerate()
        .map(|(i, _)| IntVar::new(format!("b{i}"), playable.clone()))
        .collect();

    let mut constraints = Vec::new();
    for (i, &b_onset) in spec.b_onsets.iter().enumerate() {
        for &(a_onset, a_pitch) in &spec.a_line {
            if a_onset == b_onset {
                constraints.push(Constraint::ForbiddenIntervalClasses {
                    var: VarId(i),
                    fixed: i64::from(a_pitch.0),
                    classes: DISSONANT_CLASSES.to_vec(),
                });
            }
        }
    }

    let a_lo = a_pitches.iter().map(|p| i64::from(p.0)).min().unwrap_or(0);
    let a_hi = a_pitches.iter().map(|p| i64::from(p.0)).max().unwrap_or(0);
    constraints.push(Constraint::BandOverlapAtMost {
        vars: (0..vars.len()).map(VarId).collect(),
        fixed_lo: a_lo,
        fixed_hi: a_hi,
        num: 1,
        den: 2,
    });

    Ok(OracleProblem::new("pair-cleanliness", vars, constraints))
}
