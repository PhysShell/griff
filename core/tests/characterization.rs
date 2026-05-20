//! S0 characterization of the pre-canonical library pipeline.
//!
//! Pins the observable behavior of `midi` import/summarise, `feature`,
//! `slice`, `classify`, and deterministic `generate` over the shared fixture
//! corpus. These golden dumps describe what the code *does* today. Re-bless
//! deliberately: `GRIFF_BLESS=1 cargo test -p griff-core`.

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
    classify::{bar_features, classify_bar},
    event::{Pitch, Tempo, Ticks, TimeSignature, Velocity},
    feature::phrase_features,
    generate::{generate_repeating_phrase, GeneratePhraseRequest, RepeatingPattern},
    midi::{self, MidiSong},
    slice::timed_phrase_events,
};

const INFALLIBLE: &str = "writing to a String is infallible";

fn dump_bars(song: &MidiSong, out: &mut String) {
    for (ti, track) in song.tracks.iter().enumerate() {
        let name = track.name.as_deref().unwrap_or("<unnamed>");
        writeln!(
            out,
            "track {ti} name={name} ch={} bars={}",
            track.channel,
            track.phrase.bars.len()
        )
        .expect(INFALLIBLE);
        for (bi, bar) in track.phrase.bars.iter().enumerate() {
            let f = bar_features(bar);
            let c = classify_bar(f);
            writeln!(
                out,
                "  bar {bi} {}/{} {:.1}bpm notes={} class={c} vel={} span={}",
                bar.time_signature.numerator,
                bar.time_signature.denominator,
                bar.tempo.0,
                f.note_count,
                f.avg_velocity,
                f.pitch_span,
            )
            .expect(INFALLIBLE);
        }
    }
}

fn dump_phrase_analysis(song: &MidiSong, out: &mut String) {
    for (ti, track) in song.tracks.iter().enumerate() {
        let pf = phrase_features(&track.phrase).expect("phrase features");
        let pitch = pf.pitch_range.map_or_else(
            || "none".to_owned(),
            |r| format!("{}..{}", r.lowest.0, r.highest.0),
        );
        let vel = pf.velocity_range.map_or_else(
            || "none".to_owned(),
            |r| format!("{}..{}", r.lowest.0, r.highest.0),
        );
        writeln!(
            out,
            "features[{ti}] bars={} events={} notes={} rests={} artic={} dur={} pitch={pitch} vel={vel}",
            pf.bar_count,
            pf.event_count,
            pf.note_count,
            pf.rest_count,
            pf.articulated_note_count,
            pf.total_duration.0,
        )
        .expect(INFALLIBLE);
        let timed = timed_phrase_events(&track.phrase).expect("timed events");
        let first = timed.first().map_or(0, |e| e.absolute_start.0);
        let last = timed.last().map_or(0, |e| e.absolute_start.0);
        writeln!(
            out,
            "timed[{ti}] count={} first@{first} last@{last}",
            timed.len()
        )
        .expect(INFALLIBLE);
    }
}

#[test]
fn library_pipeline_golden() {
    for (name, bytes) in fixtures() {
        let song = midi::import(bytes).expect("fixture must import");
        let summary = midi::summarise(&song);

        let mut out = String::new();
        writeln!(out, "== {name} ==").expect(INFALLIBLE);
        writeln!(out, "ppqn {} tracks {}", summary.ppqn, summary.tracks.len()).expect(INFALLIBLE);
        for t in &summary.tracks {
            writeln!(
                out,
                "summary idx={} name={} ch={} bars={} notes={}",
                t.index,
                t.name.as_deref().unwrap_or("<unnamed>"),
                t.channel,
                t.bar_count,
                t.note_count,
            )
            .expect(INFALLIBLE);
        }
        dump_bars(&song, &mut out);
        dump_phrase_analysis(&song, &mut out);

        assert_golden(&format!("characterize__{name}"), &out);
    }
}

/// S0 hard rule: generation is deterministic under a fixed request.
#[test]
fn generate_is_deterministic_golden() {
    let request = GeneratePhraseRequest {
        bar_count: 3,
        time_signature: TimeSignature::new(7, 8).unwrap(),
        tempo: Tempo::new(160.0).unwrap(),
        ticks_per_quarter: Ticks(480),
        pattern: RepeatingPattern {
            pitches: vec![Pitch(40), Pitch(43), Pitch(45), Pitch(47), Pitch(50)],
            step: Ticks(240),
            velocity: Velocity(104),
            articulation: None,
        },
    };

    let a = generate_repeating_phrase(&request).expect("generation must succeed");
    let b = generate_repeating_phrase(&request).expect("generation must succeed");
    assert_eq!(
        a, b,
        "generation must be deterministic for an identical request"
    );

    let mut out = String::new();
    writeln!(out, "bars={}", a.bars.len()).expect(INFALLIBLE);
    for (bi, bar) in a.bars.iter().enumerate() {
        writeln!(
            out,
            "bar {bi} {}/{} events={}",
            bar.time_signature.numerator,
            bar.time_signature.denominator,
            bar.events.len()
        )
        .expect(INFALLIBLE);
        for ev in &bar.events {
            writeln!(out, "  {ev:?}").expect(INFALLIBLE);
        }
    }
    assert_golden("generate__deterministic_7_8", &out);
}
