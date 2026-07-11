// TDD red phase: candidate set + explainable rerank — the wiring the
// melodic-closure research note left open (§7.2 "wired into S6 / S14
// candidate reranking", §7.3 caller-side novelty cut; ADR-0017).
//
// S6 `generate` produces exactly one candidate from one hardcoded strategy;
// the S6 stage doc promises `Vec<GenerationCandidate>` and the ADR-0017
// vocabulary exists precisely to rank such a set. This suite specifies the
// missing seam as two pure functions in `griff_core::rerank`:
//
// - `generate_candidate_set` — fan a base request out over every S6 strategy
//   × a caller-chosen number of seed variants (deterministically derived from
//   the base seed; independent of whether gesture carving is on), skipping
//   `RhythmCopyPitchSubstitute` when no rhythm template exists, rotating
//   templates across variants so a multi-template corpus actually shows up in
//   the set, and optionally carving each candidate through the S6 gesture
//   compiler (`generate_gestured`, research note §3.5).
// - `rerank_candidates` — score each candidate on the four closure axes plus
//   the two novelty axes (against caller-supplied reference scores) under a
//   caller-supplied `WeightPolicy`, and return `Scored` envelopes in rank
//   order (aggregate descending, ties by candidate index — `rank_indices`).
//
// References `griff_core::rerank::{…}`, which does not exist yet, so the
// suite fails to compile until the green step.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::missing_assert_message,
    clippy::float_cmp,
    clippy::arithmetic_side_effects
)]

use std::slice;

use griff_core::{
    closure::CLOSURE_AXIS_LABELS,
    event::{Pitch, Tempo, Ticks, TimeSignature},
    generate::{GenerationConstraints, GenerationSeed, GenerationStrategy, PitchMaterial},
    gesture::GestureControl,
    novelty::NOVELTY_AXIS_LABELS,
    rerank::{
        generate_candidate_set, rerank_candidates, rerank_weights_v1, SetCandidate, SetError,
        SetRequest, RERANK_AXIS_LABELS,
    },
    score::{AtomEvent, Score},
    scoring::WeightPolicy,
};

// ── helpers ───────────────────────────────────────────────────────────────────

/// E minor pentatonic from E2 — the swancore-ish default material used across
/// the generator suites.
fn material() -> PitchMaterial {
    PitchMaterial {
        root: Pitch(40), // E2
        intervals: vec![0, 3, 5, 7, 10],
    }
}

/// 4/4 at 480 PPQN over `bar_count` bars, range C2–C5.
const fn constraints(bar_count: usize) -> GenerationConstraints {
    GenerationConstraints {
        bar_count,
        time_signature: TimeSignature {
            numerator: 4,
            denominator: 4,
        },
        tempo: Tempo(120.0),
        ticks_per_quarter: Ticks(480),
        pitch_lo: Pitch(36), // C2
        pitch_hi: Pitch(72), // C5
    }
}

/// A base set request; `source_rhythms` and `gesture` vary per test.
fn set_request(
    seed: u64,
    variants_per_strategy: usize,
    source_rhythms: Vec<Vec<Ticks>>,
    gesture: Option<GestureControl>,
) -> SetRequest {
    SetRequest {
        seed: GenerationSeed(seed),
        pitch_material: material(),
        constraints: constraints(4),
        source_rhythms,
        variants_per_strategy,
        gesture,
    }
}

/// One bar of quarter notes — the simplest usable rhythm template.
fn quarters() -> Vec<Vec<Ticks>> {
    vec![vec![Ticks(480); 4]]
}

/// The `(onset, duration, pitch)` triples of the candidate's single voice.
fn notes(score: &Score) -> Vec<(u32, u32, u8)> {
    score.tracks[0].voices[0]
        .event_groups
        .iter()
        .flat_map(|g| g.atoms.iter())
        .filter_map(|a| match a {
            AtomEvent::Note(n) => Some((n.absolute_start.0, n.duration.0, n.pitch.0)),
            AtomEvent::Rest(_) => None,
        })
        .collect()
}

/// Comparable identity of a candidate: provenance plus produced notes.
type Fingerprint = (GenerationStrategy, u64, Vec<(u32, u32, u8)>);

fn fingerprint(c: &SetCandidate) -> Fingerprint {
    (c.strategy, c.seed.0, notes(&c.score))
}

/// Every S6 strategy, in declaration order.
const ALL_STRATEGIES: [GenerationStrategy; 5] = [
    GenerationStrategy::RhythmCopyPitchSubstitute,
    GenerationStrategy::MotifTransposeVariation,
    GenerationStrategy::ConstrainedRandomWalk,
    GenerationStrategy::ShuffleMotifs,
    GenerationStrategy::RepeatVariation,
];

// ── candidate set ─────────────────────────────────────────────────────────────

#[test]
fn set_covers_every_strategy_and_derives_distinct_seeds() {
    let set = generate_candidate_set(&set_request(7, 2, quarters(), None))
        .expect("set generation succeeds");

    assert_eq!(set.len(), 10, "5 strategies × 2 variants");
    for strategy in ALL_STRATEGIES {
        let count = set.iter().filter(|c| c.strategy == strategy).count();
        assert_eq!(count, 2, "two variants of {strategy:?}");
    }

    let mut seeds: Vec<u64> = set.iter().map(|c| c.seed.0).collect();
    seeds.sort_unstable();
    seeds.dedup();
    assert_eq!(seeds.len(), 10, "every variant runs under its own seed");

    for candidate in &set {
        assert!(
            !notes(&candidate.score).is_empty(),
            "{:?} produced an empty candidate",
            candidate.strategy
        );
    }
}

#[test]
fn set_skips_rhythm_copy_without_templates() {
    let set = generate_candidate_set(&set_request(7, 1, Vec::new(), None)).expect("set generation");

    assert_eq!(set.len(), 4, "remaining strategies × 1 variant");
    assert!(
        set.iter()
            .all(|c| c.strategy != GenerationStrategy::RhythmCopyPitchSubstitute),
        "rhythm-copy needs a template and must be skipped, not fail the set"
    );
}

#[test]
fn set_is_deterministic_for_a_fixed_request() {
    let request = set_request(1234, 3, quarters(), None);
    let a: Vec<_> = generate_candidate_set(&request)
        .expect("first run")
        .iter()
        .map(fingerprint)
        .collect();
    let b: Vec<_> = generate_candidate_set(&request)
        .expect("second run")
        .iter()
        .map(fingerprint)
        .collect();
    assert_eq!(a, b, "the same request always yields the same set");
}

#[test]
fn rhythm_copy_variants_rotate_templates() {
    // Two deliberately extreme templates: one whole note vs eight eighths.
    // Variant 0 must draw the first template, variant 1 the second, so a
    // multi-template corpus is audible in the candidate set.
    let templates = vec![vec![Ticks(1920)], vec![Ticks(240); 8]];
    let mut request = set_request(9, 2, templates, None);
    request.constraints = constraints(1);

    let set = generate_candidate_set(&request).expect("set generation");
    let mut counts: Vec<usize> = set
        .iter()
        .filter(|c| c.strategy == GenerationStrategy::RhythmCopyPitchSubstitute)
        .map(|c| notes(&c.score).len())
        .collect();
    counts.sort_unstable();
    assert_eq!(
        counts,
        vec![1, 8],
        "one variant per template: whole-note bar and eighths bar"
    );
}

#[test]
fn set_applies_gesture_control() {
    let control = GestureControl {
        burst_notes: 2,
        rest_quarters: 1.0,
    };
    let plain = generate_candidate_set(&set_request(7, 1, quarters(), None)).expect("plain set");
    let gestured = generate_candidate_set(&set_request(7, 1, quarters(), Some(control)))
        .expect("gestured set");

    assert_eq!(plain.len(), gestured.len());
    for (p, g) in plain.iter().zip(&gestured) {
        assert_eq!(p.strategy, g.strategy);
        assert_eq!(
            p.seed, g.seed,
            "variant seeds depend only on (base seed, strategy, variant)"
        );
        assert_eq!(g.gesture, Some(control), "carved candidates keep the ask");
        assert!(
            notes(&g.score).len() < notes(&p.score).len(),
            "{:?}: carving gesture rests must drop notes from wall-to-wall output",
            p.strategy
        );
    }
    assert!(
        plain.iter().all(|c| c.gesture.is_none()),
        "no control asked, none recorded"
    );
}

#[test]
fn set_rejects_zero_variants() {
    // `Score` (inside `SetCandidate`) carries no `PartialEq`, so match on the
    // error rather than comparing whole `Result`s.
    assert!(matches!(
        generate_candidate_set(&set_request(7, 0, quarters(), None)),
        Err(SetError::VariantCountZero),
    ));
}

// ── rerank ────────────────────────────────────────────────────────────────────

#[test]
fn rerank_labels_join_closure_and_novelty() {
    assert_eq!(RERANK_AXIS_LABELS.len(), 6);
    assert_eq!(&RERANK_AXIS_LABELS[..4], &CLOSURE_AXIS_LABELS);
    assert_eq!(&RERANK_AXIS_LABELS[4..], &NOVELTY_AXIS_LABELS);
}

#[test]
fn rerank_returns_ranked_explainable_envelopes() {
    let request = set_request(7, 1, quarters(), None);
    let set = generate_candidate_set(&request).expect("set generation");
    let expected = set.len();
    let policy = rerank_weights_v1();

    let ranked = rerank_candidates(set, &material(), &[], &policy);

    assert_eq!(ranked.len(), expected, "every candidate is scored");
    for pair in ranked.windows(2) {
        assert!(
            pair[0].aggregate() >= pair[1].aggregate(),
            "envelopes come back in rank order"
        );
    }
    for scored in &ranked {
        assert_eq!(scored.provenance.policy_id, "generation_rerank");
        assert_eq!(scored.provenance.policy_version, 1);
        assert_eq!(
            scored.provenance.seed,
            Some(scored.value.seed.0),
            "provenance carries the candidate's own seed"
        );
        for label in RERANK_AXIS_LABELS {
            assert!(
                scored.axes.get(label).is_some(),
                "axis {label} present on every envelope"
            );
        }
        // No corpus yet: nothing to quote, both novelty axes read fully novel.
        assert_eq!(scored.axes.get("quote_novelty"), Some(1.0));
        assert_eq!(scored.axes.get("ngram_novelty"), Some(1.0));
    }
}

#[test]
fn rerank_is_deterministic() {
    let request = set_request(21, 2, quarters(), None);
    let policy = rerank_weights_v1();

    let a: Vec<(GenerationStrategy, u64)> = rerank_candidates(
        generate_candidate_set(&request).expect("set"),
        &material(),
        &[],
        &policy,
    )
    .iter()
    .map(|s| (s.value.strategy, s.value.seed.0))
    .collect();
    let b: Vec<(GenerationStrategy, u64)> = rerank_candidates(
        generate_candidate_set(&request).expect("set"),
        &material(),
        &[],
        &policy,
    )
    .iter()
    .map(|s| (s.value.strategy, s.value.seed.0))
    .collect();

    assert_eq!(
        a, b,
        "rank order is stable under a fixed request and policy"
    );
}

#[test]
fn rerank_penalises_a_verbatim_quote_under_a_novelty_policy() {
    let set = generate_candidate_set(&set_request(7, 1, quarters(), None)).expect("set");
    // The corpus "contains" candidate 0 verbatim: quoting it must cost rank.
    let quoted_seed = set[0].seed;
    let reference = set[0].score.clone();
    let policy = WeightPolicy::new(
        "novelty_only",
        1,
        vec![(NOVELTY_AXIS_LABELS[0], 1.0), (NOVELTY_AXIS_LABELS[1], 1.0)],
    );

    let ranked = rerank_candidates(set, &material(), slice::from_ref(&reference), &policy);

    let quoted = ranked
        .iter()
        .find(|s| s.value.seed == quoted_seed)
        .expect("the quoted candidate is still scored");
    assert_eq!(
        quoted.axes.get("quote_novelty"),
        Some(0.0),
        "a full self-quote has zero quote novelty"
    );
    let top = &ranked[0];
    assert!(
        top.value.seed != quoted_seed,
        "the verbatim quote must not win the rank"
    );
    assert!(
        top.aggregate() > quoted.aggregate(),
        "novel material outranks the quote"
    );
}
