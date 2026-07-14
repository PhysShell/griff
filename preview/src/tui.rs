//! Interactive terminal piano-roll built on `ratatui`.
//!
//! [`App`] holds the view-model ([`PianoRollView`]) plus engine-derived
//! [`Analysis`] (named sections + structure metrics) and the viewport state
//! (scroll, zoom, selection, playhead). It renders into any `ratatui` backend,
//! so the same `render` path drives both the live crossterm loop ([`run`]) and a
//! headless [`App::snapshot`] via `TestBackend` — the latter makes the UI
//! verifiable without a terminal.

// The renderer is bounded grid arithmetic over terminal cells and tick spans,
// with integer/float casts for the linear tick↔column and metric↔meter maps.
// Values are bounded by the viewport and denominators are guarded non-zero, so
// the overflow/precision/length lints carry no signal here.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use std::io;
use std::time::{Duration, Instant};

use ratatui::backend::TestBackend;
use ratatui::buffer::{Buffer, Cell};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame, Terminal};

use crate::analysis::Analysis;
use crate::curation::{tag_palette, RecordSummary};
use crate::scene::{resolve, CellRole, GridSize, Scene, SceneCell, GUTTER};
use crate::theme::{cell_style, Rgb, Theme};
use crate::view::PianoRollView;
use crate::viewport::{CurationDecision, Intent, Step, ViewContext, Viewport};

/// Width of the inspector dock.
const INSPECTOR_W: u16 = 32;
/// Width of a metric meter bar in the inspector.
const METER_W: usize = 22;

// The palette is the core's (ADR-0028). This renderer had its own — and it did
// not even agree with the cockpit's: lane 3 was amber here and blue there, and
// Unknown a different grey in each. Two renderers, one `Scene`, two palettes.
// All that is left here is the conversion into ratatui's colour type.

/// The core's colour, in ratatui's terms.
const fn color(c: Rgb) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}

/// Interactive piano-roll application.
///
/// Holds the renderer-agnostic view-model and analysis, the shared interaction
/// [`Viewport`] and its [`ViewContext`], and the source file label. All
/// interaction logic lives in the viewport core; this type only maps keys to
/// [`Intent`]s and the state to `ratatui` cells.
#[derive(Debug, Clone)]
pub struct App {
    view: PianoRollView,
    analysis: Analysis,
    file: String,
    record: Option<RecordSummary>,
    /// The tag palette in wire casing; the viewport cycles its indices.
    palette: Vec<String>,
    /// The rename buffer while [`Viewport::renaming`]; frontend-local.
    rename_buf: String,
    /// A committed rename awaiting quit-time persistence.
    pending_title: Option<String>,
    /// The merge partner's title (`--merge`), shown while the merge is
    /// armed; the partner record itself stays shell-side.
    merge_partner: Option<String>,
    vp: Viewport,
    ctx: ViewContext,
    /// The palette every cell resolves through (ADR-0028) — the same one the
    /// egui cockpit paints from.
    theme: Theme,
}

impl App {
    /// Builds the app from a view, its analysis, and the source file label.
    #[must_use]
    pub fn new(view: PianoRollView, analysis: Analysis, file: String) -> Self {
        let ctx = ViewContext {
            tick_start: view.tick_start,
            tick_end: view.tick_end,
            ppq: view.ppq,
            tempo_bpm: view.tempo_bpm,
            section_starts: analysis.sections.iter().map(|s| s.tick_start).collect(),
            tag_count: 0,
            initial_tags: 0,
            has_record: false,
            can_merge: false,
            // The split gate mirrors persistence's floor-to-bar rule, so the
            // core needs the plotted grid's bar length (0 = no grid known).
            bar_ticks: match view.bar_lines.as_slice() {
                [first, second, ..] => second.saturating_sub(*first),
                _ => 0,
            },
            // A ringing note extends tick_end past the last barline; the
            // split gate must stop at the barline (the record has no bars
            // past it).
            grid_end: view.bar_lines.last().copied().unwrap_or(0),
        };
        let vp = Viewport::new(&ctx, view.high_pitch);
        Self {
            view,
            analysis,
            file,
            record: None,
            palette: Vec::new(),
            rename_buf: String::new(),
            pending_title: None,
            merge_partner: None,
            vp,
            ctx,
            theme: Theme::dark(),
        }
    }

    /// Attaches the digest of the `--record` chunk so the inspector shows
    /// the record's current curation state (title, reviewer, tags), and
    /// seeds the tag-editing state: the palette becomes cyclable and the
    /// record's tags arrive in the viewport as a bitmask.
    pub fn set_record(&mut self, record: RecordSummary) {
        self.palette = tag_palette();
        let mask = self
            .palette
            .iter()
            .enumerate()
            .filter(|(_, name)| record.tags.contains(name))
            .fold(0_u32, |m, (i, _)| m | (1_u32 << i));
        self.ctx.tag_count = u8::try_from(self.palette.len()).unwrap_or(u8::MAX);
        self.ctx.initial_tags = mask;
        self.ctx.has_record = true;
        self.vp.tags = mask;
        self.record = Some(record);
    }

    /// Attaches the `--merge` partner's title and unlocks the merge intent
    /// (`ViewContext::can_merge`); the partner record itself stays with the
    /// shell, which performs the join at persist time.
    pub fn set_merge_partner(&mut self, title: String) {
        self.ctx.can_merge = true;
        self.merge_partner = Some(title);
    }

    /// What the curator left behind for the shell to persist.
    #[must_use]
    pub fn outcome(&self) -> CurationOutcome {
        CurationOutcome {
            decision: self.vp.decision,
            tags: self.tags_if_changed(),
            title: self.title_if_changed(),
            split_tick: self.vp.split_tick,
            merge: self.vp.merging,
        }
    }

    /// The live tag set in palette order when it differs from the record's,
    /// or `None` when nothing changed (nothing to persist).
    #[must_use]
    pub fn tags_if_changed(&self) -> Option<Vec<String>> {
        if self.record.is_none() || self.vp.tags == self.ctx.initial_tags {
            return None;
        }
        Some(self.live_tags())
    }

    /// The committed rename when it differs from the record's title, or
    /// `None` when nothing changed (nothing to persist).
    #[must_use]
    pub fn title_if_changed(&self) -> Option<String> {
        let rec = self.record.as_ref()?;
        let pending = self.pending_title.as_ref()?;
        (*pending != rec.title).then(|| pending.clone())
    }

    /// The title the dock shows: a committed rename wins over the record's.
    fn live_title(&self) -> Option<&str> {
        self.pending_title
            .as_deref()
            .or_else(|| self.record.as_ref().map(|r| r.title.as_str()))
    }

    /// The viewport's tag bitmask mapped back to palette names, in order.
    fn live_tags(&self) -> Vec<String> {
        self.palette
            .iter()
            .enumerate()
            .filter(|(i, _)| self.vp.tags & (1_u32 << i) != 0)
            .map(|(_, name)| name.clone())
            .collect()
    }

    /// Renders one frame into a `TestBackend` of the given size and returns the
    /// buffer as plain text rows (colours dropped) — used for headless previews
    /// and tests.
    ///
    /// # Errors
    /// Propagates any backend error from `ratatui`.
    pub fn snapshot(&mut self, width: u16, height: u16) -> io::Result<Vec<String>> {
        self.fit(width);
        let mut terminal = Terminal::new(TestBackend::new(width, height))?;
        terminal.draw(|f| self.render(f))?;
        Ok(buffer_lines(terminal.backend().buffer()))
    }

    /// Chooses a zoom (ticks per column) that fits the whole span in the plot.
    fn fit(&mut self, total_width: u16) {
        let reserved = GUTTER.saturating_add(if self.vp.show_inspector {
            INSPECTOR_W
        } else {
            0
        });
        let plot_cols = u32::from(total_width.saturating_sub(reserved).max(1));
        self.vp.fit(plot_cols, &self.ctx);
    }

    // ── input ───────────────────────────────────────────────────────────
    /// Handles a key press; returns `false` when the app should quit.
    ///
    /// Translates the key into a semantic [`Intent`] and lets the shared
    /// viewport reducer interpret it — no interaction logic lives here.
    fn on_key(&mut self, code: KeyCode) -> bool {
        if self.vp.renaming {
            return self.on_rename_key(code);
        }
        if self.vp.show_help {
            // The overlay is modal: any key dismisses it (the rename-mode
            // precedent), so a stray keystroke never leaks to the roll.
            self.vp.apply(Intent::ToggleHelp, &self.ctx);
            return true;
        }
        let Some(intent) = Self::key_intent(code) else {
            return true;
        };
        let step = self.vp.apply(intent, &self.ctx);
        if intent == Intent::RenameStart && self.vp.renaming {
            // Entering the mode: seed the buffer with the live title.
            self.rename_buf = self.live_title().unwrap_or_default().to_owned();
        }
        step == Step::Continue
    }

    /// Handles a key inside the rename mode: typed characters edit the
    /// frontend-local buffer ('q' is text here, not quit), Enter commits
    /// the trimmed, non-empty buffer, Esc cancels.
    fn on_rename_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Enter => {
                let title = self.rename_buf.trim();
                if !title.is_empty() {
                    self.pending_title = Some(title.to_owned());
                }
                self.vp.apply(Intent::RenameEnd, &self.ctx);
            }
            KeyCode::Esc => {
                self.vp.apply(Intent::RenameEnd, &self.ctx);
            }
            KeyCode::Backspace => {
                self.rename_buf.pop();
            }
            KeyCode::Char(c) => self.rename_buf.push(c),
            _ => {}
        }
        true
    }

    /// Maps a raw key to a semantic [`Intent`], or `None` if unbound.
    const fn key_intent(code: KeyCode) -> Option<Intent> {
        Some(match code {
            KeyCode::Char('q') | KeyCode::Esc => Intent::Quit,
            KeyCode::Char(' ') => Intent::TogglePlay,
            KeyCode::Left | KeyCode::Char('h') => Intent::ScrollLeft,
            KeyCode::Right | KeyCode::Char('l') => Intent::ScrollRight,
            KeyCode::Up | KeyCode::Char('k') => Intent::PitchUp,
            KeyCode::Down | KeyCode::Char('j') => Intent::PitchDown,
            KeyCode::Char('+' | '=') => Intent::ZoomIn,
            KeyCode::Char('-' | '_') => Intent::ZoomOut,
            KeyCode::Char('[') => Intent::PrevSection,
            KeyCode::Char(']') | KeyCode::Tab => Intent::NextSection,
            KeyCode::Char('i') => Intent::ToggleInspector,
            KeyCode::PageDown => Intent::InspectorScrollDown,
            KeyCode::PageUp => Intent::InspectorScrollUp,
            KeyCode::Char('0') | KeyCode::Home => Intent::Home,
            KeyCode::Char('a') => Intent::Approve,
            KeyCode::Char('x') => Intent::Reject,
            KeyCode::Char('t') => Intent::TagNext,
            KeyCode::Char('T') => Intent::TagToggle,
            KeyCode::Char('r') => Intent::RenameStart,
            KeyCode::Char('s') => Intent::SplitAtPlayhead,
            KeyCode::Char('m') => Intent::MergeToggle,
            KeyCode::Char('?') => Intent::ToggleHelp,
            _ => return None,
        })
    }

    /// Advances playback by `dt`.
    fn tick(&mut self, dt: Duration) {
        self.vp.advance_playback(dt.as_secs_f64(), &self.ctx);
    }

    fn autoscroll(&mut self, roll_width: u16) {
        let plot_cols = u32::from(roll_width.saturating_sub(GUTTER));
        self.vp.autoscroll(plot_cols);
    }

    // ── rendering ─────────────────────────────────────────────────────────
    fn render(&mut self, frame: &mut Frame<'_>) {
        let [header, sections, body, footer] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .areas(frame.area());

        let (roll, inspector) = if self.vp.show_inspector && body.width > INSPECTOR_W + 12 {
            let [r, i] = Layout::horizontal([Constraint::Min(0), Constraint::Length(INSPECTOR_W)])
                .areas(body);
            (r, Some(i))
        } else {
            (body, None)
        };

        self.autoscroll(roll.width);
        self.render_header(header, frame);
        if let Some(area) = inspector {
            self.render_inspector(area, frame);
        }
        render_footer(footer, frame);

        // All piano-roll layout lives in the shared scene resolver; this renderer
        // only maps placed cells to ratatui styling.
        let scene = resolve(
            &self.view,
            &self.analysis,
            &self.vp,
            GridSize {
                cols: roll.width,
                rows: roll.height,
            },
        );
        let buf = frame.buffer_mut();
        paint_band(&scene, roll.x, sections.y, buf, &self.theme);
        paint_plane(&scene, roll, buf, &self.theme);

        // The help overlay paints last so it sits above the roll, inspector,
        // and footer (it is modal; any key dismisses it).
        if self.vp.show_help {
            render_help(frame);
        }
    }

    fn render_header(&self, area: Rect, frame: &mut Frame<'_>) {
        let line = Line::from(vec![
            Span::styled(
                "griff·preview",
                Style::new()
                    .fg(Color::Rgb(59, 157, 255))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(
                "  {}  ·  ♩={:.0}  ·  {} bars  ·  pos {}",
                self.file,
                self.view.tempo_bpm,
                self.view.bar_count,
                self.position_label()
            )),
        ]);
        frame.render_widget(Paragraph::new(line), area);
    }

    fn render_inspector(&mut self, area: Rect, frame: &mut Frame<'_>) {
        let block = Block::bordered()
            .title(" Inspector ")
            .border_style(Style::new().fg(Color::Rgb(70, 70, 78)));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        // The reducer steps the offset blindly; clamp to the real overflow so
        // the dock never scrolls past its last line. Ratatui scrolls *after*
        // wrapping, so the overflow must count post-wrap rows — line_count,
        // not the pre-wrap Line entries (Codex P2, PR #41).
        let paragraph = Paragraph::new(self.inspector_lines()).wrap(Wrap { trim: true });
        let overflow = u16::try_from(paragraph.line_count(inner.width))
            .unwrap_or(u16::MAX)
            .saturating_sub(inner.height);
        let scroll = self.vp.inspector_scroll.min(overflow);
        // Write the clamp back (the autoscroll precedent: render owns the
        // bounds), so overscrolling leaves no hidden excess for PgUp to burn
        // through (Codex P2, PR #41).
        self.vp.inspector_scroll = scroll;
        frame.render_widget(paragraph.scroll((scroll, 0)), inner);
    }

    /// Appends the loaded record's curation state (title, prior reviewer
    /// decision, tags) to the inspector lines, when a `--record` is attached.
    fn push_record_lines(&self, lines: &mut Vec<Line<'static>>, dim: Style) {
        let Some(rec) = &self.record else { return };
        lines.push(Line::raw(""));
        if self.vp.renaming {
            lines.push(Line::from(vec![
                Span::styled("name\u{25b8} ", dim),
                Span::raw(format!("{}\u{258f}", self.rename_buf)),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled("record ", dim),
                Span::raw(self.live_title().unwrap_or(&rec.title).to_owned()),
            ]));
        }
        lines.push(Line::from(format!(
            "review {}",
            rec.reviewer.as_deref().unwrap_or("—")
        )));
        let live = self.live_tags();
        if !live.is_empty() {
            lines.push(Line::from(format!("tags  {}", live.join(" "))));
        }
        if let Some(name) = self.palette.get(usize::from(self.vp.tag_cursor)) {
            let set = self.vp.tags & (1_u32 << self.vp.tag_cursor) != 0;
            lines.push(Line::from(format!(
                "tag▸ {name} [{}]",
                if set { "x" } else { " " }
            )));
        }
        if let Some(tick) = self.vp.split_tick {
            lines.push(Line::from(format!("split▸ at bar {}", self.bar_at(tick))));
        }
        if self.vp.merging {
            if let Some(partner) = &self.merge_partner {
                lines.push(Line::from(format!("merge▸ + {partner}")));
            }
        }
    }

    /// Builds the inspector's content lines: track, section, curation,
    /// transport (live state first — the PR #38 liveness ordering), then the
    /// static structure and complexity blocks.
    fn inspector_lines(&self) -> Vec<Line<'static>> {
        let dim = Style::new().fg(Color::Rgb(120, 120, 128));
        let accent = Style::new().fg(Color::Rgb(59, 157, 255));
        let mut lines: Vec<Line<'static>> = Vec::new();

        let track = self
            .view
            .lanes
            .get(self.analysis.focus_track)
            .map_or_else(|| "—".to_owned(), |l| l.name.clone());
        lines.push(Line::from(vec![
            Span::styled("track  ", dim),
            Span::raw(track),
        ]));

        if let Some(s) = self.analysis.sections.get(self.vp.sel_section) {
            lines.push(Line::from(Span::styled(
                format!(" {} ", s.class),
                Style::new()
                    // The chip is the selected section, so it wears the selected
                    // fill — and the ink the theme guarantees reads on it.
                    .bg(color(self.theme.class_fill(s.class, true)))
                    .fg(color(self.theme.class_ink(s.class, true)))
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(format!(
                "bars {}–{} · {} bar(s)",
                s.bar_start + 1,
                s.bar_end,
                s.bar_count()
            )));
        }

        lines.push(Line::from(format!(
            "curation  {}",
            match self.vp.decision {
                Some(CurationDecision::Approve) => "approved",
                Some(CurationDecision::Reject) => "rejected",
                None => "—",
            }
        )));

        // Transport sits above the metrics blocks: when the content exceeds
        // the dock, clipping eats the tail of the static metrics, never the
        // live play state (Codex P2, PR #38).
        lines.push(Line::raw(""));
        lines.push(Line::styled("transport", dim));
        lines.push(Line::from(format!(
            "♩={:.0}   {}",
            self.view.tempo_bpm,
            if self.vp.playing {
                "▶ playing"
            } else {
                "⏸ paused"
            }
        )));
        lines.push(Line::from(format!("pos {}", self.position_label())));

        // The record digest is static (loaded once at startup), so it sits
        // below the live transport state, with the other static blocks
        // (Codex P2, PR #42 — the same liveness ordering as PR #38).
        self.push_record_lines(&mut lines, dim);

        self.push_metric_blocks(&mut lines, dim, accent);

        lines
    }

    /// Pushes the static `structure (S14)` and `complexity (S14)` blocks.
    ///
    /// On a single-bar score the bar-ratio metrics are vacuous, not
    /// measured: repeatability is core's no-second-bar abstention (0.0),
    /// so variation (`1 − 0`) and the distinct-signature ratios
    /// (structural complexity, the `str` axis — `1/1`) follow by
    /// construction. Abstain with a dash; loopability is a real seam
    /// measurement on any span and stays numeric.
    fn push_metric_blocks(&self, lines: &mut Vec<Line<'static>>, dim: Style, accent: Style) {
        let bars_vacuous = self.view.bar_count < 2;

        lines.push(Line::raw(""));
        lines.push(Line::styled("structure (S14)", dim));
        if let Some(m) = &self.analysis.metrics {
            push_metric(lines, "loopability", Some(m.loopability_score), accent);
            push_metric(
                lines,
                "repeatability",
                (!bars_vacuous).then_some(m.repeatability_score),
                accent,
            );
            push_metric(
                lines,
                "variation",
                (!bars_vacuous).then_some(m.variation_score),
                accent,
            );
            push_metric(
                lines,
                "complexity",
                (!bars_vacuous).then_some(m.structural_complexity),
                accent,
            );
            let period = m
                .detected_pattern_period_bars
                .map_or_else(|| "—".to_owned(), |p| format!("{p} bars"));
            lines.push(Line::from(format!("pattern   {period}")));
        } else {
            lines.push(Line::raw("—"));
        }

        lines.push(Line::raw(""));
        lines.push(Line::styled("complexity (S14)", dim));
        if let Some(c) = &self.analysis.complexity {
            lines.push(complexity_pair(
                "rhy",
                Some(c.rhythmic),
                "pit",
                Some(c.pitch),
            ));
            lines.push(complexity_pair(
                "tec",
                Some(c.technical),
                "har",
                Some(c.harmonic),
            ));
            lines.push(complexity_pair(
                "ply",
                Some(c.playability),
                "str",
                (!bars_vacuous).then_some(c.structural),
            ));
        } else {
            lines.push(Line::raw("—"));
        }
    }

    // ── helpers ─────────────────────────────────────────────────────────
    /// The 1-based number of the bar containing `tick`.
    fn bar_at(&self, tick: u32) -> usize {
        let mut bar_idx = 0usize;
        for (i, &t) in self.view.bar_lines.iter().enumerate() {
            if t <= tick {
                bar_idx = i;
            } else {
                break;
            }
        }
        bar_idx.saturating_add(1)
    }

    fn position_label(&self) -> String {
        let ppq = u32::from(self.view.ppq).max(1);
        let mut bar_start = self.view.tick_start;
        for &t in &self.view.bar_lines {
            if t <= self.vp.play_tick {
                bar_start = t;
            } else {
                break;
            }
        }
        let beat = self.vp.play_tick.saturating_sub(bar_start) / ppq;
        format!(
            "{}:{}",
            self.bar_at(self.vp.play_tick),
            beat.saturating_add(1)
        )
    }
}

/// Renders the static footer hint line. `? help` leads so the cheatsheet
/// stays discoverable even where the line truncates on a narrow terminal.
fn render_footer(area: Rect, frame: &mut Frame<'_>) {
    let hint =
        "? help · q quit · space play · ←/→ scroll · ↑/↓ pitch · +/- zoom · [/]/tab section \
· a/x curate · t/T tag · r rename · s split · m merge";
    frame.render_widget(
        Paragraph::new(Line::styled(
            hint,
            Style::new().fg(Color::Rgb(120, 120, 128)),
        )),
        area,
    );
}

/// Draws the `?` help overlay: a centered, bordered cheatsheet of every
/// keybinding, painted over the roll. Modal — any key dismisses it.
fn render_help(frame: &mut Frame<'_>) {
    let lines = help_lines();
    let area = frame.area();
    let width = 56.min(area.width);
    let height = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .saturating_add(2)
        .min(area.height);
    let x = area.x.saturating_add(area.width.saturating_sub(width) / 2);
    let y = area
        .y
        .saturating_add(area.height.saturating_sub(height) / 2);
    let rect = Rect {
        x,
        y,
        width,
        height,
    };
    let block = Block::bordered()
        .title(" Help · press any key to close ")
        .border_style(Style::new().fg(Color::Rgb(59, 157, 255)));
    // Clear wipes the roll underneath so the cheatsheet reads cleanly.
    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(lines).block(block), rect);
}

/// The keybinding cheatsheet rendered inside the help overlay.
fn help_lines() -> Vec<Line<'static>> {
    const KEYS: [(&str, &str); 12] = [
        ("q / Esc", "quit (saves pending curation)"),
        ("Space", "play / pause"),
        ("← → / h l", "scroll    ↑ ↓ / k j   pitch"),
        ("+ / -", "zoom      [ ] Tab     section"),
        ("0 / Home", "jump to start"),
        ("i", "inspector   PgUp/PgDn  scroll it"),
        ("", ""),
        ("a / x", "approve / reject  (needs --record)"),
        ("t / T", "tag cursor / toggle tag"),
        ("r", "rename       s   split"),
        ("m", "merge (needs --merge)"),
        ("?", "toggle this help"),
    ];
    let key = Style::new().fg(Color::Rgb(255, 207, 77));
    let dim = Style::new().fg(Color::Rgb(160, 160, 168));
    KEYS.into_iter()
        .map(|(k, desc)| {
            Line::from(vec![
                Span::styled(format!("{k:<11}"), key),
                Span::styled(desc, dim),
            ])
        })
        .collect()
}

/// Builds a meter line plus its label/percent line for an inspector metric.
/// One inspector line carrying two abbreviated complexity axes — `rhy`thmic,
/// `pit`ch, `tec`hnical, `har`monic, `ply` (playability), `str`uctural — the
/// vector compressed to three rows so the dock keeps its transport block
/// visible in short terminals.
/// A `[0, 1]` value as a 4-cell percentage, or an em dash when the metric
/// abstains (vacuous on this input — see `inspector_lines`).
fn pct_or_dash(value: Option<f64>) -> String {
    value.map_or_else(
        || format!("{:>4}", "—"),
        |v| format!("{:>3.0}%", (v.clamp(0.0, 1.0) * 100.0).round()),
    )
}

fn complexity_pair(
    label_a: &str,
    value_a: Option<f64>,
    label_b: &str,
    value_b: Option<f64>,
) -> Line<'static> {
    Line::from(format!(
        "{label_a} {}   {label_b} {}",
        pct_or_dash(value_a),
        pct_or_dash(value_b)
    ))
}

/// Pushes one metric row; a measured value also gets its block meter, an
/// abstention gets the dash alone (a meter would assert a magnitude).
fn push_metric(lines: &mut Vec<Line<'static>>, label: &str, value: Option<f64>, style: Style) {
    lines.push(Line::from(format!("{label:<13}{}", pct_or_dash(value))));
    if let Some(v) = value {
        lines.push(Line::styled(meter(v, METER_W), style));
    }
}

/// Renders a 0..1 value as a unicode block meter of `width` cells.
fn meter(value: f64, width: usize) -> String {
    let filled = (value.clamp(0.0, 1.0) * width as f64).round() as usize;
    let filled = filled.min(width);
    let mut s = String::with_capacity(width);
    for i in 0..width {
        s.push(if i < filled { '█' } else { '░' });
    }
    s
}

/// Blits the section band of `scene` onto `buf`, with the band's left edge at
/// `x0` (the roll's left edge) on row `y` (the sections row).
fn paint_band(scene: &Scene, x0: u16, y: u16, buf: &mut Buffer, theme: &Theme) {
    for col in 0..scene.cols {
        if let Some(cell) = scene.band_cell(col) {
            paint_cell(buf, x0.saturating_add(col), y, cell, theme);
        }
    }
}

/// Blits the roll plane of `scene` onto `buf`, anchored at `area`'s top-left.
fn paint_plane(scene: &Scene, area: Rect, buf: &mut Buffer, theme: &Theme) {
    for r in 0..scene.rows {
        for col in 0..scene.cols {
            if let Some(cell) = scene.plane_cell(r, col) {
                paint_cell(
                    buf,
                    area.x.saturating_add(col),
                    area.y.saturating_add(r),
                    cell,
                    theme,
                );
            }
        }
    }
}

/// Maps one placed [`SceneCell`] to ratatui glyph + styling. This is the only
/// place semantic roles become concrete colours; `set_fg`/`set_style` carry no
/// background so a note keeps the black-key shade laid down underneath it.
fn paint_cell(buf: &mut Buffer, x: u16, y: u16, cell: &SceneCell, theme: &Theme) {
    let Some(c) = buf.cell_mut((x, y)) else {
        return;
    };
    let style = cell_style(*cell, theme);
    if cell.shade {
        c.set_bg(color(theme.row_shade));
    }
    match cell.role {
        CellRole::Empty => {}
        // The band is the one role that fills: its cell is a coloured block with
        // the section's class name written across it, so it takes the theme's
        // fill as a background and its ink as the text on top. Selection is
        // already the brighter fill; bold and underline are what a terminal can
        // add on top of that.
        CellRole::BandFill { selected, .. } => {
            let mut band = Style::new()
                .fg(style.ink.map_or(Color::White, color))
                .bg(style.fill.map_or(Color::Reset, color));
            if selected {
                band = band.add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
            }
            c.set_char(cell.glyph).set_style(band);
        }
        CellRole::Playhead => {
            let mut head = Style::new().add_modifier(Modifier::BOLD);
            if let Some(fill) = style.fill {
                head = head.fg(color(fill));
            }
            c.set_char(cell.glyph).set_style(head);
        }
        // Everything else is drawn, not filled: in a terminal a cell's colour is
        // its glyph's, and no background is set so a note keeps the black-key
        // shade laid down underneath it.
        CellRole::Separator
        | CellRole::GridLine
        | CellRole::BoundaryMark
        | CellRole::SectionMark(_)
        | CellRole::Note(_)
        | CellRole::PitchLabel
        | CellRole::BandHeader => {
            if let Some(fg) = style.ink.or(style.fill) {
                c.set_char(cell.glyph).set_style(Style::new().fg(color(fg)));
            }
        }
    }
}

/// Dumps a rendered buffer to plain-text rows (one `String` per row).
fn buffer_lines(buf: &Buffer) -> Vec<String> {
    let area = buf.area();
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buf.cell((x, y)).map_or(" ", Cell::symbol))
                .collect::<String>()
        })
        .collect()
}

/// What the curator left behind on quit, for the shell to persist.
///
/// The pending decision (if any), the tag set and the committed rename
/// (when they changed), and the pending split/merge rewrite (mutually
/// exclusive by the reducer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurationOutcome {
    /// The pending approve/reject decision.
    pub decision: Option<CurationDecision>,
    /// The live tag set in wire casing, when it changed.
    pub tags: Option<Vec<String>>,
    /// The committed rename, when it changed.
    pub title: Option<String>,
    /// The pending split point as a chunk-relative tick.
    pub split_tick: Option<u32>,
    /// Whether the merge with the `--merge` partner is armed.
    pub merge: bool,
}

/// Runs the interactive TUI to completion and returns the curation outcome
/// pending when the user quit, for the shell to persist.
///
/// # Errors
/// Propagates terminal I/O errors from `ratatui`.
pub fn run(mut app: App) -> io::Result<CurationOutcome> {
    let mut terminal = ratatui::try_init()?;
    let size = terminal.size()?;
    app.fit(size.width);
    let result = event_loop(&mut terminal, &mut app);
    ratatui::restore();
    result.map(|()| app.outcome())
}

fn event_loop(terminal: &mut DefaultTerminal, app: &mut App) -> io::Result<()> {
    let frame_dt = Duration::from_millis(16);
    let mut last = Instant::now();
    loop {
        terminal.draw(|f| app.render(f))?;
        let timeout = frame_dt.saturating_sub(last.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press && !app.on_key(key.code) {
                    return Ok(());
                }
            }
        }
        let now = Instant::now();
        let dt = now.saturating_duration_since(last);
        if dt >= frame_dt {
            app.tick(dt);
            last = now;
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
        clippy::str_to_string
    )]

    use std::path::PathBuf;
    use std::{env, fs};

    use super::*;
    use crate::analysis::Section;
    use crate::view::{Lane, NoteRect};
    use griff_core::classify::BarClass;
    use griff_core::structure::{ComplexityProfile, StructureMetrics};

    /// Compare `actual` to the stored golden frame, or write it when
    /// `GRIFF_BLESS=1` (the core characterization convention).
    fn assert_golden(name: &str, actual: &str) {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("golden")
            .join(format!("{name}.txt"));
        if env::var("GRIFF_BLESS").as_deref() == Ok("1") {
            fs::write(&path, actual).unwrap();
            return;
        }
        let expected = fs::read_to_string(&path).unwrap_or_else(|_| {
            panic!(
                "missing golden frame {}; create it with \
                 `GRIFF_BLESS=1 cargo test -p griff-preview`",
                path.display()
            )
        });
        assert_eq!(
            actual, expected,
            "rendered frame drifted from golden `{name}`. If intended, \
             re-bless with `GRIFF_BLESS=1 cargo test -p griff-preview`."
        );
    }

    // TDD red phase: scrolling the inspector reveals the clipped metrics
    // tail on a short terminal (the PR #38 liveness decision deliberately
    // let the tail clip; the scroll is the real fix).
    #[test]
    fn inspector_scroll_reveals_the_clipped_tail() {
        let mut app = demo_app();
        let before = app.snapshot(80, 12).expect("snapshot");
        assert!(
            !before.iter().any(|l| l.contains("ply")),
            "the complexity tail clips on a 12-row terminal"
        );
        for _ in 0..12 {
            app.vp.apply(Intent::InspectorScrollDown, &app.ctx.clone());
        }
        let after = app.snapshot(80, 12).expect("snapshot");
        assert!(
            after.iter().any(|l| l.contains("ply")),
            "scrolling brings the tail into view"
        );
    }

    // TDD red phase: Codex P2 (PR #41) — the scroll clamp must count
    // post-wrap rows. A long track name wraps in the 30-column dock, so the
    // pre-wrap clamp leaves the final wrapped rows unreachable.
    #[test]
    fn wrapped_inspector_lines_stay_reachable() {
        let mut app = demo_app();
        app.view.lanes[0].name =
            "An Extremely Long Imported MIDI Track Name That Wraps".to_string();
        for _ in 0..30 {
            app.vp.apply(Intent::InspectorScrollDown, &app.ctx.clone());
        }
        let after = app.snapshot(80, 12).expect("snapshot");
        assert!(
            after.iter().any(|l| l.contains("ply")),
            "the tail stays reachable when lines wrap"
        );
    }

    // TDD red phase: the inspector surfaces the loaded record's curation
    // state (the S8 slice before rename/tag). References `App::set_record`,
    // which does not exist yet, so the crate fails to compile until the
    // green step.
    #[test]
    fn inspector_shows_the_loaded_record_state() {
        use crate::curation::RecordSummary;

        let mut app = demo_app();
        app.set_record(RecordSummary {
            title: "Curated".to_string(),
            reviewer: Some("accepted".to_string()),
            tags: vec!["clean_riff".to_string(), "maj7".to_string()],
        });
        let frame = app.snapshot(80, 24).expect("snapshot");
        let text = frame.join("\n");
        assert!(text.contains("Curated"), "record title shows in the dock");
        assert!(text.contains("accepted"), "prior reviewer decision shows");
        assert!(text.contains("clean_riff"), "record tags show");
    }

    // TDD red phase: Codex P2 (PR #42) — the record digest is static and
    // must not push the live transport state out of a short dock: the
    // liveness ordering (PR #38) clips static tails, never live state.
    #[test]
    fn record_summary_keeps_transport_visible() {
        use crate::curation::RecordSummary;

        let mut app = demo_app();
        app.set_record(RecordSummary {
            title: "Curated".to_string(),
            reviewer: Some("accepted".to_string()),
            tags: vec!["clean_riff".to_string()],
        });
        let frame = app.snapshot(80, 12).expect("snapshot");
        let text = frame.join("\n");
        assert!(
            text.contains("transport"),
            "live transport stays visible above the static record digest"
        );
    }

    // TDD red phase: Codex P2 (PR #41, round 2) — overscrolling must not
    // accumulate hidden excess: one PgUp from the bottom moves the dock,
    // because the render writes the clamped offset back.
    #[test]
    fn pgup_responds_immediately_after_overscroll() {
        let mut app = demo_app();
        for _ in 0..30 {
            app.vp.apply(Intent::InspectorScrollDown, &app.ctx.clone());
        }
        let bottom = app.snapshot(80, 12).expect("snapshot");
        app.vp.apply(Intent::InspectorScrollUp, &app.ctx.clone());
        let up = app.snapshot(80, 12).expect("snapshot");
        assert_ne!(bottom, up, "one PgUp from the bottom moves the dock");
    }

    // TDD red phase: tag editing reaches the TUI (S8 curation slice 3).
    // 't' cycles the palette cursor, 'T' toggles the tag, the record block
    // shows the live tag set and the cursor, and the changed set is
    // surfaced for quit-time persistence. References items that do not
    // exist yet, so the crate fails to compile until the green step.

    #[test]
    fn tag_keys_map_to_tag_intents() {
        assert_eq!(App::key_intent(KeyCode::Char('t')), Some(Intent::TagNext));
        assert_eq!(App::key_intent(KeyCode::Char('T')), Some(Intent::TagToggle));
    }

    #[test]
    fn set_record_seeds_the_tag_state() {
        use crate::curation::{tag_palette, RecordSummary};

        let mut app = demo_app();
        app.set_record(RecordSummary {
            title: "Curated".to_string(),
            reviewer: None,
            tags: vec!["clean_riff".to_string(), "maj7".to_string()],
        });
        assert_eq!(
            usize::from(app.ctx.tag_count),
            tag_palette().len(),
            "the full palette is cyclable"
        );
        assert_eq!(app.vp.tags & 0b1, 0b1, "clean_riff (palette[0]) is set");
        assert_eq!(
            app.tags_if_changed(),
            None,
            "untouched set persists nothing"
        );
    }

    #[test]
    fn toggling_a_tag_updates_the_dock_and_the_outcome() {
        use crate::curation::RecordSummary;

        let mut app = demo_app();
        app.set_record(RecordSummary {
            title: "Curated".to_string(),
            reviewer: None,
            tags: vec!["clean_riff".to_string(), "maj7".to_string()],
        });
        let before = app.snapshot(80, 24).expect("snapshot").join("\n");
        assert!(
            before.contains("tag▸ clean_riff"),
            "the cursor line names palette[0]"
        );

        // Cursor sits on clean_riff; toggling clears it (repeat to undo).
        app.vp.apply(Intent::TagToggle, &app.ctx.clone());
        let after = app.snapshot(80, 24).expect("snapshot").join("\n");
        assert!(
            !after.contains("tags  clean_riff"),
            "the live tags line drops the cleared tag"
        );
        assert_eq!(
            app.tags_if_changed(),
            Some(vec!["maj7".to_string()]),
            "the changed set is surfaced for persistence"
        );
    }

    // TDD red phase: rename reaches the TUI (S8 curation slice 4). 'r'
    // enters the mode seeded with the live title; typed keys edit the
    // frontend-local buffer; Enter commits (trim, non-empty), Esc cancels;
    // the committed title is surfaced for quit-time persistence.
    // References items that do not exist yet, so the crate fails to
    // compile until the green step.

    #[test]
    fn r_maps_to_rename_start() {
        assert_eq!(
            App::key_intent(KeyCode::Char('r')),
            Some(Intent::RenameStart)
        );
    }

    #[test]
    fn rename_flow_commits_a_new_title() {
        use crate::curation::RecordSummary;

        let mut app = demo_app();
        app.set_record(RecordSummary {
            title: "Curated".to_string(),
            reviewer: None,
            tags: Vec::new(),
        });
        assert_eq!(app.title_if_changed(), None, "nothing pending initially");

        app.on_key(KeyCode::Char('r'));
        assert!(app.vp.renaming, "r enters the rename mode");
        // The buffer arrives seeded with the live title; extend it.
        app.on_key(KeyCode::Char(' '));
        app.on_key(KeyCode::Char('2'));
        app.on_key(KeyCode::Char('x'));
        app.on_key(KeyCode::Backspace);
        let frame = app.snapshot(80, 24).expect("snapshot").join("\n");
        assert!(
            frame.contains("name▸ Curated 2"),
            "the dock shows the live buffer while renaming"
        );

        app.on_key(KeyCode::Enter);
        assert!(!app.vp.renaming, "enter leaves the mode");
        assert_eq!(
            app.title_if_changed(),
            Some("Curated 2".to_string()),
            "the committed title is surfaced for persistence"
        );
    }

    #[test]
    fn rename_esc_cancels_without_committing() {
        use crate::curation::RecordSummary;

        let mut app = demo_app();
        app.set_record(RecordSummary {
            title: "Curated".to_string(),
            reviewer: None,
            tags: Vec::new(),
        });
        app.on_key(KeyCode::Char('r'));
        app.on_key(KeyCode::Char('!'));
        let cont = app.on_key(KeyCode::Esc);
        assert!(cont, "esc cancels the rename, not the app");
        assert!(!app.vp.renaming);
        assert_eq!(app.title_if_changed(), None, "nothing committed");

        // 'q' quits again once the mode is left.
        assert!(!app.on_key(KeyCode::Char('q')));
    }

    #[test]
    fn typing_q_inside_the_rename_mode_does_not_quit() {
        use crate::curation::RecordSummary;

        let mut app = demo_app();
        app.set_record(RecordSummary {
            title: "Curated".to_string(),
            reviewer: None,
            tags: Vec::new(),
        });
        app.on_key(KeyCode::Char('r'));
        assert!(app.on_key(KeyCode::Char('q')), "q is text while renaming");
        app.on_key(KeyCode::Enter);
        assert_eq!(
            app.title_if_changed(),
            Some("Curatedq".to_string()),
            "the q landed in the buffer"
        );
    }

    #[test]
    fn page_keys_map_to_inspector_scroll() {
        assert_eq!(
            App::key_intent(KeyCode::PageDown),
            Some(Intent::InspectorScrollDown)
        );
        assert_eq!(
            App::key_intent(KeyCode::PageUp),
            Some(Intent::InspectorScrollUp)
        );
    }

    // TDD red phase: the `?` help overlay reaches the TUI (the
    // discoverability slice). '?' toggles a centered cheatsheet, any key
    // dismisses it (the rename-modal precedent), and the overlay names the
    // curation keys. References an intent and a field that do not exist yet,
    // so the crate fails to compile until the green step.

    #[test]
    fn question_mark_maps_to_toggle_help() {
        assert_eq!(
            App::key_intent(KeyCode::Char('?')),
            Some(Intent::ToggleHelp)
        );
    }

    #[test]
    fn help_overlay_lists_the_curation_keys() {
        let mut app = demo_app();
        app.on_key(KeyCode::Char('?'));
        assert!(app.vp.show_help, "? opens the help overlay");
        let text = app.snapshot(80, 24).expect("snapshot").join("\n");
        assert!(text.contains("Help"), "the overlay carries a title");
        assert!(text.contains("split"), "the overlay lists split");
        assert!(text.contains("merge"), "the overlay lists merge");
    }

    #[test]
    fn any_key_dismisses_the_help_overlay() {
        let mut app = demo_app();
        app.on_key(KeyCode::Char('?'));
        assert!(app.vp.show_help);
        let cont = app.on_key(KeyCode::Char('j'));
        assert!(cont, "dismissing the help keeps the app running");
        assert!(!app.vp.show_help, "any key closes the overlay");
    }

    // TDD red phase: split/merge reaches the TUI (S8 curation slice 5).
    // 's' marks the playhead as the pending split point, 'm' arms the merge
    // with the partner record attached via the new `set_merge_partner`, the
    // record block shows the pending rewrite, and quit-time persistence
    // receives everything through the new `CurationOutcome` struct built by
    // `App::outcome`. References items that do not exist yet, so the crate
    // fails to compile until the green step.

    #[test]
    fn split_and_merge_keys_map_to_intents() {
        assert_eq!(
            App::key_intent(KeyCode::Char('s')),
            Some(Intent::SplitAtPlayhead)
        );
        assert_eq!(
            App::key_intent(KeyCode::Char('m')),
            Some(Intent::MergeToggle)
        );
    }

    #[test]
    fn split_flow_marks_the_playhead_and_surfaces_the_outcome() {
        use crate::curation::RecordSummary;

        let mut app = demo_app();
        app.set_record(RecordSummary {
            title: "Curated".to_string(),
            reviewer: None,
            tags: Vec::new(),
        });
        app.vp.play_tick = 960;
        app.on_key(KeyCode::Char('s'));
        assert_eq!(app.vp.split_tick, Some(960), "s marks the playhead");
        let text = app.snapshot(80, 24).expect("snapshot").join("\n");
        assert!(text.contains("split"), "the dock shows the pending split");
        assert_eq!(app.outcome().split_tick, Some(960));

        app.vp.play_tick = 960;
        app.on_key(KeyCode::Char('s'));
        assert_eq!(app.outcome().split_tick, None, "the same spot disarms");
    }

    // TDD red phase: the split gate matches persistence (Codex P2, PR #45)
    // — App::new must seed the core's bar length from the view's bar grid,
    // so 's' inside the first bar never arms a doomed split.
    #[test]
    fn split_key_refuses_the_first_bar() {
        use crate::curation::RecordSummary;

        let mut app = demo_app();
        app.set_record(RecordSummary {
            title: "Curated".to_string(),
            reviewer: None,
            tags: Vec::new(),
        });
        app.vp.play_tick = 500; // inside the first bar (bars at 0/960/1920)
        app.on_key(KeyCode::Char('s'));
        assert_eq!(
            app.vp.split_tick, None,
            "persistence would reject this split, so the UI must not arm it"
        );
    }

    // TDD red phase (Codex P2, PR #45, round 4): App::new must seed the
    // core's grid end from the view's final barline, so the split gate can
    // refuse a playhead in a ringing note's tail past it.
    #[test]
    fn app_seeds_the_grid_end_from_the_final_barline() {
        let app = demo_app();
        assert_eq!(
            app.ctx.grid_end, 1920,
            "the last bar line is the split gate's ceiling"
        );
    }

    #[test]
    fn merge_flow_requires_an_attached_partner() {
        use crate::curation::RecordSummary;

        let mut app = demo_app();
        app.set_record(RecordSummary {
            title: "Curated".to_string(),
            reviewer: None,
            tags: Vec::new(),
        });
        app.on_key(KeyCode::Char('m'));
        assert!(!app.vp.merging, "no partner, no merge");

        app.set_merge_partner("Other".to_string());
        app.on_key(KeyCode::Char('m'));
        assert!(
            app.vp.merging,
            "m arms the merge once a partner is attached"
        );
        let text = app.snapshot(80, 24).expect("snapshot").join("\n");
        assert!(text.contains("merge"), "the dock shows the pending merge");
        assert!(text.contains("Other"), "the dock names the partner");
        assert!(app.outcome().merge);

        app.on_key(KeyCode::Char('m'));
        assert!(!app.outcome().merge, "the same intent again disarms");
    }

    #[test]
    fn outcome_carries_the_decision_alongside_the_marks() {
        use crate::viewport::CurationDecision;

        let mut app = demo_app();
        app.on_key(KeyCode::Char('a'));
        let outcome = app.outcome();
        assert_eq!(outcome.decision, Some(CurationDecision::Approve));
        assert_eq!(outcome.tags, None);
        assert_eq!(outcome.title, None);
        assert_eq!(outcome.split_tick, None);
        assert!(!outcome.merge);
    }

    fn demo_app() -> App {
        let view = PianoRollView {
            ppq: 480,
            tick_start: 0,
            tick_end: 1920,
            low_pitch: 40,
            high_pitch: 52,
            bar_lines: vec![0, 960, 1920],
            lanes: vec![Lane {
                name: "Rhythm".to_string(),
                notes: vec![
                    NoteRect {
                        onset: 0,
                        end: 480,
                        pitch: 40,
                    },
                    NoteRect {
                        onset: 960,
                        end: 1440,
                        pitch: 47,
                    },
                ],
            }],
            tempo_bpm: 120.0,
            bar_count: 2,
        };
        let analysis = Analysis {
            focus_track: 0,
            sections: vec![
                Section {
                    class: BarClass::Riff,
                    bar_start: 0,
                    bar_end: 1,
                    tick_start: 0,
                    tick_end: 960,
                },
                Section {
                    class: BarClass::Solo,
                    bar_start: 1,
                    bar_end: 2,
                    tick_start: 960,
                    tick_end: 1920,
                },
            ],
            metrics: None,
            boundaries: vec![480],
            complexity: Some(ComplexityProfile {
                rhythmic: 0.25,
                pitch: 0.5,
                technical: 0.0,
                harmonic: 0.125,
                playability: 1.0,
                structural: 0.5,
            }),
        };
        App::new(view, analysis, "demo.mid".to_string())
    }

    // Characterization goldens: pin the exact rendered frame before and after a
    // scripted interaction, so the viewport refactor cannot silently change what
    // the terminal draws. Regenerate deliberately if the UI is meant to change.
    #[test]
    fn transport_stays_visible_with_full_metrics() {
        // Codex P2 (PR #38): with both structure metrics and complexity
        // measured (the real imported-score path), the inspector content
        // exceeds a 20-row terminal. The clipping must eat the tail of the
        // static metrics, not the live transport block.
        let mut app = demo_app();
        app.analysis.metrics = Some(StructureMetrics {
            bar_count: 2,
            detected_pattern_period_bars: Some(1),
            detected_pattern_period_ticks: Some(960),
            detected_subbar_period_ticks: None,
            repeatability_score: 0.75,
            variation_score: 0.25,
            loopability_score: 0.5,
            structural_complexity: 0.5,
        });
        let text = app.snapshot(80, 20).expect("renders").join("\n");
        assert!(text.contains("transport"), "transport block visible");
        assert!(text.contains("⏸ paused"), "play state visible");
        assert!(text.contains("pos 1:1"), "play position visible");
    }

    // TDD red phase: on a single-bar score the bar-ratio metrics are
    // vacuous, not measured — `repeatability` has no second bar to compare
    // (core returns its 0.0 abstention), so `variation = 1 − 0` and the
    // distinct-signature ratios (`complexity`, the `str` axis) are `1/1` by
    // construction. The inspector must abstain with an em dash instead of
    // asserting 0%/100%. `loopability` (a seam measurement on the span) and
    // the per-note axes stay numeric.
    #[test]
    fn single_bar_score_dashes_out_bar_ratio_metrics() {
        let mut app = demo_app();
        app.view.bar_count = 1;
        app.analysis.metrics = Some(StructureMetrics {
            bar_count: 1,
            detected_pattern_period_bars: None,
            detected_pattern_period_ticks: None,
            detected_subbar_period_ticks: None,
            repeatability_score: 0.0,
            variation_score: 1.0,
            loopability_score: 0.625,
            structural_complexity: 1.0,
        });
        let frame = app.snapshot(80, 32).expect("renders");
        let line = |label: &str| {
            frame
                .iter()
                .find(|l| l.contains(label) && !l.contains("(S14)"))
                .cloned()
                .unwrap_or_else(|| panic!("inspector line `{label}` missing"))
        };

        assert!(
            line("loopability").contains('%'),
            "loopability is a real seam measurement even on one bar"
        );
        for label in ["repeatability", "variation", "complexity"] {
            let l = line(label);
            assert!(l.contains('—'), "`{label}` abstains with a dash: {l}");
            assert!(!l.contains('%'), "`{label}` shows no percentage: {l}");
        }
        let pair = line("ply");
        let after_str = pair.split("str").nth(1).expect("str axis on the line");
        assert!(
            after_str.contains('—') && !after_str.contains('%'),
            "the str axis abstains with a dash: {pair}"
        );
        assert!(
            pair.split("str").next().is_some_and(|s| s.contains('%')),
            "ply stays numeric on one bar: {pair}"
        );
    }

    // The two-bar control: the same metrics render as percentages when a
    // second bar exists to compare against.
    #[test]
    fn two_bar_score_keeps_bar_ratio_percentages() {
        let mut app = demo_app();
        app.analysis.metrics = Some(StructureMetrics {
            bar_count: 2,
            detected_pattern_period_bars: Some(1),
            detected_pattern_period_ticks: Some(960),
            detected_subbar_period_ticks: None,
            repeatability_score: 0.75,
            variation_score: 0.25,
            loopability_score: 0.5,
            structural_complexity: 0.5,
        });
        let frame = app.snapshot(80, 32).expect("renders");
        let line = |label: &str| {
            frame
                .iter()
                .find(|l| l.contains(label) && !l.contains("(S14)"))
                .cloned()
                .unwrap_or_else(|| panic!("inspector line `{label}` missing"))
        };
        for label in ["repeatability", "variation", "complexity"] {
            assert!(
                line(label).contains('%'),
                "`{label}` is measured with two bars"
            );
        }
        let pair = line("ply");
        assert!(
            pair.split("str")
                .nth(1)
                .is_some_and(|s| s.contains('%') && !s.contains('—')),
            "the str axis is measured with two bars: {pair}"
        );
    }

    #[test]
    fn curation_keys_set_and_show_the_decision() {
        // S8 curation slice: 'a' approves, 'x' rejects, the inspector shows
        // the pending decision, and repeating a key clears it.
        let mut app = demo_app();
        let initial = app.snapshot(80, 20).expect("renders").join("\n");
        assert!(
            initial.contains("curation"),
            "the inspector names the block"
        );

        app.on_key(KeyCode::Char('a'));
        let approved = app.snapshot(80, 20).expect("renders").join("\n");
        assert!(approved.contains("approved"), "a marks the chunk approved");

        app.on_key(KeyCode::Char('x'));
        let rejected = app.snapshot(80, 20).expect("renders").join("\n");
        assert!(rejected.contains("rejected"), "x overwrites with rejected");
    }

    #[test]
    fn render_byte_stable_initial() {
        let mut app = demo_app();
        let got = app.snapshot(80, 20).expect("renders").join("\n");
        assert_golden("initial_80x20", &got);
    }

    #[test]
    fn render_byte_stable_after_actions() {
        let mut app = demo_app();
        app.fit(80);
        app.on_key(KeyCode::Char(' ')); // play
        app.on_key(KeyCode::Char(']')); // next section
        app.on_key(KeyCode::Char('+')); // zoom in
        let got = app.snapshot(80, 20).expect("renders").join("\n");
        assert_golden("acted_80x20", &got);
    }

    #[test]
    fn snapshot_has_exact_dimensions() {
        let mut app = demo_app();
        let lines = app.snapshot(80, 20).expect("snapshot renders");
        assert_eq!(lines.len(), 20, "row count equals height");
        for l in &lines {
            assert_eq!(l.chars().count(), 80, "each row equals width");
        }
    }

    #[test]
    fn snapshot_shows_header_and_sections() {
        let mut app = demo_app();
        let text = app.snapshot(90, 22).expect("renders").join("\n");
        assert!(text.contains("griff·preview"), "header present");
        assert!(
            text.contains("Riff") || text.contains("Solo"),
            "named section present"
        );
        assert!(text.contains("Inspector"), "inspector dock present");
        assert!(text.contains('█'), "note blocks are drawn in the plane");
    }

    #[test]
    fn quit_key_stops_and_space_toggles_play() {
        let mut app = demo_app();
        assert!(app.on_key(KeyCode::Char(' ')), "space keeps running");
        assert!(app.vp.playing, "space starts playback");
        assert!(!app.on_key(KeyCode::Char('q')), "q requests quit");
    }

    #[test]
    fn section_nav_moves_scroll_to_section() {
        let mut app = demo_app();
        app.on_key(KeyCode::Char(']'));
        assert_eq!(app.vp.sel_section, 1);
        assert_eq!(app.vp.scroll_tick, 960, "selecting a section scrolls to it");
    }

    #[test]
    fn zoom_keys_change_resolution() {
        let mut app = demo_app();
        app.fit(80);
        let base = app.vp.ticks_per_col;
        app.on_key(KeyCode::Char('+'));
        assert!(app.vp.ticks_per_col <= base, "zoom in lowers ticks/col");
    }
}
