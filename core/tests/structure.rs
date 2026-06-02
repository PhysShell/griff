//! Red → green tests for the S14 Phase-0 structure-metrics pass (ADR-0015).
//!
//! Pins the public `structure` API: `measure_structure` derives
//! `StructureMetrics` (detected pattern period, repeatability, variation,
//! loopability, structural complexity) from a score's track. Pure, deterministic,
//! and dependent on neither the graph layer nor DP/Viterbi. References
//! `griff_core::structure`, which does not exist yet, so the suite fails to
//! compile until the green step.

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
    event::{Pitch, Tempo, Ticks, TimeSignature, Velocity},
    score::{
        AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, Score, Track, Voice,
    },
    slice::TickRange,
    structure::{measure_structure, StructureError},
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

/// A 4/4 score; bar `i` plays `bars[i]` as contiguous quarter notes from its
/// downbeat. A bar with fewer than 4 pitches leaves a trailing gap.
fn build_score(bars: &[Vec<u8>]) -> Score {
    let master_bars = (0..bars.len())
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
    for (b, pitches) in bars.iter().enumerate() {
        let bar_start = u32::try_from(b).unwrap() * BAR;
        for (i, &p) in pitches.iter().enumerate() {
            let onset = bar_start + u32::try_from(i).unwrap() * QUARTER;
            groups.push(EventGroup {
                kind: EventGroupKind::Single,
                atoms: vec![quarter_note(onset, p)],
                technique_spans: Vec::new(),
            });
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

// ── period detection ─────────────────────────────────────────────────────────────

#[test]
fn detects_one_bar_period_in_a_tiled_loop() {
    // A A A A — every bar identical.
    let a = vec![60, 62, 64, 65];
    let score = build_score(&[a.clone(), a.clone(), a.clone(), a]);
    let m = measure_structure(&score, 0).expect("measure ok");

    assert_eq!(m.detected_pattern_period_bars, Some(1), "1-bar tile");
    assert_eq!(m.detected_pattern_period_ticks, Some(BAR));
    assert!(
        m.repeatability_score > 0.9,
        "identical bars repeat strongly"
    );
    assert!(m.variation_score < 0.1, "almost no variation");
    assert_eq!(m.structural_complexity, 0.25, "1 distinct bar of 4");
}

#[test]
fn detects_two_bar_period_in_an_abab_pattern() {
    let a = vec![60, 62, 64, 65];
    let b = vec![67, 69, 71, 72];
    let score = build_score(&[a.clone(), b.clone(), a, b]);
    let m = measure_structure(&score, 0).expect("measure ok");

    assert_eq!(
        m.detected_pattern_period_bars,
        Some(2),
        "A B A B → period 2"
    );
    assert_eq!(m.detected_pattern_period_ticks, Some(2 * BAR));
    assert!(
        m.repeatability_score > 0.9,
        "the 2-bar unit repeats exactly"
    );
    // Repeatability and structural complexity are *separate* axes: a strongly
    // repeating 2-bar pattern still has two distinct bars of material.
    assert_eq!(m.structural_complexity, 0.5, "2 distinct bars of 4");
}

#[test]
fn reports_no_period_for_through_composed_material() {
    let score = build_score(&[
        vec![60, 61, 62, 63],
        vec![64, 65, 66, 67],
        vec![68, 69, 70, 71],
        vec![72, 73, 74, 75],
    ]);
    let m = measure_structure(&score, 0).expect("measure ok");

    assert_eq!(m.detected_pattern_period_bars, None, "no repeating idea");
    assert_eq!(m.detected_pattern_period_ticks, None);
    assert!(m.repeatability_score < 0.3, "little to no repetition");
    assert_eq!(m.structural_complexity, 1.0, "every bar distinct");
}

#[test]
fn detects_period_across_whole_bar_rests() {
    // "A rest rest" tiled twice — the rest bars are part of the repeated idea.
    let a = vec![60, 62, 64, 65];
    let rest: Vec<u8> = vec![];
    let score = build_score(&[a.clone(), rest.clone(), rest.clone(), a, rest.clone(), rest]);
    let m = measure_structure(&score, 0).expect("measure ok");

    assert_eq!(
        m.detected_pattern_period_bars,
        Some(3),
        "matching whole-bar rests must count toward the period",
    );
    assert_eq!(m.detected_pattern_period_ticks, Some(3 * BAR));
    assert!(
        m.repeatability_score > 0.9,
        "the 3-bar unit repeats exactly"
    );
}

#[test]
fn measures_all_voices_of_a_track() {
    // Voice 0 carries bars 0 & 2; voice 1 carries bars 1 & 3; every bar is the
    // same material A. Measured as a whole track this is a clean 1-bar loop —
    // a first-voice-only view would instead see "A rest A rest" (period 2).
    let a = [60, 62, 64, 65];
    let voice_groups = |bars: &[usize]| -> Vec<EventGroup> {
        let mut groups = Vec::new();
        for &b in bars {
            let bar_start = u32::try_from(b).unwrap() * BAR;
            for (i, &p) in a.iter().enumerate() {
                groups.push(EventGroup {
                    kind: EventGroupKind::Single,
                    atoms: vec![quarter_note(
                        bar_start + u32::try_from(i).unwrap() * QUARTER,
                        p,
                    )],
                    technique_spans: Vec::new(),
                });
            }
        }
        groups
    };

    let master_bars = (0..4)
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

    let score = Score {
        ticks_per_quarter: PPQN,
        master_bars,
        tracks: vec![Track {
            name: Some("multi".to_string()),
            channel: 0,
            voices: vec![
                Voice {
                    id: 0,
                    event_groups: voice_groups(&[0, 2]),
                },
                Voice {
                    id: 1,
                    event_groups: voice_groups(&[1, 3]),
                },
            ],
        }],
        source_meta: None,
        loss: LossReport::new(),
    };

    let m = measure_structure(&score, 0).expect("measure ok");
    assert_eq!(
        m.detected_pattern_period_bars,
        Some(1),
        "all voices merged → every bar is A → 1-bar period",
    );
    assert!(m.repeatability_score > 0.9);
}

// ── loopability ────────────────────────────────────────────────────────────────

#[test]
fn loopability_high_when_seam_is_smooth() {
    // Two full bars, last note returns to the first pitch; material fills to the end.
    let bar = vec![60, 62, 64, 60];
    let score = build_score(&[bar.clone(), bar]);
    let m = measure_structure(&score, 0).expect("measure ok");
    assert!(
        m.loopability_score > 0.7,
        "unison seam + full fill should loop cleanly, got {}",
        m.loopability_score,
    );
}

#[test]
fn loopability_low_when_seam_jumps_and_leaves_a_gap() {
    // Bar 2 is a lone high note far from the start, leaving a 3-beat trailing gap.
    let score = build_score(&[vec![60, 62, 64, 65], vec![84]]);
    let m = measure_structure(&score, 0).expect("measure ok");
    assert!(
        m.loopability_score < 0.4,
        "octave+ seam jump and a big gap should not loop, got {}",
        m.loopability_score,
    );
}

#[test]
fn loopability_penalizes_leading_silence() {
    // Last note reaches the span end (no trailing gap) and the seam is a unison,
    // but the first note starts a 3-beat rest late — the wrapped loop waits
    // through that leading silence before the riff sounds again, so it is not
    // seamless. (Without counting leading silence this would score 1.0.)
    let master_bars = (0..2)
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

    let single = |start: u32, p: u8| EventGroup {
        kind: EventGroupKind::Single,
        atoms: vec![quarter_note(start, p)],
        technique_spans: Vec::new(),
    };
    // bar 0: one note at beat 4 (onset 1440); bar 1: full, ending on a unison.
    let mut groups = vec![single(1440, 60)];
    for (i, &p) in [60, 62, 64, 60].iter().enumerate() {
        groups.push(single(BAR + u32::try_from(i).unwrap() * QUARTER, p));
    }

    let score = Score {
        ticks_per_quarter: PPQN,
        master_bars,
        tracks: vec![Track {
            name: Some("lead-in".to_string()),
            channel: 0,
            voices: vec![Voice {
                id: 0,
                event_groups: groups,
            }],
        }],
        source_meta: None,
        loss: LossReport::new(),
    };

    let m = measure_structure(&score, 0).expect("measure ok");
    assert!(
        m.loopability_score < 0.7,
        "leading silence must lower loopability, got {}",
        m.loopability_score,
    );
}

// ── contract ───────────────────────────────────────────────────────────────────

#[test]
fn is_deterministic() {
    let a = vec![60, 62, 64, 65];
    let score = build_score(&[a.clone(), a.clone(), a.clone(), a]);
    assert_eq!(
        measure_structure(&score, 0).expect("ok"),
        measure_structure(&score, 0).expect("ok"),
        "pure measurement must be deterministic",
    );
}

#[test]
fn rejects_out_of_range_track() {
    let score = build_score(&[vec![60]]);
    assert!(matches!(
        measure_structure(&score, 9),
        Err(StructureError::TrackIndexOutOfRange)
    ));
}

#[test]
fn rejects_score_without_master_bars() {
    let mut score = build_score(&[vec![60]]);
    score.master_bars.clear();
    assert!(matches!(
        measure_structure(&score, 0),
        Err(StructureError::EmptyScore)
    ));
}
