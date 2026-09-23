//! Label-safe, transparent estimators for technique-origin string recovery.
//!
//! Research-only: observed tablature is deliberately represented by a
//! different type from estimator inputs.

use std::collections::BTreeSet;

use griff_core::event::{FretboardPosition, Pitch, Tuning};
use serde::{Deserialize, Serialize};

use crate::ties::{optimum_set, Chain};

/// Stable identity of one projected origin/target relation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OriginIdentity {
    source: String,
    track: usize,
    voice: usize,
    origin_note_id: usize,
    target_note_id: usize,
    origin_onset: u32,
    target_onset: u32,
}

impl OriginIdentity {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        source: impl Into<String>,
        track: usize,
        voice: usize,
        origin_note_id: usize,
        target_note_id: usize,
        origin_onset: u32,
        target_onset: u32,
    ) -> Self {
        Self {
            source: source.into(),
            track,
            voice,
            origin_note_id,
            target_note_id,
            origin_onset,
            target_onset,
        }
    }
}

/// BLIND input. It cannot represent target score information or either
/// endpoint's observed realization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlindOriginProblem {
    identity: OriginIdentity,
    tuning: Vec<u8>,
    max_fret: u8,
    origin_pitch: u8,
}

impl BlindOriginProblem {
    #[must_use]
    pub fn new(
        identity: OriginIdentity,
        tuning: &Tuning,
        max_fret: u8,
        origin_pitch: Pitch,
    ) -> Self {
        Self {
            identity,
            tuning: tuning.open_strings().iter().map(|pitch| pitch.0).collect(),
            max_fret,
            origin_pitch: origin_pitch.0,
        }
    }
}

/// Score-level relation intent supplied by an upstream planner.
#[allow(clippy::struct_field_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TechniqueIntent {
    target_note_id: usize,
    target_pitch: u8,
    target_onset: u32,
}

impl TechniqueIntent {
    #[must_use]
    pub const fn new(target_note_id: usize, target_pitch: Pitch, target_onset: u32) -> Self {
        Self {
            target_note_id,
            target_pitch: target_pitch.0,
            target_onset,
        }
    }
}

/// `INTENT_AWARE` input. Target realization remains structurally absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntentAwareOriginProblem {
    blind: BlindOriginProblem,
    intent: TechniqueIntent,
}

impl IntentAwareOriginProblem {
    #[must_use]
    pub fn new(blind: BlindOriginProblem, intent: TechniqueIntent) -> Self {
        Self { blind, intent }
    }
}

/// Evaluation-only imported origin position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObservedOriginRealization(FretboardPosition);

impl ObservedOriginRealization {
    #[must_use]
    pub const fn new(position: FretboardPosition) -> Self {
        Self(position)
    }

    #[must_use]
    pub const fn position(self) -> FretboardPosition {
        self.0
    }
}

/// Causal hand state available before the origin decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HandEstimate {
    Unknown,
    Absent,
    Known(u8),
}

/// Epistemic result: deterministic implementation order is not certainty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum OriginStringEstimate {
    Known { string: u8 },
    Ambiguous { strings: Vec<u8> },
    Unsupported { reason: String },
}

impl OriginStringEstimate {
    #[must_use]
    pub fn strings(&self) -> &[u8] {
        match self {
            Self::Known { string } => std::slice::from_ref(string),
            Self::Ambiguous { strings } => strings,
            Self::Unsupported { .. } => &[],
        }
    }
}

/// Exact primary optimum after fixing the origin to one physical string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StringProfileEntry {
    string: u8,
    fret: u8,
    cost: i64,
    delta: i64,
    dense_rank: usize,
}

impl StringProfileEntry {
    #[must_use]
    pub const fn string(self) -> u8 {
        self.string
    }

    #[must_use]
    pub const fn cost(self) -> i64 {
        self.cost
    }

    #[must_use]
    pub const fn delta(self) -> i64 {
        self.delta
    }

    #[must_use]
    pub const fn dense_rank(self) -> usize {
        self.dense_rank
    }
}

/// Complete exact origin-string landscape for one line note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OriginStringProfile {
    optimum: i64,
    entries: Vec<StringProfileEntry>,
}

impl OriginStringProfile {
    #[must_use]
    pub fn entries(&self) -> &[StringProfileEntry] {
        &self.entries
    }
}

/// Solve the frozen primary chain once per legal string.
#[must_use]
pub fn conditioned_profile(chain: &Chain, note: usize) -> OriginStringProfile {
    let mut strings = BTreeSet::new();
    for candidate in chain.candidates(note) {
        strings.insert((candidate.string, candidate.fret));
    }
    let mut raw = Vec::with_capacity(strings.len());
    for (string, fret) in strings {
        if let Some(conditioned) = chain.clone().condition_string(note, string) {
            raw.push((string, fret, optimum_set(&conditioned, None).optimum));
        }
    }
    let optimum = raw.iter().map(|entry| entry.2).min().unwrap_or(0);
    let costs: BTreeSet<_> = raw.iter().map(|entry| entry.2).collect();
    let entries = raw
        .into_iter()
        .map(|(string, fret, cost)| StringProfileEntry {
            string,
            fret,
            cost,
            delta: cost.saturating_sub(optimum),
            dense_rank: costs.iter().take_while(|&&other| other < cost).count() + 1,
        })
        .collect();
    OriginStringProfile { optimum, entries }
}

fn eligible<'a>(
    profile: &'a OriginStringProfile,
    allowed: Option<&[u8]>,
) -> Vec<&'a StringProfileEntry> {
    let best = profile
        .entries
        .iter()
        .filter(|entry| allowed.is_none_or(|set| set.contains(&entry.string)))
        .map(|entry| entry.cost)
        .min();
    profile
        .entries
        .iter()
        .filter(|entry| {
            Some(entry.cost) == best && allowed.is_none_or(|set| set.contains(&entry.string))
        })
        .collect()
}

fn typed(mut strings: Vec<u8>) -> OriginStringEstimate {
    strings.sort_unstable();
    strings.dedup();
    match strings.as_slice() {
        [] => OriginStringEstimate::Unsupported {
            reason: "empty candidate domain".into(),
        },
        [string] => OriginStringEstimate::Known { string: *string },
        _ => OriginStringEstimate::Ambiguous { strings },
    }
}

/// Primary-only estimate, optionally after a transparent domain restriction.
#[must_use]
pub fn estimate_primary(
    profile: &OriginStringProfile,
    allowed: Option<&[u8]>,
) -> OriginStringEstimate {
    typed(
        eligible(profile, allowed)
            .into_iter()
            .map(|entry| entry.string)
            .collect(),
    )
}

/// Primary then hand-distance lexicographic estimate.
#[must_use]
pub fn estimate_with_hand(
    profile: &OriginStringProfile,
    allowed: Option<&[u8]>,
    hand: HandEstimate,
) -> OriginStringEstimate {
    let primary = eligible(profile, allowed);
    let HandEstimate::Known(anchor) = hand else {
        return typed(primary.into_iter().map(|entry| entry.string).collect());
    };
    let best = primary
        .iter()
        .map(|entry| entry.fret.abs_diff(anchor))
        .min();
    typed(
        primary
            .into_iter()
            .filter(|entry| Some(entry.fret.abs_diff(anchor)) == best)
            .map(|entry| entry.string)
            .collect(),
    )
}

/// Origin strings on which both endpoint pitches are physically playable.
#[must_use]
pub fn technique_feasible_strings(
    tuning: &Tuning,
    max_fret: u8,
    origin: Pitch,
    target: Pitch,
) -> Vec<u8> {
    let origin: BTreeSet<_> = tuning
        .candidates(origin, max_fret)
        .into_iter()
        .map(|position| position.string)
        .collect();
    let target: BTreeSet<_> = tuning
        .candidates(target, max_fret)
        .into_iter()
        .map(|position| position.string)
        .collect();
    origin.intersection(&target).copied().collect()
}
