//! S0 MIDI roundtrip baseline (canonical model).
//!
//! Pins `import_score → export_score → import_score` behavior over the shared
//! fixture corpus.
//!
//! Hard invariant (S0 acceptance): **bar alignment is preserved** — track
//! count, score-level master-bar count, per-bar time signature, and the per-bar
//! note content all survive a roundtrip unchanged, and the roundtrip reaches a
//! fixed point after the first pass.
//!
//! Known, deliberately-pinned losses of the MIDI adapter (only the first tempo
//! retained, so mid-song tempo changes collapse) are documented in the
//! `roundtrip__*` golden dumps rather than asserted away. S0 freezes behavior;
//! it does not fix it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing
)]

mod common;

use std::fmt::Write as _;

use common::{assert_golden, fixtures};
use griff_core::{
    midi::{self},
    score::{AtomEvent, Score, Track},
    slice::TickRange,
};

const INFALLIBLE: &str = "writing to a String is infallible";

/// `(pitch, velocity, duration)` of every note, grouped per master bar, per track.
type NoteShape = Vec<Vec<Vec<(u8, u8, u32)>>>;

/// Note atoms of a track's primary voice whose onset falls in `range`.
fn notes_in_range(track: &Track, range: TickRange) -> Vec<(u8, u8, u32)> {
    let Some(voice) = track.voices.first() else {
        return Vec::new();
    };
    voice
        .event_groups
        .iter()
        .flat_map(|g| &g.atoms)
        .filter_map(|a| match a {
            AtomEvent::Note(n) => Some(n),
            AtomEvent::Rest(_) => None,
        })
        .filter(|n| n.absolute_start.0 >= range.start.0 && n.absolute_start.0 < range.end.0)
        .map(|n| (n.pitch.0, n.velocity.0, n.duration.0))
        .collect()
}

fn note_shape(score: &Score) -> NoteShape {
    score
        .tracks
        .iter()
        .map(|t| {
            score
                .master_bars
                .iter()
                .map(|mb| notes_in_range(t, mb.tick_range))
                .collect()
        })
        .collect()
}

/// Score-level per-bar time signatures (one row; master bars are shared).
fn time_sigs(score: &Score) -> Vec<(u8, u8)> {
    score
        .master_bars
        .iter()
        .map(|mb| (mb.time_signature.numerator, mb.time_signature.denominator))
        .collect()
}

fn roundtrip(score: &Score) -> Score {
    let bytes = midi::export_score(score)
        .expect("export must succeed")
        .bytes;
    midi::import_score(&bytes).expect("re-import must succeed")
}

fn dump(tag: &str, score: &Score, out: &mut String) {
    writeln!(
        out,
        "[{tag}] ppqn={} master_bars={} tracks={}",
        score.ticks_per_quarter,
        score.master_bars.len(),
        score.tracks.len(),
    )
    .expect(INFALLIBLE);
    for mb in &score.master_bars {
        writeln!(
            out,
            "  bar {} {}/{} {:.1}bpm",
            mb.index,
            mb.time_signature.numerator,
            mb.time_signature.denominator,
            mb.tempo.as_f64(),
        )
        .expect(INFALLIBLE);
    }
    for (ti, t) in score.tracks.iter().enumerate() {
        writeln!(
            out,
            "  track {ti} name={} ch={}",
            t.name.as_deref().unwrap_or("<unnamed>"),
            t.channel,
        )
        .expect(INFALLIBLE);
        for mb in &score.master_bars {
            let notes = notes_in_range(t, mb.tick_range).len();
            writeln!(out, "    bar {} notes={notes}", mb.index).expect(INFALLIBLE);
        }
    }
}

#[test]
fn roundtrip_preserves_bar_alignment() {
    for (name, bytes) in fixtures() {
        let original = midi::import_score(bytes).expect("fixture must import");
        let rt1 = roundtrip(&original);
        let rt2 = roundtrip(&rt1);

        assert_eq!(
            original.ticks_per_quarter, rt1.ticks_per_quarter,
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
        let a = midi::export_score(&original).expect("export").bytes;
        let b = midi::export_score(&original).expect("export").bytes;
        assert_eq!(a, b, "{name}: export must be byte-deterministic");

        let mut snap = String::new();
        dump("orig", &original, &mut snap);
        dump("roundtrip", &rt1, &mut snap);
        assert_golden(&format!("roundtrip__{name}"), &snap);
    }
}
