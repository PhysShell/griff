// TDD red phase: the explicit *scoped tonal context* contract (S15 Phase 2).
//
// Phase 1 gave the core an evidence layer (`PitchEvidence`) and a ranked
// inference layer (`TonalEstimate`). Phase 2 lets a generation-facing request
// and its provenance *carry* one of those estimates — explicitly scoped,
// optional, and inert — without changing note selection.
//
// The contract under test:
//
//   - `TonalContext` bundles the caller's `EvidenceScope`, a compact immutable
//     `TonalProjection` of the ranked estimate (winner, runner-up, margin), and
//     a `TonalProvenance` saying how the estimate was measured (method, the
//     histogram that actually weighted it, notes observed).
//   - Absence is first class *twice over*: no context at all (`Option` at the
//     request) and a context whose scope had nothing to estimate
//     (`projection: None`). Those are different states and never conflated.
//   - Ambiguity survives the projection: the runner-up and the margin are part
//     of the compact form, so a near tie stays visible. No threshold, no
//     "confident" verdict — that is Phase 3B calibration territory.
//   - The scope is the caller's. There is no scope-free constructor, and
//     nothing re-picks a scope behind the caller.
//   - Serialisation is deterministic and round-trips; deserialisation is
//     fail-closed on a foreign field.
//   - `GenerationAsk` and `RankedSet` carry the context and *ignore* it: a pass
//     with context is byte-identical to a pass without.
//
// References `griff_core::tonal::TonalContext` and the new `tonal` fields on
// `GenerationAsk` / `RankedSet`, none of which exist yet, so this suite fails
// to compile until the green step.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing,
    clippy::float_cmp,
    clippy::arithmetic_side_effects,
    clippy::str_to_string,
    // `score` (the Score) and `scope` (the region) are distinct domain terms.
    clippy::similar_names
)]

use griff_core::{
    event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity},
    generate::GenerationStrategy,
    generation_input::{ranked_candidates, GenerationAsk, RankedSet},
    score::{
        AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, RepeatMarker,
        Score, Track, Voice,
    },
    slice::TickRange,
    tonal::{
        estimate_key, EvidenceScope, KeyMode, PitchEvidence, TonalContext, TonalMethod,
        TonalProjection, TonalWeighting,
    },
};

const PPQN: u16 = 480;
const QUARTER: u32 = 480;
const BAR: u32 = 1920; // 4/4 at 480 PPQN

// ── fixtures ─────────────────────────────────────────────────────────────────

fn note(start: u32, pitch: u8, duration: u32) -> AtomEvent {
    AtomEvent::Note(AtomNote {
        absolute_start: Ticks(start),
        duration: Ticks(duration),
        pitch: Pitch::new(pitch).expect("valid pitch"),
        velocity: Velocity::new(90).expect("valid velocity"),
        marks: NoteMarks::empty(),
        position: None,
    })
}

fn voice_of(pitches: &[u8], duration: u32) -> Voice {
    Voice {
        id: 0,
        event_groups: pitches
            .iter()
            .enumerate()
            .map(|(i, &p)| EventGroup {
                kind: EventGroupKind::Single,
                atoms: vec![note(u32::try_from(i).unwrap() * QUARTER, p, duration)],
                technique_spans: Vec::new(),
            })
            .collect(),
    }
}

fn track_of(pitches: &[u8], duration: u32) -> Track {
    Track {
        name: None,
        channel: 0,
        voices: vec![voice_of(pitches, duration)],
        tuning: Tuning::standard_e(),
    }
}

fn score_of(tracks: Vec<Track>, bars: usize) -> Score {
    Score {
        ticks_per_quarter: PPQN,
        master_bars: (0..bars)
            .map(|i| {
                let start = u32::try_from(i).unwrap() * BAR;
                MasterBar {
                    index: i,
                    tick_range: TickRange::new(Ticks(start), Ticks(start + BAR)).expect("ordered"),
                    time_signature: TimeSignature {
                        numerator: 4,
                        denominator: 4,
                    },
                    tempo: Tempo::from_bpm_integer(120).expect("120 BPM"),
                    repeat: RepeatMarker::default(),
                }
            })
            .collect(),
        tracks,
        source_meta: None,
        loss: LossReport::new(),
    }
}

/// C major, tonic doubled at the octave — the exact histogram shape Phase 1
/// pins to C major.
const C_MAJOR: [u8; 8] = [60, 62, 64, 65, 67, 69, 71, 72];
/// The same shape transposed up a tritone: F# major, tonic doubled.
const FS_MAJOR: [u8; 8] = [66, 68, 70, 71, 73, 75, 77, 78];

/// Two tracks in keys a tritone apart — the scope, and only the scope, decides
/// which key the context reports.
fn two_key_score() -> Score {
    score_of(
        vec![track_of(&C_MAJOR, QUARTER), track_of(&FS_MAJOR, QUARTER)],
        2,
    )
}

// ── scope: explicit, preserved, never re-picked ──────────────────────────────

#[test]
fn the_context_reports_the_scope_the_caller_named() {
    let score = two_key_score();
    for scope in [
        EvidenceScope::WholeScore,
        EvidenceScope::Track(0),
        EvidenceScope::Track(1),
        EvidenceScope::Voice { track: 1, voice: 0 },
    ] {
        assert_eq!(
            TonalContext::measure(&score, scope).scope,
            scope,
            "the measured context keeps the caller's scope verbatim"
        );
    }
}

#[test]
fn the_scope_decides_the_key_nothing_re_picks_it() {
    // Both tracks sound; each names a different key. A context measured over
    // track 0 must report C major and one over track 1 F# major — there is no
    // "best track" search, no whole-score override.
    let score = two_key_score();

    let t0 = TonalContext::measure(&score, EvidenceScope::Track(0));
    let t1 = TonalContext::measure(&score, EvidenceScope::Track(1));

    let w0 = t0.projection.expect("track 0 sounds").winner;
    let w1 = t1.projection.expect("track 1 sounds").winner;

    assert_eq!(
        (w0.tonic, w0.mode),
        (0, KeyMode::Major),
        "track 0 is C major"
    );
    assert_eq!(
        (w1.tonic, w1.mode),
        (6, KeyMode::Major),
        "track 1 is F# major"
    );
    assert_ne!(
        (w0.tonic, w0.mode),
        (w1.tonic, w1.mode),
        "the caller's scope, not the score, chose the key"
    );
}

// ── absence: a silent scope abstains, coherently ─────────────────────────────

#[test]
fn a_silent_scope_yields_a_context_that_abstains() {
    let score = score_of(vec![track_of(&C_MAJOR, QUARTER)], 2);
    let ctx = TonalContext::measure(&score, EvidenceScope::Track(9));

    assert_eq!(ctx.scope, EvidenceScope::Track(9), "the asked-for scope");
    assert_eq!(
        ctx.projection, None,
        "nothing to project: honest abstention"
    );
    assert_eq!(ctx.provenance.note_count, 0);
    assert_eq!(
        ctx.provenance.weighting, None,
        "no estimate ran, so no histogram weighted one"
    );
    assert_eq!(ctx.provenance.method, TonalMethod::KsV1);
}

#[test]
fn absence_is_coherent_across_the_whole_context() {
    // The projection and the weighting go missing together, always: a context
    // never claims to have weighted a histogram it never estimated from, and
    // never hides a projection it did produce.
    let score = two_key_score();
    for scope in [
        EvidenceScope::WholeScore,
        EvidenceScope::Track(0),
        EvidenceScope::Track(9),
        EvidenceScope::Voice { track: 0, voice: 7 },
    ] {
        let ctx = TonalContext::measure(&score, scope);
        assert_eq!(
            ctx.projection.is_none(),
            ctx.provenance.weighting.is_none(),
            "projection and weighting agree about absence at {scope:?}"
        );
        assert_eq!(
            ctx.projection.is_some(),
            ctx.provenance.note_count > 0,
            "a projection exists exactly when the scope sounded, at {scope:?}"
        );
    }
}

// ── ambiguity survives the compact projection ────────────────────────────────

#[test]
fn a_flat_chromatic_scope_projects_a_zero_margin_and_keeps_its_rival() {
    // Every pitch class once, identical durations: the estimator's honest
    // ambiguity case. The compact form must not launder it into a winner.
    let chromatic: Vec<u8> = (60..72).collect();
    let score = score_of(vec![track_of(&chromatic, QUARTER)], 3);

    let ctx = TonalContext::measure(&score, EvidenceScope::WholeScore);
    let projection = ctx.projection.expect("a chromatic scope still estimates");

    assert_eq!(
        projection.confidence_margin, 0.0,
        "a flat histogram has no winner"
    );
    assert!(
        projection.runner_up.is_some(),
        "the rival survives into the compact projection, so a tie stays visible"
    );
    assert_eq!(
        projection.winner.correlation,
        projection.runner_up.expect("rival").correlation,
        "an exact tie: equal correlations, zero margin"
    );
}

// ── the compact projection is the ranked estimate's top two ──────────────────

#[test]
fn the_projection_is_exactly_the_ranked_estimates_top_two_and_margin() {
    let score = two_key_score();
    let scope = EvidenceScope::Track(0);

    let evidence = PitchEvidence::measure(&score, scope);
    let estimate = estimate_key(&evidence).expect("track 0 sounds");
    let projection = TonalContext::measure(&score, scope)
        .projection
        .expect("track 0 sounds");

    assert_eq!(projection.winner, estimate.candidates[0]);
    assert_eq!(projection.runner_up, Some(estimate.candidates[1]));
    assert_eq!(projection.confidence_margin, estimate.confidence_margin);
}

#[test]
fn a_projection_built_from_an_estimate_matches_the_measured_one() {
    let score = two_key_score();
    let scope = EvidenceScope::Track(1);

    let evidence = PitchEvidence::measure(&score, scope);
    let estimate = estimate_key(&evidence).expect("track 1 sounds");

    assert_eq!(
        TonalProjection::from_estimate(&estimate),
        TonalContext::measure(&score, scope).projection,
        "one projection rule, whichever door the caller comes through"
    );
}

// ── provenance names the histogram that actually weighted the estimate ───────

#[test]
fn provenance_reports_duration_mass_when_the_notes_have_duration() {
    let score = two_key_score();
    let ctx = TonalContext::measure(&score, EvidenceScope::Track(0));

    assert_eq!(ctx.provenance.weighting, Some(TonalWeighting::DurationMass));
    assert_eq!(ctx.provenance.note_count, C_MAJOR.len());
    assert_eq!(ctx.provenance.method, TonalMethod::KsV1);
}

#[test]
fn provenance_reports_the_onset_fallback_when_every_note_has_zero_duration() {
    // KS v1 falls back to raw onset counts only when the total duration mass is
    // zero. The context must say so — a consumer can otherwise not tell which
    // fact the estimate rests on.
    let score = score_of(vec![track_of(&C_MAJOR, 0)], 2);
    let ctx = TonalContext::measure(&score, EvidenceScope::Track(0));

    assert_eq!(ctx.provenance.weighting, Some(TonalWeighting::OnsetCounts));
    assert!(
        ctx.projection.is_some(),
        "the fallback keeps the estimate defined"
    );
}

// ── serialisation: deterministic, round-tripping, fail-closed ────────────────

#[test]
fn an_abstaining_context_serialises_to_its_pinned_shape() {
    // Pinned because this is a provenance artifact contract, not an internal
    // struct. The abstaining case carries no floats, so the whole envelope can
    // be written out exactly.
    let score = score_of(vec![track_of(&C_MAJOR, QUARTER)], 2);
    let silent = TonalContext::measure(&score, EvidenceScope::Track(9));

    assert_eq!(
        serde_json::to_string(&silent).expect("serialise"),
        r#"{"scope":{"kind":"track","at":9},"projection":null,"provenance":{"method":"ks_v1","weighting":null,"note_count":0}}"#
    );
}

#[test]
fn a_projecting_context_serialises_its_scope_winner_and_provenance() {
    let score = score_of(vec![track_of(&C_MAJOR, QUARTER)], 2);
    let json = serde_json::to_string(&TonalContext::measure(&score, EvidenceScope::WholeScore))
        .expect("serialise");

    assert!(
        json.starts_with(
            r#"{"scope":{"kind":"whole_score"},"projection":{"winner":{"tonic":0,"mode":"major","#
        ),
        "the envelope leads with the scope, then the winner: {json}"
    );
    assert!(
        json.ends_with(
            r#""provenance":{"method":"ks_v1","weighting":"duration_mass","note_count":8}}"#
        ),
        "and closes with how the estimate was measured: {json}"
    );
}

#[test]
fn every_scope_shape_serialises_tagged_and_round_trips() {
    let score = two_key_score();
    for (scope, tagged) in [
        (EvidenceScope::WholeScore, r#"{"kind":"whole_score"}"#),
        (EvidenceScope::Track(1), r#"{"kind":"track","at":1}"#),
        (
            EvidenceScope::Voice { track: 1, voice: 0 },
            r#"{"kind":"voice","at":{"track":1,"voice":0}}"#,
        ),
    ] {
        let ctx = TonalContext::measure(&score, scope);
        let json = serde_json::to_string(&ctx).expect("serialise");
        assert!(
            json.contains(tagged),
            "scope {scope:?} serialises as {tagged}, got {json}"
        );
        let back: TonalContext = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(back, ctx, "round-trip is exact, floats included");
    }
}

#[test]
fn serialising_the_same_context_twice_yields_the_same_bytes() {
    let score = two_key_score();
    let ctx = TonalContext::measure(&score, EvidenceScope::WholeScore);
    assert_eq!(
        serde_json::to_string(&ctx).expect("serialise"),
        serde_json::to_string(&ctx).expect("serialise"),
        "serialisation is deterministic"
    );
}

#[test]
fn a_foreign_field_is_refused_rather_than_dropped() {
    // Fail-closed: a provenance artifact that silently absorbs unknown fields
    // cannot be trusted to mean what it says.
    let cases = [
        r#"{"scope":{"kind":"whole_score"},"projection":null,"provenance":{"method":"ks_v1","weighting":null,"note_count":0},"confident":true}"#,
        r#"{"scope":{"kind":"whole_score"},"projection":null,"provenance":{"method":"ks_v1","weighting":null,"note_count":0,"threshold":0.1}}"#,
    ];
    for case in cases {
        assert!(
            serde_json::from_str::<TonalContext>(case).is_err(),
            "an unknown field must refuse: {case}"
        );
    }
}

// ── replay: the context reproduces itself from its own scope ─────────────────

#[test]
fn re_measuring_from_the_contexts_own_scope_reproduces_it_exactly() {
    let score = two_key_score();
    for scope in [
        EvidenceScope::WholeScore,
        EvidenceScope::Track(0),
        EvidenceScope::Track(1),
        EvidenceScope::Track(9),
    ] {
        let first = TonalContext::measure(&score, scope);
        let replayed = TonalContext::measure(&score, first.scope);
        assert_eq!(
            replayed, first,
            "replay from the carried scope at {scope:?}"
        );
        assert_eq!(
            serde_json::to_string(&replayed).expect("serialise"),
            serde_json::to_string(&first).expect("serialise"),
            "and byte-identically at {scope:?}"
        );
    }
}

#[test]
fn a_deserialised_context_replays_from_its_carried_scope() {
    // The whole point of carrying the scope: a context read back from an
    // artifact can be re-derived against the same score without a second guess
    // about what was measured.
    let score = two_key_score();
    let json = serde_json::to_string(&TonalContext::measure(&score, EvidenceScope::Track(1)))
        .expect("serialise");
    let back: TonalContext = serde_json::from_str(&json).expect("deserialise");

    assert_eq!(TonalContext::measure(&score, back.scope), back);
}

// ── generation: the context is carried and ignored ───────────────────────────

/// Four bars of quarter notes on one track — enough material to seed a pass.
fn generation_source() -> Score {
    score_of(vec![track_of(&C_MAJOR, QUARTER)], 2)
}

const fn ask(tonal: Option<TonalContext>) -> GenerationAsk {
    GenerationAsk {
        seed: 42,
        bars: 4,
        variants_per_strategy: 2,
        gesture: false,
        tonal,
    }
}

/// Everything about a pass a tonal context must not touch: the candidates in
/// rank order with their strategy, derived seed, aggregate bits, and notes.
type Fingerprint = Vec<(GenerationStrategy, u64, u64, Vec<(u32, u32, u8, u8)>)>;

fn fingerprint(set: &RankedSet) -> Fingerprint {
    set.ranked
        .iter()
        .map(|c| {
            let notes = c
                .value
                .score
                .tracks
                .iter()
                .flat_map(|t| &t.voices)
                .flat_map(|v| &v.event_groups)
                .flat_map(|g| &g.atoms)
                .filter_map(|a| match a {
                    AtomEvent::Note(n) => {
                        Some((n.absolute_start.0, n.duration.0, n.pitch.0, n.velocity.0))
                    }
                    AtomEvent::Rest(_) => None,
                })
                .collect();
            (
                c.value.strategy,
                c.value.seed.0,
                c.aggregate().to_bits(),
                notes,
            )
        })
        .collect()
}

#[test]
fn a_pass_with_tonal_context_is_identical_to_one_without() {
    let source = generation_source();
    let context = TonalContext::measure(&source, EvidenceScope::WholeScore);

    let plain = ranked_candidates(&source, None, &ask(None), None).expect("seeds");
    let with_context = ranked_candidates(&source, None, &ask(Some(context)), None).expect("seeds");

    assert_eq!(
        fingerprint(&plain),
        fingerprint(&with_context),
        "carrying a tonal context changes no candidate, seed, score, or ranking"
    );
}

#[test]
fn tonal_context_restricts_no_pitch_and_reweights_no_policy() {
    let source = generation_source();
    let context = TonalContext::measure(&source, EvidenceScope::Track(0));

    let plain = ranked_candidates(&source, None, &ask(None), None).expect("seeds");
    let with_context = ranked_candidates(&source, None, &ask(Some(context)), None).expect("seeds");

    assert_eq!(
        plain.base.pitch_material.root.0, with_context.base.pitch_material.root.0,
        "the tab-seeded anchor is untouched: no tonic substitution"
    );
    assert_eq!(
        plain.base.pitch_material.intervals, with_context.base.pitch_material.intervals,
        "the palette is untouched: a key estimate is not a note whitelist"
    );
    assert_eq!(
        (plain.policy.id, plain.policy.version),
        (with_context.policy.id, with_context.policy.version),
        "the rerank policy is untouched"
    );
}

#[test]
fn the_ranked_set_carries_the_asks_context_verbatim() {
    let source = generation_source();
    let context = TonalContext::measure(&source, EvidenceScope::Track(0));

    let set = ranked_candidates(&source, None, &ask(Some(context)), None).expect("seeds");
    assert_eq!(
        set.tonal,
        Some(context),
        "provenance echoes the ask's context, unchanged"
    );

    let plain = ranked_candidates(&source, None, &ask(None), None).expect("seeds");
    assert_eq!(
        plain.tonal, None,
        "and a pass that was never given one reports none, not a measured default"
    );
}
