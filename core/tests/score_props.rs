//! Property-based invariants over `normalize` (ADR-0021, ADR-0020 tier C).
//!
//! A valid-by-construction `proptest` `Score` strategy feeds `normalize`, and
//! the properties recompute their expectation *independently* of the dump:
//! canonical note order, voice order, in-range positions, half-open onsets, and
//! note preservation (nothing dropped / added / mutated). Anti-vacuity gates
//! assert each run actually exercised a same-onset chord and a multi-voice bar,
//! so the suite can never pass vacuously.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::missing_assert_message,
    clippy::cast_possible_truncation
)]

use griff_core::{
    dump::normalize,
    event::{
        FretboardPosition, NoteMarks, NotePosition, Pitch, Tempo, Ticks, TimeSignature, Tuning,
        Velocity,
    },
    score::{
        AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, Score, Track, Voice,
    },
    slice::TickRange,
};
use proptest::prelude::*;

/// Pulses per quarter note for generated scores.
const PPQN: u16 = 480;
/// Ticks per generated bar (4/4 at `PPQN`).
const BAR_TICKS: u32 = 1920;

/// One generated chord: an onset slot (×120 ticks within the bar) and its
/// `(string, fret)` notes.
type ChordSpec = (u32, Vec<(u8, u8)>);
/// One generated bar: chords for voice 0 and chords for voice 1.
type BarSpec = (Vec<ChordSpec>, Vec<ChordSpec>);

/// A note's comparison-relevant identity, used as the canonical sort key and as
/// the preservation oracle's element.
type NoteKey = (u32, Option<u8>, Option<u8>, u8);

/// Builds one positioned note; pitch is `open(string) + fret`, clamped to MIDI.
fn note_atom(onset: u32, string: u8, fret: u8, open: &[u8]) -> AtomEvent {
    let midi = open[usize::from(string.saturating_sub(1))]
        .saturating_add(fret)
        .min(127);
    AtomEvent::Note(AtomNote {
        absolute_start: Ticks(onset),
        duration: Ticks(240),
        pitch: Pitch::new(midi).unwrap(),
        velocity: Velocity::new(80).unwrap(),
        marks: NoteMarks::empty(),
        position: Some(NotePosition::explicit(FretboardPosition { string, fret })),
    })
}

/// Builds an event group from `(string, fret)` notes at one onset, deduping
/// repeated strings (a string cannot sound twice at once). `None` when empty.
fn chord_group(onset: u32, notes: &[(u8, u8)], open: &[u8]) -> Option<EventGroup> {
    let mut seen: Vec<u8> = Vec::new();
    let mut atoms: Vec<AtomEvent> = Vec::new();
    for &(string, fret) in notes {
        if (1..=6).contains(&string) && !seen.contains(&string) {
            seen.push(string);
            atoms.push(note_atom(onset, string, fret, open));
        }
    }
    if atoms.is_empty() {
        return None;
    }
    let kind = if atoms.len() == 1 {
        EventGroupKind::Single
    } else {
        EventGroupKind::Chord
    };
    Some(EventGroup {
        kind,
        atoms,
        technique_spans: Vec::new(),
    })
}

/// Assembles a valid `Score` from generated per-bar chord specs. Bar 0 always
/// carries a guaranteed two-note chord (strings 2 then 1 — the reverse of the
/// canonical order) in voice 0 and a note in voice 1, and the track's voices are
/// stored out of id order, so every case exercises both orderings.
fn build_score(bars: Vec<BarSpec>) -> Score {
    let open: Vec<u8> = Tuning::standard_e()
        .open_strings()
        .iter()
        .map(|p| p.0)
        .collect();
    let num_bars = bars.len().max(1);

    let master_bars: Vec<MasterBar> = (0..num_bars)
        .map(|i| {
            let start = u32::try_from(i).unwrap_or(0).saturating_mul(BAR_TICKS);
            MasterBar {
                index: i,
                tick_range: TickRange::new(Ticks(start), Ticks(start.saturating_add(BAR_TICKS)))
                    .unwrap(),
                time_signature: TimeSignature::new(4, 4).unwrap(),
                tempo: Tempo::new(120.0).unwrap(),
            }
        })
        .collect();

    let mut v0: Vec<EventGroup> = Vec::new();
    let mut v1: Vec<EventGroup> = Vec::new();
    for (bi, (c0, c1)) in bars.into_iter().enumerate() {
        let bar_start = u32::try_from(bi).unwrap_or(0).saturating_mul(BAR_TICKS);
        if bi == 0 {
            // Guaranteed chord + second voice so every case is non-vacuous.
            if let Some(g) = chord_group(bar_start, &[(2, 0), (1, 0)], &open) {
                v0.push(g);
            }
            if let Some(g) = chord_group(bar_start, &[(3, 2)], &open) {
                v1.push(g);
            }
        }
        for (slot, notes) in c0 {
            let onset = bar_start.saturating_add(slot.min(15).saturating_mul(120));
            if let Some(g) = chord_group(onset, &notes, &open) {
                v0.push(g);
            }
        }
        for (slot, notes) in c1 {
            let onset = bar_start.saturating_add(slot.min(15).saturating_mul(120));
            if let Some(g) = chord_group(onset, &notes, &open) {
                v1.push(g);
            }
        }
    }

    Score {
        ticks_per_quarter: PPQN,
        master_bars,
        // Deliberately out of id order: the dump must canonicalise it.
        tracks: vec![Track {
            name: None,
            channel: 0,
            voices: vec![
                Voice {
                    id: 1,
                    event_groups: v1,
                },
                Voice {
                    id: 0,
                    event_groups: v0,
                },
            ],
            tuning: Tuning::standard_e(),
        }],
        source_meta: None,
        loss: LossReport::new(),
    }
}

/// A `proptest` strategy producing valid, varied `Score`s.
fn arb_score() -> impl Strategy<Value = Score> {
    let chord = (0u32..16, prop::collection::vec((1u8..=6, 0u8..=12), 1..=4));
    let voice_bar = prop::collection::vec(chord, 0..=4);
    let bar = (voice_bar.clone(), voice_bar);
    prop::collection::vec(bar, 1..=3).prop_map(build_score)
}

/// Independent oracle: every `(onset, string, fret, pitch)` note in the input,
/// sorted, regardless of grouping or voice/bar bucketing.
fn input_note_keys(score: &Score) -> Vec<NoteKey> {
    let mut keys: Vec<NoteKey> = score
        .tracks
        .iter()
        .flat_map(|t| t.voices.iter())
        .flat_map(|v| v.event_groups.iter())
        .flat_map(|g| g.atoms.iter())
        .filter_map(|atom| match atom {
            AtomEvent::Note(n) => {
                let (string, fret) = n.position.map_or((None, None), |p| {
                    (Some(p.position.string), Some(p.position.fret))
                });
                Some((n.absolute_start.0, string, fret, n.pitch.0))
            }
            AtomEvent::Rest(_) => None,
        })
        .collect();
    keys.sort_unstable();
    keys
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// `normalize` orders, places, and preserves notes per ADR-0020 / ADR-0021.
    #[test]
    fn normalize_holds_canonical_invariants(score in arb_score()) {
        let norm = normalize(&score);

        let mut saw_chord = false;
        let mut saw_multivoice_bar = false;
        let mut dump_keys: Vec<NoteKey> = Vec::new();

        for track in &norm.tracks {
            let tuning_len = u8::try_from(track.tuning.len()).unwrap_or(u8::MAX);
            for bar in &track.bars {
                prop_assert!(bar.start_tick <= bar.end_tick, "bar range must be half-open");

                // Voices ordered by id (independent oracle: a sorted copy).
                let ids: Vec<u8> = bar.voices.iter().map(|v| v.id).collect();
                let mut ids_sorted = ids.clone();
                ids_sorted.sort_unstable();
                prop_assert_eq!(&ids, &ids_sorted, "voices must be id-ordered");
                if bar.voices.len() >= 2 {
                    saw_multivoice_bar = true;
                }

                for voice in &bar.voices {
                    // Notes ordered by the canonical key (independent oracle).
                    let keys: Vec<NoteKey> = voice
                        .notes
                        .iter()
                        .map(|n| (n.onset_tick, n.string, n.fret, n.pitch))
                        .collect();
                    let mut keys_sorted = keys.clone();
                    keys_sorted.sort_unstable();
                    prop_assert_eq!(
                        &keys,
                        &keys_sorted,
                        "notes must be (onset, string, fret, pitch)-ordered"
                    );

                    if voice
                        .notes
                        .windows(2)
                        .any(|w| w[0].onset_tick == w[1].onset_tick)
                    {
                        saw_chord = true;
                    }

                    for n in &voice.notes {
                        prop_assert!(n.pitch <= 127, "pitch in MIDI range");
                        prop_assert!(
                            n.onset_tick >= bar.start_tick && n.onset_tick < bar.end_tick,
                            "onset must fall in the bar's half-open range"
                        );
                        if let Some(string) = n.string {
                            prop_assert!(
                                (1..=tuning_len).contains(&string),
                                "string must lie within the track tuning"
                            );
                            prop_assert!(n.fret.is_some(), "a positioned note keeps its fret");
                        }
                        dump_keys.push((n.onset_tick, n.string, n.fret, n.pitch));
                    }
                }
            }
        }

        // Preservation oracle: the dump's notes are exactly the input's notes.
        dump_keys.sort_unstable();
        prop_assert_eq!(
            dump_keys,
            input_note_keys(&score),
            "normalize must neither drop, add, nor mutate a note"
        );

        // Anti-vacuity: the run must have exercised the shapes under test.
        prop_assert!(saw_chord, "generator must produce a same-onset chord");
        prop_assert!(saw_multivoice_bar, "generator must produce a multi-voice bar");
    }
}
