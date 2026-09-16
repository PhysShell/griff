//! Fingering optimality experiments — the first optimization-phase subject.
//!
//! Three pieces, all pure and deterministic:
//!
//! - **Tablature lines** ([`tab_lines`]): monophonic runs of a Guitar Pro
//!   track that carry the tab author's own `(string, fret)` choices — the
//!   human reference a fingering model is measured against.
//! - **The production objective, mirrored** ([`v1_cost`], [`v1_problem`]):
//!   the exact cost `griff_core::fretboard::infer_positions` minimizes,
//!   re-implemented independently so an external solver's optimum can be
//!   compared with the production DP's path.
//! - **A hand-position model** ([`HandModel`], [`solve_hand`],
//!   [`hand_problem`]): a hidden index-finger position with a four-fret box,
//!   stretch, shift events and distances, and string distance — the
//!   finger-span layer ADR-0019 §7 defers. Experimental: calibration
//!   evidence only, no authority over production.

use std::ops::RangeInclusive;

use griff_core::event::{FretboardPosition, Pitch, Tuning};
use griff_core::fretboard::{FingeringWeights, STANDARD_MAX_FRET};
use griff_core::score::Score;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::optir::OptProblem;
use crate::problems::LabError;

/// Variables per note in a [`v1_problem`]: `s{i}` (string), `f{i}` (fret).
pub const V1_VARS_PER_NOTE: usize = 2;
/// Variables per note in a [`hand_problem`]: `s{i}`, `f{i}`, `h{i}` (hand).
pub const HAND_VARS_PER_NOTE: usize = 3;

/// How a track is cut into tablature lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct LineCut {
    /// Lines shorter than this are dropped (and counted).
    pub min_notes: usize,
    /// A silence of at least this many quarters between a note's end and the
    /// next onset cuts the line; `0` disables rest cuts.
    pub max_rest_quarters: u32,
    /// Positions above this fret cut the line (and are counted).
    pub max_fret: u8,
}

impl LineCut {
    /// The experiment's baseline cut: ≥ 4 notes, a whole-bar-in-4/4 rest
    /// cuts, [`STANDARD_MAX_FRET`].
    #[must_use]
    pub const fn v1() -> Self {
        Self {
            min_notes: 4,
            max_rest_quarters: 4,
            max_fret: STANDARD_MAX_FRET,
        }
    }
}

/// Where the notes of a track went when it was cut into lines.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct CutStats {
    /// Note atoms read.
    pub notes_seen: u64,
    /// Onsets carrying more than one note (each cuts the line).
    pub chord_onsets: u64,
    /// Single notes without a position (each cuts the line).
    pub unpositioned: u64,
    /// Single notes positioned above `max_fret` (each cuts the line).
    pub beyond_max_fret: u64,
    /// Single notes whose position does not sound their pitch under the
    /// track tuning (each cuts the line).
    pub pitch_mismatch: u64,
    /// Rests long enough to cut a non-empty line.
    pub rest_cuts: u64,
    /// Non-empty lines dropped as shorter than `min_notes`.
    pub short_lines: u64,
    /// Notes inside those dropped lines.
    pub short_line_notes: u64,
    /// Lines kept.
    pub kept_lines: u64,
    /// Notes inside kept lines.
    pub kept_notes: u64,
}

impl CutStats {
    /// Adds another track's counts into this one.
    pub fn absorb(&mut self, other: &Self) {
        let _ = other;
        todo!("cut stats — green step")
    }
}

/// One monophonic tablature line with the tab author's positions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabLine {
    /// Track index in the score.
    pub track: usize,
    /// Voice id within the track.
    pub voice: u8,
    /// Onset tick of the first note.
    pub start_tick: u32,
    /// The track tuning.
    pub tuning: Tuning,
    /// Pitches, in onset order.
    pub pitches: Vec<Pitch>,
    /// The tab author's positions — one per pitch, each sounding it.
    pub human: Vec<FretboardPosition>,
}

/// Cuts one track into monophonic tablature lines, per voice.
///
/// A line is a maximal run of single-note onsets whose explicit positions
/// sound their pitch under the track tuning. A chord onset, an unpositioned
/// note, a position above `cut.max_fret`, a pitch/position mismatch, or a
/// long enough rest ends the current line; each cause is counted.
///
/// # Errors
///
/// [`LabError::NoSuchTrack`] when `track_index` is out of range.
pub fn tab_lines(
    score: &Score,
    track_index: usize,
    cut: &LineCut,
) -> Result<(Vec<TabLine>, CutStats), LabError> {
    let _ = (score, track_index, cut);
    todo!("tablature line extraction — green step")
}

/// The production fingering objective (ADR-0019 `v1`), re-implemented
/// independently of `infer_positions`: per note `fret·w.fret − [open]·w.open_string`,
/// per step `|Δfret|·w.position_shift + [string changed]·w.string_change`.
#[must_use]
pub fn v1_cost(line: &[FretboardPosition], weights: &FingeringWeights) -> i64 {
    let _ = (line, weights);
    todo!("v1 objective mirror — green step")
}

/// The production objective as an [`OptProblem`]: per note `s{i}` and
/// `f{i}` tied by the candidate table, unary fret costs, `AbsDiff` fret
/// travel and `NotEqual` string change between neighbours. Zero-weight terms
/// and zero-cost table entries are omitted.
///
/// # Errors
///
/// [`LabError::EmptyLine`] for no pitches; [`LabError::UnpositionablePitch`]
/// when a pitch has no candidate at or below `max_fret`.
pub fn v1_problem(
    pitches: &[Pitch],
    tuning: &Tuning,
    weights: &FingeringWeights,
    max_fret: u8,
) -> Result<OptProblem, LabError> {
    let _ = (pitches, tuning, weights, max_fret);
    todo!("v1 problem builder — green step")
}

/// Encodes positions as a [`v1_problem`] witness (`s0, f0, s1, f1, …`).
#[must_use]
pub fn encode_v1_witness(line: &[FretboardPosition]) -> Vec<i64> {
    let _ = line;
    todo!("v1 witness encoding — green step")
}

/// Encodes positions and hands as a [`hand_problem`] witness
/// (`s0, f0, h0, s1, …`); `None` when the lengths differ.
#[must_use]
pub fn encode_hand_witness(line: &[FretboardPosition], hands: &[u8]) -> Option<Vec<i64>> {
    let _ = (line, hands);
    todo!("hand witness encoding — green step")
}

/// Decodes the per-note positions of a witness laid out with
/// `vars_per_note` variables per note, string then fret first; `None` for a
/// ragged length or out-of-range values.
#[must_use]
pub fn decode_positions(witness: &[i64], vars_per_note: usize) -> Option<Vec<FretboardPosition>> {
    let _ = (witness, vars_per_note);
    todo!("witness decoding — green step")
}

/// Weights of the hand-position model. Transition weights and `stretch` are
/// non-negative; `height` and `open_string` may be negative (a preference
/// for high positions, a bonus for open strings).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandWeights {
    /// Per fret of hand height above the first position, per note.
    pub height: i64,
    /// Per open-string note.
    pub open_string: i64,
    /// Per note played one fret outside the four-fret box.
    pub stretch: i64,
    /// Per hand shift (the position changes at all).
    pub shift: i64,
    /// Per fret of hand travel.
    pub shift_distance: i64,
    /// Per string crossed between consecutive notes.
    pub string_distance: i64,
}

/// How a fret is reached from a hand position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// An open string — reachable from any hand position.
    Open,
    /// Inside the four-fret box `[hand, hand + 3]`.
    InBox,
    /// One fret outside the box: `hand − 1` (≥ 1) or `hand + 4`.
    Stretch,
}

/// Typed refusals for a hand model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum HandModelError {
    /// A weight that must be non-negative is negative.
    #[error("weight {name} must be non-negative, got {value}")]
    NegativeWeight {
        /// The weight's field name.
        name: &'static str,
        /// Its value.
        value: i64,
    },
    /// The neck is too short for a four-fret box.
    #[error("max_fret {max_fret} leaves no room for a four-fret box")]
    NoRoom {
        /// The refused fret range.
        max_fret: u8,
    },
}

/// A validated hand-position model over frets `0..=max_fret`; hand
/// positions are `1..=max_fret − 3`, so the box stays on the neck.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandModel {
    weights: HandWeights,
    max_fret: u8,
}

impl HandModel {
    /// Frets covered by the hand without a stretch.
    pub const BOX_FRETS: u8 = 4;

    /// Validates the weights and the neck range.
    ///
    /// # Errors
    ///
    /// [`HandModelError::NegativeWeight`] for a negative `stretch`, `shift`,
    /// `shift_distance`, or `string_distance`; [`HandModelError::NoRoom`]
    /// when `max_fret < 4`.
    pub fn new(weights: HandWeights, max_fret: u8) -> Result<Self, HandModelError> {
        let _ = (weights, max_fret);
        todo!("hand model validation — green step")
    }

    /// The weights.
    #[must_use]
    pub const fn weights(&self) -> HandWeights {
        self.weights
    }

    /// The highest fret.
    #[must_use]
    pub const fn max_fret(&self) -> u8 {
        self.max_fret
    }

    /// Admissible hand positions, ascending.
    #[must_use]
    pub fn hands(&self) -> RangeInclusive<u8> {
        todo!("hand range — green step")
    }

    /// How `fret` is reached from `hand`; `None` when it is not reachable
    /// (or `hand` is not an admissible position).
    #[must_use]
    pub fn reach(&self, fret: u8, hand: u8) -> Option<Reach> {
        let _ = (fret, hand);
        todo!("reach — green step")
    }
}

/// Why a `(positions, hands)` pair cannot be scored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum HandError {
    /// Positions and hands differ in length.
    #[error("{positions} positions but {hands} hands")]
    Length {
        /// Positions supplied.
        positions: usize,
        /// Hands supplied.
        hands: usize,
    },
    /// A note is not reachable from its hand position.
    #[error("note {index} is not reachable from its hand position")]
    Unreachable {
        /// Index of the note.
        index: usize,
    },
}

/// Scores a complete `(positions, hands)` realization under the model:
/// per note `height·(hand − 1) + [open]·open_string + [stretch]·stretch`,
/// per step `[hand changed]·shift + |Δhand|·shift_distance + |Δstring|·string_distance`.
///
/// # Errors
///
/// See [`HandError`].
pub fn hand_cost(
    line: &[FretboardPosition],
    hands: &[u8],
    model: &HandModel,
) -> Result<i64, HandError> {
    let _ = (line, hands, model);
    todo!("hand cost — green step")
}

/// The cheapest hand sequence for **fixed** positions (e.g. a human tab):
/// the model's score of that fingering. `None` when some position is
/// unreachable from every hand position.
#[must_use]
pub fn best_hands(line: &[FretboardPosition], model: &HandModel) -> Option<(i64, Vec<u8>)> {
    let _ = (line, model);
    todo!("best hands — green step")
}

/// An optimal realization under a [`HandModel`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandSolution {
    /// The optimal cost.
    pub cost: i64,
    /// One position per pitch.
    pub positions: Vec<FretboardPosition>,
    /// One hand position per pitch.
    pub hands: Vec<u8>,
}

/// Exact joint DP over `(candidate, hand)` states — the in-repo reference
/// optimum of the hand model. Deterministic. `None` when some pitch has no
/// candidate at or below the model's `max_fret`; an empty line costs `0`.
#[must_use]
pub fn solve_hand(pitches: &[Pitch], tuning: &Tuning, model: &HandModel) -> Option<HandSolution> {
    let _ = (pitches, tuning, model);
    todo!("hand DP — green step")
}

/// The hand model as an [`OptProblem`]: per note `s{i}`, `f{i}`, `h{i}`;
/// the candidate table ties string to fret, a reach table ties fret to hand;
/// unary height and open-string costs, a fret×hand stretch table, and
/// `NotEqual` / `AbsDiff` hand shifts plus `AbsDiff` string distance between
/// neighbours. Zero-weight terms and zero-cost entries are omitted.
///
/// # Errors
///
/// [`LabError::EmptyLine`] for no pitches; [`LabError::UnpositionablePitch`]
/// when a pitch has no candidate at or below the model's `max_fret`.
pub fn hand_problem(
    pitches: &[Pitch],
    tuning: &Tuning,
    model: &HandModel,
) -> Result<OptProblem, LabError> {
    let _ = (pitches, tuning, model);
    todo!("hand problem builder — green step")
}

/// A song identity for holdout splits: the file stem, lowercased, with
/// trailing parenthesized groups (e.g. `(ver 2 by …)`) and the extension
/// removed, so arrangements of one song share a key.
#[must_use]
pub fn song_key(file_name: &str) -> String {
    let _ = file_name;
    todo!("song key — green step")
}

/// A deterministic holdout bucket in `0..buckets` for a [`song_key`]
/// (FNV-1a 64 modulo `buckets`); `0` when `buckets` is `0`.
#[must_use]
pub fn holdout_bucket(key: &str, buckets: u64) -> u64 {
    let _ = (key, buckets);
    todo!("holdout bucket — green step")
}
