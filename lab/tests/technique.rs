//! Red → contract tests for technique-aware fingering (`technique`), oracle
//! stage: tap labels come from the tab.
//!
//! Pins, against hand computation and brute force over position assignments:
//! the tap-aware cost carries the fretting-hand anchor across tapped notes and
//! charges the picking hand its own travel; it equals `v1_cost` without taps;
//! its chain encoding scores every assignment identically, has the same
//! optimum and optimal-assignment count, and without taps reproduces the `v1`
//! chain's optimum and production path.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use griff_constraint_lab::{
    fingering::v1_cost,
    problems::LabError,
    technique::{tap_aware_chain, tap_aware_cost},
    ties::{lexicographic_path, optimum_set, Chain, FEATURES},
};
use griff_core::{
    event::{FretboardPosition, Pitch, Tuning},
    fretboard::{FingeringWeights, STANDARD_MAX_FRET},
};

fn pitch(p: u8) -> Pitch {
    Pitch::new(p).expect("valid pitch")
}

fn pitches_of(raw: &[u8]) -> Vec<Pitch> {
    raw.iter().map(|&p| pitch(p)).collect()
}

fn pos(string: u8, fret: u8) -> FretboardPosition {
    FretboardPosition { string, fret }
}

fn weights(fret: i64, open: i64, shift: i64, change: i64) -> FingeringWeights {
    FingeringWeights {
        fret,
        open_string: open,
        position_shift: shift,
        string_change: change,
    }
}

fn weight_sets() -> Vec<FingeringWeights> {
    vec![
        FingeringWeights::v1(),
        weights(0, -3, 1, 0),
        weights(2, 4, 0, 3),
    ]
}

/// Every position assignment of a line.
fn assignments(pitches: &[Pitch], tuning: &Tuning) -> Vec<Vec<FretboardPosition>> {
    let mut out = vec![Vec::new()];
    for &p in pitches {
        let cands = tuning.candidates(p, STANDARD_MAX_FRET);
        out = out
            .into_iter()
            .flat_map(|prefix| {
                cands.iter().map(move |&c| {
                    let mut next = prefix.clone();
                    next.push(c);
                    next
                })
            })
            .collect();
    }
    out
}

/// Every state path of a chain, as candidate indices.
fn state_paths(chain: &Chain) -> Vec<Vec<usize>> {
    let mut out = vec![Vec::new()];
    for note in 0..chain.len() {
        let k = chain.candidates(note).len();
        out = out
            .into_iter()
            .flat_map(|prefix| {
                (0..k).map(move |c| {
                    let mut next = prefix.clone();
                    next.push(c);
                    next
                })
            })
            .collect();
    }
    out
}

fn flags(mask: u32, len: usize) -> Vec<bool> {
    (0..len).map(|i| mask & (1 << i) != 0).collect()
}

#[test]
fn tapped_notes_do_not_move_the_fretting_hand() {
    // D string 5, 8, tap 12, 8, 5 with v1-fit weights (shift 1).
    let line = [pos(4, 5), pos(4, 8), pos(4, 12), pos(4, 8), pos(4, 5)];
    let w = weights(0, -3, 1, 0);
    assert_eq!(v1_cost(&line, &w), 14, "tap-blind: 3 + 4 + 4 + 3");
    let tapped = [false, false, true, false, false];
    // Fretting hand 5 → 8 → (8, across the tap) → 5: 3 + 0 + 3; first tap free.
    assert_eq!(tap_aware_cost(&line, &tapped, &w, 1), Some(6));
    // Two taps: the picking hand pays its own travel, 12 → 15 at tap_shift 2.
    let line = [pos(4, 5), pos(4, 12), pos(4, 8), pos(4, 15), pos(4, 5)];
    let tapped = [false, true, false, true, false];
    // Fretting: 5 → 8 (3) → 5 (3); picking: 12 → 15 (3 · 2).
    assert_eq!(tap_aware_cost(&line, &tapped, &w, 2), Some(12));
    assert_eq!(tap_aware_cost(&line, &tapped[..4], &w, 2), None);
}

#[test]
fn tap_aware_cost_charges_string_changes_between_neighbours() {
    let w = weights(0, 0, 1, 5);
    let line = [pos(4, 7), pos(3, 9), pos(4, 7)];
    // Fretting hand stays at 7 across the tap; two string changes.
    assert_eq!(
        tap_aware_cost(&line, &[false, true, false], &w, 1),
        Some(10)
    );
}

#[test]
fn tap_aware_cost_without_taps_is_v1_cost() {
    let tuning = Tuning::standard_e();
    for w in weight_sets() {
        for line in assignments(&pitches_of(&[40, 52, 57, 64]), &tuning) {
            assert_eq!(
                tap_aware_cost(&line, &[false; 4], &w, 7),
                Some(v1_cost(&line, &w))
            );
        }
    }
}

#[test]
fn tap_aware_chain_matches_brute_force_over_assignments() {
    let tuning = Tuning::standard_e();
    let lines: [&[u8]; 3] = [&[52, 57, 64, 59], &[47, 55, 62, 55], &[45, 50, 57, 64]];
    for w in weight_sets() {
        for raw in lines {
            let pitches = pitches_of(raw);
            for mask in 0..(1_u32 << pitches.len()) {
                let tapped = flags(mask, pitches.len());
                let chain =
                    tap_aware_chain(&pitches, &tuning, &w, 2, &tapped, STANDARD_MAX_FRET).unwrap();
                let costs: Vec<i64> = assignments(&pitches, &tuning)
                    .iter()
                    .map(|a| tap_aware_cost(a, &tapped, &w, 2).unwrap())
                    .collect();
                let optimum = *costs.iter().min().unwrap();
                let count = costs.iter().filter(|c| **c == optimum).count() as u64;
                let set = optimum_set(&chain, None);
                assert_eq!(set.optimum, optimum, "{raw:?} {tapped:?} {w:?}");
                assert_eq!(set.count.exact, count, "{raw:?} {tapped:?} {w:?}");

                let path = lexicographic_path(&chain, &[0; FEATURES], None);
                let positions = chain.positions_of(&path).unwrap();
                assert_eq!(tap_aware_cost(&positions, &tapped, &w, 2), Some(optimum));
                // Every admissible state path scores its assignment exactly.
                for state in state_paths(&chain) {
                    let cost = chain.cost(&state).unwrap();
                    let assignment = chain.positions_of(&state).unwrap();
                    let direct = tap_aware_cost(&assignment, &tapped, &w, 2).unwrap();
                    assert!(cost == direct || cost > direct + 1_000_000);
                }
            }
        }
    }
}

#[test]
fn tap_aware_chain_without_taps_is_the_v1_chain() {
    let tuning = Tuning::standard_e();
    for w in weight_sets() {
        for raw in [
            &[40_u8, 52, 57, 64, 59, 47][..],
            &[64, 62, 60, 59, 57, 55][..],
        ] {
            let pitches = pitches_of(raw);
            let aware = tap_aware_chain(
                &pitches,
                &tuning,
                &w,
                3,
                &vec![false; raw.len()],
                STANDARD_MAX_FRET,
            )
            .unwrap();
            let blind = Chain::v1(&pitches, &tuning, &w, STANDARD_MAX_FRET).unwrap();
            let (a, b) = (optimum_set(&aware, None), optimum_set(&blind, None));
            assert_eq!((a.optimum, a.count.exact), (b.optimum, b.count.exact));
            let zero = [0; FEATURES];
            assert_eq!(
                aware.positions_of(&lexicographic_path(&aware, &zero, None)),
                blind.positions_of(&lexicographic_path(&blind, &zero, None))
            );
        }
    }
}

#[test]
fn tap_aware_chain_refuses_bad_input() {
    let tuning = Tuning::standard_e();
    let w = FingeringWeights::v1();
    assert_eq!(
        tap_aware_chain(&[], &tuning, &w, 1, &[], STANDARD_MAX_FRET),
        Err(LabError::EmptyLine)
    );
    assert_eq!(
        tap_aware_chain(
            &pitches_of(&[40, 45]),
            &tuning,
            &w,
            1,
            &[true],
            STANDARD_MAX_FRET
        ),
        Err(LabError::LabelLength {
            notes: 2,
            labels: 1
        })
    );
    assert_eq!(
        tap_aware_chain(
            &pitches_of(&[40, 30]),
            &tuning,
            &w,
            1,
            &[false, true],
            STANDARD_MAX_FRET
        ),
        Err(LabError::UnpositionablePitch {
            index: 1,
            pitch: 30
        })
    );
}
