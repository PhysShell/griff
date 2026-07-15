// S16 Phase 3 law 5 (spec §3.5): a named strategy is **selection only** —
// the first ranked candidate of that strategy from the unchanged,
// already-ranked set. `auto` is the reranked winner every frontend already
// picks. The set, the seeds, and the reranker are never touched.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::missing_assert_message,
    clippy::absolute_paths,
    clippy::arithmetic_side_effects,
    clippy::str_to_string
)]

use std::ptr;

use griff_core::event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity};
use griff_core::generate::GenerationStrategy;
use griff_core::generation_input::{ranked_candidates, select_ranked, GenerationAsk, RankedSet};
use griff_core::score::{
    AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, RepeatMarker, Score,
    Track, Voice,
};
use griff_core::slice::TickRange;

const PPQN: u16 = 480;
const BAR: u32 = 1920;

/// A minimal one-track 4/4 score sounding quarters — enough to seed
/// `ranked_candidates` (same fixture shape as the explicit-rhythm suite).
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

/// A real ranked set: every strategy contributes two variants.
fn ranked_set() -> RankedSet {
    ranked_candidates(
        &seed_score(2),
        None,
        &GenerationAsk {
            seed: 42,
            bars: 2,
            variants_per_strategy: 2,
            gesture: false,
        },
        None,
    )
    .expect("the seed score ranks")
}

/// The ranking's identity: (strategy, variant seed) in rank order.
fn ranking_fingerprint(set: &RankedSet) -> Vec<(GenerationStrategy, u64)> {
    set.ranked
        .iter()
        .map(|c| (c.value.strategy, c.value.seed.0))
        .collect()
}

const ALL_STRATEGIES: [GenerationStrategy; 5] = [
    GenerationStrategy::RhythmCopyPitchSubstitute,
    GenerationStrategy::MotifTransposeVariation,
    GenerationStrategy::ConstrainedRandomWalk,
    GenerationStrategy::ShuffleMotifs,
    GenerationStrategy::RepeatVariation,
];

#[test]
fn auto_is_the_reranked_winner() {
    let set = ranked_set();
    let selected = select_ranked(&set, None).expect("a winner exists");
    assert!(
        ptr::eq(selected, &set.ranked[0]),
        "None is today's behavior: the reranked winner across all strategies"
    );
}

#[test]
fn a_named_strategy_is_its_first_ranked_candidate_from_the_unchanged_set() {
    let set = ranked_set();
    let before = ranking_fingerprint(&set);

    let target = GenerationStrategy::RepeatVariation;
    let selected = select_ranked(&set, Some(target)).expect("the strategy contributed");
    let first_index = set
        .ranked
        .iter()
        .position(|c| c.value.strategy == target)
        .expect("present in a full set");
    assert!(
        ptr::eq(selected, &set.ranked[first_index]),
        "the FIRST ranked candidate of that strategy — not a re-ranking"
    );

    assert_eq!(
        ranking_fingerprint(&set),
        before,
        "selection only: the set, the seeds, and the order are untouched"
    );
}

#[test]
fn every_strategy_in_a_full_set_is_selectable() {
    let set = ranked_set();
    for strategy in ALL_STRATEGIES {
        let selected = select_ranked(&set, Some(strategy))
            .unwrap_or_else(|| panic!("{strategy:?} contributed to the set"));
        assert_eq!(selected.value.strategy, strategy);
    }
}

#[test]
fn an_absent_strategy_selects_nothing() {
    let mut set = ranked_set();
    set.ranked
        .retain(|c| c.value.strategy != GenerationStrategy::ShuffleMotifs);
    assert!(
        select_ranked(&set, Some(GenerationStrategy::ShuffleMotifs)).is_none(),
        "no candidate of the strategy means no selection — never a fallback"
    );
}
