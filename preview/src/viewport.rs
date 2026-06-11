//! Interaction core: the renderer-agnostic UI state and reducer shared by every
//! `griff` frontend (ratatui today, egui later — see ADR-0016).
//!
//! [`Viewport`] holds the interactive state (scroll, zoom, pitch offset, section
//! selection, playback, inspector toggle). [`Intent`] is a semantic,
//! device-independent action; a frontend translates raw input (keys, mouse) into
//! intents and [`Viewport::apply`] is the *only* place that interprets them.
//! Playback advance and autoscroll are pure functions of elapsed time and plot
//! width. The reducer reads only a narrow [`ViewContext`] — never the view-model
//! directly — so this layer has no dependency on any renderer or projection.

// The reducer and playback math are bounded saturating integer arithmetic over
// tick spans and column counts, plus one ticks-per-second cast for playback.
// Values are clamped and every denominator is guarded non-zero, so the
// overflow/precision lints carry no signal here.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

/// The read-only facts the reducer needs about the score projection: the plotted
/// tick span, timing, and the onset tick of each named section (in order).
#[derive(Debug, Clone, PartialEq)]
pub struct ViewContext {
    /// Inclusive low tick of the plotted span.
    pub tick_start: u32,
    /// Exclusive high tick of the plotted span; always `> tick_start`.
    pub tick_end: u32,
    /// Pulses per quarter note of the source score.
    pub ppq: u16,
    /// Tempo (BPM) at the start of the score.
    pub tempo_bpm: f64,
    /// Onset tick of each named section, in playback order.
    pub section_starts: Vec<u32>,
}

/// A curation decision pending on the viewed chunk (S8).
///
/// A UI-level fact — deliberately not `corpus::ReviewerDecision`, so the
/// interaction core keeps zero `griff-core` domain types (the ADR-0016 crate
/// boundary); the frontend shell maps it when persisting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurationDecision {
    /// The curator approves the viewed chunk.
    Approve,
    /// The curator rejects the viewed chunk.
    Reject,
}

/// Interactive, renderer-agnostic viewport state. Mutated only by the reducer
/// ([`Viewport::apply`]) and the pure playback helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    /// Leftmost visible tick.
    pub scroll_tick: u32,
    /// Zoom: ticks mapped onto one column. Always `>= 1`.
    pub ticks_per_col: u32,
    /// Pitch shown on the top row of the plane.
    pub top_pitch: u8,
    /// Index of the selected section.
    pub sel_section: usize,
    /// Whether playback is running.
    pub playing: bool,
    /// Current playhead tick.
    pub play_tick: u32,
    /// Whether the inspector dock is shown.
    pub show_inspector: bool,
    /// The pending curation decision, or `None` until the curator acts.
    pub decision: Option<CurationDecision>,
}

/// A semantic, device-independent UI action. Frontends map raw input to these;
/// the reducer is the single interpreter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// Request to quit the app.
    Quit,
    /// Start/stop playback (restart from the top when stopped at the end).
    TogglePlay,
    /// Scroll the plane left (towards the start).
    ScrollLeft,
    /// Scroll the plane right (towards the end).
    ScrollRight,
    /// Move the visible pitch band up.
    PitchUp,
    /// Move the visible pitch band down.
    PitchDown,
    /// Zoom in (fewer ticks per column).
    ZoomIn,
    /// Zoom out (more ticks per column).
    ZoomOut,
    /// Select the previous section and jump to it.
    PrevSection,
    /// Select the next section and jump to it.
    NextSection,
    /// Show/hide the inspector dock.
    ToggleInspector,
    /// Jump back to the start of the score.
    Home,
    /// Mark the viewed chunk approved; repeating it clears the decision.
    Approve,
    /// Mark the viewed chunk rejected; repeating it clears the decision.
    Reject,
}

/// The outcome of reducing an [`Intent`]: whether the app should keep running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Keep running.
    Continue,
    /// The user asked to quit.
    Quit,
}

/// Default zoom (ticks per column) before a [`Viewport::fit`].
const DEFAULT_TICKS_PER_COL: u32 = 60;

impl Viewport {
    /// Builds the initial viewport for a score whose highest pitch is
    /// `high_pitch`, anchored at the start of `ctx`'s span.
    #[must_use]
    pub fn new(ctx: &ViewContext, high_pitch: u8) -> Self {
        let top_pitch = high_pitch.saturating_add(1).min(127);
        Self {
            scroll_tick: ctx.tick_start,
            ticks_per_col: DEFAULT_TICKS_PER_COL,
            top_pitch,
            sel_section: 0,
            playing: false,
            play_tick: ctx.tick_start,
            show_inspector: true,
            decision: None,
        }
    }

    /// Chooses a zoom that fits the whole span into `plot_cols` columns and
    /// resets the scroll to the start of the span.
    pub fn fit(&mut self, plot_cols: u32, ctx: &ViewContext) {
        let cols = plot_cols.max(1);
        let span = ctx.tick_end.saturating_sub(ctx.tick_start).max(1);
        self.ticks_per_col = span.div_ceil(cols).max(1);
        self.scroll_tick = ctx.tick_start;
    }

    /// Applies one [`Intent`], mutating the viewport, and reports whether the app
    /// should keep running.
    pub fn apply(&mut self, intent: Intent, ctx: &ViewContext) -> Step {
        let scroll_step = self.ticks_per_col.saturating_mul(8).max(1);
        match intent {
            Intent::Quit => return Step::Quit,
            Intent::TogglePlay => {
                if !self.playing && self.play_tick >= ctx.tick_end {
                    self.play_tick = ctx.tick_start;
                }
                self.playing = !self.playing;
            }
            Intent::ScrollLeft => {
                self.scroll_tick = self
                    .scroll_tick
                    .saturating_sub(scroll_step)
                    .max(ctx.tick_start);
            }
            Intent::ScrollRight => {
                self.scroll_tick = self.scroll_tick.saturating_add(scroll_step);
            }
            Intent::PitchUp => self.top_pitch = self.top_pitch.saturating_add(2).min(127),
            Intent::PitchDown => self.top_pitch = self.top_pitch.saturating_sub(2),
            Intent::ZoomIn => {
                self.ticks_per_col = (self.ticks_per_col.saturating_mul(2) / 3).max(1);
            }
            Intent::ZoomOut => {
                self.ticks_per_col = (self.ticks_per_col.saturating_mul(3) / 2).max(1);
            }
            Intent::PrevSection => self.select_section(self.sel_section.saturating_sub(1), ctx),
            Intent::NextSection => self.select_section(self.sel_section.saturating_add(1), ctx),
            Intent::ToggleInspector => self.show_inspector = !self.show_inspector,
            Intent::Home => {
                self.scroll_tick = ctx.tick_start;
                self.play_tick = ctx.tick_start;
            }
            Intent::Approve => self.toggle_decision(CurationDecision::Approve),
            Intent::Reject => self.toggle_decision(CurationDecision::Reject),
        }
        Step::Continue
    }

    /// Sets `decision`, clearing it when the same decision is already pending
    /// (the same intent again is an undo); a different one overwrites.
    fn toggle_decision(&mut self, decision: CurationDecision) {
        self.decision = if self.decision == Some(decision) {
            None
        } else {
            Some(decision)
        };
    }

    /// Selects section `idx` (clamped to the last) and jumps the scroll and
    /// playhead to its onset.
    fn select_section(&mut self, idx: usize, ctx: &ViewContext) {
        let last = ctx.section_starts.len().saturating_sub(1);
        self.sel_section = idx.min(last);
        if let Some(&start) = ctx.section_starts.get(self.sel_section) {
            self.scroll_tick = start;
            self.play_tick = start;
        }
    }

    /// Advances playback by `dt_secs` seconds of wall-clock time. A no-op when
    /// paused; stops at the end of the span.
    pub fn advance_playback(&mut self, dt_secs: f64, ctx: &ViewContext) {
        if !self.playing {
            return;
        }
        let tps = f64::from(ctx.ppq) * ctx.tempo_bpm / 60.0;
        let adv = (tps * dt_secs) as u32;
        self.play_tick = self.play_tick.saturating_add(adv.max(1));
        if self.play_tick >= ctx.tick_end {
            self.play_tick = ctx.tick_end;
            self.playing = false;
        }
    }

    /// Recenters the scroll on the playhead when it leaves the visible window of
    /// `plot_cols` columns. A no-op when paused.
    pub fn autoscroll(&mut self, plot_cols: u32) {
        if !self.playing || plot_cols == 0 {
            return;
        }
        let right = self
            .scroll_tick
            .saturating_add(plot_cols.saturating_mul(self.ticks_per_col));
        if self.play_tick < self.scroll_tick || self.play_tick >= right {
            let back = (plot_cols / 4).saturating_mul(self.ticks_per_col);
            self.scroll_tick = self.play_tick.saturating_sub(back);
        }
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
        clippy::float_cmp
    )]

    use super::*;

    fn ctx() -> ViewContext {
        ViewContext {
            tick_start: 0,
            tick_end: 1920,
            ppq: 480,
            tempo_bpm: 120.0,
            section_starts: vec![0, 960],
        }
    }

    // TDD red phase: the scrollable inspector (the S8 follow-up recorded
    // with the PR #38 liveness decision). References a field and intents
    // that do not exist yet, so the crate fails to compile until green.

    #[test]
    fn inspector_scroll_intents_step_and_saturate() {
        let c = ctx();
        let mut vp = Viewport::new(&c, 52);
        assert_eq!(vp.inspector_scroll, 0, "starts unscrolled");
        vp.apply(Intent::InspectorScrollDown, &c);
        vp.apply(Intent::InspectorScrollDown, &c);
        assert_eq!(vp.inspector_scroll, 2);
        vp.apply(Intent::InspectorScrollUp, &c);
        assert_eq!(vp.inspector_scroll, 1);
        vp.apply(Intent::InspectorScrollUp, &c);
        vp.apply(Intent::InspectorScrollUp, &c);
        assert_eq!(vp.inspector_scroll, 0, "saturates at the top");
    }

    #[test]
    fn hiding_the_inspector_resets_its_scroll() {
        let c = ctx();
        let mut vp = Viewport::new(&c, 52);
        vp.apply(Intent::InspectorScrollDown, &c);
        vp.apply(Intent::ToggleInspector, &c);
        assert_eq!(vp.inspector_scroll, 0, "a hidden dock forgets its scroll");
        assert!(!vp.show_inspector);
    }

    #[test]
    fn new_anchors_at_span_start() {
        let c = ctx();
        let vp = Viewport::new(&c, 52);
        assert_eq!(vp.scroll_tick, 0);
        assert_eq!(vp.play_tick, 0);
        assert_eq!(vp.top_pitch, 53, "top is one above the highest pitch");
        assert_eq!(vp.sel_section, 0);
        assert!(!vp.playing);
        assert!(vp.show_inspector);
        assert!(vp.ticks_per_col >= 1);
    }

    #[test]
    fn new_clamps_top_pitch_to_127() {
        let vp = Viewport::new(&ctx(), 127);
        assert_eq!(vp.top_pitch, 127);
    }

    #[test]
    fn fit_maps_span_over_columns_and_resets_scroll() {
        let c = ctx();
        let mut vp = Viewport::new(&c, 52);
        vp.scroll_tick = 500;
        vp.fit(96, &c);
        assert_eq!(vp.ticks_per_col, 1920 / 96);
        assert_eq!(vp.scroll_tick, 0, "fit resets scroll to the span start");
    }

    #[test]
    fn fit_never_yields_zero_zoom() {
        let c = ctx();
        let mut vp = Viewport::new(&c, 52);
        vp.fit(100_000, &c);
        assert!(vp.ticks_per_col >= 1);
    }

    #[test]
    fn fit_uses_ceiling_zoom_to_cover_the_whole_span() {
        let c = ctx();
        let mut vp = Viewport::new(&c, 52);
        vp.fit(35, &c);
        assert_eq!(vp.ticks_per_col, 55, "1920 ticks over 35 columns rounds up");
        assert!(
            35 * vp.ticks_per_col >= c.tick_end - c.tick_start,
            "fitted viewport covers the full score tail"
        );
    }

    #[test]
    fn toggle_play_starts_and_stops() {
        let c = ctx();
        let mut vp = Viewport::new(&c, 52);
        assert_eq!(vp.apply(Intent::TogglePlay, &c), Step::Continue);
        assert!(vp.playing);
        vp.apply(Intent::TogglePlay, &c);
        assert!(!vp.playing);
    }

    #[test]
    fn toggle_play_restarts_when_stopped_at_end() {
        let c = ctx();
        let mut vp = Viewport::new(&c, 52);
        vp.play_tick = c.tick_end;
        vp.apply(Intent::TogglePlay, &c);
        assert!(vp.playing);
        assert_eq!(vp.play_tick, c.tick_start, "replays from the top");
    }

    #[test]
    fn scroll_clamps_left_at_span_start() {
        let c = ctx();
        let mut vp = Viewport::new(&c, 52);
        vp.apply(Intent::ScrollRight, &c);
        assert!(vp.scroll_tick > 0);
        for _ in 0..10 {
            vp.apply(Intent::ScrollLeft, &c);
        }
        assert_eq!(vp.scroll_tick, c.tick_start, "never scrolls past the start");
    }

    #[test]
    fn zoom_in_lowers_and_zoom_out_raises_resolution() {
        let c = ctx();
        let mut vp = Viewport::new(&c, 52);
        vp.ticks_per_col = 60;
        vp.apply(Intent::ZoomIn, &c);
        assert!(vp.ticks_per_col < 60, "zoom in lowers ticks/col");
        let z = vp.ticks_per_col;
        vp.apply(Intent::ZoomOut, &c);
        assert!(vp.ticks_per_col > z, "zoom out raises ticks/col");
    }

    #[test]
    fn next_and_prev_section_jump_and_clamp() {
        let c = ctx();
        let mut vp = Viewport::new(&c, 52);
        vp.apply(Intent::NextSection, &c);
        assert_eq!(vp.sel_section, 1);
        assert_eq!(vp.scroll_tick, 960, "jumps to the section onset");
        assert_eq!(vp.play_tick, 960);
        // Already at the last section: stays clamped.
        vp.apply(Intent::NextSection, &c);
        assert_eq!(vp.sel_section, 1);
        vp.apply(Intent::PrevSection, &c);
        assert_eq!(vp.sel_section, 0);
        assert_eq!(vp.scroll_tick, 0);
    }

    #[test]
    fn pitch_moves_and_clamps() {
        let c = ctx();
        let mut vp = Viewport::new(&c, 52);
        vp.top_pitch = 126;
        vp.apply(Intent::PitchUp, &c);
        assert_eq!(vp.top_pitch, 127, "clamps at 127");
        vp.top_pitch = 1;
        vp.apply(Intent::PitchDown, &c);
        assert_eq!(vp.top_pitch, 0, "saturates at 0 without underflow");
    }

    #[test]
    fn toggle_inspector_flips() {
        let c = ctx();
        let mut vp = Viewport::new(&c, 52);
        let before = vp.show_inspector;
        vp.apply(Intent::ToggleInspector, &c);
        assert_eq!(vp.show_inspector, !before);
    }

    #[test]
    fn home_returns_to_start() {
        let c = ctx();
        let mut vp = Viewport::new(&c, 52);
        vp.scroll_tick = 800;
        vp.play_tick = 800;
        vp.apply(Intent::Home, &c);
        assert_eq!(vp.scroll_tick, c.tick_start);
        assert_eq!(vp.play_tick, c.tick_start);
    }

    // TDD red phase: curation decisions join the interaction core (S8,
    // ADR-0016 layer 2) — a UI-level fact, persisted by the frontend shell.
    // References a field and intents that do not exist yet, so the crate
    // fails to compile until the green step.

    #[test]
    fn approve_and_reject_set_the_decision() {
        let c = ctx();
        let mut vp = Viewport::new(&c, 52);
        assert_eq!(vp.decision, None, "no decision until the curator acts");
        vp.apply(Intent::Approve, &c);
        assert_eq!(vp.decision, Some(CurationDecision::Approve));
        vp.apply(Intent::Reject, &c);
        assert_eq!(
            vp.decision,
            Some(CurationDecision::Reject),
            "the other intent overwrites"
        );
    }

    #[test]
    fn repeating_a_decision_clears_it() {
        let c = ctx();
        let mut vp = Viewport::new(&c, 52);
        vp.apply(Intent::Approve, &c);
        vp.apply(Intent::Approve, &c);
        assert_eq!(vp.decision, None, "the same intent again is an undo");
        vp.apply(Intent::Reject, &c);
        vp.apply(Intent::Reject, &c);
        assert_eq!(vp.decision, None);
    }

    #[test]
    fn quit_intent_reports_quit() {
        let c = ctx();
        let mut vp = Viewport::new(&c, 52);
        assert_eq!(vp.apply(Intent::Quit, &c), Step::Quit);
    }

    #[test]
    fn advance_playback_moves_and_stops_at_end() {
        let c = ctx();
        let mut vp = Viewport::new(&c, 52);
        vp.advance_playback(1.0, &c);
        assert_eq!(vp.play_tick, 0, "paused playback does not move");
        vp.playing = true;
        vp.advance_playback(0.1, &c);
        assert!(vp.play_tick > 0, "running playback advances");
        vp.advance_playback(100.0, &c);
        assert_eq!(vp.play_tick, c.tick_end, "clamps at the end");
        assert!(!vp.playing, "stops at the end");
    }

    #[test]
    fn autoscroll_recenters_when_playhead_leaves_window() {
        let c = ctx();
        let mut vp = Viewport::new(&c, 52);
        vp.ticks_per_col = 10;
        vp.playing = true;
        vp.play_tick = 1500;
        vp.scroll_tick = 0;
        vp.autoscroll(40); // window covers 0..400, playhead at 1500 is past it
        assert!(vp.scroll_tick > 0, "scroll follows the playhead");
        assert!(vp.scroll_tick <= vp.play_tick);
    }

    #[test]
    fn autoscroll_is_noop_when_paused() {
        let c = ctx();
        let mut vp = Viewport::new(&c, 52);
        vp.playing = false;
        vp.scroll_tick = 0;
        vp.play_tick = 1500;
        vp.autoscroll(40);
        assert_eq!(vp.scroll_tick, 0);
    }
}
