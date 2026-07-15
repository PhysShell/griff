#![no_main]

//! Fuzz target: pattern expansion limits and the time-domain lowering
//! (P1, S16 Phases 1–2, ADR-0010).
//!
//! Structure-aware: `arbitrary` builds a kernel, a budget, an optional
//! seeded pruning, and a bar/unit geometry, then drives
//! `Kernel -> fractalize -> linearize -> map_rhythm`.
//!
//! Oracle (normalised invariants):
//!   * No panic / hang / unbounded allocation (libFuzzer limits). The cell
//!     budget is capped at `u16::MAX` — a legitimate multi-gigabyte grid
//!     under a huge explicit budget is not a finding.
//!   * `Kernel::from_rows`: `Ok` xor a typed `PatternError`.
//!   * `fractalize`: on `Ok` the grid never exceeds `max_cells` (the budget
//!     law); on a budget breach the error tells the truth
//!     (`needed > max_cells`).
//!   * `linearize` covers every cell for both traversals — timed rests
//!     included, nothing dropped.
//!   * `map_rhythm`: on `Ok` every note is one `unit` long, sits on a slot
//!     boundary, and stays inside its bar; refusals are typed
//!     `LowerError`s.

use arbitrary::Arbitrary;
use griff_core::event::Ticks;
use griff_pattern::{
    fractalize, linearize, DensityBps, ExpansionBudget, Kernel, PatternError, PruneSpec, Traversal,
};
use griff_swang::{map_rhythm, TailPolicy};
use libfuzzer_sys::fuzz_target;

#[derive(Debug, Arbitrary)]
struct FuzzInput {
    rows: Vec<String>,
    depth: u8,
    /// The structural budget, capped so an accepted expansion stays small.
    max_cells: u16,
    /// Optional pruning: (density in bps after `% 10001`, seed).
    prune: Option<(u16, u64)>,
    snake: bool,
    bar_duration: u32,
    unit: u32,
    rest_pad: bool,
}

fuzz_target!(|input: FuzzInput| {
    let rows: Vec<&str> = input.rows.iter().map(String::as_str).collect();
    // A typed rejection is the contract working, not a finding.
    let Ok(kernel) = Kernel::from_rows(&rows) else {
        return;
    };

    let budget = ExpansionBudget {
        max_depth: input.depth,
        max_cells: u64::from(input.max_cells),
    };
    let prune = input.prune.and_then(|(bps, seed)| {
        DensityBps::new(bps % 10_001)
            .ok()
            .map(|density| PruneSpec { seed, density })
    });

    let expansion = match fractalize(&kernel, input.depth, prune, budget) {
        Ok(expansion) => expansion,
        Err(PatternError::MaxCellsExceeded {
            needed, max_cells, ..
        }) => {
            assert!(needed > max_cells, "a budget breach tells the truth");
            return;
        }
        Err(_) => return, // typed refusal — the contract working
    };

    let expected_cells = expansion
        .width()
        .checked_mul(expansion.height())
        .expect("accepted expansion dimensions fit usize");
    assert!(
        u64::try_from(expected_cells).unwrap_or(u64::MAX) <= u64::from(input.max_cells),
        "the budget law: {expected_cells} cells under a {} budget",
        input.max_cells
    );
    let expected_active = expansion.active_count();
    for traversal in [Traversal::RowMajor, Traversal::Snake] {
        let sequence = linearize(&expansion, traversal);
        assert_eq!(
            sequence.cells().len(),
            expected_cells,
            "a traversal covers every cell of the grid — timed rests included"
        );
        assert_eq!(
            sequence.cells().iter().filter(|&&cell| cell).count(),
            expected_active,
            "a traversal preserves every onset"
        );
    }

    let sequence = linearize(
        &expansion,
        if input.snake {
            Traversal::Snake
        } else {
            Traversal::RowMajor
        },
    );
    let tail = if input.rest_pad {
        TailPolicy::RestPad
    } else {
        TailPolicy::Reject
    };
    match map_rhythm(
        &sequence,
        Ticks(input.bar_duration),
        Ticks(input.unit),
        tail,
    ) {
        Ok(templates) => {
            for template in &templates {
                for note in &template.notes {
                    assert_eq!(note.offset.0 % input.unit, 0, "a note sits on a slot");
                    assert_eq!(note.duration.0, input.unit, "a note is one unit long");
                    let end = note
                        .offset
                        .0
                        .checked_add(note.duration.0)
                        .expect("an accepted note's end does not overflow");
                    assert!(
                        end <= input.bar_duration,
                        "a note ends inside its bar: {end} > {}",
                        input.bar_duration
                    );
                }
            }
        }
        Err(_) => {} // typed LowerError — the contract working
    }
});
