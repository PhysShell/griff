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
    /// Length of the tag palette the shell exposes (0 = no record attached,
    /// tag intents are no-ops). The palette itself is shell-side; the
    /// viewport sees only indices (ADR-0016).
    pub tag_count: u8,
    /// Bitmask of palette indices set on the loaded record, seeding
    /// [`Viewport::tags`].
    pub initial_tags: u32,
    /// Whether a chunk record is attached (`--record`): gates record-editing
    /// intents such as [`Intent::RenameStart`].
    pub has_record: bool,
    /// Whether a merge partner record is attached (`--merge`): gates
    /// [`Intent::MergeToggle`]. The partner itself is shell-side; the
    /// viewport sees only the flag (ADR-0016).
    pub can_merge: bool,
    /// One bar of the plotted grid in ticks (`0` = unknown, no bar gate).
    /// Persistence floors a split tick to its containing bar, so a tick
    /// inside the first bar can never persist; the split gate uses this to
    /// refuse it up front.
    pub bar_ticks: u32,
    /// The final barline tick (`0` = unknown, the gate falls back to
    /// [`ViewContext::tick_end`]). A ringing note can extend the plotted
    /// span past the last barline, and a split in that tail floors past the
    /// record's range at persist time; the split gate stops here instead.
    pub grid_end: u32,
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
// The flags are independent UI facts, not an encoded state machine, so the
// excessive-bools lint carries no signal here.
#[allow(clippy::struct_excessive_bools)]
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
    /// Inspector dock scroll offset in rows. Renderers clamp it to their own
    /// content overflow at draw time; the reducer only steps and resets it.
    pub inspector_scroll: u16,
    /// Cursor into the shell's tag palette.
    pub tag_cursor: u8,
    /// Membership bitmask over the palette: bit `i` set = tag `i` on the
    /// chunk. Seeded from [`ViewContext::initial_tags`].
    pub tags: u32,
    /// Whether the rename mode is active. The text buffer itself is
    /// frontend-local; the core keeps only the mode.
    pub renaming: bool,
    /// The pending split point as a tick, or `None` until the curator marks
    /// one. The shell maps it to a source bar at persist time (ADR-0016).
    pub split_tick: Option<u32>,
    /// Whether a merge with the attached partner record is pending.
    pub merging: bool,
    /// Whether the help overlay (the `?` cheatsheet) is shown. The overlay
    /// text is frontend-local; the core keeps only the toggle (ADR-0016).
    pub show_help: bool,
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
    /// Scroll the inspector dock down one row.
    InspectorScrollDown,
    /// Scroll the inspector dock up one row.
    InspectorScrollUp,
    /// Jump back to the start of the score.
    Home,
    /// Mark the viewed chunk approved; repeating it clears the decision.
    Approve,
    /// Mark the viewed chunk rejected; repeating it clears the decision.
    Reject,
    /// Move the tag cursor to the next palette entry (wraps).
    TagNext,
    /// Toggle the cursor's tag on the chunk; repeating it untoggles.
    TagToggle,
    /// Enter the rename mode (no-op without an attached record).
    RenameStart,
    /// Leave the rename mode (the frontend commits or cancels its buffer).
    RenameEnd,
    /// Mark the playhead tick as the pending split point; the same spot
    /// again clears it. Arming a split disarms a pending merge.
    SplitAtPlayhead,
    /// Toggle the pending merge with the attached partner record. Arming a
    /// merge disarms a pending split.
    MergeToggle,
    /// Show or hide the help overlay (the `?` keybinding cheatsheet).
    ToggleHelp,
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
            inspector_scroll: 0,
            tag_cursor: 0,
            tags: ctx.initial_tags,
            renaming: false,
            split_tick: None,
            merging: false,
            show_help: false,
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
            Intent::ToggleInspector => {
                self.show_inspector = !self.show_inspector;
                // A hidden dock forgets its scroll; reopening starts at the top.
                self.inspector_scroll = 0;
            }
            Intent::InspectorScrollDown => {
                self.inspector_scroll = self.inspector_scroll.saturating_add(1);
            }
            Intent::InspectorScrollUp => {
                self.inspector_scroll = self.inspector_scroll.saturating_sub(1);
            }
            Intent::Home => {
                self.scroll_tick = ctx.tick_start;
                self.play_tick = ctx.tick_start;
            }
            Intent::Approve => self.toggle_decision(CurationDecision::Approve),
            Intent::Reject => self.toggle_decision(CurationDecision::Reject),
            Intent::TagNext => {
                if ctx.tag_count > 0 {
                    self.tag_cursor = self.tag_cursor.wrapping_add(1) % ctx.tag_count;
                }
            }
            Intent::TagToggle => {
                if ctx.tag_count > 0 {
                    self.tags ^= 1_u32 << self.tag_cursor;
                }
            }
            Intent::RenameStart => {
                if ctx.has_record {
                    self.renaming = true;
                }
            }
            Intent::RenameEnd => self.renaming = false,
            Intent::SplitAtPlayhead => self.toggle_split(ctx),
            Intent::MergeToggle => self.toggle_merge(ctx),
            Intent::ToggleHelp => self.show_help = !self.show_help,
        }
        Step::Continue
    }

    /// Marks the playhead as the pending split point, or clears the mark when
    /// it is already there. Only a playhead between the second bar boundary
    /// and the final barline splits a record into two non-empty extents —
    /// persistence floors the tick to its containing bar, so anything inside
    /// the first bar or in a ringing tail past the grid would be rejected at
    /// quit time. Arming a split disarms a pending merge — one record cannot
    /// take both rewrites in one pass.
    fn toggle_split(&mut self, ctx: &ViewContext) {
        let first_valid = ctx.tick_start.saturating_add(ctx.bar_ticks.max(1));
        let end = if ctx.grid_end > 0 {
            ctx.grid_end.min(ctx.tick_end)
        } else {
            ctx.tick_end
        };
        if ctx.has_record && self.play_tick >= first_valid && self.play_tick < end {
            self.split_tick = if self.split_tick == Some(self.play_tick) {
                None
            } else {
                Some(self.play_tick)
            };
            self.merging = false;
        }
    }

    /// Toggles the pending merge with the attached partner record; arming it
    /// disarms a pending split (the same one-rewrite-per-pass rule).
    fn toggle_merge(&mut self, ctx: &ViewContext) {
        if ctx.can_merge {
            self.merging = !self.merging;
            if self.merging {
                self.split_tick = None;
            }
        }
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
            tag_count: 0,
            initial_tags: 0,
            has_record: false,
            can_merge: false,
            bar_ticks: 0,
            grid_end: 0,
        }
    }

    // TDD red phase: tag editing (S8 curation slice 3). The viewport keeps
    // UI-level tag state only — a cursor over an opaque palette and a
    // membership bitmask, both seeded from the context; the shell maps
    // indices to schema names at the persistence seam (ADR-0016).
    // References fields and intents that do not exist yet, so the crate
    // fails to compile until the green step.

    fn tag_ctx() -> ViewContext {
        ViewContext {
            tag_count: 4,
            initial_tags: 0b0101,
            ..ctx()
        }
    }

    #[test]
    fn new_seeds_tags_from_the_context() {
        let c = tag_ctx();
        let vp = Viewport::new(&c, 52);
        assert_eq!(vp.tags, 0b0101, "the record's tags arrive as a bitmask");
        assert_eq!(vp.tag_cursor, 0);
    }

    #[test]
    fn tag_cursor_cycles_through_the_palette() {
        let c = tag_ctx();
        let mut vp = Viewport::new(&c, 52);
        for expected in [1, 2, 3, 0] {
            vp.apply(Intent::TagNext, &c);
            assert_eq!(vp.tag_cursor, expected, "wraps at tag_count");
        }
    }

    #[test]
    fn tag_toggle_flips_the_cursor_bit() {
        let c = tag_ctx();
        let mut vp = Viewport::new(&c, 52);
        vp.apply(Intent::TagToggle, &c);
        assert_eq!(vp.tags, 0b0100, "bit 0 cleared (repeat-to-undo idiom)");
        vp.apply(Intent::TagToggle, &c);
        assert_eq!(vp.tags, 0b0101, "toggled back");
    }

    #[test]
    fn tag_intents_are_noops_without_a_palette() {
        let c = ctx(); // tag_count = 0: no record attached
        let mut vp = Viewport::new(&c, 52);
        vp.apply(Intent::TagNext, &c);
        vp.apply(Intent::TagToggle, &c);
        assert_eq!(vp.tag_cursor, 0);
        assert_eq!(vp.tags, 0);
    }

    // TDD red phase: the rename mode (S8 curation slice 4). The interaction
    // core keeps only the mode flag — the text buffer is frontend-local
    // (egui brings its own text widget; ADR-0016 keeps the core
    // renderer-agnostic and Copy). References a field and intents that do
    // not exist yet, so the crate fails to compile until the green step.

    #[test]
    fn rename_mode_toggles_via_intents_when_a_record_is_attached() {
        let c = ViewContext {
            has_record: true,
            ..ctx()
        };
        let mut vp = Viewport::new(&c, 52);
        assert!(!vp.renaming, "starts outside the rename mode");
        vp.apply(Intent::RenameStart, &c);
        assert!(vp.renaming);
        vp.apply(Intent::RenameEnd, &c);
        assert!(!vp.renaming);
    }

    #[test]
    fn rename_start_is_a_noop_without_a_record() {
        let c = ctx(); // has_record = false
        let mut vp = Viewport::new(&c, 52);
        vp.apply(Intent::RenameStart, &c);
        assert!(!vp.renaming, "nothing to rename without --record");
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

    // TDD red phase: an in-app help overlay (the `?` discoverability slice).
    // The interaction core keeps only the toggle flag — the cheatsheet text
    // is frontend-local, like the rename buffer (ADR-0016). References a
    // field and an intent that do not exist yet, so the crate fails to
    // compile until the green step.

    #[test]
    fn help_overlay_toggles_via_intent() {
        let c = ctx();
        let mut vp = Viewport::new(&c, 52);
        assert!(!vp.show_help, "the overlay starts hidden");
        vp.apply(Intent::ToggleHelp, &c);
        assert!(vp.show_help, "the intent reveals it");
        vp.apply(Intent::ToggleHelp, &c);
        assert!(!vp.show_help, "the same intent again hides it");
    }

    // TDD red phase: split/merge marks (S8 curation slice 5). The interaction
    // core keeps UI-level facts only — the split point as a tick (ticks are
    // already the core's currency) and a pending-merge flag; the shell maps
    // the tick to a source bar and owns the partner record (ADR-0016).
    // References fields, intents, and a context gate that do not exist yet,
    // so the crate fails to compile until the green step.

    fn record_ctx() -> ViewContext {
        ViewContext {
            has_record: true,
            can_merge: true,
            bar_ticks: 480,
            ..ctx()
        }
    }

    #[test]
    fn new_starts_with_no_pending_split_or_merge() {
        let c = record_ctx();
        let vp = Viewport::new(&c, 52);
        assert_eq!(vp.split_tick, None);
        assert!(!vp.merging);
    }

    #[test]
    fn split_mark_toggles_at_the_playhead() {
        let c = record_ctx();
        let mut vp = Viewport::new(&c, 52);
        vp.play_tick = 960;
        vp.apply(Intent::SplitAtPlayhead, &c);
        assert_eq!(vp.split_tick, Some(960), "the mark lands on the playhead");
        vp.apply(Intent::SplitAtPlayhead, &c);
        assert_eq!(vp.split_tick, None, "the same spot again is an undo");
    }

    #[test]
    fn split_mark_follows_a_moved_playhead() {
        let c = record_ctx();
        let mut vp = Viewport::new(&c, 52);
        vp.play_tick = 960;
        vp.apply(Intent::SplitAtPlayhead, &c);
        vp.play_tick = 480;
        vp.apply(Intent::SplitAtPlayhead, &c);
        assert_eq!(vp.split_tick, Some(480), "a new spot moves the mark");
    }

    #[test]
    fn split_mark_requires_a_record_and_an_interior_playhead() {
        let plain = ctx(); // has_record = false
        let mut vp = Viewport::new(&plain, 52);
        vp.play_tick = 960;
        vp.apply(Intent::SplitAtPlayhead, &plain);
        assert_eq!(vp.split_tick, None, "nothing to split without --record");

        let c = record_ctx();
        let mut gated = Viewport::new(&c, 52);
        gated.play_tick = c.tick_start;
        gated.apply(Intent::SplitAtPlayhead, &c);
        assert_eq!(gated.split_tick, None, "a split at the very start is empty");
        gated.play_tick = c.tick_end;
        gated.apply(Intent::SplitAtPlayhead, &c);
        assert_eq!(gated.split_tick, None, "a split at the very end is empty");
    }

    // TDD red phase: the split gate must also stop at the final barline
    // (Codex P2, PR #45, round 4) — a note ringing past it extends the
    // plotted tick_end, and a playhead in that tail floors to a bar past
    // the record's range at persist time. References ViewContext.grid_end,
    // which does not exist yet, so the crate fails to compile until green.

    #[test]
    fn split_mark_refuses_the_ringing_tail_past_the_last_barline() {
        let c = ViewContext {
            tick_end: 2200, // a note rings past the final barline at 1920
            grid_end: 1920,
            ..record_ctx()
        };
        let mut vp = Viewport::new(&c, 52);
        vp.play_tick = 2000;
        vp.apply(Intent::SplitAtPlayhead, &c);
        assert_eq!(
            vp.split_tick, None,
            "the tail past the last barline floors out of the record's range"
        );
        vp.play_tick = 1900;
        vp.apply(Intent::SplitAtPlayhead, &c);
        assert_eq!(
            vp.split_tick,
            Some(1900),
            "inside the bar grid the mark still arms"
        );
    }

    #[test]
    fn merge_toggles_when_a_partner_is_attached() {
        let c = record_ctx();
        let mut vp = Viewport::new(&c, 52);
        vp.apply(Intent::MergeToggle, &c);
        assert!(vp.merging);
        vp.apply(Intent::MergeToggle, &c);
        assert!(!vp.merging, "the same intent again is an undo");
    }

    // TDD red phase: the split gate must match the persistence rule (Codex
    // P2, PR #45) — a tick inside the first bar floors to the range start
    // and is always SplitOutOfRange at persist time, so arming it lies to
    // the curator. References ViewContext.bar_ticks, which does not exist
    // yet, so the crate fails to compile until the green step.

    #[test]
    fn split_mark_refuses_the_first_bar() {
        let c = record_ctx(); // bar_ticks = 480
        let mut vp = Viewport::new(&c, 52);
        vp.play_tick = 100;
        vp.apply(Intent::SplitAtPlayhead, &c);
        assert_eq!(
            vp.split_tick, None,
            "a tick inside the first bar floors to the range start — never persistable"
        );
        vp.play_tick = 479;
        vp.apply(Intent::SplitAtPlayhead, &c);
        assert_eq!(vp.split_tick, None, "still inside the first bar");
        vp.play_tick = 480;
        vp.apply(Intent::SplitAtPlayhead, &c);
        assert_eq!(
            vp.split_tick,
            Some(480),
            "the second bar boundary is the first valid split point"
        );
    }

    #[test]
    fn merge_is_a_noop_without_a_partner() {
        let c = ViewContext {
            has_record: true,
            ..ctx() // can_merge = false: no --merge partner attached
        };
        let mut vp = Viewport::new(&c, 52);
        vp.apply(Intent::MergeToggle, &c);
        assert!(!vp.merging);
    }

    #[test]
    fn split_and_merge_are_mutually_exclusive() {
        let c = record_ctx();
        let mut vp = Viewport::new(&c, 52);
        vp.play_tick = 960;
        vp.apply(Intent::SplitAtPlayhead, &c);
        vp.apply(Intent::MergeToggle, &c);
        assert!(vp.merging);
        assert_eq!(vp.split_tick, None, "arming a merge disarms the split");
        vp.apply(Intent::SplitAtPlayhead, &c);
        assert_eq!(vp.split_tick, Some(960));
        assert!(!vp.merging, "arming a split disarms the merge");
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
