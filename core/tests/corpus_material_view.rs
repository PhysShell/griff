// TDD red: the corpus-material view under `ranked_candidates` (generator
// observatory design note §8.1) — a borrowed subset of the corpus channels,
// the one implementation the public entry point enters, and the contribution
// a pass actually took from it.
//
// The fixture mirrors `generation_input_characterization.rs`, whose pins keep
// the public path identical to main while this seam lands underneath it.
#![allow(clippy::expect_used, clippy::missing_assert_message, clippy::float_cmp)]

use griff_core::event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity};
use griff_core::generate::{RhythmTemplate, TemplateNote};
use griff_core::generation_input::{
    generation_request_from_score, ranked_candidates, ranked_candidates_from_view,
    CorpusContribution, CorpusMaterial, CorpusMaterialView, GenerationAsk, RankedSet,
};
use griff_core::gesture::GestureControl;
use griff_core::novelty::NOVELTY_AXIS_LABELS;
use griff_core::score::{
    index_from_ordinal, AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar,
    RepeatMarker, Score, Track, Voice,
};
use griff_core::slice::TickRange;

const PPQN: u32 = 480;
const BAR: u32 = 1920;

/// Two 4/4 bars of the given `(onset, duration, pitch)` notes on one track.
fn score_of(notes: &[(u32, u32, u8)]) -> Score {
    let master_bars = (0..2_usize)
        .map(|i| {
            let start = u32::try_from(i).expect("two bars").saturating_mul(BAR);
            MasterBar {
                index: index_from_ordinal(i),
                tick_range: TickRange::new(Ticks(start), Ticks(start.saturating_add(BAR)))
                    .expect("ordered"),
                time_signature: TimeSignature::new(4, 4).expect("4/4"),
                tempo: Tempo::from_bpm_integer(120).expect("120 BPM"),
                repeat: RepeatMarker::default(),
            }
        })
        .collect();
    let event_groups = notes
        .iter()
        .map(|&(onset, duration, pitch)| EventGroup {
            kind: EventGroupKind::Single,
            atoms: vec![AtomEvent::Note(AtomNote {
                absolute_start: Ticks(onset),
                duration: Ticks(duration),
                pitch: Pitch::new(pitch).expect("valid pitch"),
                velocity: Velocity::new(96).expect("valid velocity"),
                marks: NoteMarks::empty(),
                position: None,
            })],
            technique_spans: Vec::new(),
        })
        .collect();
    Score {
        ticks_per_quarter: u16::try_from(PPQN).expect("fits"),
        master_bars,
        tracks: vec![Track {
            name: Some("guitar".to_owned()),
            channel: 0,
            voices: vec![Voice {
                id: 0,
                event_groups,
            }],
            tuning: Tuning::standard_e(),
        }],
        source_meta: None,
        loss: LossReport::new(),
    }
}

fn source() -> Score {
    score_of(&[
        (0, 480, 40),
        (480, 480, 43),
        (960, 480, 45),
        (1440, 480, 47),
        (1920, 960, 50),
        (2880, 480, 47),
        (3360, 480, 45),
    ])
}

fn template(notes: &[(u32, u32)]) -> RhythmTemplate {
    RhythmTemplate {
        notes: notes
            .iter()
            .map(|&(offset, duration)| TemplateNote {
                offset: Ticks(offset),
                duration: Ticks(duration),
            })
            .collect(),
    }
}

/// A corpus with every channel populated: two rhythm templates, two
/// references (one quoting the source's opening), and a gesture ask.
fn material() -> CorpusMaterial {
    CorpusMaterial {
        rhythms: vec![
            template(&[(0, 240), (240, 240), (480, 480), (960, 960)]),
            template(&[(0, 480), (720, 240), (960, 240), (1440, 480)]),
        ],
        references: vec![
            score_of(&[(0, 480, 40), (480, 480, 43), (960, 480, 45)]),
            score_of(&[(0, 240, 52), (240, 240, 50), (480, 960, 47)]),
        ],
        gesture: Some(GestureControl {
            burst_notes: 3,
            rest_quarters: 1.0,
        }),
        skipped: vec!["unreadable.chunk.json".to_owned()],
    }
}

const fn ask(gesture: bool) -> GenerationAsk {
    GenerationAsk {
        seed: 42,
        bars: 4,
        variants_per_strategy: 2,
        gesture,
        tonal: None,
    }
}

/// Two passes are the same pass: the same candidates in the same order, each
/// with the same score, strategy, seed and axis values, over the same rhythm
/// rotation and gesture.
fn assert_same_pass(a: &RankedSet, b: &RankedSet) {
    assert_eq!(a.ranked.len(), b.ranked.len(), "candidate count");
    for (x, y) in a.ranked.iter().zip(&b.ranked) {
        assert_eq!(x.value.score, y.value.score);
        assert_eq!(x.value.strategy, y.value.strategy);
        assert_eq!(x.value.seed, y.value.seed);
        assert_eq!(x.value.gesture, y.value.gesture);
        assert_eq!(x.aggregate().to_bits(), y.aggregate().to_bits());
        let bits = |s: &RankedSet| -> Vec<u64> {
            s.ranked
                .iter()
                .flat_map(|c| c.axes.iter().map(|a| a.value.to_bits()))
                .collect()
        };
        assert_eq!(bits(a), bits(b), "axis values");
    }
    assert_eq!(a.source_rhythms, b.source_rhythms);
    assert_eq!(a.rhythm_explicit, b.rhythm_explicit);
    assert_eq!(a.gesture, b.gesture);
}

fn pass(view: CorpusMaterialView<'_>, gesture: bool) -> RankedSet {
    ranked_candidates_from_view(&source(), view, &ask(gesture), None).expect("seeds")
}

// ── the view is the one implementation ───────────────────────────────────────

#[test]
fn the_whole_view_is_the_material_path() {
    let m = material();
    let direct = ranked_candidates(&source(), Some(&m), &ask(true), None).expect("seeds");
    assert_same_pass(&pass(CorpusMaterialView::of(&m), true), &direct);
    assert_same_pass(
        &pass(CorpusMaterialView::of_option(Some(&m)), true),
        &direct,
    );
}

#[test]
fn the_empty_view_is_the_no_corpus_path() {
    let direct = ranked_candidates(&source(), None, &ask(true), None).expect("seeds");
    assert_same_pass(&pass(CorpusMaterialView::empty(), true), &direct);
    assert_same_pass(&pass(CorpusMaterialView::of_option(None), true), &direct);
}

#[test]
fn an_explicit_palette_still_wins_over_the_views_rhythms() {
    let m = material();
    let palette = [template(&[(0, 960), (960, 960)])];
    let direct = ranked_candidates(&source(), Some(&m), &ask(true), Some(&palette)).expect("seeds");
    let viewed = ranked_candidates_from_view(
        &source(),
        CorpusMaterialView::of(&m),
        &ask(true),
        Some(&palette),
    )
    .expect("seeds");
    assert_same_pass(&viewed, &direct);
}

// ── a masked channel does not leak ───────────────────────────────────────────

#[test]
fn without_references_every_candidate_reads_fully_novel_while_rhythms_still_rotate() {
    let m = material();
    let view = CorpusMaterialView {
        references: &[],
        ..CorpusMaterialView::of(&m)
    };
    let set = pass(view, true);
    assert_eq!(set.source_rhythms, m.rhythms, "the corpus rhythms rotate");
    for candidate in &set.ranked {
        for label in NOVELTY_AXIS_LABELS {
            assert_eq!(
                candidate.axes.get(label),
                Some(1.0),
                "no reference was consulted for {label}"
            );
        }
    }
}

#[test]
fn without_rhythms_the_source_first_bar_rotates_while_the_gesture_still_carves() {
    let m = material();
    let view = CorpusMaterialView {
        rhythms: &[],
        ..CorpusMaterialView::of(&m)
    };
    let set = pass(view, true);
    let base = generation_request_from_score(&source(), 42, 4).expect("seeds");
    assert_eq!(
        set.source_rhythms, base.source_rhythms,
        "no corpus template"
    );
    assert_eq!(set.gesture, m.gesture, "the gesture channel is still open");
}

#[test]
fn without_a_gesture_nothing_is_carved_while_rhythms_still_rotate() {
    let m = material();
    let view = CorpusMaterialView {
        gesture: None,
        ..CorpusMaterialView::of(&m)
    };
    let set = pass(view, true);
    assert_eq!(set.source_rhythms, m.rhythms);
    assert_eq!(set.gesture, None);
    assert!(set.ranked.iter().all(|c| c.value.gesture.is_none()));
}

// ── what the pass actually took ──────────────────────────────────────────────

#[test]
fn a_whole_corpus_contributes_every_channel() {
    let m = material();
    let view = CorpusMaterialView::of(&m);
    assert_eq!(
        CorpusContribution::of_pass(view, &pass(view, true)),
        CorpusContribution {
            templates: 2,
            references: 2,
            gesture: true,
        }
    );
}

#[test]
fn no_corpus_contributes_nothing() {
    let view = CorpusMaterialView::empty();
    assert!(CorpusContribution::of_pass(view, &pass(view, true)).is_seed_only());
}

#[test]
fn an_attached_but_empty_corpus_contributes_nothing() {
    let empty = CorpusMaterial {
        rhythms: Vec::new(),
        references: Vec::new(),
        gesture: None,
        skipped: vec!["every record skipped".to_owned()],
    };
    let view = CorpusMaterialView::of(&empty);
    assert!(CorpusContribution::of_pass(view, &pass(view, true)).is_seed_only());
}

#[test]
fn a_gesture_the_ask_declines_is_not_contributed() {
    let m = material();
    let view = CorpusMaterialView::of(&m);
    let contribution = CorpusContribution::of_pass(view, &pass(view, false));
    assert!(!contribution.gesture, "offered, not carved");
    assert_eq!(contribution.templates, 2);
}

#[test]
fn corpus_rhythms_an_explicit_palette_overrode_are_not_contributed() {
    let m = material();
    let view = CorpusMaterialView::of(&m);
    let palette = [template(&[(0, 960), (960, 960)])];
    let set =
        ranked_candidates_from_view(&source(), view, &ask(true), Some(&palette)).expect("seeds");
    let contribution = CorpusContribution::of_pass(view, &set);
    assert_eq!(
        contribution.templates, 0,
        "the palette, not the corpus, rotated"
    );
    assert_eq!(
        contribution.references, 2,
        "novelty still measured the corpus"
    );
}
