use griff_constraint_lab::chord_event::{
    analyze_regimes, rotate_anchors_within_song, ChordEventAtom, ChordEventIdentity,
    ChordEventProblem, HandAnchor, IncomingTechnique, ObservedAtomPosition,
    ObservedChordVoicing, TechniqueKind,
};
use griff_core::event::{FretboardPosition, Pitch, Tuning};

fn standard() -> Tuning {
    Tuning::standard_e()
}

fn identity(onset: u32) -> ChordEventIdentity {
    ChordEventIdentity {
        source: "song.gp5".into(),
        song_key: "song".into(),
        track: 0,
        voice: 0,
        onset,
    }
}

fn atom(note_id: usize, pitch: u8) -> ChordEventAtom {
    ChordEventAtom {
        note_id,
        pitch: Pitch(pitch),
        duration: 120,
        tapped: false,
    }
}

#[test]
fn problem_input_is_separate_from_observed_target_positions() {
    let problem = ChordEventProblem::new(
        identity(100),
        standard(),
        24,
        vec![atom(4, 64), atom(9, 67)],
        None,
        vec![],
    )
    .unwrap();
    let observed = ObservedChordVoicing::new(vec![
        ObservedAtomPosition::new(4, FretboardPosition { string: 1, fret: 0 }),
        ObservedAtomPosition::new(9, FretboardPosition { string: 2, fret: 3 }),
    ])
    .unwrap();
    assert_eq!(problem.atoms().len(), 2);
    assert_eq!(observed.positions().len(), 2);
}

#[test]
fn r2_constrains_stable_target_identity_and_reduces_exact_count() {
    let technique = IncomingTechnique {
        kind: TechniqueKind::Legato,
        origin_note_id: 1,
        origin_onset: 80,
        origin_pitch: Pitch(62),
        origin_position: FretboardPosition { string: 2, fret: 0 },
        target_atom_id: 9,
    };
    let problem = ChordEventProblem::new(
        identity(100),
        standard(),
        24,
        vec![atom(4, 64), atom(9, 67)],
        None,
        vec![technique],
    )
    .unwrap();
    let result = analyze_regimes(&problem, None).unwrap();
    assert!(result.r2.admissible_count.value < result.r0.admissible_count.value);
    assert!(result.r2.assignments.iter().all(|assignment| {
        assignment
            .position(9)
            .is_some_and(|position| position.string == 2)
    }));
}

#[test]
fn conflicting_incoming_relations_are_typed_infeasible() {
    let incoming = [1_u8, 2]
        .into_iter()
        .map(|string| IncomingTechnique {
            kind: TechniqueKind::Legato,
            origin_note_id: usize::from(string),
            origin_onset: 80,
            origin_pitch: Pitch(62),
            origin_position: FretboardPosition { string, fret: 0 },
            target_atom_id: 9,
        })
        .collect();
    let problem = ChordEventProblem::new(
        identity(100),
        standard(),
        24,
        vec![atom(4, 64), atom(9, 67)],
        None,
        incoming,
    )
    .unwrap();
    let result = analyze_regimes(&problem, None).unwrap();
    assert!(result.r2.conflict.is_some());
    assert!(result.r2.assignments.is_empty());
}

#[test]
fn r1_and_r3_minimize_anchor_distance_with_open_zero_semantics() {
    let problem = ChordEventProblem::new(
        identity(100),
        standard(),
        24,
        vec![atom(4, 64), atom(9, 67)],
        Some(HandAnchor {
            fret: 5,
            onset: 80,
            source_note_id: Some(1),
        }),
        vec![],
    )
    .unwrap();
    let result = analyze_regimes(&problem, None).unwrap();
    assert_eq!(result.r1.as_ref().unwrap().optimum, 2);
    assert_eq!(result.r3.as_ref().unwrap().optimum, 2);
}

#[test]
fn duplicate_pitch_agreement_uses_atom_identity() {
    let problem = ChordEventProblem::new(
        identity(100),
        standard(),
        24,
        vec![atom(4, 64), atom(9, 64)],
        None,
        vec![],
    )
    .unwrap();
    let observed = ObservedChordVoicing::new(vec![
        ObservedAtomPosition::new(4, FretboardPosition { string: 1, fret: 0 }),
        ObservedAtomPosition::new(9, FretboardPosition { string: 2, fret: 5 }),
    ])
    .unwrap();
    let result = analyze_regimes(&problem, Some(&observed)).unwrap();
    let metrics = result.r0.human.as_ref().unwrap();
    assert!(metrics.ceiling > metrics.floor);
}

#[test]
fn rotated_anchor_control_is_deterministic_and_never_self_assigns() {
    let rows = vec![
        (identity(100), HandAnchor { fret: 3, onset: 80, source_note_id: Some(1) }),
        (identity(200), HandAnchor { fret: 7, onset: 180, source_note_id: Some(2) }),
        (identity(300), HandAnchor { fret: 9, onset: 280, source_note_id: Some(3) }),
    ];
    let rotated = rotate_anchors_within_song(&rows);
    assert_eq!(rotated.len(), 3);
    for ((event, anchor), replacement) in rows.iter().zip(&rotated) {
        assert_eq!(&replacement.0, event);
        assert_ne!(replacement.1, *anchor);
    }
    assert_eq!(rotated, rotate_anchors_within_song(&rows));
}
