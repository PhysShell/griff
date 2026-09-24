#![allow(clippy::unwrap_used, clippy::missing_assert_message)]

use griff_constraint_lab::{
    technique_origin::{
        conditioned_profile, deterministic_restricted_string, estimate_primary, estimate_with_hand,
        technique_feasible_strings, BlindOriginProblem, HandEstimate, IntentAwareOriginProblem,
        ObservedOriginRealization, OriginIdentity, OriginStringEstimate, TechniqueIntent,
    },
    ties::{lexicographic_path, Chain, FEATURES},
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
fn restricted_reporting_matches_full_chain_dp_on_generated_ties() {
    let tuning = Tuning::standard_e();
    let mut tied_profiles = 0;
    for first in 40..=76 {
        for origin in 40..=76 {
            for last in 40..=76 {
                let chain = Chain::v1(
                    &[Pitch(first), Pitch(origin), Pitch(last)],
                    &tuning,
                    &FingeringWeights {
                        fret: 0,
                        open_string: 0,
                        position_shift: 1,
                        string_change: 1,
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
                if allowed.len() < 2 {
                    continue;
                }
                tied_profiles += 1;
                let restricted = chain.restrict_note_strings(1, &allowed).unwrap();
                let path = lexicographic_path(&restricted, &[0; FEATURES], None);
                let selected = restricted.positions_of(&path).unwrap()[1].string;
                assert_eq!(
                    deterministic_restricted_string(
                        &restricted,
                        1,
                        &profile,
                        &allowed,
                        HandEstimate::Unknown
                    ),
                    Some(selected)
                );
            }
        }
    }
    assert!(tied_profiles > 1_000);
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

/// Ground truth for [`deterministic_restricted_string`] on a 3-note chain,
/// independent of [`conditioned_profile`]/[`estimate_with_hand`]/
/// [`Chain::restrict_note_strings`]: brute forces every path, keeping only
/// those whose `note` string is in `allowed`, and returns the `note` string of
/// the one minimizing `(primary cost, hand distance at note)` lexicographically
/// — the registered `abs(origin_fret(s) - h)` secondary, charged once, only at
/// `note`. Remaining ties keep the lowest candidate-index path, matching
/// ascending enumeration order.
type BruteForceKey = (i64, u8, [usize; 3]);

fn brute_force_deterministic_string(
    chain: &Chain,
    note: usize,
    allowed: &[u8],
    hand: HandEstimate,
) -> Option<u8> {
    assert_eq!(chain.len(), 3, "brute force helper assumes a 3-note chain");
    let mut best: Option<BruteForceKey> = None;
    for a in 0..chain.candidates(0).len() {
        for b in 0..chain.candidates(1).len() {
            for c in 0..chain.candidates(2).len() {
                let path = [a, b, c];
                let positions = chain.positions_of(&path).unwrap();
                if !allowed.contains(&positions[note].string) {
                    continue;
                }
                let cost = chain.cost(&path).unwrap();
                let hand_cost = match hand {
                    HandEstimate::Known(anchor) => positions[note].fret.abs_diff(anchor),
                    HandEstimate::Unknown | HandEstimate::Absent => 0,
                };
                if best.is_none_or(|(bc, bh, _)| (cost, hand_cost) < (bc, bh)) {
                    best = Some((cost, hand_cost, path));
                }
            }
        }
    }
    best.map(|(_, _, path)| chain.positions_of(&path).unwrap()[note].string)
}

/// Differential coverage of [`deterministic_restricted_string`] with a known
/// hand anchor, over many generated 3-note chains: the bug class the generated
/// `HandEstimate::Unknown` sweep above cannot reach, since it never exercises
/// the `with_anchor` branch.
#[test]
fn deterministic_restricted_string_matches_brute_force_with_known_hand() {
    let tuning = Tuning::standard_e();
    let production_weights = FingeringWeights {
        fret: 0,
        open_string: -3,
        position_shift: 1,
        string_change: 0,
    };
    let mut checked = 0;
    for first in (40..=76).step_by(9) {
        for origin in 40..=76 {
            for last in (40..=76).step_by(9) {
                let chain = Chain::v1(
                    &[Pitch(first), Pitch(origin), Pitch(last)],
                    &tuning,
                    &production_weights,
                    24,
                )
                .unwrap();
                let profile = conditioned_profile(&chain, 1);
                let allowed: Vec<_> = profile
                    .entries()
                    .iter()
                    .map(|entry| entry.string())
                    .collect();
                if allowed.len() < 2 {
                    continue;
                }
                for anchor in [0_u8, 5, 10, 17] {
                    let hand = HandEstimate::Known(anchor);
                    checked += 1;
                    assert_eq!(
                        deterministic_restricted_string(&chain, 1, &profile, &allowed, hand),
                        brute_force_deterministic_string(&chain, 1, &allowed, hand),
                        "first={first} origin={origin} last={last} anchor={anchor}"
                    );
                }
            }
        }
    }
    assert!(checked > 1_000);
}

/// The reviewed regression: an open neighboring note must not out-vote the
/// origin-local hand distance. `[40, 60, 64]` gives the origin note (pitch 60,
/// index one) two technique-domain (strings 2-6) primary-optimal paths,
/// through origin string 3 and through origin string 2 via the target's open
/// string-1 candidate. The registered secondary is `abs(origin_fret(s) - h)`,
/// evaluated only at the origin; it must not become the whole path's summed
/// `anchor_distance`, which charges the target note too and lets its open
/// (zero-distance) candidate win regardless of the origin's own distance from
/// the anchor.
#[test]
fn known_hand_ignores_anchor_distance_at_an_open_neighbor() {
    let tuning = Tuning::standard_e();
    let production_weights = FingeringWeights {
        fret: 0,
        open_string: -3,
        position_shift: 1,
        string_change: 0,
    };
    let chain = Chain::v1(
        &[Pitch(40), Pitch(60), Pitch(64)],
        &tuning,
        &production_weights,
        24,
    )
    .unwrap();
    let allowed = vec![2, 3, 4, 5, 6];
    let profile = conditioned_profile(&chain, 1);
    let min_cost = profile
        .entries()
        .iter()
        .filter(|entry| allowed.contains(&entry.string()))
        .map(|entry| entry.cost())
        .min()
        .unwrap();
    let tied = profile
        .entries()
        .iter()
        .filter(|entry| allowed.contains(&entry.string()) && entry.cost() == min_cost)
        .count();
    assert!(
        tied >= 2,
        "fixture must exercise a genuine origin-string tie, found {tied}"
    );

    let hand = HandEstimate::Known(10);
    let expected = brute_force_deterministic_string(&chain, 1, &allowed, hand);
    assert_eq!(
        deterministic_restricted_string(&chain, 1, &profile, &allowed, hand),
        expected
    );
    assert_eq!(expected, Some(3));
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
