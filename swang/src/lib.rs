//! Swang's lowering into the canonical model (ADR-0029 §2).
//!
//! `griff-pattern` speaks structure; `griff-core` speaks music. This crate is
//! the seam: [`map_rhythm`] cuts an [`ActivitySequence`] into one-bar
//! [`RhythmTemplate`] values under an explicit time unit, an explicit bar
//! geometry, and an explicit tail policy — no defaults, no silent fitting
//! (`docs/swang/spec.md` §1.11). The AST, parser, and formatter arrive in
//! later phases; nothing here is grammar.

use griff_core::event::Ticks;
use griff_core::generate::{RhythmTemplate, TemplateNote};
use griff_pattern::ActivitySequence;

/// What happens to a final bar the sequence does not fill exactly
/// (spec §1.11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TailPolicy {
    /// An incomplete final bar is a typed error (`SWG0302`) — the default
    /// wherever a frontend documents one.
    Reject,
    /// The final bar's missing slots become timed rests: the template simply
    /// carries no notes there, and the bar keeps its length.
    RestPad,
}

/// Everything the pattern-to-rhythm lowering can reject. Codes per
/// `docs/swang/spec.md` §1.5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LowerError {
    /// The unit is zero ticks — no slot can have no duration.
    ZeroUnit,
    /// The unit does not divide the bar exactly (`SWG0301`): a slot must
    /// never cross a bar boundary.
    UnitDoesNotDivideBar {
        /// The bar's duration.
        bar_duration: Ticks,
        /// The offending unit.
        unit: Ticks,
    },
    /// The sequence stops mid-bar under [`TailPolicy::Reject`] (`SWG0302`).
    IncompleteFinalBar {
        /// Slots the sequence put into the final bar.
        have_slots: usize,
        /// Slots a full bar holds.
        slots_per_bar: usize,
    },
}

impl std::fmt::Display for LowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let _ = f;
        unimplemented!("red phase: S16 Phase 2 (ADR-0029)")
    }
}

impl std::error::Error for LowerError {}

/// Cuts `sequence` into one-bar placed-onset templates: `X` at slot `i`
/// becomes a note of one `unit` at offset `(i mod slots_per_bar) × unit` in
/// bar `i div slots_per_bar`; `.` contributes no note, and the bar keeps its
/// length because offsets are absolute (spec §1.11).
///
/// An all-silent bar yields an **empty template at its position** — the
/// lowering is faithful; what the S6 seam's empty-template filtering then
/// does to the rotation is the caller's contract to surface.
///
/// # Errors
/// [`LowerError::ZeroUnit`], [`LowerError::UnitDoesNotDivideBar`]
/// (`SWG0301`), and [`LowerError::IncompleteFinalBar`] (`SWG0302` under
/// [`TailPolicy::Reject`]).
pub fn map_rhythm(
    sequence: &ActivitySequence,
    bar_duration: Ticks,
    unit: Ticks,
    tail: TailPolicy,
) -> Result<Vec<RhythmTemplate>, LowerError> {
    let _ = (sequence, bar_duration, unit, tail);
    unimplemented!("red phase: S16 Phase 2 (ADR-0029)")
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::missing_assert_message,
    clippy::arithmetic_side_effects
)]
mod tests {
    use griff_core::event::Ticks;
    use griff_core::generate::TemplateNote;
    use griff_pattern::{fractalize, linearize, ExpansionBudget, Kernel, Traversal};

    use super::{map_rhythm, LowerError, TailPolicy};

    /// PPQN 480: a 1/16 is 120 ticks, a 4/4 bar is 1920 — 16 slots.
    const UNIT: Ticks = Ticks(120);
    const BAR: Ticks = Ticks(1920);

    /// The spec's worked sequence: kernel `X.X/XX./.XX`, depth 0, row-major
    /// — onsets at slots 0, 2, 3, 4, 7, 8 across 9 slots.
    fn spec_sequence() -> griff_pattern::ActivitySequence {
        let kernel = Kernel::from_rows(&["X.X", "XX.", ".XX"]).expect("spec kernel");
        let expansion = fractalize(
            &kernel,
            0,
            None,
            ExpansionBudget {
                max_depth: 1,
                max_cells: 100,
            },
        )
        .expect("depth 0 expands");
        linearize(&expansion, Traversal::RowMajor)
    }

    fn note(offset: u32, duration: u32) -> TemplateNote {
        TemplateNote {
            offset: Ticks(offset),
            duration: Ticks(duration),
        }
    }

    #[test]
    fn the_spec_sequence_lowers_into_placed_onsets() {
        // 9 slots into a 16-slot bar: one partial bar, rest-padded.
        let templates =
            map_rhythm(&spec_sequence(), BAR, UNIT, TailPolicy::RestPad).expect("lowers");
        assert_eq!(templates.len(), 1);
        assert_eq!(
            templates[0].notes,
            vec![
                note(0, 120),
                note(240, 120),
                note(360, 120),
                note(480, 120),
                note(840, 120),
                note(960, 120),
            ]
        );
    }

    #[test]
    fn adjacent_onsets_stay_two_short_notes() {
        // Slots 2 and 3 are adjacent X: two one-unit notes, never one 1/8.
        let templates =
            map_rhythm(&spec_sequence(), BAR, UNIT, TailPolicy::RestPad).expect("lowers");
        assert_eq!(templates[0].notes[1], note(240, 120));
        assert_eq!(templates[0].notes[2], note(360, 120));
    }

    #[test]
    fn a_rest_is_a_gap_between_offsets_not_an_absence_of_time() {
        // Slot 1 is silent: offsets jump 0 -> 240 and the bar keeps its
        // length — the gap *is* the rest (the existing seam's contract).
        let templates =
            map_rhythm(&spec_sequence(), BAR, UNIT, TailPolicy::RestPad).expect("lowers");
        let offsets: Vec<u32> = templates[0].notes.iter().map(|n| n.offset.0).collect();
        assert!(!offsets.contains(&120), "slot 1 must stay silent");
        assert_eq!(offsets[0], 0);
        assert_eq!(offsets[1], 240);
    }

    #[test]
    fn slots_cut_into_bars_most_significant_first() {
        // 18 slots at 8 slots per bar (2/4 at 1/16): bars of 8, 8, then a
        // 2-slot tail. Slot 16 lands at bar 2, offset 0; slot 17 at 120.
        let kernel = Kernel::from_rows(&["XX"]).expect("kernel");
        let expansion = fractalize(
            &kernel,
            3,
            None,
            ExpansionBudget {
                max_depth: 3,
                max_cells: 100,
            },
        )
        .expect("expands to 16 slots");
        let sixteen = linearize(&expansion, Traversal::RowMajor);
        assert_eq!(sixteen.len(), 16, "sanity: depth 3 of 1x2 is 16 cells");

        let half_bar = Ticks(960); // 2/4: 8 sixteenth slots
        let templates =
            map_rhythm(&sixteen, half_bar, UNIT, TailPolicy::Reject).expect("16 = 2 full bars");
        assert_eq!(templates.len(), 2);
        assert_eq!(templates[0].notes.len(), 8);
        assert_eq!(templates[1].notes.len(), 8);
        assert_eq!(templates[1].notes[0], note(0, 120));
    }

    #[test]
    fn reject_refuses_an_incomplete_final_bar() {
        let err = map_rhythm(&spec_sequence(), BAR, UNIT, TailPolicy::Reject)
            .expect_err("9 slots into 16-slot bars");
        assert_eq!(
            err,
            LowerError::IncompleteFinalBar {
                have_slots: 9,
                slots_per_bar: 16
            }
        );
    }

    #[test]
    fn rest_pad_pads_only_the_tail_of_the_final_bar() {
        // Same sequence under 4-slot bars (1/4 bar at 1/16): 9 slots =
        // 2 full bars + 1 slot. Three templates; the last holds only slot 8.
        let quarter_bar = Ticks(480);
        let templates =
            map_rhythm(&spec_sequence(), quarter_bar, UNIT, TailPolicy::RestPad).expect("padded");
        assert_eq!(templates.len(), 3);
        assert_eq!(templates[0].notes.len(), 3); // slots 0, 2, 3
        assert_eq!(templates[1].notes.len(), 2); // slots 4, 7
        assert_eq!(templates[2].notes, vec![note(0, 120)]); // slot 8
    }

    #[test]
    fn an_all_silent_bar_yields_an_empty_template_at_its_position() {
        // 12 slots at 4 per bar, with bar 1 (slots 4..8) fully silent.
        let kernel = Kernel::from_rows(&["XXXX....XXXX"]).expect("kernel");
        let expansion = fractalize(
            &kernel,
            0,
            None,
            ExpansionBudget {
                max_depth: 0,
                max_cells: 16,
            },
        )
        .expect("expands");
        let seq = linearize(&expansion, Traversal::RowMajor);
        let quarter_bar = Ticks(480);
        let templates =
            map_rhythm(&seq, quarter_bar, UNIT, TailPolicy::Reject).expect("3 full bars");
        assert_eq!(templates.len(), 3, "the silent bar is not dropped");
        assert_eq!(templates[0].notes.len(), 4);
        assert!(templates[1].notes.is_empty(), "bar 1 is silent, not absent");
        assert_eq!(templates[2].notes.len(), 4);
    }

    #[test]
    fn a_unit_that_does_not_divide_the_bar_is_rejected() {
        // 7/8 bar at PPQN 480 is 1680 ticks; a unit of 900 does not divide.
        let err = map_rhythm(
            &spec_sequence(),
            Ticks(1680),
            Ticks(900),
            TailPolicy::RestPad,
        )
        .expect_err("900 does not divide 1680");
        assert_eq!(
            err,
            LowerError::UnitDoesNotDivideBar {
                bar_duration: Ticks(1680),
                unit: Ticks(900)
            }
        );
    }

    #[test]
    fn a_zero_unit_is_rejected() {
        let err = map_rhythm(&spec_sequence(), BAR, Ticks(0), TailPolicy::RestPad)
            .expect_err("zero unit");
        assert_eq!(err, LowerError::ZeroUnit);
    }

    #[test]
    fn silence_is_duration_not_emptiness() {
        // A sequence of only rests is NOT empty — it is silence with
        // duration. One inactive cell fills a one-slot bar with an empty
        // template rather than being rejected or dropped.
        let kernel = Kernel::from_rows(&["."]).expect("kernel");
        let expansion = fractalize(
            &kernel,
            0,
            None,
            ExpansionBudget {
                max_depth: 0,
                max_cells: 4,
            },
        )
        .expect("expands");
        let silent = linearize(&expansion, Traversal::RowMajor);
        let templates = map_rhythm(&silent, Ticks(120), UNIT, TailPolicy::Reject)
            .expect("one silent slot fills a one-slot bar");
        assert_eq!(templates.len(), 1);
        assert!(templates[0].notes.is_empty());
    }

    #[test]
    fn the_seven_eight_bar_works_when_the_unit_divides() {
        // 7/8 at PPQN 480 = 1680 ticks = 14 sixteenth slots: odd meters are
        // first-class as long as the unit divides the bar exactly.
        let kernel = Kernel::from_rows(&["X.X.X.X.X.X.X."]).expect("kernel");
        let expansion = fractalize(
            &kernel,
            0,
            None,
            ExpansionBudget {
                max_depth: 0,
                max_cells: 16,
            },
        )
        .expect("expands");
        let seq = linearize(&expansion, Traversal::RowMajor);
        let templates =
            map_rhythm(&seq, Ticks(1680), UNIT, TailPolicy::Reject).expect("one full 7/8 bar");
        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].notes.len(), 7);
        assert_eq!(templates[0].notes[6], note(1440, 120));
    }
}
