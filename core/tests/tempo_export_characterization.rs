//! Characterization of tempo and confidence scalar boundaries (S16 Phase
//! 4-pre A).
//!
//! Pins the behavior the exact-scalar migration must preserve *before* the
//! representation changes (per the stage's "characterization tests land
//! first" rule):
//!
//! - the microsecond values MIDI export writes for integer BPM tempos,
//!   including the rounding of a BPM whose microsecond form is not integral;
//! - that a tempo imported from MIDI re-exports to the identical
//!   microseconds-per-quarter value;
//! - the MIDI-inferred fretboard-position confidence baseline (the documented
//!   0.5 placeholder, i.e. half of explicit confidence).
//!
//! Only constructors may change when the scalars migrate; every assertion
//! here states a fact that must survive.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing
)]

use griff_core::{
    event::{NoteMarks, Pitch, TechniqueSource, Tempo, Ticks, TimeSignature, Tuning, Velocity},
    midi,
    score::{
        AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, RepeatMarker,
        Score, Track, Voice,
    },
    slice::TickRange,
};

/// Every tempo meta event (`FF 51 03 tt tt tt`) in an SMF byte stream, as
/// microseconds per quarter note. A raw scan is deliberate: the assertion is
/// about the bytes on the wire, not about any parser's reading of them.
fn tempo_meta_micros(bytes: &[u8]) -> Vec<u32> {
    let mut found = Vec::new();
    for window in bytes.windows(6) {
        if window[0] == 0xFF && window[1] == 0x51 && window[2] == 0x03 {
            found.push(
                (u32::from(window[3]) << 16) | (u32::from(window[4]) << 8) | u32::from(window[5]),
            );
        }
    }
    found
}

/// A minimal one-bar score at the given tempo: 4/4, one track, one voice, one
/// quarter note.
fn one_bar_score(tempo: Tempo) -> Score {
    Score {
        ticks_per_quarter: 480,
        master_bars: vec![MasterBar {
            index: 0,
            tick_range: TickRange::new(Ticks(0), Ticks(1920)).expect("ordered range"),
            time_signature: TimeSignature::new(4, 4).expect("4/4 is valid"),
            tempo,
            repeat: RepeatMarker::default(),
        }],
        tracks: vec![Track {
            name: None,
            channel: 0,
            voices: vec![Voice {
                id: 0,
                event_groups: vec![EventGroup {
                    kind: EventGroupKind::Single,
                    atoms: vec![AtomEvent::Note(AtomNote {
                        absolute_start: Ticks(0),
                        duration: Ticks(480),
                        pitch: Pitch::new(40).expect("E2 is a valid pitch"),
                        velocity: Velocity::new(96).expect("valid velocity"),
                        marks: NoteMarks::empty(),
                        position: None,
                    })],
                    technique_spans: Vec::new(),
                }],
            }],
            tuning: Tuning::standard_e(),
        }],
        source_meta: None,
        loss: LossReport::new(),
    }
}

fn export_bytes(score: &Score) -> Vec<u8> {
    midi::export_score(score).expect("export must succeed")
}

#[test]
fn integer_bpm_120_exports_exactly_500_000_micros() {
    let score = one_bar_score(Tempo::new(120.0).expect("120 BPM"));
    let micros = tempo_meta_micros(&export_bytes(&score));
    assert_eq!(
        micros,
        vec![500_000],
        "120 BPM is exactly 500 000 µs/quarter on the wire"
    );
}

#[test]
fn integer_bpm_121_exports_the_rounded_495_868_micros() {
    // 60 000 000 / 121 = 495 867.768… — not integral. The exporter rounds to
    // nearest; this pins the value the exact-scalar migration must keep
    // producing (while making the approximation a reported fact).
    let score = one_bar_score(Tempo::new(121.0).expect("121 BPM"));
    let micros = tempo_meta_micros(&export_bytes(&score));
    assert_eq!(micros, vec![495_868]);
}

#[test]
fn integer_bpm_250_exports_exactly_240_000_micros() {
    let score = one_bar_score(Tempo::new(250.0).expect("250 BPM"));
    let micros = tempo_meta_micros(&export_bytes(&score));
    assert_eq!(micros, vec![240_000]);
}

#[test]
fn midi_imported_tempo_reexports_to_identical_micros() {
    // A tempo that reached the model *from MIDI* must go back out as the very
    // microsecond value the source carried — including one with no integral
    // BPM form (470 003 µs ≈ 127.659 BPM).
    for source_micros in [500_000_u32, 495_868, 470_003, 750_000] {
        let bpm = 60_000_000.0_f64 / f64::from(source_micros);
        let score = one_bar_score(Tempo::new(bpm).expect("positive finite BPM"));
        let bytes = export_bytes(&score);
        assert_eq!(
            tempo_meta_micros(&bytes),
            vec![source_micros],
            "micros {source_micros} must survive the model round-trip"
        );

        let reimported = midi::import_score(&bytes).expect("import must succeed");
        let bytes_again = export_bytes(&reimported);
        assert_eq!(
            tempo_meta_micros(&bytes_again),
            vec![source_micros],
            "micros {source_micros} must survive import → export"
        );
    }
}

#[test]
fn midi_inferred_position_confidence_is_half_of_explicit() {
    // MIDI carries no string/fret; the importer infers positions and marks
    // them `InferredFromMidi` with the documented placeholder confidence 0.5 —
    // exactly half of explicit. The migration re-expresses this ratio in
    // basis points (5 000 of 10 000); the ratio itself is the pinned fact.
    let score = one_bar_score(Tempo::new(120.0).expect("120 BPM"));
    let imported = midi::import_score(&export_bytes(&score)).expect("import must succeed");

    let note = imported
        .tracks
        .iter()
        .flat_map(|t| &t.voices)
        .flat_map(|v| &v.event_groups)
        .flat_map(|g| &g.atoms)
        .find_map(|a| match a {
            AtomEvent::Note(n) => Some(n),
            AtomEvent::Rest(_) => None,
        })
        .expect("the imported score has a note");

    let position = note.position.expect("MIDI import infers a position");
    assert_eq!(position.evidence.source, TechniqueSource::InferredFromMidi);
    let confidence = position.evidence.confidence;
    let explicit = griff_core::event::TechniqueEvidence::explicit().confidence;
    assert!(
        (confidence * 2.0 - explicit).abs() < f64::EPSILON,
        "inferred baseline is half of explicit confidence, got {confidence}"
    );
}
