//! Red → contract tests for the Constraint Lab spike.
//!
//! Pins the public lab API: the solver-neutral IR, deterministic `MiniZinc`
//! emission, the exact reference solver, and the two Constraint Inventory
//! problems (bounded-travel realization; complement pair cleanliness pinned to
//! the production `PairValidation` laws). Written red-first; the green
//! implementation and every review-driven contract change re-enter through
//! this suite.

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
    ir::{Constraint, IntVar, IrError, OracleProblem, VarId},
    manifest::{OutcomeRecord, RunManifest, SolverIdentity, Status, SCHEMA, SCHEMA_VERSION},
    problems::{bounded_travel_problem, pair_cleanliness_problem, LabError, PairSpec},
    solve::{solve_exact, verify_witness, Outcome},
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

    // Every field the manifests key evidence off must shift the fingerprint:
    // domain, problem name, constraint bound, and constraint operands.
    let base = build().fingerprint();
    let variant = |name: &str, domain: Vec<i64>, b: usize, bound: i64| {
        OracleProblem::new(
            name,
            vec![IntVar::new("f0", domain), IntVar::new("f1", vec![1])],
            vec![Constraint::AbsDiffLe {
                a: VarId(0),
                b: VarId(b),
                bound,
            }],
        )
        .fingerprint()
    };
    let reference = variant("p", vec![0, 5], 0, 3);
    assert_ne!(base, variant("p", vec![0, 7], 0, 3), "domain must matter");
    assert_ne!(
        reference,
        variant("q", vec![0, 5], 0, 3),
        "problem name must matter"
    );
    assert_ne!(
        reference,
        variant("p", vec![0, 5], 0, 4),
        "constraint bound must matter"
    );
    assert_ne!(
        reference,
        variant("p", vec![0, 5], 1, 3),
        "constraint operands must matter"
    );
}

#[test]
fn ir_invariants_are_validated_at_construction() {
    // Dangling VarId.
    let dangling = OracleProblem::try_new(
        "bad",
        vec![IntVar::new("x", vec![0])],
        vec![Constraint::AbsDiffLe {
            a: VarId(0),
            b: VarId(7),
            bound: 1,
        }],
    );
    assert!(matches!(dangling, Err(IrError::DanglingVarId { .. })));

    // Empty domain.
    let empty = OracleProblem::try_new("bad", vec![IntVar::new("x", vec![])], vec![]);
    assert!(matches!(empty, Err(IrError::EmptyDomain { .. })));

    // Duplicate names.
    let dup = OracleProblem::try_new(
        "bad",
        vec![IntVar::new("x", vec![0]), IntVar::new("x", vec![1])],
        vec![],
    );
    assert!(matches!(dup, Err(IrError::DuplicateName { .. })));

    // MiniZinc-unsafe name.
    let unsafe_name = OracleProblem::try_new("bad", vec![IntVar::new("2x y", vec![0])], vec![]);
    assert!(matches!(unsafe_name, Err(IrError::UnsafeName { .. })));

    // Non-positive denominator.
    let bad_den = OracleProblem::try_new(
        "bad",
        vec![IntVar::new("x", vec![0])],
        vec![Constraint::BandOverlapAtMost {
            vars: vec![VarId(0)],
            fixed_lo: 0,
            fixed_hi: 4,
            num: 1,
            den: 0,
        }],
    );
    assert!(matches!(
        bad_den,
        Err(IrError::NonPositiveDenominator { .. })
    ));
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
            assert!(p.vars()[0].domain.contains(&witness[0]));
            assert!(p.vars()[1].domain.contains(&witness[1]));
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

#[test]
fn band_overlap_degenerate_variable_band_takes_the_emptiness_branch() {
    // The *variable* band is the single-pitch one (b_lo == b_hi) inside a
    // wide fixed band: production band_overlap takes narrower = min(spans)
    // = 0, so overlap is 1.0-iff-inside — the emptiness branch, stricter
    // than the proportional rule, and symmetric across which band is thin.
    let inside = OracleProblem::new(
        "thin-b-inside",
        vec![IntVar::new("b0", vec![50])],
        vec![Constraint::BandOverlapAtMost {
            vars: vec![VarId(0)],
            fixed_lo: 45,
            fixed_hi: 57,
            num: 1,
            den: 2,
        }],
    );
    assert!(
        matches!(solve_exact(&inside), Outcome::Unsat { .. }),
        "single-pitch band inside [45,57] → overlap 1.0 → mud"
    );

    let outside = OracleProblem::new(
        "thin-b-outside",
        vec![IntVar::new("b0", vec![60])],
        vec![Constraint::BandOverlapAtMost {
            vars: vec![VarId(0)],
            fixed_lo: 45,
            fixed_hi: 57,
            num: 1,
            den: 2,
        }],
    );
    assert!(
        matches!(solve_exact(&outside), Outcome::Sat { .. }),
        "single-pitch band outside [45,57] → overlap 0.0 → clean"
    );
}

#[test]
fn verify_witness_checks_a_full_assignment_against_every_constraint() {
    let p = OracleProblem::new(
        "verify",
        vec![IntVar::new("x", vec![0, 5]), IntVar::new("y", vec![4, 12])],
        vec![Constraint::AbsDiffLe {
            a: VarId(0),
            b: VarId(1),
            bound: 4,
        }],
    );
    assert!(verify_witness(&p, &[0, 4]), "|0-4| <= 4 holds");
    assert!(!verify_witness(&p, &[0, 12]), "|0-12| > 4 violates");
    assert!(
        !verify_witness(&p, &[0]),
        "partial assignment is not a witness"
    );
    assert!(
        !verify_witness(&p, &[1, 4]),
        "a witness value outside the domain is invalid"
    );
}

// ── Problem A — bounded-travel fretboard realization ──────────────────────────

#[test]
fn fret_domains_come_from_tuning_candidates() {
    // Standard E: pitch 40 (low E) is playable only as string 6 open (fret 0);
    // pitch 45 (A2) as string 5 open or string 6 fret 5.
    let p = bounded_travel_problem(&[pitch(40), pitch(45)], &std_e(), STANDARD_MAX_FRET, 12)
        .expect("both pitches are playable");
    assert_eq!(p.vars().len(), 2);
    assert_eq!(p.vars()[0].domain, vec![0]);
    assert_eq!(p.vars()[1].domain, vec![0, 5]);
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
    let score = two_track_score(&[(0, 52)], &[]);
    let spec = PairSpec::from_score_part_a(
        &score,
        0,
        vec![0],
        vec![pitch(53), pitch(58), pitch(63)], // classes 1, 6, 11
        std_e(),
    )
    .expect("spec builds");
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
    let a_score = two_track_score(&[(0, 52), (QUARTER, 55)], &[]);
    let b_onsets = vec![0, QUARTER];
    let spec = PairSpec::from_score_part_a(
        &a_score,
        0,
        b_onsets.clone(),
        vec![pitch(64), pitch(67), pitch(71)],
        std_e(),
    )
    .expect("spec builds");
    let p = pair_cleanliness_problem(&spec).expect("problem builds");
    let witness = match solve_exact(&p) {
        Outcome::Sat { witness, .. } => witness,
        Outcome::Unsat { .. } => panic!("a clean assignment exists"),
    };

    let b_notes: Vec<(u32, u8)> = b_onsets
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
    let score = two_track_score(&[(0, 45), (QUARTER, 57)], &[]);
    let spec = PairSpec::from_score_part_a(
        &score,
        0,
        vec![2 * QUARTER, 3 * QUARTER],
        vec![pitch(50), pitch(52)],
        std_e(),
    )
    .expect("spec builds");
    let p = pair_cleanliness_problem(&spec).expect("problem builds");
    assert!(matches!(solve_exact(&p), Outcome::Unsat { .. }));

    // Production agrees on *every* assignment in the domain — the UNSAT
    // claim is exhaustive, so its cross-check must be too (2 onsets × {50, 52}).
    for b0 in [50u8, 52] {
        for b1 in [50u8, 52] {
            let score = two_track_score(
                &[(0, 45), (QUARTER, 57)],
                &[(2 * QUARTER, b0), (3 * QUARTER, b1)],
            );
            let v = validate_pair(&score, 0, 1).expect("validate ok");
            assert!(
                !v.is_clean(),
                "register mud must flag B = ({b0}, {b1}): {v:?}"
            );
        }
    }
}

#[test]
fn pair_spec_from_score_is_a_full_note_snapshot_of_the_canonical_model() {
    // Track A holds a chord at onset 0 (45 under 52). validate_pair counts
    // dissonances and builds the register band over *all* notes, so the
    // snapshot must carry the full onset-sorted note list — only playability
    // folds to the top note, and it does so inside the problem builder.
    let score = two_track_score(&[(0, 45), (0, 52), (QUARTER, 55)], &[]);
    let from_score = PairSpec::from_score_part_a(
        &score,
        0,
        vec![0, QUARTER],
        vec![pitch(64), pitch(67), pitch(71)],
        std_e(),
    )
    .expect("extraction succeeds");
    assert_eq!(
        from_score.a_line(),
        &[(0, pitch(45)), (0, pitch(52)), (QUARTER, pitch(55))]
    );

    // Same score, same inputs → same problem: the constructor is
    // deterministic and the canonical model is the only entry path.
    let again = PairSpec::from_score_part_a(
        &score,
        0,
        vec![0, QUARTER],
        vec![pitch(64), pitch(67), pitch(71)],
        std_e(),
    )
    .expect("extraction succeeds");
    let a = pair_cleanliness_problem(&from_score).expect("builds");
    let b = pair_cleanliness_problem(&again).expect("builds");
    assert_eq!(a.fingerprint(), b.fingerprint());

    let missing = PairSpec::from_score_part_a(&score, 7, vec![0], vec![pitch(64)], std_e());
    assert!(matches!(missing, Err(LabError::NoSuchTrack { .. })));
}

#[test]
fn duplicate_b_onsets_are_a_typed_refusal() {
    // Problem B promises one pitch variable per onset; a duplicated onset
    // would silently turn B into a chord, and the per-note domain filter is
    // no longer production's top-note folding there.
    let score = two_track_score(&[(0, 52)], &[]);
    let dup = PairSpec::from_score_part_a(
        &score,
        0,
        vec![0, QUARTER, QUARTER],
        vec![pitch(64)],
        std_e(),
    );
    assert!(matches!(
        dup,
        Err(LabError::DuplicateBOnset { onset }) if onset == QUARTER
    ));
}

#[test]
fn b_playability_is_filtered_under_the_b_tuning_not_a_s() {
    // MIDI 40 is playable in A's Standard E (open low string) but not under
    // a five-string tuning whose lowest open pitch is 45: production filters
    // B under B's own tuning, so the oracle must too.
    let high_b_tuning = Tuning::new(vec![
        pitch(64), // E4
        pitch(59), // B3
        pitch(55), // G3
        pitch(50), // D3
        pitch(45), // A2 — lowest: MIDI 40 is unreachable
    ]);
    let score = two_track_score(&[(0, 52)], &[]);
    let err = PairSpec::from_score_part_a(
        &score,
        0,
        vec![2 * QUARTER],
        vec![pitch(40)],
        high_b_tuning,
    )
    .and_then(|spec| pair_cleanliness_problem(&spec))
    .expect_err("40 is unplayable under the B tuning");
    assert!(matches!(err, LabError::EmptyPlayableDomain { .. }));

    // The same domain under Standard E for B is fine — proving the filter
    // reads the B tuning, not A's.
    let ok = PairSpec::from_score_part_a(&score, 0, vec![2 * QUARTER], vec![pitch(40)], std_e())
        .and_then(|spec| pair_cleanliness_problem(&spec));
    assert!(ok.is_ok(), "40 is playable under Standard E: {ok:?}");
}

#[test]
fn chordal_a_playability_folds_to_the_top_note_like_production() {
    // Production part_playability folds each onset to its highest pitch
    // before the fingering DP: MIDI 30 under a playable 64 must not refuse
    // the problem, because validate_pair never measures the 30.
    let score = two_track_score(&[(0, 30), (0, 64)], &[]);
    let spec = PairSpec::from_score_part_a(&score, 0, vec![2 * QUARTER], vec![pitch(76)], std_e())
        .expect("spec builds");
    let p = pair_cleanliness_problem(&spec)
        .expect("the folded line (64) is playable, so the problem must build");
    assert!(matches!(solve_exact(&p), Outcome::Sat { .. }));

    // And production agrees end-to-end on the reconstructed pair.
    let score = two_track_score(&[(0, 30), (0, 64)], &[(2 * QUARTER, 76)]);
    let v = validate_pair(&score, 0, 1).expect("validate ok");
    assert!(
        v.is_clean(),
        "production folds the chord for playability and calls this clean: {v:?}"
    );
}

#[test]
fn unplayable_a_line_is_a_typed_precondition_failure() {
    let score = two_track_score(&[(0, 30)], &[]); // below Standard E
    let spec = PairSpec::from_score_part_a(&score, 0, vec![0], vec![pitch(64)], std_e())
        .expect("spec builds");
    let err = pair_cleanliness_problem(&spec).expect_err("A must be playable first");
    assert!(matches!(err, LabError::PartAUnplayable { .. }));
}

#[test]
fn unplayable_b_domain_pitches_are_filtered_out_not_ignored() {
    // 100 is beyond fret 24 on string 1 (64 + 24 = 88): not playable.
    let score = two_track_score(&[(0, 52)], &[]);
    let spec = PairSpec::from_score_part_a(&score, 0, vec![0], vec![pitch(100)], std_e())
        .expect("spec builds");
    let err = pair_cleanliness_problem(&spec).expect_err("empty playable domain");
    assert!(matches!(err, LabError::EmptyPlayableDomain { .. }));
}

// ── manifests ─────────────────────────────────────────────────────────────────

#[test]
fn run_manifest_serializes_with_schema_identity_and_round_trips() {
    let m = RunManifest {
        schema: SCHEMA.to_string(),
        version: SCHEMA_VERSION,
        problem: "tiny".to_string(),
        fingerprint_hex: "00ff".to_string(),
        solver: SolverIdentity {
            name: "griff-lab-exact".to_string(),
            version: "1".to_string(),
        },
        outcome: OutcomeRecord {
            status: Status::Sat,
            witness: Some(vec![("f0".to_string(), 0)]),
            nodes: 3,
        },
    };
    let json = serde_json::to_string_pretty(&m).expect("serializes");
    assert!(json.contains("\"schema\": \"griff.constraint-lab-run\""));
    assert!(
        json.contains("\"status\": \"sat\""),
        "the typed status keeps the lowercase wire form: {json}"
    );
    let back: RunManifest = serde_json::from_str(&json).expect("round-trips");
    assert_eq!(back, m);
    assert_eq!(SCHEMA, "griff.constraint-lab-run");
    assert_eq!(SCHEMA_VERSION, 1);
}
