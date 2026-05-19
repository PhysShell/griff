//! Bar-level style classification.
//!
//! Assigns a broad playing-style label to a single [`Bar`] using simple
//! heuristics (note density, average velocity, pitch span).  The thresholds
//! are intentionally coarse; they are meant to be a first-pass triage, not a
//! ground-truth classifier.

use std::fmt;

use crate::event::{Bar, Event};

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

/// Computes per-bar metrics without allocating.
pub fn bar_features(bar: &Bar) -> BarFeatures {
    let mut note_count: usize = 0;
    let mut vel_sum: u32 = 0;
    let mut lowest: Option<u8> = None;
    let mut highest: Option<u8> = None;

    for event in &bar.events {
        if let Event::Note(note) = event {
            note_count = note_count.saturating_add(1);
            vel_sum = vel_sum.saturating_add(u32::from(note.velocity.0));
            let pitch = note.pitch.0;
            lowest = Some(lowest.map_or(pitch, |lo| lo.min(pitch)));
            highest = Some(highest.map_or(pitch, |hi| hi.max(pitch)));
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
#[allow(clippy::expect_used)]
mod tests {
    use super::{bar_features, classify_bar, BarClass, BarFeatures};
    use crate::event::{
        Articulation, Bar, Event, Note, Pitch, Rest, Tempo, Ticks, TimeSignature, Velocity,
    };

    fn test_bar(events: Vec<Event>) -> Bar {
        Bar {
            time_signature: TimeSignature {
                numerator: 4,
                denominator: 4,
            },
            tempo: Tempo::new(120.0).expect("120 BPM valid"),
            events,
        }
    }

    fn note(pitch: u8, vel: u8, dur: u32) -> Event {
        Event::Note(Note {
            pitch: Pitch::new(pitch).expect("valid pitch"),
            velocity: Velocity::new(vel).expect("valid velocity"),
            duration: Ticks(dur),
            articulation: None::<Articulation>,
        })
    }

    fn rest(dur: u32) -> Event {
        Event::Rest(Rest {
            duration: Ticks(dur),
        })
    }

    // ── bar_features ──────────────────────────────────────────────────────────

    #[test]
    fn bar_features_empty_bar_gives_zeros() {
        let f = bar_features(&test_bar(vec![]));
        assert_eq!(
            f,
            BarFeatures {
                note_count: 0,
                avg_velocity: 0,
                pitch_span: 0
            }
        );
    }

    #[test]
    fn bar_features_rests_only_gives_zeros() {
        let f = bar_features(&test_bar(vec![rest(480), rest(480)]));
        assert_eq!(
            f,
            BarFeatures {
                note_count: 0,
                avg_velocity: 0,
                pitch_span: 0
            }
        );
    }

    #[test]
    fn bar_features_single_note() {
        let f = bar_features(&test_bar(vec![note(60, 90, 480)]));
        assert_eq!(f.note_count, 1);
        assert_eq!(f.avg_velocity, 90);
        assert_eq!(f.pitch_span, 0);
    }

    #[test]
    fn bar_features_computes_avg_velocity() {
        // two notes: vel 60 + 100 → avg 80
        let f = bar_features(&test_bar(vec![note(60, 60, 240), note(62, 100, 240)]));
        assert_eq!(f.avg_velocity, 80);
    }

    #[test]
    fn bar_features_computes_pitch_span() {
        // E2 (40) to E5 (64) = 24 semitones
        let f = bar_features(&test_bar(vec![note(40, 80, 240), note(64, 80, 240)]));
        assert_eq!(f.pitch_span, 24);
    }

    #[test]
    fn bar_features_ignores_rests_in_counts() {
        let f = bar_features(&test_bar(vec![
            note(60, 80, 240),
            rest(240),
            note(62, 80, 240),
        ]));
        assert_eq!(f.note_count, 2);
    }

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
