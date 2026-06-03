//! Rasterises a [`PianoRollView`] into a fixed-size grid of text rows.
//!
//! Pure and deterministic: same view + same dimensions ⇒ same lines. No
//! terminal, no I/O — the binary (or a `ratatui` front-end later) decides how to
//! present the returned rows.

// The rasteriser does a lot of small, bounded grid arithmetic (terminal
// dimensions and tick spans) plus integer/float casts for the linear pitch and
// tick maps. All values are bounded by the viewport and every denominator is
// guarded non-zero, so the overflow/precision lints carry no signal here.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use crate::view::PianoRollView;

/// Width of the left gutter: 4 columns of pitch label plus a separator column.
const GUTTER: usize = 5;

/// Glyphs used to distinguish lanes, cycled by lane index.
const LANE_GLYPHS: [char; 6] = ['█', '▓', '▒', '▚', '◆', '●'];

/// Vertical bar gridline, drawn under (and overwritten by) notes.
const BARLINE: char = '┆';

/// Gutter / plane separator column.
const SEPARATOR: char = '│';

/// Note names for a 12-tone octave, index = pitch % 12.
const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// Human-readable note name for a MIDI pitch, e.g. `60 -> "C4"` (middle C).
pub fn pitch_name(pitch: u8) -> String {
    let name = NOTE_NAMES
        .get(usize::from(pitch % 12))
        .copied()
        .unwrap_or("?");
    let octave = i32::from(pitch / 12) - 1;
    format!("{name}{octave}")
}

/// The glyph used for a given lane index.
pub fn lane_glyph(lane_index: usize) -> char {
    LANE_GLYPHS
        .get(lane_index % LANE_GLYPHS.len())
        .copied()
        .unwrap_or('█')
}

/// Rasterises `view` into exactly `height` rows of `width` characters each.
///
/// Pitch runs top (high) to bottom (low); ticks run left to right, auto-fit to
/// the available plot width.
pub fn render_frame(view: &PianoRollView, width: usize, height: usize) -> Vec<String> {
    if height == 0 {
        return Vec::new();
    }
    // Too narrow to plot anything useful: emit blank rows of the right size.
    if width <= GUTTER {
        return vec![" ".repeat(width); height];
    }
    let plot_w = width - GUTTER;
    let span = u64::from(view.tick_end.saturating_sub(view.tick_start)).max(1);

    let low = u32::from(view.low_pitch);
    let high = u32::from(view.high_pitch.max(view.low_pitch));
    let pitch_range = high - low; // >= 0

    // Pitch → row. If every semitone fits, lay one per row and centre the band;
    // otherwise scale the band linearly across the full height.
    let one_per_row = (pitch_range as usize).saturating_add(1) <= height;
    let v_offset = if one_per_row {
        height.saturating_sub(pitch_range as usize + 1) / 2
    } else {
        0
    };
    let row_for_pitch = |pitch: u32| -> usize {
        let from_top = high.saturating_sub(pitch.clamp(low, high));
        let row = if pitch_range == 0 {
            height / 2
        } else if one_per_row {
            v_offset + from_top as usize
        } else {
            (u64::from(from_top) * (height as u64 - 1) / u64::from(pitch_range)) as usize
        };
        row.min(height - 1)
    };

    // Tick → plot column offset in `0..=plot_w`.
    let plot_x = |tick: u32| -> usize {
        let rel = u64::from(tick.saturating_sub(view.tick_start));
        let x = rel.saturating_mul(plot_w as u64) / span;
        (x as usize).min(plot_w)
    };

    // Build the grid, pre-filled with spaces.
    let mut grid: Vec<Vec<char>> = vec![vec![' '; width]; height];

    // Separator column between gutter and plane.
    for row in &mut grid {
        if let Some(cell) = row.get_mut(GUTTER - 1) {
            *cell = SEPARATOR;
        }
    }

    // Bar gridlines across the plane.
    for &tick in &view.bar_lines {
        let col = GUTTER + plot_x(tick).min(plot_w.saturating_sub(1));
        for row in &mut grid {
            if let Some(cell) = row.get_mut(col) {
                if *cell == ' ' {
                    *cell = BARLINE;
                }
            }
        }
    }

    // Notes, lane by lane (later lanes draw over earlier ones on overlap).
    for (lane_index, lane) in view.lanes.iter().enumerate() {
        let glyph = lane_glyph(lane_index);
        for note in &lane.notes {
            let row = row_for_pitch(u32::from(note.pitch));
            let x0 = plot_x(note.onset).min(plot_w.saturating_sub(1));
            let x1 = plot_x(note.end).clamp(x0 + 1, plot_w);
            if let Some(grid_row) = grid.get_mut(row) {
                for x in x0..x1 {
                    if let Some(cell) = grid_row.get_mut(GUTTER + x) {
                        *cell = glyph;
                    }
                }
            }
        }
    }

    // Pitch labels in the gutter: every C, plus the top and bottom rows.
    write_label(&mut grid, row_for_pitch(high), &pitch_name(view.high_pitch));
    write_label(&mut grid, row_for_pitch(low), &pitch_name(view.low_pitch));
    for pitch in low..=high {
        if pitch % 12 == 0 {
            let row = row_for_pitch(pitch);
            write_label(&mut grid, row, &pitch_name(pitch as u8));
        }
    }

    grid.into_iter()
        .map(|row| row.into_iter().collect())
        .collect()
}

/// Writes a left-aligned label into the gutter of `row`, leaving the separator
/// column intact. Truncated to the gutter width.
fn write_label(grid: &mut [Vec<char>], row: usize, label: &str) {
    let Some(grid_row) = grid.get_mut(row) else {
        return;
    };
    for (i, ch) in label.chars().take(GUTTER - 1).enumerate() {
        if let Some(cell) = grid_row.get_mut(i) {
            *cell = ch;
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
        clippy::str_to_string
    )]

    use super::*;
    use crate::view::{Lane, NoteRect, PianoRollView};

    fn view_with(notes: Vec<NoteRect>, low: u8, high: u8) -> PianoRollView {
        PianoRollView {
            ppq: 480,
            tick_start: 0,
            tick_end: 1920,
            low_pitch: low,
            high_pitch: high,
            bar_lines: vec![0, 1920],
            lanes: vec![Lane {
                name: "lead".to_string(),
                notes,
            }],
            tempo_bpm: 120.0,
            bar_count: 1,
        }
    }

    #[test]
    fn frame_has_exact_dimensions() {
        let view = view_with(
            vec![NoteRect {
                onset: 0,
                end: 480,
                pitch: 60,
            }],
            48,
            72,
        );
        let frame = render_frame(&view, 40, 12);
        assert_eq!(frame.len(), 12, "row count must equal height");
        for line in &frame {
            assert_eq!(line.chars().count(), 40, "each row must be `width` chars");
        }
    }

    #[test]
    fn a_note_is_drawn_in_the_plane() {
        let view = view_with(
            vec![NoteRect {
                onset: 0,
                end: 1920,
                pitch: 60,
            }],
            60,
            60,
        );
        let frame = render_frame(&view, 40, 5);
        let glyph = lane_glyph(0);
        assert!(
            frame.iter().any(|l| l.contains(glyph)),
            "the lane glyph should appear somewhere in the frame",
        );
    }

    #[test]
    fn gutter_shows_a_c_label() {
        // Band spanning C4 (60) guarantees a C label in the gutter.
        let view = view_with(
            vec![NoteRect {
                onset: 0,
                end: 480,
                pitch: 60,
            }],
            55,
            67,
        );
        let frame = render_frame(&view, 40, 24);
        assert!(
            frame.iter().any(|l| l.contains("C4")),
            "a C pitch in range must be labelled in the gutter",
        );
    }

    #[test]
    fn narrow_terminal_does_not_panic() {
        let view = view_with(
            vec![NoteRect {
                onset: 0,
                end: 480,
                pitch: 60,
            }],
            48,
            72,
        );
        let frame = render_frame(&view, 3, 4);
        assert_eq!(frame.len(), 4);
        for line in &frame {
            assert_eq!(line.chars().count(), 3);
        }
    }

    #[test]
    fn zero_height_yields_no_rows() {
        let view = view_with(vec![], 48, 72);
        assert!(render_frame(&view, 40, 0).is_empty());
    }

    #[test]
    fn pitch_name_uses_middle_c_octave_4() {
        assert_eq!(pitch_name(60), "C4");
        assert_eq!(pitch_name(69), "A4");
        assert_eq!(pitch_name(0), "C-1");
    }
}
