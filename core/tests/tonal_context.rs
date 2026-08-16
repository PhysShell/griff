// TDD red phase: the explicit *scoped tonal context* contract (S15 Phase 2),
// revised after the Phase-2 review.
//
// Phase 1 gave the core an evidence layer (`PitchEvidence`) and a ranked
// inference layer (`TonalEstimate`). Phase 2 lets a generation-facing request
// and its provenance *carry* one of those estimates — explicitly scoped,
// optional, and inert.
//
// The review found two holes in the first cut, and this suite pins the fixes:
//
//   1. Coherence was "by construction" only for values built through the
//      constructor. Public fields plus a derived `Deserialize` let anyone hand
//      the type a logically impossible envelope (`projection: null` with
//      `weighting: "duration_mass"`). A type called *provenance* could lie
//      about its own origin. Now the fields are private, the artifact has one
//      wire form, and deserialisation **re-derives the whole context from the
//      evidence it carries and compares** — the ADR-0033 "prove, do not trust"
//      posture — refusing typed on any disagreement.
//   2. Replayability was claimed but not delivered: the context carried a
//      scope, not the material, so "re-measure it" silently required the caller
//      to still hold the exact same `Score`. The context now carries its
//      `PitchEvidence`, so the full 24-key ranking is recoverable from the
//      artifact **alone** — which is what justifies dropping 22 candidates.
//
// And the smaller tail of (1): `deny_unknown_fields` did not reach the scope
// level, because serde's adjacently tagged enums accept unknown keys silently.
// The artifact now has exactly one raw wire representation, fail-closed at
// every level, and the suite tests a foreign field at each of them.
//
// References the getters, `TonalContext::evidence`, and `TonalArtifactError`,
// none of which exist yet, so this suite fails to compile until the green step.

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
    feature::PitchRange,
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
use proptest::prelude::*;
use serde_json::json;

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

fn json_of(context: &TonalContext) -> String {
    serde_json::to_string(context).expect("serialise")
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
            TonalContext::measure(&score, scope).scope(),
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

    let w0 = t0.projection().expect("track 0 sounds").winner();
    let w1 = t1.projection().expect("track 1 sounds").winner();

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

    assert_eq!(ctx.scope(), EvidenceScope::Track(9), "the asked-for scope");
    assert_eq!(
        ctx.projection(),
        None,
        "nothing to project: honest abstention"
    );
    assert_eq!(ctx.evidence().note_count, 0);
    assert_eq!(
        ctx.provenance().weighting(),
        None,
        "no estimate ran, so no histogram weighted one"
    );
    assert_eq!(ctx.provenance().method(), TonalMethod::KsV1);
}

#[test]
fn absence_is_coherent_across_the_whole_context() {
    let score = two_key_score();
    for scope in [
        EvidenceScope::WholeScore,
        EvidenceScope::Track(0),
        EvidenceScope::Track(9),
        EvidenceScope::Voice { track: 0, voice: 7 },
    ] {
        let ctx = TonalContext::measure(&score, scope);
        assert_eq!(
            ctx.projection().is_none(),
            ctx.provenance().weighting().is_none(),
            "projection and weighting agree about absence at {scope:?}"
        );
        assert_eq!(
            ctx.projection().is_some(),
            ctx.evidence().note_count > 0,
            "a projection exists exactly when the scope sounded, at {scope:?}"
        );
    }
}

// ── ambiguity survives the compact projection ────────────────────────────────

#[test]
fn a_flat_chromatic_scope_projects_a_zero_margin_and_keeps_its_rival() {
    let chromatic: Vec<u8> = (60..72).collect();
    let score = score_of(vec![track_of(&chromatic, QUARTER)], 3);

    let ctx = TonalContext::measure(&score, EvidenceScope::WholeScore);
    let projection = ctx.projection().expect("a chromatic scope still estimates");

    assert_eq!(
        projection.confidence_margin(),
        0.0,
        "a flat histogram has no winner"
    );
    assert!(
        projection.runner_up().is_some(),
        "the rival survives into the compact projection, so a tie stays visible"
    );
    assert_eq!(
        projection.winner().correlation,
        projection.runner_up().expect("rival").correlation,
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
        .projection()
        .expect("track 0 sounds");

    assert_eq!(projection.winner(), estimate.candidates[0]);
    assert_eq!(projection.runner_up(), Some(estimate.candidates[1]));
    assert_eq!(projection.confidence_margin(), estimate.confidence_margin);
}

#[test]
fn a_projection_built_from_an_estimate_matches_the_measured_one() {
    let score = two_key_score();
    let scope = EvidenceScope::Track(1);

    let evidence = PitchEvidence::measure(&score, scope);
    let estimate = estimate_key(&evidence).expect("track 1 sounds");

    assert_eq!(
        TonalProjection::from_estimate(&estimate),
        TonalContext::measure(&score, scope).projection(),
        "one projection rule, whichever door the caller comes through"
    );
}

// ── provenance names the histogram that actually weighted the estimate ───────

#[test]
fn provenance_reports_duration_mass_when_the_notes_have_duration() {
    let score = two_key_score();
    let ctx = TonalContext::measure(&score, EvidenceScope::Track(0));

    assert_eq!(
        ctx.provenance().weighting(),
        Some(TonalWeighting::DurationMass)
    );
    assert_eq!(ctx.evidence().note_count, C_MAJOR.len());
    assert_eq!(ctx.provenance().method(), TonalMethod::KsV1);
}

#[test]
fn provenance_reports_the_onset_fallback_when_every_note_has_zero_duration() {
    let score = score_of(vec![track_of(&C_MAJOR, 0)], 2);
    let ctx = TonalContext::measure(&score, EvidenceScope::Track(0));

    assert_eq!(
        ctx.provenance().weighting(),
        Some(TonalWeighting::OnsetCounts)
    );
    assert!(
        ctx.projection().is_some(),
        "the fallback keeps the estimate defined"
    );
}

// ── replay is self-contained: the artifact carries its own evidence ──────────

#[test]
fn the_full_ranking_is_recoverable_from_the_artifact_without_the_score() {
    // The claim that justifies dropping 22 of 24 candidates. The score is built
    // and dropped inside the block; everything after it has the JSON and
    // nothing else.
    let json = {
        let score = two_key_score();
        json_of(&TonalContext::measure(&score, EvidenceScope::Track(1)))
    };

    let ctx: TonalContext = serde_json::from_str(&json).expect("deserialise");
    let estimate = estimate_key(ctx.evidence()).expect("the carried evidence estimates");

    assert_eq!(
        estimate.candidates.len(),
        24,
        "all 24 keys come back from the artifact alone"
    );
    let projection = ctx.projection().expect("track 1 sounded");
    assert_eq!(estimate.candidates[0], projection.winner());
    assert_eq!(
        estimate.candidates[1],
        projection.runner_up().expect("rival")
    );
    assert_eq!(estimate.confidence_margin, projection.confidence_margin());
}

#[test]
fn the_carried_evidence_is_the_evidence_of_the_carried_scope() {
    let score = two_key_score();
    for scope in [
        EvidenceScope::WholeScore,
        EvidenceScope::Track(0),
        EvidenceScope::Track(9),
    ] {
        let ctx = TonalContext::measure(&score, scope);
        assert_eq!(
            *ctx.evidence(),
            PitchEvidence::measure(&score, scope),
            "the artifact's evidence is exactly what its scope measures at {scope:?}"
        );
    }
}

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
        let replayed = TonalContext::measure(&score, first.scope());
        assert_eq!(
            replayed, first,
            "replay from the carried scope at {scope:?}"
        );
        assert_eq!(
            json_of(&replayed),
            json_of(&first),
            "and byte-identically at {scope:?}"
        );
    }
}

// ── serialisation: deterministic, round-tripping, pinned ─────────────────────

#[test]
fn an_abstaining_context_serialises_to_its_pinned_shape() {
    // Pinned because this is a provenance artifact contract, not an internal
    // struct. The abstaining case carries no floats, so the whole envelope can
    // be written out exactly.
    let score = score_of(vec![track_of(&C_MAJOR, QUARTER)], 2);
    let silent = TonalContext::measure(&score, EvidenceScope::Track(9));

    assert_eq!(
        json_of(&silent),
        concat!(
            r#"{"evidence":{"scope":{"kind":"track","track":9},"note_count":0,"#,
            r#""onset_counts":[0,0,0,0,0,0,0,0,0,0,0,0],"#,
            r#""duration_mass":[0,0,0,0,0,0,0,0,0,0,0,0],"pitch_range":null},"#,
            r#""projection":null,"provenance":{"method":"ks_v1","weighting":null}}"#
        )
    );
}

#[test]
fn a_projecting_context_serialises_its_evidence_winner_and_provenance() {
    let score = score_of(vec![track_of(&C_MAJOR, QUARTER)], 2);
    let json = json_of(&TonalContext::measure(&score, EvidenceScope::WholeScore));

    assert!(
        json.starts_with(concat!(
            r#"{"evidence":{"scope":{"kind":"whole_score"},"note_count":8,"#,
            r#""onset_counts":[2,0,1,0,1,1,0,1,0,1,0,1],"#
        )),
        "the envelope leads with the evidence the estimate rests on: {json}"
    );
    assert!(
        json.contains(r#""pitch_range":{"lowest":60,"highest":72}"#),
        "the observed span travels with it: {json}"
    );
    assert!(
        json.contains(r#""projection":{"winner":{"tonic":0,"mode":"major","#),
        "then the winner: {json}"
    );
    assert!(
        json.ends_with(r#""provenance":{"method":"ks_v1","weighting":"duration_mass"}}"#),
        "and closes with how the estimate was measured: {json}"
    );
}

#[test]
fn every_scope_shape_serialises_flat_and_round_trips() {
    let score = two_key_score();
    for (scope, tagged) in [
        (EvidenceScope::WholeScore, r#"{"kind":"whole_score"}"#),
        (EvidenceScope::Track(1), r#"{"kind":"track","track":1}"#),
        (
            EvidenceScope::Voice { track: 1, voice: 0 },
            r#"{"kind":"voice","track":1,"voice":0}"#,
        ),
    ] {
        let ctx = TonalContext::measure(&score, scope);
        let json = json_of(&ctx);
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
        json_of(&ctx),
        json_of(&ctx),
        "serialisation is deterministic"
    );
}

// ── fail-closed: a foreign field is refused at every level ───────────────────

/// A valid, projecting envelope, as a `serde_json::Value` to mutate per case.
fn valid_envelope() -> serde_json::Value {
    let score = score_of(vec![track_of(&C_MAJOR, QUARTER)], 2);
    serde_json::from_str(&json_of(&TonalContext::measure(
        &score,
        EvidenceScope::Track(0),
    )))
    .expect("valid JSON")
}

fn refuses(envelope: &serde_json::Value, why: &str) {
    let text = envelope.to_string();
    assert!(
        serde_json::from_str::<TonalContext>(&text).is_err(),
        "must refuse ({why}): {text}"
    );
}

#[test]
fn a_foreign_field_is_refused_at_every_level_of_the_artifact() {
    // The whole artifact is fail-closed, not just its top two floors: an
    // envelope that silently absorbs unknown fields cannot be trusted to mean
    // what it says.
    let paths: [&[&str]; 6] = [
        &[],
        &["evidence"],
        &["evidence", "scope"],
        &["projection"],
        &["projection", "winner"],
        &["provenance"],
    ];
    for path in paths {
        let mut envelope = valid_envelope();
        let mut cursor = &mut envelope;
        for step in path {
            cursor = cursor.get_mut(*step).expect("path exists");
        }
        cursor
            .as_object_mut()
            .expect("object")
            .insert("confident".to_string(), serde_json::json!(true));
        refuses(&envelope, &format!("foreign field at {path:?}"));
    }
}

#[test]
fn a_scope_whose_kind_and_payload_disagree_is_refused() {
    for scope in [
        // A kind with no payload where one is required, and vice versa.
        serde_json::json!({"kind": "track"}),
        serde_json::json!({"kind": "voice", "track": 0}),
        serde_json::json!({"kind": "whole_score", "track": 0}),
        serde_json::json!({"kind": "track", "track": 0, "voice": 1}),
        serde_json::json!({"kind": "part", "track": 0}),
    ] {
        let mut envelope = valid_envelope();
        envelope["evidence"]["scope"] = scope.clone();
        refuses(&envelope, &format!("scope shape {scope}"));
    }
}

// ── fail-closed: a semantically impossible envelope is refused ───────────────

#[test]
fn an_envelope_that_abstains_but_claims_a_weighting_is_refused() {
    // The exact hole the review found: `deny_unknown_fields` guards field
    // *names*, never contradictory field *values*.
    let mut envelope = valid_envelope();
    envelope["projection"] = serde_json::Value::Null;
    refuses(&envelope, "no projection, but a weighting is claimed");
}

#[test]
fn an_envelope_that_projects_but_claims_no_weighting_is_refused() {
    let mut envelope = valid_envelope();
    envelope["provenance"]["weighting"] = serde_json::Value::Null;
    refuses(
        &envelope,
        "a projection with nothing said to have weighted it",
    );
}

#[test]
fn an_envelope_that_projects_from_a_silent_scope_is_refused() {
    let mut envelope = valid_envelope();
    envelope["evidence"]["note_count"] = serde_json::json!(0);
    refuses(&envelope, "a projection over zero observed notes");
}

#[test]
fn an_evidence_that_contradicts_its_own_histogram_is_refused() {
    let mut envelope = valid_envelope();
    envelope["evidence"]["note_count"] = serde_json::json!(7); // the histogram sums to 8
    refuses(&envelope, "note_count disagrees with the onset histogram");
}

#[test]
fn an_evidence_that_sounds_but_observed_no_range_is_refused() {
    let mut envelope = valid_envelope();
    envelope["evidence"]["pitch_range"] = serde_json::Value::Null;
    refuses(&envelope, "notes observed but no pitch range");
}

#[test]
fn an_inverted_or_out_of_range_pitch_span_is_refused() {
    for range in [
        serde_json::json!({"lowest": 72, "highest": 60}),
        serde_json::json!({"lowest": 60, "highest": 200}),
    ] {
        let mut envelope = valid_envelope();
        envelope["evidence"]["pitch_range"] = range.clone();
        refuses(&envelope, &format!("pitch range {range}"));
    }
}

#[test]
fn a_candidate_outside_its_declared_ranges_is_refused() {
    for (path, value, why) in [
        (["winner", "tonic"], serde_json::json!(12), "tonic above B"),
        (
            ["winner", "scale_fit"],
            serde_json::json!(1.5),
            "scale_fit above 1",
        ),
        (
            ["winner", "scale_fit"],
            serde_json::json!(-0.1),
            "scale_fit below 0",
        ),
        (
            ["runner_up", "tonic"],
            serde_json::json!(99),
            "rival tonic out of range",
        ),
    ] {
        let mut envelope = valid_envelope();
        envelope["projection"][path[0]][path[1]] = value;
        refuses(&envelope, why);
    }
}

#[test]
fn a_margin_that_is_negative_or_does_not_match_its_candidates_is_refused() {
    for (margin, why) in [
        (serde_json::json!(-0.5), "a negative margin"),
        (serde_json::json!(0.5), "a margin that is not the gap"),
    ] {
        let mut envelope = valid_envelope();
        envelope["projection"]["confidence_margin"] = margin;
        refuses(&envelope, why);
    }
}

#[test]
fn an_envelope_whose_projection_contradicts_its_evidence_is_refused() {
    // The catch-all, and the point of carrying the evidence: the artifact is
    // re-derived from the material it carries and compared, so a well-formed
    // projection that simply is not what that evidence yields cannot pass.
    let score = two_key_score();
    let c_major = TonalContext::measure(&score, EvidenceScope::Track(0));
    let fs_major = TonalContext::measure(&score, EvidenceScope::Track(1));

    let mut envelope: serde_json::Value =
        serde_json::from_str(&json_of(&c_major)).expect("valid JSON");
    let other: serde_json::Value = serde_json::from_str(&json_of(&fs_major)).expect("valid JSON");

    // A perfectly well-formed F# major projection, over C major's evidence.
    envelope["projection"] = other["projection"].clone();
    refuses(&envelope, "the projection is not what this evidence yields");
}

#[test]
fn an_envelope_claiming_the_wrong_weighting_branch_is_refused() {
    // The evidence carries duration mass, so KS v1 cannot have taken the
    // onset-count fallback.
    let mut envelope = valid_envelope();
    envelope["provenance"]["weighting"] = serde_json::json!("onset_counts");
    refuses(&envelope, "a fallback that this evidence never triggered");
}

#[test]
fn an_unknown_method_is_refused_rather_than_read_as_the_current_one() {
    let mut envelope = valid_envelope();
    envelope["provenance"]["method"] = serde_json::json!("ks_v2");
    refuses(&envelope, "a method this build does not implement");
}

#[test]
fn a_valid_envelope_still_deserialises_whatever_its_key_order() {
    // The guard rail above must not have closed the road. `serde_json::Value`
    // re-emits object keys in sorted order, so this also pins that validation
    // reads the document rather than pattern-matching our own byte layout.
    let score = score_of(vec![track_of(&C_MAJOR, QUARTER)], 2);
    let context = TonalContext::measure(&score, EvidenceScope::Track(0));
    let reordered = valid_envelope().to_string();

    assert_ne!(
        reordered,
        json_of(&context),
        "the fixture is genuinely reordered"
    );
    let back: TonalContext =
        serde_json::from_str(&reordered).expect("a valid artifact deserialises");
    assert_eq!(back, context);
}

// ── the root of trust: evidence no measurement could have produced ───────────
//
// Re-derivation proves the *estimate* follows from the evidence. It says
// nothing about whether the evidence itself is possible, and the estimator only
// ever reads the two histograms — never `pitch_range`, and never the two
// histograms against each other. So a document can forge the facts, recompute
// an impeccable estimate from the forgery, and prove itself right about a lie.
// `PitchEvidence::validate` is the one gate that closes it, on both doors: the
// wire boundary and the public `TonalContext::from_evidence`.

/// Evidence built by hand rather than measured — the shape a forgery takes.
fn forged_evidence(
    note_count: usize,
    onset_counts: [u32; 12],
    duration_mass: [u64; 12],
    span: Option<(u8, u8)>,
) -> PitchEvidence {
    PitchEvidence {
        scope: EvidenceScope::Track(0),
        note_count,
        onset_counts,
        duration_mass,
        pitch_range: span.map(|(lowest, highest)| PitchRange {
            lowest: Pitch::new(lowest).expect("valid pitch"),
            highest: Pitch::new(highest).expect("valid pitch"),
        }),
    }
}

fn one_at(index: usize, value: u32) -> [u32; 12] {
    let mut histogram = [0_u32; 12];
    histogram[index] = value;
    histogram
}

fn mass_at(index: usize, value: u64) -> [u64; 12] {
    let mut histogram = [0_u64; 12];
    histogram[index] = value;
    histogram
}

/// Refuses through *both* doors: the public constructor and the wire form.
fn refuses_evidence(evidence: &PitchEvidence, why: &str) {
    assert!(
        TonalContext::from_evidence(evidence).is_err(),
        "from_evidence must refuse ({why})"
    );

    let mut envelope = valid_envelope();
    envelope["evidence"] = json!({
        "scope": {"kind": "track", "track": 0},
        "note_count": evidence.note_count,
        "onset_counts": evidence.onset_counts,
        "duration_mass": evidence.duration_mass,
        "pitch_range": evidence.pitch_range.map(|r| json!({
            "lowest": r.lowest.0, "highest": r.highest.0
        })),
    });
    refuses(&envelope, why);
}

#[test]
fn a_silent_scope_that_carries_duration_mass_is_refused() {
    // Nothing sounded, yet a pitch class holds sounded ticks.
    refuses_evidence(
        &forged_evidence(0, [0; 12], mass_at(0, 1), None),
        "duration mass with no notes at all",
    );
}

#[test]
fn duration_mass_for_a_pitch_class_that_never_sounded_is_refused() {
    refuses_evidence(
        &forged_evidence(1, one_at(0, 1), mass_at(6, 480), Some((60, 60))),
        "F# holds the duration while only C ever sounded",
    );
}

#[test]
fn duration_mass_beyond_what_its_onsets_could_produce_is_refused() {
    // One onset can contribute at most one `u32` of ticks.
    refuses_evidence(
        &forged_evidence(1, one_at(0, 1), mass_at(0, u64::MAX), Some((60, 60))),
        "more sounded ticks than one note can hold",
    );
}

#[test]
fn an_onset_outside_the_observed_pitch_range_is_refused() {
    // C and F# both sounded, but the observed span is a single C: F# has no
    // representative anywhere in `60..=60`.
    let mut onsets = one_at(0, 1);
    onsets[6] = 1;
    let mut mass = mass_at(0, 480);
    mass[6] = 480;
    refuses_evidence(
        &forged_evidence(2, onsets, mass, Some((60, 60))),
        "a pitch class that cannot fit inside the observed span",
    );
}

#[test]
fn a_range_endpoint_that_never_sounded_is_refused() {
    // The span claims to reach D4, but no D was ever counted — an endpoint is
    // by definition an observed note.
    refuses_evidence(
        &forged_evidence(1, one_at(0, 1), mass_at(0, 480), Some((60, 62))),
        "a span endpoint absent from the onset histogram",
    );
}

#[test]
fn an_evidence_forgery_is_refused_even_when_its_projection_is_honest() {
    // The nastiest shape: forge the facts, then compute the estimate *honestly*
    // from the forgery. Re-derivation is satisfied — the document really does
    // follow from its own stated evidence — so only validating the evidence
    // itself can catch it.
    let forged = forged_evidence(1, one_at(0, 1), mass_at(6, 480), Some((60, 60)));
    let estimate = estimate_key(&forged).expect("the forged facts still estimate");
    let winner = estimate.candidates[0];
    let rival = estimate.candidates[1];

    let candidate = |c: griff_core::tonal::TonalCandidate| {
        json!({
            "tonic": c.tonic,
            "mode": if c.mode == KeyMode::Major { "major" } else { "minor" },
            "correlation": c.correlation,
            "scale_fit": c.scale_fit,
        })
    };

    let mut envelope = valid_envelope();
    envelope["evidence"] = json!({
        "scope": {"kind": "track", "track": 0},
        "note_count": 1,
        "onset_counts": forged.onset_counts,
        "duration_mass": forged.duration_mass,
        "pitch_range": {"lowest": 60, "highest": 60},
    });
    envelope["projection"] = json!({
        "winner": candidate(winner),
        "runner_up": candidate(rival),
        "confidence_margin": estimate.confidence_margin,
    });
    envelope["provenance"] = json!({"method": "ks_v1", "weighting": "duration_mass"});

    refuses(&envelope, "impossible facts, impeccably reasoned from");
}

#[test]
fn an_overflowing_duration_histogram_is_refused_rather_than_panicking() {
    let mut envelope = valid_envelope();
    envelope["evidence"]["duration_mass"] =
        json!([u64::MAX, u64::MAX, u64::MAX, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    refuses(&envelope, "a duration histogram whose total does not fit u64");
}

#[test]
fn estimating_from_an_overflowing_duration_histogram_does_not_panic() {
    // `estimate_key` is public and `PitchEvidence` is a plain struct with public
    // fields, so the estimator must not assume the histogram is summable. This
    // is the "no unchecked integer sum on untrusted evidence" half — a panic
    // here would be a crash where a typed refusal was promised.
    let evidence = PitchEvidence {
        scope: EvidenceScope::WholeScore,
        note_count: 3,
        onset_counts: [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        duration_mass: [u64::MAX, u64::MAX, u64::MAX, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        pitch_range: Some(PitchRange {
            lowest: Pitch::new(60).expect("valid"),
            highest: Pitch::new(60).expect("valid"),
        }),
    };

    let estimate = estimate_key(&evidence).expect("a saturating total still estimates");
    assert_eq!(estimate.candidates.len(), 24);
    for candidate in &estimate.candidates {
        assert!(
            candidate.correlation.is_finite(),
            "correlations stay finite at the top of the u64 range"
        );
    }
}

#[test]
fn a_valid_measurement_passes_both_doors() {
    // The gate must not have closed the road for real measurements.
    let score = two_key_score();
    let evidence = PitchEvidence::measure(&score, EvidenceScope::Track(0));

    assert!(evidence.validate().is_ok());
    assert_eq!(
        TonalContext::from_evidence(&evidence).expect("valid evidence projects"),
        TonalContext::measure(&score, EvidenceScope::Track(0)),
        "both doors reach the same context"
    );
}

/// A track whose notes carry their own `(pitch, duration)`.
fn track_of_pairs(notes: &[(u8, u32)]) -> Track {
    Track {
        name: None,
        channel: 0,
        voices: vec![Voice {
            id: 0,
            event_groups: notes
                .iter()
                .enumerate()
                .map(|(i, &(pitch, duration))| EventGroup {
                    kind: EventGroupKind::Single,
                    atoms: vec![note(
                        u32::try_from(i).unwrap() * QUARTER,
                        pitch,
                        duration,
                    )],
                    technique_spans: Vec::new(),
                })
                .collect(),
        }],
        tuning: Tuning::standard_e(),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Whatever `PitchEvidence::measure` produces, the validator accepts. The
    /// gate constrains forgeries, not reality — and this is the check that a
    /// stricter invariant cannot quietly start rejecting real scores.
    #[test]
    fn measured_evidence_always_validates(
        notes in prop::collection::vec((0_u8..=127_u8, 0_u32..=4096_u32), 0..40)
    ) {
        let score = score_of(vec![track_of_pairs(&notes)], 1);
        for scope in [
            EvidenceScope::WholeScore,
            EvidenceScope::Track(0),
            EvidenceScope::Voice { track: 0, voice: 0 },
            EvidenceScope::Track(9),
        ] {
            let evidence = PitchEvidence::measure(&score, scope);
            prop_assert!(
                evidence.validate().is_ok(),
                "measured evidence must validate at {scope:?}: {:?}",
                evidence.validate()
            );
            prop_assert!(TonalContext::from_evidence(&evidence).is_ok());
        }
    }
}

// ── generation: the context is carried and ignored ───────────────────────────

/// Two bars of quarter notes on one track — enough material to seed a pass.
fn generation_source() -> Score {
    score_of(vec![track_of(&C_MAJOR, QUARTER)], 2)
}

// Carrying the evidence made `TonalContext` a 248-byte `Copy` type, so a
// by-value parameter trips the size lint. Production code never does this:
// `GenerationAsk` is always taken by reference (`ranked_candidates`), and this
// is a fixture builder.
#[allow(clippy::large_types_passed_by_value)]
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
