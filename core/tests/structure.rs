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
    event::{
        NoteMark, NoteMarks, Pitch, SpanTechnique, TechniqueEvidence, Tempo, Ticks, TimeSignature,
        Tuning, Velocity,
    },
    score::{
        AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, Score,
        TechniqueSpan, Track, Voice,
    },
    slice::TickRange,
    structure::{measure_complexity, measure_structure, StructureError},
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
        marks: NoteMarks::empty(),
        position: None,
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
            tuning: Tuning::standard_e(),
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
    // Genuinely through-composed: the onset grid *and* the pitch material
    // change every bar, with no constant interval shift between bars. (The
    // previous fixture — a chromatic sequence over a constant rhythm — is a
    // transposed repeat, which the contour-aware signature rightly reads as
    // structure; see `transposed_repeats_read_as_medium_repeatability`.)
    let score = build_score(&[
        vec![60, 61, 62, 63],
        vec![72],
        vec![65, 71],
        vec![58, 64, 59],
    ]);
    let m = measure_structure(&score, 0).expect("measure ok");

    assert_eq!(m.detected_pattern_period_bars, None, "no repeating idea");
    assert_eq!(m.detected_pattern_period_ticks, None);
    assert!(m.repeatability_score < 0.3, "little to no repetition");
    assert_eq!(m.structural_complexity, 1.0, "every bar distinct");
}

// ── contour-aware bar similarity ───────────────────────────────────────────────

#[test]
fn transposed_repeats_read_as_medium_repeatability() {
    // A A' A'' A''' — one motif climbing a fourth per bar: identical rhythm,
    // constant interval shift. Exact-pitch comparison is blind to this (the
    // documented Phase-0 limitation); the contour-aware signature must read
    // it as a 1-bar period with *medium* repeatability — clearly above
    // unrelated material, clearly below identical tiles.
    let score = build_score(&[
        vec![60, 62, 64, 65],
        vec![65, 67, 69, 70],
        vec![70, 72, 74, 75],
        vec![75, 77, 79, 80],
    ]);
    let m = measure_structure(&score, 0).expect("measure ok");

    assert_eq!(
        m.detected_pattern_period_bars,
        Some(1),
        "a transposed tile is still a tile"
    );
    assert_eq!(m.detected_pattern_period_ticks, Some(BAR));
    assert!(
        (0.6..0.9).contains(&m.repeatability_score),
        "transposed repeats are medium, not high or low: {}",
        m.repeatability_score,
    );
    assert!(
        (0.1..0.4).contains(&m.variation_score),
        "variation mirrors repeatability: {}",
        m.variation_score,
    );
}

#[test]
fn rhythm_only_repeats_read_as_partial_repeatability() {
    // The same onset grid every bar, but unrelated pitches with no constant
    // interval shift: the tile is purely rhythmic. That is still structure
    // (a rhythm cell looping), but only partial repeatability — between
    // through-composed and a transposed repeat.
    let score = build_score(&[
        vec![60, 67, 62, 65],
        vec![66, 60, 71, 61],
        vec![63, 70, 59, 72],
        vec![68, 58, 66, 64],
    ]);
    let m = measure_structure(&score, 0).expect("measure ok");

    assert_eq!(
        m.detected_pattern_period_bars,
        Some(1),
        "a repeating onset grid is a rhythmic period"
    );
    assert!(
        (0.4..0.6).contains(&m.repeatability_score),
        "rhythm-only repeats are partial: {}",
        m.repeatability_score,
    );
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
            tuning: Tuning::standard_e(),
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
            tuning: Tuning::standard_e(),
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

// ── sub-bar (beat-level) period detection (S14 deferred refinement) ───────────
//
// TDD red phase: `StructureMetrics` grows `detected_subbar_period_ticks` — the
// strongest verbatim-tiling lag shorter than one bar, found by autocorrelation
// over per-beat cell signatures on a uniform timeline. Cells are compared by
// exact `(onset, pitch)` signatures: at one-onset granularity the bar-level
// rhythm floor and transposition credit are degenerate (almost any two
// single-note cells would "match"), so the sub-bar pass demands verbatim
// repeats. References a field that does not exist yet, so the suite fails to
// compile until the green step.

#[test]
fn detects_a_half_bar_subbar_period() {
    // The two-pitch alternation tiles every half bar (two beats).
    let score = build_score(&[vec![40, 47, 40, 47], vec![40, 47, 40, 47]]);
    let m = measure_structure(&score, 0).expect("measure");
    assert_eq!(
        m.detected_subbar_period_ticks,
        Some(2 * QUARTER),
        "the alternation tiles every half bar"
    );
    assert_eq!(
        m.detected_pattern_period_bars,
        Some(1),
        "the bar-level period is unchanged by the refinement"
    );
}

#[test]
fn a_uniform_pulse_reads_as_a_one_beat_subbar_period() {
    let score = build_score(&[vec![40, 40, 40, 40], vec![40, 40, 40, 40]]);
    let m = measure_structure(&score, 0).expect("measure");
    assert_eq!(
        m.detected_subbar_period_ticks,
        Some(QUARTER),
        "every beat is identical; ties resolve to the shortest lag"
    );
}

#[test]
fn no_subbar_period_for_whole_bar_material() {
    // A scale run: no beat cell repeats verbatim at a sub-bar lag.
    let score = build_score(&[vec![60, 62, 64, 65], vec![60, 62, 64, 65]]);
    let m = measure_structure(&score, 0).expect("measure");
    assert_eq!(
        m.detected_subbar_period_ticks, None,
        "the repeating idea is the whole bar, not a sub-bar cell"
    );
    assert_eq!(m.detected_pattern_period_bars, Some(1));
}

#[test]
fn no_subbar_period_for_a_silent_track() {
    let score = build_score(&[vec![], vec![]]);
    let m = measure_structure(&score, 0).expect("measure");
    assert_eq!(
        m.detected_subbar_period_ticks, None,
        "silence has no repeating sub-bar idea"
    );
}

#[test]
fn no_subbar_period_on_a_mixed_meter_timeline() {
    // A 4/4 bar followed by a 3/4 bar: beat cells do not align across bars,
    // so the sub-bar pass abstains instead of measuring a misaligned grid.
    let mut score = build_score(&[vec![40, 47, 40, 47]]);
    let start = BAR;
    let end = BAR + 3 * QUARTER;
    score.master_bars.push(MasterBar {
        index: 1,
        tick_range: TickRange::new(Ticks(start), Ticks(end)).expect("ordered"),
        time_signature: TimeSignature {
            numerator: 3,
            denominator: 4,
        },
        tempo: Tempo::new(120.0).expect("120 BPM"),
    });
    for (i, &p) in [40_u8, 47, 40].iter().enumerate() {
        score.tracks[0].voices[0].event_groups.push(EventGroup {
            kind: EventGroupKind::Single,
            atoms: vec![quarter_note(start + u32::try_from(i).unwrap() * QUARTER, p)],
            technique_spans: Vec::new(),
        });
    }
    let m = measure_structure(&score, 0).expect("measure");
    assert_eq!(
        m.detected_subbar_period_ticks, None,
        "a non-uniform timeline has no shared beat grid to autocorrelate"
    );
}

#[test]
fn silence_does_not_establish_a_subbar_period() {
    // Codex P2 (PR #38): one sounded beat per bar with non-repeating pitches.
    // Empty-empty beat pairs are uninformative and must sit out of the mean —
    // without that, lag 1 clears the threshold on rests alone (4/7 of the
    // pairs are empty-empty) and a false one-beat period persists into the
    // corpus.
    let score = build_score(&[vec![60], vec![67]]);
    let m = measure_structure(&score, 0).expect("measure");
    assert_eq!(
        m.detected_subbar_period_ticks, None,
        "silence alone must not establish a sub-bar period"
    );
}

#[test]
fn rest_aligned_sparse_repeats_still_read_as_a_subbar_period() {
    // Characterization guard for the fix above: an X . X . bar still reads as
    // a half-bar tile — the sounded cells carry the period on their own.
    let mut score = build_score(&[vec![], vec![]]);
    for bar in 0..2_u32 {
        for beat in [0_u32, 2] {
            score.tracks[0].voices[0].event_groups.push(EventGroup {
                kind: EventGroupKind::Single,
                atoms: vec![quarter_note(bar * BAR + beat * QUARTER, 40)],
                technique_spans: Vec::new(),
            });
        }
    }
    let m = measure_structure(&score, 0).expect("measure");
    assert_eq!(
        m.detected_subbar_period_ticks,
        Some(2 * QUARTER),
        "the sounded cells alone carry the half-bar tile"
    );
}

// ── ComplexityProfile (S14 deferred refinement: the per-axis vector) ──────────
//
// TDD red phase: `measure_complexity` derives a `ComplexityProfile` — the
// rhythmic / pitch / technical / harmonic / playability / structural vector
// the stage doc separates from length and period (ADR-0015, glossary §7).
// Every axis is a fact in [0, 1], untuned v1 measures; measurement only, no
// corpus persistence yet ("measure before target"). References an API that
// does not exist yet, so the suite fails to compile until the green step.

#[test]
fn complexity_axes_are_low_for_a_uniform_pulse() {
    // Four identical bars of the same open-string quarter pulse.
    let score = build_score(&[vec![40; 4], vec![40; 4], vec![40; 4], vec![40; 4]]);
    let c = measure_complexity(&score, 0).expect("measure");
    assert_eq!(c.rhythmic, 0.0, "one distinct inter-onset interval");
    assert_eq!(c.pitch, 0.0, "one distinct melodic interval (unison)");
    assert_eq!(c.technical, 0.0, "no marks, no spans");
    assert_eq!(c.harmonic, 0.0, "a single pitch class fits its key exactly");
    assert_eq!(c.playability, 0.0, "an open-string pedal needs no travel");
    assert_eq!(c.structural, 0.25, "one distinct bar signature out of four");
}

#[test]
fn rhythmic_axis_reads_inter_onset_variety() {
    // Onsets 0, 480, 720, 960: inter-onset intervals {480, 240, 240} — two
    // distinct values out of three intervals.
    let mut score = build_score(&[vec![]]);
    for onset in [0_u32, 480, 720, 960] {
        score.tracks[0].voices[0].event_groups.push(EventGroup {
            kind: EventGroupKind::Single,
            atoms: vec![quarter_note(onset, 40)],
            technique_spans: Vec::new(),
        });
    }
    let c = measure_complexity(&score, 0).expect("measure");
    assert_eq!(c.rhythmic, 0.5, "(2 distinct − 1) / (3 intervals − 1)");
    assert_eq!(c.pitch, 0.0, "a single pitch has no interval variety");
}

#[test]
fn pitch_axis_reads_melodic_interval_variety() {
    // Line 40 42 45 47: absolute intervals {2, 3, 2} — two distinct out of
    // three; the rhythm is a uniform quarter grid.
    let score = build_score(&[vec![40, 42, 45, 47]]);
    let c = measure_complexity(&score, 0).expect("measure");
    assert_eq!(c.pitch, 0.5, "(2 distinct − 1) / (3 intervals − 1)");
    assert_eq!(c.rhythmic, 0.0);
}

#[test]
fn technical_axis_reads_marked_and_spanned_share() {
    // Four notes: one accented (mark), one covered by a palm-mute span on its
    // group — half the notes carry technique.
    let mut score = build_score(&[vec![40, 40, 40, 40]]);
    let groups = &mut score.tracks[0].voices[0].event_groups;
    if let AtomEvent::Note(n) = &mut groups[0].atoms[0] {
        n.marks = NoteMarks::empty().with(NoteMark::Accent);
    }
    groups[2].technique_spans.push(TechniqueSpan {
        technique: SpanTechnique::PalmMute,
        tick_range: TickRange::new(Ticks(2 * QUARTER), Ticks(3 * QUARTER)).expect("ordered"),
        evidence: TechniqueEvidence::explicit(),
    });
    let c = measure_complexity(&score, 0).expect("measure");
    assert_eq!(c.technical, 0.5, "2 of 4 notes carry a mark or sit in a span");
}

#[test]
fn harmonic_axis_reads_chromaticism() {
    // The C major scale plus a chromatic C#: scale fit 8/9, so the harmonic
    // complexity is 1/9 (the off-scale share of the estimated key).
    let score = build_score(&[vec![60, 61, 62, 64, 65, 67, 69, 71, 72]]);
    let c = measure_complexity(&score, 0).expect("measure");
    assert!(
        (c.harmonic - 1.0 / 9.0).abs() < 1e-9,
        "one chromatic note out of nine, got {}",
        c.harmonic
    );
}

#[test]
fn playability_axis_is_max_for_an_unreachable_note() {
    // Pitch 5 sits below Standard E's lowest open string: unreachable.
    let score = build_score(&[vec![5, 40]]);
    let c = measure_complexity(&score, 0).expect("measure");
    assert_eq!(c.playability, 1.0, "an unpositionable line note maxes the axis");
}

#[test]
fn structural_axis_matches_structure_metrics() {
    let score = build_score(&[vec![60, 62, 64, 65], vec![40, 47, 40, 47]]);
    let c = measure_complexity(&score, 0).expect("measure");
    let m = measure_structure(&score, 0).expect("measure");
    assert_eq!(
        c.structural, m.structural_complexity,
        "one structural fact, measured once"
    );
}

#[test]
fn complexity_is_deterministic_and_in_unit_range() {
    let score = build_score(&[vec![40, 52, 45, 47], vec![60, 62, 40, 65]]);
    let a = measure_complexity(&score, 0).expect("measure");
    let b = measure_complexity(&score, 0).expect("measure");
    assert_eq!(a, b, "pure and deterministic (SPEC §6)");
    for (label, v) in [
        ("rhythmic", a.rhythmic),
        ("pitch", a.pitch),
        ("technical", a.technical),
        ("harmonic", a.harmonic),
        ("playability", a.playability),
        ("structural", a.structural),
    ] {
        assert!((0.0..=1.0).contains(&v), "{label} out of range: {v}");
    }
}

#[test]
fn complexity_rejects_out_of_range_track() {
    let score = build_score(&[vec![40]]);
    assert!(matches!(
        measure_complexity(&score, 9),
        Err(StructureError::TrackIndexOutOfRange)
    ));
}
