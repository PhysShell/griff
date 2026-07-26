//! Red → contract tests for the Constraint Lab spike.
//!
//! Pins the public lab API: the solver-neutral IR, deterministic `MiniZinc`
//! emission, the exact reference solver, and the two Constraint Inventory
//! problems (bounded-travel realization; complement pair cleanliness pinned to
//! the production `PairValidation` laws). References `griff_constraint_lab`
//! modules that do not exist yet, so the suite fails until the green step.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::str_to_string
)]

use griff_constraint_lab::{
    emit::to_minizinc,
    ir::{Constraint, IntVar, OracleProblem, VarId},
    manifest::{OutcomeRecord, RunManifest, SolverIdentity},
    problems::{bounded_travel_problem, pair_cleanliness_problem, LabError, PairSpec},
    solve::{solve_exact, Outcome},
};
use griff_core::{
    complement::validate_pair,
    event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity},
    fretboard::{measure_playability, FingeringWeights, STANDARD_MAX_FRET},
    score::{
        AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, RepeatMarker,
        Score, Track, Voice,
    },
    slice::TickRange,
};

const PPQN: u16 = 480;
const QUARTER: u32 = 480;
const BAR: u32 = 1920; // 4/4 at 480 PPQN

// ── score-building helpers (the complement test convention) ───────────────────

fn quarter_note(start: u32, pitch: u8) -> AtomEvent {
    AtomEvent::Note(AtomNote {
        absolute_start: Ticks(start),
        duration: Ticks(QUARTER),
        pitch: Pitch::new(pitch).expect("valid pitch"),
        velocity: Velocity::new(90).expect("valid velocity"),
        marks: NoteMarks::empty(),
        position: None,
    })
}

fn single_group(atom: AtomEvent) -> EventGroup {
    EventGroup {
        kind: EventGroupKind::Single,
        atoms: vec![atom],
        technique_spans: Vec::new(),
    }
}

fn track_from(name: &str, notes: &[(u32, u8)]) -> Track {
    Track {
        name: Some(name.to_string()),
        channel: 0,
        voices: vec![Voice {
            id: 0,
            event_groups: notes
                .iter()
                .map(|&(onset, pitch)| single_group(quarter_note(onset, pitch)))
                .collect(),
        }],
        tuning: Tuning::standard_e(),
    }
}

/// A one-bar 4/4 score with two tracks (part A and part B) built from
/// `(onset, pitch)` lists — the shape `validate_pair` consumes.
fn two_track_score(a: &[(u32, u8)], b: &[(u32, u8)]) -> Score {
    Score {
        ticks_per_quarter: PPQN,
        master_bars: vec![MasterBar {
            index: 0,
            tick_range: TickRange::new(Ticks(0), Ticks(BAR)).expect("ordered"),
            time_signature: TimeSignature {
                numerator: 4,
                denominator: 4,
            },
            tempo: Tempo::from_bpm_integer(120).expect("120 BPM"),
            repeat: RepeatMarker::default(),
        }],
        tracks: vec![track_from("A", a), track_from("B", b)],
        source_meta: None,
        loss: LossReport::new(),
    }
}

fn std_e() -> Tuning {
    Tuning::standard_e()
}

fn pitch(p: u8) -> Pitch {
    Pitch::new(p).expect("valid pitch")
}

// ── IR ────────────────────────────────────────────────────────────────────────

#[test]
fn int_var_domains_are_sorted_and_deduped() {
    let v = IntVar::new("f0", vec![5, 0, 5, 3]);
    assert_eq!(v.domain, vec![0, 3, 5]);
}

#[test]
fn fingerprint_is_stable_and_content_sensitive() {
    let build = || {
        OracleProblem::new(
            "p",
            vec![IntVar::new("f0", vec![0, 5])],
            vec![Constraint::AbsDiffLe {
                a: VarId(0),
                b: VarId(0),
                bound: 3,
            }],
        )
    };
    assert_eq!(build().fingerprint(), build().fingerprint());

    let other = OracleProblem::new(
        "p",
        vec![IntVar::new("f0", vec![0, 7])],
        vec![Constraint::AbsDiffLe {
            a: VarId(0),
            b: VarId(0),
            bound: 3,
        }],
    );
    assert_ne!(build().fingerprint(), other.fingerprint());
}

// ── MiniZinc emission ─────────────────────────────────────────────────────────

#[test]
fn minizinc_emission_is_deterministic_and_names_the_constraints() {
    let p = OracleProblem::new(
        "tiny",
        vec![IntVar::new("f0", vec![0]), IntVar::new("f1", vec![12, 17])],
        vec![Constraint::AbsDiffLe {
            a: VarId(0),
            b: VarId(1),
            bound: 11,
        }],
    );
    let a = to_minizinc(&p);
    let b = to_minizinc(&p);
    assert_eq!(a, b, "emission must be deterministic");
    assert!(a.contains("var {0}: f0;"), "domain set for f0: {a}");
    assert!(a.contains("var {12,17}: f1;"), "domain set for f1: {a}");
    assert!(
        a.contains("constraint abs(f0 - f1) <= 11;"),
        "travel constraint: {a}"
    );
    assert!(a.contains("solve satisfy;"), "satisfaction goal: {a}");
}

// ── exact reference solver ────────────────────────────────────────────────────

#[test]
fn solver_finds_a_witness_that_satisfies_every_constraint() {
    let p = OracleProblem::new(
        "sat",
        vec![
            IntVar::new("x", vec![0, 5, 9]),
            IntVar::new("y", vec![4, 12]),
        ],
        vec![Constraint::AbsDiffLe {
            a: VarId(0),
            b: VarId(1),
            bound: 4,
        }],
    );
    match solve_exact(&p) {
        Outcome::Sat { witness, .. } => {
            assert_eq!(witness.len(), 2);
            assert!((witness[0] - witness[1]).abs() <= 4, "witness {witness:?}");
            assert!(p.vars[0].domain.contains(&witness[0]));
            assert!(p.vars[1].domain.contains(&witness[1]));
        }
        Outcome::Unsat { .. } => panic!("problem is satisfiable"),
    }
}

#[test]
fn solver_proves_unsat_when_domains_cannot_meet_the_bound() {
    let p = OracleProblem::new(
        "unsat",
        vec![IntVar::new("x", vec![0]), IntVar::new("y", vec![12, 17])],
        vec![Constraint::AbsDiffLe {
            a: VarId(0),
            b: VarId(1),
            bound: 11,
        }],
    );
    assert!(matches!(solve_exact(&p), Outcome::Unsat { .. }));
}

#[test]
fn forbidden_interval_classes_mirror_the_dissonance_law() {
    // Fixed pitch 52; domain of exactly the three dissonant classes → UNSAT.
    let dissonant = OracleProblem::new(
        "dissonant-only",
        vec![IntVar::new("b0", vec![53, 58, 63])], // classes 1, 6, 11 vs 52
        vec![Constraint::ForbiddenIntervalClasses {
            var: VarId(0),
            fixed: 52,
            classes: vec![1, 6, 11],
        }],
    );
    assert!(matches!(solve_exact(&dissonant), Outcome::Unsat { .. }));

    // Adding one consonant option (a fifth, class 7) makes it SAT on that value.
    let with_fifth = OracleProblem::new(
        "one-consonance",
        vec![IntVar::new("b0", vec![53, 58, 59, 63])],
        vec![Constraint::ForbiddenIntervalClasses {
            var: VarId(0),
            fixed: 52,
            classes: vec![1, 6, 11],
        }],
    );
    match solve_exact(&with_fifth) {
        Outcome::Sat { witness, .. } => assert_eq!(witness, vec![59]),
        Outcome::Unsat { .. } => panic!("59 satisfies the law"),
    }
}

#[test]
fn band_overlap_constraint_mirrors_the_degenerate_single_pitch_rule() {
    // Fixed band is the single pitch 52 (narrower span = 0): the production
    // law says mud iff the point lies inside the variable band, so the
    // constraint (overlap <= 1/2) demands an empty intersection.
    let containing = OracleProblem::new(
        "degenerate-contained",
        vec![IntVar::new("b0", vec![50]), IntVar::new("b1", vec![55])],
        vec![Constraint::BandOverlapAtMost {
            vars: vec![VarId(0), VarId(1)],
            fixed_lo: 52,
            fixed_hi: 52,
            num: 1,
            den: 2,
        }],
    );
    assert!(
        matches!(solve_exact(&containing), Outcome::Unsat { .. }),
        "band [50,55] contains the point 52 → mud → UNSAT"
    );

    let above = OracleProblem::new(
        "degenerate-clear",
        vec![IntVar::new("b0", vec![55]), IntVar::new("b1", vec![57])],
        vec![Constraint::BandOverlapAtMost {
            vars: vec![VarId(0), VarId(1)],
            fixed_lo: 52,
            fixed_hi: 52,
            num: 1,
            den: 2,
        }],
    );
    assert!(
        matches!(solve_exact(&above), Outcome::Sat { .. }),
        "band [55,57] excludes the point 52 → clean"
    );
}

// ── Problem A — bounded-travel fretboard realization ──────────────────────────

#[test]
fn fret_domains_come_from_tuning_candidates() {
    // Standard E: pitch 40 (low E) is playable only as string 6 open (fret 0);
    // pitch 45 (A2) as string 5 open or string 6 fret 5.
    let p = bounded_travel_problem(&[pitch(40), pitch(45)], &std_e(), STANDARD_MAX_FRET, 12)
        .expect("both pitches are playable");
    assert_eq!(p.vars.len(), 2);
    assert_eq!(p.vars[0].domain, vec![0]);
    assert_eq!(p.vars[1].domain, vec![0, 5]);
}

#[test]
fn unpositionable_pitch_is_a_typed_refusal() {
    // Pitch 30 is below every open string in Standard E.
    let err = bounded_travel_problem(&[pitch(30)], &std_e(), STANDARD_MAX_FRET, 12)
        .expect_err("nothing can play MIDI 30 in Standard E");
    assert!(matches!(
        err,
        LabError::UnpositionablePitch { index: 0, .. }
    ));
}

#[test]
fn playable_line_is_sat_at_the_standard_fret_range() {
    let line = [pitch(40), pitch(43), pitch(45), pitch(47), pitch(50)];
    let report = measure_playability(&line, &std_e(), &FingeringWeights::v1(), STANDARD_MAX_FRET);
    assert!(report.is_playable(), "fixture line must be playable");

    let p = bounded_travel_problem(&line, &std_e(), STANDARD_MAX_FRET, 24)
        .expect("playable line builds a problem");
    assert!(
        matches!(solve_exact(&p), Outcome::Sat { .. }),
        "a playable line is SAT under the loosest travel bound"
    );
}

#[test]
fn travel_bound_sweep_finds_the_exact_frontier() {
    // Pitch 40 has the single fret 0; pitch 76 has frets {12, 17, 21}.
    // Minimal possible travel is therefore 12: bound 11 UNSAT, bound 12 SAT.
    let line = [pitch(40), pitch(76)];
    let tight = bounded_travel_problem(&line, &std_e(), STANDARD_MAX_FRET, 11).expect("builds");
    assert!(matches!(solve_exact(&tight), Outcome::Unsat { .. }));

    let exact = bounded_travel_problem(&line, &std_e(), STANDARD_MAX_FRET, 12).expect("builds");
    match solve_exact(&exact) {
        Outcome::Sat { witness, .. } => assert_eq!(witness, vec![0, 12]),
        Outcome::Unsat { .. } => panic!("bound 12 admits string-1 fret-12"),
    }
}

// ── Problem B — complement pair cleanliness (production laws) ─────────────────

#[test]
fn dissonant_only_domain_is_unsat_and_agrees_with_validate_pair() {
    let spec = PairSpec {
        a_line: vec![(0, pitch(52))],
        b_onsets: vec![0],
        b_domain: vec![pitch(53), pitch(58), pitch(63)], // classes 1, 6, 11
        tuning: std_e(),
        max_fret: STANDARD_MAX_FRET,
    };
    let p = pair_cleanliness_problem(&spec).expect("problem builds");
    assert!(matches!(solve_exact(&p), Outcome::Unsat { .. }));

    // Cross-check against production: every domain choice really is unclean.
    for b in [53u8, 58, 63] {
        let score = two_track_score(&[(0, 52)], &[(0, b)]);
        let v = validate_pair(&score, 0, 1).expect("validate ok");
        assert!(!v.is_clean(), "pitch {b} must be flagged by validate_pair");
    }
}

#[test]
fn sat_witness_reconstructs_a_clean_pair_under_validate_pair() {
    // A occupies [52, 55]; the B domain sits an octave-ish above, is consonant
    // against both coincident A notes, and is playable in Standard E.
    let spec = PairSpec {
        a_line: vec![(0, pitch(52)), (QUARTER, pitch(55))],
        b_onsets: vec![0, QUARTER],
        b_domain: vec![pitch(64), pitch(67), pitch(71)],
        tuning: std_e(),
        max_fret: STANDARD_MAX_FRET,
    };
    let p = pair_cleanliness_problem(&spec).expect("problem builds");
    let witness = match solve_exact(&p) {
        Outcome::Sat { witness, .. } => witness,
        Outcome::Unsat { .. } => panic!("a clean assignment exists"),
    };

    let b_notes: Vec<(u32, u8)> = spec
        .b_onsets
        .iter()
        .zip(&witness)
        .map(|(&onset, &p)| (onset, u8::try_from(p).expect("midi pitch")))
        .collect();
    let score = two_track_score(&[(0, 52), (QUARTER, 55)], &b_notes);
    let v = validate_pair(&score, 0, 1).expect("validate ok");
    assert!(
        v.is_clean(),
        "the oracle witness must satisfy the production validator: {v:?}"
    );
}

#[test]
fn register_mud_alone_makes_the_pair_problem_unsat() {
    // No coincident onsets (so no dissonance constraints), but the only
    // available B band sits inside A's: overlap 1.0 > 0.5 → mud → UNSAT.
    let spec = PairSpec {
        a_line: vec![(0, pitch(45)), (QUARTER, pitch(57))],
        b_onsets: vec![2 * QUARTER, 3 * QUARTER],
        b_domain: vec![pitch(50), pitch(52)],
        tuning: std_e(),
        max_fret: STANDARD_MAX_FRET,
    };
    let p = pair_cleanliness_problem(&spec).expect("problem builds");
    assert!(matches!(solve_exact(&p), Outcome::Unsat { .. }));

    // Production agrees on a representative assignment.
    let score = two_track_score(
        &[(0, 45), (QUARTER, 57)],
        &[(2 * QUARTER, 50), (3 * QUARTER, 52)],
    );
    let v = validate_pair(&score, 0, 1).expect("validate ok");
    assert!(!v.is_clean(), "register mud must flag the pair: {v:?}");
}

#[test]
fn unplayable_a_line_is_a_typed_precondition_failure() {
    let spec = PairSpec {
        a_line: vec![(0, pitch(30))], // below Standard E
        b_onsets: vec![0],
        b_domain: vec![pitch(64)],
        tuning: std_e(),
        max_fret: STANDARD_MAX_FRET,
    };
    let err = pair_cleanliness_problem(&spec).expect_err("A must be playable first");
    assert!(matches!(err, LabError::PartAUnplayable { .. }));
}

#[test]
fn unplayable_b_domain_pitches_are_filtered_out_not_ignored() {
    // 100 is beyond fret 24 on string 1 (64 + 24 = 88): not playable.
    let spec = PairSpec {
        a_line: vec![(0, pitch(52))],
        b_onsets: vec![0],
        b_domain: vec![pitch(100)],
        tuning: std_e(),
        max_fret: STANDARD_MAX_FRET,
    };
    let err = pair_cleanliness_problem(&spec).expect_err("empty playable domain");
    assert!(matches!(err, LabError::EmptyPlayableDomain { .. }));
}

// ── manifests ─────────────────────────────────────────────────────────────────

#[test]
fn run_manifest_serializes_with_schema_identity_and_round_trips() {
    let m = RunManifest {
        schema: "griff.constraint-lab-run".to_string(),
        version: 1,
        problem: "tiny".to_string(),
        fingerprint_hex: "00ff".to_string(),
        solver: SolverIdentity {
            name: "griff-lab-exact".to_string(),
            version: "1".to_string(),
        },
        outcome: OutcomeRecord {
            status: "sat".to_string(),
            witness: Some(vec![("f0".to_string(), 0)]),
            nodes: 3,
        },
    };
    let json = serde_json::to_string_pretty(&m).expect("serializes");
    assert!(json.contains("\"schema\": \"griff.constraint-lab-run\""));
    let back: RunManifest = serde_json::from_str(&json).expect("round-trips");
    assert_eq!(back, m);
}
