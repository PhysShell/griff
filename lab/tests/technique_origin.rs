#![allow(clippy::unwrap_used, clippy::missing_assert_message)]

use griff_constraint_lab::{
    technique_origin::{
        conditioned_profile, estimate_primary, estimate_with_hand, technique_feasible_strings,
        BlindOriginProblem, HandEstimate, IntentAwareOriginProblem, ObservedOriginRealization,
        OriginIdentity, OriginStringEstimate, TechniqueIntent,
    },
    ties::{lexicographic_path, Chain, Features, FEATURES},
};
use griff_core::{
    event::{FretboardPosition, Pitch, Tuning},
    fretboard::FingeringWeights,
};

fn weights() -> FingeringWeights {
    FingeringWeights {
        fret: 0,
        open_string: -3,
        position_shift: 1,
        string_change: 0,
    }
}

#[test]
fn restricted_full_chain_dp_not_canonical_ambiguous_representative() {
    let chain = Chain::v1(
        &[Pitch(59), Pitch(64), Pitch(59)],
        &Tuning::standard_e(),
        &FingeringWeights {
            fret: 0,
            open_string: 0,
            position_shift: 1,
            string_change: 0,
        },
        24,
    )
    .unwrap();
    let profile = conditioned_profile(&chain, 1);
    let allowed: Vec<_> = profile
        .entries()
        .iter()
        .filter(|entry| entry.delta() == 0)
        .map(|entry| entry.string())
        .collect();
    let restricted = chain.restrict_note_strings(1, &allowed).unwrap();
    let weights: Features = [0; FEATURES];
    let path = lexicographic_path(&restricted, &weights, None);
    let selected = restricted.positions_of(&path).unwrap()[1].string;
    assert!(allowed.len() > 1);
    assert_ne!(selected, allowed[0]);
}

#[test]
fn reported_restricted_string_matches_actual_dp() {
    let chain = Chain::v1(
        &[Pitch(59), Pitch(64), Pitch(67), Pitch(59)],
        &Tuning::standard_e(),
        &weights(),
        24,
    )
    .unwrap();
    for allowed in [vec![1, 2], vec![2, 3, 4], vec![1, 3, 5]] {
        let Some(restricted) = chain.clone().restrict_note_strings(1, &allowed) else {
            continue;
        };
        let path = lexicographic_path(&restricted, &[0; FEATURES], None);
        let selected = restricted.positions_of(&path).unwrap()[1].string;
        assert!(allowed.contains(&selected));
    }
}

fn identity() -> OriginIdentity {
    OriginIdentity::new("song.gp", 1, 0, 17, 23, 480, 960)
}

#[test]
fn estimator_inputs_are_structurally_separate_from_labels() {
    let blind = BlindOriginProblem::new(identity(), &Tuning::standard_e(), 24, Pitch(64));
    let label = ObservedOriginRealization::new(FretboardPosition { string: 1, fret: 5 });
    let blind_json = serde_json::to_value(&blind).unwrap();
    assert!(blind_json.get("origin_string").is_none());
    assert!(blind_json.get("origin_fret").is_none());
    assert!(blind_json.get("target_pitch").is_none());
    assert_eq!(label.position().string, 1);
}

#[test]
fn intent_exposes_target_score_identity_but_not_realization() {
    let intent = TechniqueIntent::new(23, Pitch(67), 960);
    let problem = IntentAwareOriginProblem::new(
        BlindOriginProblem::new(identity(), &Tuning::standard_e(), 24, Pitch(64)),
        intent,
    );
    let json = serde_json::to_value(&problem).unwrap();
    assert_eq!(json.pointer("/intent/target_note_id"), Some(&23.into()));
    assert_eq!(json.pointer("/intent/target_pitch"), Some(&67.into()));
    assert!(json.pointer("/intent/target_string").is_none());
    assert!(json.pointer("/intent/target_fret").is_none());
}

#[test]
fn conditioned_profile_matches_brute_force_costs() {
    let chain = Chain::v1(
        &[Pitch(64), Pitch(67), Pitch(69)],
        &Tuning::standard_e(),
        &weights(),
        24,
    )
    .unwrap();
    let profile = conditioned_profile(&chain, 1);
    for entry in profile.entries() {
        let conditioned = chain.clone().condition_string(1, entry.string()).unwrap();
        let mut best = i64::MAX;
        for a in 0..conditioned.candidates(0).len() {
            for b in 0..conditioned.candidates(1).len() {
                for c in 0..conditioned.candidates(2).len() {
                    best = best.min(conditioned.cost(&[a, b, c]).unwrap());
                }
            }
        }
        assert_eq!(entry.cost(), best);
    }
}

#[test]
fn dense_rank_and_positive_delta_are_explicit() {
    let chain = Chain::v1(
        &[Pitch(64), Pitch(76)],
        &Tuning::standard_e(),
        &FingeringWeights {
            fret: 1,
            open_string: 0,
            position_shift: 1,
            string_change: 0,
        },
        24,
    )
    .unwrap();
    let profile = conditioned_profile(&chain, 0);
    assert!(profile.entries().iter().any(|entry| entry.delta() > 0));
    assert_eq!(profile.entries()[0].dense_rank(), 1);
}

#[test]
fn equivalent_primary_strings_remain_ambiguous() {
    let chain = Chain::v1(
        &[Pitch(64)],
        &Tuning::standard_e(),
        &FingeringWeights {
            fret: 0,
            open_string: 0,
            position_shift: 0,
            string_change: 0,
        },
        24,
    )
    .unwrap();
    let estimate = estimate_primary(&conditioned_profile(&chain, 0), None);
    assert!(matches!(estimate, OriginStringEstimate::Ambiguous { .. }));
    assert!(estimate.strings().len() > 1);
}

#[test]
fn unique_primary_string_is_known() {
    let chain = Chain::v1(&[Pitch(40)], &Tuning::standard_e(), &weights(), 24).unwrap();
    assert_eq!(
        estimate_primary(&conditioned_profile(&chain, 0), None),
        OriginStringEstimate::Known { string: 6 }
    );
}

#[test]
fn technique_feasibility_uses_pitches_not_target_realization() {
    let tuning = Tuning::standard_e();
    let strings = technique_feasible_strings(&tuning, 12, Pitch(64), Pitch(76));
    assert!(strings.iter().all(|&string| {
        tuning
            .candidates(Pitch(64), 12)
            .iter()
            .any(|p| p.string == string)
            && tuning
                .candidates(Pitch(76), 12)
                .iter()
                .any(|p| p.string == string)
    }));
    assert!(!strings.is_empty());
}

#[test]
fn target_unreachable_string_is_removed() {
    let tuning = Tuning::standard_e();
    let origin: Vec<_> = tuning
        .candidates(Pitch(64), 12)
        .into_iter()
        .map(|p| p.string)
        .collect();
    let feasible = technique_feasible_strings(&tuning, 12, Pitch(64), Pitch(76));
    assert!(origin.iter().any(|string| !feasible.contains(string)));
}

#[test]
fn known_hand_breaks_only_primary_ties_lexicographically() {
    let chain = Chain::v1(
        &[Pitch(64)],
        &Tuning::standard_e(),
        &FingeringWeights {
            fret: 0,
            open_string: 0,
            position_shift: 0,
            string_change: 0,
        },
        24,
    )
    .unwrap();
    let profile = conditioned_profile(&chain, 0);
    let estimate = estimate_with_hand(&profile, None, HandEstimate::Known(5));
    assert_eq!(estimate, OriginStringEstimate::Known { string: 2 });
}

#[test]
fn unknown_hand_does_not_become_zero_or_absent() {
    let chain = Chain::v1(
        &[Pitch(64)],
        &Tuning::standard_e(),
        &FingeringWeights {
            fret: 0,
            open_string: 0,
            position_shift: 0,
            string_change: 0,
        },
        24,
    )
    .unwrap();
    let profile = conditioned_profile(&chain, 0);
    assert_eq!(
        estimate_with_hand(&profile, None, HandEstimate::Unknown),
        estimate_primary(&profile, None)
    );
    assert_ne!(HandEstimate::Unknown, HandEstimate::Absent);
}

#[test]
fn allowed_domain_is_sorted_and_permutation_stable() {
    let chain = Chain::v1(
        &[Pitch(64)],
        &Tuning::standard_e(),
        &FingeringWeights {
            fret: 0,
            open_string: 0,
            position_shift: 0,
            string_change: 0,
        },
        24,
    )
    .unwrap();
    let profile = conditioned_profile(&chain, 0);
    let a = estimate_primary(&profile, Some(&[4, 1, 3, 2]));
    let b = estimate_primary(&profile, Some(&[2, 3, 1, 4]));
    assert_eq!(a, b);
    assert!(a.strings().windows(2).all(|pair| pair[0] < pair[1]));
}
