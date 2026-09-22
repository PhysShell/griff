//! Contract for the preregistered legato-into-chord feasibility oracle.

use griff_constraint_lab::chord::{
    analyze_chord, solve_chord, ChordAtom, ChordCostPolicy, TargetStringConstraint,
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

fn solve(
    atoms: &[ChordAtom],
    tuning: &Tuning,
    constraint: Option<TargetStringConstraint>,
) -> Option<griff_constraint_lab::chord::ChordOptimum> {
    solve_chord(
        atoms,
        tuning,
        STANDARD_MAX_FRET,
        constraint,
        &ChordCostPolicy::v1_unary(),
    )
    .unwrap()
}

#[test]
fn simple_dyad_has_multiple_legal_voicings_and_exact_optimum_count() {
    let atoms = [atom(10, 64, None), atom(11, 59, None)];
    let result = solve(&atoms, &Tuning::standard_e(), None).unwrap();
    assert_eq!(result.optimum, -2);
    assert_eq!(result.optimum_count, 1);
    assert_eq!(result.chosen, vec![pos(1, 0), pos(2, 0)]);
    assert!(result.admissible_count > result.optimum_count);
}

#[test]
fn observed_target_string_can_be_feasible_and_free() {
    let atoms = [atom(10, 64, None), atom(11, 59, None)];
    let result = analyze_chord(
        &atoms,
        &Tuning::standard_e(),
        STANDARD_MAX_FRET,
        TargetStringConstraint {
            atom_id: 10,
            string: 1,
        },
        &ChordCostPolicy::v1_unary(),
    )
    .unwrap();
    assert_eq!(result.observed.as_ref().unwrap().optimum, result.unconstrained.optimum);
    assert_eq!(result.observed_dense_rank, Some(1));
}

#[test]
fn observed_target_string_can_be_feasible_but_costly() {
    let atoms = [atom(10, 64, None), atom(11, 59, None)];
    let result = analyze_chord(
        &atoms,
        &Tuning::standard_e(),
        STANDARD_MAX_FRET,
        TargetStringConstraint {
            atom_id: 10,
            string: 2,
        },
        &ChordCostPolicy::v1_unary(),
    )
    .unwrap();
    assert!(result.observed.as_ref().unwrap().optimum > result.unconstrained.optimum);
}

#[test]
fn target_string_constraint_can_make_a_chord_infeasible() {
    let tuning = Tuning::new(vec![Pitch(64), Pitch(59)]);
    let atoms = [atom(10, 64, None), atom(11, 59, None)];
    assert!(solve(
        &atoms,
        &tuning,
        Some(TargetStringConstraint {
            atom_id: 10,
            string: 2,
        }),
    )
    .is_none());
}

#[test]
fn duplicate_pitches_are_constrained_by_stable_atom_identity() {
    let tuning = Tuning::new(vec![Pitch(64), Pitch(59)]);
    let atoms = [atom(10, 64, None), atom(11, 64, None)];
    let result = solve(
        &atoms,
        &tuning,
        Some(TargetStringConstraint {
            atom_id: 11,
            string: 1,
        }),
    )
    .unwrap();
    assert_eq!(result.chosen, vec![pos(2, 5), pos(1, 0)]);
}

#[test]
fn two_atoms_never_share_one_physical_string() {
    let tuning = Tuning::new(vec![Pitch(64)]);
    let atoms = [atom(10, 64, None), atom(11, 64, None)];
    assert!(solve(&atoms, &tuning, None).is_none());
}

#[test]
fn open_string_target_is_retained_by_the_control_map() {
    let atoms = [atom(10, 64, Some(pos(1, 0))), atom(11, 59, Some(pos(2, 0)))];
    let result = analyze_chord(
        &atoms,
        &Tuning::standard_e(),
        STANDARD_MAX_FRET,
        TargetStringConstraint {
            atom_id: 10,
            string: 1,
        },
        &ChordCostPolicy::v1_unary(),
    )
    .unwrap();
    assert!(result.target_strings.iter().any(|entry| entry.string == 1));
    assert_eq!(result.observed_best_tie_size, Some(1));
}

#[test]
fn control_map_covers_the_complete_legal_target_string_domain() {
    let tuning = Tuning::standard_e();
    let atoms = [atom(10, 64, None), atom(11, 59, None)];
    let result = analyze_chord(
        &atoms,
        &tuning,
        STANDARD_MAX_FRET,
        TargetStringConstraint {
            atom_id: 10,
            string: 2,
        },
        &ChordCostPolicy::v1_unary(),
    )
    .unwrap();
    let expected: Vec<u8> = tuning
        .candidates(Pitch(64), STANDARD_MAX_FRET)
        .into_iter()
        .map(|candidate| candidate.string)
        .collect();
    assert_eq!(
        result.target_strings.iter().map(|entry| entry.string).collect::<Vec<_>>(),
        expected
    );
}

#[test]
fn imported_human_voicing_membership_is_measured_not_assumed() {
    let atoms = [atom(10, 64, Some(pos(2, 5))), atom(11, 59, Some(pos(3, 4)))];
    let result = analyze_chord(
        &atoms,
        &Tuning::standard_e(),
        STANDARD_MAX_FRET,
        TargetStringConstraint {
            atom_id: 10,
            string: 2,
        },
        &ChordCostPolicy::v1_unary(),
    )
    .unwrap();
    let human = result.human.unwrap();
    assert!(human.feasible);
    assert!(human.satisfies_observed_constraint);
    assert!(!human.in_unconstrained_optimum);
    assert!(human.in_observed_optimum);
    assert!(human.excess_unconstrained.unwrap() > 0);
    assert_eq!(human.excess_observed, Some(0));
}

#[test]
fn optimum_and_ties_are_deterministic_and_match_small_brute_force() {
    let tuning = Tuning::new(vec![Pitch(64), Pitch(64), Pitch(59)]);
    let atoms = [atom(1, 64, None), atom(2, 64, None)];
    let a = solve(&atoms, &tuning, None).unwrap();
    let b = solve(&atoms, &tuning, None).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.optimum, -2);
    assert_eq!(a.optimum_count, 2);
    assert_eq!(a.chosen, vec![pos(1, 0), pos(2, 0)]);
}

#[test]
fn low_first_imported_tuning_keeps_its_original_string_identity() {
    let tuning = Tuning::new(vec![Pitch(40), Pitch(45), Pitch(50), Pitch(55), Pitch(59), Pitch(64)]);
    let atoms = [atom(10, 40, Some(pos(1, 0))), atom(11, 45, Some(pos(2, 0)))];
    let result = analyze_chord(
        &atoms,
        &tuning,
        STANDARD_MAX_FRET,
        TargetStringConstraint {
            atom_id: 10,
            string: 1,
        },
        &ChordCostPolicy::v1_unary(),
    )
    .unwrap();
    assert_eq!(result.observed.as_ref().unwrap().chosen[0], pos(1, 0));
    assert!(result.human.unwrap().feasible);
}
