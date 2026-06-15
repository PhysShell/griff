//! Property-based invariants over `normalize` (ADR-0021, ADR-0020 tier C).
//!
//! A valid-by-construction `proptest` `Score` strategy feeds `normalize`, and
//! the properties recompute their expectation *independently* of the dump:
//! canonical note order, voice order, in-range positions, half-open onsets, and
//! note preservation (nothing dropped / added / mutated). Anti-vacuity gates
//! assert each run exercised the shapes under test — a same-onset chord, a
//! three-voice bar, an unpositioned (no-fretboard) note, a non-4/4 bar, and a
//! note exactly on a bar boundary — so the suite can never pass vacuously.
//!
//! Default `cases` is 3000 (sub-second). For a deeper sweep, raise it at runtime
//! — e.g. `PROPTEST_CASES=15000 cargo test -p griff-core --test score_props` —
//! without touching the committed default; any counterexample proptest finds is
//! persisted under `proptest-regressions/` and replayed on every later run.

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

/// A generated note: either a fretboard position or an unpositioned MIDI pitch.
#[derive(Debug, Clone)]
enum NoteSpec {
    /// `(string 1..=6, fret)` — a positioned note.
    Fretted(u8, u8),
    /// A raw MIDI pitch with no fretboard position (`position == None`).
    Open(u8),
}

/// One generated group: an onset slot (0..16, scaled into the bar) and its notes.
type GroupSpec = (u32, Vec<NoteSpec>);
/// One voice's groups within one bar.
type VoiceBar = Vec<GroupSpec>;
/// One bar: a `(numerator, denominator)` meter and content for three voices.
type BarSpec = ((u8, u8), (VoiceBar, VoiceBar, VoiceBar));

/// A note's comparison-relevant identity: the canonical sort key and the
/// preservation oracle's element.
type NoteKey = (u32, Option<u8>, Option<u8>, u8);
/// One laid-out bar: `(start_tick, length, numerator, denominator)`.
type BarLayout = (u32, u32, u8, u8);

/// Ticks spanned by a `numerator/denominator` bar at [`PPQN`].
fn bar_ticks(numerator: u8, denominator: u8) -> u32 {
    u32::from(PPQN)
        .saturating_mul(4)
        .checked_div(u32::from(denominator))
        .unwrap_or(0)
        .saturating_mul(u32::from(numerator))
        .max(1)
}

/// Builds a positioned note; pitch is `open(string) + fret`, clamped to MIDI.
fn fretted_atom(onset: u32, string: u8, fret: u8, open: &[u8]) -> AtomEvent {
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

/// Builds an unpositioned note carrying a raw MIDI pitch (`position == None`).
fn open_atom(onset: u32, pitch: u8) -> AtomEvent {
    AtomEvent::Note(AtomNote {
        absolute_start: Ticks(onset),
        duration: Ticks(240),
        pitch: Pitch::new(pitch.min(127)).unwrap(),
        velocity: Velocity::new(80).unwrap(),
        marks: NoteMarks::empty(),
        position: None,
    })
}

/// Assembles an event group at one onset, deduping repeated strings (a string
/// cannot sound twice at once); unpositioned notes pass through. `None` if empty.
fn build_group(onset: u32, notes: &[NoteSpec], open: &[u8]) -> Option<EventGroup> {
    let mut seen_strings: Vec<u8> = Vec::new();
    let mut atoms: Vec<AtomEvent> = Vec::new();
    for note in notes {
        match *note {
            NoteSpec::Fretted(string, fret) => {
                if (1..=6).contains(&string) && !seen_strings.contains(&string) {
                    seen_strings.push(string);
                    atoms.push(fretted_atom(onset, string, fret, open));
                }
            }
            NoteSpec::Open(pitch) => atoms.push(open_atom(onset, pitch)),
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

/// Appends `notes` as one group, placing it at `start + scaled(slot)` so the
/// onset stays inside the bar `(start, len)`'s half-open range.
fn push_group(
    groups: &mut Vec<EventGroup>,
    bar: (u32, u32),
    slot: u32,
    notes: &[NoteSpec],
    open: &[u8],
) {
    let (start, len) = bar;
    let offset = slot
        .min(15)
        .saturating_mul(len)
        .checked_div(16)
        .unwrap_or(0);
    if let Some(group) = build_group(start.saturating_add(offset), notes, open) {
        groups.push(group);
    }
}

/// Lays bars out contiguously: `(start, len, numerator, denominator)` per bar.
fn bar_layout(meters: &[(u8, u8)]) -> Vec<BarLayout> {
    let mut layout: Vec<BarLayout> = Vec::new();
    let mut cursor = 0_u32;
    for &(num, den) in meters {
        let len = bar_ticks(num, den);
        layout.push((cursor, len, num, den));
        cursor = cursor.saturating_add(len);
    }
    layout
}

/// Fills three voices from the generated per-bar specs, plus the guaranteed
/// non-vacuous content (reverse-order chord, unpositioned note, three voices in
/// bar 0, and a note on the bar 0 / bar 1 boundary).
fn fill_voices(bars: Vec<BarSpec>, layout: &[BarLayout], open: &[u8]) -> [Vec<EventGroup>; 3] {
    let mut voices: [Vec<EventGroup>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for (bi, (_, (c0, c1, c2))) in bars.into_iter().enumerate() {
        let (start, len, _, _) = layout[bi];
        let bar = (start, len);
        if bi == 0 {
            let chord = [NoteSpec::Fretted(2, 0), NoteSpec::Fretted(1, 0)];
            push_group(&mut voices[0], bar, 0, &chord, open);
            push_group(&mut voices[0], bar, 4, &[NoteSpec::Open(60)], open);
            push_group(&mut voices[1], bar, 0, &[NoteSpec::Fretted(3, 2)], open);
            push_group(&mut voices[2], bar, 0, &[NoteSpec::Fretted(4, 5)], open);
        }
        if bi == 1 {
            push_group(&mut voices[0], bar, 0, &[NoteSpec::Open(55)], open);
        }
        for (slot, notes) in c0 {
            push_group(&mut voices[0], bar, slot, &notes, open);
        }
        for (slot, notes) in c1 {
            push_group(&mut voices[1], bar, slot, &notes, open);
        }
        for (slot, notes) in c2 {
            push_group(&mut voices[2], bar, slot, &notes, open);
        }
    }
    voices
}

/// Assembles a valid `Score` from generated per-bar specs. Every case carries
/// guaranteed content so the property is never vacuous: a reverse-order chord
/// and an unpositioned note in voice 0, three voices in bar 0, a non-4/4 bar,
/// and a note exactly on the bar 0 / bar 1 boundary. Voices are stored out of id
/// order, so the dump must canonicalise both note and voice ordering.
fn build_score(bars: Vec<BarSpec>) -> Score {
    let open: Vec<u8> = Tuning::standard_e()
        .open_strings()
        .iter()
        .map(|p| p.0)
        .collect();

    // Meters, with at least one non-4/4 bar guaranteed.
    let mut meters: Vec<(u8, u8)> = bars.iter().map(|(meter, _)| *meter).collect();
    if meters.iter().all(|&(num, den)| num == 4 && den == 4) {
        if let Some(first) = meters.first_mut() {
            *first = (7, 8);
        }
    }

    let layout = bar_layout(&meters);
    let master_bars: Vec<MasterBar> = layout
        .iter()
        .enumerate()
        .map(|(i, &(start, len, num, den))| MasterBar {
            index: i,
            tick_range: TickRange::new(Ticks(start), Ticks(start.saturating_add(len))).unwrap(),
            time_signature: TimeSignature::new(num, den).unwrap(),
            tempo: Tempo::new(120.0).unwrap(),
        })
        .collect();

    let [v0, v1, v2] = fill_voices(bars, &layout, &open);
    Score {
        ticks_per_quarter: PPQN,
        master_bars,
        // Voices deliberately out of id order: the dump must canonicalise it.
        tracks: vec![Track {
            name: None,
            channel: 0,
            voices: vec![
                Voice {
                    id: 2,
                    event_groups: v2,
                },
                Voice {
                    id: 0,
                    event_groups: v0,
                },
                Voice {
                    id: 1,
                    event_groups: v1,
                },
            ],
            tuning: Tuning::standard_e(),
        }],
        source_meta: None,
        loss: LossReport::new(),
    }
}

/// A `proptest` strategy producing valid, varied multi-bar, multi-voice `Score`s.
fn arb_score() -> impl Strategy<Value = Score> {
    let note = prop_oneof![
        (1u8..=6, 0u8..=12).prop_map(|(string, fret)| NoteSpec::Fretted(string, fret)),
        (40u8..=88).prop_map(NoteSpec::Open),
    ];
    let group = (0u32..16, prop::collection::vec(note, 1..=4));
    let voice_bar = prop::collection::vec(group, 0..=3);
    let meter = (
        1u8..=9,
        prop_oneof![Just(2u8), Just(4u8), Just(8u8), Just(16u8)],
    );
    let bar = (meter, (voice_bar.clone(), voice_bar.clone(), voice_bar));
    // At least two bars so a bar boundary always exists.
    prop::collection::vec(bar, 2..=4).prop_map(build_score)
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
    #![proptest_config(ProptestConfig::with_cases(3000))]

    /// `normalize` orders, places, and preserves notes per ADR-0020 / ADR-0021.
    #[test]
    fn normalize_holds_canonical_invariants(score in arb_score()) {
        let norm = normalize(&score);

        let mut saw_chord = false;
        let mut saw_three_voices = false;
        let mut saw_unpositioned = false;
        let mut saw_nonstandard_meter = false;
        let mut saw_boundary = false;
        let mut dump_keys: Vec<NoteKey> = Vec::new();

        for track in &norm.tracks {
            let tuning_len = u8::try_from(track.tuning.len()).expect("tuning fits in u8");
            for bar in &track.bars {
                prop_assert!(bar.start_tick <= bar.end_tick, "bar range must be half-open");
                if bar.time_sig != [4, 4] {
                    saw_nonstandard_meter = true;
                }

                // Voices ordered by id (independent oracle: a sorted copy).
                let ids: Vec<u8> = bar.voices.iter().map(|v| v.id).collect();
                let mut ids_sorted = ids.clone();
                ids_sorted.sort_unstable();
                prop_assert_eq!(&ids, &ids_sorted, "voices must be id-ordered");
                if bar.voices.len() >= 3 {
                    saw_three_voices = true;
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
                        prop_assert_eq!(
                            n.string.is_some(),
                            n.fret.is_some(),
                            "string and fret must both be present or both absent"
                        );
                        if let Some(string) = n.string {
                            prop_assert!(
                                (1..=tuning_len).contains(&string),
                                "string must lie within the track tuning"
                            );
                        } else {
                            saw_unpositioned = true;
                        }
                        if bar.index > 0 && n.onset_tick == bar.start_tick {
                            saw_boundary = true;
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

        // Anti-vacuity: the run must have exercised every shape under test.
        prop_assert!(saw_chord, "generator must produce a same-onset chord");
        prop_assert!(saw_three_voices, "generator must produce a three-voice bar");
        prop_assert!(saw_unpositioned, "generator must produce an unpositioned note");
        prop_assert!(saw_nonstandard_meter, "generator must produce a non-4/4 bar");
        prop_assert!(saw_boundary, "generator must produce a note on a bar boundary");
    }
}
