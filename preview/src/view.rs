//! Piano-roll view-model: a pure projection of a [`Score`] onto a pitch × tick
//! plane, independent of any renderer.

use std::iter::once;

use griff_core::score::{AtomEvent, Score};

/// One note laid out on the pitch × tick plane. The tick range is half-open
/// `[onset, end)` and always non-empty (`end > onset`), so a zero-duration note
/// still occupies one tick of width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteRect {
    /// Absolute onset tick.
    pub onset: u32,
    /// Absolute end tick (exclusive); always `> onset`.
    pub end: u32,
    /// MIDI pitch (0–127).
    pub pitch: u8,
}

/// One horizontal lane — the merged notes of a single track (all its voices).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lane {
    /// Display name (track name, or a synthesised `track N`).
    pub name: String,
    /// Notes in this lane, sorted by onset then pitch.
    pub notes: Vec<NoteRect>,
}

/// A renderer-agnostic piano-roll: the bounds of the plane, the bar gridlines,
/// and the per-track lanes.
#[derive(Debug, Clone, PartialEq)]
pub struct PianoRollView {
    /// Tick resolution (pulses per quarter note) of the source score.
    pub ppq: u16,
    /// Inclusive low tick of the plotted span.
    pub tick_start: u32,
    /// Exclusive high tick of the plotted span; always `> tick_start`.
    pub tick_end: u32,
    /// Lowest MIDI pitch present (inclusive).
    pub low_pitch: u8,
    /// Highest MIDI pitch present (inclusive); always `>= low_pitch`.
    pub high_pitch: u8,
    /// Absolute ticks of every bar start, plus the final bar end.
    pub bar_lines: Vec<u32>,
    /// Per-track lanes.
    pub lanes: Vec<Lane>,
    /// Tempo (BPM) at the start of the score.
    pub tempo_bpm: f64,
    /// Number of master bars in the source score.
    pub bar_count: usize,
}

/// Default pitch band used when a score carries no notes (C3–C5), so the plane
/// is never degenerate.
const DEFAULT_LOW_PITCH: u8 = 48;
const DEFAULT_HIGH_PITCH: u8 = 72;

/// Builds a [`PianoRollView`] from a score. Pure and total: an empty score
/// yields a minimal, non-degenerate plane rather than an error.
pub fn build_view(score: &Score) -> PianoRollView {
    let mut lanes: Vec<Lane> = Vec::with_capacity(score.tracks.len());
    let mut low: Option<u8> = None;
    let mut high: Option<u8> = None;
    let mut max_note_end: u32 = 0;

    for (i, track) in score.tracks.iter().enumerate() {
        let mut notes: Vec<NoteRect> = Vec::new();
        for group in track.voices.iter().flat_map(|v| &v.event_groups) {
            for atom in &group.atoms {
                if let AtomEvent::Note(n) = atom {
                    let onset = n.absolute_start.0;
                    let end = onset.saturating_add(n.duration.0.max(1));
                    let pitch = n.pitch.0;
                    low = Some(low.map_or(pitch, |l| l.min(pitch)));
                    high = Some(high.map_or(pitch, |h| h.max(pitch)));
                    max_note_end = max_note_end.max(end);
                    notes.push(NoteRect { onset, end, pitch });
                }
            }
        }
        notes.sort_by_key(|r| (r.onset, r.pitch));
        let name = track.name.clone().unwrap_or_else(|| format!("track {i}"));
        lanes.push(Lane { name, notes });
    }

    let tick_start = score
        .master_bars
        .first()
        .map_or(0, |b| b.tick_range.start.0);
    let bars_end = score
        .master_bars
        .last()
        .map_or(tick_start, |b| b.tick_range.end.0);
    // Span covers the bars and any note that rings past the final barline.
    let tick_end = bars_end.max(max_note_end).max(tick_start.saturating_add(1));

    let bar_lines: Vec<u32> = score
        .master_bars
        .iter()
        .map(|b| b.tick_range.start.0)
        .chain(once(bars_end))
        .collect();

    let tempo_bpm = score.master_bars.first().map_or(120.0, |b| b.tempo.0);

    PianoRollView {
        ppq: score.ticks_per_quarter,
        tick_start,
        tick_end,
        low_pitch: low.unwrap_or(DEFAULT_LOW_PITCH),
        high_pitch: high.unwrap_or(DEFAULT_HIGH_PITCH),
        bar_lines,
        lanes,
        tempo_bpm,
        bar_count: score.master_bars.len(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        clippy::missing_assert_message,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::str_to_string
    )]

    use super::*;
    use griff_core::event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity};
    use griff_core::score::{
        AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, RepeatMarker,
        Score, Track, Voice,
    };
    use griff_core::slice::TickRange;

    const PPQN: u16 = 480;
    const BAR: u32 = 1920;

    fn note(onset: u32, dur: u32, pitch: u8) -> AtomEvent {
        AtomEvent::Note(AtomNote {
            absolute_start: Ticks(onset),
            duration: Ticks(dur),
            pitch: Pitch::new(pitch).expect("valid pitch"),
            velocity: Velocity::new(96).expect("valid velocity"),
            marks: NoteMarks::empty(),
            position: None,
        })
    }

    fn score_with(bars: usize, tracks: Vec<Vec<AtomEvent>>) -> Score {
        let master_bars = (0..bars)
            .map(|i| {
                let start = u32::try_from(i).expect("small") * BAR;
                MasterBar {
                    index: i,
                    tick_range: TickRange::new(Ticks(start), Ticks(start + BAR)).expect("ordered"),
                    time_signature: TimeSignature::new(4, 4).expect("4/4"),
                    tempo: Tempo::new(120.0).expect("120 BPM"),
                    repeat: RepeatMarker::default(),
                }
            })
            .collect();
        let tracks = tracks
            .into_iter()
            .enumerate()
            .map(|(i, atoms)| Track {
                name: Some(format!("t{i}")),
                channel: 0,
                voices: vec![Voice {
                    id: 0,
                    event_groups: atoms
                        .into_iter()
                        .map(|a| EventGroup {
                            kind: EventGroupKind::Single,
                            atoms: vec![a],
                            technique_spans: Vec::new(),
                        })
                        .collect(),
                }],
                tuning: Tuning::standard_e(),
            })
            .collect();
        Score {
            ticks_per_quarter: PPQN,
            master_bars,
            tracks,
            source_meta: None,
            loss: LossReport::new(),
        }
    }

    #[test]
    fn empty_score_yields_non_degenerate_plane() {
        let view = build_view(&score_with(0, vec![]));
        assert!(view.tick_end > view.tick_start, "span must be non-empty");
        assert_eq!(view.low_pitch, DEFAULT_LOW_PITCH);
        assert_eq!(view.high_pitch, DEFAULT_HIGH_PITCH);
        assert!(view.lanes.is_empty());
    }

    #[test]
    fn single_note_sets_pitch_bounds_and_span() {
        let view = build_view(&score_with(1, vec![vec![note(0, 480, 64)]]));
        assert_eq!((view.low_pitch, view.high_pitch), (64, 64));
        assert_eq!(view.lanes.len(), 1);
        assert_eq!(
            view.lanes.first().map(|l| l.notes.len()),
            Some(1),
            "the single note lands in the one lane",
        );
        assert_eq!(view.tick_end, BAR, "one 4/4 bar spans 1920 ticks");
    }

    #[test]
    fn span_extends_past_barline_for_ringing_note() {
        // A note that rings 480 ticks past the single bar's end.
        let view = build_view(&score_with(1, vec![vec![note(BAR - 240, 720, 60)]]));
        assert_eq!(view.tick_end, BAR + 480, "span must cover the ringing tail");
    }

    #[test]
    fn bar_lines_have_one_more_entry_than_bars() {
        let view = build_view(&score_with(3, vec![vec![note(0, 480, 60)]]));
        assert_eq!(view.bar_lines.len(), 4, "3 bar starts + final end");
        assert_eq!(view.bar_count, 3);
    }

    #[test]
    fn pitch_bounds_span_all_tracks() {
        let view = build_view(&score_with(
            1,
            vec![vec![note(0, 240, 40)], vec![note(240, 240, 80)]],
        ));
        assert_eq!((view.low_pitch, view.high_pitch), (40, 80));
        assert_eq!(view.lanes.len(), 2);
    }
}
