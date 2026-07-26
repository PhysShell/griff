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
//! It times three tiers (proposal §5) — generation only; generation +
//! fingerprint; generation + full metrics — at 1k/10k/100k trials, a distinct
//! seed per trial. Absolute nanoseconds are machine-relative; the load-bearing
//! result is the ratio between tiers (generation is cheap, the metric layer
//! dominates). Adds no production code.

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

/// A stand-in for the Lab's future canonical fingerprint: a stable hash of the
/// `(onset, duration, pitch)` signature — a lower bound on real fingerprint cost.
fn fingerprint(score: &Score) -> u64 {
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

fn bench<F: FnMut(u64) -> u64>(label: &str, ns: &[usize], mut f: F) {
    for &n in ns {
        let start = Instant::now();
        let mut sink = 0u64;
        for i in 0..n {
            sink = sink.wrapping_add(f(i as u64));
        }
        let elapsed = start.elapsed();
        let per = elapsed.as_nanos() as f64 / n as f64;
        eprintln!(
            "{label:<24} N={n:>7}  total={elapsed:>12?}  per-trial={per:>9.1} ns  (sink={sink})"
        );
    }
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

    eprintln!("\n== Phase-0 cost: {bars}-bar 4/4 eighth grid, {strat:?}, release ==");
    bench("gen only", &ns, |seed| {
        generate(&request(seed, bars, strat))
            .unwrap()
            .score
            .tracks
            .len() as u64
    });
    bench("gen + fingerprint", &ns, |seed| {
        fingerprint(&generate(&request(seed, bars, strat)).unwrap().score)
    });
    bench("gen + full metrics", &ns, |seed| {
        let s = generate(&request(seed, bars, strat)).unwrap().score;
        let st = measure_structure(&s, 0).unwrap();
        let _g = measure_gesture(&s, 0).unwrap();
        let _c = measure_complexity(&s, 0).unwrap();
        let nv = measure_novelty(&s, 0, &refs).unwrap();
        st.bar_count as u64 + nv.candidate_notes as u64
    });
}
