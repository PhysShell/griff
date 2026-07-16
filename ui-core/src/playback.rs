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

/// The tempo across the master timeline: `(start tick, BPM)` segments.
///
/// Ascending, always with one at tick 0. Playback walks these so a tempo
/// change is honoured — the master timeline is the single source of tempo —
/// and advances a **fractional** tick position, so no frame's rounding drifts.
#[derive(Debug, Clone)]
pub struct TempoMap {
    /// `(start_tick, bpm)`, ascending by tick, `[0]` at tick 0.
    segments: Vec<(u32, f64)>,
}

impl TempoMap {
    /// A constant-tempo map — the whole score at `bpm`.
    #[must_use]
    pub fn single(bpm: f64) -> Self {
        Self {
            segments: vec![(0, bpm.max(1.0))],
        }
    }

    /// Builds a map from `(start_tick, bpm)` pairs. They are sorted; a segment
    /// at tick 0 is synthesised if missing; runs of equal BPM collapse.
    #[must_use]
    pub fn new(mut segments: Vec<(u32, f64)>) -> Self {
        segments.retain(|&(_, bpm)| bpm.is_finite() && bpm > 0.0);
        segments.sort_by_key(|&(t, _)| t);
        if segments.first().is_none_or(|&(t, _)| t != 0) {
            let bpm = segments.first().map_or(120.0, |&(_, b)| b);
            segments.insert(0, (0, bpm));
        }
        segments.dedup_by(|a, b| (a.1 - b.1).abs() < f64::EPSILON);
        Self { segments }
    }

    /// The BPM in force at `tick` — the last segment starting at or before it.
    #[must_use]
    pub fn bpm_at(&self, tick: u32) -> f64 {
        let idx = self
            .segments
            .partition_point(|&(t, _)| t <= tick)
            .saturating_sub(1);
        self.segments.get(idx).map_or(120.0, |&(_, b)| b).max(1.0)
    }

    /// The first segment boundary strictly after `tick`, if any.
    fn next_boundary(&self, tick: u32) -> Option<u32> {
        self.segments.iter().map(|&(t, _)| t).find(|&t| t > tick)
    }

    /// Advances a fractional tick position `from` by `dt` seconds, splitting
    /// the interval at every tempo boundary it crosses, at resolution `ppq`
    /// and audition `scale`. Fractional in and out: the caller keeps the
    /// remainder, so sub-tick frames never round to zero or drift.
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // pos ≥ 0, ticks never near u32::MAX
    pub fn advance(&self, from: f64, dt: f64, ppq: u16, scale: f64) -> f64 {
        let mut pos = from.max(0.0);
        let mut remaining = dt.max(0.0);
        // At most one hop per segment boundary, plus the final partial step.
        for _ in 0..=self.segments.len() {
            let tick = pos as u32;
            let tps = ticks_per_second(ppq, self.bpm_at(tick), scale);
            if tps <= 0.0 {
                break;
            }
            // If a boundary lies ahead and the frame has time to reach it,
            // hop to it and re-rate; otherwise take the final partial step.
            if let Some(boundary) = self.next_boundary(tick) {
                let secs_to_boundary = (f64::from(boundary) - pos) / tps;
                if remaining > secs_to_boundary {
                    pos = f64::from(boundary);
                    remaining -= secs_to_boundary;
                    continue;
                }
            }
            pos += tps * remaining;
            break;
        }
        pos
    }

    /// The seconds to travel from tick `from` to tick `to` (`to >= from`),
    /// integrating across every tempo segment between them — the inverse of
    /// [`Self::advance`]. `0.0` when `to <= from`. Playback's loop uses it to
    /// find the exact time a frame spends reaching the loop end, so a lap can
    /// be split at the boundary instead of overshooting into the tempo past it.
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // pos ≥ 0, ticks never near u32::MAX
    pub fn time_to(&self, from: f64, to: f64, ppq: u16, scale: f64) -> f64 {
        if to <= from {
            return 0.0;
        }
        let mut pos = from.max(0.0);
        let mut secs = 0.0;
        // At most one hop per segment boundary between `from` and `to`.
        for _ in 0..=self.segments.len() {
            let tps = ticks_per_second(ppq, self.bpm_at(pos as u32), scale);
            if tps <= 0.0 {
                break;
            }
            // Walk to the next boundary, but never past the target.
            let seg_end = self
                .next_boundary(pos as u32)
                .map_or(to, |b| f64::from(b).min(to));
            secs += (seg_end - pos) / tps;
            pos = seg_end;
            if pos >= to {
                break;
            }
        }
        secs
    }
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
    /// How many notes are holding each pitch (indexed by pitch 0..=127). A
    /// physical note-off fires only when a pitch's count returns to zero, so
    /// two overlapping notes of the same pitch do not cut each other short.
    holds: Vec<u16>,
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
            holds: vec![0; 128],
        }
    }

    /// Fires the pending note-on at `on_cursor`: raises the pitch's hold count
    /// and sounds it (a re-attack retriggers, which is correct for MIDI).
    fn fire_on<S: PlaybackSink>(&mut self, sink: &mut S) {
        if let Some(&(_, pitch)) = self.ons.get(self.on_cursor) {
            self.on_cursor = self.on_cursor.saturating_add(1);
            if let Some(count) = self.holds.get_mut(usize::from(pitch)) {
                *count = count.saturating_add(1);
            }
            sink.note_on(pitch, DEFAULT_VELOCITY);
        }
    }

    /// Fires the pending note-off at `off_cursor`: lowers the pitch's hold
    /// count and physically releases it only when the last holder ends. An
    /// off for a pitch not held (its onset was seeked past) is a no-op.
    fn fire_off<S: PlaybackSink>(&mut self, sink: &mut S) {
        if let Some(&(_, pitch)) = self.offs.get(self.off_cursor) {
            self.off_cursor = self.off_cursor.saturating_add(1);
            if let Some(count) = self.holds.get_mut(usize::from(pitch)) {
                if *count > 0 {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        sink.note_off(pitch);
                    }
                }
            }
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
        // Merge the two cursors in tick order, so a frame that steps clean over
        // a short note fires its on THEN its off — never off-then-on, which
        // would drop the off and hang the note. At a shared tick the off wins,
        // so a re-attacked pitch releases before it restarts.
        loop {
            let next_on = self
                .ons
                .get(self.on_cursor)
                .copied()
                .filter(|&(t, _)| t < now);
            let next_off = self
                .offs
                .get(self.off_cursor)
                .copied()
                .filter(|&(t, _)| t < now);
            match (next_on, next_off) {
                (None, None) => break,
                (Some(_), None) => self.fire_on(sink),
                (None, Some(_)) => self.fire_off(sink),
                (Some((on_t, _)), Some((off_t, _))) => {
                    if off_t <= on_t {
                        self.fire_off(sink);
                    } else {
                        self.fire_on(sink);
                    }
                }
            }
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
        for pitch in 0..self.holds.len() {
            if self.holds.get(pitch).is_some_and(|&c| c > 0) {
                sink.note_off(u8::try_from(pitch).unwrap_or(0));
            }
        }
        self.holds.iter_mut().for_each(|c| *c = 0);
        sink.all_notes_off();
    }

    /// How many distinct pitches are sounding right now.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.holds.iter().filter(|&&c| c > 0).count()
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
    fn one_advance_across_a_whole_short_note_fires_on_then_off() {
        // #125 review 1: a frame that steps clean over a note entirely
        // within [prev, now) must fire ON then OFF, in that order — never
        // OFF-then-ON, which the old two-pass scheduler dropped, hanging it.
        let mut player = Player::from_view(&view(vec![note(100, 120, 64)]));
        let mut sink = MockSink::default();
        player.advance_to(200, &mut sink); // 0 → 200 crosses both onset and end
        assert_eq!(
            sink.events,
            vec![("on", 64), ("off", 64)],
            "the whole note sounds and releases in one frame",
        );
        assert_eq!(player.active_count(), 0, "nothing left hanging");
    }

    #[test]
    fn overlapping_same_pitch_notes_release_once_at_the_last_end() {
        // #125 review 4: two notes of the same pitch overlap; the first note's
        // end must NOT physically release the pitch while the second still
        // holds it. Reference count: one physical off, at the last end.
        let mut player = Player::from_view(&view(vec![note(0, 480, 60), note(240, 720, 60)]));
        let mut sink = MockSink::default();

        player.advance_to(500, &mut sink); // both onsets + the first end
        assert_eq!(
            sink.events,
            vec![("on", 60), ("on", 60)],
            "both attacks sound; the first end does not release the held pitch",
        );
        assert_eq!(player.active_count(), 1, "the pitch is still held once");

        player.advance_to(800, &mut sink); // the second end
        assert_eq!(
            sink.events.last(),
            Some(&("off", 60)),
            "released at the last end"
        );
        assert_eq!(player.active_count(), 0);
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

    #[test]
    fn a_tempo_change_bends_the_playhead_at_its_boundary() {
        use super::TempoMap;
        // 120 BPM until tick 480, then 240 BPM. At 480 ppq: 960 then 1920 t/s.
        let map = TempoMap::new(vec![(0, 120.0), (480, 240.0)]);
        assert!((map.bpm_at(0) - 120.0).abs() < 1e-9);
        assert!((map.bpm_at(479) - 120.0).abs() < 1e-9);
        assert!((map.bpm_at(480) - 240.0).abs() < 1e-9);

        // 0.5 s at 960 t/s reaches exactly the boundary (480).
        let at_boundary = map.advance(0.0, 0.5, 480, 1.0);
        assert!(
            (at_boundary - 480.0).abs() < 1e-6,
            "half a second lands on the tempo change, got {at_boundary}"
        );
        // 0.75 s: 0.5 s to the boundary, then 0.25 s at the doubled rate =
        // 480 more ticks → 960. A single-tempo walk would give only 720.
        let past = map.advance(0.0, 0.75, 480, 1.0);
        assert!(
            (past - 960.0).abs() < 1e-6,
            "the frame is split at the boundary, got {past}"
        );
    }

    #[test]
    fn sub_tick_frames_accumulate_without_drift() {
        use super::TempoMap;
        // A slow rate so each tiny frame is well under one tick: 1 BPM at
        // 480 ppq is 8 tick/s, so a 1 ms frame is 0.008 tick.
        let map = TempoMap::single(1.0);
        let mut pos = 0.0_f64;
        for _ in 0..1000 {
            pos = map.advance(pos, 0.001, 480, 1.0);
        }
        // 1000 frames × 1 ms = 1 s of travel = 8 ticks, fractional intact.
        assert!(
            (pos - 8.0).abs() < 1e-6,
            "a thousand sub-tick frames sum to exactly 8 ticks, got {pos}"
        );
    }

    #[test]
    fn a_map_always_has_a_segment_at_the_start() {
        use super::TempoMap;
        // Given only a late segment, tick 0 still resolves (to that BPM).
        let map = TempoMap::new(vec![(960, 90.0)]);
        assert!((map.bpm_at(0) - 90.0).abs() < 1e-9, "start is synthesised");
        assert!((map.bpm_at(960) - 90.0).abs() < 1e-9);
    }

    #[test]
    fn time_to_is_the_inverse_of_advance() {
        use super::TempoMap;
        // Constant tempo: 480 ppq at 120 BPM is 960 tick/s, so 960 ticks = 1 s.
        let flat = TempoMap::single(120.0);
        assert!((flat.time_to(0.0, 960.0, 480, 1.0) - 1.0).abs() < 1e-9);
        assert!(
            flat.time_to(200.0, 150.0, 480, 1.0).abs() < 1e-12,
            "backwards is zero seconds",
        );

        // Across a tempo change at 480: 0→480 at 120 BPM (0.5 s) then 480→960 at
        // 240 BPM (0.25 s) = 0.75 s — exactly the dt that advance() maps to 960.
        let bent = TempoMap::new(vec![(0, 120.0), (480, 240.0)]);
        let secs = bent.time_to(0.0, 960.0, 480, 1.0);
        assert!(
            (secs - 0.75).abs() < 1e-9,
            "integrates each segment, got {secs}"
        );
        let round = bent.advance(0.0, secs, 480, 1.0);
        assert!((round - 960.0).abs() < 1e-6, "advance(time_to) is identity");
    }

    #[test]
    fn time_to_the_loop_end_ignores_the_tempo_past_it() {
        use super::TempoMap;
        // A fast segment starts exactly at the loop end (200). Time to reach 200
        // must use only the in-loop tempo (120), never the 1000-BPM tail.
        let map = TempoMap::new(vec![(0, 120.0), (200, 1000.0)]);
        let inside = TempoMap::single(120.0).time_to(0.0, 200.0, 480, 1.0);
        assert!(
            (map.time_to(0.0, 200.0, 480, 1.0) - inside).abs() < 1e-9,
            "reaching the boundary is unaffected by what lies past it",
        );
    }
}
