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
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::too_many_lines
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
use crate::render::pitch_name;
use crate::view::PianoRollView;

/// Left gutter width inside the roll: 4 columns of pitch label + 1 separator.
const GUTTER: u16 = 5;
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

const fn is_black_key(pitch: u8) -> bool {
    matches!(pitch % 12, 1 | 3 | 6 | 8 | 10)
}

/// Interactive piano-roll application state.
#[derive(Debug, Clone)]
pub struct App {
    view: PianoRollView,
    analysis: Analysis,
    file: String,
    scroll_tick: u32,
    ticks_per_col: u32,
    top_pitch: u8,
    sel_section: usize,
    playing: bool,
    play_tick: u32,
    show_inspector: bool,
}

impl App {
    /// Builds the app from a view, its analysis, and the source file label.
    #[must_use]
    pub fn new(view: PianoRollView, analysis: Analysis, file: String) -> Self {
        let top_pitch = view.high_pitch.saturating_add(1).min(127);
        let scroll_tick = view.tick_start;
        Self {
            view,
            analysis,
            file,
            scroll_tick,
            ticks_per_col: 60,
            top_pitch,
            sel_section: 0,
            playing: false,
            play_tick: scroll_tick,
            show_inspector: true,
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
        let reserved = GUTTER.saturating_add(if self.show_inspector { INSPECTOR_W } else { 0 });
        let plot = total_width.saturating_sub(reserved).max(1);
        let span = self
            .view
            .tick_end
            .saturating_sub(self.view.tick_start)
            .max(1);
        self.ticks_per_col = (span / u32::from(plot)).max(1);
        self.scroll_tick = self.view.tick_start;
    }

    // ── input ───────────────────────────────────────────────────────────
    /// Handles a key press; returns `false` when the app should quit.
    fn on_key(&mut self, code: KeyCode) -> bool {
        let scroll_step = self.ticks_per_col.saturating_mul(8).max(1);
        match code {
            KeyCode::Char('q') | KeyCode::Esc => return false,
            KeyCode::Char(' ') => {
                if !self.playing && self.play_tick >= self.view.tick_end {
                    self.play_tick = self.view.tick_start;
                }
                self.playing = !self.playing;
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.scroll_tick = self
                    .scroll_tick
                    .saturating_sub(scroll_step)
                    .max(self.view.tick_start);
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.scroll_tick = self.scroll_tick.saturating_add(scroll_step);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.top_pitch = self.top_pitch.saturating_add(2).min(127);
            }
            KeyCode::Down | KeyCode::Char('j') => self.top_pitch = self.top_pitch.saturating_sub(2),
            KeyCode::Char('+' | '=') => {
                self.ticks_per_col = (self.ticks_per_col.saturating_mul(2) / 3).max(1);
            }
            KeyCode::Char('-' | '_') => {
                self.ticks_per_col = (self.ticks_per_col.saturating_mul(3) / 2).max(1);
            }
            KeyCode::Char('[') => self.select_section(self.sel_section.saturating_sub(1)),
            KeyCode::Char(']') | KeyCode::Tab => {
                self.select_section(self.sel_section.saturating_add(1));
            }
            KeyCode::Char('i') => self.show_inspector = !self.show_inspector,
            KeyCode::Char('0') | KeyCode::Home => {
                self.scroll_tick = self.view.tick_start;
                self.play_tick = self.view.tick_start;
            }
            _ => {}
        }
        true
    }

    fn select_section(&mut self, idx: usize) {
        let last = self.analysis.sections.len().saturating_sub(1);
        self.sel_section = idx.min(last);
        if let Some(s) = self.analysis.sections.get(self.sel_section) {
            self.scroll_tick = s.tick_start;
            self.play_tick = s.tick_start;
        }
    }

    /// Advances playback by `dt`.
    fn tick(&mut self, dt: Duration) {
        if !self.playing {
            return;
        }
        let tps = f64::from(self.view.ppq) * self.view.tempo_bpm / 60.0;
        let adv = (tps * dt.as_secs_f64()) as u32;
        self.play_tick = self.play_tick.saturating_add(adv.max(1));
        if self.play_tick >= self.view.tick_end {
            self.play_tick = self.view.tick_end;
            self.playing = false;
        }
    }

    fn autoscroll(&mut self, roll_width: u16) {
        if !self.playing || roll_width <= GUTTER {
            return;
        }
        let plot_w = u32::from(roll_width - GUTTER);
        let right = self
            .scroll_tick
            .saturating_add(plot_w.saturating_mul(self.ticks_per_col));
        if self.play_tick < self.scroll_tick || self.play_tick >= right {
            let back = (plot_w / 4).saturating_mul(self.ticks_per_col);
            self.scroll_tick = self.play_tick.saturating_sub(back);
        }
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

        let (roll, inspector) = if self.show_inspector && body.width > INSPECTOR_W + 12 {
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

        let buf = frame.buffer_mut();
        self.render_sections(sections, roll, buf);
        self.render_roll(roll, buf);
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

    fn render_sections(&self, row: Rect, roll: Rect, buf: &mut Buffer) {
        if roll.width <= GUTTER {
            return;
        }
        let y = row.y;
        let plot_x0 = roll.x + GUTTER;
        let plot_w = roll.width - GUTTER;
        let xmax = plot_x0.saturating_add(plot_w);
        put_str(
            buf,
            (row.x, y),
            GUTTER - 1,
            "SEC",
            Style::new().fg(Color::Rgb(110, 110, 118)),
        );

        for (i, s) in self.analysis.sections.iter().enumerate() {
            let a = self.tick_to_col(s.tick_start, plot_x0, plot_w);
            let b = self.tick_to_col(s.tick_end, plot_x0, plot_w);
            if b <= a {
                continue;
            }
            let mut style = Style::new().bg(class_color(s.class)).fg(Color::White);
            if i == self.sel_section {
                style = style.add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
            }
            for x in a..b.min(xmax) {
                if let Some(c) = buf.cell_mut((x, y)) {
                    c.set_char(' ').set_style(style);
                }
            }
            let label = s.class.to_string();
            let w = usize::from(b - a);
            if w >= label.chars().count() {
                let off = ((w - label.chars().count()) / 2) as u16;
                put_str(buf, (a + off, y), plot_w, &label, style);
            }
        }
    }

    fn render_roll(&self, area: Rect, buf: &mut Buffer) {
        if area.width <= GUTTER || area.height == 0 {
            return;
        }
        let plot_x0 = area.x + GUTTER;
        let plot_w = area.width - GUTTER;
        let rows = area.height;
        let dim = Style::new().fg(Color::Rgb(70, 70, 80));

        // black-key shading, separator column, pitch labels
        for r in 0..rows {
            let y = area.y + r;
            let pitch = i32::from(self.top_pitch) - i32::from(r);
            if pitch >= 0 && is_black_key(pitch as u8) {
                for x in plot_x0..plot_x0.saturating_add(plot_w) {
                    if let Some(c) = buf.cell_mut((x, y)) {
                        c.set_bg(Color::Rgb(38, 38, 44));
                    }
                }
            }
            if let Some(c) = buf.cell_mut((plot_x0.saturating_sub(1), y)) {
                c.set_char('│').set_style(dim);
            }
            if pitch >= 0 && (pitch % 12 == 0 || r == 0) {
                put_str(
                    buf,
                    (area.x, y),
                    GUTTER - 1,
                    &pitch_name(pitch as u8),
                    Style::new().fg(Color::Rgb(150, 150, 158)),
                );
            }
        }

        // bar gridlines
        for &t in &self.view.bar_lines {
            if let Some(x) = self.visible_col(t, plot_x0, plot_w) {
                for r in 0..rows {
                    if let Some(c) = buf.cell_mut((x, area.y + r)) {
                        if c.symbol() == " " {
                            c.set_char('│').set_style(dim);
                        }
                    }
                }
            }
        }

        // section boundary markers
        for s in &self.analysis.sections {
            if s.tick_start <= self.scroll_tick {
                continue;
            }
            if let Some(x) = self.visible_col(s.tick_start, plot_x0, plot_w) {
                let st = Style::new().fg(class_color(s.class));
                for r in 0..rows {
                    if let Some(c) = buf.cell_mut((x, area.y + r)) {
                        if c.symbol() == " " || c.symbol() == "│" {
                            c.set_char('╎').set_style(st);
                        }
                    }
                }
            }
        }

        // notes
        for (li, lane) in self.view.lanes.iter().enumerate() {
            let color = lane_color(li);
            for note in &lane.notes {
                if i32::from(note.pitch) > i32::from(self.top_pitch) {
                    continue;
                }
                let row = i32::from(self.top_pitch) - i32::from(note.pitch);
                if row < 0 || row >= i32::from(rows) || note.end <= self.scroll_tick {
                    continue;
                }
                let y = area.y + row as u16;
                let c0 = note.onset.saturating_sub(self.scroll_tick) / self.ticks_per_col;
                let c1 = note.end.saturating_sub(self.scroll_tick).saturating_sub(1)
                    / self.ticks_per_col;
                let last = u32::from(plot_w).saturating_sub(1);
                for col in c0.min(last)..=c1.min(last) {
                    if let Some(c) = buf.cell_mut((plot_x0 + col as u16, y)) {
                        c.set_char('█').set_fg(color);
                    }
                }
            }
        }

        // playhead
        if let Some(x) = self.visible_col(self.play_tick, plot_x0, plot_w) {
            let st = Style::new()
                .fg(Color::Rgb(255, 207, 77))
                .add_modifier(Modifier::BOLD);
            for r in 0..rows {
                if let Some(c) = buf.cell_mut((x, area.y + r)) {
                    c.set_char('┃').set_style(st);
                }
            }
        }
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

        if let Some(s) = self.analysis.sections.get(self.sel_section) {
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
            if self.playing {
                "▶ playing"
            } else {
                "⏸ paused"
            }
        )));
        lines.push(Line::from(format!("pos {}", self.position_label())));

        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
    }

    // ── helpers ─────────────────────────────────────────────────────────
    /// Maps a tick to a screen column, or `None` if off the right edge.
    fn visible_col(&self, tick: u32, plot_x0: u16, plot_w: u16) -> Option<u16> {
        if tick < self.scroll_tick {
            return None;
        }
        let col = (tick - self.scroll_tick) / self.ticks_per_col;
        (col < u32::from(plot_w)).then(|| plot_x0 + col as u16)
    }

    /// Maps a tick to a clamped column within `[plot_x0, plot_x0+plot_w]`.
    fn tick_to_col(&self, tick: u32, plot_x0: u16, plot_w: u16) -> u16 {
        if tick <= self.scroll_tick {
            return plot_x0;
        }
        let col = ((tick - self.scroll_tick) / self.ticks_per_col).min(u32::from(plot_w));
        plot_x0.saturating_add(col as u16)
    }

    fn position_label(&self) -> String {
        let ppq = u32::from(self.view.ppq).max(1);
        let mut bar_idx = 0usize;
        let mut bar_start = self.view.tick_start;
        for (i, &t) in self.view.bar_lines.iter().enumerate() {
            if t <= self.play_tick {
                bar_idx = i;
                bar_start = t;
            } else {
                break;
            }
        }
        let beat = self.play_tick.saturating_sub(bar_start) / ppq;
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

/// Writes up to `maxw` chars of `s` starting at `pos`, applying `style`.
fn put_str(buf: &mut Buffer, pos: (u16, u16), maxw: u16, s: &str, style: Style) {
    let (x, y) = pos;
    for (i, ch) in s.chars().take(usize::from(maxw)).enumerate() {
        if let Some(c) = buf.cell_mut((x.saturating_add(i as u16), y)) {
            c.set_char(ch).set_style(style);
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
        assert!(app.playing, "space starts playback");
        assert!(!app.on_key(KeyCode::Char('q')), "q requests quit");
    }

    #[test]
    fn section_nav_moves_scroll_to_section() {
        let mut app = demo_app();
        app.on_key(KeyCode::Char(']'));
        assert_eq!(app.sel_section, 1);
        assert_eq!(app.scroll_tick, 960, "selecting a section scrolls to it");
    }

    #[test]
    fn zoom_keys_change_resolution() {
        let mut app = demo_app();
        app.fit(80);
        let base = app.ticks_per_col;
        app.on_key(KeyCode::Char('+'));
        assert!(app.ticks_per_col <= base, "zoom in lowers ticks/col");
    }
}
