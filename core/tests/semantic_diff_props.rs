//! Property-based invariants over `exact_semantic_diff` (S16 Phase 4-pre B1).
//!
//! A compact valid-by-construction `Score` strategy drives three laws:
//!
//! - **reflexivity** — `diff(s, s)` is empty;
//! - **determinism** — two runs of the same diff compare equal and render
//!   byte-identically;
//! - **directional inversion** — for a value-level perturbation that keeps
//!   the structural coordinates, `diff(a, b)` and `diff(b, a)` carry the
//!   same paths with kinds inverted and `expected`/`actual` swapped.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing
)]

use griff_core::{
    event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity},
    score::{
        AtomEvent, AtomNote, AtomRest, EventGroup, EventGroupKind, ImportWarning, LossReport,
        MasterBar, RepeatMarker, Score, Track, Voice,
    },
    semantic_diff::{exact_semantic_diff, SemanticDifferenceKind},
    slice::TickRange,
};
use proptest::prelude::*;

const PPQN: u16 = 480;
const BAR: u32 = 1920;

/// One generated atom: `Some((pitch, velocity))` = note, `None` = rest.
type AtomSpec = Option<(u8, u8)>;

fn atom(onset: u32, spec: AtomSpec) -> AtomEvent {
    match spec {
        Some((pitch, velocity)) => AtomEvent::Note(AtomNote {
            absolute_start: Ticks(onset),
            duration: Ticks(240),
            pitch: Pitch::new(pitch.min(127)).expect("clamped pitch"),
            velocity: Velocity::new(velocity.min(127)).expect("clamped velocity"),
            marks: NoteMarks::empty(),
            position: None,
        }),
        None => AtomEvent::Rest(AtomRest {
            absolute_start: Ticks(onset),
            duration: Ticks(240),
        }),
    }
}

fn build_score(bpms: &[u32], tracks: &[Vec<AtomSpec>], warnings: usize) -> Score {
    let master_bars = bpms
        .iter()
        .enumerate()
        .map(|(index, &bpm)| {
            let start = u32::try_from(index).expect("few bars") * BAR;
            MasterBar {
                index,
                tick_range: TickRange::new(Ticks(start), Ticks(start + BAR)).expect("ordered"),
                time_signature: TimeSignature::new(4, 4).expect("4/4"),
                tempo: Tempo::from_bpm_integer(bpm.max(1)).expect("positive BPM"),
                repeat: RepeatMarker::default(),
            }
        })
        .collect();
    let tracks = tracks
        .iter()
        .map(|atoms| Track {
            name: None,
            channel: 0,
            voices: vec![Voice {
                id: 0,
                event_groups: vec![EventGroup {
                    kind: EventGroupKind::Single,
                    atoms: atoms
                        .iter()
                        .enumerate()
                        .map(|(i, spec)| atom(u32::try_from(i).expect("few atoms") * 240, *spec))
                        .collect(),
                    technique_spans: Vec::new(),
                }],
            }],
            tuning: Tuning::standard_e(),
        })
        .collect();
    Score {
        ticks_per_quarter: PPQN,
        master_bars,
        tracks,
        source_meta: None,
        loss: LossReport {
            warnings: (0..warnings)
                .map(|i| ImportWarning::Other(format!("w{i}")))
                .collect(),
        },
    }
}

prop_compose! {
    fn arb_score()(
        bpms in prop::collection::vec(40_u32..300, 1..=3),
        tracks in prop::collection::vec(
            prop::collection::vec(prop::option::of((30_u8..90, 20_u8..120)), 1..=4),
            1..=3,
        ),
        warnings in 0_usize..3,
    ) -> Score {
        build_score(&bpms, &tracks, warnings)
    }
}

/// A value-level perturbation that keeps every structural coordinate: the
/// first bar's tempo moves to a value outside the generator's range.
fn perturb(score: &Score) -> Score {
    let mut out = score.clone();
    out.master_bars[0].tempo = Tempo::from_bpm_integer(1000).expect("1000 BPM");
    out
}

fn inverted(kind: &SemanticDifferenceKind) -> SemanticDifferenceKind {
    match kind {
        SemanticDifferenceKind::ValueMismatch => SemanticDifferenceKind::ValueMismatch,
        SemanticDifferenceKind::VariantMismatch => SemanticDifferenceKind::VariantMismatch,
        SemanticDifferenceKind::CardinalityMismatch { expected, actual } => {
            SemanticDifferenceKind::CardinalityMismatch {
                expected: *actual,
                actual: *expected,
            }
        }
        SemanticDifferenceKind::MissingExpected => SemanticDifferenceKind::MissingActual,
        SemanticDifferenceKind::MissingActual => SemanticDifferenceKind::MissingExpected,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn diff_is_reflexive(score in arb_score()) {
        let report = exact_semantic_diff(&score, &score.clone());
        prop_assert!(report.is_empty(), "diff(s, s) must be empty");
    }

    #[test]
    fn diff_is_deterministic(a in arb_score(), b in arb_score()) {
        let first = exact_semantic_diff(&a, &b);
        let second = exact_semantic_diff(&a, &b);
        prop_assert_eq!(&first, &second);
        prop_assert_eq!(format!("{first:?}"), format!("{second:?}"));
    }

    #[test]
    fn diff_inverts_directionally(a in arb_score()) {
        let b = perturb(&a);
        let forward = exact_semantic_diff(&a, &b);
        let backward = exact_semantic_diff(&b, &a);
        prop_assert_eq!(forward.differences.len(), backward.differences.len());
        for (f, r) in forward.differences.iter().zip(backward.differences.iter()) {
            prop_assert_eq!(f.path.to_string(), r.path.to_string());
            prop_assert_eq!(&inverted(&f.kind), &r.kind);
            prop_assert_eq!(&f.expected, &r.actual);
            prop_assert_eq!(&f.actual, &r.expected);
        }
    }
}
