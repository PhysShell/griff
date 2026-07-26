//! Phase-0 cost benchmark — the dev-only instrument behind §5 of
//! `docs/audit/2026-07-generator-reachability-metric-inventory.md`.
//!
//! Not a correctness test and **not a CI cost**: it is `#[ignore]`d, so
//! `cargo test --workspace` compiles it (guarding it against API drift) but
//! skips the ~14 s timing run. Reproduce the audited numbers with:
//!
//! ```text
//! cargo test --release -p griff-core --test phase0_cost -- --ignored --nocapture
//! ```
//!
//! It times the three tiers of proposal §5 — generation only; generation +
//! checksum; generation + full metrics — plus per-metric component subtiers
//! (structure / gesture / complexity / novelty) that attribute the aggregate,
//! at 1k/10k/100k trials, a distinct seed per trial. `std::hint::black_box`
//! barriers wrap every measured input and result, so the optimizer cannot elide
//! a metric whose output is otherwise unused. Absolute nanoseconds are
//! machine-relative; the load-bearing result is the ratio between tiers
//! (generation is cheap, the metric layer dominates). Adds no production code.

#![allow(
    clippy::unwrap_used,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::arithmetic_side_effects,
    clippy::missing_assert_message
)]

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::hint::black_box;
use std::time::Instant;

use griff_core::{
    event::{Pitch, Tempo, Ticks, TimeSignature},
    generate::{
        generate, GenerationConstraints, GenerationSeed, GenerationStrategy, PitchMaterial,
        RhythmTemplate, RuleGenerationRequest,
    },
    gesture::measure_gesture,
    novelty::measure_novelty,
    score::{AtomEvent, Score},
    structure::{measure_complexity, measure_structure},
};

/// The fixed workload the §5 numbers were taken over.
fn request(seed: u64, bars: usize, strategy: GenerationStrategy) -> RuleGenerationRequest {
    RuleGenerationRequest {
        seed: GenerationSeed(seed),
        pitch_material: PitchMaterial {
            root: Pitch(40),
            intervals: vec![0, 3, 5, 7, 10],
        },
        constraints: GenerationConstraints {
            bar_count: bars,
            time_signature: TimeSignature {
                numerator: 4,
                denominator: 4,
            },
            tempo: Tempo::from_bpm_integer(120).unwrap(),
            ticks_per_quarter: Ticks(480),
            pitch_lo: Pitch(36),
            pitch_hi: Pitch(72),
        },
        explicit_rhythms: None,
        source_rhythms: vec![RhythmTemplate::from_durations(&[Ticks(240); 8])], // eighth grid
        strategy,
    }
}

/// A **benchmark-only checksum** over the canonical `(onset, duration, pitch)`
/// signature — a lower bound on the cost a real fingerprint would add. Uses
/// `DefaultHasher`, whose algorithm is **not** version-stable and is not a
/// durable fingerprint; that is fine here (we time the traversal, not persist
/// the value) but must not be mistaken for the Lab's future canonical hash.
fn benchmark_checksum(score: &Score) -> u64 {
    let mut h = DefaultHasher::new();
    for track in &score.tracks {
        for voice in &track.voices {
            for group in &voice.event_groups {
                for atom in &group.atoms {
                    if let AtomEvent::Note(n) = atom {
                        (n.absolute_start.0, n.duration.0, n.pitch.0).hash(&mut h);
                    }
                }
            }
        }
    }
    h.finish()
}

/// Times one tier over **pre-built** requests, so request construction (the
/// `Vec` allocations in `request`) is never inside the measured window — the
/// tiers measure generation and metrics only.
fn time_tier<F: FnMut(&RuleGenerationRequest) -> u64>(
    label: &str,
    requests: &[RuleGenerationRequest],
    mut f: F,
) {
    let start = Instant::now();
    let mut sink = 0u64;
    for r in requests {
        sink = sink.wrapping_add(f(r));
    }
    let elapsed = start.elapsed();
    let per = elapsed.as_nanos() as f64 / requests.len() as f64;
    eprintln!(
        "{label:<24} N={:>7}  total={elapsed:>12?}  per-trial={per:>9.1} ns  (sink={sink})",
        requests.len()
    );
}

#[test]
#[ignore = "dev-only cost benchmark; run with --ignored --nocapture (see module doc)"]
fn phase0_cost() {
    let bars = 4;
    let strat = GenerationStrategy::ConstrainedRandomWalk;
    let ns = [1_000usize, 10_000, 100_000];

    // An 8-fragment reference set for the full-metrics tier's novelty term.
    let refs: Vec<Score> = (0..8)
        .map(|i| {
            generate(&request(1_000_000 + i, bars, strat))
                .unwrap()
                .score
        })
        .collect();
    assert!(!refs.is_empty()); // the harness produced material to measure

    // Report the profile actually built, so a debug run is not read as release.
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    eprintln!("\n== Phase-0 cost: {bars}-bar 4/4 eighth grid, {strat:?}, {profile} ==");

    for &n in &ns {
        // Pre-build the per-seed requests OUTSIDE every timer.
        let requests: Vec<RuleGenerationRequest> =
            (0..n as u64).map(|i| request(i, bars, strat)).collect();

        // Generation with optimizer barriers on input and output, so the pass
        // is never elided and its result is observed by every tier below.
        let gen_score =
            |r: &RuleGenerationRequest| black_box(generate(black_box(r)).unwrap().score);

        time_tier("gen only", &requests, |r| {
            black_box(gen_score(r).tracks.len() as u64)
        });
        time_tier("gen + checksum", &requests, |r| {
            black_box(benchmark_checksum(&gen_score(r)))
        });

        // Per-metric marginal cost (this subtier − "gen only") attributes the
        // aggregate to the individual measures, rather than assuming which one
        // dominates. Each result is observed so the metric cannot be elided.
        time_tier("gen + structure", &requests, |r| {
            black_box(measure_structure(&gen_score(r), 0).unwrap().bar_count as u64)
        });
        time_tier("gen + gesture", &requests, |r| {
            black_box(measure_gesture(&gen_score(r), 0).unwrap().note_count as u64)
        });
        time_tier("gen + complexity", &requests, |r| {
            black_box(
                measure_complexity(&gen_score(r), 0)
                    .unwrap()
                    .rhythmic
                    .to_bits(),
            )
        });
        time_tier("gen + novelty", &requests, |r| {
            black_box(
                measure_novelty(&gen_score(r), 0, black_box(&refs))
                    .unwrap()
                    .candidate_notes as u64,
            )
        });

        time_tier("gen + full metrics", &requests, |r| {
            let s = gen_score(r);
            let structure = measure_structure(&s, 0).unwrap();
            let gesture = measure_gesture(&s, 0).unwrap();
            let complexity = measure_complexity(&s, 0).unwrap();
            let novelty = measure_novelty(&s, 0, black_box(&refs)).unwrap();
            let measured = black_box((structure, gesture, complexity, novelty));
            measured.0.bar_count as u64 + measured.3.candidate_notes as u64
        });
    }
}
