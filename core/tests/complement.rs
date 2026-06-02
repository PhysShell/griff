//! Red → characterization tests for the ComplementArranger first slice (S13).
//!
//! Pins the public `complement` API: analysing part A into a `PartProfile`,
//! arranging a `rhythm_lock` complement (delegating to the S6 generator), and a
//! minimal pair validator. References `griff_core::complement`, which does not
//! exist yet, so the suite fails to compile until the green step. ADR-0012.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing,
    clippy::missing_const_for_fn,
    clippy::float_cmp,
    clippy::arithmetic_side_effects,
    clippy::str_to_string,
    clippy::doc_markdown
)]

use griff_core::{
    complement::{
        analyze_part, arrange_complement, validate_pair, ComplementError, ComplementSpec,
        RelationMode,
    },
    event::{Pitch, Tempo, Ticks, TimeSignature, Velocity},
    generate::GenerationSeed,
    score::{
        AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, Score, Track, Voice,
    },
    slice::TickRange,
};

const PPQN: u16 = 480;
const QUARTER: u32 = 480;
const BAR: u32 = 1920; // 4/4 at 480 PPQN

fn quarter_note(start: u32, pitch: u8) -> AtomEvent {
    AtomEvent::Note(AtomNote {
        absolute_start: Ticks(start),
        duration: Ticks(QUARTER),
        pitch: Pitch::new(pitch).expect("valid pitch"),
        velocity: Velocity::new(90).expect("valid velocity"),
        articulation: None,
    })
}

fn single_group(atom: AtomEvent) -> EventGroup {
    EventGroup {
        kind: EventGroupKind::Single,
        atoms: vec![atom],
        technique_spans: Vec::new(),
    }
}

/// A `bar_count`-bar 4/4 score whose single track plays `pitches` (one quarter
/// note each) contiguously per bar, repeated every bar.
fn score_with_part_a(bar_count: usize, pitches: &[u8]) -> Score {
    let master_bars = (0..bar_count)
        .map(|i| {
            let start = u32::try_from(i).unwrap() * BAR;
            MasterBar {
                index: i,
                tick_range: TickRange::new(Ticks(start), Ticks(start + BAR)).expect("ordered"),
                time_signature: TimeSignature {
                    numerator: 4,
                    denominator: 4,
                },
                tempo: Tempo::new(120.0).expect("120 BPM"),
            }
        })
        .collect();

    let mut groups = Vec::new();
    for bar in 0..bar_count {
        let bar_start = u32::try_from(bar).unwrap() * BAR;
        for (i, &p) in pitches.iter().enumerate() {
            let onset = bar_start + u32::try_from(i).unwrap() * QUARTER;
            groups.push(single_group(quarter_note(onset, p)));
        }
    }

    Score {
        ticks_per_quarter: PPQN,
        master_bars,
        tracks: vec![Track {
            name: Some("A".to_string()),
            channel: 0,
            voices: vec![Voice {
                id: 0,
                event_groups: groups,
            }],
        }],
        source_meta: None,
        loss: LossReport::new(),
    }
}

/// Absolute onsets of every note atom in a track's primary voice.
fn note_onsets(score: &Score, track_index: usize) -> Vec<u32> {
    score.tracks[track_index].voices[0]
        .event_groups
        .iter()
        .flat_map(|g| &g.atoms)
        .filter_map(|a| match a {
            AtomEvent::Note(n) => Some(n.absolute_start.0),
            AtomEvent::Rest(_) => None,
        })
        .collect()
}

fn note_pitches(score: &Score, track_index: usize) -> Vec<u8> {
    score.tracks[track_index].voices[0]
        .event_groups
        .iter()
        .flat_map(|g| &g.atoms)
        .filter_map(|a| match a {
            AtomEvent::Note(n) => Some(n.pitch.0),
            AtomEvent::Rest(_) => None,
        })
        .collect()
}

// ── analyze_part ────────────────────────────────────────────────────────────────

#[test]
fn analyze_part_extracts_rhythm_grid_and_density() {
    // 2 bars × 4 quarter notes.
    let score = score_with_part_a(2, &[60, 62, 64, 65]);
    let profile = analyze_part(&score, 0).expect("analysis must succeed");

    assert_eq!(
        profile.bar_rhythms.len(),
        2,
        "one rhythm row per master bar"
    );
    assert_eq!(
        profile.bar_rhythms[0],
        vec![
            Ticks(QUARTER),
            Ticks(QUARTER),
            Ticks(QUARTER),
            Ticks(QUARTER)
        ],
    );
    assert_eq!(profile.note_count, 8);
    assert_eq!(profile.density, 4.0, "8 notes over 2 bars = 4 notes/bar");

    let reg = profile.register.expect("part A has notes");
    assert_eq!(reg.lowest.0, 60);
    assert_eq!(reg.highest.0, 65);
}

#[test]
fn analyze_part_rejects_track_index_out_of_range() {
    let score = score_with_part_a(1, &[60]);
    assert!(matches!(
        analyze_part(&score, 5),
        Err(ComplementError::TrackIndexOutOfRange)
    ));
}

// ── arrange_complement: rhythm_lock ───────────────────────────────────────────────

#[test]
fn rhythm_lock_shares_master_bars_and_ppqn() {
    let score = score_with_part_a(3, &[60, 62, 64, 65]);
    let spec = ComplementSpec {
        mode: RelationMode::RhythmLock,
        register_offset: -12,
    };
    let cand = arrange_complement(&score, 0, spec, GenerationSeed(7)).expect("arrange ok");

    assert_eq!(
        cand.score.ticks_per_quarter, score.ticks_per_quarter,
        "B must share A's PPQN"
    );
    assert_eq!(
        cand.score.master_bars.len(),
        score.master_bars.len(),
        "B must share A's master bars"
    );
    // The original part A track is preserved and B is appended.
    assert_eq!(cand.score.tracks.len(), 2, "A plus appended B");
    assert_eq!(cand.part_b_index, 1);
    assert_eq!(
        note_onsets(&cand.score, 0),
        note_onsets(&score, 0),
        "A untouched"
    );
}

#[test]
fn rhythm_lock_b_onsets_match_part_a_grid() {
    // Contiguous uniform rhythm: the S6 rhythm-copy reproduces A's onset grid.
    let score = score_with_part_a(2, &[60, 62, 64, 65]);
    let spec = ComplementSpec {
        mode: RelationMode::RhythmLock,
        register_offset: -12,
    };
    let cand = arrange_complement(&score, 0, spec, GenerationSeed(1)).expect("arrange ok");

    assert_eq!(
        note_onsets(&cand.score, cand.part_b_index),
        note_onsets(&score, 0),
        "rhythm_lock: B's onsets must match A's onset grid",
    );
}

#[test]
fn rhythm_lock_b_pitches_within_offset_register() {
    let score = score_with_part_a(2, &[60, 62, 64, 65]);
    let spec = ComplementSpec {
        mode: RelationMode::RhythmLock,
        register_offset: -12,
    };
    let cand = arrange_complement(&score, 0, spec, GenerationSeed(3)).expect("arrange ok");

    // A spans [60,65]; an octave-down offset puts B's band at [48,53].
    for p in note_pitches(&cand.score, cand.part_b_index) {
        assert!(
            (48..=53).contains(&p),
            "B pitch {p} must lie in the offset register [48,53]",
        );
    }
}

#[test]
fn rhythm_lock_axis_scores_report_locked_rhythm() {
    let score = score_with_part_a(2, &[60, 62, 64, 65]);
    let spec = ComplementSpec {
        mode: RelationMode::RhythmLock,
        register_offset: -12,
    };
    let cand = arrange_complement(&score, 0, spec, GenerationSeed(9)).expect("arrange ok");
    assert_eq!(
        cand.axis_scores.rhythm_similarity, 1.0,
        "rhythm_lock must report identical rhythm",
    );
}

#[test]
fn arrange_is_deterministic_for_fixed_seed() {
    let score = score_with_part_a(3, &[60, 62, 64, 65]);
    let spec = ComplementSpec {
        mode: RelationMode::RhythmLock,
        register_offset: -12,
    };
    let a = arrange_complement(&score, 0, spec, GenerationSeed(42)).expect("arrange ok");
    let b = arrange_complement(&score, 0, spec, GenerationSeed(42)).expect("arrange ok");
    assert_eq!(
        note_pitches(&a.score, a.part_b_index),
        note_pitches(&b.score, b.part_b_index),
        "fixed seed + fixed A must produce identical B pitches",
    );
    assert_eq!(
        note_onsets(&a.score, a.part_b_index),
        note_onsets(&b.score, b.part_b_index),
    );
}

#[test]
fn arrange_unimplemented_mode_is_reported() {
    let score = score_with_part_a(1, &[60, 62, 64, 65]);
    let spec = ComplementSpec {
        mode: RelationMode::CounterMelody,
        register_offset: 0,
    };
    assert!(matches!(
        arrange_complement(&score, 0, spec, GenerationSeed(1)),
        Err(ComplementError::ModeNotImplemented(
            RelationMode::CounterMelody
        )),
    ));
}

#[test]
fn arrange_rejects_part_with_no_notes() {
    let mut score = score_with_part_a(1, &[60]);
    score.tracks[0].voices[0].event_groups.clear();
    let spec = ComplementSpec {
        mode: RelationMode::RhythmLock,
        register_offset: -12,
    };
    assert!(matches!(
        arrange_complement(&score, 0, spec, GenerationSeed(1)),
        Err(ComplementError::PartHasNoNotes)
    ));
}

fn note_durations(score: &Score, track_index: usize) -> Vec<u32> {
    score.tracks[track_index].voices[0]
        .event_groups
        .iter()
        .flat_map(|g| &g.atoms)
        .filter_map(|a| match a {
            AtomEvent::Note(n) => Some(n.duration.0),
            AtomEvent::Rest(_) => None,
        })
        .collect()
}

/// Regression for Codex P1+P2: an irregular A — sparse/off-beat rhythm in bar 0
/// and a 4/4 → 3/4 meter change — must still be reproduced exactly by B,
/// onsets *and* durations, since rhythm_lock copies A's real grid.
#[test]
fn rhythm_lock_preserves_irregular_grid_and_meter_change() {
    // bar 0: 4/4 [0,1920) — notes at 0 and 960 (a gap between, sparse).
    // bar 1: 3/4 [1920,3360) — notes at 1920 and 2880.
    let three_four = 1440_u32; // 3/4 at 480 PPQN
    let master_bars = vec![
        MasterBar {
            index: 0,
            tick_range: TickRange::new(Ticks(0), Ticks(BAR)).expect("ordered"),
            time_signature: TimeSignature {
                numerator: 4,
                denominator: 4,
            },
            tempo: Tempo::new(120.0).expect("120 BPM"),
        },
        MasterBar {
            index: 1,
            tick_range: TickRange::new(Ticks(BAR), Ticks(BAR + three_four)).expect("ordered"),
            time_signature: TimeSignature {
                numerator: 3,
                denominator: 4,
            },
            tempo: Tempo::new(120.0).expect("120 BPM"),
        },
    ];
    let onsets = [0_u32, 960, 1920, 2880];
    let groups: Vec<EventGroup> = onsets
        .iter()
        .map(|&o| single_group(quarter_note(o, 64)))
        .collect();
    let score = Score {
        ticks_per_quarter: PPQN,
        master_bars,
        tracks: vec![Track {
            name: Some("A".to_string()),
            channel: 0,
            voices: vec![Voice {
                id: 0,
                event_groups: groups,
            }],
        }],
        source_meta: None,
        loss: LossReport::new(),
    };

    let spec = ComplementSpec {
        mode: RelationMode::RhythmLock,
        register_offset: -12,
    };
    let cand = arrange_complement(&score, 0, spec, GenerationSeed(5)).expect("arrange ok");

    assert_eq!(
        note_onsets(&cand.score, cand.part_b_index),
        onsets.to_vec(),
        "B must reproduce A's exact onsets across the meter change",
    );
    assert_eq!(
        note_durations(&cand.score, cand.part_b_index),
        note_durations(&score, 0),
        "B must reproduce A's exact note durations",
    );
    assert_eq!(
        cand.axis_scores.rhythm_similarity, 1.0,
        "rhythm is genuinely locked",
    );
}

// ── validate_pair ──────────────────────────────────────────────────────────────

#[test]
fn validate_pair_flags_coincident_dissonance() {
    // Two tracks: identical onsets, B a minor second above A on every onset.
    let mut score = score_with_part_a(1, &[60, 62, 64, 65]);
    let b_groups: Vec<EventGroup> = [61, 63, 65, 66]
        .iter()
        .enumerate()
        .map(|(i, &p)| single_group(quarter_note(u32::try_from(i).unwrap() * QUARTER, p)))
        .collect();
    score.tracks.push(Track {
        name: Some("B".to_string()),
        channel: 1,
        voices: vec![Voice {
            id: 0,
            event_groups: b_groups,
        }],
    });

    let v = validate_pair(&score, 0, 1).expect("validate ok");
    assert!(
        v.coincident_dissonances > 0,
        "minor seconds on coincident onsets must be flagged",
    );
    assert!(!v.is_clean());
}

#[test]
fn validate_pair_passes_consonant_octave_apart() {
    // B an octave below A on the same onsets: consonant, no register mud.
    let mut score = score_with_part_a(1, &[60, 64, 67, 72]);
    let b_groups: Vec<EventGroup> = [48, 52, 55, 60]
        .iter()
        .enumerate()
        .map(|(i, &p)| single_group(quarter_note(u32::try_from(i).unwrap() * QUARTER, p)))
        .collect();
    score.tracks.push(Track {
        name: Some("B".to_string()),
        channel: 1,
        voices: vec![Voice {
            id: 0,
            event_groups: b_groups,
        }],
    });

    let v = validate_pair(&score, 0, 1).expect("validate ok");
    assert_eq!(v.coincident_dissonances, 0, "octaves/consonances are clean");
    assert!(
        !v.register_mud,
        "an octave of separation is not register mud"
    );
    assert!(v.is_clean());
}
