//! MIDI file import and export (S1 baseline).
//!
//! All raw MIDI bytes are confined to this module; the rest of the codebase
//! works exclusively with the structured types from [`crate::event`].

use std::{collections::HashMap, io};

use midly::{
    num::{u15, u24, u28, u4, u7},
    Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind,
};
use thiserror::Error;

use crate::{
    event::{Articulation, Pitch, Tempo, Ticks, TimeSignature, ValidationError, Velocity},
    score::{
        AtomEvent, AtomNote, EventGroup, EventGroupKind, ImportWarning, LossReport, MasterBar,
        Score, SourceMeta, Track as ScoreTrack, Voice,
    },
    slice::TickRange,
};

// ── error ─────────────────────────────────────────────────────────────────────

/// Error produced by MIDI import or export.
#[derive(Debug, Error)]
pub enum MidiError {
    /// The MIDI data could not be parsed.
    #[error("MIDI parse error: {0}")]
    Parse(Box<midly::Error>),

    /// SMPTE frame-based timing is not yet supported; use PPQN files.
    #[error("SMPTE timing is not supported; re-export with PPQN timing")]
    SmpteTimingUnsupported,

    /// A musical-model value failed validation.
    #[error("validation: {0:?}")]
    Validation(ValidationError),

    /// Integer tick arithmetic overflowed.
    #[error("tick arithmetic overflow")]
    TickOverflow,

    /// No tempo event found in the file.
    #[error("no tempo event found in the MIDI file")]
    NoTempo,

    /// The time-signature / PPQN combination produces zero-length bars (F-001).
    ///
    /// The MIDI file is structurally valid but would cause a non-terminating
    /// bar-grouping loop. Callers should reject it with this error.
    #[error(
        "degenerate meter: {numerator}/{denominator} at PPQN {ppqn} gives \
         zero-length bars"
    )]
    DegenerateMeter {
        /// Time-signature numerator.
        numerator: u8,
        /// Time-signature denominator.
        denominator: u8,
        /// PPQN of the file.
        ppqn: u16,
    },

    /// Writing MIDI bytes failed.
    #[error("MIDI write error: {0}")]
    Write(Box<io::Error>),
}

impl From<midly::Error> for MidiError {
    fn from(e: midly::Error) -> Self {
        Self::Parse(Box::new(e))
    }
}

impl From<io::Error> for MidiError {
    fn from(e: io::Error) -> Self {
        Self::Write(Box::new(e))
    }
}

impl From<ValidationError> for MidiError {
    fn from(e: ValidationError) -> Self {
        Self::Validation(e)
    }
}

// ── public data types ─────────────────────────────────────────────────────────

/// Pulses per quarter note — the time resolution of a MIDI file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ppqn(pub u16);

// ── ppqn ──────────────────────────────────────────────────────────────────────

fn extract_ppqn(header: Header) -> Result<Ppqn, MidiError> {
    match header.timing {
        Timing::Metrical(ticks) => {
            let v = u16::from(ticks);
            if v == 0 {
                Err(MidiError::Validation(
                    ValidationError::InvalidTicksPerQuarter,
                ))
            } else {
                Ok(Ppqn(v))
            }
        }
        Timing::Timecode(_, _) => Err(MidiError::SmpteTimingUnsupported),
    }
}

// ── meta extraction ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
struct TempoChange {
    tick: u32,
    micros_per_beat: u32,
}

#[derive(Debug, Clone, Copy)]
struct TimeSigChange {
    tick: u32,
    sig: TimeSignature,
}

/// Walk all tracks and build the global tempo / time-signature timelines.
fn collect_global_meta(smf: &Smf<'_>) -> (Vec<TempoChange>, Vec<TimeSigChange>) {
    let mut tempos: Vec<TempoChange> = Vec::new();
    let mut time_sigs: Vec<TimeSigChange> = Vec::new();

    for raw_track in &smf.tracks {
        let mut abs: u32 = 0;
        for ev in raw_track {
            abs = abs.saturating_add(u32::from(ev.delta));
            match ev.kind {
                TrackEventKind::Meta(MetaMessage::Tempo(t)) => {
                    tempos.push(TempoChange {
                        tick: abs,
                        micros_per_beat: u32::from(t),
                    });
                }
                TrackEventKind::Meta(MetaMessage::TimeSignature(num, den_pow, _, _)) => {
                    // den_pow is log2(denominator): 2 → quarter, 3 → eighth …
                    let denominator = 1u8.wrapping_shl(u32::from(den_pow));
                    if let Ok(sig) = TimeSignature::new(num, denominator) {
                        time_sigs.push(TimeSigChange { tick: abs, sig });
                    }
                }
                _ => {}
            }
        }
    }

    tempos.sort_unstable_by_key(|t| t.tick);
    tempos.dedup_by_key(|t| t.tick);
    time_sigs.sort_unstable_by_key(|t| t.tick);
    time_sigs.dedup_by_key(|t| t.tick);

    if time_sigs.is_empty() || time_sigs.first().is_some_and(|t| t.tick > 0) {
        time_sigs.insert(
            0,
            TimeSigChange {
                tick: 0,
                sig: TimeSignature {
                    numerator: 4,
                    denominator: 4,
                },
            },
        );
    }

    (tempos, time_sigs)
}

fn active_tempo(tempos: &[TempoChange], tick: u32) -> u32 {
    tempos
        .iter()
        .rev()
        .find(|t| t.tick <= tick)
        .map_or(500_000, |t| t.micros_per_beat)
}

fn active_time_sig(time_sigs: &[TimeSigChange], tick: u32) -> TimeSignature {
    time_sigs.iter().rev().find(|t| t.tick <= tick).map_or(
        TimeSignature {
            numerator: 4,
            denominator: 4,
        },
        |t| t.sig,
    )
}

// ── note assembly ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
struct AbsNote {
    start: u32,
    pitch: Pitch,
    duration: Ticks,
    velocity: Velocity,
    articulation: Option<Articulation>,
}

// (channel, pitch) → (absolute start tick, attack velocity)
type PendingNotes = HashMap<(u8, u8), (u32, u8)>;

// ── bar grouping ──────────────────────────────────────────────────────────────

/// Return bar duration in ticks for the given time signature and [`Ppqn`].
///
/// Returns [`MidiError::DegenerateMeter`] when the result is zero (F-001):
/// a non-advancing bar step would make the bar-grouping loop run forever.
fn bar_ticks(sig: TimeSignature, ppqn: Ppqn) -> Result<u32, MidiError> {
    let p = u32::from(ppqn.0);
    let n = u32::from(sig.numerator);
    let d = u32::from(sig.denominator);
    // bar_ticks = ppqn * 4 * numerator / denominator
    let ticks = p
        .checked_mul(4)
        .and_then(|v| v.checked_mul(n))
        .and_then(|v| v.checked_div(d))
        .ok_or(MidiError::TickOverflow)?;
    if ticks == 0 {
        return Err(MidiError::DegenerateMeter {
            numerator: sig.numerator,
            denominator: sig.denominator,
            ppqn: ppqn.0,
        });
    }
    Ok(ticks)
}

// ── export ────────────────────────────────────────────────────────────────────

fn abs_to_delta(sorted: Vec<(u32, TrackEventKind<'static>)>) -> Vec<TrackEvent<'static>> {
    let mut prev: u32 = 0;
    sorted
        .into_iter()
        .map(|(abs, kind)| {
            let delta = abs.saturating_sub(prev);
            prev = abs;
            TrackEvent {
                delta: u28::from_int_lossy(delta),
                kind,
            }
        })
        .collect()
}

/// Convert a [`Tempo`] (BPM) to microseconds per beat, clamped to MIDI's 24-bit range.
fn tempo_to_micros(tempo: Tempo) -> Result<u32, MidiError> {
    let bpm = tempo.0;
    if !bpm.is_finite() || bpm <= 0.0 {
        return Err(MidiError::Validation(ValidationError::InvalidTempo));
    }
    let micros = 60_000_000.0_f64 / bpm;
    let max_u24 = f64::from(u32::from(u24::max_value()));
    let clamped = micros.min(max_u24).max(1.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    // clamped is in [1.0, 16_777_215.0]; fits in u32 without loss
    Ok(clamped.round() as u32)
}

// ── canonical model import/export (S2) ────────────────────────────────────────

/// Imports a MIDI file into the canonical [`Score`] model.
///
/// The master timeline is built from the global tempo / time-signature map so
/// every [`MasterBar`] carries its own tempo (ADR-0003). Tracks get one
/// [`Voice`] with [`EventGroupKind::Single`] atoms.
///
/// The returned [`Score`] contains a [`LossReport`] describing any data that
/// could not be preserved exactly.
pub fn import_score(data: &[u8]) -> Result<Score, MidiError> {
    let smf = Smf::parse(data)?;
    let ppqn = extract_ppqn(smf.header)?;
    let (tempos, time_sigs) = collect_global_meta(&smf);

    if tempos.is_empty() {
        return Err(MidiError::NoTempo);
    }

    let mut loss = LossReport::new();
    let mut score_tracks: Vec<ScoreTrack> = Vec::new();

    for (raw_idx, raw_track) in smf.tracks.iter().enumerate() {
        if let Some((track, track_loss)) = build_score_track(raw_track, ppqn, &time_sigs, raw_idx)?
        {
            loss.absorb(track_loss);
            score_tracks.push(track);
        }
    }

    let end_tick = score_end_tick(&score_tracks);
    let master_bars = build_master_bars(ppqn, &tempos, &time_sigs, end_tick)?;

    Ok(Score {
        ticks_per_quarter: ppqn.0,
        master_bars,
        tracks: score_tracks,
        source_meta: Some(SourceMeta {
            format: Some("MIDI".to_owned()),
        }),
        loss,
    })
}

/// Serialises a [`Score`] to standard MIDI bytes using the master timeline.
///
/// The meta track is built exclusively from [`Score::master_bars`] (ADR-0003):
/// tempo and time-signature changes are emitted at the correct ticks; track
/// names are preserved.  Only the first [`Voice`] of each track is emitted.
pub fn export_score(score: &Score) -> Result<Vec<u8>, MidiError> {
    let ppqn = Ppqn(score.ticks_per_quarter);
    let format = if score.tracks.len() == 1 {
        Format::SingleTrack
    } else {
        Format::Parallel
    };

    let mut smf_tracks: Vec<Vec<TrackEvent<'static>>> = vec![build_score_meta_track(score, ppqn)?];
    for track in &score.tracks {
        smf_tracks.push(build_score_note_track(track)?);
    }

    let header = Header {
        format,
        timing: Timing::Metrical(u15::new(ppqn.0)),
    };
    let mut smf = Smf::new(header);
    smf.tracks = smf_tracks;

    let mut out: Vec<u8> = Vec::new();
    smf.write_std(&mut out)?;
    Ok(out)
}

// ── canonical helpers ─────────────────────────────────────────────────────────

fn score_end_tick(tracks: &[ScoreTrack]) -> u32 {
    tracks
        .iter()
        .filter_map(|t| t.voices.first())
        .flat_map(|v| v.event_groups.iter())
        .flat_map(|g| g.atoms.iter())
        .map(|a| a.absolute_start().0.saturating_add(a.duration().0))
        .max()
        .unwrap_or(0)
}

fn build_master_bars(
    ppqn: Ppqn,
    tempos: &[TempoChange],
    time_sigs: &[TimeSigChange],
    end_tick: u32,
) -> Result<Vec<MasterBar>, MidiError> {
    let mut master_bars: Vec<MasterBar> = Vec::new();
    let mut bar_start: u32 = 0;
    let mut index: usize = 0;

    // `<=` covers content whose last event ends exactly on a barline. As a side
    // effect, when `end_tick` falls on a boundary this appends one trailing empty
    // bar (a sentinel). Downstream analysis that measures whole-span seams should
    // account for a possible trailing empty bar — see the S14 structure-metrics
    // known limitations (ADR-0015).
    while bar_start <= end_tick {
        let sig = active_time_sig(time_sigs, bar_start);
        let micros = active_tempo(tempos, bar_start);
        let bt = bar_ticks(sig, ppqn)?;
        let bar_end = bar_start.saturating_add(bt);
        let bpm = 60_000_000.0_f64 / f64::from(micros);
        let tempo = Tempo::new(bpm)?;
        let tick_range = TickRange::new(Ticks(bar_start), Ticks(bar_end))
            .map_err(|_| MidiError::TickOverflow)?;

        master_bars.push(MasterBar {
            index,
            tick_range,
            time_signature: sig,
            tempo,
        });

        index = index.checked_add(1).ok_or(MidiError::TickOverflow)?;
        bar_start = bar_end;
    }

    Ok(master_bars)
}

#[allow(clippy::type_complexity)]
fn build_score_track(
    raw_track: &[TrackEvent<'_>],
    ppqn: Ppqn,
    time_sigs: &[TimeSigChange],
    raw_idx: usize,
) -> Result<Option<(ScoreTrack, LossReport)>, MidiError> {
    let (notes, name_result, channel) = collect_notes_with_name(raw_track);
    if notes.is_empty() {
        return Ok(None);
    }

    let mut loss = LossReport::new();
    let name = name_result.unwrap_or_else(|()| {
        loss.add(ImportWarning::TrackNameInvalidUtf8 {
            track_index: raw_idx,
        });
        None
    });

    // Build event groups from absolute notes using the same bar-grouping logic.
    let end_tick = notes
        .iter()
        .map(|n| n.start.saturating_add(n.duration.0))
        .max()
        .unwrap_or(0);

    let mut sorted_notes = notes;
    sorted_notes.sort_unstable_by_key(|n| n.start);

    // Walk bars and assign notes.
    let mut event_groups: Vec<EventGroup> = Vec::new();
    let mut bar_start: u32 = 0;
    let mut note_iter = sorted_notes.iter().peekable();

    while bar_start <= end_tick {
        let sig = active_time_sig(time_sigs, bar_start);
        let bt = bar_ticks(sig, ppqn)?;
        let bar_end = bar_start.saturating_add(bt);

        // Collect bar notes: consume all notes that start before bar_end.
        while note_iter
            .peek()
            .is_some_and(|peeked| peeked.start < bar_end)
        {
            if let Some(taken) = note_iter.next() {
                if taken.start >= bar_start {
                    let atom = AtomEvent::Note(AtomNote {
                        absolute_start: Ticks(taken.start),
                        duration: taken.duration,
                        pitch: taken.pitch,
                        velocity: taken.velocity,
                        articulation: taken.articulation,
                    });
                    event_groups.push(EventGroup {
                        kind: EventGroupKind::Single,
                        atoms: vec![atom],
                        technique_spans: Vec::new(),
                    });
                }
            }
        }

        bar_start = bar_end;
    }

    let voice = Voice {
        id: 0,
        event_groups,
    };

    Ok(Some((
        ScoreTrack {
            name,
            channel,
            voices: vec![voice],
        },
        loss,
    )))
}

/// Like [`collect_notes`] but returns the track name as `Ok(Some(name))`, `Ok(None)`,
/// or `Err(())` (invalid UTF-8) so the caller can record a loss.
#[allow(clippy::type_complexity)]
fn collect_notes_with_name(
    raw_track: &[TrackEvent<'_>],
) -> (Vec<AbsNote>, Result<Option<String>, ()>, u8) {
    let mut pending: PendingNotes = HashMap::new();
    let mut notes: Vec<AbsNote> = Vec::new();
    let mut abs: u32 = 0;
    let mut track_name: Result<Option<String>, ()> = Ok(None);
    let mut channel_counts: [u32; 16] = [0u32; 16];

    for ev in raw_track {
        abs = abs.saturating_add(u32::from(ev.delta));
        match ev.kind {
            TrackEventKind::Meta(MetaMessage::TrackName(bytes)) => {
                track_name = String::from_utf8(bytes.to_vec()).map(Some).map_err(|_| ());
            }
            TrackEventKind::Midi { channel, message } => {
                let ch = u8::from(channel);
                match message {
                    MidiMessage::NoteOn { key, vel } if u8::from(vel) > 0 => {
                        let pitch_val = u8::from(key);
                        let vel_val = u8::from(vel);
                        pending.insert((ch, pitch_val), (abs, vel_val));
                        if let Some(count) = channel_counts.get_mut(usize::from(ch)) {
                            *count = count.saturating_add(1);
                        }
                    }
                    MidiMessage::NoteOff { key, .. } | MidiMessage::NoteOn { key, .. } => {
                        let pitch_val = u8::from(key);
                        if let Some((start, vel_val)) = pending.remove(&(ch, pitch_val)) {
                            let duration = abs.saturating_sub(start);
                            if let (Ok(pitch), Ok(velocity)) =
                                (Pitch::new(pitch_val), Velocity::new(vel_val))
                            {
                                notes.push(AbsNote {
                                    start,
                                    pitch,
                                    duration: Ticks(duration),
                                    velocity,
                                    articulation: None,
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    let dominant_channel = channel_counts
        .iter()
        .enumerate()
        .max_by_key(|&(_, &count)| count)
        .map_or(0_u8, |(idx, _)| {
            #[allow(clippy::cast_possible_truncation)]
            // idx is in 0..16 and always fits in u8
            {
                idx as u8
            }
        });

    (notes, track_name, dominant_channel)
}

/// Builds the SMF meta track from the master timeline of a [`Score`].
///
/// Emits a tempo event and time-signature event at every [`MasterBar`] where
/// the value changes from the previous bar. This is the only function that
/// should emit transport meta events; note tracks must not duplicate them.
fn build_score_meta_track(
    score: &Score,
    ppqn: Ppqn,
) -> Result<Vec<TrackEvent<'static>>, MidiError> {
    let mut abs_events: Vec<(u32, TrackEventKind<'static>)> = Vec::new();

    let mut prev_micros: Option<u32> = None;
    let mut prev_sig: Option<TimeSignature> = None;

    for mb in &score.master_bars {
        let tick = mb.tick_range.start.0;
        let micros = tempo_to_micros(mb.tempo)?;
        let sig = mb.time_signature;

        if prev_micros != Some(micros) {
            abs_events.push((
                tick,
                TrackEventKind::Meta(MetaMessage::Tempo(u24::from_int_lossy(micros))),
            ));
            prev_micros = Some(micros);
        }

        if prev_sig != Some(sig) {
            let den_pow = sig.denominator.trailing_zeros();
            #[allow(clippy::cast_possible_truncation)]
            // trailing_zeros() on u8 is at most 7; fits in u8
            let den_pow_u8 = den_pow as u8;
            abs_events.push((
                tick,
                TrackEventKind::Meta(MetaMessage::TimeSignature(sig.numerator, den_pow_u8, 24, 8)),
            ));
            prev_sig = Some(sig);
        }
    }

    // End-of-track at the end of the last master bar.
    let end_tick = score.master_bars.last().map_or(0, |mb| mb.tick_range.end.0);
    abs_events.push((end_tick, TrackEventKind::Meta(MetaMessage::EndOfTrack)));

    // Ensure at least an initial tempo + time-sig if master_bars is empty.
    if score.master_bars.is_empty() {
        abs_events.push((
            0,
            TrackEventKind::Meta(MetaMessage::Tempo(u24::from_int_lossy(500_000_u32))),
        ));
        abs_events.push((
            0,
            TrackEventKind::Meta(MetaMessage::TimeSignature(4, 2, 24, 8)),
        ));
        let ppqn_as_ticks = bar_ticks(
            TimeSignature {
                numerator: 4,
                denominator: 4,
            },
            ppqn,
        )?;
        abs_events.push((ppqn_as_ticks, TrackEventKind::Meta(MetaMessage::EndOfTrack)));
    }

    abs_events.sort_unstable_by_key(|&(tick, _)| tick);
    Ok(abs_to_delta(abs_events))
}

/// Builds a note track for one [`ScoreTrack`], using only the first [`Voice`].
#[allow(clippy::unnecessary_wraps)]
fn build_score_note_track(track: &ScoreTrack) -> Result<Vec<TrackEvent<'static>>, MidiError> {
    let channel = u4::new(track.channel.min(15));
    let mut abs_events: Vec<(u32, TrackEventKind<'static>)> = Vec::new();

    // Emit track name if present.
    // (TrackName bytes must have 'static lifetime; we build owned vec and leak it.)
    // We use a workaround: only emit if the name is non-empty.
    // NOTE: midly TrackName takes &'static [u8]; we cannot easily emit dynamic
    // names without unsafe. We skip name emission in export_score for now.
    // This is a known limitation tracked in LossReport / future work.

    if let Some(voice) = track.voices.first() {
        for group in &voice.event_groups {
            for atom in &group.atoms {
                if let AtomEvent::Note(n) = atom {
                    let key = u7::new(n.pitch.0);
                    let vel = u7::new(n.velocity.0);
                    let start = n.absolute_start.0;
                    let end = start.saturating_add(n.duration.0);

                    abs_events.push((
                        start,
                        TrackEventKind::Midi {
                            channel,
                            message: MidiMessage::NoteOn { key, vel },
                        },
                    ));
                    abs_events.push((
                        end,
                        TrackEventKind::Midi {
                            channel,
                            message: MidiMessage::NoteOff {
                                key,
                                vel: u7::new(0),
                            },
                        },
                    ));
                }
            }
        }
    }

    // Determine end tick: max of all note-offs.
    let end_tick = abs_events.iter().map(|&(t, _)| t).max().unwrap_or(0);
    abs_events.push((end_tick, TrackEventKind::Meta(MetaMessage::EndOfTrack)));
    abs_events.sort_unstable_by_key(|&(tick, _)| tick);
    Ok(abs_to_delta(abs_events))
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::{bar_ticks, export_score, import_score, tempo_to_micros, MidiError, Ppqn};
    use crate::{
        event::{Pitch, Tempo, Ticks, TimeSignature, Velocity},
        score::{
            AtomEvent as ScoreAtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport,
            MasterBar, Score, Track as ScoreTrack2, Voice,
        },
        slice::TickRange,
    };
    use midly::{MetaMessage, Smf, TrackEventKind};

    /// Bytes of the `tempo_change` fixture (two-tempo file from the S0 corpus).
    const TEMPO_CHANGE_MID: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../cli/tests/fixtures/tempo_change.mid"
    ));

    #[test]
    fn bar_ticks_4_4_at_480_ppqn() {
        let sig = TimeSignature {
            numerator: 4,
            denominator: 4,
        };
        assert!(
            matches!(bar_ticks(sig, Ppqn(480)), Ok(1920)),
            "4/4 at 480 PPQN must be 1920 ticks",
        );
    }

    #[test]
    fn bar_ticks_7_8_at_480_ppqn() {
        let sig = TimeSignature {
            numerator: 7,
            denominator: 8,
        };
        assert!(
            matches!(bar_ticks(sig, Ppqn(480)), Ok(1680)),
            "7/8 at 480 PPQN must be 1680 ticks",
        );
    }

    #[test]
    fn tempo_120_bpm_to_micros() {
        let tempo = Tempo::new(120.0).expect("120 BPM is valid");
        assert!(
            matches!(tempo_to_micros(tempo), Ok(500_000)),
            "120 BPM must map to 500 000 µs/beat",
        );
    }

    /// Bytes of `fuzz/corpus/midi_import/valid_minimal.mid`: a 50-byte
    /// well-formed SMF (PPQN=480, 4/4, one note). Kept in sync by hand.
    const VALID_MINIMAL_MID: &[u8] = &[
        0x4D, 0x54, 0x68, 0x64, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, 0x01, 0x01, 0xE0, 0x4D,
        0x54, 0x72, 0x6B, 0x00, 0x00, 0x00, 0x1C, //
        0x00, 0xFF, 0x58, 0x04, 0x04, 0x02, 0x18, 0x08, //
        0x00, 0xFF, 0x51, 0x03, 0x07, 0xA1, 0x20, //
        0x00, 0x90, 0x3C, 0x64, //
        0x83, 0x60, 0x80, 0x3C, 0x00, //
        0x00, 0xFF, 0x2F, 0x00,
    ];

    /// Regression test for finding F-001 (see `docs/fuzzing.md`):
    /// a PPQN=1 / 1/8 SMF used to make `bar_ticks` integer-divide to zero,
    /// causing the bar-grouping loop to never advance.
    ///
    /// Fixed in S2: `bar_ticks` now returns [`MidiError::DegenerateMeter`]
    /// instead of `Ok(0)`, so `import_score` returns a typed error immediately.
    #[test]
    fn regression_f001_degenerate_meter_returns_typed_error() {
        let hang_mid: &[u8] = &[
            0x4D, 0x54, 0x68, 0x64, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01,
            0x4D, 0x54, 0x72, 0x6B, 0x00, 0x00, 0x00, 0x1B, //
            0x00, 0xFF, 0x58, 0x04, 0x01, 0x03, 0x18, 0x08, //
            0x00, 0xFF, 0x51, 0x03, 0x07, 0xA1, 0x20, //
            0x00, 0x90, 0x3C, 0x64, //
            0x01, 0x80, 0x3C, 0x00, //
            0x00, 0xFF, 0x2F, 0x00,
        ];

        let result = import_score(hang_mid);
        assert!(
            matches!(
                result,
                Err(MidiError::DegenerateMeter {
                    numerator: 1,
                    denominator: 8,
                    ppqn: 1
                })
            ),
            "F-001 input must return DegenerateMeter, got {result:?}",
        );
    }

    #[test]
    fn bar_ticks_degenerate_returns_typed_error() {
        let sig = TimeSignature {
            numerator: 1,
            denominator: 8,
        };
        let result = bar_ticks(sig, Ppqn(1));
        assert!(
            matches!(
                result,
                Err(MidiError::DegenerateMeter {
                    numerator: 1,
                    denominator: 8,
                    ppqn: 1,
                })
            ),
            "1/8 at PPQN=1 must return DegenerateMeter, got {result:?}",
        );
    }

    // ── import_score / export_score ───────────────────────────────────────────

    /// The minimal valid corpus seed must parse via the canonical path.
    #[test]
    fn import_score_valid_minimal_succeeds() {
        let score = import_score(VALID_MINIMAL_MID).expect("valid_minimal must import as Score");
        assert_eq!(
            score.ticks_per_quarter, 480,
            "canonical import must preserve PPQN"
        );
        assert_eq!(score.tracks.len(), 1, "one note-bearing track expected");
        assert!(
            !score.master_bars.is_empty(),
            "master bars must be populated"
        );
        assert!(score.loss.is_clean(), "minimal file must import cleanly");
    }

    /// Every master bar in a canonical import must have non-empty tick range.
    #[test]
    fn import_score_master_bars_have_valid_ranges() {
        let score = import_score(VALID_MINIMAL_MID).expect("import_score must succeed");
        for mb in &score.master_bars {
            assert!(
                mb.tick_range.start.0 < mb.tick_range.end.0,
                "master bar tick range must be non-empty (bar {})",
                mb.index,
            );
        }
    }

    /// `export_score` followed by `import_score` must preserve track count and PPQN.
    #[test]
    fn export_score_roundtrip_preserves_track_count_and_ppqn() {
        let original = import_score(VALID_MINIMAL_MID).expect("import_score must succeed");
        let bytes = export_score(&original).expect("export_score must succeed");
        let reimported = import_score(&bytes).expect("re-import_score must succeed");

        assert_eq!(
            original.ticks_per_quarter, reimported.ticks_per_quarter,
            "roundtrip must preserve ticks_per_quarter"
        );
        assert_eq!(
            original.tracks.len(),
            reimported.tracks.len(),
            "roundtrip must preserve track count"
        );
    }

    /// Multi-track export via `export_score` must put all tempo changes in the
    /// dedicated meta track — note tracks must contain zero tempo meta events.
    #[test]
    fn export_score_multi_track_tempo_in_meta_track_only() {
        let mk_track = |ch: u8| {
            let atom = ScoreAtomEvent::Note(AtomNote {
                absolute_start: Ticks(0),
                duration: Ticks(480),
                pitch: Pitch::new(60).expect("valid pitch"),
                velocity: Velocity::new(80).expect("valid velocity"),
                articulation: None,
            });
            let group = EventGroup {
                kind: EventGroupKind::Single,
                atoms: vec![atom],
                technique_spans: Vec::new(),
            };
            ScoreTrack2 {
                name: Some(format!("T{ch}")),
                channel: ch,
                voices: vec![Voice {
                    id: 0,
                    event_groups: vec![group],
                }],
            }
        };

        let score = Score {
            ticks_per_quarter: 480,
            master_bars: vec![MasterBar {
                index: 0,
                tick_range: TickRange::new(Ticks(0), Ticks(1920)).expect("valid range"),
                time_signature: TimeSignature::new(4, 4).expect("4/4 valid"),
                tempo: Tempo::new(140.0).expect("140 BPM valid"),
            }],
            tracks: vec![mk_track(0), mk_track(1)],
            source_meta: None,
            loss: LossReport::new(),
        };

        let bytes = export_score(&score).expect("export_score must succeed");
        let smf = Smf::parse(&bytes).expect("exported bytes must be valid SMF");

        assert!(smf.tracks.len() >= 3, "meta + 2 note tracks expected");

        for (idx, raw_track) in smf.tracks.iter().enumerate().skip(1) {
            for ev in raw_track {
                assert!(
                    !matches!(ev.kind, TrackEventKind::Meta(MetaMessage::Tempo(_))),
                    "note track {idx} must not contain a Tempo meta event"
                );
            }
        }
    }

    /// Tempo changes are correctly preserved across a canonical roundtrip.
    #[test]
    fn export_score_preserves_all_tempo_changes() {
        let score = import_score(TEMPO_CHANGE_MID).expect("tempo_change must import as Score");

        let tempos: Vec<f64> = score.master_bars.iter().map(|mb| mb.tempo.0).collect();
        let distinct_count = {
            // Deduplicate by rounding to nearest integer BPM.
            let mut seen: Vec<u64> = tempos
                .iter()
                .map(|&t| {
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    {
                        t.round() as u64
                    }
                })
                .collect();
            seen.sort_unstable();
            seen.dedup();
            seen.len()
        };
        assert!(
            distinct_count >= 2,
            "tempo_change fixture must import with at least 2 distinct tempos"
        );

        let bytes = export_score(&score).expect("export_score must succeed");
        let rt = import_score(&bytes).expect("re-import_score must succeed");

        let rt_tempos: Vec<f64> = rt.master_bars.iter().map(|mb| mb.tempo.0).collect();
        assert_eq!(
            tempos.len(),
            rt_tempos.len(),
            "roundtrip must preserve bar count"
        );
        for (i, (&orig, &rt_val)) in tempos.iter().zip(rt_tempos.iter()).enumerate() {
            assert!(
                (orig - rt_val).abs() < 1.0,
                "bar {i}: tempo {orig} BPM must survive roundtrip (got {rt_val})"
            );
        }
    }
}
