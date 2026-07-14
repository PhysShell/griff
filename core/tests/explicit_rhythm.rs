// S16 Phase 2: the explicit rhythm scheduler (ADR-0029 §7) — the four laws
// from the #114 review. An explicit palette is honored verbatim by its own
// scheduler: silent bars stay in the rotation, nothing falls back to
// quarters, the automatic corpus/source paths keep their filtering, and
// diagnostics never compress the palette.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::missing_assert_message,
    clippy::absolute_paths,
    clippy::missing_const_for_fn
)]

use griff_core::event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity};
use griff_core::generate::{
    explicit_rhythm_diagnostics, generate, rhythm_diagnostics, GenerationConstraints,
    GenerationSeed, GenerationStrategy, PitchMaterial, RhythmTemplate, RuleGenerationRequest,
    TemplateNote,
};
use griff_core::generation_input::{ranked_candidates, CorpusMaterial, GenerationAsk};
use griff_core::score::{
    AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, RepeatMarker, Score,
    Track, Voice,
};
use griff_core::slice::TickRange;

const PPQN: u16 = 480;
const BAR: u32 = 1920;

// ── fixtures ─────────────────────────────────────────────────────────────────

fn e_minor_pentatonic() -> PitchMaterial {
    PitchMaterial {
        root: Pitch(40),
        intervals: vec![0, 3, 5, 7, 10],
    }
}

fn constraints(bar_count: usize) -> GenerationConstraints {
    GenerationConstraints {
        bar_count,
        time_signature: TimeSignature {
            numerator: 4,
            denominator: 4,
        },
        tempo: Tempo(120.0),
        ticks_per_quarter: Ticks(u32::from(PPQN)),
        pitch_lo: Pitch(36),
        pitch_hi: Pitch(72),
    }
}

/// Two placed sixteenths: beat 1 and beat 3.
fn two_note_template() -> RhythmTemplate {
    RhythmTemplate {
        notes: vec![
            TemplateNote {
                offset: Ticks(0),
                duration: Ticks(120),
            },
            TemplateNote {
                offset: Ticks(960),
                duration: Ticks(120),
            },
        ],
    }
}

fn request_with_palette(
    strategy: GenerationStrategy,
    bar_count: usize,
    palette: Vec<RhythmTemplate>,
) -> RuleGenerationRequest {
    RuleGenerationRequest {
        seed: GenerationSeed(42),
        pitch_material: e_minor_pentatonic(),
        constraints: constraints(bar_count),
        source_rhythms: Vec::new(),
        explicit_rhythms: Some(palette),
        strategy,
    }
}

/// Onsets of every generated note, ascending.
fn onsets(score: &Score) -> Vec<u32> {
    let mut all: Vec<u32> = score.tracks[0].voices[0]
        .event_groups
        .iter()
        .flat_map(|g| g.atoms.iter())
        .filter_map(|a| match a {
            AtomEvent::Note(n) => Some(n.absolute_start.0),
            AtomEvent::Rest(_) => None,
        })
        .collect();
    all.sort_unstable();
    all
}

/// Note count inside bar `index` (half-open tick range).
fn notes_in_bar(score: &Score, index: u32) -> usize {
    let start = index * BAR;
    onsets(score)
        .iter()
        .filter(|&&o| o >= start && o < start + BAR)
        .count()
}

/// A minimal one-track 4/4 score sounding quarters — enough to seed
/// `ranked_candidates`.
fn seed_score(bar_count: usize) -> Score {
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
                tempo: Tempo::new(120.0).expect("valid tempo"),
                repeat: RepeatMarker::default(),
            }
        })
        .collect();

    let mut groups = Vec::new();
    for bar in 0..bar_count {
        let bar_start = u32::try_from(bar).unwrap() * BAR;
        for beat in 0..4_u32 {
            groups.push(EventGroup {
                kind: EventGroupKind::Single,
                atoms: vec![AtomEvent::Note(AtomNote {
                    absolute_start: Ticks(bar_start + beat * 480),
                    duration: Ticks(480),
                    pitch: Pitch::new(40 + u8::try_from(beat).unwrap()).expect("valid pitch"),
                    velocity: Velocity::new(90).expect("valid velocity"),
                    marks: NoteMarks::empty(),
                    position: None,
                })],
                technique_spans: Vec::new(),
            });
        }
    }

    Score {
        ticks_per_quarter: PPQN,
        master_bars,
        tracks: vec![Track {
            name: Some("seed".to_string()),
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

// ── law 1 + 3: silent bars survive, in rotation ─────────────────────────────

#[test]
fn silent_bars_stay_in_the_rotation() {
    let candidate = generate(&request_with_palette(
        GenerationStrategy::MotifTransposeVariation,
        4,
        vec![two_note_template(), RhythmTemplate::default()],
    ))
    .expect("generates");

    // The palette is a 2-cycle: sounding, silent, sounding, silent — the
    // silent template is a bar of the cycle, not a dropped entry.
    assert_eq!(notes_in_bar(&candidate.score, 0), 2);
    assert_eq!(notes_in_bar(&candidate.score, 1), 0, "bar 1 must be silent");
    assert_eq!(notes_in_bar(&candidate.score, 2), 2);
    assert_eq!(notes_in_bar(&candidate.score, 3), 0, "bar 3 must be silent");

    // And the sounding bars sit exactly on the template's offsets.
    let all = onsets(&candidate.score);
    assert_eq!(all, vec![0, 960, 2 * BAR, 2 * BAR + 960]);
}

// ── law 1: no quarter fallback for an explicit palette ──────────────────────

#[test]
fn an_all_silent_explicit_palette_stays_silent() {
    let candidate = generate(&request_with_palette(
        GenerationStrategy::MotifTransposeVariation,
        2,
        vec![RhythmTemplate::default()],
    ))
    .expect("generates a silent part");
    assert_eq!(
        onsets(&candidate.score).len(),
        0,
        "explicit silence must not fall back to quarters"
    );
}

// ── law 2: the automatic path keeps its filtering and fallback ──────────────

#[test]
fn the_automatic_path_still_filters_and_falls_back() {
    let candidate = generate(&RuleGenerationRequest {
        seed: GenerationSeed(42),
        pitch_material: e_minor_pentatonic(),
        constraints: constraints(1),
        source_rhythms: vec![RhythmTemplate::default()],
        explicit_rhythms: None,
        strategy: GenerationStrategy::MotifTransposeVariation,
    })
    .expect("generates");
    assert_eq!(
        notes_in_bar(&candidate.score, 0),
        4,
        "an all-empty automatic palette still falls back to quarters"
    );
}

// ── precedence: explicit beats corpus; novelty and gesture stay corpus ──────

#[test]
fn an_explicit_palette_beats_the_corpus_and_keeps_its_silence() {
    let palette = vec![two_note_template(), RhythmTemplate::default()];
    let material = CorpusMaterial {
        rhythms: vec![RhythmTemplate::from_durations(&[Ticks(480); 4])],
        references: vec![seed_score(1)],
        gesture: None,
        skipped: Vec::new(),
    };
    let set = ranked_candidates(
        &seed_score(2),
        Some(&material),
        &GenerationAsk {
            seed: 42,
            bars: 2,
            variants_per_strategy: 1,
            gesture: false,
        },
        Some(&palette),
    )
    .expect("ranks");

    assert!(set.rhythm_explicit, "provenance must say explicit");
    assert_eq!(
        set.source_rhythms, palette,
        "the palette is provenance, verbatim — silent template included"
    );
}

#[test]
fn without_an_override_the_corpus_still_wins() {
    let corpus_rhythm = RhythmTemplate::from_durations(&[Ticks(480); 4]);
    let material = CorpusMaterial {
        rhythms: vec![corpus_rhythm.clone()],
        references: Vec::new(),
        gesture: None,
        skipped: Vec::new(),
    };
    let set = ranked_candidates(
        &seed_score(2),
        Some(&material),
        &GenerationAsk {
            seed: 42,
            bars: 2,
            variants_per_strategy: 1,
            gesture: false,
        },
        None,
    )
    .expect("ranks");

    assert!(!set.rhythm_explicit);
    assert_eq!(set.source_rhythms, vec![corpus_rhythm]);
}

// ── law 4: diagnostics never compress an explicit palette ───────────────────

#[test]
fn explicit_diagnostics_report_the_palette_uncompressed() {
    let palette = [two_note_template(), RhythmTemplate::default()];
    let explicit = explicit_rhythm_diagnostics(&palette, Ticks(BAR));
    assert_eq!(explicit.loaded, 2);
    assert_eq!(explicit.effective, 2, "the silent template is not dropped");
    assert_eq!(explicit.fingerprints.len(), 2);

    // Contrast: the automatic diagnostics compress the same palette to one
    // effective grid — which is exactly why the explicit path needs its own.
    let automatic = rhythm_diagnostics(&palette, Ticks(BAR));
    assert_eq!(automatic.effective, 1);
}
