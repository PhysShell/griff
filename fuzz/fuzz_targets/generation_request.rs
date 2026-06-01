#![no_main]

//! Fuzz target: structure-aware generation request → rule-based generator (P2, ADR-0010).
//!
//! Uses `arbitrary` to construct typed `RuleGenerationRequest` inputs so the
//! generator is exercised beyond hand-written unit tests.
//!
//! Oracle (normalised invariants):
//!   * No panic / hang / unbounded allocation (libFuzzer limits).
//!   * On `Ok(candidate)`:
//!     - `candidate.score.master_bars.len() == constraints.bar_count`.
//!     - Every note pitch is in `[pitch_lo, pitch_hi]`.
//!     - Fixed seed is deterministic: running twice yields the same voice.
//!   * On `Err(_)`: the typed error is one of the declared variants — no panic.

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

use griff_core::{
    event::{Pitch, Tempo, Ticks, TimeSignature},
    generate::{
        generate, GenerationConstraints, GenerationSeed, GenerationStrategy, PitchMaterial,
        RuleGenerationRequest,
    },
    score::AtomEvent,
};

#[derive(Debug, Arbitrary)]
struct FuzzInput {
    seed: u64,
    root: u8,
    intervals: Vec<u8>,
    bar_count: u8,
    /// Numerator; mapped to at-least-1.
    numerator: u8,
    /// Exponent for denominator: 2^(exp % 4) → 1, 2, 4, or 8.
    denom_exp: u8,
    /// Ticks per quarter; mapped to at-least-1.
    tpq: u16,
    pitch_lo: u8,
    pitch_hi: u8,
    /// Tempo offset; final BPM = 60 + offset (so always > 0).
    tempo_offset: u8,
    strategy_idx: u8,
    /// Source rhythm template: each entry is a note duration in ticks.
    source_rhythm: Vec<u16>,
}

fuzz_target!(|input: FuzzInput| {
    let strategies = [
        GenerationStrategy::RhythmCopyPitchSubstitute,
        GenerationStrategy::MotifTransposeVariation,
        GenerationStrategy::ConstrainedRandomWalk,
        GenerationStrategy::ShuffleMotifs,
        GenerationStrategy::RepeatVariation,
    ];
    let strategy = strategies[usize::from(input.strategy_idx) % strategies.len()];

    let numerator = input.numerator.max(1);
    // Denominator must be a power-of-two; 2^(denom_exp % 4) gives 1, 2, 4, or 8.
    let denom_exp = u32::from(input.denom_exp % 4);
    let denominator = 1_u8.wrapping_shl(denom_exp);

    let tpq = Ticks(u32::from(input.tpq.max(1)));
    let tempo_bpm = f64::from(input.tempo_offset).max(0.0) + 60.0;

    let Ok(time_sig) = TimeSignature::new(numerator, denominator) else {
        return;
    };
    let Ok(tempo) = Tempo::new(tempo_bpm) else {
        return;
    };

    let pitch_lo = Pitch(input.pitch_lo.min(127));
    let pitch_hi = Pitch(input.pitch_hi.min(127));

    let rhythm: Vec<Ticks> = input
        .source_rhythm
        .iter()
        .filter(|&&d| d > 0)
        .map(|&d| Ticks(u32::from(d)))
        .collect();

    let req = RuleGenerationRequest {
        seed: GenerationSeed(input.seed),
        pitch_material: PitchMaterial {
            root: Pitch(input.root.min(127)),
            intervals: input.intervals.iter().map(|&i| i % 12).collect(),
        },
        constraints: GenerationConstraints {
            bar_count: usize::from(input.bar_count),
            time_signature: time_sig,
            tempo,
            ticks_per_quarter: tpq,
            pitch_lo,
            pitch_hi,
        },
        source_rhythms: if rhythm.is_empty() {
            Vec::new()
        } else {
            vec![rhythm]
        },
        strategy,
    };

    let Ok(candidate) = generate(&req) else {
        // Typed error — no panic, oracle satisfied.
        return;
    };

    // Invariant: master-bar count matches request.
    assert_eq!(
        candidate.score.master_bars.len(),
        req.constraints.bar_count,
        "generated master-bar count must match request",
    );

    // Invariant: all notes within pitch range.
    let lo = pitch_lo.0.min(pitch_hi.0);
    let hi = pitch_lo.0.max(pitch_hi.0).min(127);
    for track in &candidate.score.tracks {
        for voice in &track.voices {
            for group in &voice.event_groups {
                for atom in &group.atoms {
                    if let AtomEvent::Note(n) = atom {
                        assert!(
                            n.pitch.0 >= lo && n.pitch.0 <= hi,
                            "note pitch {} out of [{lo}, {hi}]",
                            n.pitch.0,
                        );
                    }
                }
            }
        }
    }

    // Invariant: fixed seed is deterministic (compare the generated voice).
    let candidate2 = generate(&req).expect("second call with same request must succeed");
    assert_eq!(
        candidate.score.tracks[0].voices[0],
        candidate2.score.tracks[0].voices[0],
        "same request must produce identical voice",
    );
});
