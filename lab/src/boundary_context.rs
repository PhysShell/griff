//! Typed Lab-only ownership and transport contract for state crossing solver
//! partitions. Producer inputs deliberately contain no target positions.

use std::collections::{BTreeMap, BTreeSet};

use griff_core::event::FretboardPosition;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ties::Chain;

/// Stable identity of one imported voice.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct VoiceIdentity {
    source: String,
    track: usize,
    voice: u8,
}

impl VoiceIdentity {
    /// Creates an imported-voice identity.
    #[must_use]
    pub fn new(source: impl Into<String>, track: usize, voice: u8) -> Self {
        Self {
            source: source.into(),
            track,
            voice,
        }
    }
}

/// Provenance for a known fretting-hand state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownHandState {
    fret: u8,
    source_note_id: usize,
    source_onset: u32,
}

/// Hand-state knowledge has three intentionally distinct meanings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum HandState {
    /// The producer cannot establish the state.
    Unknown,
    /// Music semantics explicitly establish no fretting-hand anchor.
    Absent,
    /// A solved causal note established the state.
    Known(KnownHandState),
}

impl HandState {
    /// Creates a known state with causal provenance.
    #[must_use]
    pub const fn known(fret: u8, source_note_id: usize, source_onset: u32) -> Self {
        Self::Known(KnownHandState {
            fret,
            source_note_id,
            source_onset,
        })
    }

    /// Returns the optional musical anchor, refusing epistemic unknown.
    ///
    /// # Errors
    ///
    /// Returns [`BoundaryContextError::UnknownHand`] for unknown state.
    pub const fn anchor_fret(self) -> Result<Option<u8>, BoundaryContextError> {
        match self {
            Self::Unknown => Err(BoundaryContextError::UnknownHand),
            Self::Absent => Ok(None),
            Self::Known(state) => Ok(Some(state.fret)),
        }
    }
}

/// Technique label transported with the obligation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TechniqueKind {
    /// Imported hammer-on-like span.
    HammerOn,
    /// Imported pull-off-like span.
    PullOff,
    /// Generic imported legato span.
    Legato,
}

/// Relation identity available to the producer; contains no target position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectedTechnique {
    origin_note_id: usize,
    origin_onset: u32,
    target_note_id: usize,
    kind: TechniqueKind,
}

impl ProjectedTechnique {
    /// Creates projected relation identity.
    #[must_use]
    pub const fn new(
        origin_note_id: usize,
        origin_onset: u32,
        target_note_id: usize,
        kind: TechniqueKind,
    ) -> Self {
        Self {
            origin_note_id,
            origin_onset,
            target_note_id,
            kind,
        }
    }
}

/// One chosen note in a completed solver partition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SolvedNote {
    note_id: usize,
    onset: u32,
    position: FretboardPosition,
    tapped: bool,
}

impl SolvedNote {
    /// Creates a causal solved-note record.
    #[must_use]
    pub const fn new(
        note_id: usize,
        onset: u32,
        position: FretboardPosition,
        tapped: bool,
    ) -> Self {
        Self {
            note_id,
            onset,
            position,
            tapped,
        }
    }
}

/// Completed partition input to the producer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolvedPartition {
    voice: VoiceIdentity,
    notes: Vec<SolvedNote>,
}

impl SolvedPartition {
    /// Validates stable note identity within one partition.
    ///
    /// # Errors
    ///
    /// Refuses duplicate stable note ids.
    pub fn new(voice: VoiceIdentity, notes: Vec<SolvedNote>) -> Result<Self, BoundaryContextError> {
        let mut ids = BTreeSet::new();
        for note in &notes {
            if !ids.insert(note.note_id) {
                return Err(BoundaryContextError::DuplicateSolvedNote(note.note_id));
            }
        }
        Ok(Self { voice, notes })
    }
}

/// Pending one-shot same-string obligation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PendingTechniqueObligation {
    origin_note_id: usize,
    origin_onset: u32,
    target_note_id: usize,
    required_string: u8,
    kind: TechniqueKind,
}

impl PendingTechniqueObligation {
    /// Stable target identity.
    #[must_use]
    pub const fn target_note_id(self) -> usize {
        self.target_note_id
    }

    /// String chosen for the causal origin by the producer.
    #[must_use]
    pub const fn required_string(self) -> u8 {
        self.required_string
    }
}

/// State owned between solver partitions of one imported voice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundaryContext {
    voice: VoiceIdentity,
    hand: HandState,
    pending: Vec<PendingTechniqueObligation>,
}

impl BoundaryContext {
    /// Starts transport with epistemically unknown hand state.
    #[must_use]
    pub fn unknown(voice: VoiceIdentity) -> Self {
        Self {
            voice,
            hand: HandState::Unknown,
            pending: Vec::new(),
        }
    }

    /// Current hand state.
    #[must_use]
    pub const fn hand(&self) -> HandState {
        self.hand
    }

    /// Canonically ordered pending obligations.
    #[must_use]
    pub fn pending(&self) -> &[PendingTechniqueObligation] {
        &self.pending
    }
}

/// Result of consuming obligations targeted at one fresh line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumedBoundary {
    consumed: Vec<PendingTechniqueObligation>,
    remaining: BoundaryContext,
}

impl ConsumedBoundary {
    /// Obligations matched by stable target identity.
    #[must_use]
    pub fn consumed(&self) -> &[PendingTechniqueObligation] {
        &self.consumed
    }

    /// Context after one-shot consumption.
    #[must_use]
    pub const fn remaining(&self) -> &BoundaryContext {
        &self.remaining
    }

    /// Moves the remaining context to the next partition.
    #[must_use]
    pub fn into_remaining(self) -> BoundaryContext {
        self.remaining
    }

    /// Hand anchor transported into this consumer.
    ///
    /// # Errors
    ///
    /// Returns [`BoundaryContextError::UnknownHand`] for unknown state.
    pub const fn anchor_fret(&self) -> Result<Option<u8>, BoundaryContextError> {
        self.remaining.hand.anchor_fret()
    }
}

/// Typed contract refusal.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BoundaryContextError {
    /// Hand knowledge is unavailable rather than musically absent.
    #[error("hand state is unknown")]
    UnknownHand,
    /// Context crossed between unrelated voices.
    #[error("boundary context voice mismatch")]
    VoiceMismatch,
    /// Stable id is duplicated in a solved partition.
    #[error("duplicate solved note id {0}")]
    DuplicateSolvedNote(usize),
    /// Projected relation origin is absent from the solved prefix.
    #[error("projected origin {0} is absent from solved partition")]
    MissingOrigin(usize),
    /// Target identity occurs more than once in a consumer line.
    #[error("ambiguous stable target id {0}")]
    AmbiguousTarget(usize),
    /// A consumed target is absent from the consumer identity vector.
    #[error("consumed target {0} is absent from consumer line")]
    MissingConsumedTarget(usize),
    /// Required string leaves no target candidate.
    #[error("required string is infeasible for target {0}")]
    InfeasibleTarget(usize),
    /// Two obligations disagree for one stable relation identity.
    #[error("conflicting obligation for target {0}")]
    ConflictingObligation(usize),
    /// Serialized context is malformed.
    #[error("invalid boundary context encoding: {0}")]
    Encoding(String),
}

fn canonicalize_pending(
    mut pending: Vec<PendingTechniqueObligation>,
) -> Result<Vec<PendingTechniqueObligation>, BoundaryContextError> {
    pending.sort_unstable();
    pending.dedup();
    let mut required_by_target = BTreeMap::new();
    for obligation in &pending {
        if let Some(required) =
            required_by_target.insert(obligation.target_note_id, obligation.required_string)
        {
            if required != obligation.required_string {
                return Err(BoundaryContextError::ConflictingObligation(
                    obligation.target_note_id,
                ));
            }
        }
    }
    Ok(pending)
}

/// Produces outgoing state using only a completed prefix and projected ids.
///
/// # Errors
///
/// Refuses voice mismatch, missing origins and conflicting obligations.
pub fn produce_context(
    previous: &BoundaryContext,
    partition: &SolvedPartition,
    relations: &[ProjectedTechnique],
) -> Result<BoundaryContext, BoundaryContextError> {
    if previous.voice != partition.voice {
        return Err(BoundaryContextError::VoiceMismatch);
    }
    let hand = partition
        .notes
        .iter()
        .filter(|note| !note.tapped && note.position.fret > 0)
        .max_by_key(|note| (note.onset, std::cmp::Reverse(note.position.fret)))
        .map_or(previous.hand, |note| {
            HandState::known(note.position.fret, note.note_id, note.onset)
        });
    let by_id: BTreeMap<usize, &SolvedNote> = partition
        .notes
        .iter()
        .map(|note| (note.note_id, note))
        .collect();
    let mut pending = previous.pending.clone();
    for relation in relations {
        let origin = by_id
            .get(&relation.origin_note_id)
            .ok_or(BoundaryContextError::MissingOrigin(relation.origin_note_id))?;
        pending.push(PendingTechniqueObligation {
            origin_note_id: relation.origin_note_id,
            origin_onset: relation.origin_onset,
            target_note_id: relation.target_note_id,
            required_string: origin.position.string,
            kind: relation.kind,
        });
    }
    let canonical = canonicalize_pending(pending)?;
    Ok(BoundaryContext {
        voice: previous.voice.clone(),
        hand,
        pending: canonical,
    })
}

/// Consumes obligations whose stable target occurs exactly once in this line.
///
/// # Errors
///
/// Refuses voice mismatch and ambiguous target identity.
pub fn consume_for_line(
    mut context: BoundaryContext,
    voice: &VoiceIdentity,
    note_ids: &[usize],
) -> Result<ConsumedBoundary, BoundaryContextError> {
    if &context.voice != voice {
        return Err(BoundaryContextError::VoiceMismatch);
    }
    let mut counts = BTreeMap::new();
    for &note_id in note_ids {
        *counts.entry(note_id).or_insert(0_usize) += 1;
    }
    for obligation in &context.pending {
        if counts.get(&obligation.target_note_id).copied().unwrap_or(0) > 1 {
            return Err(BoundaryContextError::AmbiguousTarget(
                obligation.target_note_id,
            ));
        }
    }
    let (consumed, pending): (Vec<_>, Vec<_>) = context
        .pending
        .into_iter()
        .partition(|obligation| counts.contains_key(&obligation.target_note_id));
    context.pending = pending;
    Ok(ConsumedBoundary {
        consumed,
        remaining: context,
    })
}

/// Applies consumed stable-id obligations to a fresh consumer chain.
///
/// Neither imported target positions nor a human reference are inputs.
///
/// # Errors
///
/// Refuses missing stable targets and infeasible required strings.
pub fn condition_consumer_chain(
    mut chain: Chain,
    note_ids: &[usize],
    consumed: &ConsumedBoundary,
) -> Result<Chain, BoundaryContextError> {
    for obligation in &consumed.consumed {
        let target = note_ids
            .iter()
            .position(|note_id| *note_id == obligation.target_note_id)
            .ok_or(BoundaryContextError::MissingConsumedTarget(
                obligation.target_note_id,
            ))?;
        chain = chain
            .condition_string(target, obligation.required_string)
            .ok_or(BoundaryContextError::InfeasibleTarget(
                obligation.target_note_id,
            ))?;
    }
    Ok(chain)
}

/// Encodes canonical transport bytes.
///
/// # Errors
///
/// Returns [`BoundaryContextError::Encoding`] on serialization failure.
pub fn encode_context(context: &BoundaryContext) -> Result<Vec<u8>, BoundaryContextError> {
    serde_json::to_vec(context).map_err(|error| BoundaryContextError::Encoding(error.to_string()))
}

/// Decodes and canonicalizes transport bytes.
///
/// # Errors
///
/// Returns [`BoundaryContextError::Encoding`] for malformed input.
pub fn decode_context(bytes: &[u8]) -> Result<BoundaryContext, BoundaryContextError> {
    let mut context: BoundaryContext = serde_json::from_slice(bytes)
        .map_err(|error| BoundaryContextError::Encoding(error.to_string()))?;
    context.pending = canonicalize_pending(context.pending)?;
    Ok(context)
}
