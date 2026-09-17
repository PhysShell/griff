// Characterization: what `ranked_candidates` produces on main, pinned before
// the corpus-material view seam lands underneath it (generator-observatory
// design note §8.1: the public entry point must stay output-identical).
//
// Each scenario folds the whole ranked set — every candidate's strategy,
// variant seed, aggregate bits, and every atom of its score — into one FNV-1a
// value. The constants were measured on main (4f6c505). A mismatch means the
// generation path changed behaviour, not that the fixture needs updating.
#![allow(clippy::expect_used, clippy::missing_assert_message)]

use griff_core::event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity};
use griff_core::generate::{GenerationStrategy, RhythmTemplate, TemplateNote};
use griff_core::generation_input::{ranked_candidates, CorpusMaterial, GenerationAsk, RankedSet};
use griff_core::gesture::GestureControl;
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

/// FNV-1a over a byte stream — explicit, so the pin does not depend on a
/// standard-library hasher's unspecified algorithm.
struct Fnv(u64);

impl Fnv {
    const fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
    }

    fn u64(&mut self, v: u64) {
        self.bytes(&v.to_le_bytes());
    }
}

const fn strategy_index(s: GenerationStrategy) -> u64 {
    match s {
        GenerationStrategy::RhythmCopyPitchSubstitute => 0,
        GenerationStrategy::MotifTransposeVariation => 1,
        GenerationStrategy::ConstrainedRandomWalk => 2,
        GenerationStrategy::ShuffleMotifs => 3,
        GenerationStrategy::RepeatVariation => 4,
    }
}

/// Everything a caller can observe of a ranked set, folded into one value.
fn fold(set: &RankedSet) -> u64 {
    let mut h = Fnv::new();
    h.u64(set.ranked.len() as u64);
    for c in &set.ranked {
        h.u64(strategy_index(c.value.strategy));
        h.u64(c.value.seed.0);
        h.u64(c.aggregate().to_bits());
        for axis in &c.axes {
            h.u64(axis.value.to_bits());
        }
        h.u64(u64::from(c.value.gesture.is_some()));
        for bar in &c.value.score.master_bars {
            h.u64(u64::from(bar.tick_range.start.0));
            h.u64(u64::from(bar.tick_range.end.0));
        }
        for atom in c
            .value
            .score
            .tracks
            .iter()
            .flat_map(|t| &t.voices)
            .flat_map(|v| &v.event_groups)
            .flat_map(|g| &g.atoms)
        {
            match atom {
                AtomEvent::Note(n) => {
                    h.u64(1);
                    h.u64(u64::from(n.absolute_start.0));
                    h.u64(u64::from(n.duration.0));
                    h.u64(u64::from(n.pitch.0));
                    h.u64(u64::from(n.velocity.0));
                }
                AtomEvent::Rest(r) => {
                    h.u64(2);
                    h.u64(u64::from(r.absolute_start.0));
                    h.u64(u64::from(r.duration.0));
                }
            }
        }
    }
    h.u64(set.source_rhythms.len() as u64);
    for t in &set.source_rhythms {
        for n in &t.notes {
            h.u64(u64::from(n.offset.0));
            h.u64(u64::from(n.duration.0));
        }
    }
    h.u64(u64::from(set.rhythm_explicit));
    h.u64(u64::from(set.gesture.is_some()));
    h.bytes(set.policy.id.as_bytes());
    h.u64(u64::from(set.policy.version));
    h.0
}

fn pinned(material: Option<&CorpusMaterial>, gesture: bool) -> u64 {
    fold(&ranked_candidates(&source(), material, &ask(gesture), None).expect("seeds"))
}

#[test]
fn without_a_corpus_the_ranked_set_is_unchanged() {
    assert_eq!(pinned(None, true), 0xe2a2_6c00_a5e3_f05f);
}

#[test]
fn with_every_corpus_channel_the_ranked_set_is_unchanged() {
    assert_eq!(pinned(Some(&material()), true), 0x38b4_acec_3c77_e490);
}

#[test]
fn with_a_corpus_but_no_gesture_ask_the_ranked_set_is_unchanged() {
    assert_eq!(pinned(Some(&material()), false), 0x094d_5d24_6d80_59bd);
}

#[test]
fn with_an_attached_but_empty_corpus_the_ranked_set_is_unchanged() {
    let empty = CorpusMaterial {
        rhythms: Vec::new(),
        references: Vec::new(),
        gesture: None,
        skipped: Vec::new(),
    };
    assert_eq!(pinned(Some(&empty), true), pinned(None, true));
}

#[test]
fn an_explicit_palette_still_wins_over_corpus_rhythms() {
    let palette = [template(&[(0, 960), (960, 960)])];
    let set =
        ranked_candidates(&source(), Some(&material()), &ask(true), Some(&palette)).expect("seeds");
    assert_eq!(fold(&set), 0x2bcd_7f50_8041_72c3);
}
