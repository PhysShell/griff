//! Red-first contract for the preregistered chord preceding-context oracle.

#![allow(
    clippy::indexing_slicing,
    clippy::missing_assert_message,
    clippy::unwrap_used
)]

use griff_constraint_lab::{
    chord::{ChordAtom, ChordCostPolicy},
    chord_context::{
        analyze_chord_context, assignment_context, ChordContext, ContextClassification,
    },
};
use griff_core::{
    event::{FretboardPosition, Pitch, Tuning},
    fretboard::STANDARD_MAX_FRET,
};

fn pos(string: u8, fret: u8) -> FretboardPosition {
    FretboardPosition { string, fret }
}

fn atom(id: usize, pitch: u8, imported: Option<FretboardPosition>) -> ChordAtom {
    ChordAtom {
        note_id: id,
        pitch: Pitch(pitch),
        imported_position: imported,
        tapped: false,
    }
}

fn analyze(
    origin_fret: u8,
    anchor_fret: Option<u8>,
    observed_string: u8,
) -> griff_constraint_lab::chord_context::ChordContextAnalysis {
    analyze_chord_context(
        &[atom(10, 64, Some(pos(2, 5))), atom(11, 59, Some(pos(3, 4)))],
        &Tuning::standard_e(),
        STANDARD_MAX_FRET,
        10,
        observed_string,
        ChordContext {
            origin_fret,
            anchor_fret,
        },
        &ChordCostPolicy::v1_unary(),
    )
    .unwrap()
}

#[test]
fn origin_fret_can_rank_the_observed_condition_above_b0() {
    let result = analyze(5, Some(5), 2);
    assert_eq!(result.observed.base_rank, Some(3));
    assert_eq!(result.observed.origin_rank, Some(1));
    assert_eq!(
        result.observed.origin_change.classification,
        ContextClassification::Improved
    );
    assert_eq!(result.observed.origin_change.rank_delta, Some(2));
}

#[test]
fn origin_context_is_allowed_not_to_improve_the_label() {
    let result = analyze(24, Some(5), 2);
    assert_eq!(result.observed.base_rank, Some(3));
    assert_eq!(result.observed.origin_rank, Some(5));
    assert_eq!(
        result.observed.origin_change.classification,
        ContextClassification::Worsened
    );
}

#[test]
fn anchor_distance_matches_199_and_open_strings_contribute_zero() {
    let values = assignment_context(
        &[pos(2, 5), pos(3, 4), pos(1, 0)],
        0,
        ChordContext {
            origin_fret: 7,
            anchor_fret: Some(5),
        },
    )
    .unwrap();
    assert_eq!(values.origin_distance, 2);
    assert_eq!(values.anchor_distance, Some(1));
}

#[test]
fn missing_anchor_is_typed_and_does_not_become_zero() {
    let result = analyze(5, None, 2);
    assert_eq!(result.observed.anchor_rank, None);
    assert_eq!(result.observed.origin_anchor_rank, None);
    assert_eq!(result.observed.anchor_origin_rank, None);
}

#[test]
fn complete_domain_and_b0_remain_the_209_oracle() {
    let result = analyze(5, Some(5), 2);
    assert_eq!(
        result
            .target_strings
            .iter()
            .map(|condition| condition.string)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5, 6]
    );
    assert_eq!(
        result.baseline.target_strings.len(),
        result.target_strings.len()
    );
    assert_eq!(
        result.observed.base_rank,
        result.baseline.observed_dense_rank
    );
    assert_eq!(
        result.observed.base_optimum,
        result
            .baseline
            .observed
            .as_ref()
            .map(|optimum| optimum.optimum)
    );
}

#[test]
fn context_ties_and_chosen_assignments_are_deterministic() {
    let first = analyze(5, Some(5), 2);
    let second = analyze(5, Some(5), 2);
    assert_eq!(first, second);
}

#[test]
fn pareto_frontier_is_global_across_target_strings() {
    let result = analyze(5, Some(5), 2);
    let observed = result
        .target_strings
        .iter()
        .find(|condition| condition.string == 2)
        .unwrap();
    assert_eq!(observed.pareto_member, Some(true));
    assert!(result
        .target_strings
        .iter()
        .any(|condition| condition.pareto_member == Some(false)));
}

#[test]
fn duplicate_pitch_identity_and_low_first_orientation_are_preserved() {
    let tuning = Tuning::new(vec![
        Pitch(40),
        Pitch(45),
        Pitch(50),
        Pitch(55),
        Pitch(59),
        Pitch(64),
    ]);
    let atoms = [atom(10, 45, Some(pos(1, 5))), atom(11, 45, Some(pos(2, 0)))];
    let result = analyze_chord_context(
        &atoms,
        &tuning,
        STANDARD_MAX_FRET,
        11,
        2,
        ChordContext {
            origin_fret: 0,
            anchor_fret: Some(5),
        },
        &ChordCostPolicy::v1_unary(),
    )
    .unwrap();
    assert_eq!(result.observed.origin_rank, Some(1));
    assert_eq!(result.human.as_ref().unwrap().positions[1], pos(2, 0));
}
