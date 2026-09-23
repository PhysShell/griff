#![allow(clippy::unwrap_used, clippy::missing_assert_message)]

use griff_constraint_lab::boundary_context::{
    condition_consumer_chain, consume_for_line, decode_context, encode_context, produce_context,
    BoundaryContext, BoundaryContextError, HandState, ProjectedTechnique, SolvedNote,
    SolvedPartition, TechniqueKind, VoiceIdentity,
};
use griff_constraint_lab::ties::{lexicographic_path, Chain, Features, FEATURES};
use griff_core::{
    event::{FretboardPosition, Pitch, Tuning},
    fretboard::FingeringWeights,
};

fn voice() -> VoiceIdentity {
    VoiceIdentity::new("song.gp5", 1, 0)
}

fn partition(position: FretboardPosition) -> SolvedPartition {
    SolvedPartition::new(voice(), vec![SolvedNote::new(10, 120, position, false)]).unwrap()
}

fn relation() -> ProjectedTechnique {
    ProjectedTechnique::new(10, 120, 42, TechniqueKind::Legato)
}

#[test]
fn producer_depends_only_on_solved_prefix() {
    let previous = BoundaryContext::unknown(voice());
    let prefix = partition(FretboardPosition { string: 3, fret: 7 });
    let first_target_reference = FretboardPosition { string: 3, fret: 9 };
    let mutated_target_reference = FretboardPosition { string: 1, fret: 0 };

    let first = produce_context(&previous, &prefix, &[relation()]).unwrap();
    let second = produce_context(&previous, &prefix, &[relation()]).unwrap();
    assert_ne!(first_target_reference, mutated_target_reference);
    assert_eq!(
        encode_context(&first).unwrap(),
        encode_context(&second).unwrap()
    );
}

#[test]
fn obligation_matches_stable_identity_after_reindexing() {
    let context = produce_context(
        &BoundaryContext::unknown(voice()),
        &partition(FretboardPosition { string: 3, fret: 7 }),
        &[relation()],
    )
    .unwrap();
    let first = consume_for_line(context.clone(), &voice(), &[41, 42, 43]).unwrap();
    let reindexed = consume_for_line(context, &voice(), &[99, 41, 7, 42, 43]).unwrap();
    assert_eq!(first.consumed()[0].target_note_id(), 42);
    assert_eq!(reindexed.consumed()[0].target_note_id(), 42);
    assert_eq!(first.consumed()[0].required_string(), 3);
    assert_eq!(first.remaining().pending().len(), 0);
}

#[test]
fn obligation_is_consumed_once_while_hand_lives_until_updated() {
    let context = produce_context(
        &BoundaryContext::unknown(voice()),
        &partition(FretboardPosition { string: 3, fret: 7 }),
        &[relation()],
    )
    .unwrap();
    let consumed = consume_for_line(context, &voice(), &[42]).unwrap();
    assert_eq!(consumed.consumed().len(), 1);
    assert_eq!(consumed.remaining().hand().anchor_fret().unwrap(), Some(7));
    let next = consume_for_line(consumed.into_remaining(), &voice(), &[42, 50]).unwrap();
    assert!(next.consumed().is_empty());
    assert_eq!(next.remaining().hand().anchor_fret().unwrap(), Some(7));
}

#[test]
fn unknown_hand_is_not_musical_absence_or_fret_zero() {
    let unknown = HandState::Unknown;
    let absent = HandState::Absent;
    let zero = HandState::known(0, 1, 10);
    assert_eq!(
        unknown.anchor_fret(),
        Err(BoundaryContextError::UnknownHand)
    );
    assert_eq!(absent.anchor_fret().unwrap(), None);
    assert_eq!(zero.anchor_fret().unwrap(), Some(0));
    assert_ne!(
        serde_json::to_vec(&unknown).unwrap(),
        serde_json::to_vec(&absent).unwrap()
    );
}

#[test]
fn transport_round_trip_is_lossless_deterministic_and_canonical() {
    let prefix = partition(FretboardPosition { string: 3, fret: 7 });
    let a = ProjectedTechnique::new(10, 120, 42, TechniqueKind::Legato);
    let b = ProjectedTechnique::new(10, 120, 35, TechniqueKind::HammerOn);
    let left = produce_context(&BoundaryContext::unknown(voice()), &prefix, &[a, b]).unwrap();
    let right = produce_context(&BoundaryContext::unknown(voice()), &prefix, &[b, a]).unwrap();
    let bytes = encode_context(&left).unwrap();
    assert_eq!(bytes, encode_context(&right).unwrap());
    let decoded = decode_context(&bytes).unwrap();
    assert_eq!(left, decoded);
    assert_eq!(bytes, encode_context(&decoded).unwrap());
    assert_eq!(
        consume_for_line(left, &voice(), &[35, 42]).unwrap(),
        consume_for_line(decoded, &voice(), &[35, 42]).unwrap()
    );
}

#[test]
fn wrong_voice_and_ambiguous_target_fail_closed() {
    let context = produce_context(
        &BoundaryContext::unknown(voice()),
        &partition(FretboardPosition { string: 3, fret: 7 }),
        &[relation()],
    )
    .unwrap();
    let other = VoiceIdentity::new("song.gp5", 1, 1);
    assert_eq!(
        consume_for_line(context.clone(), &other, &[42]),
        Err(BoundaryContextError::VoiceMismatch)
    );
    assert_eq!(
        consume_for_line(context, &voice(), &[42, 42]),
        Err(BoundaryContextError::AmbiguousTarget(42))
    );
}

#[test]
fn serialized_fresh_consumer_matches_direct_causal_lht_without_oracle_input() {
    let context = produce_context(
        &BoundaryContext::unknown(voice()),
        &partition(FretboardPosition { string: 2, fret: 5 }),
        &[relation()],
    )
    .unwrap();
    let bytes = encode_context(&context).unwrap();
    let base = Chain::v1(
        &[Pitch(64), Pitch(67)],
        &Tuning::standard_e(),
        &FingeringWeights::v1(),
        24,
    )
    .unwrap();
    let note_ids = [42, 50];
    let direct = consume_for_line(context, &voice(), &note_ids).unwrap();
    let fresh = consume_for_line(decode_context(&bytes).unwrap(), &voice(), &note_ids).unwrap();
    let direct_chain = condition_consumer_chain(base.clone(), &note_ids, &direct)
        .unwrap()
        .with_anchor(direct.anchor_fret().unwrap());
    let fresh_chain = condition_consumer_chain(base, &note_ids, &fresh)
        .unwrap()
        .with_anchor(fresh.anchor_fret().unwrap());
    let mut anchor: Features = [0; FEATURES];
    anchor[FEATURES - 1] = 1;
    let direct_path = direct_chain.positions_of(&lexicographic_path(&direct_chain, &anchor, None));
    let fresh_path = fresh_chain.positions_of(&lexicographic_path(&fresh_chain, &anchor, None));
    assert_eq!(direct_path, fresh_path);

    let oracle_reference = [
        FretboardPosition { string: 2, fret: 5 },
        FretboardPosition { string: 3, fret: 5 },
    ];
    let mutated_reference = [
        FretboardPosition { string: 1, fret: 0 },
        FretboardPosition {
            string: 6,
            fret: 15,
        },
    ];
    assert_ne!(oracle_reference, mutated_reference);
    assert_eq!(direct_path, fresh_path);
}
