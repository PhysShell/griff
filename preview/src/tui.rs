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
use ratatui::widgets::{Block, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame, Terminal};

use griff_core::classify::BarClass;

use crate::analysis::Analysis;
use crate::scene::{resolve, CellRole, GridSize, Scene, SceneCell, GUTTER};
use crate::view::PianoRollView;
use crate::viewport::{Intent, Step, ViewContext, Viewport};

/// Width of the inspector dock.
const INSPECTOR_W: u16 = 32;
/// Width of a metric meter bar in the inspector.
const METER_W: usize = 22;

/// Distinct lane colours, cycled by lane index (mirrors the HTML mockup).
const LANE_COLORS: [Color; 6] = [
    Color::Rgb(255, 122, 69),
    Color::Rgb(54, 207, 201),
    Color::Rgb(146, 84, 222),
    Color::Rgb(255, 196, 77),
    Color::Rgb(110, 180, 255),
    Color::Rgb(245, 93, 108),
];

fn lane_color(i: usize) -> Color {
    LANE_COLORS
        .get(i % LANE_COLORS.len())
        .copied()
        .unwrap_or(Color::White)
}

/// Section colour by classification (matches the mockup's section bands).
const fn class_color(c: BarClass) -> Color {
    match c {
        BarClass::Riff => Color::Rgb(22, 104, 220),
        BarClass::Breakdown => Color::Rgb(207, 19, 34),
        BarClass::Solo => Color::Rgb(212, 136, 6),
        BarClass::Clean => Color::Rgb(56, 158, 13),
        BarClass::Unknown => Color::Rgb(90, 90, 100),
    }
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
    vp: Viewport,
    ctx: ViewContext,
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
        };
        let vp = Viewport::new(&ctx, view.high_pitch);
        Self {
            view,
            analysis,
            file,
            vp,
            ctx,
        }
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
        let Some(intent) = Self::key_intent(code) else {
            return true;
        };
        self.vp.apply(intent, &self.ctx) == Step::Continue
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
            KeyCode::Char('0') | KeyCode::Home => Intent::Home,
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
        paint_band(&scene, roll.x, sections.y, buf);
        paint_plane(&scene, roll, buf);
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

    fn render_inspector(&self, area: Rect, frame: &mut Frame<'_>) {
        let block = Block::bordered()
            .title(" Inspector ")
            .border_style(Style::new().fg(Color::Rgb(70, 70, 78)));
        let inner = block.inner(area);
        frame.render_widget(block, area);

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
                    .bg(class_color(s.class))
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(format!(
                "bars {}–{} · {} bar(s)",
                s.bar_start + 1,
                s.bar_end,
                s.bar_count()
            )));
        }

        lines.push(Line::raw(""));
        lines.push(Line::styled("structure (S14)", dim));
        if let Some(m) = &self.analysis.metrics {
            push_metric(&mut lines, "loopability", m.loopability_score, accent);
            push_metric(&mut lines, "repeatability", m.repeatability_score, accent);
            push_metric(&mut lines, "variation", m.variation_score, accent);
            push_metric(&mut lines, "complexity", m.structural_complexity, accent);
            let period = m
                .detected_pattern_period_bars
                .map_or_else(|| "—".to_owned(), |p| format!("{p} bars"));
            lines.push(Line::from(format!("pattern   {period}")));
        } else {
            lines.push(Line::raw("—"));
        }

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

        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
    }

    // ── helpers ─────────────────────────────────────────────────────────
    fn position_label(&self) -> String {
        let ppq = u32::from(self.view.ppq).max(1);
        let mut bar_idx = 0usize;
        let mut bar_start = self.view.tick_start;
        for (i, &t) in self.view.bar_lines.iter().enumerate() {
            if t <= self.vp.play_tick {
                bar_idx = i;
                bar_start = t;
            } else {
                break;
            }
        }
        let beat = self.vp.play_tick.saturating_sub(bar_start) / ppq;
        format!("{}:{}", bar_idx.saturating_add(1), beat.saturating_add(1))
    }
}

/// Renders the static footer hint line.
fn render_footer(area: Rect, frame: &mut Frame<'_>) {
    let hint =
        "q quit · space play · ←/→ scroll · ↑/↓ pitch · +/- zoom · [/]/tab section · i inspector";
    frame.render_widget(
        Paragraph::new(Line::styled(
            hint,
            Style::new().fg(Color::Rgb(120, 120, 128)),
        )),
        area,
    );
}

/// Builds a meter line plus its label/percent line for an inspector metric.
fn push_metric(lines: &mut Vec<Line<'static>>, label: &str, value: f64, style: Style) {
    let pct = (value.clamp(0.0, 1.0) * 100.0).round();
    lines.push(Line::from(format!("{label:<13}{pct:>3.0}%")));
    lines.push(Line::styled(meter(value, METER_W), style));
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
fn paint_band(scene: &Scene, x0: u16, y: u16, buf: &mut Buffer) {
    for col in 0..scene.cols {
        if let Some(cell) = scene.band_cell(col) {
            paint_cell(buf, x0.saturating_add(col), y, cell);
        }
    }
}

/// Blits the roll plane of `scene` onto `buf`, anchored at `area`'s top-left.
fn paint_plane(scene: &Scene, area: Rect, buf: &mut Buffer) {
    for r in 0..scene.rows {
        for col in 0..scene.cols {
            if let Some(cell) = scene.plane_cell(r, col) {
                paint_cell(
                    buf,
                    area.x.saturating_add(col),
                    area.y.saturating_add(r),
                    cell,
                );
            }
        }
    }
}

/// Maps one placed [`SceneCell`] to ratatui glyph + styling. This is the only
/// place semantic roles become concrete colours; `set_fg`/`set_style` carry no
/// background so a note keeps the black-key shade laid down underneath it.
fn paint_cell(buf: &mut Buffer, x: u16, y: u16, cell: &SceneCell) {
    let Some(c) = buf.cell_mut((x, y)) else {
        return;
    };
    if cell.shade {
        c.set_bg(Color::Rgb(38, 38, 44));
    }
    let dim = Style::new().fg(Color::Rgb(70, 70, 80));
    match cell.role {
        CellRole::Empty => {}
        CellRole::Separator | CellRole::GridLine => {
            c.set_char(cell.glyph).set_style(dim);
        }
        CellRole::SectionMark(class) => {
            c.set_char(cell.glyph)
                .set_style(Style::new().fg(class_color(class)));
        }
        CellRole::Note(lane) => {
            c.set_char(cell.glyph).set_fg(lane_color(usize::from(lane)));
        }
        CellRole::Playhead => {
            c.set_char(cell.glyph).set_style(
                Style::new()
                    .fg(Color::Rgb(255, 207, 77))
                    .add_modifier(Modifier::BOLD),
            );
        }
        CellRole::PitchLabel => {
            c.set_char(cell.glyph)
                .set_style(Style::new().fg(Color::Rgb(150, 150, 158)));
        }
        CellRole::BandHeader => {
            c.set_char(cell.glyph)
                .set_style(Style::new().fg(Color::Rgb(110, 110, 118)));
        }
        CellRole::BandFill { class, selected } => {
            let mut style = Style::new().bg(class_color(class)).fg(Color::White);
            if selected {
                style = style.add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
            }
            c.set_char(cell.glyph).set_style(style);
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

/// Runs the interactive crossterm event loop until the user quits.
///
/// # Errors
/// Propagates terminal setup, draw, and input errors from `ratatui`.
pub fn run(mut app: App) -> io::Result<()> {
    let mut terminal = ratatui::try_init()?;
    let size = terminal.size()?;
    app.fit(size.width);
    let result = event_loop(&mut terminal, &mut app);
    ratatui::restore();
    result
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

    use super::*;
    use crate::analysis::Section;
    use crate::view::{Lane, NoteRect};

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
        };
        App::new(view, analysis, "demo.mid".to_string())
    }

    // Characterization goldens: pin the exact rendered frame before and after a
    // scripted interaction, so the viewport refactor cannot silently change what
    // the terminal draws. Regenerate deliberately if the UI is meant to change.
    #[test]
    fn render_byte_stable_initial() {
        let mut app = demo_app();
        let got = app.snapshot(80, 20).expect("renders").join("\n");
        assert_eq!(got, include_str!("golden/initial_80x20.txt"));
    }

    #[test]
    fn render_byte_stable_after_actions() {
        let mut app = demo_app();
        app.fit(80);
        app.on_key(KeyCode::Char(' ')); // play
        app.on_key(KeyCode::Char(']')); // next section
        app.on_key(KeyCode::Char('+')); // zoom in
        let got = app.snapshot(80, 20).expect("renders").join("\n");
        assert_eq!(got, include_str!("golden/acted_80x20.txt"));
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
