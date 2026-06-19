//! Red → tests for auto-derived technique tags from notation (curation
//! automation, feature #1). Guitar Pro import already records techniques
//! explicitly (ADR-0018); curation should read them instead of re-asking a
//! human. References `griff_core::technique`, which does not exist yet, so the
//! suite fails to compile until the green step.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing
)]

use griff_core::corpus::SwancoreTag;
use griff_core::event::{
    NoteMark, NoteMarks, Pitch, SpanTechnique, TechniqueEvidence, Ticks, Tuning, Velocity,
};
use griff_core::score::{
    AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, Score, TechniqueSpan, Track, Voice,
};
use griff_core::slice::TickRange;
use griff_core::technique::{derive_techniques, merge_tags, DerivedTechniques};

/// A one-track score whose single note carries `marks` and whose group carries
/// `spans` — the minimal shape `derive_techniques` scans.
fn score_with(spans: &[SpanTechnique], marks: NoteMarks) -> Score {
    let note = AtomEvent::Note(AtomNote {
        absolute_start: Ticks(0),
        duration: Ticks(480),
        pitch: Pitch(60),
        velocity: Velocity(90),
        marks,
        position: None,
    });
    let technique_spans = spans
        .iter()
        .map(|&technique| TechniqueSpan {
            technique,
            tick_range: TickRange::new(Ticks(0), Ticks(480)).expect("ordered range"),
            evidence: TechniqueEvidence::explicit(),
        })
        .collect();
    Score {
        ticks_per_quarter: 480,
        master_bars: Vec::new(),
        tracks: vec![Track {
            name: Some("g".to_owned()),
            channel: 0,
            voices: vec![Voice {
                id: 0,
                event_groups: vec![EventGroup {
                    kind: EventGroupKind::Single,
                    atoms: vec![note],
                    technique_spans,
                }],
            }],
            tuning: Tuning::standard_e(),
        }],
        source_meta: None,
        loss: LossReport::new(),
    }
}

#[test]
fn maps_spans_and_marks_to_direct_tags_and_a_superset_name_list() {
    let marks = NoteMarks::empty()
        .with(NoteMark::HarmonicPinch)
        .with(NoteMark::Accent);
    let score = score_with(
        &[
            SpanTechnique::HammerOn,
            SpanTechnique::PalmMute,
            SpanTechnique::Legato,
        ],
        marks,
    );
    let d = derive_techniques(&score, 0);

    // Direct tags: the two spans with a dedicated tag + the pinch harmonic.
    assert!(d.tags.contains(&SwancoreTag::HammerOn));
    assert!(d.tags.contains(&SwancoreTag::PalmMute));
    assert!(d.tags.contains(&SwancoreTag::ArtificialHarmonic));
    // Legato and accent have no dedicated SwancoreTag — they must NOT be tagged…
    assert!(!d.tags.contains(&SwancoreTag::NaturalHarmonic));
    assert_eq!(d.tags.len(), 3, "only the directly-taggable techniques: {:?}", d.tags);

    // …but the free-form name list is the superset and records them anyway.
    for name in ["hammer_on", "palm_mute", "legato", "pinch_harmonic", "accent"] {
        assert!(d.names.contains(&name.to_owned()), "names missing {name}: {:?}", d.names);
    }
}

#[test]
fn let_ring_span_derives_the_let_ring_tag_and_name() {
    // let-ring is newly parsed from GP (#75): a LetRing span must auto-derive
    // both the dedicated SwancoreTag::LetRing and the "let_ring" name.
    let d = derive_techniques(&score_with(&[SpanTechnique::LetRing], NoteMarks::empty()), 0);
    assert_eq!(d.tags, vec![SwancoreTag::LetRing]);
    assert_eq!(d.names, vec!["let_ring".to_owned()]);
}

#[test]
fn presence_only_so_repeats_dedupe_and_output_is_deterministic() {
    // The same technique across groups/notes is "present", counted once.
    let score = score_with(&[SpanTechnique::Slide, SpanTechnique::Slide], NoteMarks::empty());
    let a = derive_techniques(&score, 0);
    let b = derive_techniques(&score, 0);
    assert_eq!(a, b, "pure function of the score (SPEC §6)");
    assert_eq!(a.names, vec!["slide".to_owned()]);
    assert_eq!(a.tags, vec![SwancoreTag::Slide]);
}

#[test]
fn empty_for_plain_notes_and_out_of_range_track() {
    let plain = score_with(&[], NoteMarks::empty());
    assert_eq!(derive_techniques(&plain, 0), DerivedTechniques::default());
    assert_eq!(derive_techniques(&plain, 9), DerivedTechniques::default());
}

#[test]
fn merge_tags_keeps_chosen_order_and_appends_only_new_derived() {
    let chosen = [SwancoreTag::Intro, SwancoreTag::HammerOn];
    let derived = [SwancoreTag::HammerOn, SwancoreTag::PalmMute];
    // HammerOn was already chosen → not duplicated; PalmMute is appended.
    assert_eq!(
        merge_tags(&chosen, &derived),
        vec![SwancoreTag::Intro, SwancoreTag::HammerOn, SwancoreTag::PalmMute]
    );
    // Idempotent: merging the result again changes nothing.
    let once = merge_tags(&chosen, &derived);
    assert_eq!(merge_tags(&once, &derived), once);
}

#[test]
fn derives_from_all_voices_like_structure_measures() {
    // A technique in a secondary voice (voice 1) IS derived: structure and
    // complexity (track_notes/technique_share) measure the track as a whole, so
    // technique metadata must not omit it (Codex P2 on PR #69; cf. PR #38).
    let mut score = score_with(&[], NoteMarks::empty()); // voice 0: plain note
    score.tracks[0].voices.push(Voice {
        id: 1,
        event_groups: vec![EventGroup {
            kind: EventGroupKind::Single,
            atoms: vec![AtomEvent::Note(AtomNote {
                absolute_start: Ticks(0),
                duration: Ticks(480),
                pitch: Pitch(60),
                velocity: Velocity(90),
                marks: NoteMarks::empty().with(NoteMark::HarmonicPinch),
                position: None,
            })],
            technique_spans: vec![TechniqueSpan {
                technique: SpanTechnique::PalmMute,
                tick_range: TickRange::new(Ticks(0), Ticks(480)).expect("ordered range"),
                evidence: TechniqueEvidence::explicit(),
            }],
        }],
    });
    let d = derive_techniques(&score, 0);
    assert!(d.tags.contains(&SwancoreTag::PalmMute), "secondary-voice span: {:?}", d.tags);
    assert!(
        d.tags.contains(&SwancoreTag::ArtificialHarmonic),
        "secondary-voice mark: {:?}",
        d.tags
    );
    assert!(d.names.contains(&"palm_mute".to_owned()));
    assert!(d.names.contains(&"pinch_harmonic".to_owned()));
}
