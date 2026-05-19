//! S0 MIDI roundtrip baseline.
//!
//! Pins `import → export → import` behavior over the shared fixture corpus.
//!
//! Hard invariant (S0 acceptance): **bar alignment is preserved** — track
//! count, per-track bar count, per-bar time signature, and the per-bar note
//! content all survive a roundtrip unchanged, and the roundtrip reaches a
//! fixed point after the first pass.
//!
//! Known, deliberately-pinned losses of the pre-canonical adapter (track
//! names dropped; only the first tempo retained, so mid-song tempo changes
//! collapse) are documented in the `roundtrip__*` golden dumps rather than
//! asserted away. S0 freezes behavior; it does not fix it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_assert_message
)]

mod common;

use std::fmt::Write as _;

use common::{assert_golden, fixtures};
use griff_core::{
    event::Event,
    midi::{self, MidiSong},
};

const INFALLIBLE: &str = "writing to a String is infallible";

/// `(pitch, velocity, duration)` of every note, grouped per bar, per track.
type NoteShape = Vec<Vec<Vec<(u8, u8, u32)>>>;

fn note_shape(song: &MidiSong) -> NoteShape {
    song.tracks
        .iter()
        .map(|t| {
            t.phrase
                .bars
                .iter()
                .map(|b| {
                    b.events
                        .iter()
                        .filter_map(|e| match e {
                            Event::Note(n) => Some((n.pitch.0, n.velocity.0, n.duration.0)),
                            Event::Rest(_) => None,
                        })
                        .collect()
                })
                .collect()
        })
        .collect()
}

fn time_sigs(song: &MidiSong) -> Vec<Vec<(u8, u8)>> {
    song.tracks
        .iter()
        .map(|t| {
            t.phrase
                .bars
                .iter()
                .map(|b| (b.time_signature.numerator, b.time_signature.denominator))
                .collect()
        })
        .collect()
}

fn roundtrip(song: &MidiSong) -> MidiSong {
    let bytes = midi::export(song).expect("export must succeed");
    midi::import(&bytes).expect("re-import must succeed")
}

fn dump(tag: &str, song: &MidiSong, out: &mut String) {
    writeln!(
        out,
        "[{tag}] ppqn={} tracks={}",
        song.ppqn.0,
        song.tracks.len()
    )
    .expect(INFALLIBLE);
    for (ti, t) in song.tracks.iter().enumerate() {
        writeln!(
            out,
            "  track {ti} name={} ch={} bars={}",
            t.name.as_deref().unwrap_or("<unnamed>"),
            t.channel,
            t.phrase.bars.len(),
        )
        .expect(INFALLIBLE);
        for (bi, b) in t.phrase.bars.iter().enumerate() {
            let notes = b
                .events
                .iter()
                .filter(|e| matches!(e, Event::Note(_)))
                .count();
            writeln!(
                out,
                "    bar {bi} {}/{} {:.1}bpm notes={notes}",
                b.time_signature.numerator, b.time_signature.denominator, b.tempo.0,
            )
            .expect(INFALLIBLE);
        }
    }
}

#[test]
fn roundtrip_preserves_bar_alignment() {
    for (name, bytes) in fixtures() {
        let original = midi::import(bytes).expect("fixture must import");
        let rt1 = roundtrip(&original);
        let rt2 = roundtrip(&rt1);

        assert_eq!(
            original.ppqn, rt1.ppqn,
            "{name}: PPQN must survive a roundtrip"
        );
        assert_eq!(
            original.tracks.len(),
            rt1.tracks.len(),
            "{name}: track count must survive a roundtrip"
        );
        assert_eq!(
            time_sigs(&original),
            time_sigs(&rt1),
            "{name}: per-bar time signatures (bar alignment) must survive"
        );
        assert_eq!(
            note_shape(&original),
            note_shape(&rt1),
            "{name}: per-bar note content (bar alignment) must survive"
        );

        // Fixed point: a second roundtrip changes nothing further.
        assert_eq!(
            note_shape(&rt1),
            note_shape(&rt2),
            "{name}: roundtrip must be idempotent after the first pass"
        );
        assert_eq!(
            time_sigs(&rt1),
            time_sigs(&rt2),
            "{name}: roundtrip must be idempotent after the first pass"
        );

        // Export is deterministic.
        let a = midi::export(&original).expect("export");
        let b = midi::export(&original).expect("export");
        assert_eq!(a, b, "{name}: export must be byte-deterministic");

        let mut snap = String::new();
        dump("orig", &original, &mut snap);
        dump("roundtrip", &rt1, &mut snap);
        assert_golden(&format!("roundtrip__{name}"), &snap);
    }
}
