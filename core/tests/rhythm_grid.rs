// TDD red phase: the rhythm grid — corpus rhythm becomes an input of *every*
// strategy, and rests become first-class in generated output.
//
// The 2026-07-11 corpus playtest (decisions.log; 220 chunks, 330 templates)
// showed the corpus is audible only when a RhythmCopyPitchSubstitute
// candidate wins the rerank: the other four strategies hardcode wall-to-wall
// quarter notes, so corpus rhythm templates and in-bar silence never reach
// their output. This suite specifies the fix:
//
// - `RhythmTemplate` replaces the bare duration list: one bar of
//   onset-*placed* notes (`TemplateNote { offset, duration }`), so gaps —
//   rests, syncopation — survive extraction and generation.
//   `RhythmTemplate::from_durations` rebuilds the legacy back-to-back shape.
// - Every strategy lays its pitches onto the same per-bar grid: the first
//   usable template, clamped to the bar (offsets past the bar end drop the
//   note; durations clamp to the bar end; an unusable grid and an absent
//   template both fall back to the quarter-note grid, preserving today's
//   behaviour). Pitch logic stays the strategy's own.
//
// References `griff_core::generate::{RhythmTemplate, TemplateNote}`, which do
// not exist yet, so the suite fails to compile until the green step.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::missing_assert_message,
    clippy::missing_const_for_fn
)]

use griff_core::{
    event::{Pitch, Tempo, Ticks, TimeSignature},
    generate::{
        generate, GenerationCandidate, GenerationConstraints, GenerationSeed, GenerationStrategy,
        PitchMaterial, RhythmTemplate, RuleGenerationRequest, TemplateNote,
    },
    score::{AtomEvent, AtomNote, Voice},
};

// ── helpers ───────────────────────────────────────────────────────────────────

fn material() -> PitchMaterial {
    PitchMaterial {
        root: Pitch(40), // E2
        intervals: vec![0, 3, 5, 7, 10],
    }
}

/// 4/4 at 480 PPQN over two bars (bar = 1920 ticks), range C2–C5.
fn constraints() -> GenerationConstraints {
    GenerationConstraints {
        bar_count: 2,
        time_signature: TimeSignature {
            numerator: 4,
            denominator: 4,
        },
        tempo: Tempo(120.0),
        ticks_per_quarter: Ticks(480),
        pitch_lo: Pitch(36),
        pitch_hi: Pitch(72),
    }
}

fn request(strategy: GenerationStrategy, templates: Vec<RhythmTemplate>) -> RuleGenerationRequest {
    RuleGenerationRequest {
        seed: GenerationSeed(42),
        pitch_material: material(),
        constraints: constraints(),
        source_rhythms: templates,
        strategy,
    }
}

/// A template note at `offset` for `duration` ticks.
fn tn(offset: u32, duration: u32) -> TemplateNote {
    TemplateNote {
        offset: Ticks(offset),
        duration: Ticks(duration),
    }
}

const ALL_STRATEGIES: [GenerationStrategy; 5] = [
    GenerationStrategy::RhythmCopyPitchSubstitute,
    GenerationStrategy::MotifTransposeVariation,
    GenerationStrategy::ConstrainedRandomWalk,
    GenerationStrategy::ShuffleMotifs,
    GenerationStrategy::RepeatVariation,
];

/// The single generated voice (track 0, voice 0).
fn voice(candidate: &GenerationCandidate) -> &Voice {
    &candidate.score.tracks[0].voices[0]
}

/// `(onset, duration)` of every note atom, in order.
fn placements(candidate: &GenerationCandidate) -> Vec<(u32, u32)> {
    voice(candidate)
        .event_groups
        .iter()
        .flat_map(|g| g.atoms.iter())
        .filter_map(|a| match a {
            AtomEvent::Note(n) => Some((n.absolute_start.0, n.duration.0)),
            AtomEvent::Rest(_) => None,
        })
        .collect()
}

/// All note atoms, in order.
fn notes(candidate: &GenerationCandidate) -> Vec<AtomNote> {
    voice(candidate)
        .event_groups
        .iter()
        .flat_map(|g| g.atoms.iter())
        .filter_map(|a| match a {
            AtomEvent::Note(n) => Some(*n),
            AtomEvent::Rest(_) => None,
        })
        .collect()
}

/// Note count per bar of `bar_ticks`, indexed by bar.
fn notes_per_bar(candidate: &GenerationCandidate, bar_ticks: u32) -> Vec<usize> {
    let mut per_bar = vec![0_usize; candidate.score.master_bars.len()];
    for (onset, _) in placements(candidate) {
        let bar = (onset / bar_ticks) as usize;
        if let Some(slot) = per_bar.get_mut(bar) {
            *slot += 1;
        }
    }
    per_bar
}

// ── the grid reaches every strategy ───────────────────────────────────────────

#[test]
fn gapped_template_places_notes_at_offsets_for_every_strategy() {
    // Burst-and-rest bar: a note on the downbeat, silence, a note on beat 3 —
    // the gap must survive into every strategy's output (research note §1.3:
    // a metrically placed rest is phrasing, not absence).
    let template = RhythmTemplate {
        notes: vec![tn(0, 240), tn(960, 240)],
    };
    for strategy in ALL_STRATEGIES {
        let candidate =
            generate(&request(strategy, vec![template.clone()])).expect("generate succeeds");
        assert_eq!(
            placements(&candidate),
            vec![(0, 240), (960, 240), (1920, 240), (2880, 240)],
            "{strategy:?}: the gapped grid places two notes per bar, silence between"
        );
    }
}

#[test]
fn from_durations_rebuilds_the_wall_to_wall_layout() {
    // The legacy shape — back-to-back durations — is a template whose offsets
    // accumulate. A quarter grid must therefore reproduce today's layout.
    let template = RhythmTemplate::from_durations(&[Ticks(480); 4]);
    assert_eq!(
        template.notes,
        vec![tn(0, 480), tn(480, 480), tn(960, 480), tn(1440, 480)],
    );

    let candidate = generate(&request(
        GenerationStrategy::ConstrainedRandomWalk,
        vec![template],
    ))
    .expect("generate succeeds");
    let quarters: Vec<(u32, u32)> = (0..8).map(|i| (i * 480, 480)).collect();
    assert_eq!(placements(&candidate), quarters);
}

#[test]
fn strategies_fall_back_to_quarters_without_a_template() {
    // No corpus, no template: today's wall-to-wall quarter behaviour holds
    // (characterization — the grid must not change the no-input case).
    let candidate = generate(&request(
        GenerationStrategy::ConstrainedRandomWalk,
        Vec::new(),
    ))
    .expect("generate succeeds");
    let quarters: Vec<(u32, u32)> = (0..8).map(|i| (i * 480, 480)).collect();
    assert_eq!(placements(&candidate), quarters);
}

#[test]
fn template_durations_clamp_to_the_bar_end() {
    // A note starting on beat 4 asking for two beats gets one: durations
    // clamp to the bar boundary rather than bleeding into the next bar.
    let template = RhythmTemplate {
        notes: vec![tn(1440, 960)],
    };
    let candidate = generate(&request(GenerationStrategy::ShuffleMotifs, vec![template]))
        .expect("generate succeeds");
    assert_eq!(placements(&candidate), vec![(1440, 480), (3360, 480)]);
}

#[test]
fn unusable_template_falls_back_to_quarters() {
    // Every note sits past the bar end: the grid is unusable, and the bar
    // falls back to quarters instead of generating silence-only bars that
    // the rerank would drop.
    let template = RhythmTemplate {
        notes: vec![tn(2000, 240)],
    };
    let candidate = generate(&request(
        GenerationStrategy::ConstrainedRandomWalk,
        vec![template],
    ))
    .expect("generate succeeds");
    let quarters: Vec<(u32, u32)> = (0..8).map(|i| (i * 480, 480)).collect();
    assert_eq!(placements(&candidate), quarters);
}

// ── per-bar template rotation ─────────────────────────────────────────────────

#[test]
fn multiple_templates_rotate_across_bars() {
    // Two rhythms — bar of eighths (8 notes) vs one whole note (1) — must
    // alternate across bars so a corpus's rhythmic variety survives *inside*
    // one phrase (2026-07 A/B: distinct_dur stuck at 1 because every bar
    // reused the first template).
    let eighths = RhythmTemplate::from_durations(&[Ticks(240); 8]);
    let whole = RhythmTemplate {
        notes: vec![tn(0, 1920)],
    };
    // Every per-bar-independent strategy rotates; RepeatVariation is its own
    // case (its identity is repetition — tested separately below).
    for strategy in [
        GenerationStrategy::RhythmCopyPitchSubstitute,
        GenerationStrategy::MotifTransposeVariation,
        GenerationStrategy::ConstrainedRandomWalk,
        GenerationStrategy::ShuffleMotifs,
    ] {
        let mut req = request(strategy, vec![eighths.clone(), whole.clone()]);
        req.constraints.bar_count = 4;
        let candidate = generate(&req).expect("generate succeeds");
        assert_eq!(
            notes_per_bar(&candidate, 1920),
            vec![8, 1, 8, 1],
            "{strategy:?}: bars must cycle the two template rhythms"
        );
    }
}

#[test]
fn repeat_variation_keeps_one_rhythm_across_bars() {
    // RepeatVariation's identity is repetition (call/response), so it stays on
    // the first template's rhythm even when several exist — otherwise the
    // "repeat" no longer reads as one.
    let eighths = RhythmTemplate::from_durations(&[Ticks(240); 8]);
    let whole = RhythmTemplate {
        notes: vec![tn(0, 1920)],
    };
    let mut req = request(
        GenerationStrategy::RepeatVariation,
        vec![eighths, whole],
    );
    req.constraints.bar_count = 4;
    let candidate = generate(&req).expect("generate succeeds");
    assert_eq!(
        notes_per_bar(&candidate, 1920),
        vec![8, 8, 8, 8],
        "repeat variation repeats the first template's rhythm"
    );
}

#[test]
fn single_template_still_repeats_every_bar() {
    // One template → rotation is trivial: every bar uses it (the grid must not
    // regress the single-template case).
    let template = RhythmTemplate {
        notes: vec![tn(0, 240), tn(960, 240)],
    };
    let mut req = request(GenerationStrategy::ConstrainedRandomWalk, vec![template]);
    req.constraints.bar_count = 3;
    let candidate = generate(&req).expect("generate succeeds");
    assert_eq!(notes_per_bar(&candidate, 1920), vec![2, 2, 2]);
}

#[test]
fn gapped_generation_is_deterministic_and_in_range() {
    let template = RhythmTemplate {
        notes: vec![tn(0, 240), tn(360, 120), tn(960, 480)],
    };
    for strategy in ALL_STRATEGIES {
        let a = generate(&request(strategy, vec![template.clone()])).expect("run a");
        let b = generate(&request(strategy, vec![template.clone()])).expect("run b");
        assert_eq!(voice(&a), voice(&b), "{strategy:?}: deterministic");
        for n in notes(&a) {
            assert!(
                (36..=72).contains(&n.pitch.0),
                "{strategy:?}: pitch {} in range",
                n.pitch.0
            );
        }
    }
}
