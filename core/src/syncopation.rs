//! Deriving the `syncopated` rhythm tag from note-onset placement (#75).
//!
//! Tags [`SwancoreTag::Syncopated`] when a track's phrasing is *displaced*: the
//! off-beat eighth-note "and" just before a beat is struck while the beat tick
//! itself carries no onset — the classic anticipation / sustain-over feel. Steady
//! eighth- or sixteenth-note runs score zero (every beat is still struck), so the
//! metric does not over-tag busy-but-regular riffs.
//!
//! Unlike the threshold-free technique/harmony derivers, syncopation is a matter
//! of degree, so it leans on one documented constant
//! ([`SYNCOPATION_THRESHOLD`]) and otherwise stays a pure, deterministic function
//! of the score (SPEC §6). It derives the rhythm tag `Syncopated`, not the style
//! tag `SyncopatedRiff` (a passage-dominance judgement left to curation).

use std::collections::HashSet;

use crate::corpus::SwancoreTag;
use crate::score::{AtomEvent, MasterBar, Score, Track};

/// Fraction of beats that must be *displaced* for a track to read as syncopated.
///
/// A beat is displaced when the off-beat "and" before it is struck while the beat
/// itself is not. Balanced (#75) so characteristic displacement tags while
/// incidental anticipation does not; steady runs score zero by construction.
const SYNCOPATION_THRESHOLD: f64 = 0.25;

/// Derives the rhythm tag present in `track_index`.
///
/// Returns `[Syncopated]` when the displaced-beat share meets
/// [`SYNCOPATION_THRESHOLD`], otherwise empty — empty too when the track is out of
/// range or the score carries no bars to place onsets against.
#[must_use]
pub fn derive_syncopated(score: &Score, track_index: usize) -> Vec<SwancoreTag> {
    let Some(track) = score.tracks.get(track_index) else {
        return Vec::new();
    };
    if score.master_bars.is_empty() {
        return Vec::new();
    }

    let onsets = track_onsets(track);
    let (displaced, total) = displaced_beats(&score.master_bars, &onsets);
    if total == 0 {
        return Vec::new();
    }
    // `total > 0`, so the divisor is non-zero and both casts are exact.
    let ratio = f64::from(displaced) / f64::from(total);
    if ratio >= SYNCOPATION_THRESHOLD {
        vec![SwancoreTag::Syncopated]
    } else {
        Vec::new()
    }
}

/// The set of note-onset ticks across every voice of the track.
///
/// Some importers split one track into several voices, so the rhythm is read as a
/// whole.
fn track_onsets(track: &Track) -> HashSet<u32> {
    track
        .voices
        .iter()
        .flat_map(|voice| &voice.event_groups)
        .flat_map(|group| &group.atoms)
        .filter_map(|atom| match atom {
            AtomEvent::Note(note) => Some(note.absolute_start.0),
            AtomEvent::Rest(_) => None,
        })
        .collect()
}

/// Counts `(displaced, total)` beats over the bars.
///
/// A beat is *displaced* when the eighth-note "and" immediately before it carries
/// an onset but the beat tick itself does not. Bars whose beat does not divide
/// into an even tick count (no clean "and" to test) are skipped.
fn displaced_beats(bars: &[MasterBar], onsets: &HashSet<u32>) -> (u32, u32) {
    let mut displaced = 0_u32;
    let mut total = 0_u32;
    for bar in bars {
        let bar_start = bar.tick_range.start.0;
        let span = bar.tick_range.end.0.saturating_sub(bar_start);
        let beats = u32::from(bar.time_signature.numerator);
        let Some(beat_ticks) = span.checked_div(beats) else {
            continue; // numerator 0, or no span
        };
        let half = beat_ticks.checked_div(2).unwrap_or(0);
        if half == 0 || half.checked_mul(2) != Some(beat_ticks) {
            continue; // zero-width or odd beat: no clean off-beat grid
        }
        for b in 0..beats {
            let beat_tick = bar_start.saturating_add(b.saturating_mul(beat_ticks));
            let and_tick = beat_tick.saturating_sub(half);
            total = total.saturating_add(1);
            if onsets.contains(&and_tick) && !onsets.contains(&beat_tick) {
                displaced = displaced.saturating_add(1);
            }
        }
    }
    (displaced, total)
}
