//! Bar-level style classification.
//!
//! Assigns a broad playing-style label to a single bar using simple heuristics
//! (note density, average velocity, pitch span). The thresholds are
//! intentionally coarse; they are meant to be a first-pass triage, not a
//! ground-truth classifier.
//!
//! Bars are a score-level [`MasterBar`](crate::score::MasterBar) concept in the
//! canonical model (ADR-0003): [`bar_features_in_range`] computes the metrics
//! over a [`Voice`]'s note atoms whose onset falls inside a half-open tick
//! range.

use std::fmt;
use std::slice::from_ref;

use crate::score::{AtomEvent, Voice};
use crate::slice::TickRange;

// ── types ─────────────────────────────────────────────────────────────────────

/// Broad playing-style category for a single bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BarClass {
    /// Power-chord or single-note chug: sparse, heavy velocity.
    Breakdown,
    /// Lightly-played notes: low velocity, typical of clean-picked phrases.
    Clean,
    /// Lead melody or solo spanning a wide pitch range.
    Solo,
    /// Fast rhythm picking or general riff material.
    Riff,
    /// Bar with no note events (silence / pure rests).
    Unknown,
}

impl fmt::Display for BarClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Breakdown => "Breakdown",
            Self::Clean => "Clean",
            Self::Solo => "Solo",
            Self::Riff => "Riff",
            Self::Unknown => "Unknown",
        };
        f.pad(label)
    }
}

/// Per-bar metrics used for classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarFeatures {
    /// Number of note events in the bar.
    pub note_count: usize,
    /// Average MIDI velocity across notes; 0 when there are no notes.
    pub avg_velocity: u8,
    /// Pitch range in semitones (highest − lowest); 0 when fewer than two distinct pitches.
    pub pitch_span: u8,
}

// ── public functions ──────────────────────────────────────────────────────────

/// Computes per-bar metrics over a [`Voice`]'s note atoms whose onset falls in
/// the half-open `range` `[start, end)`.
///
/// Rest atoms and notes outside the range are ignored. Every note atom of every
/// event group counts, so chord and arpeggio atoms each contribute. A thin
/// single-voice wrapper over [`bar_features_across_voices`].
pub fn bar_features_in_range(voice: &Voice, range: TickRange) -> BarFeatures {
    bar_features_across_voices(from_ref(voice), range)
}

/// Computes per-bar metrics over the note atoms of *several* voices whose onset
/// falls in the half-open `range` `[start, end)` — the multi-voice aggregate of
/// [`bar_features_in_range`].
///
/// Used when one logical part is split across voices (e.g. one [`Voice`] per
/// Guitar Pro voice) and a single classification is wanted over all of them, so
/// material in voice 2+ is not invisible to bar classification. Rest atoms and
/// notes outside the range are ignored.
pub fn bar_features_across_voices(voices: &[Voice], range: TickRange) -> BarFeatures {
    let mut note_count: usize = 0;
    let mut vel_sum: u32 = 0;
    let mut lowest: Option<u8> = None;
    let mut highest: Option<u8> = None;

    for voice in voices {
        for group in &voice.event_groups {
            for atom in &group.atoms {
                let AtomEvent::Note(note) = atom else {
                    continue;
                };
                let onset = note.absolute_start.0;
                if onset < range.start.0 || onset >= range.end.0 {
                    continue;
                }
                note_count = note_count.saturating_add(1);
                vel_sum = vel_sum.saturating_add(u32::from(note.velocity.0));
                let pitch = note.pitch.0;
                lowest = Some(lowest.map_or(pitch, |lo| lo.min(pitch)));
                highest = Some(highest.map_or(pitch, |hi| hi.max(pitch)));
            }
        }
    }

    let avg_velocity = if note_count == 0 {
        0_u8
    } else {
        let count32 = u32::try_from(note_count).unwrap_or(u32::MAX);
        // average velocity is always ≤ 127 (max MIDI velocity), truncation is safe
        #[allow(clippy::cast_possible_truncation, clippy::arithmetic_side_effects)]
        {
            (vel_sum / count32) as u8
        }
    };

    let pitch_span = match (lowest, highest) {
        (Some(lo), Some(hi)) => hi.saturating_sub(lo),
        _ => 0,
    };

    BarFeatures {
        note_count,
        avg_velocity,
        pitch_span,
    }
}

/// Assigns a [`BarClass`] using heuristic thresholds.
///
/// Priority order (first match wins):
/// 1. `Unknown` — no note events.
/// 2. `Breakdown` — sparse (≤ 4 notes) and heavy (avg vel ≥ 80).
/// 3. `Clean` — low average velocity (< 65).
/// 4. `Solo` — wide pitch span (≥ 24 semitones / two octaves).
/// 5. `Riff` — everything else.
pub const fn classify_bar(features: BarFeatures) -> BarClass {
    if features.note_count == 0 {
        return BarClass::Unknown;
    }
    if features.note_count <= 4 && features.avg_velocity >= 80 {
        return BarClass::Breakdown;
    }
    if features.avg_velocity < 65 {
        return BarClass::Clean;
    }
    if features.pitch_span >= 24 {
        return BarClass::Solo;
    }
    BarClass::Riff
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{classify_bar, BarClass, BarFeatures};

    // Note: `bar_features_in_range` (the canonical feature extractor) is
    // characterized in `core/tests/classify_canonical.rs`. These unit tests pin
    // the pure `classify_bar` thresholds, which operate on `BarFeatures` alone.

    // ── classify_bar ──────────────────────────────────────────────────────────

    #[test]
    fn classify_empty_is_unknown() {
        assert_eq!(
            classify_bar(BarFeatures {
                note_count: 0,
                avg_velocity: 0,
                pitch_span: 0
            }),
            BarClass::Unknown
        );
    }

    #[test]
    fn classify_sparse_heavy_is_breakdown() {
        assert_eq!(
            classify_bar(BarFeatures {
                note_count: 3,
                avg_velocity: 100,
                pitch_span: 7
            }),
            BarClass::Breakdown,
        );
    }

    #[test]
    fn classify_light_touch_is_clean() {
        // 8 notes but low velocity
        assert_eq!(
            classify_bar(BarFeatures {
                note_count: 8,
                avg_velocity: 60,
                pitch_span: 12
            }),
            BarClass::Clean,
        );
    }

    #[test]
    fn classify_wide_range_is_solo() {
        assert_eq!(
            classify_bar(BarFeatures {
                note_count: 6,
                avg_velocity: 85,
                pitch_span: 28
            }),
            BarClass::Solo,
        );
    }

    #[test]
    fn classify_dense_medium_is_riff() {
        assert_eq!(
            classify_bar(BarFeatures {
                note_count: 10,
                avg_velocity: 80,
                pitch_span: 12
            }),
            BarClass::Riff,
        );
    }

    #[test]
    fn classify_breakdown_takes_priority_over_clean_check() {
        // note_count ≤ 4 AND avg_vel ≥ 80 → Breakdown, even if vel < 65 condition would also match
        // (it can't match both, but the priority order is tested by: note_count=2, vel=80, span=0)
        assert_eq!(
            classify_bar(BarFeatures {
                note_count: 2,
                avg_velocity: 80,
                pitch_span: 0
            }),
            BarClass::Breakdown,
        );
    }
}
