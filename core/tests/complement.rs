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
        analyze_part, arrange_complement, measure_pair_axes, validate_pair, ComplementError,
        ComplementSpec, KeyMode, RelationMode,
    },
    event::{
        NoteMark, NoteMarks, Pitch, SpanTechnique, TechniqueEvidence, Tempo, Ticks, TimeSignature,
        Tuning, Velocity,
    },
    generate::GenerationSeed,
    score::{
        AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, RepeatMarker,
        Score, TechniqueSpan, Track, Voice,
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
                repeat: RepeatMarker::default(),
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
            tuning: Tuning::standard_e(),
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

/// Regression: B must draw from A's *whole* register band, not just its bottom
/// octave. A part spanning multiple octaves previously collapsed B into
/// `[band_lo, band_lo + 11]` because pitch selection only placed scale degrees
/// in the lowest octave — so B always sounded low and cramped relative to A.
#[test]
fn rhythm_lock_b_uses_full_register_band_not_just_bottom_octave() {
    // A spans 48..84 (three octaves); offset 0 keeps B's band identical to A's.
    let score = score_with_part_a(2, &[48, 55, 60, 67, 72, 79, 84]);
    let spec = ComplementSpec {
        mode: RelationMode::RhythmLock,
        register_offset: 0,
    };
    let cand = arrange_complement(&score, 0, spec, GenerationSeed(5)).expect("arrange ok");

    let pitches = note_pitches(&cand.score, cand.part_b_index);
    let hi = *pitches.iter().max().expect("B has notes");
    // The band floor is 48, so the old bottom-octave cap was 59. Reaching above
    // 60 is impossible unless B uses the band's upper octaves.
    assert!(
        hi > 60,
        "B must reach A's upper register (band 48..84), got max {hi}",
    );
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
            repeat: RepeatMarker::default(),
        },
        MasterBar {
            index: 1,
            tick_range: TickRange::new(Ticks(BAR), Ticks(BAR + three_four)).expect("ordered"),
            time_signature: TimeSignature {
                numerator: 3,
                denominator: 4,
            },
            tempo: Tempo::new(120.0).expect("120 BPM"),
            repeat: RepeatMarker::default(),
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
            tuning: Tuning::standard_e(),
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
        tuning: Tuning::standard_e(),
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
        tuning: Tuning::standard_e(),
    });

    let v = validate_pair(&score, 0, 1).expect("validate ok");
    assert_eq!(v.coincident_dissonances, 0, "octaves/consonances are clean");
    assert!(
        !v.register_mud,
        "an octave of separation is not register mud"
    );
    assert!(v.is_clean());
}

#[test]
fn analyze_part_includes_spanning_techniques() {
    // Part A has a palm-mute span but no per-note marks. analyze_part must still
    // surface the technique, otherwise a technique-free complement scores as a
    // perfect technique match (jaccard of two empty sets = 1.0). Codex P2,
    // ADR-0018 Slice 2.
    let note = AtomEvent::Note(AtomNote {
        absolute_start: Ticks(0),
        duration: Ticks(QUARTER),
        pitch: Pitch::new(60).expect("valid pitch"),
        velocity: Velocity::new(90).expect("valid velocity"),
        marks: NoteMarks::empty(),
        position: None,
    });
    let group = EventGroup {
        kind: EventGroupKind::Single,
        atoms: vec![note],
        technique_spans: vec![TechniqueSpan {
            technique: SpanTechnique::PalmMute,
            tick_range: TickRange::new(Ticks(0), Ticks(QUARTER)).expect("ordered range"),
            evidence: TechniqueEvidence::explicit(),
        }],
    };
    let score = Score {
        ticks_per_quarter: PPQN,
        master_bars: vec![MasterBar {
            index: 0,
            tick_range: TickRange::new(Ticks(0), Ticks(BAR)).expect("ordered range"),
            time_signature: TimeSignature {
                numerator: 4,
                denominator: 4,
            },
            tempo: Tempo(120.0),
            repeat: RepeatMarker::default(),
        }],
        tracks: vec![Track {
            name: None,
            channel: 0,
            voices: vec![Voice {
                id: 0,
                event_groups: vec![group],
            }],
            tuning: Tuning::standard_e(),
        }],
        source_meta: None,
        loss: LossReport::default(),
    };

    let profile = analyze_part(&score, 0).expect("analyze");
    assert!(
        profile.techniques.contains("palm_mute"),
        "spanning techniques must be in the profile, not only per-note marks"
    );
}

// ── octave_double ───────────────────────────────────────────────────────────────

#[test]
fn octave_double_reproduces_contour_an_octave_down() {
    let score = score_with_part_a(2, &[52, 55, 59, 62]);
    let spec = ComplementSpec {
        mode: RelationMode::OctaveDouble,
        register_offset: -12,
    };
    let cand = arrange_complement(&score, 0, spec, GenerationSeed(7)).expect("arrange ok");

    assert_eq!(
        note_onsets(&cand.score, cand.part_b_index),
        note_onsets(&cand.score, 0),
        "the double keeps A's onsets"
    );
    assert_eq!(
        note_durations(&cand.score, cand.part_b_index),
        note_durations(&cand.score, 0),
        "the double keeps A's durations"
    );
    let expected: Vec<u8> = note_pitches(&cand.score, 0)
        .iter()
        .map(|p| p - 12)
        .collect();
    assert_eq!(
        note_pitches(&cand.score, cand.part_b_index),
        expected,
        "every pitch shifts by exactly one octave"
    );
}

#[test]
fn octave_double_shifts_up_as_well() {
    let score = score_with_part_a(1, &[52, 55]);
    let spec = ComplementSpec {
        mode: RelationMode::OctaveDouble,
        register_offset: 24,
    };
    let cand = arrange_complement(&score, 0, spec, GenerationSeed(7)).expect("arrange ok");
    assert_eq!(note_pitches(&cand.score, cand.part_b_index), vec![76, 79]);
}

#[test]
fn octave_double_clamps_at_the_midi_floor() {
    let score = score_with_part_a(1, &[5, 17]);
    let spec = ComplementSpec {
        mode: RelationMode::OctaveDouble,
        register_offset: -12,
    };
    let cand = arrange_complement(&score, 0, spec, GenerationSeed(7)).expect("arrange ok");
    assert_eq!(
        note_pitches(&cand.score, cand.part_b_index),
        vec![0, 5],
        "out-of-range doubles clamp to the MIDI floor instead of failing"
    );
}

#[test]
fn octave_double_requires_a_whole_octave_offset() {
    let score = score_with_part_a(1, &[60, 62]);
    for offset in [0_i8, -7, 5, 13] {
        let spec = ComplementSpec {
            mode: RelationMode::OctaveDouble,
            register_offset: offset,
        };
        assert!(
            matches!(
                arrange_complement(&score, 0, spec, GenerationSeed(1)),
                Err(ComplementError::InvalidSpec(RelationMode::OctaveDouble)),
            ),
            "offset {offset} is not a non-zero whole octave"
        );
    }
}

#[test]
fn octave_double_preserves_note_marks() {
    let mut score = score_with_part_a(1, &[60, 62]);
    if let AtomEvent::Note(n) = &mut score.tracks[0].voices[0].event_groups[0].atoms[0] {
        n.marks.insert(NoteMark::Accent);
    }
    let spec = ComplementSpec {
        mode: RelationMode::OctaveDouble,
        register_offset: -12,
    };
    let cand = arrange_complement(&score, 0, spec, GenerationSeed(7)).expect("arrange ok");
    let b_track = &cand.score.tracks[cand.part_b_index];
    let AtomEvent::Note(first) = &b_track.voices[0].event_groups[0].atoms[0] else {
        panic!("first B atom must be a note");
    };
    assert!(
        first.marks.contains(NoteMark::Accent),
        "the double articulates like A"
    );
}

#[test]
fn octave_double_axis_scores_report_locked_rhythm_and_density() {
    let score = score_with_part_a(2, &[52, 55, 59, 62]);
    let spec = ComplementSpec {
        mode: RelationMode::OctaveDouble,
        register_offset: -12,
    };
    let cand = arrange_complement(&score, 0, spec, GenerationSeed(7)).expect("arrange ok");
    assert_eq!(cand.axis_scores.rhythm_similarity, 1.0);
    assert_eq!(cand.axis_scores.density_ratio, 1.0);
    assert_eq!(
        cand.axis_scores.register_overlap, 0.0,
        "bands [52, 62] and [40, 50] are disjoint"
    );
    assert_eq!(
        cand.axis_scores.technique_overlap, 1.0,
        "both parts are technique-free"
    );
}

// ── register_contrast ───────────────────────────────────────────────────────────

#[test]
fn register_contrast_band_is_disjoint_and_grid_locked() {
    let score = score_with_part_a(2, &[60, 62, 64, 65]);
    let spec = ComplementSpec {
        mode: RelationMode::RegisterContrast,
        register_offset: -24,
    };
    let cand = arrange_complement(&score, 0, spec, GenerationSeed(11)).expect("arrange ok");

    assert_eq!(
        note_onsets(&cand.score, cand.part_b_index),
        note_onsets(&score, 0),
        "register_contrast keeps A's onset grid"
    );
    assert_eq!(
        note_durations(&cand.score, cand.part_b_index),
        note_durations(&score, 0),
    );
    // A's band [60, 65] shifted down two octaves → [36, 41], fully disjoint.
    for p in note_pitches(&cand.score, cand.part_b_index) {
        assert!(
            (36..=41).contains(&p),
            "B pitch {p} must lie in the disjoint band [36, 41]"
        );
    }
    assert_eq!(
        cand.axis_scores.register_overlap, 0.0,
        "disjoint registers by construction"
    );
}

#[test]
fn register_contrast_draws_pitches_from_a_harmonic_context() {
    let score = score_with_part_a(2, &[60, 62, 64, 65]);
    let spec = ComplementSpec {
        mode: RelationMode::RegisterContrast,
        register_offset: -24,
    };
    let cand = arrange_complement(&score, 0, spec, GenerationSeed(11)).expect("arrange ok");
    // A (C D E F) infers C major, so the substitution material is the C major
    // scale as intervals above B's band floor 36; the band [36, 41] clamps the
    // upper scale degrees back onto 41.
    let allowed = [36, 38, 40, 41];
    for p in note_pitches(&cand.score, cand.part_b_index) {
        assert!(
            allowed.contains(&p),
            "B pitch {p} must come from A's harmonic context in the new band"
        );
    }
}

#[test]
fn register_contrast_rejects_overlapping_shift() {
    let score = score_with_part_a(1, &[60, 62, 64, 65]);
    // A's band [60, 65] has span 5: shifts of 0 or ±5 still overlap it.
    for offset in [0_i8, -5, 5] {
        let spec = ComplementSpec {
            mode: RelationMode::RegisterContrast,
            register_offset: offset,
        };
        assert!(
            matches!(
                arrange_complement(&score, 0, spec, GenerationSeed(1)),
                Err(ComplementError::InvalidSpec(RelationMode::RegisterContrast)),
            ),
            "offset {offset} leaves the bands overlapping"
        );
    }
}

#[test]
fn register_contrast_rejects_overlap_reintroduced_by_clamping() {
    // A's band is [0, 20]; shifting down two octaves clamps back onto pitch 0,
    // which A occupies — the contrast contract cannot be met.
    let score = score_with_part_a(1, &[0, 20]);
    let spec = ComplementSpec {
        mode: RelationMode::RegisterContrast,
        register_offset: -24,
    };
    assert!(matches!(
        arrange_complement(&score, 0, spec, GenerationSeed(1)),
        Err(ComplementError::InvalidSpec(RelationMode::RegisterContrast)),
    ));
}

#[test]
fn register_contrast_is_deterministic_for_fixed_seed() {
    let score = score_with_part_a(3, &[60, 62, 64, 65]);
    let spec = ComplementSpec {
        mode: RelationMode::RegisterContrast,
        register_offset: 24,
    };
    let a = arrange_complement(&score, 0, spec, GenerationSeed(42)).expect("arrange ok");
    let b = arrange_complement(&score, 0, spec, GenerationSeed(42)).expect("arrange ok");
    assert_eq!(
        note_pitches(&a.score, a.part_b_index),
        note_pitches(&b.score, b.part_b_index),
    );
}

// ── support_layer ───────────────────────────────────────────────────────────────

#[test]
fn support_layer_is_sparser_and_pedals_the_shifted_root() {
    let score = score_with_part_a(2, &[60, 62, 64, 65]);
    let spec = ComplementSpec {
        mode: RelationMode::SupportLayer,
        register_offset: -12,
    };
    let cand = arrange_complement(&score, 0, spec, GenerationSeed(5)).expect("arrange ok");

    // One note per bar: A's first onset in each bar, A's lowest pitch − 12.
    assert_eq!(
        note_onsets(&cand.score, cand.part_b_index),
        vec![0, BAR],
        "one B note at each bar's first A onset"
    );
    assert_eq!(note_pitches(&cand.score, cand.part_b_index), vec![48, 48]);
    assert_eq!(
        note_durations(&cand.score, cand.part_b_index),
        vec![QUARTER, QUARTER],
        "B keeps the duration of the A note it shadows"
    );

    assert_eq!(
        cand.axis_scores.density_ratio, 0.25,
        "1 note/bar against A's 4 notes/bar"
    );
    assert!(
        cand.axis_scores.density_ratio < 1.0,
        "support layer must be sparser than A"
    );
}

#[test]
fn support_layer_skips_bars_where_a_is_silent() {
    let mut score = score_with_part_a(2, &[60, 62]);
    // Silence bar 1: keep only the groups whose notes start inside bar 0.
    score.tracks[0].voices[0]
        .event_groups
        .retain(|g| match &g.atoms[0] {
            AtomEvent::Note(n) => n.absolute_start.0 < BAR,
            AtomEvent::Rest(_) => false,
        });
    let spec = ComplementSpec {
        mode: RelationMode::SupportLayer,
        register_offset: -12,
    };
    let cand = arrange_complement(&score, 0, spec, GenerationSeed(5)).expect("arrange ok");
    assert_eq!(
        note_onsets(&cand.score, cand.part_b_index),
        vec![0],
        "no pedal in a bar where A is silent"
    );
}

#[test]
fn support_layer_rhythm_similarity_is_the_onset_jaccard() {
    let score = score_with_part_a(2, &[60, 62, 64, 65]);
    let spec = ComplementSpec {
        mode: RelationMode::SupportLayer,
        register_offset: -12,
    };
    let cand = arrange_complement(&score, 0, spec, GenerationSeed(5)).expect("arrange ok");
    // B's 2 onsets are a subset of A's 8: |∩| / |∪| = 2/8.
    assert_eq!(cand.axis_scores.rhythm_similarity, 0.25);
}

// ── call_response ───────────────────────────────────────────────────────────────

/// A score whose single track plays exactly `notes` (`(onset, duration, pitch)`).
fn score_with_notes(bar_count: usize, notes: &[(u32, u32, u8)]) -> Score {
    let mut score = score_with_part_a(bar_count, &[60]);
    score.tracks[0].voices[0].event_groups = notes
        .iter()
        .map(|&(onset, duration, pitch)| {
            single_group(AtomEvent::Note(AtomNote {
                absolute_start: Ticks(onset),
                duration: Ticks(duration),
                pitch: Pitch::new(pitch).expect("valid pitch"),
                velocity: Velocity::new(90).expect("valid velocity"),
                marks: NoteMarks::empty(),
                position: None,
            }))
        })
        .collect();
    score
}

#[test]
fn call_response_answers_in_a_gaps() {
    // A plays the first half of the bar; the second half is one 2-quarter gap.
    let score = score_with_notes(1, &[(0, QUARTER, 60), (QUARTER, QUARTER, 64)]);
    let spec = ComplementSpec {
        mode: RelationMode::CallResponse,
        register_offset: -12,
    };
    let cand = arrange_complement(&score, 0, spec, GenerationSeed(13)).expect("arrange ok");

    assert_eq!(
        note_onsets(&cand.score, cand.part_b_index),
        vec![2 * QUARTER],
        "one answer at the gap start"
    );
    assert_eq!(
        note_durations(&cand.score, cand.part_b_index),
        vec![2 * QUARTER],
        "the answer sustains through the gap"
    );
    // A's band [60, 64] shifted down an octave; A (C, E) infers C major, and
    // the band [48, 52] clamps the upper scale degrees onto 52.
    for p in note_pitches(&cand.score, cand.part_b_index) {
        assert!(
            [48, 50, 52].contains(&p),
            "B pitch {p} must come from A's harmonic context in the shifted band"
        );
    }
    assert_eq!(
        cand.axis_scores.rhythm_similarity, 0.0,
        "call and response onsets are disjoint by construction"
    );
}

#[test]
fn call_response_answers_every_gap_across_bars() {
    let score = score_with_notes(2, &[(0, QUARTER, 60), (BAR, QUARTER, 62)]);
    let spec = ComplementSpec {
        mode: RelationMode::CallResponse,
        register_offset: -12,
    };
    let cand = arrange_complement(&score, 0, spec, GenerationSeed(13)).expect("arrange ok");
    assert_eq!(
        note_onsets(&cand.score, cand.part_b_index),
        vec![QUARTER, BAR + QUARTER],
        "one answer per gap, in both bars"
    );
    assert_eq!(
        note_durations(&cand.score, cand.part_b_index),
        vec![3 * QUARTER, 3 * QUARTER],
    );
}

#[test]
fn call_response_ignores_leading_silence_and_sub_quarter_gaps() {
    // Leading silence (no call yet), then only sub-quarter holes: nothing to
    // answer anywhere.
    let score = score_with_notes(
        1,
        &[(QUARTER, QUARTER, 60), (1200, 240, 62), (1560, 360, 64)],
    );
    let spec = ComplementSpec {
        mode: RelationMode::CallResponse,
        register_offset: -12,
    };
    assert!(matches!(
        arrange_complement(&score, 0, spec, GenerationSeed(13)),
        Err(ComplementError::NoGapsToAnswer),
    ));
}

#[test]
fn call_response_on_a_wall_to_wall_part_is_an_error() {
    let score = score_with_part_a(2, &[60, 62, 64, 65]);
    let spec = ComplementSpec {
        mode: RelationMode::CallResponse,
        register_offset: -12,
    };
    assert!(matches!(
        arrange_complement(&score, 0, spec, GenerationSeed(13)),
        Err(ComplementError::NoGapsToAnswer),
    ));
}

#[test]
fn call_response_is_deterministic_for_fixed_seed() {
    let score = score_with_notes(2, &[(0, QUARTER, 60), (BAR, QUARTER, 64)]);
    let spec = ComplementSpec {
        mode: RelationMode::CallResponse,
        register_offset: -12,
    };
    let a = arrange_complement(&score, 0, spec, GenerationSeed(21)).expect("arrange ok");
    let b = arrange_complement(&score, 0, spec, GenerationSeed(21)).expect("arrange ok");
    assert_eq!(
        note_pitches(&a.score, a.part_b_index),
        note_pitches(&b.score, b.part_b_index),
    );
}

// ── counter_melody ──────────────────────────────────────────────────────────────

#[test]
fn counter_melody_is_an_independent_line_in_the_shifted_band() {
    let score = score_with_part_a(2, &[60, 62, 64, 65]);
    let spec = ComplementSpec {
        mode: RelationMode::CounterMelody,
        register_offset: -24,
    };
    let cand = arrange_complement(&score, 0, spec, GenerationSeed(17)).expect("arrange ok");

    assert_eq!(cand.score.tracks.len(), 2, "A plus appended B");
    assert_eq!(
        cand.score.master_bars.len(),
        score.master_bars.len(),
        "B lives on A's master bars"
    );
    let onsets = note_onsets(&cand.score, cand.part_b_index);
    assert!(!onsets.is_empty(), "the counter-melody plays something");
    let span_end = score.master_bars.last().unwrap().tick_range.end.0;
    for onset in &onsets {
        assert!(
            *onset < span_end,
            "B onset {onset} must lie within A's span"
        );
    }
    // A's band [60, 65] shifted down two octaves with A's harmonic context —
    // the inferred C major scale above the new floor 36 — clamped to [36, 41].
    let allowed = [36, 38, 40, 41];
    for p in note_pitches(&cand.score, cand.part_b_index) {
        assert!(
            allowed.contains(&p),
            "B pitch {p} must come from A's harmonic context in the shifted band"
        );
    }
}

#[test]
fn counter_melody_is_deterministic_for_fixed_seed() {
    let score = score_with_part_a(3, &[60, 62, 64, 65]);
    let spec = ComplementSpec {
        mode: RelationMode::CounterMelody,
        register_offset: -24,
    };
    let a = arrange_complement(&score, 0, spec, GenerationSeed(33)).expect("arrange ok");
    let b = arrange_complement(&score, 0, spec, GenerationSeed(33)).expect("arrange ok");
    assert_eq!(
        note_onsets(&a.score, a.part_b_index),
        note_onsets(&b.score, b.part_b_index),
    );
    assert_eq!(
        note_pitches(&a.score, a.part_b_index),
        note_pitches(&b.score, b.part_b_index),
    );
}

#[test]
fn counter_melody_rejects_a_non_uniform_timeline() {
    // The S6 delegate lays bars back-to-back from one meter; a mid-score meter
    // change would misalign with A's master bars.
    let mut score = score_with_part_a(2, &[60, 62]);
    score.master_bars[1].time_signature = TimeSignature {
        numerator: 3,
        denominator: 4,
    };
    score.master_bars[1].tick_range =
        TickRange::new(Ticks(BAR), Ticks(BAR + 3 * QUARTER)).expect("ordered");
    let spec = ComplementSpec {
        mode: RelationMode::CounterMelody,
        register_offset: -24,
    };
    assert!(matches!(
        arrange_complement(&score, 0, spec, GenerationSeed(1)),
        Err(ComplementError::NonUniformTimeline),
    ));
}

// ── measure_pair_axes: relation between two real tracks (schema v4) ───────────

/// Appends a second track playing `atoms` to a one-bar part-A score.
fn with_part_b(mut score: Score, atoms: Vec<AtomEvent>) -> Score {
    score.tracks.push(Track {
        name: Some("B".to_string()),
        channel: 1,
        voices: vec![Voice {
            id: 0,
            event_groups: atoms.into_iter().map(single_group).collect(),
        }],
        tuning: Tuning::standard_e(),
    });
    score
}

#[test]
fn identical_parts_measure_unit_axes() {
    let pitches = [60, 62, 64, 65];
    let base = score_with_part_a(1, &pitches);
    let b_atoms: Vec<AtomEvent> = pitches
        .iter()
        .enumerate()
        .map(|(i, &p)| quarter_note(u32::try_from(i).unwrap() * QUARTER, p))
        .collect();
    let score = with_part_b(base, b_atoms);

    let axes = measure_pair_axes(&score, 0, 1).expect("measure");
    assert_eq!(axes.rhythm_similarity, 1.0, "same onset set");
    assert_eq!(axes.register_overlap, 1.0, "same band");
    assert_eq!(axes.density_ratio, 1.0, "same density");
    assert_eq!(axes.technique_overlap, 1.0, "both technique sets empty");
}

#[test]
fn sparse_disjoint_pair_measures_exact_shares() {
    // A: four quarters 60 62 64 65. B: two notes at onsets 0 and 960 (a
    // subset of A's onsets — Jaccard 2/4), an octave-plus below (disjoint
    // band), half A's density.
    let base = score_with_part_a(1, &[60, 62, 64, 65]);
    let score = with_part_b(
        base,
        vec![quarter_note(0, 40), quarter_note(2 * QUARTER, 41)],
    );

    let axes = measure_pair_axes(&score, 0, 1).expect("measure");
    assert_eq!(axes.rhythm_similarity, 0.5);
    assert_eq!(axes.register_overlap, 0.0);
    assert_eq!(axes.density_ratio, 0.5);
    assert_eq!(axes.technique_overlap, 1.0);
}

#[test]
fn density_ratio_reads_the_second_part_relative_to_the_first() {
    let base = score_with_part_a(1, &[60, 62, 64, 65]);
    let score = with_part_b(
        base,
        vec![quarter_note(0, 40), quarter_note(2 * QUARTER, 41)],
    );

    let forward = measure_pair_axes(&score, 0, 1).expect("measure");
    let swapped = measure_pair_axes(&score, 1, 0).expect("measure");
    assert_eq!(forward.density_ratio, 0.5);
    assert_eq!(
        swapped.density_ratio, 2.0,
        "orientation is (b relative to a)"
    );
    assert_eq!(
        forward.rhythm_similarity, swapped.rhythm_similarity,
        "Jaccard axes are symmetric"
    );
}

#[test]
fn measure_pair_rejects_bad_tracks() {
    let base = score_with_part_a(1, &[60, 62, 64, 65]);
    let score = with_part_b(base, Vec::new());

    assert!(matches!(
        measure_pair_axes(&score, 0, 5),
        Err(ComplementError::TrackIndexOutOfRange),
    ));
    assert!(matches!(
        measure_pair_axes(&score, 0, 1),
        Err(ComplementError::PartHasNoNotes),
    ));
}

#[test]
fn measured_axes_are_deterministic_and_comparable() {
    let base = score_with_part_a(1, &[60, 62, 64, 65]);
    let score = with_part_b(
        base,
        vec![quarter_note(0, 40), quarter_note(2 * QUARTER, 41)],
    );

    let a = measure_pair_axes(&score, 0, 1).expect("measure");
    let b = measure_pair_axes(&score, 0, 1).expect("measure");
    assert_eq!(a, b, "AxisScores must be comparable for persistence");
}

// ── validate_pair: per-part playability (S13 backlog) ─────────────────────────
//
// TDD red phase: the pair validator grows the per-part playability filter
// the stage doc lists beyond the harmonic / register-mud checks. Each part's
// melodic line (the shared highest-pitch-per-onset convention) is measured
// on the optimal fingering path under the track's own tuning
// (`fretboard::measure_playability`, ADR-0019); a part with an unreachable
// note is not playable, and the pair is not clean. References fields that
// do not exist yet, so the suite fails until the green step.

/// Appends a track named "B" playing `pitches` as contiguous quarters.
fn push_part_b(score: &mut Score, pitches: &[u8]) {
    let b_groups: Vec<EventGroup> = pitches
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
        tuning: Tuning::standard_e(),
    });
}

#[test]
fn validate_pair_measures_part_playability() {
    // The consonant octave-apart pair: every note reachable on Standard E.
    let mut score = score_with_part_a(1, &[60, 64, 67, 72]);
    push_part_b(&mut score, &[48, 52, 55, 60]);

    let v = validate_pair(&score, 0, 1).expect("validate ok");
    assert_eq!(v.a_playability.note_count, 4);
    assert_eq!(v.a_playability.unpositionable, 0);
    assert!(v.a_playability.is_playable());
    assert!(v.b_playability.is_playable());
    assert!(v.is_clean(), "playable consonant pair stays clean");
}

#[test]
fn validate_pair_flags_unplayable_part() {
    // B's lone note (C2 = 36) sits below Standard E's low string: harmonically
    // consonant with A (two octaves below 60) and far from register mud, so
    // only the playability filter can reject this pair.
    let mut score = score_with_part_a(1, &[60, 62, 64, 65]);
    push_part_b(&mut score, &[36]);

    let v = validate_pair(&score, 0, 1).expect("validate ok");
    assert_eq!(
        v.coincident_dissonances, 0,
        "two octaves apart is consonant"
    );
    assert!(!v.register_mud);
    assert!(v.a_playability.is_playable());
    assert_eq!(v.b_playability.unpositionable, 1);
    assert!(!v.b_playability.is_playable());
    assert!(!v.is_clean(), "an unplayable part is not a clean pair");
}

#[test]
fn validate_pair_playability_reads_the_top_line() {
    // A's first onset is a two-note chord (36 + 60): the line convention
    // folds it to its top note, so the unreachable 36 is chord voicing —
    // deferred per ADR-0019 §7 — not a line-playability loss.
    let mut score = score_with_part_a(1, &[60, 62]);
    score.tracks[0].voices[0]
        .event_groups
        .push(single_group(quarter_note(0, 36)));
    push_part_b(&mut score, &[84, 86]);

    let v = validate_pair(&score, 0, 1).expect("validate ok");
    assert_eq!(v.a_playability.note_count, 2, "one entry per onset");
    assert_eq!(v.a_playability.unpositionable, 0);
    assert!(v.a_playability.is_playable());
    assert!(v.is_clean());
}

// ── PartProfile: harmonic context (S13 backlog, key/scale fit) ────────────────
//
// TDD red phase: `analyze_part` grows the harmonic context the stage doc lists
// as its last backlog item — a Krumhansl–Schmuckler key estimate (the
// Krumhansl–Kessler profiles correlated against the part's duration-weighted
// pitch-class histogram) carried as a fact in `PartProfile::harmony`, with
// `scale_fit` the duration-weighted fraction of notes on the inferred key's
// scale. Pitch material is enriched, not replaced: B substitutes from A's
// literal pitch classes *plus* the inferred key's scale. References fields
// that do not exist yet, so the suite fails to compile until the green step.

#[test]
fn analyze_part_infers_a_major_key() {
    // The C major scale, tonic doubled at the octave: unambiguous, diatonic.
    let score = score_with_part_a(2, &[60, 62, 64, 65, 67, 69, 71, 72]);
    let profile = analyze_part(&score, 0).expect("analyze ok");
    let harmony = profile
        .harmony
        .expect("a note-bearing part has a key estimate");
    assert_eq!(harmony.tonic_pitch_class, 0, "C");
    assert_eq!(harmony.mode, KeyMode::Major);
    assert!(
        (harmony.scale_fit - 1.0).abs() < 1e-12,
        "every note diatonic => scale_fit 1.0, got {}",
        harmony.scale_fit
    );
}

#[test]
fn analyze_part_infers_a_minor_key() {
    // E natural minor with the tonic doubled at the octave (E3 + E4): the
    // pitch-class set equals G major's, so only the tonal weighting of the
    // profiles can (and must) pick the minor tonic.
    let score = score_with_part_a(2, &[52, 55, 57, 59, 60, 62, 64]);
    let profile = analyze_part(&score, 0).expect("analyze ok");
    let harmony = profile
        .harmony
        .expect("a note-bearing part has a key estimate");
    assert_eq!(harmony.tonic_pitch_class, 4, "E");
    assert_eq!(harmony.mode, KeyMode::Minor);
    assert!(
        (harmony.scale_fit - 1.0).abs() < 1e-12,
        "every note on the natural-minor scale => scale_fit 1.0, got {}",
        harmony.scale_fit
    );
}

#[test]
fn analyze_part_scale_fit_drops_on_chromatic_notes() {
    // The C major scale plus a chromatic C#: 8 of 9 equal-duration notes are
    // diatonic, so the duration-weighted scale fit is exactly 8/9.
    let score = score_with_part_a(1, &[60, 61, 62, 64, 65, 67, 69, 71, 72]);
    let profile = analyze_part(&score, 0).expect("analyze ok");
    let harmony = profile
        .harmony
        .expect("a note-bearing part has a key estimate");
    assert_eq!(harmony.tonic_pitch_class, 0, "C");
    assert_eq!(harmony.mode, KeyMode::Major);
    assert!(
        (harmony.scale_fit - 8.0 / 9.0).abs() < 1e-12,
        "one chromatic note out of nine => scale_fit 8/9, got {}",
        harmony.scale_fit
    );
}

#[test]
fn analyze_part_empty_part_has_no_harmony() {
    let score = score_with_part_a(1, &[]);
    let profile = analyze_part(&score, 0).expect("analyze ok");
    assert!(
        profile.harmony.is_none(),
        "no notes => no key estimate, not a fabricated one"
    );
}

#[test]
fn pitch_material_is_enriched_by_the_inferred_key() {
    // A sounds only E (pitch class 4, two octaves): literal pitch-class
    // material would collapse B onto a single pitch per band. The inferred
    // key (E major) opens the whole scale as substitution material.
    let score = score_with_part_a(2, &[52, 64, 52, 64]);
    let spec = ComplementSpec {
        mode: RelationMode::RhythmLock,
        register_offset: -12,
    };
    let cand = arrange_complement(&score, 0, spec, GenerationSeed(11)).expect("arrange ok");

    // E major pitch classes: E F# G# A B C# D#.
    let e_major = [4_u8, 6, 8, 9, 11, 1, 3];
    let pitches = note_pitches(&cand.score, cand.part_b_index);
    for &p in &pitches {
        assert!(
            e_major.contains(&(p % 12)),
            "B pitch {p} must stay inside A's inferred key"
        );
    }
    let mut distinct: Vec<u8> = pitches.iter().map(|p| p % 12).collect();
    distinct.sort_unstable();
    distinct.dedup();
    assert!(
        distinct.len() > 1,
        "the key estimate opens material beyond A's literal pitch classes"
    );
}

#[test]
fn the_inferred_key_holds_under_a_non_octave_shift() {
    // Codex P2 (PR #38): the substitution material is anchored to A's
    // inferred key's pitch classes; the register band only picks the octave.
    // A C-major part shifted up a fifth must not grow an F# — the offsets
    // must be measured from the band floor's pitch class, not transposed
    // wholesale with the band.
    let score = score_with_part_a(2, &[60, 62, 64, 65, 67, 69, 71, 72]);
    let spec = ComplementSpec {
        mode: RelationMode::RhythmLock,
        register_offset: 7,
    };
    let cand = arrange_complement(&score, 0, spec, GenerationSeed(11)).expect("arrange ok");
    let c_major = [0_u8, 2, 4, 5, 7, 9, 11];
    for p in note_pitches(&cand.score, cand.part_b_index) {
        assert!(
            c_major.contains(&(p % 12)),
            "B pitch {p} (pc {}) must stay in A's inferred key",
            p % 12
        );
    }
}
