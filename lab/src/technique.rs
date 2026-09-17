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

use crate::problems::LabError;
use crate::ties::Chain;

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
    let _ = (line, tapped, weights, tap_shift);
    todo!("tap-aware cost — green step")
}

/// [`tap_aware_cost`] as a [`Chain`], so the exact optimum-set DPs apply.
///
/// Per note the chain's states pair the note's candidate with the candidate of
/// the most recent note played by the *other* hand (or none yet); a transition
/// is admissible only when that carried candidate agrees with the previous
/// state, so state paths and position assignments correspond one to one.
/// Inadmissible transitions carry a cost no optimal path can take.
///
/// # Errors
///
/// [`LabError::EmptyLine`] for no pitches; [`LabError::UnpositionablePitch`]
/// when a pitch has no candidate at or below `max_fret`;
/// [`LabError::LabelLength`] when `tapped` does not have one flag per pitch.
pub fn tap_aware_chain(
    pitches: &[Pitch],
    tuning: &Tuning,
    weights: &FingeringWeights,
    tap_shift: i64,
    tapped: &[bool],
    max_fret: u8,
) -> Result<Chain, LabError> {
    let _ = (pitches, tuning, weights, tap_shift, tapped, max_fret);
    todo!("tap-aware chain — green step")
}
