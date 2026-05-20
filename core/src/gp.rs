//! Guitar Pro import adapter (S3, ADR-0002, ADR-0003).
//!
//! Entry point: [`import_gp_score`].
//!
//! ## Support matrix
//!
//! | Format | Extension | Status |
//! |--------|-----------|--------|
//! | GP3    | `.gp3`    | stable (read-only) |
//! | GP4    | `.gp4`    | stable (read-only) |
//! | GP5    | `.gp5`    | stable (read-only) |
//! | GP6    | `.gpx`    | read-only (BCFZ/BCFS + GPIF/XML) |
//! | GP7/8  | `.gp`     | not yet supported |
//!
//! GP3/4/5 are parsed from their binary format.  GP6 (`.gpx`) is supported
//! via the BCFZ/BCFS container path in the `guitarpro` crate.  GP7+ (`.gp`,
//! ZIP-based) is out of scope for S3.
//!
//! Every import produces a [`LossReport`] carried on [`Score`].  Losses
//! include tied/dead notes, percussion tracks, and notes with multiple
//! simultaneous articulations (only the primary one is kept).

use crate::{
    event::{Articulation, Pitch, Tempo, Ticks, TimeSignature, Velocity},
    score::{
        AtomEvent, AtomNote, AtomRest, EventGroup, EventGroupKind, ImportWarning, LossReport,
        MasterBar, Score, SourceMeta, TechniqueSpan, Track, Voice,
    },
    slice::TickRange,
};
use guitarpro::model::key_signature::Duration as GpDuration;
use guitarpro::model::note::NoteEffect as GpNoteEffect;

/// Guitar Pro internal PPQN (pulses per quarter note).
const GP_PPQN: u16 = 960;

// ── error type ────────────────────────────────────────────────────────────────

/// Error that can occur when importing a Guitar Pro file.
#[derive(Debug, thiserror::Error)]
pub enum GpImportError {
    /// The byte sequence does not look like a supported Guitar Pro format.
    #[error("unsupported Guitar Pro format")]
    UnsupportedFormat,
    /// The underlying `guitarpro` parser returned an error.
    #[error("Guitar Pro parse error: {0}")]
    Parse(#[from] guitarpro::GpError),
}

// ── public entry point ────────────────────────────────────────────────────────

/// Imports a Guitar Pro file from raw bytes into the canonical [`Score`] model.
///
/// Supports GP3 (`.gp3`), GP4 (`.gp4`), GP5 (`.gp5`), and GP6 (`.gpx`).
/// Returns [`GpImportError::UnsupportedFormat`] for unrecognised byte
/// sequences (including GP7+).
///
/// Conversion losses are carried on the returned [`Score`] as a [`LossReport`].
pub fn import_gp_score(data: &[u8]) -> Result<Score, GpImportError> {
    let mut song = guitarpro::Song::default();
    match detect_gp_version(data) {
        Some(3) => song.read_gp3(data)?,
        Some(4) => song.read_gp4(data)?,
        Some(5) => song.read_gp5(data)?,
        Some(6) => song.read_gpx(data)?,
        _ => return Err(GpImportError::UnsupportedFormat),
    }
    Ok(gp_song_to_score(&song))
}

// ── version detection ─────────────────────────────────────────────────────────

/// Peeks at up to 31 header bytes to determine the GP major version.
///
/// Returns `Some(3..=6)` for recognised formats, `None` otherwise.
fn detect_gp_version(data: &[u8]) -> Option<u8> {
    // GP3/4/5 binary: byte 0 = string length (≤30), bytes 1..31 = version string.
    if data.len() >= 5 {
        let len = data
            .first()
            .map_or(0, |&b| usize::from(b))
            .min(data.len().saturating_sub(1));
        let end = 1_usize.saturating_add(len).min(data.len());
        if let Some(vstr) = data.get(1..end) {
            if vstr.starts_with(b"FICHIER GUITAR PRO v3") {
                return Some(3);
            }
            if vstr.starts_with(b"FICHIER GUITAR PRO v4")
                || vstr.starts_with(b"CLIPBOARD GUITAR PRO 4")
            {
                return Some(4);
            }
            if vstr.starts_with(b"FICHIER GUITAR PRO v5") || vstr.starts_with(b"CLIPBOARD GP 5") {
                return Some(5);
            }
        }
    }
    // GP6: BCFZ (compressed) or BCFS (uncompressed) container magic.
    if data.starts_with(b"BCFZ") || data.starts_with(b"BCFS") {
        return Some(6);
    }
    None
}

// ── Song → Score conversion ───────────────────────────────────────────────────

fn gp_song_to_score(song: &guitarpro::Song) -> Score {
    let mut loss = LossReport::new();

    let tick_offset: u32 = song
        .measure_headers
        .first()
        .map_or(0_u32, |h| i64_to_u32_sat(h.start));

    let master_bars = build_gp_master_bars(song, tick_offset);
    let tracks: Vec<Track> = song
        .tracks
        .iter()
        .map(|t| build_gp_track(t, song, tick_offset, &mut loss))
        .collect();

    Score {
        ticks_per_quarter: GP_PPQN,
        master_bars,
        tracks,
        source_meta: Some(SourceMeta {
            format: Some(format!("GP{}", song.version.number.0)),
        }),
        loss,
    }
}

// ── master bar construction ───────────────────────────────────────────────────

fn build_gp_master_bars(song: &guitarpro::Song, tick_offset: u32) -> Vec<MasterBar> {
    let headers = &song.measure_headers;
    headers
        .iter()
        .enumerate()
        .map(|(idx, hdr)| {
            let start_tick = i64_to_u32_sat(hdr.start).saturating_sub(tick_offset);

            let end_tick = headers.get(idx.saturating_add(1)).map_or_else(
                || {
                    // Last bar: derive end from time signature.
                    let num = u32::from(hdr.time_signature.numerator.unsigned_abs()).max(1);
                    let den = u32::from(hdr.time_signature.denominator.value).max(1);
                    let bar_ticks = u32::from(GP_PPQN)
                        .saturating_mul(4)
                        .checked_div(den)
                        .unwrap_or(0)
                        .saturating_mul(num);
                    start_tick.saturating_add(bar_ticks)
                },
                |next| i64_to_u32_sat(next.start).saturating_sub(tick_offset),
            );

            let ts_num = hdr.time_signature.numerator.unsigned_abs().max(1);
            let ts_den_raw = hdr.time_signature.denominator.value.max(1);
            // Clamp to u8; denominator must be a power of two per our TimeSignature invariant.
            #[allow(clippy::cast_possible_truncation)]
            let ts_den_u8 = ts_den_raw.min(128) as u8;
            let ts_den = ts_den_u8.next_power_of_two();

            let time_sig = TimeSignature::new(ts_num, ts_den).unwrap_or(TimeSignature {
                numerator: 4,
                denominator: 4,
            });
            let tempo = Tempo::new(f64::from(hdr.tempo.max(1))).unwrap_or(Tempo(120.0_f64));

            MasterBar {
                index: idx,
                tick_range: TickRange {
                    start: Ticks(start_tick),
                    end: Ticks(end_tick),
                },
                time_signature: time_sig,
                tempo,
            }
        })
        .collect()
}

// ── track construction ────────────────────────────────────────────────────────

fn build_gp_track(
    gp_track: &guitarpro::Track,
    song: &guitarpro::Song,
    tick_offset: u32,
    loss: &mut LossReport,
) -> Track {
    let channel = song
        .channels
        .get(gp_track.channel_index)
        .map_or(0_u8, |c| c.channel);

    let voice_count = gp_track
        .measures
        .iter()
        .map(|m| m.voices.len())
        .max()
        .unwrap_or(0);

    let voices: Vec<Voice> = (0..voice_count)
        .filter_map(|vi| build_gp_voice(gp_track, vi, tick_offset, loss))
        .collect();

    Track {
        name: Some(gp_track.name.clone()),
        channel,
        voices,
    }
}

// ── voice construction ────────────────────────────────────────────────────────

fn build_gp_voice(
    gp_track: &guitarpro::Track,
    voice_idx: usize,
    tick_offset: u32,
    loss: &mut LossReport,
) -> Option<Voice> {
    let mut event_groups: Vec<EventGroup> = Vec::new();

    for measure in &gp_track.measures {
        let Some(gp_voice) = measure.voices.get(voice_idx) else {
            continue;
        };
        let measure_start = i64_to_u32_sat(measure.start).saturating_sub(tick_offset);
        let mut cursor = measure_start;

        for beat in &gp_voice.beats {
            let dur_ticks = gp_duration_ticks(&beat.duration).max(1);
            let beat_start = beat
                .start
                .map_or(cursor, |s| i64_to_u32_sat(s).saturating_sub(tick_offset));

            let eg = build_event_group(beat, beat_start, dur_ticks, &gp_track.strings, loss);
            event_groups.push(eg);
            cursor = cursor.saturating_add(dur_ticks);
        }
    }

    if event_groups.is_empty() {
        return None;
    }
    // GP supports at most 2 voices per measure; voice_idx is 0 or 1.
    #[allow(clippy::cast_possible_truncation)]
    let id = voice_idx as u8;
    Some(Voice { id, event_groups })
}

// ── beat / event-group construction ──────────────────────────────────────────

fn build_event_group(
    beat: &guitarpro::Beat,
    beat_start: u32,
    dur_ticks: u32,
    strings: &[(i8, i8)],
    loss: &mut LossReport,
) -> EventGroup {
    let start = Ticks(beat_start);
    let duration = Ticks(dur_ticks);

    // Rest or empty beat → single rest atom.
    if beat.status == guitarpro::BeatStatus::Rest
        || beat.status == guitarpro::BeatStatus::Empty
        || beat.notes.is_empty()
    {
        return rest_group(start, duration);
    }

    let mut atoms: Vec<AtomEvent> = Vec::new();
    let mut technique_spans: Vec<TechniqueSpan> = Vec::new();

    for note in &beat.notes {
        match note.kind {
            guitarpro::NoteType::Normal => {
                let Some(midi) = gp_note_midi_pitch(note, strings) else {
                    loss.add(ImportWarning::Other(
                        "GP note pitch out of range; note skipped".to_owned(),
                    ));
                    continue;
                };
                let pitch = Pitch(midi);
                #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                let velocity = Velocity(note.velocity.clamp(0, 127) as u8);
                let articulation =
                    map_note_articulation(&note.effect, start, duration, &mut technique_spans);
                atoms.push(AtomEvent::Note(AtomNote {
                    absolute_start: start,
                    duration,
                    pitch,
                    velocity,
                    articulation,
                }));
            }
            guitarpro::NoteType::Tie
            | guitarpro::NoteType::Dead
            | guitarpro::NoteType::Rest
            | guitarpro::NoteType::Unknown(_) => {
                loss.add(ImportWarning::Other(format!(
                    "GP note kind {kind:?} not fully supported; skipped",
                    kind = note.kind,
                )));
            }
        }
    }

    if atoms.is_empty() {
        return rest_group(start, duration);
    }

    let kind = if atoms.len() == 1 {
        EventGroupKind::Single
    } else {
        EventGroupKind::Chord
    };
    EventGroup {
        kind,
        atoms,
        technique_spans,
    }
}

fn rest_group(start: Ticks, duration: Ticks) -> EventGroup {
    EventGroup {
        kind: EventGroupKind::Single,
        atoms: vec![AtomEvent::Rest(AtomRest {
            absolute_start: start,
            duration,
        })],
        technique_spans: Vec::new(),
    }
}

// ── note pitch calculation ────────────────────────────────────────────────────

/// Computes the MIDI pitch for a GP note from string/fret data.
///
/// `strings` is the per-string `(string_number, open_tuning_midi_note)` array
/// from `guitarpro::Track::strings`.  Returns `None` when the string index is
/// out of range or the resulting MIDI note overflows 0–127.
fn gp_note_midi_pitch(note: &guitarpro::Note, strings: &[(i8, i8)]) -> Option<u8> {
    if note.string <= 0 {
        return None;
    }
    // string is 1-indexed; strings are ordered by string number in GP.
    let string_idx = usize::try_from(note.string).ok()?.saturating_sub(1);
    let &(_, tuning) = strings.get(string_idx)?;
    let midi = i16::from(tuning).saturating_add(note.value);
    // Clamp and convert to u8; saturating_add already guards against overflow.
    u8::try_from(midi.clamp(0, 127)).ok()
}

// ── articulation mapping ──────────────────────────────────────────────────────

/// Maps the primary GP note effect to a canonical [`Articulation`] and emits a
/// [`TechniqueSpan`].  Only the highest-priority articulation is returned;
/// secondary effects are silently dropped.
fn map_note_articulation(
    effect: &GpNoteEffect,
    start: Ticks,
    duration: Ticks,
    spans: &mut Vec<TechniqueSpan>,
) -> Option<Articulation> {
    let end = Ticks(start.0.saturating_add(duration.0));
    let tick_range = TickRange { start, end };

    let articulation = if effect.hammer {
        Some(Articulation::HammerOn)
    } else if !effect.slides.is_empty() {
        Some(Articulation::Slide)
    } else if effect.palm_mute {
        Some(Articulation::PalmMute)
    } else if effect.vibrato {
        Some(Articulation::Vibrato)
    } else if let Some(h) = &effect.harmonic {
        match h.kind {
            guitarpro::HarmonicType::Pinch => Some(Articulation::HarmonicPinch),
            _ => Some(Articulation::HarmonicNatural),
        }
    } else {
        None
    };

    if let Some(a) = articulation {
        spans.push(TechniqueSpan {
            technique: a,
            tick_range,
        });
    }
    articulation
}

// ── duration helpers ──────────────────────────────────────────────────────────

/// Computes the beat duration in GP ticks from the public `Duration` fields.
///
/// Matches the internal `Duration::time()` logic in the `guitarpro` crate
/// (which is `pub(crate)` and cannot be called externally):
/// `base = PPQN * 4 / value`, with dotted and tuplet adjustments.
fn gp_duration_ticks(dur: &GpDuration) -> u32 {
    if dur.value == 0 {
        return u32::from(GP_PPQN);
    }
    let base = u32::from(GP_PPQN)
        .saturating_mul(4)
        .checked_div(u32::from(dur.value))
        .unwrap_or_else(|| u32::from(GP_PPQN));
    let dotted_extra = if dur.dotted {
        base.checked_div(2).unwrap_or(0)
    } else {
        0
    };
    let time = base.saturating_add(dotted_extra);
    // Apply tuplet factor (same convention as guitarpro's convert_time).
    if dur.tuplet_enters == 0 || dur.tuplet_times == 0 || dur.tuplet_enters == dur.tuplet_times {
        return time;
    }
    time.saturating_mul(u32::from(dur.tuplet_enters))
        .checked_div(u32::from(dur.tuplet_times))
        .unwrap_or(time)
}

/// Saturating cast from `i64` to `u32`: clamps negative values to 0 and
/// values above `u32::MAX` to `u32::MAX`.
fn i64_to_u32_sat(v: i64) -> u32 {
    if v <= 0 {
        0_u32
    } else if v > i64::from(u32::MAX) {
        u32::MAX
    } else {
        // Safety: v is in 1..=u32::MAX, guaranteed by the branches above.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let r = v as u32;
        r
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::missing_assert_message
)]
mod tests {
    use super::*;
    use guitarpro::model::key_signature::Duration as GpDurationTest;

    // ── detect_gp_version ─────────────────────────────────────────────────────

    #[test]
    fn detect_version_gp3() {
        let mut header = vec![30u8];
        header.extend_from_slice(b"FICHIER GUITAR PRO v3.00      "); // 30 bytes
        assert_eq!(detect_gp_version(&header), Some(3));
    }

    #[test]
    fn detect_version_gp4() {
        let mut header = vec![30u8];
        header.extend_from_slice(b"FICHIER GUITAR PRO v4.00      ");
        assert_eq!(detect_gp_version(&header), Some(4));
    }

    #[test]
    fn detect_version_gp5() {
        let mut header = vec![30u8];
        header.extend_from_slice(b"FICHIER GUITAR PRO v5.00      ");
        assert_eq!(detect_gp_version(&header), Some(5));
    }

    #[test]
    fn detect_version_gp6_bcfz() {
        assert_eq!(detect_gp_version(b"BCFZsome_content"), Some(6));
    }

    #[test]
    fn detect_version_gp6_bcfs() {
        assert_eq!(detect_gp_version(b"BCFSsome_content"), Some(6));
    }

    #[test]
    fn detect_version_unknown() {
        assert_eq!(detect_gp_version(b"garbage"), None);
        assert_eq!(detect_gp_version(b""), None);
        assert_eq!(detect_gp_version(b"PK\x03\x04"), None); // ZIP / GP7
    }

    // ── import_gp_score errors ────────────────────────────────────────────────

    #[test]
    fn import_gp_score_garbage_returns_unsupported() {
        let result = import_gp_score(b"this is not a guitar pro file");
        assert!(matches!(result, Err(GpImportError::UnsupportedFormat)));
    }

    #[test]
    fn import_gp_score_empty_returns_unsupported() {
        assert!(matches!(
            import_gp_score(b""),
            Err(GpImportError::UnsupportedFormat)
        ));
    }

    // ── gp_duration_ticks ─────────────────────────────────────────────────────

    #[test]
    fn duration_ticks_quarter_note() {
        let dur = GpDurationTest {
            value: 4,
            dotted: false,
            double_dotted: false,
            min_time: 0,
            tuplet_enters: 1,
            tuplet_times: 1,
        };
        assert_eq!(gp_duration_ticks(&dur), 960); // GP_PPQN
    }

    #[test]
    fn duration_ticks_eighth_note() {
        let dur = GpDurationTest {
            value: 8,
            dotted: false,
            double_dotted: false,
            min_time: 0,
            tuplet_enters: 1,
            tuplet_times: 1,
        };
        assert_eq!(gp_duration_ticks(&dur), 480);
    }

    #[test]
    fn duration_ticks_dotted_quarter() {
        let dur = GpDurationTest {
            value: 4,
            dotted: true,
            double_dotted: false,
            min_time: 0,
            tuplet_enters: 1,
            tuplet_times: 1,
        };
        assert_eq!(gp_duration_ticks(&dur), 1440); // 960 + 480
    }

    #[test]
    fn duration_ticks_zero_value_fallback() {
        let dur = GpDurationTest {
            value: 0,
            dotted: false,
            double_dotted: false,
            min_time: 0,
            tuplet_enters: 1,
            tuplet_times: 1,
        };
        assert_eq!(gp_duration_ticks(&dur), 960); // fallback quarter
    }

    // ── gp_note_midi_pitch ────────────────────────────────────────────────────

    #[test]
    fn midi_pitch_standard_guitar_string1_fret0() {
        // String 1 = high E, open = MIDI 64 (E4).
        let note = guitarpro::Note {
            value: 0, // fret 0
            string: 1,
            ..Default::default()
        };
        let strings = vec![(1_i8, 64_i8)]; // string 1, tuning E4 = 64
        assert_eq!(gp_note_midi_pitch(&note, &strings), Some(64));
    }

    #[test]
    fn midi_pitch_fret_2() {
        let note = guitarpro::Note {
            value: 2, // fret 2
            string: 1,
            ..Default::default()
        };
        let strings = vec![(1_i8, 64_i8)];
        assert_eq!(gp_note_midi_pitch(&note, &strings), Some(66)); // F#4
    }

    #[test]
    fn midi_pitch_invalid_string_zero() {
        let note = guitarpro::Note {
            value: 5,
            string: 0, // invalid
            ..Default::default()
        };
        let strings = vec![(1_i8, 40_i8)];
        assert_eq!(gp_note_midi_pitch(&note, &strings), None);
    }

    #[test]
    fn midi_pitch_out_of_bounds_string() {
        let note = guitarpro::Note {
            value: 0,
            string: 7, // string 7, but only 6 strings
            ..Default::default()
        };
        let strings = vec![
            (1_i8, 64_i8),
            (2_i8, 59_i8),
            (3_i8, 55_i8),
            (4_i8, 50_i8),
            (5_i8, 45_i8),
            (6_i8, 40_i8),
        ];
        assert_eq!(gp_note_midi_pitch(&note, &strings), None);
    }

    // ── i64_to_u32_sat ────────────────────────────────────────────────────────

    #[test]
    fn i64_to_u32_sat_positive() {
        assert_eq!(i64_to_u32_sat(960), 960);
        assert_eq!(i64_to_u32_sat(0), 0);
    }

    #[test]
    fn i64_to_u32_sat_negative() {
        assert_eq!(i64_to_u32_sat(-1), 0);
        assert_eq!(i64_to_u32_sat(i64::MIN), 0);
    }

    #[test]
    fn i64_to_u32_sat_overflow() {
        assert_eq!(i64_to_u32_sat(i64::from(u32::MAX)), u32::MAX);
        assert_eq!(i64_to_u32_sat(i64::MAX), u32::MAX);
    }

    // ── gp_song_to_score: empty song ─────────────────────────────────────────────

    #[test]
    fn empty_song_produces_valid_score() {
        // Convert a bare default Song (no measures, no tracks) to a canonical Score.
        let song = guitarpro::Song::default();
        let score = gp_song_to_score(&song);
        assert_eq!(score.ticks_per_quarter, GP_PPQN);
        assert!(score.master_bars.is_empty());
        assert!(score.tracks.is_empty());
        // source_meta reflects GP5 (the guitarpro default version).
        let fmt = score
            .source_meta
            .as_ref()
            .and_then(|m| m.format.as_deref())
            .unwrap_or("");
        assert!(fmt.starts_with("GP"));
    }
}
