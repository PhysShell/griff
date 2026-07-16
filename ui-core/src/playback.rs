//! Backend-neutral playback (S8 Slice 2).
//!
//! The one place that turns a laid-out score into note events, so every
//! backend — native MIDI, the browser's Web Audio, a test's mock — hears the
//! same music.
//!
//! Events carry the score's **real onsets and durations**: a note sounds from
//! its onset until its end, never a fixed-length blip. That makes playback an
//! honest instrument for the very question the generator raised — whether it
//! varies note length — instead of hiding the answer behind uniform pips.
//!
//! The layer is pure and testable: [`Player`] holds a time-ordered schedule
//! and a cursor, and drives a [`PlaybackSink`] forward as the transport
//! advances the playhead. Stop, seek, a candidate switch, or a lost device
//! all route through [`Player::silence`] / [`Player::seek`], which emit an
//! explicit note-off for every sounding pitch **and** an all-notes-off — so
//! Griff never leaves a note ringing forever (MIDI's favourite way to remind
//! a developer of mortality).

use crate::view::{Lane, NoteRect, PianoRollView};

/// The default velocity every note sounds at.
///
/// The piano-roll view drops the score's per-note velocity (`NoteRect` is
/// onset/end/pitch), so Slice 2 plays a constant, musical mezzo-forte;
/// dynamics are a later slice.
pub const DEFAULT_VELOCITY: u8 = 96;

/// A synthesiser or MIDI port the player drives. Every call is best-effort:
/// audio must never panic the render loop, so a backend swallows its own
/// failures as silence.
pub trait PlaybackSink {
    /// Begin sounding `pitch` at `velocity` (1–127).
    fn note_on(&mut self, pitch: u8, velocity: u8);
    /// Stop sounding `pitch`.
    fn note_off(&mut self, pitch: u8);
    /// Stop **everything** now — the panic button behind stop, seek, a
    /// candidate switch, and a lost device.
    fn all_notes_off(&mut self);
}

/// The ticks-per-second the playhead advances at.
///
/// The score's `ppq`, its tempo, and the audition `scale` (playback BPM =
/// written BPM × scale). A non-positive input floors to a sane minimum so a
/// frame always moves.
#[must_use]
pub fn ticks_per_second(ppq: u16, bpm: f64, scale: f64) -> f64 {
    let bpm = bpm.max(1.0);
    let scale = scale.max(0.01);
    f64::from(ppq) * bpm * scale / 60.0
}

/// Drives a [`PlaybackSink`] across a score's note events in tick order.
///
/// The schedule is two sorted lists — note-ons keyed by onset, note-offs
/// keyed by end — walked by a monotonic cursor. [`Self::advance_to`] moves
/// the cursor forward and fires every event the playhead has reached; a
/// non-forward move goes through [`Self::seek`], which silences first.
#[derive(Debug, Clone)]
pub struct Player {
    /// `(onset, pitch)` for every note, ascending by onset.
    ons: Vec<(u32, u8)>,
    /// `(end, pitch)` for every note, ascending by end.
    offs: Vec<(u32, u8)>,
    /// Next unfired note-on.
    on_cursor: usize,
    /// Next unfired note-off.
    off_cursor: usize,
    /// Pitches currently sounding — so a silence can note-off each explicitly,
    /// not only trust an all-notes-off CC.
    active: Vec<u8>,
}

impl Player {
    /// Builds a player over every lane of a view — the whole displayed score.
    #[must_use]
    pub fn from_view(view: &PianoRollView) -> Self {
        Self::from_notes(
            view.lanes
                .iter()
                .flat_map(|lane| lane.notes.iter().copied()),
        )
    }

    /// Builds a player over a single lane (one track).
    #[must_use]
    pub fn from_lane(lane: &Lane) -> Self {
        Self::from_notes(lane.notes.iter().copied())
    }

    fn from_notes(notes: impl Iterator<Item = NoteRect>) -> Self {
        let mut ons = Vec::new();
        let mut offs = Vec::new();
        for n in notes {
            ons.push((n.onset, n.pitch));
            offs.push((n.end, n.pitch));
        }
        ons.sort_unstable();
        offs.sort_unstable();
        Self {
            ons,
            offs,
            on_cursor: 0,
            off_cursor: 0,
            active: Vec::new(),
        }
    }

    /// True when the schedule holds no notes.
    #[must_use]
    pub const fn is_silent(&self) -> bool {
        self.ons.is_empty()
    }

    /// Advances the playhead to `now` (must be ≥ the last `now`), firing every
    /// note-off then note-on the playhead has reached (`tick < now`). Offs
    /// before ons so a re-attacked pitch is released before it restarts.
    pub fn advance_to<S: PlaybackSink>(&mut self, now: u32, sink: &mut S) {
        // Offs first: a pitch ending exactly where another (or itself) begins
        // is released before the attack.
        while let Some(&(tick, pitch)) = self.offs.get(self.off_cursor) {
            if tick >= now {
                break;
            }
            self.off_cursor = self.off_cursor.saturating_add(1);
            if let Some(pos) = self.active.iter().position(|&p| p == pitch) {
                self.active.swap_remove(pos);
                sink.note_off(pitch);
            }
        }
        while let Some(&(tick, pitch)) = self.ons.get(self.on_cursor) {
            if tick >= now {
                break;
            }
            self.on_cursor = self.on_cursor.saturating_add(1);
            self.active.push(pitch);
            sink.note_on(pitch, DEFAULT_VELOCITY);
        }
    }

    /// Repositions the cursor to `tick` after a discontinuous move (seek, loop
    /// wrap, candidate switch): silences everything, then sets the cursor to
    /// the first event at or after `tick`. Notes already sounding at `tick`
    /// are **not** retriggered — a seek lands mid-rest cleanly.
    pub fn seek<S: PlaybackSink>(&mut self, tick: u32, sink: &mut S) {
        self.silence(sink);
        // The cursor lands on the first event at or after `tick`; a note
        // whose onset is behind the head is treated as already past, so it
        // does not retrigger.
        self.on_cursor = self.ons.partition_point(|&(t, _)| t < tick);
        self.off_cursor = self.offs.partition_point(|&(t, _)| t < tick);
    }

    /// Stops every sounding note now — an explicit note-off per active pitch
    /// plus an all-notes-off. Leaves the cursor where it is.
    pub fn silence<S: PlaybackSink>(&mut self, sink: &mut S) {
        for pitch in self.active.drain(..) {
            sink.note_off(pitch);
        }
        sink.all_notes_off();
    }

    /// How many pitches are sounding right now.
    #[must_use]
    pub const fn active_count(&self) -> usize {
        self.active.len()
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::missing_assert_message
)]
mod tests {
    use super::{ticks_per_second, PlaybackSink, Player};
    use crate::view::{Lane, NoteRect, PianoRollView};

    /// Records every call so a test can assert the exact event stream.
    #[derive(Default)]
    struct MockSink {
        events: Vec<(&'static str, u8)>,
    }
    impl PlaybackSink for MockSink {
        fn note_on(&mut self, pitch: u8, _velocity: u8) {
            self.events.push(("on", pitch));
        }
        fn note_off(&mut self, pitch: u8) {
            self.events.push(("off", pitch));
        }
        fn all_notes_off(&mut self) {
            self.events.push(("alloff", 0));
        }
    }

    fn note(onset: u32, end: u32, pitch: u8) -> NoteRect {
        NoteRect { onset, end, pitch }
    }

    fn view(notes: Vec<NoteRect>) -> PianoRollView {
        PianoRollView {
            ppq: 480,
            tick_start: 0,
            tick_end: 2000,
            low_pitch: 40,
            high_pitch: 80,
            bar_lines: vec![0, 1920],
            lanes: vec![Lane {
                name: "t".to_owned(),
                notes,
            }],
            tempo_bpm: 120.0,
            bar_count: 1,
        }
    }

    #[test]
    fn a_note_sounds_for_its_real_duration_not_a_fixed_blip() {
        // Two quarter notes back to back: 60 over [0,480), 62 over [480,960).
        let mut player = Player::from_view(&view(vec![note(0, 480, 60), note(480, 960, 62)]));
        let mut sink = MockSink::default();

        player.advance_to(480, &mut sink); // crossed onset 0, not yet its end
        assert_eq!(
            sink.events,
            vec![("on", 60)],
            "note 60 starts, does not blip off"
        );
        assert_eq!(player.active_count(), 1);

        player.advance_to(960, &mut sink); // end of 60 and onset of 62
        assert_eq!(
            sink.events,
            vec![("on", 60), ("off", 60), ("on", 62)],
            "60 releases exactly at its end, then 62 attacks — real durations",
        );
        assert_eq!(player.active_count(), 1, "only 62 is sounding now");

        player.advance_to(960, &mut sink); // reaching the end releases 62
                                           // 62 ends at 960; at tick 960 (exclusive) it has not released yet.
        assert_eq!(player.active_count(), 1);
        player.advance_to(1000, &mut sink);
        assert_eq!(
            sink.events.last(),
            Some(&("off", 62)),
            "62 releases at its end"
        );
    }

    #[test]
    fn an_onset_on_a_frame_boundary_fires_once() {
        let mut player = Player::from_view(&view(vec![note(480, 960, 64)]));
        let mut sink = MockSink::default();
        player.advance_to(480, &mut sink); // onset == now: half-open, not yet
        assert!(
            sink.events.is_empty(),
            "an onset at `now` waits for the next frame"
        );
        player.advance_to(481, &mut sink);
        assert_eq!(
            sink.events,
            vec![("on", 64)],
            "fired once, on the next frame"
        );
    }

    #[test]
    fn silence_releases_every_sounding_note_and_sends_all_notes_off() {
        let mut player = Player::from_view(&view(vec![note(0, 960, 60), note(0, 960, 67)]));
        let mut sink = MockSink::default();
        player.advance_to(10, &mut sink);
        assert_eq!(player.active_count(), 2, "a two-note chord sounds");

        sink.events.clear();
        player.silence(&mut sink);
        assert!(sink.events.contains(&("off", 60)) && sink.events.contains(&("off", 67)));
        assert_eq!(
            sink.events.last(),
            Some(&("alloff", 0)),
            "and the panic button"
        );
        assert_eq!(player.active_count(), 0);
    }

    #[test]
    fn seek_silences_and_does_not_retrigger_a_note_already_under_the_head() {
        let mut player = Player::from_view(&view(vec![note(0, 1920, 60), note(960, 1200, 72)]));
        let mut sink = MockSink::default();
        player.advance_to(500, &mut sink); // 60 sounding
        assert_eq!(player.active_count(), 1);

        sink.events.clear();
        player.seek(1000, &mut sink); // 60 still spans 1000, 72 started at 960
        assert!(sink.events.contains(&("alloff", 0)), "seek silences");
        // Advancing from the seek must not re-fire 60's onset (it was at 0).
        player.advance_to(1100, &mut sink);
        let retriggered_60 = sink.events.iter().any(|&(k, p)| k == "on" && p == 60);
        assert!(
            !retriggered_60,
            "a note whose onset is behind the seek does not retrigger"
        );
    }

    #[test]
    fn every_lane_contributes() {
        let mut v = view(vec![note(0, 480, 60)]);
        v.lanes.push(Lane {
            name: "t2".to_owned(),
            notes: vec![note(0, 480, 48)],
        });
        let mut player = Player::from_view(&v);
        let mut sink = MockSink::default();
        player.advance_to(10, &mut sink);
        assert_eq!(player.active_count(), 2, "both lanes sound");
    }

    #[test]
    fn tempo_scale_multiplies_the_written_rate() {
        let base = ticks_per_second(480, 120.0, 1.0);
        assert!(
            (base - 960.0).abs() < 1e-9,
            "480 ppq at 120 BPM is 960 tick/s"
        );
        assert!(
            (ticks_per_second(480, 120.0, 2.0) - 1920.0).abs() < 1e-9,
            "2x doubles"
        );
        assert!(
            (ticks_per_second(480, 120.0, 0.5) - 480.0).abs() < 1e-9,
            "half halves"
        );
        assert!(
            ticks_per_second(480, 0.0, 0.0) > 0.0,
            "a frame always moves"
        );
    }
}
