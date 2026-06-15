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
//! Every import produces a [`LossReport`] carried on [`Score`].  Tied notes
//! continue the preceding note on their string (extending its duration);
//! percussion tracks and other unsupported note kinds remain losses.
//! Co-occurring techniques are preserved (ADR-0018):
//! harmonics/accent/ghost/staccato/dead-note become per-note `NoteMark`s and
//! each spanning technique (hammer-on, slide, bend, palm-mute, vibrato) its own
//! `TechniqueSpan`.

use crate::{
    event::{
        FretboardPosition, NoteMark, NoteMarks, NotePosition, Pitch, SpanTechnique,
        TechniqueEvidence, Tempo, Ticks, TimeSignature, Tuning, Velocity,
    },
    score::{
        AtomEvent, AtomNote, AtomRest, EventGroup, EventGroupKind, ImportWarning, LossReport,
        MasterBar, Score, SourceMeta, TechniqueSpan, Track, Voice,
    },
    slice::TickRange,
};
use guitarpro::model::key_signature::Duration as GpDuration;
use guitarpro::model::note::NoteEffect as GpNoteEffect;
use std::collections::HashMap;

/// Guitar Pro internal PPQN (pulses per quarter note).
const GP_PPQN: u16 = 960;

// ── timeline helpers ──────────────────────────────────────────────────────────

/// Ticks spanned by a `numerator/denominator` bar at [`GP_PPQN`].
fn bar_ticks(numerator: u8, denominator: u8) -> u32 {
    u32::from(numerator).saturating_add(u32::from(denominator)) // STUB
}

/// Cumulative start tick of each bar, summed from its meter — so the timeline
/// never collapses, whatever (constant) start offsets the GP file carries.
fn cumulative_bar_starts(meters: &[(u8, u8)]) -> Vec<u32> {
    meters.iter().map(|_| 0).collect() // STUB
}

/// Per-bar tempo with carry-forward: a bar with no explicit tempo (GP `0`)
/// inherits the previous bar's; the default before any tempo is 120 BPM.
fn carry_tempos(raw_tempos: &[i32]) -> Vec<f64> {
    raw_tempos.iter().map(|_| 0.0).collect() // STUB
}

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
        tuning: gp_tuning(&gp_track.strings),
    }
}

/// Builds a [`Tuning`] from a Guitar Pro `(string_number, open_midi)` array,
/// ordered string 1 (highest) first (ADR-0018). Falls back to Standard E when
/// the array carries no valid open-string pitches.
fn gp_tuning(strings: &[(i8, i8)]) -> Tuning {
    let open: Vec<Pitch> = strings
        .iter()
        .filter_map(|&(_, midi)| u8::try_from(midi).ok().and_then(|m| Pitch::new(m).ok()))
        .collect();
    if open.is_empty() {
        Tuning::standard_e()
    } else {
        Tuning::new(open)
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
    // Most recent sounding note per string — `string → (group index, atom index)`
    // — so a tie can continue it (ADR-0018).
    let mut held: HashMap<u8, (usize, usize)> = HashMap::new();
    let mut acc = VoiceAccum {
        groups: &mut event_groups,
        held: &mut held,
        loss,
    };

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

            append_beat(beat, beat_start, dur_ticks, &gp_track.strings, &mut acc);
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

/// Mutable per-voice state threaded through beat construction.
struct VoiceAccum<'a> {
    groups: &'a mut Vec<EventGroup>,
    held: &'a mut HashMap<u8, (usize, usize)>,
    loss: &'a mut LossReport,
}

fn append_beat(
    beat: &guitarpro::Beat,
    beat_start: u32,
    dur_ticks: u32,
    strings: &[(i8, i8)],
    acc: &mut VoiceAccum<'_>,
) {
    let start = Ticks(beat_start);
    let duration = Ticks(dur_ticks);

    // Rest or empty beat → single rest atom.
    if beat.status == guitarpro::BeatStatus::Rest
        || beat.status == guitarpro::BeatStatus::Empty
        || beat.notes.is_empty()
    {
        acc.groups.push(rest_group(start, duration));
        return;
    }

    let group_index = acc.groups.len();
    let mut atoms: Vec<AtomEvent> = Vec::new();
    let mut technique_spans: Vec<TechniqueSpan> = Vec::new();
    let mut continued = false;

    for note in &beat.notes {
        match note.kind {
            guitarpro::NoteType::Normal | guitarpro::NoteType::Dead => {
                let Some(midi) = gp_note_midi_pitch(note, strings) else {
                    acc.loss.add(ImportWarning::Other(
                        "GP note pitch out of range; note skipped".to_owned(),
                    ));
                    continue;
                };
                let pitch = Pitch(midi);
                #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                let velocity = Velocity(note.velocity.clamp(0, 127) as u8);
                let mut marks =
                    map_gp_note_marks(&note.effect, start, duration, &mut technique_spans);
                // A dead ("X") note keeps its (string, fret) but is a muted hit.
                if matches!(note.kind, guitarpro::NoteType::Dead) {
                    marks.insert(NoteMark::DeadNote);
                }
                let atom_index = atoms.len();
                atoms.push(AtomEvent::Note(AtomNote {
                    absolute_start: start,
                    duration,
                    pitch,
                    velocity,
                    marks,
                    // Guitar Pro is a source of truth for (string, fret) — ADR-0018.
                    position: gp_note_position(note).map(NotePosition::explicit),
                }));
                if let Ok(string) = u8::try_from(note.string) {
                    acc.held.insert(string, (group_index, atom_index));
                }
            }
            guitarpro::NoteType::Tie => {
                if extend_tie(note, dur_ticks, acc) {
                    continued = true;
                }
            }
            guitarpro::NoteType::Rest | guitarpro::NoteType::Unknown(_) => {
                acc.loss.add(ImportWarning::Other(format!(
                    "GP note kind {kind:?} not fully supported; skipped",
                    kind = note.kind,
                )));
            }
        }
    }

    if atoms.is_empty() {
        // An all-tie beat has extended its held notes and needs no event itself.
        if !continued {
            acc.groups.push(rest_group(start, duration));
        }
        return;
    }

    let kind = if atoms.len() == 1 {
        EventGroupKind::Single
    } else {
        EventGroupKind::Chord
    };
    acc.groups.push(EventGroup {
        kind,
        atoms,
        technique_spans,
    });
}

/// Continues a tied note onto the most recent note on its string by extending
/// that note's duration. Returns `true` when a held note was found; otherwise
/// records a loss (an orphan tie) and returns `false`.
fn extend_tie(note: &guitarpro::Note, dur_ticks: u32, acc: &mut VoiceAccum<'_>) -> bool {
    let location = u8::try_from(note.string)
        .ok()
        .and_then(|string| acc.held.get(&string).copied());
    if let Some((group_index, atom_index)) = location {
        if let Some(AtomEvent::Note(held)) = acc
            .groups
            .get_mut(group_index)
            .and_then(|group| group.atoms.get_mut(atom_index))
        {
            held.duration = Ticks(held.duration.0.saturating_add(dur_ticks));
            return true;
        }
    }
    acc.loss.add(ImportWarning::Other(
        "GP tie has no preceding note on its string; skipped".to_owned(),
    ));
    false
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

/// Reads the source-of-truth `(string, fret)` of a GP note as a
/// [`FretboardPosition`] (ADR-0018). `None` when the GP string/fret is invalid.
fn gp_note_position(note: &guitarpro::Note) -> Option<FretboardPosition> {
    if note.string <= 0 || note.value < 0 {
        return None;
    }
    let string = u8::try_from(note.string).ok()?;
    let fret = u8::try_from(note.value).ok()?;
    Some(FretboardPosition { string, fret })
}

// ── articulation mapping ──────────────────────────────────────────────────────

/// Maps GP note effects onto the rich note model (ADR-0018): per-note harmonics
/// become [`NoteMarks`], and each spanning technique present (hammer-on, slide,
/// palm-mute, vibrato) emits its own [`TechniqueSpan`]. Replaces the former
/// single-articulation flattening — marks and spans are now independent, so a
/// note that is both hammered and a harmonic keeps both.
fn map_gp_note_marks(
    effect: &GpNoteEffect,
    start: Ticks,
    duration: Ticks,
    spans: &mut Vec<TechniqueSpan>,
) -> NoteMarks {
    let end = Ticks(start.0.saturating_add(duration.0));
    let tick_range = TickRange { start, end };

    // Guitar Pro is a source of truth, so every span is Explicit (ADR-0018).
    let mut push_span = |technique| {
        spans.push(TechniqueSpan {
            technique,
            tick_range,
            evidence: TechniqueEvidence::explicit(),
        });
    };
    if effect.hammer {
        push_span(SpanTechnique::HammerOn);
    }
    if !effect.slides.is_empty() {
        push_span(SpanTechnique::Slide);
    }
    if effect.bend.is_some() {
        push_span(SpanTechnique::Bend);
    }
    if effect.palm_mute {
        push_span(SpanTechnique::PalmMute);
    }
    if effect.vibrato {
        push_span(SpanTechnique::Vibrato);
    }

    let mut marks = NoteMarks::empty();
    if effect.accentuated_note || effect.heavy_accentuated_note {
        marks.insert(NoteMark::Accent);
    }
    if effect.ghost_note {
        marks.insert(NoteMark::Ghost);
    }
    if effect.staccato {
        marks.insert(NoteMark::Staccato);
    }
    if let Some(h) = &effect.harmonic {
        match h.kind {
            guitarpro::HarmonicType::Pinch => marks.insert(NoteMark::HarmonicPinch),
            _ => marks.insert(NoteMark::HarmonicNatural),
        }
    }
    marks
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
    clippy::missing_assert_message,
    clippy::panic
)]
mod tests {
    use super::*;
    use guitarpro::model::effects::BendEffect;
    use guitarpro::model::key_signature::Duration as GpDurationTest;
    use std::collections::HashMap;

    #[test]
    fn bar_ticks_from_meter() {
        assert_eq!(bar_ticks(4, 4), 3840); // GP_PPQN 960 * 4/4 * 4
        assert_eq!(bar_ticks(2, 4), 1920);
        assert_eq!(bar_ticks(3, 4), 2880);
        assert_eq!(bar_ticks(6, 8), 2880);
    }

    #[test]
    fn cumulative_bar_starts_accumulate_per_meter() {
        // 4/4, 2/4, 4/4 → 0, 3840, 3840 + 1920 = 5760.
        assert_eq!(
            cumulative_bar_starts(&[(4, 4), (2, 4), (4, 4)]),
            vec![0, 3840, 5760]
        );
    }

    #[test]
    fn carry_tempos_holds_the_last_set_tempo() {
        // A tempo set once holds until changed; a leading gap is the 120 default.
        assert_eq!(
            carry_tempos(&[122, 0, 0, 90, 0]),
            vec![122.0, 122.0, 122.0, 90.0, 90.0]
        );
        assert_eq!(carry_tempos(&[0, 0]), vec![120.0, 120.0]);
    }

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

    // ── gp_note_position ──────────────────────────────────────────────────────

    #[test]
    fn gp_note_position_reads_string_and_fret() {
        // ADR-0018: Guitar Pro is a source of truth for (string, fret).
        let note = guitarpro::Note {
            value: 7, // fret 7
            string: 3,
            ..Default::default()
        };
        assert_eq!(
            gp_note_position(&note),
            Some(FretboardPosition { string: 3, fret: 7 })
        );
    }

    #[test]
    fn gp_note_position_rejects_invalid_string() {
        let note = guitarpro::Note {
            value: 5,
            string: 0, // invalid (GP strings are 1-indexed)
            ..Default::default()
        };
        assert_eq!(gp_note_position(&note), None);
    }

    // ── map_gp_note_marks ─────────────────────────────────────────────────────

    #[test]
    fn gp_note_marks_capture_accent_ghost_staccato() {
        // GP accent/ghost/staccato are source-of-truth NoteMarks (ADR-0018).
        let effect = GpNoteEffect {
            accentuated_note: true,
            ghost_note: true,
            staccato: true,
            ..GpNoteEffect::default()
        };
        let mut spans = Vec::new();
        let marks = map_gp_note_marks(&effect, Ticks(0), Ticks(480), &mut spans);
        assert!(marks.contains(NoteMark::Accent));
        assert!(marks.contains(NoteMark::Ghost));
        assert!(marks.contains(NoteMark::Staccato));
        assert!(spans.is_empty(), "these are per-note marks, not spans");
    }

    #[test]
    fn gp_hammer_emits_explicit_span() {
        let effect = GpNoteEffect {
            hammer: true,
            ..GpNoteEffect::default()
        };
        let mut spans = Vec::new();
        let _ = map_gp_note_marks(&effect, Ticks(0), Ticks(480), &mut spans);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].technique, SpanTechnique::HammerOn);
        // Guitar Pro is a source of truth.
        assert_eq!(spans[0].evidence, TechniqueEvidence::explicit());
    }

    #[test]
    fn gp_bend_emits_explicit_span() {
        // A bend is source-of-truth pitch expression (ADR-0018): it must surface
        // as a Bend TechniqueSpan, not be silently dropped.
        let effect = GpNoteEffect {
            bend: Some(BendEffect::default()),
            ..GpNoteEffect::default()
        };
        let mut spans = Vec::new();
        let _ = map_gp_note_marks(&effect, Ticks(0), Ticks(480), &mut spans);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].technique, SpanTechnique::Bend);
        assert_eq!(spans[0].evidence, TechniqueEvidence::explicit());
    }

    #[test]
    fn dead_note_imports_as_dead_marked_note() {
        // A muted "X" note (NoteType::Dead) still carries a real (string, fret):
        // it must import as a positioned note bearing NoteMark::DeadNote, not be
        // dropped to loss.
        let strings = vec![(1_i8, 64_i8), (2, 59), (3, 55), (4, 50), (5, 45), (6, 40)];
        let beat = guitarpro::Beat {
            notes: vec![guitarpro::Note {
                value: 5, // fret 5
                string: 3,
                kind: guitarpro::NoteType::Dead,
                ..Default::default()
            }],
            status: guitarpro::BeatStatus::Normal,
            ..Default::default()
        };
        let mut groups: Vec<EventGroup> = Vec::new();
        let mut held: HashMap<u8, (usize, usize)> = HashMap::new();
        let mut loss = LossReport::new();
        let mut acc = VoiceAccum {
            groups: &mut groups,
            held: &mut held,
            loss: &mut loss,
        };
        append_beat(&beat, 0, 480, &strings, &mut acc);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].atoms.len(), 1);
        let AtomEvent::Note(note) = &groups[0].atoms[0] else {
            panic!("a dead note must import as a Note, not a Rest");
        };
        assert!(note.marks.contains(NoteMark::DeadNote));
        assert_eq!(
            note.position.map(|p| p.position),
            Some(FretboardPosition { string: 3, fret: 5 })
        );
    }

    #[test]
    fn tied_note_extends_previous_note_duration() {
        // A tie (NoteType::Tie) continues the previous note on the same string:
        // it must extend that note's duration, not drop the held time nor emit a
        // separate event.
        let strings = vec![(1_i8, 64_i8), (2, 59), (3, 55), (4, 50), (5, 45), (6, 40)];
        let struck = guitarpro::Beat {
            notes: vec![guitarpro::Note {
                value: 2,
                string: 3,
                kind: guitarpro::NoteType::Normal,
                ..Default::default()
            }],
            status: guitarpro::BeatStatus::Normal,
            ..Default::default()
        };
        let tie = guitarpro::Beat {
            notes: vec![guitarpro::Note {
                value: 2,
                string: 3,
                kind: guitarpro::NoteType::Tie,
                ..Default::default()
            }],
            status: guitarpro::BeatStatus::Normal,
            ..Default::default()
        };

        let mut groups: Vec<EventGroup> = Vec::new();
        let mut held: HashMap<u8, (usize, usize)> = HashMap::new();
        let mut loss = LossReport::new();
        let mut acc = VoiceAccum {
            groups: &mut groups,
            held: &mut held,
            loss: &mut loss,
        };
        append_beat(&struck, 0, 480, &strings, &mut acc);
        append_beat(&tie, 480, 240, &strings, &mut acc);

        assert_eq!(groups.len(), 1, "a tie must not create a new event group");
        assert_eq!(groups[0].atoms.len(), 1);
        let AtomEvent::Note(note) = &groups[0].atoms[0] else {
            panic!("expected the struck note");
        };
        assert_eq!(
            note.duration.0, 720,
            "tie must extend the held note to struck + tie duration"
        );
    }

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
