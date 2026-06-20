//! Red → tests for auto-derived chord-quality tags (#75). The harmony tags
//! (`maj7`/`min7`/`sus2`/`add9`/`power_chord`) already exist in the taxonomy but
//! were curator-only; griff should read the voicings the tab already spells out.
//! References `griff_core::harmony`, which does not exist yet, so the suite fails
//! to compile until the green step.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing
)]

use griff_core::corpus::SwancoreTag;
use griff_core::event::{NoteMarks, Pitch, Ticks, Tuning, Velocity};
use griff_core::harmony::derive_harmony;
use griff_core::score::{
    AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, Score, Track, Voice,
};

/// A chord-bearing [`EventGroup`] whose notes sound `pitches` (MIDI) together.
fn chord_group(pitches: &[u8]) -> EventGroup {
    let atoms = pitches
        .iter()
        .map(|&p| {
            AtomEvent::Note(AtomNote {
                absolute_start: Ticks(0),
                duration: Ticks(480),
                pitch: Pitch(p),
                velocity: Velocity(90),
                marks: NoteMarks::empty(),
                position: None,
            })
        })
        .collect();
    EventGroup {
        kind: EventGroupKind::Chord,
        atoms,
        technique_spans: Vec::new(),
    }
}

/// A one-track, one-voice score built from `groups`.
fn score_with_groups(groups: Vec<EventGroup>) -> Score {
    Score {
        ticks_per_quarter: 480,
        master_bars: Vec::new(),
        tracks: vec![Track {
            name: Some("g".to_owned()),
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

/// A one-track score whose single chord sounds `pitches` together.
fn chord_score(pitches: &[u8]) -> Score {
    score_with_groups(vec![chord_group(pitches)])
}

#[test]
fn power_chord_fifth_dyad_derives_power_chord() {
    // E5 = E2 + B2, a bare perfect fifth — the defining swancore voicing.
    assert_eq!(
        derive_harmony(&chord_score(&[40, 47]), 0),
        vec![SwancoreTag::PowerChord]
    );
}

#[test]
fn power_chord_ignores_octave_doubling() {
    // Root + fifth + octave root still reduces to the {root, fifth} dyad.
    assert_eq!(
        derive_harmony(&chord_score(&[40, 47, 52]), 0),
        vec![SwancoreTag::PowerChord]
    );
}

#[test]
fn maj7_voicing_derives_maj7() {
    // C E G B = {0, 4, 7, 11}.
    assert_eq!(
        derive_harmony(&chord_score(&[60, 64, 67, 71]), 0),
        vec![SwancoreTag::Maj7]
    );
}

#[test]
fn min7_voicing_derives_min7() {
    // A C E G = {0, 3, 7, 10} rooted on A.
    assert_eq!(
        derive_harmony(&chord_score(&[57, 60, 64, 67]), 0),
        vec![SwancoreTag::Min7]
    );
}

#[test]
fn sus2_voicing_derives_sus2() {
    // C D G = {0, 2, 7}.
    assert_eq!(
        derive_harmony(&chord_score(&[60, 62, 67]), 0),
        vec![SwancoreTag::Sus2]
    );
}

#[test]
fn add9_voicing_derives_add9() {
    // C E G D = {0, 2, 4, 7} (the 9th folds to a 2nd by pitch class).
    assert_eq!(
        derive_harmony(&chord_score(&[60, 64, 67, 74]), 0),
        vec![SwancoreTag::Add9]
    );
}

#[test]
fn inverted_maj7_still_derives_maj7() {
    // Cmaj7 with E in the bass: the voicing is present regardless of inversion,
    // so presence-only derivation still tags Maj7. (Dedicated slash-chord
    // detection is a deliberate follow-up — see docs/decisions.log.md.)
    assert_eq!(
        derive_harmony(&chord_score(&[52, 55, 59, 60]), 0),
        vec![SwancoreTag::Maj7]
    );
}

#[test]
fn lone_note_and_plain_triad_derive_nothing() {
    // A single note is no chord…
    assert!(derive_harmony(&chord_score(&[60]), 0).is_empty());
    // …and a plain major triad has no tag in the taxonomy.
    assert!(derive_harmony(&chord_score(&[60, 64, 67]), 0).is_empty());
}

#[test]
fn presence_only_dedupes_and_is_deterministic() {
    // The same quality across groups is counted once; a pure fn of the score
    // (SPEC §6).
    let score = score_with_groups(vec![
        chord_group(&[60, 64, 67, 71]),
        chord_group(&[60, 64, 67, 71]),
    ]);
    let first = derive_harmony(&score, 0);
    assert_eq!(first, derive_harmony(&score, 0));
    assert_eq!(first, vec![SwancoreTag::Maj7]);
}

#[test]
fn output_follows_canonical_taxonomy_order() {
    // A power chord then a maj7 → output in enum order (Maj7 before PowerChord).
    let score = score_with_groups(vec![chord_group(&[40, 47]), chord_group(&[60, 64, 67, 71])]);
    assert_eq!(
        derive_harmony(&score, 0),
        vec![SwancoreTag::Maj7, SwancoreTag::PowerChord]
    );
}

#[test]
fn out_of_range_track_is_empty() {
    assert!(derive_harmony(&chord_score(&[60, 64, 67, 71]), 9).is_empty());
}

#[test]
fn derives_from_every_voice() {
    // A chord in a secondary voice is still derived (the track is measured whole).
    let mut score = chord_score(&[60]); // voice 0: a lone note
    score.tracks[0].voices.push(Voice {
        id: 1,
        event_groups: vec![chord_group(&[40, 47])],
    });
    assert_eq!(derive_harmony(&score, 0), vec![SwancoreTag::PowerChord]);
}
