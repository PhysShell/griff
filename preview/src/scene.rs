//! Scene: the resolved, renderer-agnostic *placement* of one preview frame
//! (ADR-0016, layer 3).
//!
//! [`resolve`] lifts the preview's layout math — the tick↔column and pitch↔row
//! maps — into one place and emits a grid of *placed* cells: note blocks, bar
//! gridlines, section markers, the playhead, the section band, and gutter
//! labels. Each cell carries a glyph and a **semantic** [`CellRole`] (lane index,
//! section class, emphasis) rather than a concrete colour, so every renderer
//! (`ratatui` today, `egui` later) maps roles to its own styling and re-derives
//! no geometry. This is the *visual* half of "both frontends agree"; the
//! *behavioural* half lives in [`crate::viewport`].
//!
//! The scene covers the piano-roll only: the roll plane (`rows × cols`) plus the
//! section band (one row of `cols`). The header, footer, and inspector are plain
//! text with no layout math and stay renderer-local.

// The resolver is bounded grid arithmetic over terminal cells and tick spans,
// with integer casts for the linear tick↔column map. Values are bounded by the
// grid size and every denominator is guarded non-zero, so the overflow/precision
// lints carry no signal here.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use griff_core::classify::BarClass;

use crate::analysis::Analysis;
use crate::render::pitch_name;
use crate::view::PianoRollView;
use crate::viewport::Viewport;

/// Left gutter width inside the roll: 4 columns of pitch label + 1 separator.
/// The plot plane starts at column `GUTTER`.
pub const GUTTER: u16 = 5;

/// The cell area, in abstract character cells, a [`Scene`] is resolved for: the
/// roll plane. The section band shares the same `cols` on its own single row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSize {
    /// Total width in cells (gutter + plot plane).
    pub cols: u16,
    /// Plane height in cells.
    pub rows: u16,
}

/// The semantic role of a placed cell — *what* it is, not *how* it is coloured.
/// Renderers map each role to concrete styling; the glyph travels with the cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellRole {
    /// Background / unused cell.
    Empty,
    /// The gutter↔plane separator column.
    Separator,
    /// A bar gridline.
    GridLine,
    /// A section boundary marker, carrying the section's class.
    SectionMark(BarClass),
    /// A note block in the lane of the given index.
    Note(u16),
    /// The playhead column.
    Playhead,
    /// A pitch-label glyph in the gutter.
    PitchLabel,
    /// A cell of the section band, carrying its class and whether it is selected.
    BandFill {
        /// The section's classification.
        class: BarClass,
        /// Whether this is the currently selected section.
        selected: bool,
    },
    /// A glyph of the section-band gutter header (`SEC`).
    BandHeader,
}

/// One placed cell: a glyph, its semantic [`CellRole`], and whether the plane
/// shades it as a black-key row. `shade` is always `false` in the band and the
/// gutter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneCell {
    /// The character to draw.
    pub glyph: char,
    /// What the cell represents.
    pub role: CellRole,
    /// Black-key background shading (plot plane only).
    pub shade: bool,
}

impl SceneCell {
    /// A blank, unshaded background cell.
    pub const EMPTY: Self = Self {
        glyph: ' ',
        role: CellRole::Empty,
        shade: false,
    };
}

/// A resolved frame: the section band plus the roll plane, every cell placed.
///
/// The band is one row of `cols` cells; the plane is `rows × cols` cells in
/// row-major order. The pure output of [`resolve`]; a renderer only blits it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scene {
    /// Total width in cells.
    pub cols: u16,
    /// Plane height in cells.
    pub rows: u16,
    /// The section-band row, `cols` cells wide.
    pub band: Vec<SceneCell>,
    /// The roll plane, `rows × cols` cells in row-major order.
    pub plane: Vec<SceneCell>,
}

impl Scene {
    /// The band cell at column `col`, if in range.
    #[must_use]
    pub fn band_cell(&self, col: u16) -> Option<&SceneCell> {
        self.band.get(usize::from(col))
    }

    /// The plane cell at (`row`, `col`), if in range.
    #[must_use]
    pub fn plane_cell(&self, row: u16, col: u16) -> Option<&SceneCell> {
        if col >= self.cols {
            return None;
        }
        let idx = usize::from(row) * usize::from(self.cols) + usize::from(col);
        self.plane.get(idx)
    }
}

/// Resolves the piano-roll plane and section band for `view`/`analysis` under
/// the interaction state `vp` into a `size`-shaped grid of placed cells.
///
/// This is the single home for the preview's layout math; renderers consume the
/// result and re-derive none of it. Total and panic-free: a grid too narrow to
/// plot (`cols <= GUTTER`) yields a blank scene rather than an error.
#[must_use]
pub fn resolve(view: &PianoRollView, analysis: &Analysis, vp: &Viewport, size: GridSize) -> Scene {
    let GridSize { cols, rows } = size;
    let mut band = vec![SceneCell::EMPTY; usize::from(cols)];
    let mut plane = vec![SceneCell::EMPTY; usize::from(cols) * usize::from(rows)];

    // Too narrow to plot anything (gutter only): a blank scene, never a panic.
    if cols <= GUTTER {
        return Scene {
            cols,
            rows,
            band,
            plane,
        };
    }
    let plot_w = cols - GUTTER; // > 0

    resolve_plane(&mut plane, view, analysis, vp, size);
    resolve_band(&mut band, analysis, vp, plot_w);

    Scene {
        cols,
        rows,
        band,
        plane,
    }
}

/// Index of plane cell (`row`, `col`) in the row-major buffer.
fn plane_idx(cols: u16, row: u16, col: u16) -> usize {
    usize::from(row) * usize::from(cols) + usize::from(col)
}

/// tick → scene column, or `None` past the right edge. Plot column 0 maps to
/// scene column `GUTTER`. Used for gridlines, section markers, and the playhead.
fn visible_col(vp: &Viewport, plot_w: u16, tick: u32) -> Option<u16> {
    if tick < vp.scroll_tick {
        return None;
    }
    let col = (tick - vp.scroll_tick) / tpc_of(vp);
    (col < u32::from(plot_w)).then(|| GUTTER + col as u16)
}

/// tick → scene column, pinned to the plot edges `[GUTTER, GUTTER + plot_w]`.
/// Used for the section band, which never clips a span to nothing.
fn clamp_col(vp: &Viewport, plot_w: u16, tick: u32) -> u16 {
    if tick <= vp.scroll_tick {
        return GUTTER;
    }
    let col = ((tick - vp.scroll_tick) / tpc_of(vp)).min(u32::from(plot_w));
    GUTTER + col as u16
}

/// Places everything in the roll plane, layer by layer, in the same order the
/// renderer used to draw it: later layers overdraw earlier ones (notes over
/// gridlines, the playhead over all), so the precedence is preserved exactly.
fn resolve_plane(
    plane: &mut [SceneCell],
    view: &PianoRollView,
    analysis: &Analysis,
    vp: &Viewport,
    size: GridSize,
) {
    let GridSize { cols, rows } = size;
    let plot_w = cols.saturating_sub(GUTTER);

    place_scaffold(plane, vp, cols, rows);

    // Bar gridlines, only on blank cells.
    for &tick in &view.bar_lines {
        if let Some(col) = visible_col(vp, plot_w, tick) {
            paint_column(plane, cols, rows, col, |c| {
                if c.glyph == ' ' {
                    c.glyph = '│';
                    c.role = CellRole::GridLine;
                }
            });
        }
    }

    // Section boundary markers, over blank cells or gridlines.
    for s in &analysis.sections {
        if s.tick_start <= vp.scroll_tick {
            continue;
        }
        if let Some(col) = visible_col(vp, plot_w, s.tick_start) {
            paint_column(plane, cols, rows, col, |c| {
                if c.glyph == ' ' || c.glyph == '│' {
                    c.glyph = '╎';
                    c.role = CellRole::SectionMark(s.class);
                }
            });
        }
    }

    place_notes(plane, view, vp, size);

    // Playhead, over everything.
    if let Some(col) = visible_col(vp, plot_w, vp.play_tick) {
        paint_column(plane, cols, rows, col, |c| {
            c.glyph = '┃';
            c.role = CellRole::Playhead;
        });
    }
}

/// Applies `f` to every cell of plane column `col`.
fn paint_column(
    plane: &mut [SceneCell],
    cols: u16,
    rows: u16,
    col: u16,
    f: impl Fn(&mut SceneCell),
) {
    for r in 0..rows {
        if let Some(c) = plane.get_mut(plane_idx(cols, r, col)) {
            f(c);
        }
    }
}

/// Per-row scaffolding: black-key shading, the separator column, pitch labels.
fn place_scaffold(plane: &mut [SceneCell], vp: &Viewport, cols: u16, rows: u16) {
    for r in 0..rows {
        let pitch = i32::from(vp.top_pitch) - i32::from(r);
        if pitch >= 0 && is_black_key(pitch as u8) {
            for col in GUTTER..cols {
                if let Some(c) = plane.get_mut(plane_idx(cols, r, col)) {
                    c.shade = true;
                }
            }
        }
        if let Some(c) = plane.get_mut(plane_idx(cols, r, GUTTER - 1)) {
            c.glyph = '│';
            c.role = CellRole::Separator;
        }
        if pitch >= 0 && (pitch % 12 == 0 || r == 0) {
            let name = pitch_name(pitch as u8);
            for (i, ch) in name.chars().take(usize::from(GUTTER - 1)).enumerate() {
                if let Some(c) = plane.get_mut(plane_idx(cols, r, i as u16)) {
                    c.glyph = ch;
                    c.role = CellRole::PitchLabel;
                }
            }
        }
    }
}

/// Note blocks, lane by lane (later lanes overdraw earlier on overlap).
fn place_notes(plane: &mut [SceneCell], view: &PianoRollView, vp: &Viewport, size: GridSize) {
    let GridSize { cols, rows } = size;
    let last = u32::from(cols.saturating_sub(GUTTER)).saturating_sub(1);
    let tpc = tpc_of(vp);
    for (li, lane) in view.lanes.iter().enumerate() {
        let lane_role = CellRole::Note(li as u16);
        for note in &lane.notes {
            if i32::from(note.pitch) > i32::from(vp.top_pitch) {
                continue;
            }
            let row = i32::from(vp.top_pitch) - i32::from(note.pitch);
            if row < 0 || row >= i32::from(rows) || note.end <= vp.scroll_tick {
                continue;
            }
            let row = row as u16;
            let c0 = note.onset.saturating_sub(vp.scroll_tick) / tpc;
            let c1 = note.end.saturating_sub(vp.scroll_tick).saturating_sub(1) / tpc;
            let mut col = c0.min(last);
            let stop = c1.min(last);
            while col <= stop {
                if let Some(c) = plane.get_mut(plane_idx(cols, row, GUTTER + col as u16)) {
                    c.glyph = '█';
                    c.role = lane_role;
                }
                col += 1;
            }
        }
    }
}

/// Places the section band: the `SEC` gutter header, a coloured fill per section
/// span, and each section's centred class label.
fn resolve_band(band: &mut [SceneCell], analysis: &Analysis, vp: &Viewport, plot_w: u16) {
    for (i, ch) in "SEC".chars().take(usize::from(GUTTER - 1)).enumerate() {
        if let Some(c) = band.get_mut(i) {
            c.glyph = ch;
            c.role = CellRole::BandHeader;
        }
    }

    let xmax = GUTTER + plot_w; // == cols
    for (i, s) in analysis.sections.iter().enumerate() {
        let a = clamp_col(vp, plot_w, s.tick_start);
        let b = clamp_col(vp, plot_w, s.tick_end);
        if b <= a {
            continue;
        }
        let role = CellRole::BandFill {
            class: s.class,
            selected: i == vp.sel_section,
        };
        for col in a..b.min(xmax) {
            if let Some(c) = band.get_mut(usize::from(col)) {
                c.glyph = ' ';
                c.role = role;
            }
        }
        let label = s.class.to_string();
        let label_len = label.chars().count();
        let w = usize::from(b - a);
        if w >= label_len {
            let off = (w - label_len) / 2;
            for (j, ch) in label.chars().take(usize::from(plot_w)).enumerate() {
                let col = usize::from(a) + off + j;
                if let Some(c) = band.get_mut(col) {
                    c.glyph = ch;
                    c.role = role;
                }
            }
        }
    }
}

/// Ticks per column, floored at 1 (mirrors the viewport invariant).
const fn tpc_of(vp: &Viewport) -> u32 {
    if vp.ticks_per_col == 0 {
        1
    } else {
        vp.ticks_per_col
    }
}

const fn is_black_key(pitch: u8) -> bool {
    matches!(pitch % 12, 1 | 3 | 6 | 8 | 10)
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
    use crate::analysis::{Analysis, Section};
    use crate::view::{Lane, NoteRect, PianoRollView};
    use crate::viewport::Viewport;

    // A two-section, two-note fixture sized so every feature lands in-grid:
    // top_pitch 52, so pitch 40 → row 12 and pitch 47 → row 5 within rows = 14.
    fn fixture() -> (PianoRollView, Analysis, Viewport) {
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
        let vp = Viewport {
            scroll_tick: 0,
            ticks_per_col: 60,
            top_pitch: 52,
            sel_section: 0,
            playing: false,
            play_tick: 0,
            show_inspector: true,
        };
        (view, analysis, vp)
    }

    fn resolved() -> Scene {
        let (view, analysis, vp) = fixture();
        resolve(&view, &analysis, &vp, GridSize { cols: 40, rows: 14 })
    }

    #[test]
    fn grid_has_exact_cell_counts() {
        let s = resolved();
        assert_eq!(s.cols, 40);
        assert_eq!(s.rows, 14);
        assert_eq!(s.band.len(), 40, "band is one row of `cols`");
        assert_eq!(s.plane.len(), 40 * 14, "plane is `rows × cols`");
    }

    #[test]
    fn separator_column_runs_down_every_plane_row() {
        let s = resolved();
        for r in 0..s.rows {
            let cell = s.plane_cell(r, GUTTER - 1).expect("separator in range");
            assert_eq!(cell.role, CellRole::Separator);
            assert_eq!(cell.glyph, '│');
        }
    }

    #[test]
    fn a_note_is_placed_at_its_tick_and_pitch() {
        // Note (onset 0, end 480, pitch 40): row 52-40 = 12, ticks 0..480 over
        // ticks_per_col 60 → plot columns 0..=7, i.e. scene columns 5..=12.
        // Column 5 is overdrawn by the playhead, so probe column 6.
        let s = resolved();
        let cell = s.plane_cell(12, 6).expect("note cell in range");
        assert_eq!(cell.role, CellRole::Note(0));
        assert_eq!(cell.glyph, '█');
    }

    #[test]
    fn playhead_column_overdraws_the_whole_plane() {
        // play_tick 0 → scene column GUTTER (5); drawn last, so it wins.
        let s = resolved();
        for r in 0..s.rows {
            let cell = s.plane_cell(r, GUTTER).expect("playhead in range");
            assert_eq!(cell.role, CellRole::Playhead, "row {r}");
            assert_eq!(cell.glyph, '┃');
        }
    }

    #[test]
    fn bar_gridline_lands_on_a_blank_column() {
        // Final barline at tick 1920 → column GUTTER + 1920/60 = 37, with no note
        // or marker over it.
        let s = resolved();
        let cell = s.plane_cell(0, 37).expect("gridline in range");
        assert_eq!(cell.role, CellRole::GridLine);
        assert_eq!(cell.glyph, '│');
    }

    #[test]
    fn section_marker_sits_at_the_section_onset() {
        // Solo starts at tick 960 (> scroll 0) → column 21; rows without a note
        // there carry the marker.
        let s = resolved();
        let cell = s.plane_cell(0, 21).expect("marker in range");
        assert_eq!(cell.role, CellRole::SectionMark(BarClass::Solo));
        assert_eq!(cell.glyph, '╎');
    }

    #[test]
    fn black_key_rows_are_shaded_white_rows_are_not() {
        // top_pitch 52: row 1 → pitch 51 (D#, black); row 0 → pitch 52 (E, white).
        let s = resolved();
        assert!(
            s.plane_cell(1, 10).expect("plot cell").shade,
            "black key row"
        );
        assert!(
            !s.plane_cell(0, 10).expect("plot cell").shade,
            "white key row"
        );
    }

    #[test]
    fn octave_c_is_labelled_in_the_gutter() {
        // pitch 48 = C3 at row 52-48 = 4; gutter columns hold the label.
        let s = resolved();
        let c = s.plane_cell(4, 0).expect("label cell");
        assert_eq!(c.role, CellRole::PitchLabel);
        assert_eq!(c.glyph, 'C');
        assert_eq!(s.plane_cell(4, 1).expect("label cell").glyph, '3');
    }

    #[test]
    fn band_carries_header_fills_and_labels() {
        let s = resolved();
        // "SEC" gutter header.
        let h = s.band_cell(0).expect("header cell");
        assert_eq!(h.role, CellRole::BandHeader);
        assert_eq!(h.glyph, 'S');
        // Riff fill (selected) starts at column GUTTER (5).
        assert_eq!(
            s.band_cell(5).expect("riff fill").role,
            CellRole::BandFill {
                class: BarClass::Riff,
                selected: true
            }
        );
        // Riff label centred at column 11.
        assert_eq!(s.band_cell(11).expect("riff label").glyph, 'R');
        // Solo fill (not selected) starts at column 21.
        assert_eq!(
            s.band_cell(21).expect("solo fill").role,
            CellRole::BandFill {
                class: BarClass::Solo,
                selected: false
            }
        );
    }

    #[test]
    fn too_narrow_a_grid_is_blank_not_a_panic() {
        let (view, analysis, vp) = fixture();
        let s = resolve(&view, &analysis, &vp, GridSize { cols: 3, rows: 6 });
        assert_eq!(s.band.len(), 3);
        assert_eq!(s.plane.len(), 18);
        assert!(
            s.plane.iter().all(|c| c.role == CellRole::Empty),
            "no placement when there is no plot width"
        );
        assert!(s.band.iter().all(|c| c.role == CellRole::Empty));
    }
}
