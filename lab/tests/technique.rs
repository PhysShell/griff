//! Red → contract tests for technique-aware fingering (`technique`), oracle
//! stage: tap labels come from the tab.
//!
//! Pins, against hand computation and brute force over position assignments:
//! the tap-aware cost carries the fretting-hand anchor across tapped notes and
//! charges the picking hand its own travel; it equals `v1_cost` without taps;
//! its chain encoding scores every assignment identically, has the same
//! optimum and optimal-assignment count, and without taps reproduces the `v1`
//! chain's optimum and production path.
//!
//! Stage 2 (legato continuity): pitch-derived direction; the hard and soft
//! same-string terms and the pull-off open-string waiver, hand-computed; with
//! the legato terms off or no legato edge the objective is stage 1's; its
//! chain encoding agrees with brute force for every term.

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
    fingering::{v1_cost, TechniqueEdge, TechniqueKind},
    problems::LabError,
    technique::{
        derived_direction, tap_aware_chain, tap_aware_cost, technique_chain, technique_cost,
        Continuity, LegatoDirection, TechniqueObjective, HARD_VIOLATION,
    },
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

// ── stage 2: legato continuity ────────────────────────────────────────────────

/// Legato (imported `HammerOn`) into note `i` for each set bit `i ≥ 1`.
fn edge_flags(mask: u32, len: usize) -> Vec<TechniqueEdge> {
    (1..len)
        .filter(|&i| mask & (1 << i) != 0)
        .map(|i| TechniqueEdge::new(i - 1, i, TechniqueKind::HammerOn))
        .collect()
}

const fn objective(
    weights: FingeringWeights,
    tap_shift: i64,
    continuity: Continuity,
    pull_open_waiver: bool,
) -> TechniqueObjective {
    TechniqueObjective {
        weights,
        tap_shift,
        continuity,
        pull_open_waiver,
    }
}

/// Every legato term, alone and combined.
fn legato_objectives(w: FingeringWeights, tap_shift: i64) -> Vec<TechniqueObjective> {
    vec![
        objective(w, tap_shift, Continuity::Hard, false),
        objective(w, tap_shift, Continuity::Soft { k: 3 }, false),
        objective(w, tap_shift, Continuity::Off, true),
        objective(w, tap_shift, Continuity::Hard, true),
        objective(w, tap_shift, Continuity::Soft { k: 1 }, true),
    ]
}

#[test]
fn derived_direction_follows_pitch() {
    let p = pitches_of(&[55, 57, 57, 52]);
    assert_eq!(derived_direction(&p, 0), None);
    assert_eq!(derived_direction(&p, 1), Some(LegatoDirection::Ascending));
    assert_eq!(derived_direction(&p, 2), Some(LegatoDirection::Unison));
    assert_eq!(derived_direction(&p, 3), Some(LegatoDirection::Descending));
    assert_eq!(derived_direction(&p, 4), None);
}

#[test]
fn legato_edges_bind_their_notes_to_one_string() {
    // D string 5 hammered to 7: on the D string, or across to the G string at 2.
    let w = weights(0, -3, 1, 0);
    let pitches = pitches_of(&[55, 57]);
    let tapped = [false, false];
    let legato = [TechniqueEdge::new(0, 1, TechniqueKind::HammerOn)];
    let plain = [];
    let same = [pos(4, 5), pos(4, 7)];
    let across = [pos(4, 5), pos(3, 2)];
    let hard = objective(w, 1, Continuity::Hard, false);
    let soft = objective(w, 1, Continuity::Soft { k: 3 }, false);
    // Same string: travel 2 and nothing else.
    assert_eq!(
        technique_cost(&same, &pitches, &tapped, &legato, &hard),
        Some(2)
    );
    assert_eq!(
        technique_cost(&same, &pitches, &tapped, &legato, &soft),
        Some(2)
    );
    // Across: travel 3, plus 3 · position_shift (soft) or one violation (hard).
    assert_eq!(
        technique_cost(&across, &pitches, &tapped, &legato, &soft),
        Some(6)
    );
    assert_eq!(
        technique_cost(&across, &pitches, &tapped, &legato, &hard),
        Some(3 + HARD_VIOLATION)
    );
    // A plain edge pays nothing for the string change.
    assert_eq!(
        technique_cost(&across, &pitches, &tapped, &plain, &hard),
        Some(3)
    );
    // The soft penalty is in frets of hand travel.
    let w2 = weights(0, 0, 2, 0);
    let soft2 = objective(w2, 2, Continuity::Soft { k: 3 }, false);
    assert_eq!(
        technique_cost(&across, &pitches, &tapped, &legato, &soft2),
        Some(6 + 6)
    );
    // Every imported legato kind binds; a tapped origin too.
    for kind in [TechniqueKind::PullOff, TechniqueKind::Legato] {
        let edges = [TechniqueEdge::new(0, 1, kind)];
        assert_eq!(
            technique_cost(&across, &pitches, &tapped, &edges, &soft),
            Some(6)
        );
    }
    // Tapped 5 into fretted 7 across strings: no hand travel yet, penalty 3.
    assert_eq!(
        technique_cost(&across, &pitches, &[true, false], &legato, &soft),
        Some(3)
    );
}

#[test]
fn a_pull_off_onto_an_open_string_pays_no_open_string_penalty() {
    let w = weights(0, -3, 1, 0);
    let tapped = [false, false];
    let legato = [TechniqueEdge::new(0, 1, TechniqueKind::HammerOn)];
    let waiver = objective(w, 1, Continuity::Off, true);
    let no_waiver = objective(w, 1, Continuity::Off, false);
    // G string 2 pulled off to the open G: travel 2, open-string penalty 3.
    let down = pitches_of(&[57, 55]);
    let line = [pos(3, 2), pos(3, 0)];
    assert_eq!(
        technique_cost(&line, &down, &tapped, &legato, &no_waiver),
        Some(5)
    );
    assert_eq!(
        technique_cost(&line, &down, &tapped, &legato, &waiver),
        Some(2)
    );
    // Not across a plain edge.
    let plain = [];
    assert_eq!(
        technique_cost(&line, &down, &tapped, &plain, &waiver),
        Some(5)
    );
    // Not into an ascending edge: D 2 up to the open G.
    let up = pitches_of(&[52, 55]);
    let line_up = [pos(4, 2), pos(3, 0)];
    assert_eq!(
        technique_cost(&line_up, &up, &tapped, &legato, &waiver),
        Some(5)
    );
    // An open-string bonus is untouched (production `v1` earns 1 per open string).
    let v1 = FingeringWeights::v1();
    let cost = |waive| {
        technique_cost(
            &line,
            &down,
            &tapped,
            &legato,
            &objective(v1, 2, Continuity::Off, waive),
        )
    };
    assert_eq!(cost(true), cost(false));
}

#[test]
fn without_legato_terms_or_legato_edges_the_objective_is_tap_aware() {
    let tuning = Tuning::standard_e();
    let pitches = pitches_of(&[52, 57, 64, 59]);
    for w in weight_sets() {
        for line in assignments(&pitches, &tuning) {
            for tap_mask in [0_u32, 0b0100, 0b1010] {
                let tapped = flags(tap_mask, 4);
                let expected = tap_aware_cost(&line, &tapped, &w, 2);
                for edge_mask in 0..16 {
                    let edges = edge_flags(edge_mask, 4);
                    let off = TechniqueObjective::tap_aware(w, 2);
                    assert_eq!(
                        technique_cost(&line, &pitches, &tapped, &edges, &off),
                        expected
                    );
                }
                for obj in legato_objectives(w, 2) {
                    let plain = [];
                    assert_eq!(
                        technique_cost(&line, &pitches, &tapped, &plain, &obj),
                        expected
                    );
                }
            }
        }
    }
    let line = [pos(5, 7), pos(4, 7), pos(2, 5), pos(2, 0)];
    let obj = objective(FingeringWeights::v1(), 2, Continuity::Hard, true);
    let (tapped, edges): ([bool; 4], [TechniqueEdge; 0]) = ([false; 4], []);
    let invalid = [TechniqueEdge::new(3, 4, TechniqueKind::HammerOn)];
    assert_eq!(
        technique_cost(&line, &pitches, &tapped, &invalid, &obj),
        None
    );
    assert_eq!(
        technique_cost(&line, &pitches, &tapped[..3], &edges, &obj),
        None
    );
    assert_eq!(
        technique_cost(&line, &pitches[..3], &tapped, &edges, &obj),
        None
    );
}

#[test]
fn technique_chain_matches_brute_force_over_assignments() {
    let tuning = Tuning::standard_e();
    let lines: [&[u8]; 2] = [&[52, 57, 64, 59], &[57, 55, 60, 55]];
    for w in weight_sets() {
        for raw in lines {
            let pitches = pitches_of(raw);
            let all = assignments(&pitches, &tuning);
            for tap_mask in [0_u32, 0b0100, 0b1010] {
                let tapped = flags(tap_mask, pitches.len());
                for edge_mask in [0b0010_u32, 0b0110, 0b1110, 0b1000] {
                    let edges = edge_flags(edge_mask, pitches.len());
                    for obj in legato_objectives(w, 2) {
                        let chain = technique_chain(
                            &pitches,
                            &tuning,
                            &tapped,
                            &edges,
                            &obj,
                            STANDARD_MAX_FRET,
                        )
                        .unwrap();
                        let costs: Vec<i64> = all
                            .iter()
                            .map(|a| technique_cost(a, &pitches, &tapped, &edges, &obj).unwrap())
                            .collect();
                        let optimum = *costs.iter().min().unwrap();
                        let count = costs.iter().filter(|c| **c == optimum).count() as u64;
                        let set = optimum_set(&chain, None);
                        let case = format!("{raw:?} {tapped:?} {edges:?} {obj:?}");
                        assert_eq!(set.optimum, optimum, "{case}");
                        assert_eq!(set.count.exact, count, "{case}");
                        let path = lexicographic_path(&chain, &[0; FEATURES], None);
                        let positions = chain.positions_of(&path).unwrap();
                        assert_eq!(
                            technique_cost(&positions, &pitches, &tapped, &edges, &obj),
                            Some(optimum),
                            "{case}"
                        );
                    }
                }
            }
        }
    }
    // Every admissible state path scores its assignment exactly (one family).
    let pitches = pitches_of(&[57, 55, 60, 55]);
    let (tapped, edges) = (flags(0b0100, 4), edge_flags(0b1010, 4));
    for obj in legato_objectives(weights(0, -3, 1, 0), 1) {
        let chain =
            technique_chain(&pitches, &tuning, &tapped, &edges, &obj, STANDARD_MAX_FRET).unwrap();
        for state in state_paths(&chain) {
            let cost = chain.cost(&state).unwrap();
            let assignment = chain.positions_of(&state).unwrap();
            let direct = technique_cost(&assignment, &pitches, &tapped, &edges, &obj).unwrap();
            assert!(cost == direct || cost > direct + (1 << 40), "{obj:?}");
        }
    }
}

#[test]
fn technique_chain_matches_brute_force_for_non_adjacent_and_overlapping_edges() {
    let tuning = Tuning::standard_e();
    let pitches = pitches_of(&[52, 57, 64, 59]);
    let tapped = [false, false, true, false];
    let all = assignments(&pitches, &tuning);
    let edge_sets = [
        vec![TechniqueEdge::new(0, 3, TechniqueKind::HammerOn)],
        vec![
            TechniqueEdge::new(0, 3, TechniqueKind::HammerOn),
            TechniqueEdge::new(1, 2, TechniqueKind::PullOff),
        ],
    ];

    for edges in edge_sets {
        for obj in legato_objectives(weights(0, -3, 1, 0), 2) {
            let chain =
                technique_chain(&pitches, &tuning, &tapped, &edges, &obj, STANDARD_MAX_FRET)
                    .unwrap();
            let costs: Vec<i64> = all
                .iter()
                .map(|assignment| {
                    technique_cost(assignment, &pitches, &tapped, &edges, &obj).unwrap()
                })
                .collect();
            let optimum = *costs.iter().min().unwrap();
            let count = costs.iter().filter(|cost| **cost == optimum).count() as u64;
            let set = optimum_set(&chain, None);
            let case = format!("{edges:?} {obj:?}");

            assert_eq!(set.optimum, optimum, "{case}");
            assert_eq!(set.count.exact, count, "{case}");
            let positions = chain
                .positions_of(&lexicographic_path(&chain, &[0; FEATURES], None))
                .unwrap();
            assert_eq!(
                technique_cost(&positions, &pitches, &tapped, &edges, &obj),
                Some(optimum),
                "{case}"
            );
        }
    }
}

#[test]
fn technique_chain_without_legato_edges_is_the_tap_aware_chain() {
    let tuning = Tuning::standard_e();
    for w in weight_sets() {
        for raw in [
            &[40_u8, 52, 57, 64, 59, 47][..],
            &[64, 62, 60, 59, 57, 55][..],
        ] {
            let pitches = pitches_of(raw);
            for tap_mask in [0_u32, 0b00_0100, 0b10_1001] {
                let tapped = flags(tap_mask, raw.len());
                let aware =
                    tap_aware_chain(&pitches, &tuning, &w, 3, &tapped, STANDARD_MAX_FRET).unwrap();
                let plain = Vec::new();
                for obj in legato_objectives(w, 3) {
                    let chain = technique_chain(
                        &pitches,
                        &tuning,
                        &tapped,
                        &plain,
                        &obj,
                        STANDARD_MAX_FRET,
                    )
                    .unwrap();
                    let (a, b) = (optimum_set(&aware, None), optimum_set(&chain, None));
                    assert_eq!((a.optimum, a.count.exact), (b.optimum, b.count.exact));
                    let zero = [0; FEATURES];
                    assert_eq!(
                        aware.positions_of(&lexicographic_path(&aware, &zero, None)),
                        chain.positions_of(&lexicographic_path(&chain, &zero, None))
                    );
                }
            }
        }
    }
}

#[test]
fn technique_chain_refuses_bad_input() {
    let tuning = Tuning::standard_e();
    let obj = objective(FingeringWeights::v1(), 2, Continuity::Hard, true);
    assert_eq!(
        technique_chain(&[], &tuning, &[], &[], &obj, STANDARD_MAX_FRET),
        Err(LabError::EmptyLine)
    );
    let pitches = pitches_of(&[40, 45]);
    assert_eq!(
        technique_chain(
            &pitches,
            &tuning,
            &[false, false],
            &[TechniqueEdge::new(0, 2, TechniqueKind::HammerOn)],
            &obj,
            STANDARD_MAX_FRET
        ),
        Err(LabError::InvalidTechniqueEdge {
            notes: 2,
            from: 0,
            to: 2
        })
    );
    assert_eq!(
        technique_chain(&pitches, &tuning, &[false], &[], &obj, STANDARD_MAX_FRET),
        Err(LabError::LabelLength {
            notes: 2,
            labels: 1
        })
    );
}
