//! Ingest-time interpretation of an imported [`Score`], for building the
//! corpus from bulk Guitar Pro / MIDI sources.
//!
//! The first concern is **track role**: a multitrack tab mixes guitars, bass,
//! drums and vocals, but only some parts belong in a riff corpus. This module
//! classifies each track from evidence already on the imported model — its
//! name, MIDI channel, and open-string tuning — so a bulk ingest can keep the
//! guitars (both of them, for the two-guitar writing this corpus is full of),
//! optionally the bass, and skip the rest.

use crate::event::Tuning;
use crate::score::{Score, Track};

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

/// The lowest MIDI pitch a guitar's low string is expected to reach; a fretted
/// part whose lowest open string is at or below this is read as a bass. E1.
const BASS_LOW_STRING_CEILING: u8 = 28;

/// Classifies a track's instrumental role from its name, channel, and tuning.
///
/// Precedence, most reliable first:
/// 1. **Name.** An explicit part name is trusted over structure — a track
///    called "Bass" with a guitar's tuning is still a bass. "Bass" is checked
///    before "guitar" so "Bass Guitar" reads as bass.
/// 2. **Channel 9**, the General MIDI percussion channel, is drums.
/// 3. **Tuning.** A placeholder tuning (empty, or every string the same pitch —
///    the all-`C-1` shape a non-fretted track imports as) is not an instrument
///    we ingest. Otherwise the string count separates bass from guitar:
///    four or fewer is a bass, six or more a guitar, and a five-string is a
///    bass only if its lowest string reaches into bass range.
#[must_use]
pub fn classify_track_role(track: &Track) -> TrackRole {
    if let Some(name) = track.name.as_deref() {
        let name = name.to_lowercase();
        if name.contains("bass") {
            return TrackRole::Bass;
        }
        if name.contains("drum")
            || name.contains("perc")
            || name.contains("vocal")
            || name.contains("voice")
            || name.contains("sing")
        {
            return TrackRole::Other;
        }
        if name.contains("guitar") || name.contains("gtr") {
            return TrackRole::Guitar;
        }
    }

    if track.channel == 9 {
        return TrackRole::Other;
    }

    let open = track.tuning.open_strings();
    let Some(first) = open.first().map(|p| p.0) else {
        return TrackRole::Other;
    };
    if open.iter().all(|p| p.0 == first) {
        return TrackRole::Other;
    }

    match open.len() {
        0..=4 => TrackRole::Bass,
        5 => {
            let lowest = open.iter().map(|p| p.0).min().unwrap_or(first);
            if lowest <= BASS_LOW_STRING_CEILING {
                TrackRole::Bass
            } else {
                TrackRole::Guitar
            }
        }
        _ => TrackRole::Guitar,
    }
}

/// The indices of the tracks a bulk ingest should take from `score`.
///
/// Every guitar, and the bass parts too when `include_bass` is set. Order is
/// preserved. Empty when the file has no part worth ingesting — the caller
/// treats that as a skip, not an error.
#[must_use]
pub fn select_ingest_tracks(score: &Score, include_bass: bool) -> Vec<usize> {
    score
        .tracks
        .iter()
        .enumerate()
        .filter(|(_, track)| match classify_track_role(track) {
            TrackRole::Guitar => true,
            TrackRole::Bass => include_bass,
            TrackRole::Other => false,
        })
        .map(|(index, _)| index)
        .collect()
}

/// A stable, human-readable label for a tuning, for `ChunkMeta.tuning`.
///
/// Common tunings resolve to their conventional `snake_case` name
/// (`standard_e`, `drop_d`, …); anything else falls back to a deterministic
/// low-to-high spelling of its open strings (`b1_e2_a2_d3_g3_b3_e4`), so an
/// unusual tuning is still recorded exactly and never lost. The named set is
/// deliberately small and meant to grow as the corpus turns up more.
#[must_use]
pub fn tuning_label(tuning: &Tuning) -> String {
    // Open strings as griff stores them: string 1 (highest) first.
    let high_to_low: Vec<u8> = tuning.open_strings().iter().map(|p| p.0).collect();
    match high_to_low.as_slice() {
        [64, 59, 55, 50, 45, 40] => return "standard_e".to_owned(),
        [64, 59, 55, 50, 45, 38] => return "drop_d".to_owned(),
        [64, 59, 55, 50, 45, 40, 35] => return "standard_b_7".to_owned(),
        [43, 38, 33, 28] => return "bass_standard".to_owned(),
        _ => {}
    }
    high_to_low
        .iter()
        .rev()
        .map(|&midi| note_name(midi))
        .collect::<Vec<_>>()
        .join("_")
}

/// A MIDI pitch as a `snake_case` note name with octave, e.g. `e2`, `ds4`
/// (D#4). Octave numbering follows scientific pitch (MIDI 60 = `c4`).
fn note_name(midi: u8) -> String {
    let name = match midi % 12 {
        0 => "c",
        1 => "cs",
        2 => "d",
        3 => "ds",
        4 => "e",
        5 => "f",
        6 => "fs",
        7 => "g",
        8 => "gs",
        9 => "a",
        10 => "as",
        _ => "b",
    };
    let octave = i16::from(midi / 12).saturating_sub(1);
    format!("{name}{octave}")
}

#[cfg(test)]
mod tests {
    use super::{classify_track_role, select_ingest_tracks, tuning_label, TrackRole};
    use crate::event::{Pitch, Tuning};
    use crate::score::{LossReport, Score, Track};

    fn track(name: Option<&str>, channel: u8, strings: &[u8]) -> Track {
        Track {
            name: name.map(str::to_owned),
            channel,
            voices: Vec::new(),
            tuning: Tuning::new(strings.iter().map(|&m| Pitch(m)).collect()),
        }
    }

    fn score_of(tracks: Vec<Track>) -> Score {
        Score {
            ticks_per_quarter: 480,
            master_bars: Vec::new(),
            tracks,
            source_meta: None,
            loss: LossReport::new(),
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
        assert_eq!(
            classify_track_role(&track(None, 0, &STANDARD_6)),
            TrackRole::Guitar
        );
    }

    #[test]
    fn seven_string_tuning_is_a_guitar() {
        assert_eq!(
            classify_track_role(&track(None, 0, &STANDARD_7)),
            TrackRole::Guitar
        );
    }

    #[test]
    fn four_string_tuning_is_a_bass() {
        assert_eq!(
            classify_track_role(&track(None, 0, &BASS_4)),
            TrackRole::Bass
        );
    }

    #[test]
    fn five_string_with_a_low_b_is_a_bass() {
        assert_eq!(
            classify_track_role(&track(None, 0, &BASS_5)),
            TrackRole::Bass
        );
    }

    #[test]
    fn all_strings_at_one_pitch_is_a_placeholder_not_an_instrument() {
        // The all-`C-1` (MIDI 0) shape a drum/vocal track imports as.
        assert_eq!(
            classify_track_role(&track(None, 0, &[0, 0, 0, 0, 0, 0])),
            TrackRole::Other
        );
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
        assert_eq!(
            classify_track_role(&track(None, 9, &STANDARD_6)),
            TrackRole::Other
        );
    }

    // ── select_ingest_tracks ──────────────────────────────────────────────────

    fn two_guitars_bass_and_drums() -> Score {
        score_of(vec![
            track(Some("Guitar 1"), 0, &STANDARD_6),
            track(Some("Guitar 2"), 0, &STANDARD_7),
            track(Some("Bass"), 0, &BASS_4),
            track(Some("Drums"), 9, &[0, 0, 0, 0, 0, 0]),
        ])
    }

    #[test]
    fn selects_both_guitars_and_skips_bass_and_drums_by_default() {
        assert_eq!(
            select_ingest_tracks(&two_guitars_bass_and_drums(), false),
            vec![0, 1]
        );
    }

    #[test]
    fn includes_bass_when_asked_still_skipping_drums() {
        assert_eq!(
            select_ingest_tracks(&two_guitars_bass_and_drums(), true),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn a_file_with_no_fretted_part_selects_nothing() {
        let score = score_of(vec![
            track(Some("Drums"), 9, &[0, 0, 0, 0, 0, 0]),
            track(Some("Lead Vocals"), 0, &[0, 0, 0, 0, 0, 0]),
        ]);
        assert!(select_ingest_tracks(&score, true).is_empty());
    }

    #[test]
    fn a_single_guitar_is_selected() {
        let score = score_of(vec![track(None, 0, &STANDARD_6)]);
        assert_eq!(select_ingest_tracks(&score, false), vec![0]);
    }

    // ── tuning_label ──────────────────────────────────────────────────────────

    fn tuning_of(strings: &[u8]) -> Tuning {
        Tuning::new(strings.iter().map(|&m| Pitch(m)).collect())
    }

    #[test]
    fn names_standard_e() {
        assert_eq!(tuning_label(&tuning_of(&STANDARD_6)), "standard_e");
    }

    #[test]
    fn names_drop_d() {
        // High-to-low: E4 B3 G3 D3 A2 D2 — the low E dropped to D.
        assert_eq!(
            tuning_label(&tuning_of(&[64, 59, 55, 50, 45, 38])),
            "drop_d"
        );
    }

    #[test]
    fn names_seven_string_standard_b() {
        assert_eq!(tuning_label(&tuning_of(&STANDARD_7)), "standard_b_7");
    }

    #[test]
    fn names_four_string_bass_standard() {
        assert_eq!(tuning_label(&tuning_of(&BASS_4)), "bass_standard");
    }

    #[test]
    fn an_unknown_tuning_spells_its_open_strings_low_to_high() {
        // Drop C, high-to-low: D4 A3 F3 C3 G2 C2 -> spelled low to high.
        assert_eq!(
            tuning_label(&tuning_of(&[62, 57, 53, 48, 43, 36])),
            "c2_g2_c3_f3_a3_d4"
        );
    }

    #[test]
    fn a_sharp_pitch_class_spells_with_an_s() {
        // A single D#2 open string (contrived) exercises the sharp spelling.
        assert_eq!(tuning_label(&tuning_of(&[39, 39])), "ds2_ds2");
    }
}
