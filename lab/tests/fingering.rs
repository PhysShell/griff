//! Red → contract tests for the fingering optimality experiment (`fingering`).
//!
//! Pins: how a Guitar Pro track becomes human-fingered tablature lines (and
//! where every refused note is counted); that the mirrored `v1` objective is
//! the one the production DP minimizes; that the `v1` and hand-model IR
//! encodings score exactly like the domain evaluators; and that the in-repo
//! hand DP is optimal — all against brute force on exhaustive small families.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::type_complexity
)]

use griff_constraint_lab::{
    fingering::{
        best_hands, decode_positions, encode_hand_witness, encode_v1_witness, hand_cost,
        hand_problem, holdout_bucket, solve_hand, song_key, tab_lines, v1_cost, v1_problem,
        CutStats, HandError, HandModel, HandModelError, HandWeights, LineCut, Reach,
        HAND_VARS_PER_NOTE, V1_VARS_PER_NOTE,
    },
    optir::{Term, WitnessError},
    problems::LabError,
};
use griff_core::{
    event::{
        FretboardPosition, NoteMarks, NotePosition, Pitch, Tempo, Ticks, TimeSignature, Tuning,
        Velocity,
    },
    fretboard::{infer_positions, FingeringWeights, STANDARD_MAX_FRET},
    score::{
        AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, RepeatMarker,
        Score, Track, Voice,
    },
    slice::TickRange,
};

const Q: u32 = 480;

fn pitch(p: u8) -> Pitch {
    Pitch::new(p).expect("valid pitch")
}

fn pos(string: u8, fret: u8) -> FretboardPosition {
    FretboardPosition { string, fret }
}

fn note(onset: u32, p: u8, position: Option<(u8, u8)>) -> AtomEvent {
    AtomEvent::Note(AtomNote {
        absolute_start: Ticks(onset),
        duration: Ticks(Q),
        pitch: pitch(p),
        velocity: Velocity::new(90).expect("velocity"),
        marks: NoteMarks::empty(),
        position: position.map(|(s, f)| NotePosition::explicit(pos(s, f))),
    })
}

fn group(atoms: Vec<AtomEvent>) -> EventGroup {
    EventGroup {
        kind: if atoms.len() > 1 {
            EventGroupKind::Chord
        } else {
            EventGroupKind::Single
        },
        atoms,
        technique_spans: Vec::new(),
    }
}

fn score(voices: Vec<Vec<EventGroup>>) -> Score {
    Score {
        ticks_per_quarter: 480,
        master_bars: vec![MasterBar {
            index: 0,
            tick_range: TickRange::new(Ticks(0), Ticks(64 * Q)).expect("ordered"),
            time_signature: TimeSignature {
                numerator: 4,
                denominator: 4,
            },
            tempo: Tempo::from_bpm_integer(120).expect("tempo"),
            repeat: RepeatMarker::default(),
        }],
        tracks: vec![Track {
            name: Some("Guitar".into()),
            channel: 0,
            voices: voices
                .into_iter()
                .enumerate()
                .map(|(i, event_groups)| Voice {
                    id: i as u8,
                    event_groups,
                })
                .collect(),
            tuning: Tuning::standard_e(),
        }],
        source_meta: None,
        loss: LossReport::new(),
    }
}

fn single(onset: u32, p: u8, position: Option<(u8, u8)>) -> EventGroup {
    group(vec![note(onset, p, position)])
}

// ── tablature lines ───────────────────────────────────────────────────────────

/// Voice 0 exercises every cut cause once; voice 1 is one clean line sharing
/// voice 0's onsets (chords are per voice, not per track).
fn cut_fixture() -> Score {
    let voice0 = vec![
        single(0, 40, Some((6, 0))),
        single(Q, 45, Some((5, 0))),
        single(2 * Q, 47, Some((5, 2))),
        single(3 * Q, 50, Some((4, 0))),
        group(vec![
            note(4 * Q, 52, Some((4, 2))),
            note(4 * Q, 55, Some((3, 0))),
        ]),
        single(5 * Q, 57, Some((3, 2))),
        single(6 * Q, 59, Some((2, 0))),
        single(7 * Q, 60, None),
        single(8 * Q, 62, Some((2, 3))),
        single(9 * Q, 64, Some((1, 0))),
        single(10 * Q, 65, Some((1, 1))),
        single(11 * Q, 67, Some((1, 3))),
        // 4-quarter rest after the note ending at 12Q.
        single(16 * Q, 89, Some((1, 25))),
        single(17 * Q, 60, Some((2, 0))),
        single(18 * Q, 64, Some((2, 5))),
        single(19 * Q, 66, Some((2, 7))),
        single(20 * Q, 67, Some((2, 8))),
        single(21 * Q, 69, Some((2, 10))),
    ];
    let voice1 = vec![
        single(0, 52, Some((5, 7))),
        single(Q, 55, Some((5, 10))),
        single(2 * Q, 57, Some((4, 7))),
        single(3 * Q, 59, Some((4, 9))),
    ];
    score(vec![voice0, voice1])
}

#[test]
fn tab_lines_cut_at_every_cause_and_count_it() {
    let (lines, stats) = tab_lines(&cut_fixture(), 0, &LineCut::v1()).expect("track 0");
    let summary: Vec<(u8, u32, Vec<u8>)> = lines
        .iter()
        .map(|l| {
            (
                l.voice,
                l.start_tick,
                l.pitches.iter().map(|p| p.0).collect(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        vec![
            (0, 0, vec![40, 45, 47, 50]),
            (0, 8 * Q, vec![62, 64, 65, 67]),
            (0, 18 * Q, vec![64, 66, 67, 69]),
            (1, 0, vec![52, 55, 57, 59]),
        ]
    );
    assert_eq!(
        lines[1].human,
        vec![pos(2, 3), pos(1, 0), pos(1, 1), pos(1, 3)]
    );
    assert!(lines
        .iter()
        .all(|l| l.track == 0 && l.tuning == Tuning::standard_e()));
    assert!(lines.iter().all(|l| l.human.len() == l.pitches.len()));
    assert_eq!(
        stats,
        CutStats {
            notes_seen: 23,
            chord_onsets: 1,
            unpositioned: 1,
            beyond_max_fret: 1,
            pitch_mismatch: 1,
            rest_cuts: 1,
            short_lines: 1,
            short_line_notes: 2,
            kept_lines: 4,
            kept_notes: 16,
        }
    );
}

#[test]
fn rest_cut_threshold_is_inclusive_and_can_be_disabled() {
    let line = |gap_onset: u32| {
        score(vec![vec![
            single(0, 40, Some((6, 0))),
            single(Q, 45, Some((5, 0))),
            single(gap_onset, 50, Some((4, 0))),
            single(gap_onset + Q, 55, Some((3, 0))),
        ]])
    };
    let cut = LineCut {
        min_notes: 2,
        max_rest_quarters: 4,
        max_fret: STANDARD_MAX_FRET,
    };
    // The second note ends at 2Q; a rest of exactly 4Q cuts, one tick less does not.
    let (lines, stats) = tab_lines(&line(6 * Q), 0, &cut).unwrap();
    assert_eq!((lines.len(), stats.rest_cuts), (2, 1));
    let (lines, stats) = tab_lines(&line(6 * Q - 1), 0, &cut).unwrap();
    assert_eq!((lines.len(), stats.rest_cuts), (1, 0));
    let no_rests = LineCut {
        max_rest_quarters: 0,
        ..cut
    };
    let (lines, _) = tab_lines(&line(40 * Q), 0, &no_rests).unwrap();
    assert_eq!(lines.len(), 1);
}

#[test]
fn tab_lines_sort_onsets_within_a_voice() {
    let s = score(vec![vec![
        single(2 * Q, 47, Some((5, 2))),
        single(0, 40, Some((6, 0))),
        single(3 * Q, 50, Some((4, 0))),
        single(Q, 45, Some((5, 0))),
    ]]);
    let (lines, _) = tab_lines(&s, 0, &LineCut::v1()).unwrap();
    assert_eq!(
        lines[0].pitches,
        vec![pitch(40), pitch(45), pitch(47), pitch(50)]
    );
}

#[test]
fn tab_lines_refuse_a_missing_track() {
    assert_eq!(
        tab_lines(&cut_fixture(), 1, &LineCut::v1()),
        Err(LabError::NoSuchTrack { index: 1 })
    );
}

#[test]
fn cut_stats_absorb_adds_fieldwise() {
    let (_, a) = tab_lines(&cut_fixture(), 0, &LineCut::v1()).unwrap();
    let mut total = CutStats::default();
    total.absorb(&a);
    total.absorb(&a);
    assert_eq!(total.notes_seen, 2 * a.notes_seen);
    assert_eq!(total.kept_notes, 2 * a.kept_notes);
    assert_eq!(total.rest_cuts, 2 * a.rest_cuts);
    assert_eq!(total.short_line_notes, 2 * a.short_line_notes);
}

// ── brute force helpers ───────────────────────────────────────────────────────

fn candidate_lines(
    pitches: &[Pitch],
    tuning: &Tuning,
    max_fret: u8,
) -> Vec<Vec<FretboardPosition>> {
    let mut out = vec![Vec::new()];
    for &p in pitches {
        let cands = tuning.candidates(p, max_fret);
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

fn sequences(alphabet: &[u8], len: usize) -> Vec<Vec<u8>> {
    let mut out = vec![Vec::new()];
    for _ in 0..len {
        out = out
            .into_iter()
            .flat_map(|prefix| {
                alphabet.iter().map(move |&a| {
                    let mut next = prefix.clone();
                    next.push(a);
                    next
                })
            })
            .collect();
    }
    out
}

fn pitches_of(raw: &[u8]) -> Vec<Pitch> {
    raw.iter().map(|&p| pitch(p)).collect()
}

/// Deterministic pseudo-random lines (xorshift) over the guitar range.
fn lcg_lines(count: usize, len: usize, lo: u8, hi: u8) -> Vec<Vec<Pitch>> {
    let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
    let span = u64::from(hi - lo + 1);
    (0..count)
        .map(|_| {
            (0..len)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    pitch(lo + (state % span) as u8)
                })
                .collect()
        })
        .collect()
}

const V1_PITCHES: [u8; 8] = [40, 45, 47, 52, 55, 59, 64, 71];

fn v1_weight_sets() -> Vec<FingeringWeights> {
    vec![
        FingeringWeights::v1(),
        FingeringWeights {
            fret: 0,
            open_string: 0,
            position_shift: 0,
            string_change: 0,
        },
        FingeringWeights {
            fret: 0,
            open_string: 3,
            position_shift: 1,
            string_change: 4,
        },
        FingeringWeights {
            fret: 2,
            open_string: -2,
            position_shift: 5,
            string_change: 0,
        },
    ]
}

// ── v1 objective ──────────────────────────────────────────────────────────────

#[test]
fn v1_cost_scores_a_hand_computed_line() {
    // unary: (0 − 1) + 2 + 5 = 6; steps: (2·2 + 1) + (3·2 + 0) = 11.
    let line = [pos(6, 0), pos(5, 2), pos(5, 5)];
    assert_eq!(v1_cost(&line, &FingeringWeights::v1()), 17);
    assert_eq!(v1_cost(&[], &FingeringWeights::v1()), 0);
}

/// The production DP's path is `v1_cost`-optimal: the mirrored objective is
/// the one `infer_positions` minimizes.
#[test]
fn production_dp_path_is_v1_optimal_on_exhaustive_small_lines() {
    let tuning = Tuning::standard_e();
    for weights in v1_weight_sets() {
        for len in 1..=3 {
            for raw in sequences(&V1_PITCHES, len) {
                let pitches = pitches_of(&raw);
                let dp: Vec<FretboardPosition> =
                    infer_positions(&pitches, &tuning, &weights, STANDARD_MAX_FRET)
                        .into_iter()
                        .map(Option::unwrap)
                        .collect();
                let brute = candidate_lines(&pitches, &tuning, STANDARD_MAX_FRET)
                    .iter()
                    .map(|l| v1_cost(l, &weights))
                    .min()
                    .unwrap();
                assert_eq!(v1_cost(&dp, &weights), brute, "{raw:?} {weights:?}");
            }
        }
    }
}

#[test]
fn production_dp_path_is_v1_optimal_on_longer_lines() {
    let tuning = Tuning::standard_e();
    for weights in v1_weight_sets() {
        for pitches in lcg_lines(24, 6, 40, 76) {
            let dp: Vec<FretboardPosition> =
                infer_positions(&pitches, &tuning, &weights, STANDARD_MAX_FRET)
                    .into_iter()
                    .map(Option::unwrap)
                    .collect();
            let brute = candidate_lines(&pitches, &tuning, STANDARD_MAX_FRET)
                .iter()
                .map(|l| v1_cost(l, &weights))
                .min()
                .unwrap();
            assert_eq!(v1_cost(&dp, &weights), brute);
        }
    }
}

#[test]
fn v1_problem_scores_every_candidate_line_like_v1_cost() {
    let tuning = Tuning::standard_e();
    for weights in v1_weight_sets() {
        for raw in sequences(&V1_PITCHES, 3) {
            let pitches = pitches_of(&raw);
            let problem = v1_problem(&pitches, &tuning, &weights, STANDARD_MAX_FRET).unwrap();
            assert_eq!(problem.vars().len(), V1_VARS_PER_NOTE * pitches.len());
            for line in candidate_lines(&pitches, &tuning, STANDARD_MAX_FRET) {
                assert_eq!(
                    problem.evaluate(&encode_v1_witness(&line)),
                    Ok(v1_cost(&line, &weights))
                );
            }
        }
    }
}

#[test]
fn v1_problem_admits_only_candidate_positions() {
    let tuning = Tuning::standard_e();
    let pitches = pitches_of(&[40, 45]);
    let problem = v1_problem(
        &pitches,
        &tuning,
        &FingeringWeights::v1(),
        STANDARD_MAX_FRET,
    )
    .unwrap();
    let names: Vec<&str> = problem.vars().iter().map(|v| v.name.as_str()).collect();
    assert_eq!(names, ["s0", "f0", "s1", "f1"]);
    // E2 has one candidate (6, 0); A2 has (6, 5) and (5, 0).
    assert_eq!(problem.vars()[0].domain, vec![6]);
    assert_eq!(problem.vars()[3].domain, vec![0, 5]);
    // (5, 5) is in both domains but does not sound A2.
    assert_eq!(
        problem.evaluate(&[6, 0, 5, 5]),
        Err(WitnessError::HardViolated { index: 1 })
    );
}

#[test]
fn v1_problem_omits_zero_weight_terms() {
    let tuning = Tuning::standard_e();
    let pitches = pitches_of(&[40, 45, 47]);
    let weights = FingeringWeights {
        fret: 1,
        open_string: 0,
        position_shift: 0,
        string_change: 3,
    };
    let problem = v1_problem(&pitches, &tuning, &weights, STANDARD_MAX_FRET).unwrap();
    let abs = problem
        .objective()
        .iter()
        .filter(|t| matches!(t, Term::AbsDiff { .. }))
        .count();
    let neq = problem
        .objective()
        .iter()
        .filter(|t| matches!(t, Term::NotEqual { .. }))
        .count();
    assert_eq!((abs, neq), (0, 2));
    for term in problem.objective() {
        if let Term::Unary { costs, .. } = term {
            assert!(costs.iter().all(|&(_, c)| c != 0));
        }
    }
}

#[test]
fn v1_problem_refuses_empty_and_unpositionable_lines() {
    let tuning = Tuning::standard_e();
    let w = FingeringWeights::v1();
    assert_eq!(
        v1_problem(&[], &tuning, &w, STANDARD_MAX_FRET),
        Err(LabError::EmptyLine)
    );
    assert_eq!(
        v1_problem(&pitches_of(&[40, 30]), &tuning, &w, STANDARD_MAX_FRET),
        Err(LabError::UnpositionablePitch {
            index: 1,
            pitch: 30
        })
    );
}

#[test]
fn witness_encoding_round_trips() {
    let line = vec![pos(6, 0), pos(5, 12), pos(1, 24)];
    assert_eq!(encode_v1_witness(&line), vec![6, 0, 5, 12, 1, 24]);
    assert_eq!(
        decode_positions(&encode_v1_witness(&line), V1_VARS_PER_NOTE),
        Some(line.clone())
    );
    let hands = vec![1, 12, 21];
    let hw = encode_hand_witness(&line, &hands).unwrap();
    assert_eq!(hw, vec![6, 0, 1, 5, 12, 12, 1, 24, 21]);
    assert_eq!(
        decode_positions(&hw, HAND_VARS_PER_NOTE),
        Some(line.clone())
    );
    assert_eq!(encode_hand_witness(&line, &hands[..2]), None);
    assert_eq!(decode_positions(&[6, 0, 5], V1_VARS_PER_NOTE), None);
    assert_eq!(decode_positions(&[6, -1], V1_VARS_PER_NOTE), None);
    assert_eq!(decode_positions(&[256, 0], V1_VARS_PER_NOTE), None);
    assert_eq!(decode_positions(&[6, 0], 0), None);
}

// ── hand model ────────────────────────────────────────────────────────────────

fn hand_weight_sets() -> Vec<HandWeights> {
    vec![
        HandWeights {
            height: 1,
            open_string: 2,
            stretch: 3,
            shift: 5,
            shift_distance: 1,
            string_distance: 2,
        },
        HandWeights {
            height: -1,
            open_string: -4,
            stretch: 0,
            shift: 0,
            shift_distance: 2,
            string_distance: 0,
        },
        HandWeights {
            height: 0,
            open_string: 0,
            stretch: 0,
            shift: 0,
            shift_distance: 0,
            string_distance: 0,
        },
        HandWeights {
            height: 0,
            open_string: 0,
            stretch: 7,
            shift: 3,
            shift_distance: 0,
            string_distance: 1,
        },
    ]
}

fn model(weights: HandWeights, max_fret: u8) -> HandModel {
    HandModel::new(weights, max_fret).expect("valid model")
}

#[test]
fn hand_model_refuses_negative_transition_and_stretch_weights() {
    let base = hand_weight_sets()[0];
    for (name, weights) in [
        (
            "stretch",
            HandWeights {
                stretch: -1,
                ..base
            },
        ),
        ("shift", HandWeights { shift: -1, ..base }),
        (
            "shift_distance",
            HandWeights {
                shift_distance: -1,
                ..base
            },
        ),
        (
            "string_distance",
            HandWeights {
                string_distance: -1,
                ..base
            },
        ),
    ] {
        assert_eq!(
            HandModel::new(weights, 24),
            Err(HandModelError::NegativeWeight { name, value: -1 })
        );
    }
    assert!(HandModel::new(hand_weight_sets()[1], 24).is_ok());
    assert_eq!(
        HandModel::new(base, 3),
        Err(HandModelError::NoRoom { max_fret: 3 })
    );
    assert_eq!(model(base, 4).hands(), 1..=1);
    assert_eq!(model(base, 24).hands(), 1..=21);
}

#[test]
fn reach_follows_the_four_fret_box() {
    let m = model(hand_weight_sets()[0], 24);
    assert_eq!(m.reach(0, 9), Some(Reach::Open));
    assert_eq!(m.reach(5, 5), Some(Reach::InBox));
    assert_eq!(m.reach(8, 5), Some(Reach::InBox));
    assert_eq!(m.reach(9, 5), Some(Reach::Stretch));
    assert_eq!(m.reach(4, 5), Some(Reach::Stretch));
    assert_eq!(m.reach(3, 5), None);
    assert_eq!(m.reach(10, 5), None);
    assert_eq!(m.reach(1, 2), Some(Reach::Stretch));
    assert_eq!(m.reach(24, 21), Some(Reach::InBox));
    assert_eq!(m.reach(0, 22), None);
    assert_eq!(m.reach(0, 0), None);
}

#[test]
fn hand_cost_scores_a_hand_computed_realization() {
    let m = model(hand_weight_sets()[0], 24);
    let line = [pos(6, 3), pos(6, 7), pos(5, 0), pos(4, 12)];
    // unary: 2 + 3 + (3 + 2) + 9 = 19; steps: 6 + 2 + 13 = 21.
    assert_eq!(hand_cost(&line, &[3, 4, 4, 10], &m), Ok(40));
    // A stretch: fret 7 from hand 3 costs stretch 3 on top of height 2.
    assert_eq!(hand_cost(&[pos(6, 7)], &[3], &m), Ok(5));
    assert_eq!(
        hand_cost(&line, &[3, 1, 4, 10], &m),
        Err(HandError::Unreachable { index: 1 })
    );
    assert_eq!(
        hand_cost(&line, &[3, 4], &m),
        Err(HandError::Length {
            positions: 4,
            hands: 2
        })
    );
}

fn brute_best_hands(line: &[FretboardPosition], m: &HandModel) -> Option<i64> {
    let hands: Vec<u8> = m.hands().collect();
    sequences(&hands, line.len())
        .iter()
        .filter_map(|hs| hand_cost(line, hs, m).ok())
        .min()
}

#[test]
fn best_hands_is_optimal_for_fixed_positions() {
    let tuning = Tuning::standard_e();
    for weights in hand_weight_sets() {
        let m = model(weights, 7);
        for raw in sequences(&[40, 45, 47, 50, 52, 57, 59, 64], 3) {
            for line in candidate_lines(&pitches_of(&raw), &tuning, 7) {
                let got = best_hands(&line, &m);
                assert_eq!(got.as_ref().map(|g| g.0), brute_best_hands(&line, &m));
                if let Some((cost, hands)) = got {
                    assert_eq!(hand_cost(&line, &hands, &m), Ok(cost));
                }
            }
        }
    }
}

#[test]
fn solve_hand_is_optimal_on_exhaustive_small_lines() {
    let tuning = Tuning::standard_e();
    for weights in hand_weight_sets() {
        let m = model(weights, 7);
        for len in 1..=3 {
            for raw in sequences(&[40, 45, 47, 50, 52, 57, 59, 64], len) {
                let pitches = pitches_of(&raw);
                let brute = candidate_lines(&pitches, &tuning, 7)
                    .iter()
                    .filter_map(|l| brute_best_hands(l, &m))
                    .min();
                let sol = solve_hand(&pitches, &tuning, &m);
                assert_eq!(sol.as_ref().map(|s| s.cost), brute, "{raw:?} {weights:?}");
                let sol = sol.unwrap();
                for (p, q) in sol.positions.iter().zip(&pitches) {
                    assert_eq!(tuning.pitch_at(*p), Some(*q));
                }
                assert_eq!(hand_cost(&sol.positions, &sol.hands, &m), Ok(sol.cost));
            }
        }
    }
}

#[test]
fn solve_hand_is_optimal_on_longer_full_neck_lines() {
    let tuning = Tuning::standard_e();
    for weights in hand_weight_sets() {
        let m = model(weights, STANDARD_MAX_FRET);
        for pitches in lcg_lines(12, 5, 40, 80) {
            let brute = candidate_lines(&pitches, &tuning, STANDARD_MAX_FRET)
                .iter()
                .filter_map(|l| best_hands(l, &m).map(|b| b.0))
                .min();
            let sol = solve_hand(&pitches, &tuning, &m).unwrap();
            assert_eq!(Some(sol.cost), brute);
            assert_eq!(hand_cost(&sol.positions, &sol.hands, &m), Ok(sol.cost));
            assert_eq!(solve_hand(&pitches, &tuning, &m), Some(sol));
        }
    }
}

#[test]
fn solve_hand_edges() {
    let m = model(hand_weight_sets()[0], STANDARD_MAX_FRET);
    let tuning = Tuning::standard_e();
    let empty = solve_hand(&[], &tuning, &m).unwrap();
    assert_eq!((empty.cost, empty.positions.len()), (0, 0));
    assert_eq!(solve_hand(&pitches_of(&[40, 30]), &tuning, &m), None);
}

#[test]
fn hand_problem_scores_like_hand_cost_and_shares_the_optimum() {
    let tuning = Tuning::standard_e();
    for weights in hand_weight_sets() {
        let m = model(weights, 7);
        let hands: Vec<u8> = m.hands().collect();
        for raw in sequences(&[40, 47, 52, 57, 64], 2) {
            let pitches = pitches_of(&raw);
            let problem = hand_problem(&pitches, &tuning, &m).unwrap();
            assert_eq!(problem.vars().len(), HAND_VARS_PER_NOTE * pitches.len());
            assert_eq!(problem.vars()[2].name, "h0");
            assert_eq!(problem.vars()[2].domain, vec![1, 2, 3, 4]);
            let mut best: Option<i64> = None;
            for line in candidate_lines(&pitches, &tuning, 7) {
                for hs in sequences(&hands, line.len()) {
                    let witness = encode_hand_witness(&line, &hs).unwrap();
                    match hand_cost(&line, &hs, &m) {
                        Ok(cost) => {
                            assert_eq!(problem.evaluate(&witness), Ok(cost));
                            best = Some(best.map_or(cost, |b: i64| b.min(cost)));
                        }
                        Err(_) => assert!(matches!(
                            problem.evaluate(&witness),
                            Err(WitnessError::HardViolated { .. })
                        )),
                    }
                }
            }
            assert_eq!(best, solve_hand(&pitches, &tuning, &m).map(|s| s.cost));
        }
    }
}

#[test]
fn hand_problem_omits_zero_weight_terms_and_refuses_bad_lines() {
    let tuning = Tuning::standard_e();
    let zeros = model(hand_weight_sets()[2], STANDARD_MAX_FRET);
    let problem = hand_problem(&pitches_of(&[40, 45, 47]), &tuning, &zeros).unwrap();
    assert!(problem.objective().is_empty());
    assert_eq!(problem.hard().len(), 6);
    assert_eq!(hand_problem(&[], &tuning, &zeros), Err(LabError::EmptyLine));
    assert_eq!(
        hand_problem(&pitches_of(&[30]), &tuning, &zeros),
        Err(LabError::UnpositionablePitch {
            index: 0,
            pitch: 30
        })
    );
}

// ── holdout ───────────────────────────────────────────────────────────────────

#[test]
fn song_key_folds_arrangements_of_one_song() {
    assert_eq!(
        song_key("A Lot Like Birds - Connector (ver 2 by LPFzCS_LMS).gp5"),
        "a lot like birds - connector"
    );
    assert_eq!(
        song_key("A Lot Like Birds - Connector.gp5"),
        "a lot like birds - connector"
    );
    assert_eq!(song_key("Band - Song (Live) (ver 3).gpx"), "band - song");
    assert_eq!(song_key("Band - Title.gp"), "band - title");
    assert_eq!(song_key("No Extension"), "no extension");
}

#[test]
fn holdout_bucket_is_stable_and_bounded() {
    let k = song_key("A Lot Like Birds - Connector.gp5");
    assert_eq!(holdout_bucket(&k, 5), holdout_bucket(&k, 5));
    assert!(holdout_bucket(&k, 5) < 5);
    assert_eq!(holdout_bucket(&k, 0), 0);
    assert_eq!(holdout_bucket(&k, 1), 0);
    let spread: std::collections::BTreeSet<u64> = (0..200)
        .map(|i| holdout_bucket(&format!("band - song {i}"), 5))
        .collect();
    assert_eq!(spread.len(), 5);
}
