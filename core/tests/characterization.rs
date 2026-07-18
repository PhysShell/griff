//! S0 characterization of the canonical library pipeline.
//!
//! Pins the observable behavior of `midi::import_score`, `feature::voice_features`,
//! and the deterministic canonical `generate()` over the shared fixture corpus.
//! These golden dumps describe what the canonical pipeline *does* today.
//! Re-bless deliberately: `GRIFF_BLESS=1 cargo test -p griff-core`.
//!
//! `classify` and `slice` are intentionally not exercised here: they still
//! operate on the legacy model (ADR-0011 defers their canonical port) and stay
//! covered by their own unit tests and the CLI suite.

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
    event::{Pitch, Tempo, Ticks, TimeSignature},
    feature::voice_features,
    generate::{
        generate, GenerationConstraints, GenerationSeed, GenerationStrategy, PitchMaterial,
        RhythmTemplate, RuleGenerationRequest,
    },
    midi::import_score,
    score::{AtomEvent, Score, Voice},
};

const INFALLIBLE: &str = "writing to a String is infallible";

fn dump_voice_features(voice: &Voice, ti: usize, vi: usize, out: &mut String) {
    let vf = voice_features(voice).expect("voice features");
    let pitch = vf.pitch_range.map_or_else(
        || "none".to_owned(),
        |r| format!("{}..{}", r.lowest.0, r.highest.0),
    );
    let vel = vf.velocity_range.map_or_else(
        || "none".to_owned(),
        |r| format!("{}..{}", r.lowest.0, r.highest.0),
    );
    writeln!(
        out,
        "  voice[{ti}.{vi}] id={} groups={} events={} notes={} rests={} artic={} dur={} pitch={pitch} vel={vel}",
        voice.id,
        voice.event_groups.len(),
        vf.event_count,
        vf.note_count,
        vf.rest_count,
        vf.articulated_note_count,
        vf.total_duration.0,
    )
    .expect(INFALLIBLE);
}

fn dump_score(score: &Score, out: &mut String) {
    writeln!(
        out,
        "ppqn {} master_bars {} tracks {}",
        score.ticks_per_quarter,
        score.master_bars.len(),
        score.tracks.len(),
    )
    .expect(INFALLIBLE);
    for mb in &score.master_bars {
        writeln!(
            out,
            "bar {} {}/{} {:.1}bpm [{}, {})",
            mb.index,
            mb.time_signature.numerator,
            mb.time_signature.denominator,
            mb.tempo.as_f64(),
            mb.tick_range.start.0,
            mb.tick_range.end.0,
        )
        .expect(INFALLIBLE);
    }
    for (ti, track) in score.tracks.iter().enumerate() {
        let name = track.name.as_deref().unwrap_or("<unnamed>");
        writeln!(
            out,
            "track {ti} name={name} ch={} voices={}",
            track.channel,
            track.voices.len(),
        )
        .expect(INFALLIBLE);
        for (vi, voice) in track.voices.iter().enumerate() {
            dump_voice_features(voice, ti, vi, out);
        }
    }
}

#[test]
fn library_pipeline_golden() {
    for (name, bytes) in fixtures() {
        let score = import_score(bytes).expect("fixture must import as Score");

        let mut out = String::new();
        writeln!(out, "== {name} ==").expect(INFALLIBLE);
        dump_score(&score, &mut out);

        assert_golden(&format!("characterize__{name}"), &out);
    }
}

/// S0 hard rule: generation is deterministic under a fixed request.
#[test]
fn generate_is_deterministic_golden() {
    let request = RuleGenerationRequest {
        seed: GenerationSeed(2024),
        pitch_material: PitchMaterial {
            root: Pitch(40),
            intervals: vec![0, 3, 5, 7, 10],
        },
        constraints: GenerationConstraints {
            bar_count: 3,
            time_signature: TimeSignature::new(7, 8).unwrap(),
            tempo: Tempo::from_bpm_integer(160).unwrap(),
            ticks_per_quarter: Ticks(480),
            pitch_lo: Pitch(36),
            pitch_hi: Pitch(72),
        },
        explicit_rhythms: None,
        source_rhythms: vec![RhythmTemplate::from_durations(&[Ticks(240); 8])],
        strategy: GenerationStrategy::ConstrainedRandomWalk,
    };

    let a = generate(&request).expect("generation must succeed");
    let b = generate(&request).expect("generation must succeed");
    let voice_a = &a.score.tracks[0].voices[0];
    let voice_b = &b.score.tracks[0].voices[0];
    assert_eq!(
        voice_a, voice_b,
        "generation must be deterministic for an identical request"
    );

    let mut out = String::new();
    writeln!(
        out,
        "ppqn {} master_bars {}",
        a.score.ticks_per_quarter,
        a.score.master_bars.len(),
    )
    .expect(INFALLIBLE);
    for (gi, group) in voice_a.event_groups.iter().enumerate() {
        for atom in &group.atoms {
            match atom {
                AtomEvent::Note(n) => writeln!(
                    out,
                    "  g{gi} note @{} dur={} pitch={} vel={}",
                    n.absolute_start.0, n.duration.0, n.pitch.0, n.velocity.0,
                )
                .expect(INFALLIBLE),
                AtomEvent::Rest(r) => writeln!(
                    out,
                    "  g{gi} rest @{} dur={}",
                    r.absolute_start.0, r.duration.0,
                )
                .expect(INFALLIBLE),
            }
        }
    }
    assert_golden("generate__deterministic_7_8", &out);
}
