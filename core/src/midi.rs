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

use crate::event::{
    Bar, Event, Note, Phrase, Pitch, Rest, Tempo, Ticks, TimeSignature, ValidationError, Velocity,
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

/// One track as imported from a MIDI file.
#[derive(Debug, Clone)]
pub struct MidiTrack {
    /// Optional track name from the MIDI metadata.
    pub name: Option<String>,
    /// MIDI channel (0–15) used by the majority of events in this track.
    pub channel: u8,
    /// All musical content grouped into bars.
    pub phrase: Phrase,
}

/// A fully imported MIDI file.
#[derive(Debug, Clone)]
pub struct MidiSong {
    /// Pulses per quarter note.
    pub ppqn: Ppqn,
    /// Tracks that contain at least one note.
    pub tracks: Vec<MidiTrack>,
}

// ── import ────────────────────────────────────────────────────────────────────

/// Parses raw MIDI bytes into a [`MidiSong`].
pub fn import(data: &[u8]) -> Result<MidiSong, MidiError> {
    let smf = Smf::parse(data)?;
    let ppqn = extract_ppqn(smf.header)?;

    let (tempos, time_sigs) = collect_global_meta(&smf);

    if tempos.is_empty() {
        return Err(MidiError::NoTempo);
    }

    let mut tracks: Vec<MidiTrack> = Vec::new();
    for raw_track in &smf.tracks {
        if let Some(t) = build_track(raw_track, ppqn, &tempos, &time_sigs)? {
            tracks.push(t);
        }
    }

    Ok(MidiSong { ppqn, tracks })
}

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
    note: Note,
}

// (channel, pitch) → (absolute start tick, attack velocity)
type PendingNotes = HashMap<(u8, u8), (u32, u8)>;

/// Convert delta-time track events into [`AbsNote`] objects with paired durations.
fn collect_notes(raw_track: &[TrackEvent<'_>]) -> (Vec<AbsNote>, Option<String>, u8) {
    let mut pending: PendingNotes = HashMap::new();
    let mut notes: Vec<AbsNote> = Vec::new();
    let mut abs: u32 = 0;
    let mut track_name: Option<String> = None;
    let mut channel_counts: [u32; 16] = [0u32; 16];

    for ev in raw_track {
        abs = abs.saturating_add(u32::from(ev.delta));
        match ev.kind {
            TrackEventKind::Meta(MetaMessage::TrackName(bytes)) => {
                track_name = String::from_utf8(bytes.to_vec()).ok();
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
                    // NoteOff or NoteOn with vel=0 both terminate a note.
                    MidiMessage::NoteOff { key, .. } | MidiMessage::NoteOn { key, .. } => {
                        let pitch_val = u8::from(key);
                        if let Some((start, vel_val)) = pending.remove(&(ch, pitch_val)) {
                            let duration = abs.saturating_sub(start);
                            if let (Ok(pitch), Ok(velocity)) =
                                (Pitch::new(pitch_val), Velocity::new(vel_val))
                            {
                                notes.push(AbsNote {
                                    start,
                                    note: Note {
                                        pitch,
                                        duration: Ticks(duration),
                                        velocity,
                                        articulation: None,
                                    },
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

// ── bar grouping ──────────────────────────────────────────────────────────────

/// Return bar duration in ticks for the given time signature and [`Ppqn`].
fn bar_ticks(sig: TimeSignature, ppqn: Ppqn) -> Result<u32, MidiError> {
    let p = u32::from(ppqn.0);
    let n = u32::from(sig.numerator);
    let d = u32::from(sig.denominator);
    // bar_ticks = ppqn * 4 * numerator / denominator
    p.checked_mul(4)
        .and_then(|v| v.checked_mul(n))
        .and_then(|v| v.checked_div(d))
        .ok_or(MidiError::TickOverflow)
}

/// Convert a flat list of absolute notes into a [`Phrase`] of [`Bar`]s.
fn group_into_bars(
    mut notes: Vec<AbsNote>,
    ppqn: Ppqn,
    tempos: &[TempoChange],
    time_sigs: &[TimeSigChange],
) -> Result<Phrase, MidiError> {
    notes.sort_unstable_by_key(|n| n.start);

    let end_tick = notes
        .iter()
        .map(|n| n.start.saturating_add(n.note.duration.0))
        .max()
        .unwrap_or(0);

    let mut bars: Vec<Bar> = Vec::new();
    let mut bar_start: u32 = 0;

    while bar_start <= end_tick {
        let sig = active_time_sig(time_sigs, bar_start);
        let micros = active_tempo(tempos, bar_start);
        let bt = bar_ticks(sig, ppqn)?;
        let bar_end = bar_start.saturating_add(bt);

        let bpm = 60_000_000.0_f64 / f64::from(micros);
        let tempo = Tempo::new(bpm)?;

        let bar_notes: Vec<AbsNote> = notes
            .iter()
            .filter(|n| n.start >= bar_start && n.start < bar_end)
            .copied()
            .collect();

        let events = build_bar_events(&bar_notes, bar_start, bar_end);
        bars.push(Bar {
            time_signature: sig,
            tempo,
            events,
        });

        bar_start = bar_end;
    }

    Ok(Phrase { bars })
}

/// Fill a bar's tick range with [`Event`]s, inserting [`Rest`]s for gaps.
fn build_bar_events(notes: &[AbsNote], bar_start: u32, bar_end: u32) -> Vec<Event> {
    let mut events: Vec<Event> = Vec::new();
    let mut cursor = bar_start;

    for abs_note in notes {
        if abs_note.start > cursor {
            let gap = abs_note.start.saturating_sub(cursor);
            events.push(Event::Rest(Rest {
                duration: Ticks(gap),
            }));
        }
        events.push(Event::Note(abs_note.note));
        cursor = abs_note.start.saturating_add(abs_note.note.duration.0);
    }

    if cursor < bar_end {
        let tail = bar_end.saturating_sub(cursor);
        events.push(Event::Rest(Rest {
            duration: Ticks(tail),
        }));
    }

    events
}

fn build_track(
    raw_track: &[TrackEvent<'_>],
    ppqn: Ppqn,
    tempos: &[TempoChange],
    time_sigs: &[TimeSigChange],
) -> Result<Option<MidiTrack>, MidiError> {
    let (notes, name, channel) = collect_notes(raw_track);
    if notes.is_empty() {
        return Ok(None);
    }
    let phrase = group_into_bars(notes, ppqn, tempos, time_sigs)?;
    Ok(Some(MidiTrack {
        name,
        channel,
        phrase,
    }))
}

// ── export ────────────────────────────────────────────────────────────────────

/// Serialises a [`MidiSong`] back to standard MIDI bytes.
pub fn export(song: &MidiSong) -> Result<Vec<u8>, MidiError> {
    let ppqn = song.ppqn;
    let format = if song.tracks.len() == 1 {
        Format::SingleTrack
    } else {
        Format::Parallel
    };

    let mut smf_tracks: Vec<Vec<TrackEvent<'static>>> = vec![build_meta_track(song, ppqn)?];
    for midi_track in &song.tracks {
        smf_tracks.push(build_note_track(midi_track, ppqn)?);
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

/// Build the tempo/time-signature track from the first bar of the first track.
fn build_meta_track(song: &MidiSong, ppqn: Ppqn) -> Result<Vec<TrackEvent<'static>>, MidiError> {
    let mut abs_events: Vec<(u32, TrackEventKind<'static>)> = Vec::new();

    let first_bar = song.tracks.first().and_then(|t| t.phrase.bars.first());
    let (micros, sig) = match first_bar {
        Some(bar) => (tempo_to_micros(bar.tempo)?, bar.time_signature),
        None => (
            500_000_u32,
            TimeSignature {
                numerator: 4,
                denominator: 4,
            },
        ),
    };

    abs_events.push((
        0,
        TrackEventKind::Meta(MetaMessage::Tempo(u24::from_int_lossy(micros))),
    ));

    let den_pow = sig.denominator.trailing_zeros();
    #[allow(clippy::cast_possible_truncation)]
    // trailing_zeros() on u8 is at most 7; fits in u8
    let den_pow_u8 = den_pow as u8;
    abs_events.push((
        0,
        TrackEventKind::Meta(MetaMessage::TimeSignature(sig.numerator, den_pow_u8, 24, 8)),
    ));

    // Walk bars of first track to emit tempo changes for the full timeline.
    if let Some(track) = song.tracks.first() {
        let mut bar_start: u32 = 0;
        for bar in &track.phrase.bars {
            let bt = bar_ticks(bar.time_signature, ppqn)?;
            bar_start = bar_start.saturating_add(bt);
        }
        // end-of-track at last bar boundary
        abs_events.push((bar_start, TrackEventKind::Meta(MetaMessage::EndOfTrack)));
    } else {
        abs_events.push((0, TrackEventKind::Meta(MetaMessage::EndOfTrack)));
    }

    abs_events.sort_unstable_by_key(|&(tick, _)| tick);
    Ok(abs_to_delta(abs_events))
}

fn build_note_track(track: &MidiTrack, ppqn: Ppqn) -> Result<Vec<TrackEvent<'static>>, MidiError> {
    let channel = u4::new(track.channel.min(15));
    let mut abs_events: Vec<(u32, TrackEventKind<'static>)> = Vec::new();

    let mut bar_start: u32 = 0;
    for bar in &track.phrase.bars {
        let mut cursor = bar_start;
        for event in &bar.events {
            if let Event::Note(note) = event {
                let key = u7::new(note.pitch.0);
                let vel = u7::new(note.velocity.0);
                let end = cursor.saturating_add(note.duration.0);

                abs_events.push((
                    cursor,
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
            cursor = cursor.saturating_add(event.duration().0);
        }
        let bt = bar_ticks(bar.time_signature, ppqn)?;
        bar_start = bar_start.saturating_add(bt);
    }

    abs_events.push((bar_start, TrackEventKind::Meta(MetaMessage::EndOfTrack)));
    abs_events.sort_unstable_by_key(|&(tick, _)| tick);
    Ok(abs_to_delta(abs_events))
}

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

// ── inspect / summarise ───────────────────────────────────────────────────────

/// Human-readable summary of a [`MidiSong`].
#[derive(Debug, Clone)]
pub struct MidiSummary {
    /// Pulses per quarter note.
    pub ppqn: u16,
    /// One entry per note-bearing track.
    pub tracks: Vec<TrackSummary>,
}

/// Human-readable summary of one [`MidiTrack`].
#[derive(Debug, Clone)]
pub struct TrackSummary {
    /// Track index (0-based).
    pub index: usize,
    /// Optional track name from the MIDI metadata.
    pub name: Option<String>,
    /// MIDI channel (0–15).
    pub channel: u8,
    /// Number of bars.
    pub bar_count: usize,
    /// Total note count across all bars.
    pub note_count: usize,
}

/// Build a [`MidiSummary`] for display.
pub fn summarise(song: &MidiSong) -> MidiSummary {
    let tracks = song
        .tracks
        .iter()
        .enumerate()
        .map(|(index, t)| {
            let note_count = t
                .phrase
                .bars
                .iter()
                .flat_map(|b| &b.events)
                .filter(|e| matches!(e, Event::Note(_)))
                .count();
            TrackSummary {
                index,
                name: t.name.clone(),
                channel: t.channel,
                bar_count: t.phrase.bars.len(),
                note_count,
            }
        })
        .collect();

    MidiSummary {
        ppqn: song.ppqn.0,
        tracks,
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::{bar_ticks, export, import, tempo_to_micros, MidiSong, MidiTrack, Ppqn};
    use crate::event::{
        Bar, Event, Note, Phrase, Pitch, Rest, Tempo, Ticks, TimeSignature, Velocity,
    };

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

    #[test]
    fn roundtrip_minimal_midi() {
        let note = Note {
            pitch: Pitch::new(60).expect("pitch 60 valid"),
            duration: Ticks(480),
            velocity: Velocity::new(100).expect("velocity 100 valid"),
            articulation: None,
        };
        let bar = Bar {
            time_signature: TimeSignature {
                numerator: 4,
                denominator: 4,
            },
            tempo: Tempo::new(120.0).expect("120 BPM valid"),
            events: vec![
                Event::Note(note),
                Event::Rest(Rest {
                    duration: Ticks(1440),
                }),
            ],
        };
        let song = MidiSong {
            ppqn: Ppqn(480),
            tracks: vec![MidiTrack {
                name: None,
                channel: 0,
                phrase: Phrase { bars: vec![bar] },
            }],
        };

        let bytes = export(&song).expect("export must succeed");
        let reimported = import(&bytes).expect("reimport must succeed");

        assert_eq!(reimported.ppqn, Ppqn(480), "roundtrip must preserve PPQN");
        assert_eq!(
            reimported.tracks.len(),
            1,
            "roundtrip must preserve track count"
        );

        let rt_bar = reimported
            .tracks
            .first()
            .expect("track exists")
            .phrase
            .bars
            .first()
            .expect("bar exists");

        let note_count = rt_bar
            .events
            .iter()
            .filter(|e| matches!(e, Event::Note(_)))
            .count();
        assert_eq!(note_count, 1, "roundtrip bar must contain exactly one note");
    }
}
