//! Ingest-time interpretation of an imported [`Score`], for building the corpus
//! from bulk Guitar Pro / MIDI sources.
//!
//! The first concern is **track role**: a multitrack tab mixes guitars, bass,
//! drums and vocals, but only some parts belong in a riff corpus. This module
//! classifies each track from evidence already on the imported model — its
//! name, MIDI channel, and open-string tuning — so a bulk ingest can keep the
//! guitars (both of them, for the two-guitar writing this corpus is full of),
//! optionally the bass, and skip the rest.

use crate::score::Track;

/// The instrumental role of a track, as far as ingest can tell.
///
/// Deliberately coarse: only the distinctions a corpus build acts on today.
/// Drums and vocals both fall under [`TrackRole::Other`] — the corpus does not
/// use them yet, and telling them apart without a reliable name is guesswork.
/// When that scope arrives the enum extends additively.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackRole {
    /// A fretted six-or-more-string guitar part.
    Guitar,
    /// A bass part (typically four strings, or five with a low B).
    Bass,
    /// Anything the corpus does not ingest today: drums, vocals, percussion,
    /// or a part with no readable fretted tuning.
    Other,
}

/// Classifies a track's instrumental role from its name, channel, and tuning.
#[must_use]
pub fn classify_track_role(_track: &Track) -> TrackRole {
    TrackRole::Other
}

#[cfg(test)]
mod tests {
    use super::{classify_track_role, TrackRole};
    use crate::event::{Pitch, Tuning};
    use crate::score::Track;

    fn track(name: Option<&str>, channel: u8, strings: &[u8]) -> Track {
        Track {
            name: name.map(str::to_owned),
            channel,
            voices: Vec::new(),
            tuning: Tuning::new(strings.iter().map(|&m| Pitch(m)).collect()),
        }
    }

    // Standard E, string 1 (high) first: E4 B3 G3 D3 A2 E2.
    const STANDARD_6: [u8; 6] = [64, 59, 55, 50, 45, 40];
    // 7-string with a low B1.
    const STANDARD_7: [u8; 7] = [64, 59, 55, 50, 45, 40, 35];
    // 4-string bass, standard: G2 D2 A1 E1.
    const BASS_4: [u8; 4] = [43, 38, 33, 28];
    // 5-string bass with a low B0 (23).
    const BASS_5: [u8; 5] = [43, 38, 33, 28, 23];

    #[test]
    fn six_string_tuning_is_a_guitar() {
        assert_eq!(classify_track_role(&track(None, 0, &STANDARD_6)), TrackRole::Guitar);
    }

    #[test]
    fn seven_string_tuning_is_a_guitar() {
        assert_eq!(classify_track_role(&track(None, 0, &STANDARD_7)), TrackRole::Guitar);
    }

    #[test]
    fn four_string_tuning_is_a_bass() {
        assert_eq!(classify_track_role(&track(None, 0, &BASS_4)), TrackRole::Bass);
    }

    #[test]
    fn five_string_with_a_low_b_is_a_bass() {
        assert_eq!(classify_track_role(&track(None, 0, &BASS_5)), TrackRole::Bass);
    }

    #[test]
    fn all_strings_at_one_pitch_is_a_placeholder_not_an_instrument() {
        // The all-`C-1` (MIDI 0) shape a drum/vocal track imports as.
        assert_eq!(classify_track_role(&track(None, 0, &[0, 0, 0, 0, 0, 0])), TrackRole::Other);
    }

    #[test]
    fn empty_tuning_is_other() {
        assert_eq!(classify_track_role(&track(None, 0, &[])), TrackRole::Other);
    }

    #[test]
    fn name_bass_beats_a_guitar_tuning() {
        // "Bass Guitar" contains both words; bass must win.
        assert_eq!(
            classify_track_role(&track(Some("Bass Guitar"), 0, &STANDARD_6)),
            TrackRole::Bass
        );
    }

    #[test]
    fn name_guitar_beats_a_non_guitar_tuning() {
        assert_eq!(
            classify_track_role(&track(Some("Rhythm Guitar"), 0, &BASS_4)),
            TrackRole::Guitar
        );
    }

    #[test]
    fn name_drums_is_other_even_with_a_fretted_tuning() {
        assert_eq!(
            classify_track_role(&track(Some("Drums"), 0, &STANDARD_6)),
            TrackRole::Other
        );
    }

    #[test]
    fn name_vocals_is_other() {
        assert_eq!(
            classify_track_role(&track(Some("Lead Vocals"), 0, &STANDARD_6)),
            TrackRole::Other
        );
    }

    #[test]
    fn percussion_channel_is_other_without_a_name() {
        assert_eq!(classify_track_role(&track(None, 9, &STANDARD_6)), TrackRole::Other);
    }
}
