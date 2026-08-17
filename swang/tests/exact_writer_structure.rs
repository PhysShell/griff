//! SWG-4A-04 contract tests: the exact-text writer's musical structure —
//! `track` → `voice` → `group` → `note` / `rest` — committed failing before
//! any implementation.
//!
//! SWG-4A-03 wrote the transport and refused everything below it. This slice
//! lifts that frontier down to the atoms, and extends the writer domain to
//! all six clauses of the census's §3 at the same time: `Pitch` and
//! `Velocity` only become reachable once atoms are written, and a slice that
//! wrote them without checking them would emit text its own builder must
//! later refuse.
//!
//! Still refused, and refused by name: `marks` beyond the empty set,
//! `position`, technique spans, `source`, `loss`, and any string needing an
//! escape. Those are SWG-4A-05. The empty forms that a *canonical* document
//! cannot leave out — `marks []`, `tuning []` — are written here, because
//! §6.2 makes them required words and omitting them would make every golden
//! in this file non-canonical text.
//!
//! Every expectation is a decision `docs/swang/exact-score-text.md` already
//! made. The ugly ones — `channel 200`, duplicate `voice.id`, `tuplet 0/0`,
//! a zero-duration note — are §3's own list of states the model permits and
//! the text must therefore spell.

// The allowances the repository's other test files take.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::missing_assert_message
)]

use griff_core::event::{
    ConfidenceBps, FretboardPosition, NoteMark, NoteMarks, NotePosition, Pitch, SpanTechnique,
    TechniqueEvidence, Tempo, Ticks, TimeSignature, Tuning, ValidationError, Velocity,
};
use griff_core::score::{
    AtomEvent, AtomNote, AtomRest, EventGroup, EventGroupKind, LossReport, MasterBar, RepeatMarker,
    Score, TechniqueSpan, Track, Voice,
};
use griff_core::slice::TickRange;
use griff_swang::exact::{write_score, ExactWriteError};

// ── fixtures ───────────────────────────────────────────────────────────────

fn range(start: u32, end: u32) -> TickRange {
    TickRange::new(Ticks(start), Ticks(end)).expect("ordered range")
}

fn bpm(n: u32) -> Tempo {
    Tempo::from_bpm_integer(n).expect("a positive integer BPM")
}

fn bar(index: usize, start: u32, end: u32) -> MasterBar {
    MasterBar {
        index,
        tick_range: range(start, end),
        time_signature: TimeSignature::new(4, 4).expect("4/4"),
        tempo: bpm(120),
        repeat: RepeatMarker::default(),
    }
}

/// A note built through the checked constructors — the ordinary case.
fn note(at: u32, duration: u32, pitch: u8, velocity: u8) -> AtomEvent {
    AtomEvent::Note(AtomNote {
        absolute_start: Ticks(at),
        duration: Ticks(duration),
        pitch: Pitch::new(pitch).expect("a MIDI pitch"),
        velocity: Velocity::new(velocity).expect("a MIDI velocity"),
        marks: NoteMarks::empty(),
        position: None,
    })
}

const fn rest(at: u32, duration: u32) -> AtomEvent {
    AtomEvent::Rest(AtomRest {
        absolute_start: Ticks(at),
        duration: Ticks(duration),
    })
}

const fn group(kind: EventGroupKind, atoms: Vec<AtomEvent>) -> EventGroup {
    EventGroup {
        kind,
        atoms,
        technique_spans: Vec::new(),
    }
}

const fn voice(id: u8, event_groups: Vec<EventGroup>) -> Voice {
    Voice { id, event_groups }
}

fn track(name: Option<&str>, channel: u8, tuning: Tuning, voices: Vec<Voice>) -> Track {
    Track {
        name: name.map(str::to_owned),
        channel,
        voices,
        tuning,
    }
}

fn tuning(pitches: &[u8]) -> Tuning {
    Tuning::new(
        pitches
            .iter()
            .map(|&p| Pitch::new(p).expect("a MIDI pitch"))
            .collect(),
    )
}

/// One master bar and whatever tracks the test cares about.
fn scored(tracks: Vec<Track>) -> Score {
    Score {
        ticks_per_quarter: 480,
        master_bars: vec![bar(0, 0, 1920)],
        tracks,
        source_meta: None,
        loss: LossReport::new(),
    }
}

/// The shortest path to a score holding exactly one atom.
fn one_atom(atom: AtomEvent) -> Score {
    scored(vec![track(
        None,
        0,
        Tuning::new(Vec::new()),
        vec![voice(0, vec![group(EventGroupKind::Single, vec![atom])])],
    )])
}

fn write(score: &Score) -> String {
    write_score(score).expect("this score is inside the writer domain")
}

/// The `note` / `rest` tags in the order they appear, so interleaving can be
/// asserted without pinning the surrounding layout.
fn atom_sequence(text: &str) -> Vec<&'static str> {
    let mut found: Vec<(usize, &'static str)> = text
        .match_indices("note {")
        .map(|(i, _)| (i, "note"))
        .chain(text.match_indices("rest {").map(|(i, _)| (i, "rest")))
        .collect();
    found.sort_by_key(|&(i, _)| i);
    found.into_iter().map(|(_, tag)| tag).collect()
}

// ── vector order is semantics, at every level ──────────────────────────────

#[test]
fn tracks_are_written_in_vector_order() {
    let score = scored(vec![
        track(Some("Zeta"), 0, Tuning::new(Vec::new()), Vec::new()),
        track(Some("Alpha"), 1, Tuning::new(Vec::new()), Vec::new()),
    ]);
    let text = write(&score);
    let zeta = text.find("Zeta").expect("the first track is written");
    let alpha = text.find("Alpha").expect("the second track is written");
    assert!(
        zeta < alpha,
        "tracks keep their vector order and are never sorted by name: {text:?}"
    );
}

#[test]
fn voices_are_written_in_vector_order_and_duplicate_ids_are_kept() {
    // §3 lists duplicate `Voice.id` within one track as model-valid. A writer
    // that deduplicated or renumbered would silently drop a voice.
    let score = scored(vec![track(
        None,
        0,
        Tuning::new(Vec::new()),
        vec![
            voice(3, vec![group(EventGroupKind::Single, vec![rest(0, 120)])]),
            voice(1, Vec::new()),
            voice(3, Vec::new()),
        ],
    )]);
    let text = write(&score);
    assert_eq!(
        text.matches("id 3").count(),
        2,
        "both voices numbered 3 are written: {text:?}"
    );
    let first = text.find("id 3").expect("voice 3");
    let middle = text.find("id 1").expect("voice 1");
    assert!(first < middle, "voices are not sorted by id: {text:?}");
}

#[test]
fn groups_are_written_in_vector_order() {
    let score = scored(vec![track(
        None,
        0,
        Tuning::new(Vec::new()),
        vec![voice(
            0,
            vec![
                group(EventGroupKind::Grace, vec![rest(0, 60)]),
                group(EventGroupKind::Chord, vec![rest(60, 60)]),
                group(EventGroupKind::Single, vec![rest(120, 60)]),
            ],
        )],
    )]);
    let text = write(&score);
    let order: Vec<usize> = ["group grace", "group chord", "group single"]
        .iter()
        .map(|needle| text.find(needle).expect("every group is written"))
        .collect();
    assert!(
        order.windows(2).all(|w| matches!(w, [a, b] if a < b)),
        "event_groups keep their vector order: {text:?}"
    );
}

// ── the track's own fields ─────────────────────────────────────────────────

#[test]
fn a_channel_beyond_the_midi_range_is_written_not_refused() {
    // §2.3: `Track.channel` carries no `ValidationError`, so 200 is
    // model-valid and must be spellable however wrong it looks.
    let score = scored(vec![track(None, 200, Tuning::new(Vec::new()), Vec::new())]);
    let text = write(&score);
    assert!(text.contains("channel 200"), "{text:?}");
}

#[test]
fn an_empty_tuning_writes_the_empty_list() {
    // §6.2: `tuning` is a scalar list value, not a structural repeated block,
    // so its empty form is `[]` and never an absent word.
    let score = scored(vec![track(None, 0, Tuning::new(Vec::new()), Vec::new())]);
    let text = write(&score);
    assert!(text.contains("tuning []"), "{text:?}");
}

#[test]
fn a_tuning_keeps_its_vector_order() {
    // Index 0 is string 1, the highest. Reversing this retunes the
    // instrument, so the writer must not sort.
    let score = scored(vec![track(
        None,
        0,
        tuning(&[64, 59, 55, 50, 45, 40]),
        Vec::new(),
    )]);
    let text = write(&score);
    assert!(text.contains("tuning [64 59 55 50 45 40]"), "{text:?}");
}

#[test]
fn an_absent_name_and_an_empty_name_do_not_collapse() {
    let absent = write(&scored(vec![track(
        None,
        0,
        Tuning::new(Vec::new()),
        Vec::new(),
    )]));
    let empty = write(&scored(vec![track(
        Some(""),
        0,
        Tuning::new(Vec::new()),
        Vec::new(),
    )]));

    assert!(
        !absent.contains("name"),
        "`None` is spelled by absence: {absent:?}"
    );
    assert!(
        empty.contains("name \"\""),
        "`Some(\"\")` is spelled as the empty string: {empty:?}"
    );
    assert_ne!(
        absent, empty,
        "the two states are distinguishable in the model and must stay so in text"
    );
}

#[test]
fn a_plain_track_name_is_written_through() {
    let score = scored(vec![track(
        Some("Guitar"),
        0,
        Tuning::new(Vec::new()),
        Vec::new(),
    )]);
    let text = write(&score);
    assert!(text.contains("name \"Guitar\""), "{text:?}");
}

#[test]
fn a_track_name_needing_an_escape_is_not_yet_written() {
    // The 4A-04/4A-05 boundary, chosen deliberately: this slice writes only
    // names that need no escape, and hands the rest to the slice that owns
    // §6.5's encoding policy. Refusing is honest; inventing half an escape
    // policy here would not be.
    for name in ["Gui\"tar", "back\\slash", "two\nlines", "bell\u{7}"] {
        let score = scored(vec![track(
            Some(name),
            0,
            Tuning::new(Vec::new()),
            Vec::new(),
        )]);
        let err = write_score(&score).expect_err("this name needs an escape");
        assert!(
            matches!(
                err,
                ExactWriteError::NotYetWritten {
                    task: "SWG-4A-05",
                    ..
                }
            ),
            "a name needing an escape names the slice that will write it, {name:?}: {err:?}"
        );
    }
}

// ── group kinds: all six, payload opened ───────────────────────────────────

#[test]
fn every_group_kind_has_its_own_spelling() {
    let kinds = [
        (EventGroupKind::Single, "group single"),
        (EventGroupKind::Chord, "group chord"),
        (EventGroupKind::Arpeggio, "group arpeggio"),
        (EventGroupKind::Strum, "group strum"),
        (
            EventGroupKind::Tuplet { num: 3, den: 2 },
            "group tuplet 3/2",
        ),
        // §3 lists `Tuplet { 0, 0 }` as model-valid. The comparator has
        // separate TupletNum and TupletDen fields, so the payload opens even
        // when it is nonsense.
        (
            EventGroupKind::Tuplet { num: 0, den: 0 },
            "group tuplet 0/0",
        ),
        (EventGroupKind::Grace, "group grace"),
    ];
    for (kind, spelling) in kinds {
        let score = scored(vec![track(
            None,
            0,
            Tuning::new(Vec::new()),
            vec![voice(0, vec![group(kind, vec![rest(0, 120)])])],
        )]);
        let text = write(&score);
        assert!(
            text.contains(spelling),
            "{kind:?} is spelled {spelling:?}: {text:?}"
        );
    }
}

#[test]
fn the_six_kinds_have_six_distinct_spellings() {
    // Guards the failure the loop above cannot see: two kinds mapped to the
    // same word would satisfy every `contains` and lose a fact.
    let kinds = [
        EventGroupKind::Single,
        EventGroupKind::Chord,
        EventGroupKind::Arpeggio,
        EventGroupKind::Strum,
        EventGroupKind::Tuplet { num: 3, den: 2 },
        EventGroupKind::Grace,
    ];
    let mut texts: Vec<String> = kinds
        .into_iter()
        .map(|kind| {
            write(&scored(vec![track(
                None,
                0,
                Tuning::new(Vec::new()),
                vec![voice(0, vec![group(kind, vec![rest(0, 120)])])],
            )]))
        })
        .collect();
    texts.sort();
    let before = texts.len();
    texts.dedup();
    assert_eq!(before, texts.len(), "each kind writes a distinct document");
}

// ── atoms ──────────────────────────────────────────────────────────────────

#[test]
fn a_rest_is_written_as_a_rest_never_as_an_absence() {
    let text = write(&one_atom(rest(240, 480)));
    assert!(text.contains("rest { at 240 duration 480 }"), "{text:?}");
}

#[test]
fn a_note_writes_all_five_required_words() {
    let text = write(&one_atom(note(0, 480, 40, 96)));
    assert!(
        text.contains("note { at 0 duration 480 pitch 40 velocity 96 marks [] }"),
        "§6.1's inline note, with `marks []` as a required word: {text:?}"
    );
}

#[test]
fn zero_duration_notes_and_rests_are_written() {
    // §3: `0` is legal for both. A writer that treated a zero duration as
    // "nothing to write" would delete an event the diff can see.
    let note_text = write(&one_atom(note(96, 0, 60, 64)));
    assert!(note_text.contains("duration 0"), "{note_text:?}");

    let rest_text = write(&one_atom(rest(96, 0)));
    assert!(rest_text.contains("rest {"), "{rest_text:?}");
    assert!(rest_text.contains("duration 0"), "{rest_text:?}");
}

#[test]
fn a_note_crossing_a_barline_stays_one_note_with_its_own_duration() {
    // S16's standing rule: the writer never synthesizes ties. `at` plus
    // `duration` is the whole story, and a barline is not an event.
    let mut score = one_atom(note(1440, 960, 60, 80));
    score.master_bars = vec![bar(0, 0, 1920), bar(1, 1920, 3840)];
    let text = write(&score);

    assert_eq!(
        text.matches("note {").count(),
        1,
        "one note in, one note out — no tie is invented: {text:?}"
    );
    assert!(text.contains("at 1440"), "{text:?}");
    assert!(text.contains("duration 960"), "{text:?}");
    assert!(
        !text.contains("tie"),
        "there is no tie in the canonical model and none in its text: {text:?}"
    );
}

#[test]
fn note_and_rest_interleaving_is_preserved_within_one_atoms_vector() {
    // §6.2 rule 3: `note` and `rest` are variant tags inside one sum-type
    // slot, not two slots. Grouping every note before every rest would look
    // like a rule-1 normalization and would rewrite the music.
    let score = scored(vec![track(
        None,
        0,
        Tuning::new(Vec::new()),
        vec![voice(
            0,
            vec![group(
                EventGroupKind::Chord,
                vec![
                    note(0, 100, 60, 80),
                    rest(100, 100),
                    note(200, 100, 62, 80),
                    rest(300, 100),
                ],
            )],
        )],
    )]);
    let text = write(&score);
    assert_eq!(
        atom_sequence(&text),
        vec!["note", "rest", "note", "rest"],
        "the atoms vector keeps its exact interleaving: {text:?}"
    );
}

// ── the writer domain, now all six clauses ─────────────────────────────────

#[test]
fn a_pitch_beyond_the_midi_range_is_outside_the_writer_domain() {
    // H5: `Pitch(pub u8)` means `Pitch(200)` compiles, so the constructor's
    // check is advisory and the writer must run it again.
    let bad = AtomEvent::Note(AtomNote {
        absolute_start: Ticks(0),
        duration: Ticks(480),
        pitch: Pitch(200),
        velocity: Velocity::new(96).expect("a MIDI velocity"),
        marks: NoteMarks::empty(),
        position: None,
    });
    let err = write_score(&one_atom(bad)).expect_err("pitch 200 violates PitchOutOfRange");
    assert!(
        matches!(
            err,
            ExactWriteError::OutsideWriterDomain {
                reason: ValidationError::PitchOutOfRange { value: 200 },
                ..
            }
        ),
        "the refusal carries the model's own error, not one this writer invented: {err:?}"
    );
}

#[test]
fn a_velocity_beyond_the_midi_range_is_outside_the_writer_domain() {
    let bad = AtomEvent::Note(AtomNote {
        absolute_start: Ticks(0),
        duration: Ticks(480),
        pitch: Pitch::new(60).expect("a MIDI pitch"),
        velocity: Velocity(200),
        marks: NoteMarks::empty(),
        position: None,
    });
    let err = write_score(&one_atom(bad)).expect_err("velocity 200 violates VelocityOutOfRange");
    assert!(
        matches!(
            err,
            ExactWriteError::OutsideWriterDomain {
                reason: ValidationError::VelocityOutOfRange { value: 200 },
                ..
            }
        ),
        "{err:?}"
    );
}

#[test]
fn a_tuning_pitch_beyond_the_midi_range_is_outside_the_writer_domain() {
    // §3's clause is "every `Pitch` <= 127", not "every note's pitch". A
    // `Tuning` is a `Vec<Pitch>` and `Tuning::new` re-checks nothing, so an
    // out-of-range open string reaches the writer exactly like `Pitch(200)`
    // on a note — and this slice writes the tuning, so it must refuse it.
    //
    // The first round of these tests missed this. It surfaced while writing
    // the domain walk against §3's wording rather than against the tests.
    let score = scored(vec![track(
        None,
        0,
        Tuning::new(vec![Pitch(64), Pitch(200)]),
        Vec::new(),
    )]);
    let err = write_score(&score).expect_err("an open string above 127");
    assert!(
        matches!(
            err,
            ExactWriteError::OutsideWriterDomain {
                reason: ValidationError::PitchOutOfRange { value: 200 },
                ..
            }
        ),
        "{err:?}"
    );
}

#[test]
fn the_domain_verdict_still_outranks_the_slice_frontier() {
    // The order 4A-03 fixed, re-checked now that both kinds of refusal can
    // originate in the same atom: "the finished writer will refuse this too"
    // is strictly more informative than "not implemented yet".
    let bad = AtomEvent::Note(AtomNote {
        absolute_start: Ticks(0),
        duration: Ticks(480),
        pitch: Pitch(200),
        velocity: Velocity::new(96).expect("a MIDI velocity"),
        marks: NoteMarks::empty(),
        // Also beyond this slice — 4A-05 owns positions.
        position: Some(NotePosition::explicit(FretboardPosition {
            string: 4,
            fret: 2,
        })),
    });
    let err = write_score(&one_atom(bad)).expect_err("both invalid and beyond the slice");
    assert!(
        matches!(err, ExactWriteError::OutsideWriterDomain { .. }),
        "the permanent verdict wins over the temporary one: {err:?}"
    );
}

// ── this slice's frontier: what 4A-05 will write ───────────────────────────

#[test]
fn a_non_empty_mark_set_is_not_yet_written() {
    // `marks []` is written here because §6.2 makes it a required word. The
    // mark vocabulary itself is 4A-05's, so a set with anything in it is
    // refused rather than guessed at.
    let marked = AtomEvent::Note(AtomNote {
        absolute_start: Ticks(0),
        duration: Ticks(480),
        pitch: Pitch::new(60).expect("a MIDI pitch"),
        velocity: Velocity::new(96).expect("a MIDI velocity"),
        marks: NoteMarks::empty().with(NoteMark::Accent),
        position: None,
    });
    let err = write_score(&one_atom(marked)).expect_err("marks are 4A-05");
    assert!(
        matches!(
            err,
            ExactWriteError::NotYetWritten {
                task: "SWG-4A-05",
                ..
            }
        ),
        "{err:?}"
    );
}

#[test]
fn a_note_position_is_not_yet_written() {
    let positioned = AtomEvent::Note(AtomNote {
        absolute_start: Ticks(0),
        duration: Ticks(480),
        pitch: Pitch::new(60).expect("a MIDI pitch"),
        velocity: Velocity::new(96).expect("a MIDI velocity"),
        marks: NoteMarks::empty(),
        position: Some(NotePosition::explicit(FretboardPosition {
            string: 4,
            fret: 2,
        })),
    });
    let err = write_score(&one_atom(positioned)).expect_err("positions are 4A-05");
    assert!(
        matches!(
            err,
            ExactWriteError::NotYetWritten {
                task: "SWG-4A-05",
                ..
            }
        ),
        "{err:?}"
    );
}

#[test]
fn a_technique_span_is_not_yet_written() {
    let mut only = group(EventGroupKind::Chord, vec![note(0, 480, 40, 96)]);
    only.technique_spans.push(TechniqueSpan {
        technique: SpanTechnique::PalmMute,
        tick_range: range(0, 480),
        evidence: TechniqueEvidence::inferred(ConfidenceBps::HALF),
    });
    let score = scored(vec![track(
        None,
        0,
        Tuning::new(Vec::new()),
        vec![voice(0, vec![only])],
    )]);
    let err = write_score(&score).expect_err("spans are 4A-05");
    assert!(
        matches!(
            err,
            ExactWriteError::NotYetWritten {
                task: "SWG-4A-05",
                ..
            }
        ),
        "{err:?}"
    );
}

// ── mutation matrix: every structural fact is observable in the bytes ──────

#[test]
fn mutating_any_single_structural_fact_changes_the_text() {
    let base = scored(vec![
        track(
            Some("A"),
            0,
            tuning(&[64, 59]),
            vec![
                voice(
                    0,
                    vec![
                        group(
                            EventGroupKind::Chord,
                            vec![note(0, 480, 40, 96), rest(480, 240)],
                        ),
                        group(EventGroupKind::Grace, vec![note(720, 60, 52, 80)]),
                    ],
                ),
                voice(1, vec![group(EventGroupKind::Single, vec![rest(0, 720)])]),
            ],
        ),
        track(Some("B"), 1, tuning(&[40]), Vec::new()),
    ]);
    let baseline = write(&base);

    let mut tracks = base.clone();
    tracks.tracks.swap(0, 1);
    assert_ne!(write(&tracks), baseline, "track order is observable");

    let mut voices = base.clone();
    voices.tracks[0].voices.swap(0, 1);
    assert_ne!(write(&voices), baseline, "voice order is observable");

    let mut groups = base.clone();
    groups.tracks[0].voices[0].event_groups.swap(0, 1);
    assert_ne!(write(&groups), baseline, "group order is observable");

    let mut atoms = base.clone();
    atoms.tracks[0].voices[0].event_groups[0].atoms.swap(0, 1);
    assert_ne!(write(&atoms), baseline, "atom order is observable");

    let mut kind = base.clone();
    kind.tracks[0].voices[0].event_groups[0].kind = EventGroupKind::Arpeggio;
    assert_ne!(write(&kind), baseline, "group kind is observable");

    let mut name = base.clone();
    name.tracks[0].name = Some("Z".to_owned());
    assert_ne!(write(&name), baseline, "track name is observable");

    let mut channel = base.clone();
    channel.tracks[0].channel = 9;
    assert_ne!(write(&channel), baseline, "channel is observable");

    let mut tuned = base.clone();
    tuned.tracks[0].tuning = tuning(&[59, 64]);
    assert_ne!(write(&tuned), baseline, "tuning order is observable");

    let mut id = base.clone();
    id.tracks[0].voices[0].id = 7;
    assert_ne!(write(&id), baseline, "voice id is observable");

    let mut at = base.clone();
    at.tracks[0].voices[0].event_groups[0].atoms[0] = note(24, 480, 40, 96);
    assert_ne!(write(&at), baseline, "absolute_start is observable");

    let mut duration = base.clone();
    duration.tracks[0].voices[0].event_groups[0].atoms[0] = note(0, 481, 40, 96);
    assert_ne!(write(&duration), baseline, "duration is observable");

    let mut pitch = base.clone();
    pitch.tracks[0].voices[0].event_groups[0].atoms[0] = note(0, 480, 41, 96);
    assert_ne!(write(&pitch), baseline, "pitch is observable");

    let mut velocity = base;
    velocity.tracks[0].voices[0].event_groups[0].atoms[0] = note(0, 480, 40, 95);
    assert_ne!(write(&velocity), baseline, "velocity is observable");
}

// ── the canonical document, byte for byte ──────────────────────────────────

/// One `Score`, one spelling — for structure this time.
///
/// The 4A-03 review settled that `contains`-style assertions leave the
/// canonical layout entirely free: every test above would still pass with
/// `voice` nested under `group`, with the fields of `track` in any order, or
/// with blank lines anywhere. This fixture is §6.1's reference document
/// restricted to what 4A-04 writes, plus a second track that pins the three
/// things a one-track golden cannot: an omitted `name`, a `channel` past the
/// MIDI range, and `tuning []`.
#[test]
fn a_structural_document_matches_its_canonical_bytes() {
    let guitar = track(
        Some("Guitar"),
        0,
        tuning(&[64, 59, 55, 50, 45, 40]),
        vec![voice(
            0,
            vec![
                group(
                    EventGroupKind::Chord,
                    vec![note(0, 480, 40, 96), note(0, 480, 47, 96)],
                ),
                group(EventGroupKind::Single, vec![rest(480, 240)]),
                group(
                    EventGroupKind::Tuplet { num: 3, den: 2 },
                    vec![note(720, 160, 52, 80)],
                ),
            ],
        )],
    );
    let nameless = track(None, 200, Tuning::new(Vec::new()), Vec::new());

    let score = Score {
        ticks_per_quarter: 960,
        master_bars: vec![bar(0, 0, 3840)],
        tracks: vec![guitar, nameless],
        source_meta: None,
        loss: LossReport::new(),
    };

    assert_eq!(
        write(&score),
        "\
swang 2

score {
    ppqn 960

    master_bar {
        index 0
        ticks 0..3840
        meter 4/4
        tempo 120/1
    }

    track {
        name \"Guitar\"
        channel 0
        tuning [64 59 55 50 45 40]

        voice {
            id 0

            group chord {
                note { at 0 duration 480 pitch 40 velocity 96 marks [] }
                note { at 0 duration 480 pitch 47 velocity 96 marks [] }
            }

            group single {
                rest { at 480 duration 240 }
            }

            group tuplet 3/2 {
                note { at 720 duration 160 pitch 52 velocity 80 marks [] }
            }
        }
    }

    track {
        channel 200
        tuning []
    }
}
"
    );
}

/// An empty `voice` and an empty `group` are structural repeated blocks with
/// no elements, so their contents vanish while the blocks themselves stay.
#[test]
fn empty_voices_and_groups_keep_their_blocks() {
    let score = scored(vec![track(
        None,
        0,
        Tuning::new(Vec::new()),
        vec![voice(0, vec![group(EventGroupKind::Single, Vec::new())])],
    )]);
    assert_eq!(
        write(&score),
        "\
swang 2

score {
    ppqn 480

    master_bar {
        index 0
        ticks 0..1920
        meter 4/4
        tempo 120/1
    }

    track {
        channel 0
        tuning []

        voice {
            id 0

            group single {
            }
        }
    }
}
"
    );
}
